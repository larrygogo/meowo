/// 终端输入框：认框、读草稿、收草稿、清草稿、退出特殊输入模式。
///
/// 这是前端**唯一**认 CLI 输入框的地方，规则全来自插件声明的那一份 ComposerSpec（后端
/// chat_ui::ComposerSpec；宿主的同款实现在 meowo-agent 的 composer.rs，两边对同一批实拍帧
/// 必须给出同样的结论，样例在 test/fixtures/composer-frames.json）。
///
/// 为什么只能有一份：此前认框的规格有三份，发送前收草稿、撤回后清原文、发送前判空各接一份，
/// 某家缺哪一份，对应功能就静默失效或退成「请到终端处理」（走查输入根因 1）。架构测试
/// （composer.arch.test.ts）拦着别的模块自己去读 top / bottom / row 这几条正则。

import { managedTerminalFrame, managedTerminalScreen, writeManagedTerminal, type ChatUi, type TerminalScreenFrameDto } from "../../api";

export type ComposerSpec = NonNullable<ChatUi["composer"]>;
export type ComposerAltMode = ComposerSpec["alt_modes"][number];
export type ComposerState = "empty" | "occupied" | "unknown";
/// 读屏那一刻的硬件光标（与文字同一帧）。
export type ComposerCursor = { row: number; col: number; visible: boolean; dim: boolean };
/// 草稿能不能从屏幕上原样读回来。屏幕只是 CLI 画出来的样子：大段粘贴被折成占位、内容超出输入框
/// 可视高度、长行被框宽折行时，屏幕上的字不等于草稿原文（做空①，五家真机取证见
/// tests/probe_draft_fidelity.rs）。`complete` 才允许清掉终端里的原稿，`text` 保留行首缩进与空行；
/// 读不全时 `why` 说明原因：paste = 含粘贴或图片占位，overflow = 没显示全，wrap = 分不清折行与
/// 换行，unverified = 这一家没有取证过的判定依据。
export type ComposerDraft =
  | { complete: true; text: string }
  | { complete: false; why: "paste" | "overflow" | "wrap" | "unverified" };
export type ComposerRead = {
  /// empty = 没有用户的字（空白或只有占位）；occupied = 有草稿；unknown = 认不出，或处在特殊输入模式。
  state: ComposerState;
  /// 输入框里的文字（去掉提示符；多行用换行连接，每行去掉首尾空白）。只用来判空、比对开头，
  /// **不是草稿原文**；要搬走草稿用 `draft`。认不出时没有。
  text?: string;
  /// 有草稿时：能不能原样读回，以及原样的全文。
  draft?: ComposerDraft;
  inputRow?: number;
  inputCol?: number;
  /// 上边框、底边所在行（子代理视图据此取边框上的描述与底边下方的面板）。
  topRow?: number;
  bottomRow?: number;
  /// 有字，但那是 CLI 画的占位。
  isPlaceholder?: boolean;
  /// 处在哪种特殊输入模式（shell、vim NORMAL）：此时写进去的字不是普通消息，state 恒为 unknown。
  altMode?: ComposerAltMode;
};

const UNKNOWN: ComposerRead = { state: "unknown" };

/// 带捕获位置的正则（`d` 标志）。老内核不认这个标志时退回普通正则，位置改用查找估算。
function indexed(pattern: string): RegExp {
  try { return new RegExp(pattern, "d"); } catch { return new RegExp(pattern); }
}

/// 捕获组 1 在整行里从第几个字符开始。
function captureStart(match: RegExpExecArray): number {
  const indices = (match as RegExpExecArray & { indices?: ([number, number] | undefined)[] }).indices;
  const exact = indices?.[1]?.[0];
  if (exact !== undefined) return exact;
  return match[1] ? match.index + match[0].indexOf(match[1]) : match.index + match[0].length;
}

/// 占几个终端格，只往宽里估（U+1100 起一律按两格）：估宽了只会更保守地判成「分不清」。
function widthAtMost(text: string): number {
  let width = 0;
  for (const char of text) width += (char.codePointAt(0) ?? 0) < 0x1100 ? 1 : 2;
  return width;
}

