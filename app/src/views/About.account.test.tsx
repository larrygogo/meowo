import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization，与 NewSessionPanel.test.tsx 同坑）。
const api = vi.hoisted(() => ({ getAccounts: vi.fn(), availableAgents: vi.fn(), installAgent: vi.fn(), loginAgent: vi.fn(), refreshUsage: vi.fn(), getSettings: vi.fn(), setSettings: vi.fn() }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));

// 收集所有 ProviderCard 注册的 install-done / login-done 回调，测试里手动广播
// （模拟 Tauri emit 到全部监听者）。进度不透传英文、前端不订阅 install-progress，故只收集 done。
const ev = vi.hoisted(() => ({
  doneCbs: [] as Array<(e: unknown) => void>,
  loginCbs: [] as Array<(e: unknown) => void>,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "install-done") ev.doneCbs.push(cb);
    if (event === "login-done") ev.loginCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireDone = (provider: string, ok: boolean) =>
  act(() => ev.doneCbs.forEach((cb) => cb({ payload: { provider, ok, code: ok ? 0 : 1 } })));
const fireLogin = (provider: string, ok: boolean) =>
  act(() => ev.loginCbs.forEach((cb) => cb({ payload: { provider, ok } })));

import { AccountSection } from "./About";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.availableAgents.mockResolvedValue(["claude", "codex"]);
  api.refreshUsage.mockResolvedValue({ lanes: [], note: null });
  api.getSettings.mockResolvedValue({ sticker_quota_providers: [] });
  ev.doneCbs.length = 0;
  ev.loginCbs.length = 0;
});
afterEach(() => cleanup());

describe("AccountSection agent 卡", () => {
  it("三个 agent 都渲染，未装的标未安装 + 安装按钮", async () => {
    render(<AccountSection />);
    await waitFor(() => expect(screen.getByTestId("agent-card-kimi")).toBeTruthy());
    expect(screen.getByTestId("agent-card-claude")).toBeTruthy();
    expect(screen.getByTestId("agent-card-codex")).toBeTruthy();
    // kimi 未装：availableAgents() resolve 后才出现安装按钮（首帧检测中不渲染，findByTestId 等待）
    expect(await screen.findByTestId("agent-install-kimi")).toBeTruthy();
    // 已装的（claude/codex）无安装按钮
    expect(screen.queryByTestId("agent-install-claude")).toBeNull();
  });

  it("点安装调 installAgent", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(api.installAgent).toHaveBeenCalledWith("kimi"));
  });

  it("点安装进入安装中：转圈 + 本地化「安装中…」（不透传脚本英文）", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    // 安装中指示出现，且有非空本地化文案（不硬编码 i18n 串，避免 locale 依赖）
    const installing = await screen.findByTestId("agent-installing-kimi");
    expect(installing.textContent?.trim().length).toBeGreaterThan(0);
    // 安装按钮已被安装中指示替换
    expect(screen.queryByTestId("agent-install-kimi")).toBeNull();
  });

  it("install-done 成功后重查检测、卡片转已装", async () => {
    api.installAgent.mockResolvedValue(undefined);
    // 初次未装 kimi；装完重查返回含 kimi
    api.availableAgents.mockResolvedValueOnce(["claude", "codex"]).mockResolvedValue(["claude", "codex", "kimi"]);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", true);
    await waitFor(() => expect(screen.queryByTestId("agent-install-kimi")).toBeNull());
    expect(screen.queryByTestId("agent-installing-kimi")).toBeNull();
  });

  it("install-done 失败：退出安装中、显示重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 仍未装 → 按钮回来（文案为重试），testid 不变
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });

  it("install-done 失败：显示本地化失败说明 + 重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    // 失败——错误行始终显示本地化 installFailed 文案
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 失败说明行可见且非空（不硬编码 i18n 文案，避免 locale 依赖）
    const errLine = screen.getByTestId("agent-install-error-kimi");
    expect(errLine.textContent?.trim().length).toBeGreaterThan(0);
    // 重试按钮仍在（testid 与安装共用）
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });
});

describe("AccountSection 登录", () => {
  it("已装未登录才显示登录按钮；已登录/未装都不显示", async () => {
    render(<AccountSection />);
    // codex 已装（availableAgents）但 getAccounts 没返回它 → account 为 null → 未登录。
    expect(await screen.findByTestId("agent-login-codex")).toBeTruthy();
    // claude 已装且有账号 → 已登录，无登录按钮。
    expect(screen.queryByTestId("agent-login-claude")).toBeNull();
    // kimi 未装 → 该先安装，不给登录按钮。
    expect(screen.queryByTestId("agent-login-kimi")).toBeNull();
  });

  it("点登录调 loginAgent 并进入等待态", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    const btn = await screen.findByTestId("agent-login-codex");
    fireEvent.click(btn);
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalledWith("codex"));
    // spawn 成功后不落回 idle——等 login-done，按钮禁用防重复拉起终端。
    await waitFor(() => expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(true));
  });

  it("login-done 成功 → 重查账号（卡片可转已登录）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(api.loginAgent).toHaveBeenCalled());
    const before = api.getAccounts.mock.calls.length;
    // 登录成功后 codex 也有账号了
    api.getAccounts.mockResolvedValue([
      { provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true },
      { provider: "codex", account: { login_label: "API Key" }, usage: null, usage_supported: false },
    ]);
    fireLogin("codex", true);
    await waitFor(() => expect(api.getAccounts.mock.calls.length).toBeGreaterThan(before));
    await waitFor(() => expect(screen.queryByTestId("agent-login-codex")).toBeNull());
  });

  it("login-done 超时 → 落回可点 + 显示本地化提示（超时不是登录失败）", async () => {
    api.loginAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(true));
    fireLogin("codex", false);
    await waitFor(() => expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(false));
    const msg = screen.getByTestId("agent-login-error-codex");
    expect(msg.textContent?.trim().length).toBeGreaterThan(0);
  });

  it("拉起登录失败 → 落回可点 + 显示提示", async () => {
    api.loginAgent.mockRejectedValue(new Error("启动终端失败"));
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-login-codex"));
    await waitFor(() => expect(screen.getByTestId("agent-login-error-codex")).toBeTruthy());
    expect((screen.getByTestId("agent-login-codex") as HTMLButtonElement).disabled).toBe(false);
  });

  it("装完自动引导：install-done 成功后，该 agent 的登录按钮被标为主要动作", async () => {
    api.installAgent.mockResolvedValue(undefined);
    // 初次未装 kimi；装完重查返回含 kimi（此时 kimi 无账号 → 未登录）
    api.availableAgents.mockResolvedValueOnce(["claude", "codex"]).mockResolvedValue(["claude", "codex", "kimi"]);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    fireDone("kimi", true);
    const loginBtn = await screen.findByTestId("agent-login-kimi");
    // 「装完 → 登录」串成一条链路：按钮升为 primary，而非埋在一堆次要按钮里。
    expect(loginBtn.className).toContain("provider-card-action-primary");
    // 未经安装流程的 codex 则是普通次要按钮。
    expect(screen.getByTestId("agent-login-codex").className).not.toContain("provider-card-action-primary");
  });
});
