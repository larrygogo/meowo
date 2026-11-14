import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useSessionRef, useSessionScope, useSessionState, useSessionTimer } from "./sessionScope";

function useFixture() {
  const scope = useSessionScope();
  const [picking, setPicking] = useSessionState(scope, false);
  const pending = useSessionRef<string | null>(scope, null);
  const timer = useSessionTimer(scope);
  return { scope, picking, setPicking, pending, timer };
}

describe("会话级状态：换会话时自动归零", () => {
  beforeEach(() => { vi.useFakeTimers(); });
  afterEach(() => { vi.useRealTimers(); });

  it("状态与 ref 回到初始值，没到点的计时器取消", () => {
    const fired = vi.fn();
    const { result } = renderHook(() => useFixture());
    act(() => {
      result.current.setPicking(true);
      result.current.pending.current = "model-x";
      result.current.timer.start(fired, 12_000);
    });
    expect(result.current.picking).toBe(true);
    act(() => result.current.scope.reset());
    expect(result.current.picking).toBe(false);
    expect(result.current.pending.current).toBeNull();
    act(() => { vi.advanceTimersByTime(20_000); });
    expect(fired).not.toHaveBeenCalled();
  });

  it("不换会话时计时器照常到点；重新计时作废上一次", () => {
    const first = vi.fn();
    const second = vi.fn();
    const { result } = renderHook(() => useFixture());
    act(() => { result.current.timer.start(first, 1_000); result.current.timer.start(second, 1_000); });
    act(() => { vi.advanceTimersByTime(1_000); });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
