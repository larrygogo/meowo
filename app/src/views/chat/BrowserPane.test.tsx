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

const pane = (over: Partial<BrowserPaneInfo> = {}): BrowserPaneInfo => ({ id: 5, owner: 1, url: "http://localhost:5173/", title: "", loading: false, agentActive: false, agentPaused: false, zoom: 1, downloads: [], permission: null, blocked: null, ...over });

describe("面板浏览器", () => {
  // 三条提示条的文字要同一条左缘：权限条、拦截条原先没有前置图标，文字比下载条靠左 22px。
  it("权限条与拦截条都带前置图标，和下载条同一格", () => {
    const { container, rerender } = render(<BrowserPane info={pane({ permission: { id: 1, kind: "camera", host: "a.test" } })} visible suggestions={[]} />);
    expect(container.querySelector(".browser-pane-notice.is-permission > svg.browser-pane-notice-icon")).toBeTruthy();
    rerender(<BrowserPane info={pane({ blocked: { id: 2, kind: "camera", host: "a.test" } })} visible suggestions={[]} />);
    expect(container.querySelector(".browser-pane-notice.is-blocked > svg.browser-pane-notice-icon")).toBeTruthy();
  });

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

  it("网页的权限请求在面板里答：允许 / 阻止，答复送达前按钮锁住防连点", () => {
    const ask = { id: 1, kind: "downloads", host: "httpbin.org" };
    const { rerender } = render(<BrowserPane info={pane({ permission: ask })} visible suggestions={[]} />);
    expect(screen.getByRole("alert").textContent).toContain("httpbin.org 想要下载多个文件");
    fireEvent.click(screen.getByRole("button", { name: "允许" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_answer_permission", { id: 5, permission: 1, allow: true });
    // 后端还没推新状态前再点无效：后端每次答的是排在最前的一条，连点会误答下一条。
    expect((screen.getByRole("button", { name: "阻止" }) as HTMLButtonElement).disabled).toBe(true);
    // 无关的状态推送（标题、加载状态变了）同一条请求仍锁着（做空审查：按对象引用会提前解锁）。
    rerender(<BrowserPane info={pane({ permission: { ...ask }, title: "x", loading: true })} visible suggestions={[]} />);
    expect((screen.getByRole("button", { name: "阻止" }) as HTMLButtonElement).disabled).toBe(true);
    // 下一条请求（编号不同，哪怕内容相同）推来就解锁。
    rerender(<BrowserPane info={pane({ permission: { ...ask, id: 2 } })} visible suggestions={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "阻止" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_answer_permission", { id: 5, permission: 2, allow: false });
    // 不认识的种类给通用说法；答完就没有提示了。
    rerender(<BrowserPane info={pane({ permission: { id: 3, kind: "window_management", host: "a.test" } })} visible suggestions={[]} />);
    expect(screen.getByRole("alert").textContent).toContain("a.test 想要使用额外权限");
    rerender(<BrowserPane info={pane()} visible suggestions={[]} />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("阻止之后留一条「已阻止」：可刷新重试，可收起；有新的请求时让位", () => {
    const blocked = { id: 7, kind: "downloads", host: "httpbin.org" };
    const { rerender } = render(<BrowserPane info={pane({ blocked })} visible suggestions={[]} />);
    expect(screen.getByRole("status").textContent).toContain("已阻止 httpbin.org 下载多个文件");
    fireEvent.click(screen.getByRole("button", { name: "刷新重试" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_action", { id: 5, action: "reload" });
    // 新请求在等答复时只显示请求本身。
    rerender(<BrowserPane info={pane({ blocked, permission: { id: 8, kind: "camera", host: "a.test" } })} visible suggestions={[]} />);
    expect(screen.queryByText(/已阻止/)).toBeNull();
    rerender(<BrowserPane info={pane({ blocked })} visible suggestions={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByText(/已阻止/)).toBeNull();
    // 再阻止一条（编号不同）照常出来。
    rerender(<BrowserPane info={pane({ blocked: { ...blocked, id: 9 } })} visible suggestions={[]} />);
    expect(screen.getByText(/已阻止/)).not.toBeNull();
  });

  it("下载提示：下载中、已下载（打开文件夹）、失败；收起后下一次下载照常出来", () => {
    const item = (id: number, state: "progress" | "done" | "failed") => ({ id, fileName: "report.csv", state });
    const { rerender } = render(<BrowserPane info={pane({ downloads: [item(1, "progress")] })} visible suggestions={[]} />);
    expect(screen.getByRole("status").textContent).toContain("正在下载 report.csv");
    expect(screen.queryByRole("button", { name: "打开文件夹" })).toBeNull();

    rerender(<BrowserPane info={pane({ downloads: [item(1, "done")] })} visible suggestions={[]} />);
    expect(screen.getByRole("status").textContent).toContain("已下载 report.csv");
    fireEvent.click(screen.getByRole("button", { name: "打开文件夹" }));
    expect(invoke).toHaveBeenCalledWith("browser_pane_reveal_download", { id: 5, download: 1 });

    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(screen.queryByRole("status")).toBeNull();

    // 只显示最新一条。
    rerender(<BrowserPane info={pane({ downloads: [item(1, "done"), item(2, "failed")] })} visible suggestions={[]} />);
    expect(screen.getByRole("status").textContent).toContain("下载失败 report.csv");
    expect(screen.queryByRole("button", { name: "打开文件夹" })).toBeNull();
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
