import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("./platform", () => ({ isMac: () => false }));

import { WorktreeRemoveButton } from "./worktreeCleanup";
import { CardContextMenu } from "./views/sticker/CardContextMenu";
import { ChatSidebar } from "./views/ChatSidebar";
import { SessionAdmin } from "./views/chat/SessionAdmin";
import { formatBackendError } from "./i18n/errors";
import { zh } from "./i18n/zh";
import type { LiveSession, WorktreeStatusDto } from "./api";

const eligible: WorktreeStatusDto = {
  path: "C:/w/meowo-feat", branch: "feat/x", prNumber: 12, eligible: true, reason: null, count: null,
};
const dirty: WorktreeStatusDto = {
  path: "C:/w/meowo-feat", branch: "feat/x", prNumber: 12, eligible: false, reason: "dirty", count: 3,
};
const mainCheckout: WorktreeStatusDto = {
  path: "C:/w/meowo", branch: "main", prNumber: null, eligible: false, reason: "not_worktree", count: null,
};

/** 按命令名分派的 invoke 假实现；confirm_dialog 默认同意。 */
function mockInvoke(handlers: Record<string, (args: any) => unknown>) {
  invoke.mockImplementation((command: string, args: unknown) => {
    const h = handlers[command];
    if (h) return Promise.resolve(h(args));
    if (command === "confirm_dialog") return Promise.resolve(true);
    return Promise.resolve(undefined);
  });
}

const calls = (command: string) => invoke.mock.calls.filter(([c]) => c === command).map(([, a]) => a);

afterEach(cleanup);
beforeEach(() => {
  invoke.mockReset();
  localStorage.clear();
});

describe("撤销条上的「删除 worktree」按钮", () => {
  it("可清理时出现，点了先确认再删，删完换成结果文字", async () => {
    mockInvoke({
      worktree_status: () => eligible,
      remove_session_worktree: () => ({ removed: true, blockers: [], branchDeleted: true }),
    });
    const onEngage = vi.fn();
    const onSettled = vi.fn();
    render(<WorktreeRemoveButton t={zh} sessionId={7} className="b" onEngage={onEngage} onSettled={onSettled} />);
    fireEvent.click(await screen.findByRole("button", { name: "删除 worktree" }));
    expect(await screen.findByText("worktree 已删除")).toBeTruthy();
    expect(onEngage).toHaveBeenCalledTimes(1);
    expect(onSettled).toHaveBeenCalledTimes(1);
    const confirm = calls("confirm_dialog")[0] as { message: string };
    expect(confirm.message).toContain("C:/w/meowo-feat");
    expect(confirm.message).toContain("feat/x");
    expect(calls("remove_session_worktree")).toEqual([{ sessionId: 7, killProcesses: false }]);
  });

  it("不可清理时不出现", async () => {
    mockInvoke({ worktree_status: () => dirty });
    render(<WorktreeRemoveButton t={zh} sessionId={7} className="b" />);
    await waitFor(() => expect(calls("worktree_status")).toHaveLength(1));
    await act(async () => {});
    expect(screen.queryByRole("button", { name: "删除 worktree" })).toBeNull();
  });

  it("有程序占着目录：列出程序名再问一次，同意后才带 killProcesses 重试", async () => {
    let round = 0;
    mockInvoke({
      worktree_status: () => eligible,
      remove_session_worktree: () => (round++ === 0
        ? { removed: false, blockers: [{ pid: 4321, name: "node.exe" }], branchDeleted: false }
        : { removed: true, blockers: [], branchDeleted: true }),
    });
    render(<WorktreeRemoveButton t={zh} sessionId={7} className="b" />);
    fireEvent.click(await screen.findByRole("button", { name: "删除 worktree" }));
    expect(await screen.findByText("worktree 已删除")).toBeTruthy();
    const confirms = calls("confirm_dialog") as { message: string }[];
    expect(confirms).toHaveLength(2);
    expect(confirms[1].message).toContain("node.exe (4321)");
    expect(calls("remove_session_worktree")).toEqual([
      { sessionId: 7, killProcesses: false },
      { sessionId: 7, killProcesses: true },
    ]);
  });

  it("后端拒绝时显示原因而不是抛错", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "worktree_status") return Promise.resolve(eligible);
      if (command === "confirm_dialog") return Promise.resolve(true);
      if (command === "remove_session_worktree") return Promise.reject("worktree/busy");
      return Promise.resolve();
    });
    render(<WorktreeRemoveButton t={zh} sessionId={7} className="b" />);
    fireEvent.click(await screen.findByRole("button", { name: "删除 worktree" }));
    expect((await screen.findByRole("alert")).textContent).toContain("什么都没删");
  });
});

