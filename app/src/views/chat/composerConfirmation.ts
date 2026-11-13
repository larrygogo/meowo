import type { TerminalScreenFrameDto } from "../../api";
import { composerCandidate, type ComposerGuard, type ComposerSafety } from "./composerSafety";

export type ComposerConfirmationIo = {
  read: () => Promise<TerminalScreenFrameDto | null>;
};

export type ManualSendVerdict = "send" | "occupied" | "unrecognized";

// 用户在对话页亲手发送时的判定（人在场）。与 confirmComposerEmpty 的严格口径分开：
// - 认出输入框且有字 → occupied，拦住，保住原生草稿；
// - 认出输入框但只能看到占位/空白 → send。codex/gemini/opencode 的占位与同文草稿分不出，
//   严格口径下它们永远 unknown，对话页就永远发不出去（2026-11-13 接手实测，main 上可发）；
// - 有守卫却认不出输入框（菜单、对话框、布局漂移）→ unrecognized，拦住，避免回车误按进菜单；
// - 插件没声明守卫（旧版本或未适配）→ send，与 main 行为一致，没有可依据的画面规则。
export async function manualSendVerdict(io: ComposerConfirmationIo, guard: ComposerGuard | null | undefined): Promise<ManualSendVerdict> {
  if (!guard) return "send";
  try {
    const frame = await io.read();
    if (!frame) return "unrecognized";
    const candidate = composerCandidate(frame.lines, guard);
    if (candidate.state === "occupied") return "occupied";
    return candidate.state === "empty" ? "send" : "unrecognized";
  } catch {
    return "unrecognized";
  }
}

// 没有原生键消费回执，Right 不动不能证明空框；占位与同文草稿无法从当前 DTO 区分，必须拒绝。
// 严格口径。目前没有生产调用方：原生会话不做自动送达（答复只保存，见 lead_conversation 的
// native_session_status）；保留给将来真要在人不在场时往原生输入框写入的路径，届时必须用它。
export async function confirmComposerEmpty(io: ComposerConfirmationIo, guard: ComposerGuard | null | undefined): Promise<ComposerSafety> {
  if (!guard) return "unknown";
  try {
    const frame = await io.read();
    if (!frame) return "unknown";
    const candidate = composerCandidate(frame.lines, guard);
    if (candidate.state === "occupied") return "occupied";
    if (candidate.state !== "empty" || candidate.isPlaceholder !== false) return "unknown";
    return frame.cursorVisible && frame.cursorRow === candidate.inputRow && frame.cursorCol === candidate.inputCol
      ? "empty" : "unknown";
  } catch {
    return "unknown";
  }
}
