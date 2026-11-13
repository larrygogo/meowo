import type { TerminalScreenFrameDto } from "../../api";
import { composerCandidate, type ComposerGuard, type ComposerSafety } from "./composerSafety";

export type ComposerConfirmationIo = {
  read: () => Promise<TerminalScreenFrameDto | null>;
};

// 没有原生键消费回执，Right 不动不能证明空框；占位与同文草稿无法从当前 DTO 区分，必须拒绝。
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