describe("已完成会话菜单里的「删除 worktree」", () => {
  const menu = (onWorktreeDone = vi.fn()) =>
    render(
      <CardContextMenu
        x={0} y={0} starred={false} hasNote={false} archived
        onStar={() => {}} onNote={() => {}} onRename={() => {}} onArchive={() => {}}
        onOpenDir={null} onEndSession={null}
        worktreeSessionId={9} onWorktreeDone={onWorktreeDone} onClose={() => {}}
      />,
    );

  it("不可清理时置灰并写明原因，点了不删", async () => {
    mockInvoke({ worktree_status: () => dirty });
    menu();
    const item = await screen.findByRole("menuitem", { name: "删除 worktree" });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByRole("note").textContent).toBe("还有 3 个文件改动没提交");
    fireEvent.click(item);
    expect(calls("remove_session_worktree")).toHaveLength(0);
  });

  it("可清理时点了走删除流程，结果交给宿主", async () => {
    mockInvoke({
      worktree_status: () => eligible,
      remove_session_worktree: () => ({ removed: true, blockers: [], branchDeleted: false }),
    });
    const done = vi.fn();
    menu(done);
    const item = await screen.findByRole("menuitem", { name: "删除 worktree" });
    await waitFor(() => expect(item.getAttribute("aria-disabled")).toBe("false"));
    fireEvent.click(item);
    await waitFor(() => expect(done).toHaveBeenCalledWith({ kind: "removed", branchDeleted: false }));
  });

  it("主检出这类根本不是 worktree 的，不出现这一项", async () => {
    mockInvoke({ worktree_status: () => mainCheckout });
    menu();
    await waitFor(() => expect(calls("worktree_status")).toHaveLength(1));
    await act(async () => {});
    expect(screen.queryByRole("menuitem", { name: /worktree/ })).toBeNull();
  });
});

describe("会话管理页的批量清理", () => {
  const row = (id: number, title: string) => ({
    session: { id, cc_session_id: `cc-${id}`, status: "ended", started_at: 1, last_event_at: 1, ended_at: null },
    project_name: "p", task_title: title, archived: true, archived_at: 1, cwd: "C:/w/x", todos: [],
  }) as unknown as LiveSession;

  it("先列出待删清单（会话名、目录、分支），确认后逐个删，部分失败时列出失败项", async () => {
    invoke.mockImplementation((command: string, args: any) => {
      switch (command) {
        case "recent_cwds": return Promise.resolve([]);
        case "get_session_admin_page": return Promise.resolve({ items: [row(1, "修侧栏"), row(2, "做官网")], total: 2 });
        case "list_cleanable_worktrees": return Promise.resolve([
          { sessionIds: [1], status: { ...eligible, path: "C:/w/a", branch: "fix/side" } },
          { sessionIds: [2], status: { ...eligible, path: "C:/w/b", branch: "docs/site" } },
          { sessionIds: [3], status: dirty },
        ]);
        case "confirm_dialog": return Promise.resolve(true);
        case "remove_session_worktree":
          return args.sessionId === 2
            ? Promise.reject("worktree/busy")
            : Promise.resolve({ removed: true, blockers: [], branchDeleted: true });
        default: return Promise.resolve();
      }
    });
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} initialStatus="done" />);
    fireEvent.click(await screen.findByRole("button", { name: "清理已合并 worktree" }));
    await waitFor(() => expect(calls("remove_session_worktree")).toHaveLength(2));

    const confirm = calls("confirm_dialog")[0] as { message: string };
    expect(confirm.message).toContain("修侧栏 — C:/w/a (fix/side)");
    expect(confirm.message).toContain("做官网 — C:/w/b (docs/site)");
    expect(confirm.message).toContain("另有 1 个不满足条件");
    // 清单整体确认过，不再逐个问（只有一次确认框）。
    expect(calls("confirm_dialog")).toHaveLength(1);
    const toast = await screen.findByText(/删除了 1 个，1 个失败/);
    expect(toast.textContent).toContain("C:/w/b");
    expect(toast.textContent).toContain("什么都没删");
  });

  it("没有可清理的就直接说，不弹确认", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "list_cleanable_worktrees") return Promise.resolve([{ sessionIds: [3], status: dirty }]);
      if (command === "get_session_admin_page") return Promise.resolve({ items: [], total: 0 });
      return Promise.resolve([]);
    });
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} initialStatus="done" />);
    fireEvent.click(await screen.findByRole("button", { name: "清理已合并 worktree" }));
    expect(await screen.findByText("没有可以清理的 worktree")).toBeTruthy();
    expect(calls("confirm_dialog")).toHaveLength(0);
  });

  it("不看「已完成」时不出现批量清理入口", async () => {
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_session_admin_page" ? { items: [], total: 0 } : []));
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
    await waitFor(() => expect(calls("get_session_admin_page").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button", { name: "清理已合并 worktree" })).toBeNull();
  });
});

describe("目录已删除", () => {
  it("侧栏条目标出「目录已删除」，点开不抛异常", async () => {
    const gone = {
      session: { id: 5, cc_session_id: "cc-5", status: "ended", last_event_at: 1 },
      project_name: "meowo", task_title: "旧功能", connected: false, pending_review: null,
      cwd: "C:/w/meowo-old", provider: "claude", archived: true, cwd_missing: true,
    } as unknown as LiveSession;
    invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "get_live_sessions_page" ? [gone] : undefined));
    const onSelect = vi.fn();
    render(<ChatSidebar activeId={0} approvalAwaitingIds={new Set()} onSelect={onSelect} onCollapse={() => {}} />);
    expect(await screen.findByTestId("sidebar-dir-missing")).toBeTruthy();
    expect(screen.getByTestId("sidebar-dir-missing").getAttribute("data-tip")).toBe(zh.worktree.dirMissingTip);
    fireEvent.click(screen.getByRole("button", { name: /旧功能/ }));
    expect(onSelect).toHaveBeenCalledWith(5);
  });

  it("恢复会话时目录不存在，报错映射成人话", () => {
    expect(formatBackendError("目录不存在", "zh-CN")).toBe("项目目录不存在");
    expect(formatBackendError("目录不存在", "en-US")).toBe("Project directory not found");
  });
});
