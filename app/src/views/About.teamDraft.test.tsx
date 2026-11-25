// 团队子页没保存的改动在整个设置窗里不丢（做空 redteam-2 N1，探针改写）：搜索、切分区回来还在；
// 返回、Esc、关窗都拦下，保存条提示先保存或放弃。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  availableTerminals: vi.fn(),
  listAgents: vi.fn(),
  leadProviderInstallations: vi.fn(),
  teamWorkflowState: vi.fn(),
  teamPersonalConfig: vi.fn(),
  saveTeamPersonalConfig: vi.fn(),
  createTeamTaste: vi.fn(),
}));
const closed = vi.hoisted(() => ({ n: 0 }));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(() => Promise.resolve("0.0.0")) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ close: () => { closed.n++; return Promise.resolve(); }, show: () => Promise.resolve(), setFocus: () => Promise.resolve() }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../useUpdate", () => ({ useUpdate: () => ({ status: "idle", version: null }) }));
vi.mock("../useShowWhenReady", () => ({ useShowWhenReady: () => {} }));
vi.mock("./settings/AccountSection", () => ({ AccountSection: () => null }));
vi.mock("./settings/NetworkSection", () => ({ ProxyRows: () => null }));
vi.mock("./settings/RemoteAccessCard", () => ({ RemoteAccessCard: () => null }));

import { About } from "./About";
import { zh } from "../i18n/zh";
import { SETTINGS_SECTION_KEY } from "../settingsNav";

const p = zh.settings.teamPage;
const dto = {
  personalDir: "/h/.meowo/team", configPath: "/h/.meowo/team/config.json", exists: true, error: null, errorLine: null,
  errorFirstLine: 0, errorLines: [], authorityDefaulted: false, token: "t1", taste: null, tasteExists: false,
  protected: ["a-repo", "b-repo"], doAndReport: ["本地提交"], askAtEnd: ["合并"], askFirst: [], floorPrefixes: ["上生产"], blockedCommandCount: 0,
};

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(SETTINGS_SECTION_KEY, "team");
  closed.n = 0;
  api.getSettings.mockResolvedValue({});
  api.setSettings.mockResolvedValue(undefined);
  api.leadWorkflowInstalled.mockResolvedValue(true);
  api.availableTerminals.mockResolvedValue([]);
  api.listAgents.mockResolvedValue([]);
  api.leadProviderInstallations.mockResolvedValue([]);
  api.teamWorkflowState.mockResolvedValue({ installed: true, leadInstalled: true, conflict: null, personalDir: "/h/.meowo/team", personalConfigError: null, blockedFiles: [] });
  api.teamPersonalConfig.mockResolvedValue(dto);
});
afterEach(() => { cleanup(); Object.values(api).forEach((m) => m.mockReset()); });

async function dirty() {
  render(<About />);
  fireEvent.click(await screen.findByRole("button", { name: p.removeDir("a-repo") }));
  await screen.findByText(p.dirty(1));
}

describe("团队子页没保存的改动", () => {
  it("在搜索框打字再清掉，改动还在", async () => {
    await dirty();
    const input = screen.getByPlaceholderText(zh.settings.searchPlaceholder);
    fireEvent.change(input, { target: { value: "团" } });
    fireEvent.change(input, { target: { value: "" } });
    await screen.findByText("b-repo");
    expect(screen.queryByText(p.dirty(1))).not.toBeNull();
    expect(screen.queryByText("a-repo")).toBeNull();
  });

  it("切到别的分区再切回来，改动还在", async () => {
    await dirty();
    fireEvent.click(screen.getByText(zh.settings.nav.appearance, { selector: ".side-nav *" }));
    fireEvent.click(screen.getByText(zh.settings.nav.general, { selector: ".side-nav *" }));
    await screen.findByText("b-repo");
    expect(screen.queryByText(p.dirty(1))).not.toBeNull();
  });

  it("Esc 和返回不离开子页，保存条提示先保存或放弃；放弃后才能返回", async () => {
    await dirty();
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(window, { key: "Escape" });
    await screen.findByText(p.leaveFirst, { exact: false });
    expect(screen.queryByText(p.dirty(1))).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: p.back }));
    expect(screen.queryByText(p.dirty(1))).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: p.discard }));
    fireEvent.click(screen.getByRole("button", { name: p.back }));
    await screen.findByText(zh.settings.teamSettings);
  });

  it("文件被写坏时不拦离开，窗口不会被困住", async () => {
    await dirty();
    api.createTeamTaste.mockRejectedValueOnce("team/config_changed");
    api.teamPersonalConfig.mockResolvedValue({ ...dto, error: "不是合法的 JSON", errorLine: 2, errorFirstLine: 1, errorLines: ["{", "x"] });
    fireEvent.click(screen.getByRole("button", { name: p.newTaste }));
    await screen.findByText(p.brokenPending(1));
    fireEvent.click(screen.getByRole("button", { name: zh.settings.close }));
    await waitFor(() => expect(closed.n).toBe(1));
  });

  it("关窗被拦下，不丢改动", async () => {
    await dirty();
    fireEvent.click(screen.getByRole("button", { name: zh.settings.close }));
    await screen.findByText(p.leaveFirst, { exact: false });
    expect(closed.n).toBe(0);
  });
});
