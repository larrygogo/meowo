import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";

/// 包 5：会话页顶部的 setup 进度条与信任确认框。setup 失败不挡会话，能看日志、跳过、重跑；
/// 没信任过的脚本先在确认框里看原文。

const api = vi.hoisted(() => ({
  worktreeSetup: vi.fn(),
  worktreeSetupTrust: vi.fn(),
}));
vi.mock("../../api", async (orig) => ({ ...(await orig<typeof import("../../api")>()), ...api }));
vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));

import { WorktreeSetupBar } from "./WorktreeSetupBar";
import { hasEscLayers } from "../../escLayers";
import { zh } from "../../i18n/zh";

const dto = (extra: Record<string, unknown> = {}) => ({
  taskId: "wt-1",
  path: "C:/w/app-feat",
  state: "running",
  exitCode: null,
  logTail: "installing deps",
  logPath: "C:/Users/me/.meowo/worktree-setup/wt-1.log",
  startedAt: 1,
  finishedAt: null,
  reason: null,
  script: "bun install",
  configHash: "h1",
  configSource: "meowo",
  runner: "pwsh",
  ...extra,
});

beforeEach(() => {
  api.worktreeSetup.mockReset();
  api.worktreeSetupTrust.mockReset();
});
afterEach(() => cleanup());

describe("worktree setup 进度条", () => {
  it("不是 Meowo 建的 worktree、或仓库没配 setup：不显示", async () => {
    api.worktreeSetup.mockResolvedValue(null);
    const { container, rerender } = render(<WorktreeSetupBar cwd="C:/w/app" />);
    await waitFor(() => expect(api.worktreeSetup).toHaveBeenCalledWith("C:/w/app"));
    expect(container.querySelector("[data-testid=wt-setup-bar]")).toBeNull();
    api.worktreeSetup.mockResolvedValue(dto({ state: "skipped", reason: "none" }));
    rerender(<WorktreeSetupBar cwd="C:/w/app-feat" />);
    await waitFor(() => expect(api.worktreeSetup).toHaveBeenCalledWith("C:/w/app-feat"));
    expect(container.querySelector("[data-testid=wt-setup-bar]")).toBeNull();
  });

  it("运行中：进度条、看日志、跳过", async () => {
    api.worktreeSetup.mockResolvedValue(dto());
    render(<WorktreeSetupBar cwd="C:/w/app-feat/src" />);
    const bar = await screen.findByTestId("wt-setup-bar");
    expect(bar.textContent).toContain(zh.wtnew.state.running);
    expect(bar.querySelector(".wt-setup-progress")).toBeTruthy();
    fireEvent.click(screen.getByTestId("wt-setup-log"));
    expect((await screen.findByTestId("wt-setup-log-text")).textContent).toBe("installing deps");
    api.worktreeSetup.mockResolvedValue(dto({ state: "skipped", reason: "user" }));
    fireEvent.click(screen.getByTestId("wt-setup-skip"));
    await waitFor(() => expect(api.worktreeSetup).toHaveBeenCalledWith("C:/w/app-feat", "skip"));
    await waitFor(() => expect(screen.getByTestId("wt-setup-bar").textContent).toContain(zh.wtnew.state.skipped));
  });

  it("失败：显示退出码和日志，可以重跑", async () => {
    api.worktreeSetup.mockResolvedValue(dto({ state: "failed", exitCode: 3, logTail: "error: boom" }));
    render(<WorktreeSetupBar cwd="C:/w/app-feat" />);
    const bar = await screen.findByTestId("wt-setup-bar");
    expect(bar.textContent).toContain(zh.wtnew.state.failed);
    expect(bar.textContent).toContain(zh.wtnew.exitCode(3));
    fireEvent.click(screen.getByTestId("wt-setup-log"));
    expect((await screen.findByTestId("wt-setup-log-text")).textContent).toContain("error: boom");
    api.worktreeSetup.mockResolvedValue(dto({ state: "running", startedAt: 2 }));
    fireEvent.click(screen.getByTestId("wt-setup-rerun"));
    await waitFor(() => expect(api.worktreeSetup).toHaveBeenCalledWith("C:/w/app-feat", "rerun"));
    await waitFor(() => expect(screen.getByTestId("wt-setup-bar").textContent).toContain(zh.wtnew.state.running));
  });

  it("待确认：确认框展示脚本原文，Esc 只关确认框，信任后带着同一个 hash 去跑", async () => {
    api.worktreeSetup.mockResolvedValue(dto({ state: "needs_trust", startedAt: null, script: "bun install && bun run db:migrate" }));
    render(<WorktreeSetupBar cwd="C:/w/app-feat" />);
    fireEvent.click(await screen.findByTestId("wt-setup-review"));
    const dialog = await screen.findByTestId("wt-trust-dialog");
    expect(screen.getByTestId("wt-trust-script").textContent).toBe("bun install && bun run db:migrate");
    expect(dialog.textContent).toContain(".meowo/worktree.json");
    expect(dialog.textContent).toContain(zh.wtnew.trustNote);
    // 确认框注册了 Esc 层：窗口级「Esc=拒绝审批」此时让位。
    expect(hasEscLayers()).toBe(true);
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(screen.queryByTestId("wt-trust-dialog")).toBeNull());
    expect(hasEscLayers()).toBe(false);
    expect(api.worktreeSetupTrust).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("wt-setup-review"));
    api.worktreeSetupTrust.mockResolvedValue(dto({ state: "running" }));
    fireEvent.click(await screen.findByTestId("wt-trust-run"));
    await waitFor(() => expect(api.worktreeSetupTrust).toHaveBeenCalledWith("C:/w/app-feat", "h1"));
    await waitFor(() => expect(screen.queryByTestId("wt-trust-dialog")).toBeNull());
    expect(screen.getByTestId("wt-setup-bar").textContent).toContain(zh.wtnew.state.running);
  });

  it("完成后可以关掉这条", async () => {
    api.worktreeSetup.mockResolvedValue(dto({ state: "ok", exitCode: 0, finishedAt: 5 }));
    render(<WorktreeSetupBar cwd="C:/w/app-feat" />);
    await screen.findByTestId("wt-setup-bar");
    fireEvent.click(screen.getByTestId("wt-setup-dismiss"));
    expect(screen.queryByTestId("wt-setup-bar")).toBeNull();
  });
});
