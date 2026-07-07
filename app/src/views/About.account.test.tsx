import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor, act } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization，与 NewSessionPanel.test.tsx 同坑）。
const api = vi.hoisted(() => ({ getAccounts: vi.fn(), availableAgents: vi.fn(), installAgent: vi.fn(), refreshUsage: vi.fn(), getSettings: vi.fn(), setSettings: vi.fn() }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));

// 收集所有 ProviderCard 注册的事件回调，测试里手动广播（模拟 Tauri emit 到全部监听者）
const ev = vi.hoisted(() => ({
  progressCbs: [] as Array<(e: unknown) => void>,
  doneCbs: [] as Array<(e: unknown) => void>,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: unknown) => void) => {
    if (event === "install-progress") ev.progressCbs.push(cb);
    if (event === "install-done") ev.doneCbs.push(cb);
    return Promise.resolve(() => {});
  },
}));
const fireProgress = (provider: string, line: string) =>
  act(() => ev.progressCbs.forEach((cb) => cb({ payload: { provider, line } })));
const fireDone = (provider: string, ok: boolean) =>
  act(() => ev.doneCbs.forEach((cb) => cb({ payload: { provider, ok, code: ok ? 0 : 1 } })));

import { AccountSection } from "./About";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.availableAgents.mockResolvedValue(["claude", "codex"]);
  api.refreshUsage.mockResolvedValue({ lanes: [], note: null });
  api.getSettings.mockResolvedValue({ sticker_quota_providers: [] });
  ev.progressCbs.length = 0;
  ev.doneCbs.length = 0;
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

  it("点安装进入安装中：转圈 + 最新步骤行", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    fireProgress("kimi", "==> Downloading Kimi Code");
    expect(screen.getByTestId("agent-installing-kimi").textContent).toContain("Downloading Kimi Code");
    // 只更新本 provider：codex 的进度不影响 kimi
    fireProgress("codex", "==> other");
    expect(screen.getByTestId("agent-installing-kimi").textContent).toContain("Downloading Kimi Code");
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

  it("install-done 失败且无进度行：仍显示失败说明 + 重试按钮", async () => {
    api.installAgent.mockResolvedValue(undefined);
    render(<AccountSection />);
    fireEvent.click(await screen.findByTestId("agent-install-kimi"));
    await waitFor(() => expect(screen.getByTestId("agent-installing-kimi")).toBeTruthy());
    // 不先 fireProgress，直接失败——step 为空，走 installFailed 兜底
    fireDone("kimi", false);
    await waitFor(() => expect(screen.queryByTestId("agent-installing-kimi")).toBeNull());
    // 失败说明行可见且非空（不硬编码 i18n 文案，避免 locale 依赖）
    const errLine = screen.getByTestId("agent-install-error-kimi");
    expect(errLine.textContent?.trim().length).toBeGreaterThan(0);
    // 重试按钮仍在（testid 与安装共用）
    expect(screen.getByTestId("agent-install-kimi")).toBeTruthy();
  });
});
