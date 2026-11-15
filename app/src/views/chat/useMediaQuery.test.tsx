import { afterEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useMediaQuery } from "./useMediaQuery";

afterEach(() => { vi.unstubAllGlobals(); });

describe("useMediaQuery", () => {
  it("没有 matchMedia 的环境按不命中", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(renderHook(() => useMediaQuery("(min-width: 1100px)")).result.current).toBe(false);
  });

  it("命中与否跟着媒体查询的变化更新，卸载时摘掉监听", () => {
    let listener: (() => void) | null = null;
    const list = {
      matches: false,
      addEventListener: (_: string, fn: () => void) => { listener = fn; },
      removeEventListener: vi.fn(),
    };
    vi.stubGlobal("matchMedia", () => list);
    const view = renderHook(() => useMediaQuery("(min-width: 1100px)"));
    expect(view.result.current).toBe(false);
    list.matches = true;
    act(() => listener?.());
    expect(view.result.current).toBe(true);
    view.unmount();
    expect(list.removeEventListener).toHaveBeenCalledWith("change", listener);
  });
});
