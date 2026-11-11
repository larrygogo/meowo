// UI 优化 #54：侧栏会话行在工作区名后显示 PR / CI 状态点与 +N −M，只给有 PR 或有改动的会话；
// 不占行首 activityTone 状态点那一格；窗口隐藏时不发请求。
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import type { LiveSession } from "../api";

function session(id: number, title: string, cwd: string): LiveSession {
  return {
    session: { id, cc_session_id: `cc-${id}`, status: "running", last_event_at: 1_700_000_000_000 },
    project_name: cwd.split("/").pop(),
    task_title: title,
    connected: true,
    pending_review: null,
    cwd,
    provider: "claude",
  } as unknown as LiveSession;
}

const SESSIONS = [
  session(1, "有 PR 的", "C:/work/feat"),
  session(2, "同仓库另一个", "C:/work/feat"),
  session(3, "干净的", "C:/work/clean"),
];

const BADGES = [{
  cwd: "C:/work/feat", additions: 12, deletions: 3, files: 2,
  pr: { number: 162, state: "open", checks: "fail", url: "https://github.com/o/r/pull/162", title: "t" },
}];

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => state });
}

const badgeCalls = () => invoke.mock.calls.filter(([command]) => command === "sidebar_git_badges");

describe("侧栏 PR / CI 与改动数", () => {
  afterEach(() => { cleanup(); setVisibility("visible"); vi.useRealTimers(); });
  beforeEach(() => {
    invoke.mockReset();
    localStorage.clear();
    setVisibility("visible");
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") return Promise.resolve(SESSIONS);
      if (command === "sidebar_git_badges") return Promise.resolve(BADGES);
      return Promise.resolve();
    });
  });

  it("有 PR 或改动的行显示状态点与增删数，干净的行不显示，同目录只请求一次", async () => {
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await waitFor(() => expect(screen.getAllByTestId("sidebar-git-badge")).toHaveLength(2));
    expect(badgeCalls()).toHaveLength(1);
    expect(badgeCalls()[0][1]).toEqual({ cwds: ["C:/work/clean", "C:/work/feat"] });
    const badge = screen.getAllByTestId("sidebar-git-badge")[0];
    expect(badge.getAttribute("aria-label")).toBe("PR #162 已打开 · CI 失败 · 2 个文件有未提交的改动");
    expect(badge.querySelector(".chat-sidebar-pr.is-open")).not.toBeNull();
    expect(badge.querySelector(".chat-sidebar-ci.is-fail")).not.toBeNull();
    expect(badge.textContent).toBe("+12−3");
    // 跟在副行工作区名后面，不进行首状态点那一格。
    expect(badge.closest(".chat-sidebar-metaline")).not.toBeNull();
    expect(badge.closest(".chat-sidebar-lead")).toBeNull();
    const cleanRow = screen.getByText("干净的").closest(".chat-sidebar-item")!;
    expect(cleanRow.querySelector("[data-testid='sidebar-git-badge']")).toBeNull();
  });

  it("窗口隐藏时不请求，回到前台补一次", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    setVisibility("hidden");
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await screen.findByText("有 PR 的");
    await act(async () => { vi.advanceTimersByTime(95_000); });
    expect(badgeCalls()).toHaveLength(0);
    setVisibility("visible");
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
    expect(badgeCalls()).toHaveLength(1);
    expect(await screen.findAllByTestId("sidebar-git-badge")).toHaveLength(2);
  });

  it("按工作区分组时没有副行也给摘要开一行", async () => {
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    await waitFor(() => expect(screen.getAllByTestId("sidebar-git-badge")).toHaveLength(2));
  });
});
