import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import sharedFrames from "../../test/fixtures/composer-frames.json";
import type { ComposerIo, ComposerSpec } from "./composer";
import { SUGGESTION_POLL_MS, useComposerSuggestion } from "./useComposerSuggestion";

afterEach(() => vi.useRealTimers());

const specs = sharedFrames.specs as unknown as Record<string, ComposerSpec>;
const frameOf = (name: string) => {
  const frame = sharedFrames.frames.find((entry) => entry.name === name)!;
  return { lines: frame.lines, cursorRow: frame.cursor!.row, cursorCol: frame.cursor!.col, cursorVisible: true, cursorDim: frame.cursor!.dim, cols: 120, rows: 32 };
};

describe("建议回复：从终端输入框读出 Claude 的预测下一句", () => {
  it("回合结束后先是空框，几秒后出现建议：读到就停，回合再跑起来时清掉", async () => {
    vi.useFakeTimers();
    const frames = [frameOf("claude-2.1.325-placeholder-cursor"), frameOf("claude-2.1.330-suggestion-zh")];
    let reads = 0;
    const io: ComposerIo = { frame: async () => frames[Math.min(reads++, frames.length - 1)], write: async () => {} };
    const { result, rerender } = renderHook(({ idle }) => useComposerSuggestion(7, specs.claude, idle, io), { initialProps: { idle: true } });
    expect(result.current).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(SUGGESTION_POLL_MS); });
    expect(result.current).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(SUGGESTION_POLL_MS); });
    expect(result.current).toBe("加个测试跑一下");
    await act(async () => { await vi.advanceTimersByTimeAsync(SUGGESTION_POLL_MS * 5); });
    expect(reads).toBe(2);
    rerender({ idle: false });
    expect(result.current).toBeNull();
  });

  it("不画建议的那几家不读终端", async () => {
    vi.useFakeTimers();
    const frame = vi.fn(async () => frameOf("claude-2.1.330-suggestion-zh"));
    renderHook(() => useComposerSuggestion(7, specs.codex, true, { frame, write: async () => {} }));
    await act(async () => { await vi.advanceTimersByTimeAsync(SUGGESTION_POLL_MS * 3); });
    expect(frame).not.toHaveBeenCalled();
  });
});
