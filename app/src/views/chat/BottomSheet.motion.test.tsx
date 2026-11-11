// 底部面板下滑关闭的动效走 CSS（.is-settling 读 --dur-slow / --ease-out），减少动效时不做位移动画。
import { cleanup, fireEvent, render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BottomSheet } from "./BottomSheet";

const reduce = (on: boolean) => {
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: on && query.includes("reduced-motion"), media: query, addEventListener() {}, removeEventListener() {} }));
};

afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers(); });

function swipeDown(panel: Element, distance: number) {
  fireEvent.touchStart(panel, { touches: [{ clientY: 100 }] });
  fireEvent.touchMove(panel, { touches: [{ clientY: 110 }] });
  fireEvent.touchMove(panel, { touches: [{ clientY: 100 + distance }] });
  fireEvent.touchEnd(panel);
}

describe("BottomSheet 动效", () => {
  it("源码不再内联写死 transition", () => {
    const src = readFileSync(join(__dirname, "BottomSheet.tsx"), "utf8");
    expect(src).not.toMatch(/style\.transition\s*=/);
  });

  it("正常动效：滑过阈值后挂 is-settling 滑出，稍后关闭", () => {
    reduce(false);
    vi.useFakeTimers();
    const onClose = vi.fn();
    render(<BottomSheet title="t" onClose={onClose}><div /></BottomSheet>);
    const panel = document.querySelector(".chat-sheet-panel")!;
    swipeDown(panel, 200);
    expect(panel.classList.contains("is-settling")).toBe(true);
    expect((panel as HTMLElement).style.transition).toBe("");
    expect(onClose).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("减少动效：滑过阈值直接关闭，不做滑出位移", () => {
    reduce(true);
    const onClose = vi.fn();
    render(<BottomSheet title="t" onClose={onClose}><div /></BottomSheet>);
    const panel = document.querySelector(".chat-sheet-panel")!;
    swipeDown(panel, 200);
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(panel.classList.contains("is-settling")).toBe(false);
  });

  it("减少动效：没过阈值时直接归位，不挂过渡", () => {
    reduce(true);
    render(<BottomSheet title="t" onClose={() => {}}><div /></BottomSheet>);
    const panel = document.querySelector(".chat-sheet-panel") as HTMLElement;
    // 慢慢拖一点点（jsdom 的 timeStamp 不走，速度按 16ms 一帧算，每步只挪 1～7px 不算甩）。
    fireEvent.touchStart(panel, { touches: [{ clientY: 100 }] });
    fireEvent.touchMove(panel, { touches: [{ clientY: 107 }] });
    fireEvent.touchMove(panel, { touches: [{ clientY: 108 }] });
    expect(panel.style.transform).toBe("translateY(8px)");
    fireEvent.touchEnd(panel);
    expect(panel.classList.contains("is-settling")).toBe(false);
    expect(panel.style.transform).toBe("");
  });

  it("CSS：is-settling 用动效 token，减少动效时关掉过渡", () => {
    const css = readFileSync(join(__dirname, "../../styles.css"), "utf8");
    expect(css).toMatch(/\.chat-sheet-panel\.is-settling \{ transition: transform var\(--dur-slow\) var\(--ease-out\)/);
    expect(css).toMatch(/prefers-reduced-motion: reduce\)[^\n]*\.chat-sheet-panel\.is-settling \{ transition: none/);
  });
});