/// 逐行的原样内容 → 能不能确认这就是草稿全文。规则每家一份声明（spec.fidelity），这里不写任何
/// 一家的字符。行尾空白屏幕上看不见，不在「原样」之内。
function judgeDraft(rows: readonly string[], frame: { screen: readonly string[]; topRow: number; bottomRow: number; inputCol: number; cols?: number }, spec: ComposerSpec): ComposerDraft {
  const rule = spec.fidelity;
  if (!rule) return { complete: false, why: "unverified" };
  const paste = new RegExp(rule.paste_pattern);
  if (rows.some((row) => paste.test(row))) return { complete: false, why: "paste" };
  if (rule.overflow_pattern) {
    const overflow = new RegExp(rule.overflow_pattern);
    if (frame.screen.slice(frame.topRow, frame.bottomRow + 1).some((line) => overflow.test(line))) return { complete: false, why: "overflow" };
  }
  if (rule.overflow_rows != null && rows.length >= rule.overflow_rows) return { complete: false, why: "overflow" };
  if (rows.length > 1) {
    // 相邻两行之间是用户敲的换行，还是框宽折出来的：屏幕上没有标记（终端仿真层的折行标记只管终端
    // 自己的自动折行，TUI 是逐行定位画的，五家实拍都不带）。只有一种情况能确认是换行：下一行开头
    // 那个词（连同它前面的缩进）本来放得进上一行剩下的宽度，CLI 却没有放。放不进的分不清，判读不全。
    const edge = rule.wrap_edge === "border" ? [...(frame.screen[frame.bottomRow] ?? "").trimEnd()].length : frame.cols;
    if (!edge) return { complete: false, why: "unverified" };
    const width = edge - frame.inputCol - rule.wrap_inset;
    for (let index = 0; index + 1 < rows.length; index += 1) {
      const room = width - widthAtMost(rows[index]);
      const head = /^\s*\S*/.exec(rows[index + 1])?.[0] ?? "";
      if (room < Math.max(2, 1 + widthAtMost(head))) return { complete: false, why: "wrap" };
    }
  }
  let end = rows.length;
  while (end > 0 && rows[end - 1] === "") end -= 1;
  return { complete: true, text: rows.slice(0, end).join("\n") };
}

