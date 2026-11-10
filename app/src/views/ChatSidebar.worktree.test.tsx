import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import { WorkspaceDirectory } from "./chat/WorkspaceDirectory";
import { groupByDir } from "./sidebarDrag";
import type { LiveSession } from "../api";

/// 包 5：worktree 会话在侧栏 / 灵动岛 / 看板归到原项目下（分组键 origin_root ?? cwd），组内标出
/// worktree 名；「隐藏工作区」仍按会话自己的 cwd，不连带主仓库。

function session(id: number, title: string, extra: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id, cc_session_id: `cc-${id}`, status: "ended" },
    project_name: "app",
    task_title: title,
    connected: false,
    pending_review: null,
    cwd: "C:/w/app",
    provider: "claude",
    ...extra,
  } as unknown as LiveSession;
}

describe("worktree 会话归组", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    localStorage.clear();
  });

  it("groupByDir 按 origin_root ?? cwd 分组，会话自己的 cwd 不变；主检出子目录照旧单独成组", () => {
    const items = [
      session(1, "主检出", { cwd: "C:/w/app" }),
      session(2, "worktree 里", { cwd: "C:/w/app-feat", origin_root: "C:/w/app" }),
      session(3, "worktree 子目录", { cwd: "C:/w/app-feat/src", origin_root: "C:/w/app" }),
      session(4, "主检出子目录", { cwd: "C:/w/app/docs" }),
      session(5, "旧后端没带字段", { cwd: "C:/w/other" }),
    ];
    const groups = groupByDir(items);
    expect(groups.map((g) => g.label)).toEqual(["app", "docs", "other"]);
    expect(groups[0].items.map((i) => i.session.id)).toEqual([1, 2, 3]);
    expect(groups[0].cwd).toBe("C:/w/app");
    // 分组不改条目：隐藏、筛选、打开都还按会话自己的目录走。
    expect(groups[0].items[1].cwd).toBe("C:/w/app-feat");
  });

  it("组头只有一个原项目，组内的 worktree 会话标出 worktree 名", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          session(1, "主检出的会话", { cwd: "C:/w/app" }),
          session(2, "修登录", { cwd: "C:/w/app-fix-login", origin_root: "C:/w/app" }),
        ]);
      }
      return Promise.resolve();
    });
    localStorage.setItem("meowo-chat-sidebar-group-mode", "dir");
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    const wtRow = await screen.findByRole("button", { name: /修登录/ });
    // 只有一个组：两条会话都在「app」下。
    const groups = document.querySelectorAll(".chat-sidebar-group");
    expect(groups).toHaveLength(1);
    expect(within(groups[0] as HTMLElement).getByRole("button", { name: /主检出的会话/ })).toBeTruthy();
    expect(within(groups[0] as HTMLElement).getByRole("button", { name: /修登录/ })).toBeTruthy();
    expect(within(wtRow).getByTestId("sidebar-worktree-tag").textContent).toBe("app-fix-login");
    const mainRow = screen.getByRole("button", { name: /主检出的会话/ });
    expect(within(mainRow).queryByTestId("sidebar-worktree-tag")).toBeNull();
  });

  it("隐藏 worktree 那个工作区不连带主仓库", async () => {
    invoke.mockResolvedValue(undefined);
    const onHidden = vi.fn();
    render(
      <WorkspaceDirectory
        options={[{ value: "C:/w/app", label: "app" }, { value: "C:/w/app-feat", label: "app-feat" }]}
        onFilter={() => {}}
        onHidden={onHidden}
      />,
    );
    fireEvent.click(screen.getAllByRole("button", { name: "选项" })[1]);
    fireEvent.click(await screen.findByRole("menuitem", { name: "从列表中移除" }));
    expect(invoke).toHaveBeenCalledWith("hide_workspace", { cwd: "C:/w/app-feat" });
    expect(invoke).not.toHaveBeenCalledWith("hide_workspace", { cwd: "C:/w/app" });
    await waitFor(() => expect(onHidden).toHaveBeenCalled());
    expect(screen.getByText("C:/w/app")).toBeTruthy();
    expect(screen.queryByText("C:/w/app-feat")).toBeNull();
  });
});
