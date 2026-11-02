/// 撤回的终端协议：发撤回命令、菜单式撤回里逐步核对屏幕、等撤回生效、清掉 CLI 放回输入框的原文。
///
/// 为什么单独成模块（对照 Kimi 把撤回放在服务层、视图只管呈现）：这些步骤只和终端打交道，
/// 不碰 React 状态；对话窗只负责确认弹窗、把原文放回输入框、按结果报错。拆出来之后协议的
/// 每一种结局都是一个明确的返回值，ChatWindow 里不再夹着百来行的轮询循环。

import { managedTerminalScreen, managedTerminalSnapshot, writeManagedTerminal, type ChatUi } from "../../api";
import { appendTerminalText, visibleTerminalText } from "../../terminalAttention";
import { composerDraftText, stashComposerDraft, submitToTerminal } from "./terminalSubmit";

export type UndoControl = NonNullable<ChatUi["undo"]>;

export type UndoInputClearSpec = NonNullable<NonNullable<ChatUi["undo"]>["input_clear"]>;
type UndoMenuSpec = NonNullable<NonNullable<ChatUi["undo"]>["menu"]>;

/// 原文非空行（trim 后）。屏幕核对只拿行首一小段比：从行首起排，不受折行影响。
function undoTextLines(text: string): string[] {
  return text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
}

/// 输入框里含原文的行数（规格见后端 UndoInputClear / UndoComposer），认不出输入框返回 null。
/// - block（opencode）：底边行之上连续以 row_prefix 开头的行，数含原文末行开头的行。计数而非
///   判有无：历史区里同样带前缀的行可能和输入框连成一片，撤回前后比数才分得清。
/// - prompt（claude）：以 prompt 开头、上一行整行是 border 的那行（同 DraftStash 的认法），
///   首行以原文首行开头记 1。历史区的用户消息也以 prompt 开头，但上一行不是横线。
export function undoneInputRows(screen: string[] | null, clear: UndoInputClearSpec, text: string): number | null {
  const lines = undoTextLines(text);
  if (!screen || lines.length === 0) return null;
  const composer = clear.composer;
  if (composer.kind === "prompt") {
    const draft = composerDraftText(screen, { input: "", composer_prompt: composer.prompt, composer_border: composer.border, stashed_marker: "" });
    if (draft === null) return null;
    return draft !== "" && draft.startsWith(lines[0].slice(0, 12)) ? 1 : 0;
  }
  const snippet = lines[lines.length - 1].slice(0, 12);
  let bottom = screen.length - 1;
  while (bottom >= 0 && !screen[bottom].trimStart().startsWith(composer.bottom_prefix)) bottom -= 1;
  if (bottom < 0) return null;
  let rows = 0;
  let count = 0;
  for (let index = bottom - 1; index >= 0 && screen[index].trimStart().startsWith(composer.row_prefix); index -= 1) {
    rows += 1;
    if (screen[index].includes(snippet)) count += 1;
  }
  return rows > 0 ? count : null;
}

