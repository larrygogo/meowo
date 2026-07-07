import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";

// vi.mock 会被提升到文件顶部，工厂函数里引用的外部变量必须走 vi.hoisted
// （否则 TDZ：ReferenceError: Cannot access 'api' before initialization，与 NewSessionPanel.test.tsx 同坑）。
const api = vi.hoisted(() => ({ getAccounts: vi.fn(), availableAgents: vi.fn(), installAgent: vi.fn(), refreshUsage: vi.fn(), getSettings: vi.fn(), setSettings: vi.fn() }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));

import { AccountSection } from "./About";

beforeEach(() => {
  Object.values(api).forEach((m) => m.mockReset());
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.availableAgents.mockResolvedValue(["claude", "codex"]);
  api.refreshUsage.mockResolvedValue({ lanes: [], note: null });
  api.getSettings.mockResolvedValue({ sticker_quota_providers: [] });
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
});
