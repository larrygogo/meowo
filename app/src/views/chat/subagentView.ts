/// 发送前认出「终端停在子代理视图」并切回主会话。
///
/// claude 可以在提示框下方的子代理面板里打开某个子代理，此后 composer 改发给它（占位
/// `❯ Message @general-purpose…`）。对话页代表主会话，正文却被子代理收走，主会话一个字
/// 也收不到，对话页一直挂「未确认送达」（9aef7061 只做到事后认出去向）。这里在写正文前
/// 先按插件声明的面板键切回主会话，切不回才交给调用方提示用户。
///
/// 屏幕形态与键位见后端 chat_ui::SubagentView（2.1.322 真机取证）。

import type { ChatUi } from "../../api";

export type SubagentViewSpec = NonNullable<ChatUi["subagent_view"]>;
export type ComposerSpec = NonNullable<ChatUi["draft_stash"]>;

export type SubagentScreen = {
  /// 是否停在子代理视图。
  viewing: boolean;
  /// 子代理名：优先取 composer 上边框右侧的描述，没有再取占位里 `@` 后的类型。
  name: string | null;
  /// 主会话行在面板行里的序号，-1 = 面板没画出来。
  main: number;
  /// 被选中（行首是指针）的面板行序号，-1 = 面板没有焦点。
  selected: number;
};

const NOT_VIEWING: SubagentScreen = { viewing: false, name: null, main: -1, selected: -1 };

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/// 读屏幕。composer 的认法与 DraftStash 相同（提示符行、上一行是边框），只是视图里的上边框
/// 右侧挂着子代理描述（`──── probe-sleeper ─`），不再整行是横线。
///
/// 判视图要两条证据之一：空 composer 的占位以 `Message @` 开头；或边框挂了描述且面板里
/// main 行不是「正在看」的那个记号。composer 里有字时占位不画，只能靠后者。
export function readSubagentScreen(screen: string[] | null, spec: SubagentViewSpec, composer: ComposerSpec): SubagentScreen {
  if (!screen) return NOT_VIEWING;
  const border = escapeRegex(composer.composer_border);
  const plainBorder = new RegExp(`^${border}{3,}$`);
  const labeledBorder = new RegExp(`^${border}{3,}\\s+(.+?)\\s+${border}{1,3}$`);
  const isBorder = (line: string) => plainBorder.test(line.trim()) || labeledBorder.test(line.trim());
  let promptAt = -1;
  for (let index = screen.length - 1; index > 0; index -= 1) {
    if (isBorder(screen[index - 1]) && screen[index].trimStart().startsWith(composer.composer_prompt)) { promptAt = index; break; }
  }
  if (promptAt < 0) return NOT_VIEWING;
  const label = labeledBorder.exec(screen[promptAt - 1].trim())?.[1]?.trim() || null;
  const draft = screen[promptAt].trimStart().slice(composer.composer_prompt.length).trim();
  const placeholder = draft.startsWith(spec.placeholder_prefix)
    ? draft.slice(spec.placeholder_prefix.length).replace(/(…|\.\.\.)\s*$/, "").trim() || null
    : undefined;

  let bottom = promptAt + 1;
  while (bottom < screen.length && !plainBorder.test(screen[bottom].trim())) bottom += 1;
  const rows = screen.slice(bottom + 1).map((row) => row.trimEnd());
  const pointer = escapeRegex(spec.panel_pointer);
  // 记号在不同字体下可能是 ◯/●/⚪ 或 ASCII 回退 `( )`，只要求是一个短记号。
  const mainRow = new RegExp(`^\\s*(?:${pointer}\\s+)?(\\(\\s?\\)|\\S{1,2})\\s+${escapeRegex(spec.panel_main_label)}(?:\\s{2,}.*)?$`);
  const selectedRow = new RegExp(`^\\s*${pointer}\\s`);
  const main = rows.findIndex((row) => mainRow.test(row));
  const selected = rows.findIndex((row) => selectedRow.test(row));
  const mainMarker = main >= 0 ? mainRow.exec(rows[main])?.[1] : undefined;
  // 占位证据的限制：屏幕只给纯文本，占位的暗色认不出，用户自己打的「Message @…」草稿和占位一样。
  // 面板画着且 main 行是「正在看」的记号时，这条证据作废（做空第二轮 D）；面板没画出来时仍会误判，
  // 后果是这次发送先给横幅、再点一次照发，不会把话错发给别人。
  const mainViewed = mainMarker === spec.panel_viewed_marker;
  const viewing = (placeholder !== undefined && !mainViewed) || (label !== null && mainMarker !== undefined && !mainViewed);
  return { viewing, name: viewing ? label ?? placeholder ?? null : null, main, selected };
}

export type SubagentViewIo = {
  screen: () => Promise<string[] | null>;
  write: (data: string) => Promise<void>;
  sleep?: (ms: number) => Promise<void>;
};

export type LeaveResult = { ok: true } | { ok: false; name: string | null };

/// 每次按键后等屏幕变化：60ms 一看，最多 10 次。面板重绘是本机 TUI 的一帧（探针里 1.2s
/// 的间隔内全部画完，实际远快于此）。
const STEP_POLLS = 10;
const STEP_POLL_MS = 60;
/// 回车切回之后等视图消失的上限：切视图要重画整段历史，长会话慢一些。
const LEAVE_POLLS = 25;
/// 面板里最多挪几步：main 是第一行，探针里 composer 焦点 ↓ 一步就落在 main，面板焦点停在
/// 被看的子代理行时 ↑ 一步到；多给几步兜住前面还有别的行的情况。
const MAX_MOVES = 5;

/// 停在子代理视图就切回主会话。不在视图里直接 ok；切回并确认视图消失才 ok。
/// 面板没画出来、挪不到 main 行、回车后仍在视图里，都返回 {ok:false}。
/// 无论成败，最后面板若还有选中项就取消它（探针：切回后面板仍保持焦点），让后续写入落进
/// composer。只在**看见**选中项时才发取消键：面板没焦点时那个键是中断。
export async function leaveSubagentView(io: SubagentViewIo, spec: SubagentViewSpec, composer: ComposerSpec): Promise<LeaveResult> {
  const sleep = io.sleep ?? ((ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms)));
  const read = async () => readSubagentScreen(await io.screen(), spec, composer);
  let state = await read();
  if (!state.viewing) return { ok: true };
  const name = state.name;
  const settle = async (done: (next: SubagentScreen) => boolean, polls: number) => {
    for (let attempt = 0; attempt < polls; attempt += 1) {
      await sleep(STEP_POLL_MS);
      state = await read();
      if (done(state)) return true;
    }
    return false;
  };
  let left = false;
  for (let move = 0; move <= MAX_MOVES && !left; move += 1) {
    if (state.main < 0) break;
    if (state.selected === state.main) {
      await io.write(spec.open_input);
      left = await settle((next) => !next.viewing, LEAVE_POLLS);
      break;
    }
    if (move === MAX_MOVES) break;
    // 面板没焦点（-1）或选中项在 main 之上：下移；在它之下：上移。
    const before = state.selected;
    await io.write(state.selected < state.main ? spec.down_input : spec.up_input);
    await settle((next) => next.selected !== before || !next.viewing, STEP_POLLS);
    if (!state.viewing) left = true;
  }
  if (state.selected >= 0) {
    await io.write(spec.leave_panel_input).catch(() => {});
    await settle((next) => next.selected < 0, STEP_POLLS);
  }
  return left ? { ok: true } : { ok: false, name: state.name ?? name };
}
