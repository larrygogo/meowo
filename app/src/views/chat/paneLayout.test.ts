// 三栏宽度互相扣除、窗口缩小重新钳制、双击复位（验收 #6 #25 #26，调研 c4/s5/s14/c6/c7）。
import { act, renderHook } from "@testing-library/react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { CHAT_MIN_WIDTH, fitPanes, panelDefaultWidth, sidebarDefaultWidth, sidebarMaxWidth } from "./paneLayout";
import { useDiffPanelWidth } from "./useDiffPanelWidth";
import { useSidebarLayout } from "./useSidebarLayout";

afterEach(() => {
  localStorage.clear();
  document.documentElement.style.removeProperty("--cc-ui");
});

/** 假 .chat-body：宽 body，里面常驻侧栏 / 面板各占多宽（0 = 不在）。 */
function fakeBody(body: number, kids: { sidebar?: number; panel?: number }) {
  const rect = (width: number) => ({ getBoundingClientRect: () => ({ width }) });
  return {
    getBoundingClientRect: () => ({ width: body }),
    querySelector: (sel: string) => (sel.includes("chat-sidebar") ? (kids.sidebar ? rect(kids.sidebar) : null) : kids.panel ? rect(kids.panel) : null),
    closest: () => null,
  } as unknown as Element;
}
const key = (name: string, body: Element, via: "parent" | "closest") => ({
  key: name,
  preventDefault: () => {},
  currentTarget: via === "parent" ? { parentElement: body } : { closest: () => body },
}) as unknown as ReactKeyboardEvent<HTMLDivElement>;

const chatLeft = (body: number, p: { sidebar: number | null; panel: number | null }) => body - (p.sidebar ?? 0) - (p.panel ?? 0);

describe("三栏宽度钳制", () => {
  it("存储值过大：侧栏 900、面板 1200 在 1440 宽下也给对话区留 ≥320，侧栏不超绝对上限", () => {
    const fit = fitPanes({ body: 1440, sidebar: 900, panel: 1200 });
    expect(fit.sidebar).toBeLessThanOrEqual(sidebarMaxWidth(1));
    expect(chatLeft(1440, fit)).toBeGreaterThanOrEqual(CHAT_MIN_WIDTH);
  });

  it("窗口从 1440 缩到 1000：拖过的侧栏 560 + 面板 560 重新钳制，对话区仍 ≥320", () => {
    const wide = fitPanes({ body: 1440, sidebar: 560, panel: 560 });
    expect(chatLeft(1440, wide)).toBeGreaterThanOrEqual(CHAT_MIN_WIDTH);
    const narrow = fitPanes({ body: 1000, sidebar: 560, panel: 560 });
    expect(chatLeft(1000, narrow)).toBeGreaterThanOrEqual(CHAT_MIN_WIDTH);
    expect(narrow.panel).toBeGreaterThanOrEqual(320);
    // 只有侧栏（面板收着）时同样回收
    const sidebarOnly = fitPanes({ body: 1000, sidebar: 900, panel: null });
    expect(chatLeft(1000, sidebarOnly)).toBeGreaterThanOrEqual(CHAT_MIN_WIDTH);
  });

  it("最小窗口 620 开着面板：面板让到 300，对话区保 320", () => {
    const fit = fitPanes({ body: 620, sidebar: null, panel: 560 });
    expect(fit.panel).toBe(300);
  });

  it("面板拖到底（键盘一路 ←）：上限扣掉常驻侧栏，1440 宽侧栏 240 时停在 880", () => {
    localStorage.setItem("meowo-diff-panel-width", "600");
    const { result } = renderHook(() => useDiffPanelWidth());
    const body = fakeBody(1440, { sidebar: 240 });
    for (let i = 0; i < 60; i += 1) act(() => result.current.onKeyDown(key("ArrowLeft", body, "parent")));
    expect(result.current.width).toBe(1440 - 240 - CHAT_MIN_WIDTH);
  });

  it("侧栏拖到底（键盘一路 →）：上限扣掉右侧面板，且不超绝对上限", () => {
    const { result } = renderHook(() => useSidebarLayout());
    const withPanel = fakeBody(1440, { panel: 700 });
    for (let i = 0; i < 60; i += 1) act(() => result.current.nudgeSidebarWidth(key("ArrowRight", withPanel, "closest")));
    expect(result.current.sidebarWidth).toBe(1440 - 700 - CHAT_MIN_WIDTH);
    const noPanel = fakeBody(2560, {});
    for (let i = 0; i < 60; i += 1) act(() => result.current.nudgeSidebarWidth(key("ArrowRight", noPanel, "closest")));
    expect(result.current.sidebarWidth).toBe(sidebarMaxWidth(1));
  });

  it("双击复位：侧栏回到默认宽并清掉存储，面板回到默认宽", () => {
    localStorage.setItem("meowo-chat-sidebar-width", "400");
    localStorage.setItem("meowo-diff-panel-width", "900");
    const sidebar = renderHook(() => useSidebarLayout());
    expect(sidebar.result.current.sidebarWidth).toBe(400);
    act(() => sidebar.result.current.resetSidebarWidth());
    expect(sidebar.result.current.sidebarWidth).toBe(sidebarDefaultWidth(1));
    expect(localStorage.getItem("meowo-chat-sidebar-width")).toBeNull();
    const panel = renderHook(() => useDiffPanelWidth());
    act(() => panel.result.current.reset());
    expect(panel.result.current.width).toBe(panelDefaultWidth(window.innerWidth));
    expect(localStorage.getItem("meowo-diff-panel-width")).toBeNull();
  });

  it("侧栏默认宽乘界面缩放，拖过的宽度不跟着变", () => {
    document.documentElement.style.setProperty("--cc-ui", "1.5");
    expect(renderHook(() => useSidebarLayout()).result.current.sidebarWidth).toBe(360);
    localStorage.setItem("meowo-chat-sidebar-width", "260");
    expect(renderHook(() => useSidebarLayout()).result.current.sidebarWidth).toBe(260);
  });

  it("面板默认宽封顶 560：2560 宽不再默认 1140", () => {
    expect(panelDefaultWidth(2560)).toBe(560);
    expect(panelDefaultWidth(1000)).toBe(360);
    expect(panelDefaultWidth(620)).toBe(320);
  });
});