/// 在整屏上认出输入框。`cursor` 用来分开「占位文字」和「内容相同的真实草稿」：占位时光标停在
/// 输入起点，真打出来的字光标在文末（五家真机取证，2026-11-14）；没有光标就只看文字。
export function readComposer(screen: readonly string[] | null, spec: ComposerSpec | null | undefined, cursor?: ComposerCursor | null, cols?: number): ComposerRead {
  if (!screen || !spec) return UNKNOWN;
  try {
    const top = new RegExp(spec.top_pattern);
    const bottom = new RegExp(spec.bottom_pattern);
    const row = indexed(spec.row_pattern);
    let end = screen.length - 1;
    while (end >= 0 && !bottom.test(screen[end])) end -= 1;
    if (end < 0) return UNKNOWN;
    const bottomRow = end;
    let start = end;
    let topRow: number;
    let body: string[];
    if (spec.prompt_footer) {
      // footer 与正文之间有空行。不能只取最后一个非空行，否则多行尾部会遮掉真正的 prompt。
      while (end > 0 && screen[end - 1].trim() === "") end -= 1;
      start = end - 1;
      while (start >= 0 && !top.test(screen[start])) {
        if (screen[start].trim() !== "" && !row.test(screen[start])) return UNKNOWN;
        start -= 1;
      }
      if (start < 0) return UNKNOWN;
      topRow = start;
      body = screen.slice(start, end);
    } else {
      while (start > 0 && row.test(screen[start - 1])) start -= 1;
      body = screen.slice(start, end);
      // OpenCode 顶线也是空内容行；从完整连续框体起点找，不能跳过正文去找最后一个空行。
      if (body.length > 0 && top.test(body[0])) { body.shift(); topRow = start; start += 1; }
      else if (start === 0 || !top.test(screen[start - 1])) return UNKNOWN;
      else topRow = start - 1;
      if (spec.tail_rows > 0) {
        if (!spec.tail_pattern || body.length <= spec.tail_rows
          || !new RegExp(spec.tail_pattern).test(body[body.length - 1])) return UNKNOWN;
        body.splice(body.length - spec.tail_rows);
      }
    }
    if (body.length === 0) return UNKNOWN;
    const text: string[] = [];
    // 每行捕获到的原样内容与它在整行里的起点：读草稿原文时按输入起点那一列对齐，保住行首缩进。
    const captured: { value: string; start: number }[] = [];
    for (const line of body) {
      if (spec.prompt_footer && line.trim() === "") { text.push(""); captured.push({ value: "", start: 0 }); continue; }
      const match = row.exec(line);
      if (!match || match[1] === undefined) return UNKNOWN;
      text.push(match[1].trim());
      captured.push({ value: match[1], start: captureStart(match) });
    }
    // 特殊输入模式先于提示符判断：shell 模式的提示符本来就不是普通的那个。
    const altMode = spec.alt_modes.find((mode) => (mode.prompt_pattern != null && new RegExp(mode.prompt_pattern).test(text[0]))
      || (mode.below_pattern != null && screen.slice(bottomRow + 1).some((line) => new RegExp(mode.below_pattern!).test(line))));
    // prompt_prefix 可列几个提示符，用 `|` 分隔（Gemini 平常 `>`、YOLO `*`，做空 R2）；空串 = 没有提示符。
    const promptPrefix = spec.prompt_prefix.split("|").find((prefix) => text[0].startsWith(prefix));
    if (promptPrefix === undefined) {
      // 提示符对不上：只有认出是特殊模式（shell 的 `!`）时才算认出了框。
      return altMode ? { state: "unknown", text: text.join("\n").trim(), inputRow: start, topRow, bottomRow, altMode } : UNKNOWN;
    }
    const prefix = new RegExp(spec.input_start_pattern).exec(body[0]);
    if (!prefix || prefix[1] === undefined) return UNKNOWN;
    text[0] = text[0].slice(promptPrefix.length).trimStart();
    const content = text.join("\n").trim();
    // 插件只声明实拍单宽前缀；不能拿灰色占位的首字位置当输入起点。
    const inputCol = [...prefix[1]].length;
    const position = { text: content, inputRow: start, inputCol, topRow, bottomRow };
    if (altMode) return { state: "unknown", ...position, altMode };
    if (content === "") return { state: "empty", ...position, isPlaceholder: false };
    const matchesEmpty = new RegExp(spec.empty_pattern).test(content);
    const inBody = cursor != null && cursor.visible && cursor.row >= start && cursor.row < start + body.length;
    // 光标在输入框里：停在输入起点才可能是占位（暗色格，或文字对得上占位）；在别处就是用户真打出来
    // 的字，哪怕和占位一模一样。没有光标证据（隐藏，或停在别处）只能看文字。
    const placeholder = inBody
      ? cursor.row === start && cursor.col === inputCol && (cursor.dim || matchesEmpty)
      : matchesEmpty;
    if (placeholder) return { state: "empty", ...position, isPlaceholder: true };
    // 各行从输入起点那一列起才是用户的字；那一列之前只能是提示符（首行）或空白（续行），否则这个
    // 画面不是取证过的样子，不敢说读全了。
    let aligned = true;
    const rows = captured.map(({ value, start: at }, index) => {
      const skip = inputCol - at;
      if (skip < 0 || (index > 0 && value.slice(0, skip).trim() !== "")) aligned = false;
      return [...value].slice(Math.max(0, skip)).join("").trimEnd();
    });
    const draft: ComposerDraft = aligned
      ? judgeDraft(rows, { screen, topRow, bottomRow, inputCol, cols }, spec)
      : { complete: false, why: "unverified" };
    return { state: "occupied", ...position, isPlaceholder: false, draft };
  } catch {
    return UNKNOWN;
  }
}

/// 同一帧的文字与光标一起读（分两次读会把异步重绘误认成用户移动了光标）。
export function readComposerFrame(frame: TerminalScreenFrameDto | null | undefined, spec: ComposerSpec | null | undefined): ComposerRead {
  if (!frame) return UNKNOWN;
  return readComposer(frame.lines, spec, { row: frame.cursorRow, col: frame.cursorCol, visible: frame.cursorVisible, dim: frame.cursorDim === true }, frame.cols ?? undefined);
}

