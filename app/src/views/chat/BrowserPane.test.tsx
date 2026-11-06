import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import type { BrowserPaneInfo } from "../../api";
import { BrowserPane, browserTabTitle, localUrlsIn, occluded } from "./BrowserPane";
import { isLocalDevUrl, openInPanel, setPanelLinkHandler } from "./panelLinks";

// jsdom 没有 ResizeObserver；这里只关心下发了什么，不关心尺寸变化。
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver;

afterEach(() => {
  cleanup();
  invoke.mockClear();
  setPanelLinkHandler(null);
});

const pane = (over: Partial<BrowserPaneInfo> = {}): BrowserPaneInfo => ({ id: 5, owner: 1, url: "http://localhost:5173/", title: "", loading: false, agentActive: false, agentPaused: false, zoom: 1, ...over });

describe("面板浏览器", () => {
  it("页签名：标题优先，其次主机名，空白页叫新标签页", () => {
    expect(browserTabTitle(pane({ title: "Vite App" }), "新标签页")).toBe("Vite App");
    expect(browserTabTitle(pane(), "新标签页")).toBe("localhost:5173");
    expect(browserTabTitle(pane({ url: "about:blank" }), "新标签页")).toBe("新标签页");
  });

  it("从对话里挑本机地址：去重、最近的在前、去掉句末标点", () => {
    const texts = [
      "先跑 http://localhost:3000 看看",
      "服务起来了：http://127.0.0.1:8080/app。",
      "改完了，打开 http://localhost:3000 刷新一下；外网 https://example.com 不算",
    ];
    expect(localUrlsIn(texts)).toEqual(["http://localhost:3000", "http://127.0.0.1:8080/app"]);
  });

  it("菜单或弹窗盖到浏览器区域时算被挡住", () => {
    const root = document.createElement("div");
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    root.appendChild(menu);
    menu.getBoundingClientRect = () => new DOMRect(100, 100, 80, 60);
    expect(occluded(new DOMRect(150, 120, 300, 300), root)).toBe(true);
    expect(occluded(new DOMRect(400, 400, 300, 300), root)).toBe(false);
  });

  it("地址栏回车交给后端导航；空白页显示建议地址，点了就打开", () => {
    render(<BrowserPane info={pane({ url: "about:blank" })} visible suggestions={["http://localhost:5173"]} />);
    fireEvent.click(screen.getByRole("button", { name: "http://localhost:5173" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_navigate", { id: 5, url: "http://localhost:5173" });
    const input = screen.getByRole("textbox", { name: "搜索或输入网址" });
    fireEvent.change(input, { target: { value: "github.com" } });
    fireEvent.submit(input.closest("form")!);
    expect(invoke).toHaveBeenCalledWith("browser_pane_navigate", { id: 5, url: "github.com" });
  });

  it("agent 操作时显示提示，可以接管；接管后可以交还", () => {
    const { rerender } = render(<BrowserPane info={pane({ agentActive: true })} visible suggestions={[]} />);
    expect(screen.getByRole("status").textContent).toContain("Agent 正在操作");
    fireEvent.click(screen.getByRole("button", { name: "接管" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_set_agent_paused", { id: 5, paused: true });
    rerender(<BrowserPane info={pane({ agentPaused: true })} visible suggestions={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "交还" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_set_agent_paused", { id: 5, paused: false });
    rerender(<BrowserPane info={pane()} visible suggestions={[]} />);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("「…」菜单：显示当前缩放，能放大、打印", () => {
    render(<BrowserPane info={pane({ zoom: 1.25 })} visible suggestions={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "更多" }));
    expect(screen.getByRole("menuitem", { name: /重置缩放/ }).textContent).toContain("125%");
    fireEvent.click(screen.getByRole("menuitem", { name: "放大" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_action", { id: 5, action: "zoom_in" });
    fireEvent.click(screen.getByRole("button", { name: "更多" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /打印/ }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_action", { id: 5, action: "print" });
  });

  it("卸载时把原生视图藏起来（它不会跟着 DOM 消失）", () => {
    const { unmount } = render(<BrowserPane info={pane()} visible suggestions={[]} />);
    invoke.mockClear();
    unmount();
    expect(invoke).toHaveBeenCalledWith("browser_pane_set_bounds", expect.objectContaining({ id: 5, visible: false }));
  });
});

describe("对话里的链接", () => {
  it("只有本机地址交给面板，其余照旧走系统浏览器", () => {
    expect(isLocalDevUrl("http://localhost:5173/a")).toBe(true);
    expect(isLocalDevUrl("http://127.0.0.1:8000")).toBe(true);
    expect(isLocalDevUrl("https://example.com")).toBe(false);
    expect(isLocalDevUrl("http://tauri.localhost/")).toBe(false);
    const handler = vi.fn();
    expect(openInPanel("http://localhost:5173")).toBe(false);
    setPanelLinkHandler(handler);
    expect(openInPanel("https://example.com")).toBe(false);
    expect(openInPanel("http://localhost:5173")).toBe(true);
    expect(handler).toHaveBeenCalledWith("http://localhost:5173");
  });
});
