import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { TooltipLayer } from "./Tooltip";

afterEach(cleanup);

describe("TooltipLayer", () => {
  it("悬停带 data-tip 的元素，延迟后弹出提示，移出后消失", () => {
    vi.useFakeTimers();
    try {
      render(
        <>
          <button data-tip="跳转到终端">btn</button>
          <TooltipLayer />
        </>,
      );
      const btn = screen.getByText("btn");
      fireEvent.mouseOver(btn);
      // 延迟未到不弹
      act(() => void vi.advanceTimersByTime(100));
      expect(screen.queryByRole("tooltip")).toBeNull();
      // 过了 SHOW_DELAY 弹出，文案取自 data-tip
      act(() => void vi.advanceTimersByTime(300));
      expect(screen.getByRole("tooltip").textContent).toBe("跳转到终端");
      // 移出元素即消失
      fireEvent.mouseOut(btn, { relatedTarget: document.body });
      expect(screen.queryByRole("tooltip")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("无 data-tip 的元素不弹提示", () => {
    vi.useFakeTimers();
    try {
      render(
        <>
          <button>plain</button>
          <TooltipLayer />
        </>,
      );
      fireEvent.mouseOver(screen.getByText("plain"));
      act(() => void vi.advanceTimersByTime(500));
      expect(screen.queryByRole("tooltip")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