/// 输入框里有没有字、大致是什么：认不出返回 null，空（含占位）返回 ""，否则是屏幕上那几行
/// （每行去掉首尾空白）。只用来判断与比对；要把草稿搬走必须用 `read.draft`，它才知道读没读全。
export function composerDraft(read: ComposerRead): string | null {
  if (read.state === "unknown") return null;
  return read.state === "empty" ? "" : read.text ?? "";
}

/// 某段话是不是还停在输入框里（没按下回车、撤回后被 CLI 放回来）。只拿行首一小段比：从行首起排，
/// 不受折行影响；首行开头或末行开头对得上就算。
export function composerHolds(read: ComposerRead, message: string): boolean {
  if (read.state !== "occupied" || !read.text) return false;
  const lines = message.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) return false;
  const rows = read.text.split("\n");
  const first = lines[0].slice(0, 12);
  const last = lines[lines.length - 1].slice(0, 12);
  return rows[0].startsWith(first) || rows.some((row) => row.includes(last));
}

/// 读屏与按键的出入口。测试里换成回放实拍帧的假终端。
export type ComposerIo = {
  frame: () => Promise<TerminalScreenFrameDto | null>;
  write: (data: string) => Promise<void>;
  sleep?: (ms: number) => Promise<void>;
};

const defaultSleep = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

/// 某个会话的托管终端。帧（文字加光标）取不到时退回只有文字的整屏：判定退回「只看文字」，
/// 不会因为少了光标就认不出输入框。
export function terminalComposerIo(sessionId: number): ComposerIo {
  return {
    frame: async () => {
      // 同步抛错（接口不存在的旧宿主）与异步失败都落到「取不到」。
      const safely = async <T,>(read: () => Promise<T | null>): Promise<T | null> => {
        try { return await read() ?? null; } catch { return null; }
      };
      const frame = await safely(() => managedTerminalFrame(sessionId));
      if (frame) return frame;
      const lines = await safely(() => managedTerminalScreen(sessionId));
      return lines ? { lines, cursorRow: 0, cursorCol: 0, cursorVisible: false, cursorDim: false } : null;
    },
    write: (data) => writeManagedTerminal(sessionId, data),
  };
}

async function readNow(io: ComposerIo, spec: ComposerSpec): Promise<ComposerRead> {
  return readComposerFrame(await io.frame().catch(() => null), spec);
}

/// 收起草稿（CLI 自己保管，下一次提交后还原；issue #72）。暂存键是切换语义：
/// - 认不出输入框或输入框空 → 不按（空框上按会把旧暂存放出来）；
/// - 非空才按；按前有暂存标记、按后消失 = 放出了用户手动暂存的内容，再按一次收回。
/// 返回是否确认收起（或本来就空）。已知代价：输入框非空且用户另有手动暂存时，新暂存会覆盖旧的
/// （CLI 只存一份）。
export async function stashComposerDraft(io: ComposerIo, spec: ComposerSpec): Promise<boolean> {
  const stash = spec.stash;
  if (!stash) return false;
  const sleep = io.sleep ?? defaultSleep;
  try {
    const frame = await io.frame();
    const read = readComposerFrame(frame, spec);
    if (read.state === "empty") return true;
    if (read.state !== "occupied") return false;
    const hasMarker = (lines: readonly string[] | undefined) => !!lines?.some((line) => line.includes(stash.stashed_marker));
    const hadStash = hasMarker(frame?.lines);
    await io.write(stash.input);
    // 成功条件是输入框变空，不看标记出现：用户已有手动暂存时标记按前按后都在。
    for (let attempt = 0; attempt < 8; attempt += 1) {
      await sleep(50);
      const now = await io.frame();
      if (readComposerFrame(now, spec).state === "empty") return true;
      if (hadStash && !hasMarker(now?.lines)) {
        await io.write(stash.input);
        return true;
      }
    }
  } catch {
    // 落到调用方：按「没收起」处理。
  }
  return false;
}

