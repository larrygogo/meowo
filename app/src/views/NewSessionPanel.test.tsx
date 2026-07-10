import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization）。
const api = vi.hoisted(() => ({
  newSession: vi.fn(),
  recentCwds: vi.fn(),
  checkProviderHooks: vi.fn(),
  availableTerminals: vi.fn(),
  getSettings: vi.fn(),
  availableAgents: vi.fn(),
  getAccounts: vi.fn(),
  loginAgent: vi.fn(),
}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
const closeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: closeMock }) }));
// 收集 login-done 回调，测试里手动广播（模拟 Tauri emit）。其余事件（ns-prefill）照常返回 unlisten。
const ev = vi.hoisted(() => ({ loginCbs: [] as Array<(e: unknown) => void> }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "login-done") ev.loginCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireLogin = (provider: string, ok: boolean) =>
  act(() => ev.loginCbs.forEach((cb) => cb({ payload: { provider, ok } })));

import { NewSessionPanel } from "./NewSessionPanel";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  closeMock.mockReset();
  ev.loginCbs.length = 0;
  api.recentCwds.mockResolvedValue([]);
  api.checkProviderHooks.mockResolvedValue("installed");
  api.availableTerminals.mockResolvedValue(["wt"]);
  api.getSettings.mockResolvedValue({ default_agent: "claude", resume_terminal: "wt" });
  api.availableAgents.mockResolvedValue(["claude", "codex", "kimi"]);
  // 默认三家都已登录 → 不显示登录提示（各测试按需覆盖）。
  api.getAccounts.mockResolvedValue([
    { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
    { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    { provider: "kimi", account: { email: "k@b.c" }, usage: null, usage_supported: true },
  ]);
});
afterEach(() => cleanup());

describe("NewSessionPanel (独立窗口)", () => {
  it("目录为空时启动禁用", async () => {
    render(<NewSessionPanel />);
    const launch = await screen.findByTestId("ns-launch");
    expect((launch as HTMLButtonElement).disabled).toBe(true);
  });

  it("填目录后启动调 newSession → 关窗", async () => {
    api.newSession.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    await waitFor(() => expect(api.newSession).toHaveBeenCalledWith("C:/proj", "claude"));
    await waitFor(() => expect(closeMock).toHaveBeenCalled());
  });

  it("hooks 未装显示警告", async () => {
    api.checkProviderHooks.mockResolvedValue("missing");
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-hooks-warn")).toBeTruthy();
  });

  it("启动失败显示错误，不关窗", async () => {
    api.newSession.mockRejectedValue("启动终端失败");
    render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/proj" } });
    fireEvent.click(screen.getByTestId("ns-launch"));
    expect((await screen.findByTestId("ns-error")).textContent).toContain("启动终端失败");
    expect(closeMock).not.toHaveBeenCalled();
  });

  it("agent 选择只列已装的", async () => {
    api.availableAgents.mockResolvedValue(["claude", "codex"]);
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-launch");
    expect(screen.queryByTestId("ns-agent-claude")).toBeTruthy();
    expect(screen.queryByTestId("ns-agent-codex")).toBeTruthy();
    expect(screen.queryByTestId("ns-agent-kimi")).toBeNull();
  });

  it("一个都没装时提示 + 启动禁用", async () => {
    api.availableAgents.mockResolvedValue([]);
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-no-agents")).toBeTruthy();
    expect((screen.getByTestId("ns-launch") as HTMLButtonElement).disabled).toBe(true);
  });

  it("输入斜杠与最近项反斜杠方向不同时仍能高亮匹配", async () => {
    api.recentCwds.mockResolvedValue(["C:\\Users\\larry\\proj"]);
    const { container } = render(<NewSessionPanel />);
    fireEvent.change(await screen.findByTestId("ns-dir"), { target: { value: "C:/Users/larry/proj" } });
    await waitFor(() =>
      expect(container.querySelector(".ns-recent-item.is-on")?.textContent).toContain("proj")
    );
  });

  it("同一目录因斜杠方向不同重复时只保留一条", async () => {
    api.recentCwds.mockResolvedValue([
      "C:/Users/larry/proj",
      "C:\\Users\\larry\\proj",
      "C:\\Users\\larry\\other",
    ]);
    const { container } = render(<NewSessionPanel />);
    await waitFor(() =>
      expect(container.querySelectorAll(".ns-recent-item").length).toBe(2)
    );
  });
});

describe("NewSessionPanel 登录", () => {
  /** 让当前选中的 claude 处于未登录（account 为 null）。 */
  const claudeSignedOut = () =>
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: null, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
      { provider: "kimi", account: { email: "k@b.c" }, usage: null, usage_supported: true },
    ]);

  it("未登录时提示并给出登录按钮", async () => {
    claudeSignedOut();
    render(<NewSessionPanel />);
    expect(await screen.findByTestId("ns-login-warn")).toBeTruthy();
    expect(screen.getByTestId("ns-login")).toBeTruthy();
  });

  it("已登录时不提示", async () => {
    render(<NewSessionPanel />); // beforeEach 里三家都已登录
    await screen.findByTestId("ns-agent-claude");
    expect(screen.queryByTestId("ns-login-warn")).toBeNull();
  });

  it("拿不到账号时不提示（宁可不打扰，也不误报未登录）", async () => {
    api.getAccounts.mockRejectedValue(new Error("boom"));
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-agent-claude");
    expect(screen.queryByTestId("ns-login-warn")).toBeNull();
  });

  it("点登录调 loginAgent 并进入等待态", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("claude"));
    // 等 login-done 才落回，按钮禁用防重复拉起终端。
    await waitFor(() => expect((screen.getByTestId("ns-login") as HTMLButtonElement).disabled).toBe(true));
  });

  it("login-done 成功 → 提示消失", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    fireLogin("claude", true);
    await waitFor(() => expect(screen.queryByTestId("ns-login-warn")).toBeNull());
  });

  it("login-done 超时 → 落回可点 + 显示提示，提示仍在", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect((screen.getByTestId("ns-login") as HTMLButtonElement).disabled).toBe(true));
    fireLogin("claude", false);
    await waitFor(() => expect((screen.getByTestId("ns-login") as HTMLButtonElement).disabled).toBe(false));
    // 超时不等于登录失败，未登录提示仍在，错误行给出本地化说明。
    expect(screen.getByTestId("ns-login-warn")).toBeTruthy();
    expect(screen.getByTestId("ns-error").textContent?.trim().length).toBeGreaterThan(0);
  });

  it("别的 provider 的 login-done 不影响当前选中项", async () => {
    claudeSignedOut();
    api.loginAgent.mockResolvedValue(undefined);
    render(<NewSessionPanel />);
    fireEvent.click(await screen.findByTestId("ns-login"));
    await waitFor(() => expect((screen.getByTestId("ns-login") as HTMLButtonElement).disabled).toBe(true));
    fireLogin("kimi", true); // 与当前选中的 claude 无关
    expect((screen.getByTestId("ns-login") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByTestId("ns-login-warn")).toBeTruthy();
  });
});
