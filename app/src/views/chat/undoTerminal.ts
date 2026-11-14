/// 撤回的终端协议：发撤回命令、菜单式撤回里逐步核对屏幕、等撤回生效、清掉 CLI 放回输入框的原文。
///
/// 为什么单独成模块（对照 Kimi 把撤回放在服务层、视图只管呈现）：这些步骤只和终端打交道，
/// 不碰 React 状态；对话窗只负责确认弹窗、把原文放回输入框、按结果报错。拆出来之后协议的
/// 每一种结局都是一个明确的返回值，ChatWindow 里不再夹着百来行的轮询循环。

import { managedTerminalScreen, managedTerminalSnapshot, writeManagedTerminal, type ChatItem, type ChatUi } from "../../api";
import { appendTerminalText, visibleTerminalText } from "../../terminalAttention";
import { clearComposerDraft, composerDraft, composerHolds, prepareComposer, readComposerFrame, terminalComposerIo, type ComposerIo, type ComposerSpec } from "./composer";
import { submitToTerminal } from "./terminalSubmit";

export type UndoControl = NonNullable<ChatUi["undo"]>;

type UndoMenuSpec = NonNullable<NonNullable<ChatUi["undo"]>["menu"]>;

/// 输入框里手敲的是不是撤回命令（OpenCode `/undo`、Kimi `/undo 2`）：是的话返回要撤的那条用户消息，
/// 调用方改走与消息上「撤回」按钮同一条流程（确认、放回原文、清 CLI 输入框）。菜单式撤回（claude
/// `/rewind`）不在此列：它本来就走菜单识别。不带轮数 = 最近一轮；插话不算一轮（同按钮的数法）。
export function typedUndoTarget(text: string, control: UndoControl | null | undefined, items: readonly ChatItem[]): { id: string; text: string } | null {
  if (!control || control.menu) return null;
  let turns = 1;
  if (text !== control.command) {
    if (!control.count_arg || !text.startsWith(`${control.command} `)) return null;
    const count = text.slice(control.command.length).trim();
    if (!/^\d+$/.test(count)) return null;
    turns = Number(count);
  }
  if (turns < 1) return null;
  const turnsOf = items.flatMap((item) => item.type === "user_text" && !item.interjected && !item.to_agent ? [item] : []);
  const target = turnsOf[turnsOf.length - turns];
  return target ? { id: target.id, text: target.text } : null;
}

/// 原文非空行（trim 后）。屏幕核对只拿行首一小段比：从行首起排，不受折行影响。
function undoTextLines(text: string): string[] {
  return text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
}

/// 被撤的原文是不是回到了 CLI 自己的输入框：1 在、0 不在、null 认不出输入框。认框用插件那一份
/// 输入框规格（composer.ts），不另起一套。
async function heldRows(io: ComposerIo, spec: ComposerSpec, text: string): Promise<number | null> {
  const read = readComposerFrame(await io.frame().catch(() => null), spec);
  if (read.state === "unknown") return null;
  return composerHolds(read, text) ? 1 : 0;
}

/// 菜单式撤回（claude /rewind，规格见后端 UndoMenu）：打开菜单 → 上移 turns 格 → 核对选中行
/// → 回车 → 核对确认页引文 → 按「option_label」那一项的数字 → 等原文回到 CLI 输入框。
/// 每一步先看屏幕再按键：要撤的是哪条消息只有屏幕上那份列表说了算，计数与 CLI 对不上时宁可
/// 退出菜单报错，也不能撤错。菜单已由调用方用撤回命令打开。
async function runUndoMenu(sessionId: number, menu: UndoMenuSpec, returned: ComposerSpec | null, turns: number, text: string): Promise<"done" | "mismatch" | "timeout"> {
  const io = terminalComposerIo(sessionId);
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
    let done = false;
    for (let attempt = 0; attempt < 20 && !done; attempt += 1) {
      const closed = menuRows(await managedTerminalScreen(sessionId)) === null;
      done = closed && (!returned || (await heldRows(io, returned, text) ?? 0) > 0);
      if (!done) await sleep(150);
    }
    if (!done) { await cancel(); return "timeout"; }
    return "done";
  } catch {
    await cancel().catch(() => undefined);
    return "timeout";
  }
}

/// 撤回后清掉 CLI 放回它自己输入框的原文（opencode / claude）。原文已在对话页输入框里，终端那份
/// 不清会拼进下一条消息（真机复现过）。清空键在空输入框上另有含义（退出 / 打开回退菜单），所以
/// 只在「比撤回前多出了原文」时才按，按后确认空了才算干净。baseline = 发撤回命令前的 heldRows。
/// 返回是否已确认干净。
async function clearUndoneInput(io: ComposerIo, spec: ComposerSpec, text: string, baseline: number | null): Promise<boolean> {
  if (baseline === null) return false;
  try {
    let now: number | null = null;
    for (let attempt = 0; attempt < 10; attempt += 1) {
      now = await heldRows(io, spec, text);
      if (now !== null && now > baseline) break;
      await new Promise((resolve) => window.setTimeout(resolve, 100));
    }
    // 认得出输入框、没多出原文 = 本来就干净；认不出输入框则不敢按。
    if (now === null) return false;
    if (now <= baseline) return true;
    return await clearComposerDraft(io, spec);
  } catch {
    // 落到调用方的提示。
  }
  return false;
}