/// 用 CLI 自己的清空键清掉输入框里的草稿。每按一下就看一眼：已经空了就不再按——这些键在空
/// 输入框上另有含义（退出、打开回退菜单）。逐个分开写，两个 Esc 合成一次写入会被当成 Alt+Esc。
/// 一轮清不完的（gemini 逐行删）按规格重复几轮，每轮后复核。返回是否确认清空。
export async function clearComposerDraft(io: ComposerIo, spec: ComposerSpec): Promise<boolean> {
  if (spec.clear_inputs.length === 0) return false;
  const sleep = io.sleep ?? defaultSleep;
  const state = async () => (await readNow(io, spec)).state;
  try {
    for (let round = 0; round < Math.max(1, spec.clear_rounds); round += 1) {
      const before = await state();
      if (before === "empty") return true;
      // 认不出就不敢按。
      if (before !== "occupied") return false;
      for (const [index, input] of spec.clear_inputs.entries()) {
        if (index > 0) {
          await sleep(spec.clear_rounds > 1 ? 60 : 300);
          if (await state() === "empty") return true;
        }
        await io.write(input);
      }
      for (let attempt = 0; attempt < 10; attempt += 1) {
        await sleep(100);
        const now = await state();
        if (now === "empty") return true;
        // 多轮清空：这一轮已经落定（画面还认得出、还有字）就进下一轮，不等满。
        if (spec.clear_rounds > 1 && now === "occupied" && attempt >= 2) break;
      }
    }
  } catch {
    // 落到调用方的提示。
  }
  return false;
}

/// 退出特殊输入模式（Gemini 的 shell 模式按 Esc、vim NORMAL 按 i）。返回是否确认回到普通输入框。
export async function leaveAltMode(io: ComposerIo, spec: ComposerSpec, mode: ComposerAltMode): Promise<boolean> {
  const sleep = io.sleep ?? defaultSleep;
  try {
    await io.write(mode.exit_input);
    for (let attempt = 0; attempt < 10; attempt += 1) {
      await sleep(100);
      const now = await readNow(io, spec);
      if (!now.altMode && now.state !== "unknown") return true;
    }
  } catch {
    // 落到调用方的提示。
  }
  return false;
}

/// 往终端写正文之前把输入框收拾干净的结局。
export type ComposerPrepared =
  /// 可以直接写。verified = 确实读到了空输入框；false = 没有规格可依据，或认不出但这一家照发。
  | { kind: "clean"; verified: boolean }
  /// 草稿被 CLI 收起，提交后它自己还原，终端里一个字不丢。
  | { kind: "stashed" }
  /// 草稿已从终端清掉，内容在 `draft` 里，调用方接进对话页输入框。
  | { kind: "moved"; draft: string }
  /// 终端里有草稿，这次没能自动收起。`draft` 是读到的内容（调用方可以先保进对话页）；
  /// `reason`：running = 回合进行中按清空键会打断回合；failed = 按了没清掉；no_clear = 这一家没有取证过的清空键。
  | { kind: "occupied"; draft: string; reason: "running" | "failed" | "no_clear" }
  /// 终端里有草稿，但从屏幕上读不全（见 ComposerDraft）：**没有按任何键**，草稿原样留在终端里。
  /// 调用方给出能在对话页完成的选择（连同草稿一起发、放弃草稿后发），由人决定。
  | { kind: "incomplete"; why: Extract<ComposerDraft, { complete: false }>["why"] }
  /// 处在特殊输入模式且没能退出（`mode` 是模式标识）；`running` = 回合进行中没敢按退出键。
  | { kind: "alt_mode"; mode: string; running: boolean }
  /// 认不出输入框（菜单、对话框、布局漂移）。
  | { kind: "unrecognized" };

