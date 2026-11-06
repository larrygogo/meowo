import { act, renderHook } from "@testing-library/react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { DIFF_PANEL_MIN_WIDTH, useDiffPanelWidth } from "./useDiffPanelWidth";

afterEach(() => localStorage.clear());

const key = (name: string) => ({
  key: name,
  preventDefault: () => {},
  currentTarget: { parentElement: null },
}) as unknown as ReactKeyboardEvent<HTMLDivElement>;

// 没调过宽度时默认占可用宽度的一半（窗口宽减左侧栏），不低于下限（照 Kimi Code）。
const fallback = () => Math.max(DIFF_PANEL_MIN_WIDTH, Math.round((window.innerWidth - 280) / 2));

describe("改动面板宽度", () => {
  it("读回上次存的宽度，坏值或过窄退回默认", () => {
    localStorage.setItem("meowo-diff-panel-width", "520");
    expect(renderHook(() => useDiffPanelWidth()).result.current.width).toBe(520);
    localStorage.setItem("meowo-diff-panel-width", "100");
    expect(renderHook(() => useDiffPanelWidth()).result.current.width).toBe(fallback());
  });

  it("← 变宽、→ 变窄，每步 24px 并持久化，不低于下限", () => {
    const { result } = renderHook(() => useDiffPanelWidth());
    act(() => result.current.onKeyDown(key("ArrowLeft")));
    expect(result.current.width).toBe(fallback() + 24);
    expect(localStorage.getItem("meowo-diff-panel-width")).toBe(String(fallback() + 24));
    for (let i = 0; i < 20; i += 1) act(() => result.current.onKeyDown(key("ArrowRight")));
    expect(result.current.width).toBe(DIFF_PANEL_MIN_WIDTH);
    act(() => result.current.onKeyDown(key("Enter")));
    expect(result.current.width).toBe(DIFF_PANEL_MIN_WIDTH);
  });
});