/// 菜单式撤回（claude /rewind，规格见后端 UndoMenu）：打开菜单 → 上移 turns 格 → 核对选中行
/// → 回车 → 核对确认页引文 → 按「option_label」那一项的数字 → 等原文回到 CLI 输入框。
/// 每一步先看屏幕再按键：要撤的是哪条消息只有屏幕上那份列表说了算，计数与 CLI 对不上时宁可
/// 退出菜单报错，也不能撤错。菜单已由调用方用撤回命令打开。
async function runUndoMenu(sessionId: number, menu: UndoMenuSpec, clear: UndoInputClearSpec | null, turns: number, text: string): Promise<"done" | "mismatch" | "timeout"> {
  const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));
  const first = undoTextLines(text)[0] ?? "";
  // 标题行之下才是菜单区：历史区里的用户消息同样以 ❯ 开头。
  const menuRows = (screen: string[] | null) => {
    if (!screen) return null;
    const title = screen.map((row) => row.trim()).lastIndexOf(menu.title);
    return title < 0 ? null : screen.slice(title + 1);
  };
  const poll = async <T,>(read: (rows: string[] | null, screen: string[] | null) => T | null, times = 20): Promise<T | null> => {
    for (let attempt = 0; attempt < times; attempt += 1) {
      const screen = await managedTerminalScreen(sessionId);
      const value = read(menuRows(screen), screen);
      if (value !== null) return value;
      await sleep(150);
    }
    return null;
  };
  // 列表项被截断时以 … 结尾：截断前的部分与原文首行开头一致才算同一条。
  const sameMessage = (shown: string) => {
    const head = shown.trim().replace(/…$/, "").trim();
    return head.length > 0 && first.length > 0 && (first.startsWith(head) || head.startsWith(first));
  };
  const cancel = async () => {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      if (menuRows(await managedTerminalScreen(sessionId)) === null) return;
      await writeManagedTerminal(sessionId, menu.cancel_input);
      await sleep(300);
    }
  };
  try {
    if (await poll((rows) => rows?.some((row) => row.trim() === menu.ready_marker) ? true : null) === null) {
      await cancel();
      return "timeout";
    }
    for (let step = 0; step < turns; step += 1) {
      await writeManagedTerminal(sessionId, menu.step_input);
      await sleep(120);
    }
    const prefix = `${menu.selected_prefix} `;
    const selected = await poll((rows) => {
      const row = rows?.find((line) => line.trimStart().startsWith(prefix));
      return row && row.trim() !== menu.ready_marker ? row.trimStart().slice(prefix.length) : null;
    }, 10);
    if (selected === null || !sameMessage(selected)) { await cancel(); return "mismatch"; }
    await writeManagedTerminal(sessionId, menu.enter_input);
    const quoted = await poll((rows) => {
      const row = rows?.find((line) => line.trimStart().startsWith(menu.quote_prefix));
      return row ? row.trimStart().slice(menu.quote_prefix.length) : null;
    }, 10);
    if (quoted === null || !sameMessage(quoted.slice(0, Math.min(quoted.length, 12)))) { await cancel(); return "mismatch"; }
    // 选项随有无代码改动变化（有改动时它是第 2 项），按文案找数字。
    const option = new RegExp(`^[❯↑↓\\s]*(\\d+)\\.\\s+${menu.option_label.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*$`);
    const digit = await poll((rows) => rows?.map((row) => option.exec(row)?.[1]).find(Boolean) ?? null, 10);
    if (digit === null) { await cancel(); return "mismatch"; }
    await writeManagedTerminal(sessionId, digit);
    // 生效 = 菜单收起、原文回到 CLI 输入框（菜单式撤回不写 transcript，只能看屏幕）。
    const done = await poll((rows, screen) => rows === null && (!clear || (undoneInputRows(screen, clear, text) ?? 0) > 0) ? true : null);
    if (done === null) { await cancel(); return "timeout"; }
    return "done";
  } catch {
    await cancel().catch(() => undefined);
    return "timeout";
  }
}

/// 撤回后清掉 CLI 放回它自己输入框的原文（opencode / claude）。原文已在对话页输入框里，终端那份
/// 不清会拼进下一条消息（真机复现过）。清空键在空输入框上另有含义（退出 / 打开回退菜单），所以
/// 只在「比撤回前多出了原文行」时才按，按后数回基线才算干净。baseline = 发撤回命令前的
/// undoneInputRows。
/// 返回是否已确认干净。
async function clearUndoneInput(sessionId: number, clear: UndoInputClearSpec, text: string, baseline: number | null): Promise<boolean> {
  if (baseline === null) return false;
  const rows = async () => undoneInputRows(await managedTerminalScreen(sessionId), clear, text);
  try {
    let now: number | null = null;
    for (let attempt = 0; attempt < 10; attempt += 1) {
      now = await rows();
      if (now !== null && now > baseline) break;
      await new Promise((resolve) => window.setTimeout(resolve, 100));
    }
    // 认得出输入框、没多出原文 = 本来就干净；认不出输入框则不敢按。
    if (now === null) return false;
    if (now <= baseline) return true;
    // 逐个分开写：两个 Esc 合成一次写入会被当成 Alt+Esc（claude 的清空是双 Esc）。
    for (const [index, input] of clear.inputs.entries()) {
      if (index > 0) await new Promise((resolve) => window.setTimeout(resolve, 300));
      await writeManagedTerminal(sessionId, input);
    }
    for (let attempt = 0; attempt < 10; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 100));
      const after = await rows();
      if (after !== null && after <= baseline) return true;
    }
  } catch {
    // 落到调用方的提示。
  }
  return false;
}