/// 往终端写正文之前：认输入框 → 退出特殊输入模式 → 有草稿就收起（能暂存的暂存，否则清空并把内容
/// 交给调用方搬进对话页）。不让用户去终端收拾（standards 2026-11-10），也不往认不出的画面里按键。
/// `force`：用户在错误条上点了「仍然发送」，认不出也按 clean 放行。
///
/// 保不住就不清（做空①）：只有确认能把草稿原样读回来（`read.draft.complete`）才按清空键；读不全
/// 就一个键也不按，返回 incomplete。此前读到什么搬什么：Codex 的 `[Pasted Content 5120 chars]`
/// 被当成草稿搬走、原文清掉；长行折行处多出换行；超出可视高度的部分和行首缩进直接丢。
/// `draft`：用户在错误条上对读不全的草稿做了选择。keep = 草稿留着，正文接在它后面一起发
/// （先写一个 `separator` 换行隔开）；discard = 用户确认放弃，清掉草稿再发。
export async function prepareComposer(io: ComposerIo, spec: ComposerSpec | null | undefined, options: { running: boolean; force?: boolean; draft?: "keep" | "discard"; separator?: string | null }): Promise<ComposerPrepared> {
  // 没声明规格（旧版本或未适配）：没有可依据的画面规则，照发。
  if (!spec) return { kind: "clean", verified: false };
  const unrecognized = (): ComposerPrepared => (options.force || !spec.send_requires_recognition ? { kind: "clean", verified: false } : { kind: "unrecognized" });
  let read = await readNow(io, spec);
  if (read.altMode) {
    const mode = read.altMode;
    // 退出键在回合进行中可能是中断（Esc），只在空闲时按。
    if (options.running) return { kind: "alt_mode", mode: mode.id, running: true };
    if (!await leaveAltMode(io, spec, mode)) return { kind: "alt_mode", mode: mode.id, running: false };
    read = await readNow(io, spec);
  }
  if (read.state === "empty") return { kind: "clean", verified: true };
  if (read.state === "unknown") return unrecognized();
  const fidelity: ComposerDraft = read.draft ?? { complete: false, why: "unverified" };
  const draft = fidelity.complete ? fidelity.text : "";
  if (spec.stash) {
    // 暂存不经过屏幕：草稿由 CLI 自己保管、提交后原样还原，读不读得全都不影响。
    if (await stashComposerDraft(io, spec)) return { kind: "stashed" };
    // 暂存没成：按认不出处理，不再去按清空键（claude 的清空键在运行中是中断）。
    return unrecognized();
  }
  if (options.draft === "keep") {
    // 换行键写不进去也照发：最坏是正文紧接在草稿后面，内容都在。
    if (options.separator) await io.write(options.separator).catch(() => {});
    return { kind: "clean", verified: false };
  }
  if (spec.clear_inputs.length === 0) return { kind: "occupied", draft, reason: "no_clear" };
  if (options.running && !spec.clear_while_running) return { kind: "occupied", draft, reason: "running" };
  if (options.draft === "discard") {
    return await clearComposerDraft(io, spec) ? { kind: "clean", verified: true } : { kind: "occupied", draft, reason: "failed" };
  }
  if (!fidelity.complete) return { kind: "incomplete", why: fidelity.why };
  // 按清空键之前再读一遍：两次读到的原文一致才清。中间用户又在终端里打了字的话，清掉的就不是
  // 手上这份了。
  const now = await readNow(io, spec);
  if (now.state === "empty") return { kind: "clean", verified: true };
  const again = now.draft;
  if (!again?.complete || again.text !== fidelity.text) return { kind: "incomplete", why: again && !again.complete ? again.why : "unverified" };
  if (await clearComposerDraft(io, spec)) return { kind: "moved", draft };
  return { kind: "occupied", draft, reason: "failed" };
}

export type ComposerConfirmationIo = {
  read: () => Promise<TerminalScreenFrameDto | null>;
};

// 没有原生键消费回执时的严格口径：只有「没有任何文字、光标就在输入起点」才算空，占位也不放行。
// 目前没有生产调用方：原生会话不做自动送达（答复只保存，见 lead_conversation 的
// native_session_status）；保留给将来真要在人不在场时往原生输入框写入的路径，届时必须用它。
export async function confirmComposerEmpty(io: ComposerConfirmationIo, spec: ComposerSpec | null | undefined): Promise<ComposerState> {
  if (!spec) return "unknown";
  try {
    const frame = await io.read();
    if (!frame) return "unknown";
    const read = readComposerFrame(frame, spec);
    if (read.state === "occupied") return "occupied";
    if (read.state !== "empty" || read.isPlaceholder !== false) return "unknown";
    return frame.cursorVisible && frame.cursorRow === read.inputRow && frame.cursorCol === read.inputCol
      ? "empty" : "unknown";
  } catch {
    return "unknown";
  }
}
