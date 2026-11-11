import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// UI 调研 m10：手机新建会话碰到「尚未接入 / 没装 CLI / 没登录」一律写「请在桌面端处理后再启动」。
// 修复接入项是 Meowo 自己能做的写配置动作，已进远程白名单（remote.rs 模块头「例外」2026-11-11），
// 手机上直接给「修复」；登录、安装要在宿主拉起终端/浏览器，手机做不了，说明原因而不是叫人去桌面端。
const api = vi.hoisted(() => ({
  recentCwds: vi.fn(), checkProviderHooks: vi.fn(), availableTerminals: vi.fn(), getSettings: vi.fn(),
  listAgents: vi.fn(), getAccounts: vi.fn(), repairProviderHooks: vi.fn(), listSubdirectories: vi.fn(),
}));
vi.mock("../api", async (orig) => ({ ...(await orig<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: vi.fn() }) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}), emit: vi.fn(() => Promise.resolve()) }));

import { NewSessionPanel } from "./NewSessionPanel";
import { descriptors } from "../test/agents";
import { zh } from "../i18n/zh";

beforeEach(() => {
  (globalThis as Record<string, unknown>).__MEOWO_REMOTE__ = true;
  Object.values(api).forEach((m) => m.mockReset());
  api.recentCwds.mockResolvedValue(["C:/repo"]);
  api.availableTerminals.mockResolvedValue([]);
  api.getSettings.mockResolvedValue({ default_agent: "claude" });
  api.listAgents.mockResolvedValue(descriptors(["claude"]));
  api.getAccounts.mockResolvedValue([{ provider: "claude", account: { email: "a@b.c" }, usage: null, usage_supported: true }]);
  api.listSubdirectories.mockResolvedValue({ path: null, entries: [] });
});
afterEach(() => { cleanup(); delete (globalThis as Record<string, unknown>).__MEOWO_REMOTE__; });

describe("手机新建会话的阻塞提示", () => {
  it("尚未接入时手机上也能直接修复，不写去桌面端", async () => {
    api.checkProviderHooks.mockResolvedValue("missing");
    api.repairProviderHooks.mockResolvedValue({ status: "installed", reason: null });
    render(<NewSessionPanel />);
    const repair = await screen.findByTestId("ns-repair-hooks");
    expect(document.body.textContent).not.toMatch(/桌面端/);
    fireEvent.click(repair);
    await waitFor(() => expect(api.repairProviderHooks).toHaveBeenCalledWith("claude"));
    await waitFor(() => expect(screen.queryByTestId("ns-hooks-warn")).toBeNull());
  });

  it("没登录、没装 CLI 时说明为什么手机上做不了，不写去桌面端", async () => {
    api.checkProviderHooks.mockResolvedValue("installed");
    api.getAccounts.mockResolvedValue([{ provider: "claude", account: null, usage: null, usage_supported: true }]);
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-login-warn");
    expect(screen.getByText(zh.newSession.loginOnHost)).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/桌面端/);
    cleanup();
    api.listAgents.mockResolvedValue(descriptors([]));
    render(<NewSessionPanel />);
    await screen.findByTestId("ns-no-agents");
    expect(screen.getByText(zh.newSession.installOnHost)).toBeTruthy();
    expect(document.body.textContent).not.toMatch(/桌面端/);
  });
});
