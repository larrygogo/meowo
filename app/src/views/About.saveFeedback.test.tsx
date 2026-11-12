import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

// 第三轮走查（2026-11-12，research-walkthrough.md 第 4 部分）T-4 / T-5 / T-6：
// - T-4 设置搜索后 Enter/↓ 焦点停在搜索框，要按约 11 下 Tab 才进结果；无结果只有一行字。
// - T-5 worktree 分支前缀框只在失焦时保存，回车无反应；文本框提交没有「已保存」反馈。
// - T-6 开机自启失败是另一套红字、没有 ×。
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  availableTerminals: vi.fn(),
  listAgents: vi.fn(),
}));
const core = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => core);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(() => Promise.resolve("0.0.0")) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: () => Promise.resolve(), show: () => Promise.resolve(), setFocus: () => Promise.resolve() }) }));
vi.mock("../useUpdate", () => ({ useUpdate: () => ({ status: "idle", version: null }) }));
vi.mock("../useShowWhenReady", () => ({ useShowWhenReady: () => {} }));
vi.mock("./settings/AccountSection", () => ({ AccountSection: () => null }));
vi.mock("./settings/ExtensionsSection", () => ({ ExtensionsSection: () => null }));
vi.mock("./settings/NetworkSection", () => ({ NetworkSection: () => null }));
vi.mock("./settings/RemoteAccessCard", () => ({ RemoteAccessCard: () => null }));

import { About, GeneralSection, SessionsSection } from "./About";
import { zh } from "../i18n/zh";

beforeEach(() => {
  localStorage.clear();
  api.getSettings.mockResolvedValue({ worktree: { location: "sibling", branch_prefix: "meowo/", trusted: {} } });
  api.setSettings.mockResolvedValue(undefined);
  api.leadWorkflowInstalled.mockResolvedValue(false);
  api.availableTerminals.mockResolvedValue([]);
  api.listAgents.mockResolvedValue([]);
  core.invoke.mockImplementation(() => Promise.resolve(false));
});
afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
  Object.values(api).forEach((m) => m.mockReset());
});

describe("T-4 设置搜索的键盘出路", () => {
  it("搜索框按 ↓ 焦点落到第一条命中行的控件；无结果时有「清除搜索」按钮且清完回到搜索框", async () => {
    render(<About />);
    const input = await screen.findByPlaceholderText(zh.settings.searchPlaceholder);
    input.focus();
    fireEvent.change(input, { target: { value: zh.wtnew.settingsPrefix } });
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("textbox", { name: zh.wtnew.settingsPrefix }));

    input.focus();
    fireEvent.change(input, { target: { value: "zzz-没有这个设置" } });
    const clear = await waitFor(() => {
      const el = document.querySelector<HTMLButtonElement>(".settings-noresults button");
      expect(el).toBeTruthy();
      return el!;
    });
    expect(clear.textContent).toBe(zh.settings.searchClear);
    fireEvent.click(clear);
    expect((input as HTMLInputElement).value).toBe("");
    expect(document.activeElement).toBe(input);
  });
});

describe("T-5 分支前缀框回车提交", () => {
  it("回车即保存、焦点留在框里、给出已保存", async () => {
    render(<SessionsSection />);
    const box = (await screen.findByRole("textbox", { name: zh.wtnew.settingsPrefix })) as HTMLInputElement;
    await waitFor(() => expect(box.value).toBe("meowo/"));
    box.focus();
    fireEvent.change(box, { target: { value: "larry/" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await waitFor(() => expect(api.setSettings).toHaveBeenCalledWith(expect.objectContaining({ worktree: expect.objectContaining({ branch_prefix: "larry/" }) })));
    expect(await screen.findByText(zh.settings.saved)).toBeTruthy();
    expect(document.activeElement).toBe(box);
    expect(box.value).toBe("larry/");
  });
});

describe("T-6 开机自启失败", () => {
  it("用统一的 SettingsError：带 ×、带重试，重试再拨一次", async () => {
    // dev 构建下开机自启整行置灰（会注册调试二进制），测的是安装版路径。
    vi.stubEnv("DEV", false);
    let calls = 0;
    core.invoke.mockImplementation((cmd: string) => {
      if (cmd === "set_autostart") { calls += 1; return calls === 1 ? Promise.reject("boom") : Promise.resolve(); }
      return Promise.resolve(false);
    });
    render(<GeneralSection />);
    const sw = await screen.findByRole("switch", { name: zh.settings.autostart });
    await act(async () => { fireEvent.click(sw); });
    const text = await screen.findByText(zh.settings.autostartFailed);
    const box = text.closest(".settings-error");
    expect(box?.getAttribute("role")).toBe("alert");
    expect(box?.querySelector(".settings-error-close")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.settings.retry })); });
    await waitFor(() => expect(calls).toBe(2));
    await waitFor(() => expect(screen.queryByText(zh.settings.autostartFailed)).toBeNull());
  });
});