/// 撤回的结局。done.clean = CLI 放回它自己输入框的原文已确认清掉（没清掉要提醒用户手动清）。
export type UndoOutcome =
  | { kind: "done"; clean: boolean }
  | { kind: "composer_busy" }
  | { kind: "mismatch" }
  | { kind: "unconfirmed" }
  | { kind: "rejected"; line: string }
  | { kind: "stale" };

export type UndoRun = {
  sessionId: number;
  control: UndoControl;
  /// 这条起往后要撤的轮数（插话不算一轮，由调用方数好）。
  turns: number;
  /// 被撤那条用户消息的原文。
  text: string;
  draftStash: ChatUi["draft_stash"] | null | undefined;
  attentionAbort: () => string | null;
  /// 用户还停在发起撤回的那个会话。切走了就不再推进，返回 stale。
  isCurrent: () => boolean;
  /// 拉一次 transcript，返回被撤的那条是否还在（非菜单式撤回以 transcript 为准）。
  stillHas: () => Promise<boolean>;
  /// 撤回刚生效时调用（在清终端输入框之前）：调用方此刻收掉本地消息、把原文放回输入框。
  onUndone: () => void;
  /// 等待撤回生效期间为 true：refresh 据此采信「撤掉唯一一轮」的空重灌。
  onPending: (pending: boolean) => void;
};

/// 跑一次撤回。菜单式（claude /rewind）以屏幕为准；其余发命令后等 transcript 里那条消失，
/// 期间认插件声明的拒绝行（kimi 在不可撤时的提示）。写终端的异常照常抛给调用方。
export async function performUndo(run: UndoRun): Promise<UndoOutcome> {
  const { sessionId, control, turns, text } = run;
  const menu = control.menu;
  if (menu) {
    // 不走草稿暂存：claude 的暂存在下一次提交（就是这条 /rewind）后自动还原，会和撤回放回
    // 的原文挤在一起。输入框不空就请用户先清。
    const draft = run.draftStash ? composerDraftText(await managedTerminalScreen(sessionId).catch(() => null), run.draftStash) : "";
    if (draft) return { kind: "composer_busy" };
    await submitToTerminal(sessionId, control.command, run.attentionAbort);
    const outcome = await runUndoMenu(sessionId, menu, control.input_clear, turns, text);
    if (!run.isCurrent()) return { kind: "stale" };
    if (outcome !== "done") return { kind: outcome === "mismatch" ? "mismatch" : "unconfirmed" };
    run.onUndone();
    const clean = !control.input_clear || await clearUndoneInput(sessionId, control.input_clear, text, 0);
    return { kind: "done", clean };
  }
  const before = await managedTerminalSnapshot(sessionId);
  if (run.draftStash) await stashComposerDraft(sessionId, run.draftStash);
  const inputBaseline = control.input_clear
    ? undoneInputRows(await managedTerminalScreen(sessionId).catch(() => null), control.input_clear, text)
    : null;
  await submitToTerminal(sessionId, control.count_arg ? `${control.command} ${turns}` : control.command, run.attentionAbort);
  const reject = control.reject_pattern ? new RegExp(control.reject_pattern, "i") : null;
  const decoder = new TextDecoder();
  let since = before.endOffset;
  let tail = "";
  run.onPending(true);
  try {
    for (let attempt = 0; attempt < 20; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 250));
      if (!run.isCurrent()) return { kind: "stale" };
      if (!await run.stillHas()) {
        run.onUndone();
        const clean = !control.input_clear || await clearUndoneInput(sessionId, control.input_clear, text, inputBaseline);
        return { kind: "done", clean };
      }
      if (!reject) continue;
      const snapshot = await managedTerminalSnapshot(sessionId, since);
      if (!snapshot.active || snapshot.endOffset < since) continue;
      since = snapshot.endOffset;
      tail = appendTerminalText(tail, snapshot.data, decoder);
      const line = visibleTerminalText(tail).split(/\r?\n/).find((row) => reject.test(row));
      if (line) return { kind: "rejected", line: line.trim() };
    }
  } finally {
    run.onPending(false);
  }
  return { kind: "unconfirmed" };
}
