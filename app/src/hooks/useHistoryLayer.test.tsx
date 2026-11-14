// M-2（走查 2026-11-12）：手机浮层接浏览器历史。兄弟场景：新建浮层（RemoteApp，列表页、无 meowoChat）、
// 「更改」页（ChatWindow，叠在 meowoChat 上，见 ChatWindow.remote.test.tsx）、底部面板（BottomSheet，同一 hook）。
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useHistoryLayer } from "./useHistoryLayer";

const state = () => window.history.state as { meowoLayer?: string; meowoChat?: boolean } | null;
afterEach(() => window.history.replaceState({}, "", "/"));

describe("useHistoryLayer", () => {
  it("打开压一条，后退只关浮层并保留下层的 meowoChat", async () => {
    window.history.replaceState({ meowoChat: true }, "");
    const onBack = vi.fn();
    const { rerender } = renderHook(({ open }) => useHistoryLayer(open, onBack), { initialProps: { open: true } });
    expect(state()?.meowoLayer).toBeTruthy();
    expect(state()?.meowoChat).toBe(true);
    act(() => window.history.back());
    await waitFor(() => expect(onBack).toHaveBeenCalledTimes(1));
    expect(state()?.meowoLayer).toBeUndefined();
    expect(state()?.meowoChat).toBe(true);
    // 父层随之关掉：不再多退一步。
    const length = window.history.length;
    rerender({ open: false });
    expect(window.history.length).toBe(length);
  });

  it("点 × 关掉时把自己那条退掉，不触发 onBack", async () => {
    window.history.replaceState({}, "");
    const onBack = vi.fn();
    const { rerender } = renderHook(({ open }) => useHistoryLayer(open, onBack), { initialProps: { open: true } });
    expect(state()?.meowoLayer).toBeTruthy();
    rerender({ open: false });
    await waitFor(() => expect(state()?.meowoLayer).toBeUndefined());
    expect(onBack).not.toHaveBeenCalled();
  });

  it("启动成功交给对话：浮层那条原地换成对话记录", () => {
    window.history.replaceState({}, "");
    const onBack = vi.fn();
    const { result, rerender } = renderHook(({ open }) => useHistoryLayer(open, onBack), { initialProps: { open: true } });
    const length = window.history.length;
    act(() => result.current.handOffToChat());
    expect(state()).toEqual({ meowoChat: true });
    rerender({ open: false });
    expect(window.history.length).toBe(length);
    expect(onBack).not.toHaveBeenCalled();
  });

  it("enabled=false（桌面）不碰历史", () => {
    window.history.replaceState({}, "");
    const length = window.history.length;
    renderHook(() => useHistoryLayer(true, vi.fn(), false));
    expect(window.history.length).toBe(length);
    expect(state()?.meowoLayer).toBeUndefined();
  });
});
