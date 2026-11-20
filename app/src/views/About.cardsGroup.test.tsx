import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// UI 调研 shell #2：「卡片」组的「对话预览」「打开终端方式」只有已下线的贴纸在读，拨了没反应。
// 关卡 A 定案：删这两项（字段保留读旧配置），浏览器那项挪到「运行」组。
// 设置页精简（2026-11-19）：「会话列表」「已完成会话」两个单行分组合成一组「列表」。
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  leadProviderInstallations: vi.fn(),
  availableTerminals: vi.fn(),
  listAgents: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { GeneralSection, SessionsSection } from "./About";
import { zh } from "../i18n/zh";

// 旧 settings.json：两个死字段是用户改过的非默认值。
const OLD = { browser_agent: true, preview_enabled: false, terminal_open_mode: "button", card_menu_mode: "context" };

beforeEach(() => {
  api.leadWorkflowInstalled.mockResolvedValue(false);
  api.leadProviderInstallations.mockResolvedValue([]);
  api.setSettings.mockResolvedValue(undefined);
  api.availableTerminals.mockResolvedValue([]);
  api.listAgents.mockResolvedValue([]);
  api.getSettings.mockResolvedValue(OLD);
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

const captionBefore = (el: Element) => {
  let card = el.closest(".row-card");
  let prev = card?.previousElementSibling;
  while (prev && !prev.classList.contains("sec-caption")) prev = prev.previousElementSibling;
  return prev?.textContent;
};

describe("设置：列表组", () => {
  it("会话分区不再有对话预览与打开方式，卡片菜单与已完成会话同在「列表」组", async () => {
    render(<SessionsSection />);
    const cardMenu = await screen.findByText(zh.settings.cardMenu);
    expect(captionBefore(cardMenu)).toBe("列表");
    // 已完成会话是同一张卡里的一行，不再自成一组（2026-11-20 起没有轻量模式，这是唯一形态）。
    const admin = screen.getByRole("button", { name: zh.settings.archivedOpenAdmin });
    expect(admin.closest(".row-card")).toBe(cardMenu.closest(".row-card"));
    expect(Array.from(document.querySelectorAll(".sec-caption")).map((el) => el.textContent)).toEqual([zh.settings.worktreeGroup, "列表"]);
    expect(screen.queryByText("对话预览")).toBeNull();
    expect(screen.queryByText("打开方式")).toBeNull();
    expect(screen.queryByText(zh.settings.browserAgent)).toBeNull();
  });

  it("worktree 位置与分支前缀成一组", async () => {
    render(<SessionsSection />);
    const location = await screen.findByText(zh.wtnew.settingsLocation);
    const prefix = screen.getByText(zh.wtnew.settingsPrefix);
    expect(location.closest(".row-card")).toBe(prefix.closest(".row-card"));
    expect(captionBefore(location)).toBe(zh.settings.worktreeGroup);
  });

  it("浏览器开关在通用分区的「运行」组，拨它时旧配置的死字段原样写回", async () => {
    render(<GeneralSection />);
    const sw = await screen.findByRole("switch", { name: zh.settings.browserAgent });
    expect(captionBefore(sw)).toBe(zh.settings.runGroup);
    await waitFor(() => expect(sw.getAttribute("aria-checked")).toBe("true"));
    fireEvent.click(sw);
    await waitFor(() => expect(api.setSettings).toHaveBeenCalled());
    const saved = api.setSettings.mock.calls.at(-1)![0] as Record<string, unknown>;
    expect(saved.browser_agent).toBe(false);
    expect(saved.preview_enabled).toBe(false);
    expect(saved.terminal_open_mode).toBe("button");
  });
});
