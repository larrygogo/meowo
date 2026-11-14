/// 对话页往终端写东西之前，终端输入框里已有草稿时怎么办——界面这一侧。
///
/// 终端协议（认框、暂存、清空、退出特殊输入模式）在 composer.ts；这里把它的结局翻译成对话页的
/// 动作：草稿被清掉的搬进对话页输入框，收不掉的变成一条带出路的错误。ChatWindow 只留接线。
///
/// 口径（standards 2026-11-10「不要让用户去终端去清除」）：Meowo 先自己收，把用户的内容搬到
/// 对话页保住；确实做不到才报，并且错误条上有能在对话页完成的按钮，说清原因。此前四家
/// （Codex、Kimi、Gemini、OpenCode）终端里有草稿时只报「请先在终端发送或清空该草稿」，
/// 错误条没有出口，手机上是死路（走查输入 B6）。

import type { Dict } from "../../i18n/zh";
import { prepareComposer, terminalComposerIo, type ComposerIo, type ComposerPrepared, type ComposerSpec } from "./composer";

export type ComposerBlock = Extract<ComposerPrepared, { kind: "occupied" | "alt_mode" | "unrecognized" }>;

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
  options: { running: boolean; force?: boolean },
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
