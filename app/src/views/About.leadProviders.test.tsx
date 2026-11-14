import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode } from "react";

const api = vi.hoisted(() => ({ leadProviderInstallations: vi.fn(), setLeadProviderEnabled: vi.fn() }));
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), ...api }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(false)) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { LeadProviderRows } from "./About";
import type { LeadProviderInstallationDto } from "../api";
import { zh } from "../i18n/zh";
import { formatBackendError } from "../i18n/errors";

function row(provider: string, values: Partial<LeadProviderInstallationDto> = {}): LeadProviderInstallationDto {
  return { provider, name: provider, profileId: null, profileName: null, version: "test", configured: true, supported: true, managed: false, installed: false, assetsInstalled: false, hooksInstalled: false, automaticContinue: false, notes: [], reason: null, ...values };
}
beforeEach(() => { api.leadProviderInstallations.mockResolvedValue([row("codex"), row("kimi"), row("gemini"), row("opencode")]); });
afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("Lead 按 Provider / 账号独立安装", () => {
  it("StrictMode重新挂载后旧查询晚到不能覆盖本轮安装状态", async () => {
    let oldResult!: (rows: LeadProviderInstallationDto[]) => void;
    let currentResult!: (rows: LeadProviderInstallationDto[]) => void;
    api.leadProviderInstallations.mockReturnValueOnce(new Promise<LeadProviderInstallationDto[]>((resolve) => { oldResult = resolve; }))
      .mockReturnValueOnce(new Promise<LeadProviderInstallationDto[]>((resolve) => { currentResult = resolve; }));
    render(<StrictMode><LeadProviderRows /></StrictMode>);
    await waitFor(() => expect(api.leadProviderInstallations).toHaveBeenCalledTimes(2));
    await act(async () => { currentResult([row("opencode")]); });
    expect(await screen.findByRole("button", { name: `${zh.settings.leadInstall} opencode` })).toBeTruthy();
    await act(async () => { oldResult([row("kimi")]); });
    expect(screen.queryByRole("button", { name: `${zh.settings.leadInstall} kimi` })).toBeNull();
    expect(screen.getByRole("button", { name: `${zh.settings.leadInstall} opencode` })).toBeTruthy();
  });
  it("安装单个 CLI 后保留其他项，并明确自动继续仍不可用", async () => {
    api.setLeadProviderEnabled.mockResolvedValue(row("gemini", { managed: true, installed: true, assetsInstalled: true, hooksInstalled: true }));
    render(<LeadProviderRows />);
    fireEvent.click(await screen.findByRole("button", { name: `${zh.settings.leadInstall} gemini` }));
    await waitFor(() => expect(api.setLeadProviderEnabled).toHaveBeenCalledWith("gemini", null, true));
    expect(await screen.findByText(zh.settings.leadInstalledNoContinue)).toBeTruthy();
    expect(screen.queryByText(zh.settings.leadInstalled)).toBeNull();
    expect(screen.getAllByText(zh.settings.leadNotInstalled)).toHaveLength(3);
    expect(screen.getByRole("button", { name: `${zh.settings.leadUninstall} gemini` })).toBeTruthy();
  });

  it("自动继续的副作用说明来自插件声明，只在可自动继续时显示", async () => {
    const note = "lead/note_dialog_starts_turn";
    api.leadProviderInstallations.mockResolvedValue([
      row("gemini", { managed: true, installed: true, assetsInstalled: true, hooksInstalled: true, automaticContinue: true, notes: [note] }),
      row("kimi", { managed: true, installed: true, assetsInstalled: true, hooksInstalled: true, automaticContinue: false, notes: [note] }),
    ]);
    render(<LeadProviderRows />);
    expect(await screen.findAllByText(formatBackendError(note, "zh"))).toHaveLength(1);
    // 文案约束：一行内、不用破折号和分号，中英文都有映射（没映射时 formatBackendError 会原样回键名）。
    for (const locale of ["zh", "en"] as const) {
      const text = formatBackendError(note, locale);
      expect(text).not.toBe(note);
      expect(text).not.toMatch(/[—–;；]/);
    }
  });

  it("未知版本仍允许卸载托管文件，不允许重新安装", async () => {
    const old = row("kimi", { supported: false, managed: true, reason: "lead/version_unsupported" });
    api.leadProviderInstallations.mockResolvedValue([old]); api.setLeadProviderEnabled.mockResolvedValue({ ...old, managed: false });
    render(<LeadProviderRows />);
    const install = await screen.findByRole("button", { name: `${zh.settings.leadInstall} kimi` });
    expect((install as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(formatBackendError(old.reason!, "zh"))).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: `${zh.settings.leadUninstall} kimi` }));
    await waitFor(() => expect(api.setLeadProviderEnabled).toHaveBeenCalledWith("kimi", null, false));
    await waitFor(() => expect(screen.queryByRole("button", { name: `${zh.settings.leadUninstall} kimi` })).toBeNull());
  });

  it("操作只指向选中账号，同一请求未完成时不能重复提交", async () => {
    api.leadProviderInstallations.mockResolvedValue([row("codex"), row("codex", { profileId: "work", profileName: "工作" })]);
    let finish!: (value: LeadProviderInstallationDto) => void;
    api.setLeadProviderEnabled.mockReturnValue(new Promise<LeadProviderInstallationDto>((resolve) => { finish = resolve; }));
    render(<LeadProviderRows />);
    const button = await screen.findByRole("button", { name: `${zh.settings.leadInstall} codex · 工作` });
    fireEvent.click(button); fireEvent.click(button);
    expect(api.setLeadProviderEnabled).toHaveBeenCalledTimes(1);
    expect(api.setLeadProviderEnabled).toHaveBeenCalledWith("codex", "work", true);
    finish(row("codex", { profileId: "work", profileName: "工作", managed: true, installed: true }));
    expect(await screen.findByRole("button", { name: `${zh.settings.leadUninstall} codex · 工作` })).toBeTruthy();
    expect(screen.getByRole("button", { name: `${zh.settings.leadInstall} codex` })).toBeTruthy();
  });

  it("安装失败显示双语错误，保留原状态，允许重试", async () => {
    api.setLeadProviderEnabled.mockRejectedValue("lead/user_file_exists");
    render(<LeadProviderRows />);
    const button = await screen.findByRole("button", { name: `${zh.settings.leadInstall} opencode` });
    fireEvent.click(button);
    expect((await screen.findByRole("alert")).textContent).toContain(formatBackendError("lead/user_file_exists", "zh"));
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    expect(screen.getAllByText(zh.settings.leadNotInstalled)).toHaveLength(4);
  });
});
