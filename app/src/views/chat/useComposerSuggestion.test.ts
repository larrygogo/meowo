import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import sharedFrames from "../../test/fixtures/composer-frames.json";
import type { ComposerIo, ComposerSpec } from "./composer";
import { SUGGESTION_POLL_MS, SUGGESTION_RECHECK_MS, useComposerSuggestion } from "./useComposerSuggestion";

afterEach(() => vi.useRealTimers());

const specs = sharedFrames.specs as unknown as Record<string, ComposerSpec>;
const frameOf = (name: string) => {
  const frame = sharedFrames.frames.find((entry) => entry.name === name)!;
  return { lines: frame.lines, cursorRow: frame.cursor!.row, cursorCol: frame.cursor!.col, cursorVisible: true, cursorDim: frame.cursor!.dim, cols: 120, rows: 32 };
};
const EMPTY = frameOf("claude-2.1.325-placeholder-cursor");
const SUGGEST = frameOf("claude-2.1.330-suggestion-zh");
const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

describe("建议回复：从终端输入框读出 Claude 的预测下一句", () => {
  it("回合结束后先是空框，几秒后出现建议；读到后转低频复核；回合再跑起来时清掉", async () => {
    vi.useFakeTimers();
    let reads = 0;
    const io: ComposerIo = { frame: async () => (reads++ === 0 ? EMPTY : SUGGEST), write: async () => {} };
    const { result, rerender } = renderHook(({ idle }) => useComposerSuggestion(7, specs.claude, idle, io), { initialProps: { idle: true } });
    expect(result.current[0]).toBeNull();
    await advance(SUGGESTION_POLL_MS);
    expect(result.current[0]).toBeNull();
    await advance(SUGGESTION_POLL_MS);
    expect(result.current[0]).toBe("加个测试跑一下");
    // 读到后不再每 1.5 秒读一次，改成 5 秒复核。
    await advance(SUGGESTION_POLL_MS * 3);
    expect(reads).toBe(2);
    await advance(SUGGESTION_RECHECK_MS);
    expect(reads).toBe(3);
    rerender({ idle: false });
    expect(result.current[0]).toBeNull();
  });

  it("终端里的建议没了（用户在终端页打字、清掉了），对话页跟着收", async () => {
    vi.useFakeTimers();
    let current = SUGGEST;
    const io: ComposerIo = { frame: async () => current, write: async () => {} };
    const { result } = renderHook(() => useComposerSuggestion(7, specs.claude, true, io));
    await advance(SUGGESTION_POLL_MS);
    expect(result.current[0]).toBe("加个测试跑一下");
    current = EMPTY;
    await advance(SUGGESTION_RECHECK_MS);
    expect(result.current[0]).toBeNull();
  });

  it("发送后（dismiss）这一句不再冒出来，哪怕终端还没反应过来；下一轮跑完照常显示", async () => {
    vi.useFakeTimers();
    const io: ComposerIo = { frame: async () => SUGGEST, write: async () => {} };
    const { result, rerender } = renderHook(({ idle }) => useComposerSuggestion(7, specs.claude, idle, io), { initialProps: { idle: true } });
    await advance(SUGGESTION_POLL_MS);
    expect(result.current[0]).toBe("加个测试跑一下");
    act(() => result.current[1]());
    expect(result.current[0]).toBeNull();
    await advance(SUGGESTION_RECHECK_MS * 3);
    expect(result.current[0]).toBeNull();
    rerender({ idle: false });
    rerender({ idle: true });
    await advance(SUGGESTION_POLL_MS);
    expect(result.current[0]).toBe("加个测试跑一下");
  });

  it("不画建议的那几家不读终端", async () => {
    vi.useFakeTimers();
    const frame = vi.fn(async () => SUGGEST);
    renderHook(() => useComposerSuggestion(7, specs.codex, true, { frame, write: async () => {} }));
    await advance(SUGGESTION_POLL_MS * 3);
    expect(frame).not.toHaveBeenCalled();
  });
});
