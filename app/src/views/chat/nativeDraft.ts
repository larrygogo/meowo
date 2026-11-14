/// 对话页往终端写东西之前，终端输入框里已有草稿时怎么办——界面这一侧。
///
/// 终端协议（认框、暂存、清空、退出特殊输入模式）在 composer.ts；这里把它的结局翻译成对话页的
/// 动作：草稿被清掉的搬进对话页输入框，收不掉的变成一条带出路的错误。ChatWindow 只留接线。
///
/// 口径（standards 2026-11-10「不要让用户去终端去清除」）：Meowo 先自己收，把用户的内容搬到
/// 对话页保住；确实做不到才报，并且错误条上有能在对话页完成的按钮，说清原因。此前四家
/// （Codex、Kimi、Gemini、OpenCode）终端里有草稿时只报「请先在终端发送或清空该草稿」，
/// 错误条没有出口，手机上是死路（走查输入 B6）。

import type { Dispatch, SetStateAction } from "react";
import type { Dict } from "../../i18n/zh";
import type { ChatDiagEvent } from "./chatDiag";
import { clearComposerDraft, prepareComposer, readComposerFrame, terminalComposerIo, type ComposerIo, type ComposerPrepared, type ComposerSpec, type ComposerState } from "./composer";
import type { SendErrorExit } from "./SendErrorExits";
import { useSessionRef, useSessionState, type SessionScope } from "./sessionScope";

export type ComposerBlock = Extract<ComposerPrepared, { kind: "occupied" | "alt_mode" | "unrecognized" | "incomplete" }>;

/// 用户在错误条上为下一次发送做的选择（只管一次）：force = 认不出输入框也照发；
/// draft = 读不全的终端草稿怎么办（keep 连同它一起发，discard 放弃它再发）。
export type NextSend = { force?: boolean; draft?: "keep" | "discard" };

/// 输入框没收拾好，这次不写终端。带着结局，错误条据此给文案与出路。
export class ComposerBlocked extends Error {
  constructor(readonly block: ComposerBlock) {
    super(`terminal/composer_${block.kind}`);
    this.name = "ComposerBlocked";
  }
}

/// 写终端之前收拾输入框。草稿被清掉时经 `onMoved` 交给调用方（接进对话页输入框）；收不掉就抛
/// ComposerBlocked，一个字也不往终端写。返回值给调用方判断输入框是否确认干净。
export async function tidyComposer(
  target: number | ComposerIo,
  spec: ComposerSpec | null | undefined,
  options: { running: boolean; force?: boolean; draft?: "keep" | "discard"; separator?: string | null },
  onMoved: (draft: string) => void,
): Promise<ComposerPrepared> {
  const io = typeof target === "number" ? terminalComposerIo(target) : target;
  const prepared = await prepareComposer(io, spec, options);
  if (prepared.kind === "moved") {
    if (prepared.draft.trim()) onMoved(prepared.draft);
    return prepared;
  }
  if (prepared.kind === "clean" || prepared.kind === "stashed") return prepared;
  throw new ComposerBlocked(prepared);
}

/// 把搬过来的终端草稿接在对话页输入框已有内容后面（不覆盖用户正在写的东西）。
export function appendDraft(current: string, draft: string): string {
  return current.trim() ? `${current}\n\n${draft}` : draft;
}

/// 错误条：说清原因，给能在对话页完成的出路。
/// - `retryLabel` / `force`：重试钮的文案，以及重试时是否放行「认不出输入框」。认不出时 Meowo
///   不敢替用户判断，按钮写明「仍然发送」，由人看过终端画面后决定。
export function composerBlockNotice(block: ComposerBlock, t: Dict): { message: string; retryLabel: string; force: boolean } {
  if (block.kind === "unrecognized") return { message: t.chat.composerUnrecognized, retryLabel: t.chat.composerSendAnyway, force: true };
  if (block.kind === "incomplete") {
    const message = block.why === "paste" ? t.chat.composerDraftPaste
      : block.why === "overflow" ? t.chat.composerDraftOverflow
      : block.why === "wrap" ? t.chat.composerDraftWrapped
      : t.chat.composerDraftUnsure;
    return { message, retryLabel: t.chat.composerSendWithDraft, force: false };
  }
  if (block.kind === "alt_mode") {
    const message = block.running ? t.chat.composerModeRunning
      : block.mode === "shell" ? t.chat.composerModeShell
      : block.mode === "vim_normal" ? t.chat.composerModeVim
      : t.chat.composerModeOther;
    return { message, retryLabel: t.chat.errorRetry, force: false };
  }
  const message = block.reason === "running" ? t.chat.composerDraftRunning
    : block.reason === "no_clear" ? t.chat.composerDraftNoClear
    : t.chat.composerDraftStuck;
  return { message, retryLabel: t.chat.errorRetry, force: false };
}

