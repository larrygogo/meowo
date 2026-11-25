import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";

// 通用页（设置页精简 2026-11-19，审查第 2、3 条）：
// - 「有关卡待拍板时」防休眠子行的显示条件与改前一致：Claude Code 装了 Lead 或其他 CLI 有一家装着才出现；
// - 网络代理整组在通用页最后（Lead 之后），首屏留给语言、自启、更新和「运行」组。
// 防休眠一组只在 Windows / macOS 出现，CI 的 vitest 跑在 Linux：先把 UA 钉成 Windows 再导入组件。
vi.hoisted(() => {
  Object.defineProperty(window.navigator, "userAgent", { value: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) jsdom", configurable: true });
});
const api = vi.hoisted(() => ({
  getSettings: vi.fn(),
  setSettings: vi.fn(),
  leadWorkflowInstalled: vi.fn(),
  leadProviderInstallations: vi.fn(),
  listAgents: vi.fn(),
  teamWorkflowState: vi.fn(),
  setTeamWorkflow: vi.fn(),
}));
vi.mock("../api", async (o) => ({ ...(await o<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { GeneralSection } from "./About";
import { zh } from "../i18n/zh";

const provider = (installed: boolean) => ({ provider: "codex", name: "Codex", profileId: null, profileName: null, version: "1", configured: true, supported: true, managed: installed, installed, assetsInstalled: installed, hooksInstalled: installed, automaticContinue: installed, notes: [], reason: null });

beforeEach(() => {
  api.getSettings.mockResolvedValue({});
  api.setSettings.mockResolvedValue(undefined);
  api.listAgents.mockResolvedValue([]);
  api.leadProviderInstallations.mockResolvedValue([]);
  api.teamWorkflowState.mockResolvedValue(team({}));
});
afterEach(() => {
  cleanup();
  Object.values(api).forEach((m) => m.mockReset());
});

const team = (over: Partial<{ installed: boolean; leadInstalled: boolean; conflict: string | null; personalDir: string | null }>) =>
  ({ installed: false, leadInstalled: false, conflict: null, personalDir: "/home/u/.meowo/team", ...over });
const teamSwitch = () => screen.findByRole<HTMLButtonElement>("switch", { name: zh.settings.team });

const gateRow = () => screen.queryByRole("switch", { name: `${zh.settings.keepAwakeGroup} · ${zh.settings.keepAwakeLeadGate}` });

describe("「防止电脑休眠」组：关卡那一行跟着 Lead 是否装着", () => {
  it("Claude Code 和其他 CLI 都没装 Lead：组里只有「有会话在运行时」一个子开关", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(false);
    render(<GeneralSection />);
    await screen.findByRole("switch", { name: `${zh.settings.keepAwakeGroup} · ${zh.settings.keepAwakeRunning}` });
    await waitFor(() => expect(api.leadProviderInstallations).toHaveBeenCalled());
    expect(gateRow()).toBeNull();
    const head = screen.getByText(zh.settings.keepAwakeGroup).closest(".row")!;
    expect(head.nextElementSibling?.classList.contains("row-sub")).toBe(true);
    expect(head.nextElementSibling?.nextElementSibling?.classList.contains("row-sub") ?? false).toBe(false);
  });

  it("Claude Code 装了 Lead：出现「有关卡待拍板时」", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(true);
    render(<GeneralSection />);
    await waitFor(() => expect(gateRow()).toBeTruthy());
  });

  it("只有其他 CLI 装了 Lead：同样出现", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(false);
    api.leadProviderInstallations.mockResolvedValue([provider(true)]);
    render(<GeneralSection />);
    await waitFor(() => expect(gateRow()).toBeTruthy());
  });
});

describe("网络代理在通用页最后", () => {
  it("分组顺序：（首卡）→ 运行 → Lead 工作流 → 网络；网络代理不在首卡", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(false);
    const { container } = render(<GeneralSection />);
    const modeRow = (await screen.findByText(zh.proxy.mode)).closest(".row-card")!;
    expect(Array.from(container.querySelectorAll(".sec-caption")).map((el) => el.textContent)).toEqual([zh.settings.runGroup, zh.settings.leadGroup, zh.settings.networkGroup]);
    const cards = Array.from(container.querySelectorAll(":scope > .row-card"));
    expect(cards.at(-1)).toBe(modeRow);
    expect(cards[0]!.textContent).toContain(zh.settings.autoUpdate);
    expect(cards[0]!.textContent).not.toContain(zh.proxy.mode);
  });
});

// 团队（team.rs）是 Lead 的扩展：Lead 关着不能开；关掉 Lead 时后端会一起卸掉团队，所以跟着 Lead 开关重读状态。
describe("团队一行", () => {
  it("Lead 没开：开关不可用，并说明先开 Lead", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(false);
    render(<GeneralSection />);
    expect((await teamSwitch()).disabled).toBe(true);
    expect(screen.getByText(zh.settings.teamNeedsLead)).toBeTruthy();
    expect(screen.getByText(zh.settings.teamDesc)).toBeTruthy();
  });

  it("Lead 开着：打开团队调用后端，装好后显示个人层位置", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(true);
    api.teamWorkflowState.mockImplementation(() => Promise.resolve(team({ leadInstalled: true, installed: api.setTeamWorkflow.mock.calls.length > 0 })));
    api.setTeamWorkflow.mockResolvedValue(undefined);
    render(<GeneralSection />);
    const sw = await teamSwitch();
    await waitFor(() => expect(sw.disabled).toBe(false));
    sw.click();
    await waitFor(() => expect(api.setTeamWorkflow).toHaveBeenCalledWith(true));
    await screen.findByText(zh.settings.teamPersonal("/home/u/.meowo/team"));
  });

  it("本机另有一份 team 插件：说清是哪个目录，开关置灰，不再重复报错", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(true);
    api.teamWorkflowState.mockResolvedValue(team({ leadInstalled: true, conflict: "D:/x/team" }));
    render(<GeneralSection />);
    const hint = await screen.findByText(zh.settings.teamConflict("D:/x/team"));
    expect(hint.classList.contains("is-full")).toBe(true);
    expect((await teamSwitch()).disabled).toBe(true);
  });

  it("后端拒绝时显示翻译后的原因和路径", async () => {
    api.leadWorkflowInstalled.mockResolvedValue(true);
    api.teamWorkflowState.mockResolvedValue(team({ leadInstalled: true }));
    api.setTeamWorkflow.mockRejectedValue("team/user_file_exists：C:/u/.claude/agents/team-pm.md");
    render(<GeneralSection />);
    const sw = await teamSwitch();
    await waitFor(() => expect(sw.disabled).toBe(false));
    sw.click();
    await screen.findByText("已有你自己的同名团队文件，没有覆盖，已停止安装：C:/u/.claude/agents/team-pm.md");
  });
});
