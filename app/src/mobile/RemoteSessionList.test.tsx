import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  openNewSessionWindow: vi.fn(),
}));
vi.mock("../api", async (importOriginal) => ({ ...(await importOriginal<typeof import("../api")>()), ...api }));
// 侧栏本体有自己的测试，这里只看列表页的导航：换成一个占位，免得拉起整套会话数据。
vi.mock("../views/ChatSidebar", () => ({ ChatSidebar: () => <div data-testid="sidebar" /> }));

import { RemoteSessionList } from "./RemoteSessionList";

/** 手机列表页的底栏：设置和新建原先是右上角齿轮与侧栏里的一行，改成底栏后三个入口都要到得了。 */
describe("RemoteSessionList 底栏", () => {
  beforeEach(() => {
    api.getSettings.mockResolvedValue({ keep_awake_while_running: true, bark_url: "", bark_paused: false, chat_turn_folding: false });
    api.openNewSessionWindow.mockResolvedValue(undefined);
  });
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  const renderList = () => render(<RemoteSessionList activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} />);

  it("默认在「会话」屏，底栏三项齐全", () => {
    renderList();
    const nav = screen.getByRole("navigation", { name: "导航" });
    expect(Array.from(nav.querySelectorAll("button")).map((b) => b.textContent)).toEqual(["会话", "新建", "设置"]);
    expect(screen.getByRole("button", { name: "会话" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("会话");
    expect(screen.getByTestId("sidebar").closest("[hidden]")).toBeNull();
  });

  it("「设置」切到设置屏：全部开关可达，会话列表藏起来但不卸载；再点「会话」切回", async () => {
    renderList();
    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("设置");
    expect(screen.getByRole("button", { name: "设置" }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("radiogroup", { name: "外观" })).toBeTruthy();
    expect(screen.getByRole("switch", { name: "消息自动折叠" })).toBeTruthy();
    expect(await screen.findByRole("switch", { name: "运行时防止休眠" })).toBeTruthy();
    expect(screen.getByTestId("sidebar").closest("[hidden]")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "会话" }));
    expect(screen.queryByRole("switch", { name: "消息自动折叠" })).toBeNull();
    expect(screen.getByTestId("sidebar").closest("[hidden]")).toBeNull();
  });

  it("「新建」走新建会话接口，不切屏", () => {
    renderList();
    fireEvent.click(screen.getByRole("button", { name: "新建" }));
    expect(api.openNewSessionWindow).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "会话" }).getAttribute("aria-current")).toBe("page");
  });
});