/// 错误条上的出路。读不全的草稿（保不住就不清，做空①）不给「重试」，再试一次结果一样；给两个
/// 由人决定的选择：连同草稿一起发、放弃草稿后发。放弃会丢内容，要再点一次确认。草稿此刻还在
/// 终端里，所以同时给看终端画面的出口（手机上是就地展开）。
export function composerBlockExit(block: ComposerBlock, t: Dict, resend: (next: NextSend) => void): SendErrorExit {
  const notice = composerBlockNotice(block, t);
  if (block.kind !== "incomplete") {
    return { terminal: true, retryLabel: notice.retryLabel, retry: () => resend({ force: notice.force }) };
  }
  return {
    terminal: true,
    actions: [
      { label: t.chat.composerSendWithDraft, run: () => resend({ draft: "keep" }) },
      { label: t.chat.composerDiscardDraft, confirmLabel: t.chat.composerDiscardConfirm, run: () => resend({ draft: "discard" }) },
    ],
  };
}

/// 被拦下的发送写进运行日志的那一行（做空⑧：此前拦下了只有错误条，日志里没有）。只有固定代码，
/// 不带草稿或消息正文。
export function composerBlockEvent(block: ComposerBlock): ChatDiagEvent {
  if (block.kind === "unrecognized") return { kind: "send_blocked", reason: "composer_unrecognized" };
  if (block.kind === "alt_mode") return { kind: "send_blocked", reason: "composer_alt_mode", detail: { mode: block.mode, running: block.running } };
  if (block.kind === "incomplete") return { kind: "send_blocked", reason: "composer_draft_incomplete", detail: { why: block.why } };
  return { kind: "send_blocked", reason: "composer_draft_kept", detail: { why: block.reason } };
}

/// 对话窗里这一块的接线：收拾输入框、把搬来的草稿接进输入框、一次性的说明条、「仍然发送」。
/// 状态都是会话级的（换会话归零，见 sessionScope）。
export function useNativeDraft(deps: {
  scope: SessionScope;
  sessionId: number;
  spec: ComposerSpec | null | undefined;
  /// 这一家在输入框里换行的键：连同终端草稿一起发时，先用它把草稿和正文隔开。
  newline?: string | null;
  setPrompt: Dispatch<SetStateAction<string>>;
  /// 用户还停在发起的那个会话。
  isCurrent: () => boolean;
  running: () => boolean;
  /// 切走之后才搬来的草稿：记进那个会话存着的草稿里，切回去就能看到。
  storeDraft: (sessionId: number, merge: (prev: string) => string) => void;
}) {
  const { scope, sessionId, spec } = deps;
  // 终端里的草稿被搬进对话页输入框后的一句说明。
  const [moved, setMoved] = useSessionState(scope, false);
  // 错误条上做的选择：只管这个会话的下一次发送（见 NextSend）。
  const nextRef = useSessionRef<NextSend>(scope, {});
  const adopt = (draft: string) => {
    if (!deps.isCurrent()) { deps.storeDraft(sessionId, (prev) => appendDraft(prev, draft)); return; }
    deps.setPrompt((current) => appendDraft(current, draft));
    setMoved(true);
  };
  return {
    moved,
    dismiss: () => setMoved(false),
    setNext: (next: NextSend) => { nextRef.current = next; },
    /// 往终端写正文或命令之前调用。`own`：Meowo 自己写进去、还没提交成的正文——清出来的若是它们，
    /// 不当成用户的草稿搬进输入框（原文在各自的回执里，再搬一份用户会看到两遍）。
    tidy: (own: readonly string[] = []) => {
      const next = nextRef.current;
      nextRef.current = {};
      const squash = (text: string) => text.replace(/\s+/g, "");
      const ownResidue = (draft: string) => squash(draft) !== "" && own.some((text) => squash(text).startsWith(squash(draft).slice(0, 24)));
      // 切走的会话读不到实时状态：按在跑处理（不按可能打断回合的键）。
      const running = !deps.isCurrent() || deps.running();
      return tidyComposer(sessionId, spec, { running, ...next, separator: deps.newline }, (draft) => { if (!ownResidue(draft)) adopt(draft); });
    },
    /// 只读：输入框此刻的状态与里面的字（切模型前判断要不要搬草稿）。
    peek: async (): Promise<{ state: ComposerState; text: string }> => {
      const read = readComposerFrame(await terminalComposerIo(sessionId).frame(), spec);
      const text = read.text?.trim() ?? "";
      return { state: read.state === "occupied" && !text ? "unknown" : read.state, text };
    },
    /// 把输入框里的草稿清掉并搬进对话页输入框（不重启的切换路线遇到「输入框里有字」时用）。
    take: async (): Promise<"empty" | "moved" | "occupied" | "unknown"> => {
      if (!spec) return "unknown";
      const io = terminalComposerIo(sessionId);
      const read = readComposerFrame(await io.frame(), spec);
      if (read.state === "empty") return "empty";
      if (read.state !== "occupied" || !read.text?.trim()) return "unknown";
      // 读不全就不清（保不住就不清）：草稿留在终端里，切换按「输入框里有字」如实报。
      if (!read.draft?.complete) return "occupied";
      if (!await clearComposerDraft(io, spec)) return "occupied";
      adopt(read.draft.text);
      return "moved";
    },
  };
}