/// 撤回的结局。done.clean = CLI 放回它自己输入框的原文已确认清掉。没清掉时带 retryClear：
/// 错误条的「重试」再按一次 CLI 的清空键（同一套核对，返回是否确认干净），不叫用户去终端手动清
/// （用户 2026-11-10：「不要让用户去终端去清除」；走查 C-4）。
export type UndoOutcome =
  | { kind: "done"; clean: true }
  /// retryClear 缺省 = 撤回前就认不出输入框（baseline 为空）：不敢按清空键（空框上它是退出 / 回退菜单），
  /// 重试注定失败，调用方只给看终端画面并写明原因。
  | { kind: "done"; clean: false; retryClear?: () => Promise<boolean> }
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
  /// 这一家的输入框规格（认框、收草稿、清原文共用的那一份）。
  composer: ComposerSpec | null | undefined;
  attentionAbort: () => string | null;
  /// 用户还停在发起撤回的那个会话。切走了就不再推进，返回 stale。
  isCurrent: () => boolean;
  /// 拉一次 transcript，返回被撤的那条是否还在（非菜单式撤回以 transcript 为准）。
  stillHas: () => Promise<boolean>;
  /// 撤回刚生效时调用（在清终端输入框之前）：调用方此刻收掉本地消息、把原文放回输入框。
  onUndone: () => void;
  /// 终端输入框里原有的草稿已被清掉（菜单式撤回要求框是空的）：调用方立刻把它接进对话页输入框。
  /// 在撤回之前调用，撤回成功与否草稿都不丢。
  onDraftMoved?: (draft: string) => void;
  /// 等待撤回生效期间为 true：refresh 据此采信「撤掉唯一一轮」的空重灌。
  onPending: (pending: boolean) => void;
};

/// 撤回已生效后清 CLI 输入框里的原文，清不掉时把「再清一次」交给调用方。
async function undoneResult(io: ComposerIo, returned: ComposerSpec | null, text: string, baseline: number | null): Promise<UndoOutcome> {
  if (!returned) return { kind: "done", clean: true };
  if (await clearUndoneInput(io, returned, text, baseline)) return { kind: "done", clean: true };
  if (baseline === null) return { kind: "done", clean: false };
  return { kind: "done", clean: false, retryClear: () => clearUndoneInput(io, returned, text, baseline) };
}

/// 跑一次撤回。菜单式（claude /rewind）以屏幕为准；其余发命令后等 transcript 里那条消失，
/// 期间认插件声明的拒绝行（kimi 在不可撤时的提示）。写终端的异常照常抛给调用方。
export async function performUndo(run: UndoRun): Promise<UndoOutcome> {
  const { sessionId, control, turns, text } = run;
  const menu = control.menu;
  const spec = run.composer ?? null;
  const io = terminalComposerIo(sessionId);
  // CLI 会把被撤的原文放回它自己的输入框、而且这一家的输入框认得出：撤完要把那份清掉。
  const returned = control.returns_input ? spec : null;
  if (menu) {
    // 不走草稿暂存：claude 的暂存在下一次提交（就是这条 /rewind）后自动还原，会和撤回放回
    // 的原文挤在一起。以前输入框不空就请用户去终端清（用户反馈：不要让用户去终端清）。现在
    // 读出整段草稿、用 CLI 自己的清空键清掉，草稿搬进对话页输入框，再撤回。认不出框或清不掉
    // 才退回原来的提示。
    const draft = spec ? composerDraft(readComposerFrame(await io.frame().catch(() => null), spec)) ?? "" : "";
    if (draft) {
      if (!spec || !await clearComposerDraft(io, spec)) return { kind: "composer_busy" };
      run.onDraftMoved?.(draft);
    }
    await submitToTerminal(sessionId, control.command, run.attentionAbort);
    const outcome = await runUndoMenu(sessionId, menu, returned, turns, text);
    if (!run.isCurrent()) return { kind: "stale" };
    if (outcome !== "done") return { kind: outcome === "mismatch" ? "mismatch" : "unconfirmed" };
    run.onUndone();
    return await undoneResult(io, returned, text, 0);
  }
  const before = await managedTerminalSnapshot(sessionId);
  // 输入框里原有的草稿先收起：能暂存的暂存，否则清掉并搬进对话页输入框。留着的话撤回命令会接在
  // 草稿后面一起提交。收不掉就不撤（撤回只在空闲时做，running 恒为 false）。认不出输入框照旧发命令
  // （force）：撤完按「认不出，原文没能自动清掉」如实说，不把撤回整个拦下。
  const prepared = await prepareComposer(io, spec, { running: false, force: true });
  if (prepared.kind === "moved") run.onDraftMoved?.(prepared.draft);
  else if (prepared.kind !== "clean" && prepared.kind !== "stashed") return { kind: "composer_busy" };
  const inputBaseline = returned ? await heldRows(io, returned, text) : null;
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
        return await undoneResult(io, returned, text, inputBaseline);
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
