import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QUIET_TURN_MS, useQuietTurn } from "./useQuietTurn";

// 兄弟场景：一直无内容→提示；中途有内容→重新计时；回合结束→立即收起；从未运行→不提示。
describe("useQuietTurn", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("运行中持续没有新内容，到时限才提示", () => {
    const { result } = renderHook(() => useQuietTurn(true, "a"));
    act(() => { vi.advanceTimersByTime(QUIET_TURN_MS - 1); });
    expect(result.current).toBe(false);
    act(() => { vi.advanceTimersByTime(1); });
    expect(result.current).toBe(true);
  });

  it("有新内容就收起并重新计时", () => {
    const { result, rerender } = renderHook(({ activity }) => useQuietTurn(true, activity), { initialProps: { activity: "a" } });
    act(() => { vi.advanceTimersByTime(QUIET_TURN_MS); });
    expect(result.current).toBe(true);
    rerender({ activity: "b" });
    expect(result.current).toBe(false);
    act(() => { vi.advanceTimersByTime(QUIET_TURN_MS - 1); });
    expect(result.current).toBe(false);
  });

  it("回合结束立即收起，未运行时从不提示", () => {
    const { result, rerender } = renderHook(({ running }) => useQuietTurn(running, "a"), { initialProps: { running: true } });
    act(() => { vi.advanceTimersByTime(QUIET_TURN_MS); });
    expect(result.current).toBe(true);
    rerender({ running: false });
    expect(result.current).toBe(false);
    act(() => { vi.advanceTimersByTime(QUIET_TURN_MS * 3); });
    expect(result.current).toBe(false);
  });
});
