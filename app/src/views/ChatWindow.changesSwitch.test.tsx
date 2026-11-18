// 2026-11-18 用户：「每次切换会话，更改页都会闪烁一次」。换会话时「更改」页原先先清空再等后端重查，
// 中间露一下空白。现在按会话记住上一次的清单：切回来立刻显示它自己的旧结果，后台照常刷新。
import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { savePanel } from "./chat/panelTabs";

const file = (path: string) => ({ path, status: "M", additions: 1, deletions: 0, indexStatus: null, worktreeStatus: "M" });
const summary = (files: ReturnType<typeof file>[]) => ({ gitAvailable: true, isRepo: true, branch: "main", ahead: null, behind: null, files });
const repo = (cwd: string, path: string) => ({ root: cwd, name: cwd.split("/").pop(), isCwdRepo: true, summary: summary([file(path)]), base: null, baseLabel: null, committed: [] });

describe("切换会话时「更改」页不闪", () => {
  it("切回去时立刻显示那个会话上次的改动，不等后端重查", async () => {
    window.history.replaceState({}, "", "/?sessionId=7");
    for (const id of [7, 42]) savePanel(id, { tabs: [{ key: "diff", type: "diff" }], active: "diff", visible: true, expanded: false });
    const cwdOf = (id: number) => (id === 7 ? "C:/a" : "C:/b");
    // 切回会话 7 之前置位：之后的摘要和清单请求都一直不回，屏幕上能看到的只可能来自缓存。
    let hang = false;
    invoke.mockImplementation((command: string, args: { sessionId?: number; cwd?: string }) => {
      if (command === "get_chat_history") {
        const id = args?.sessionId ?? 7;
        return Promise.resolve({ sessionId: id, title: `会话 ${id}`, status: "waiting", provider: "claude", cwd: cwdOf(id), supported: true, offset: 0, reset: false, pendingReview: null, items: [] });
      }
      if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
      if (command === "git_diff_summary") {
        if (hang) return new Promise(() => {});
        return Promise.resolve(summary([file(args?.cwd === "C:/a" ? "a-file.ts" : "b-file.ts")]));
      }
      if (command === "git_changes_overview") {
        if (hang) return new Promise(() => {});
        return Promise.resolve([repo(args!.cwd!, args?.cwd === "C:/a" ? "a-file.ts" : "b-file.ts")]);
      }
      if (command === "get_live_sessions_page") {
        return Promise.resolve([
          { session: { id: 7, cc_session_id: "a", status: "running" }, project_name: "p", task_title: "第一条", connected: true, pending_review: null, provider: "claude", cwd: "C:/a" },
          { session: { id: 42, cc_session_id: "b", status: "running" }, project_name: "p", task_title: "第二条", connected: true, pending_review: null, provider: "claude", cwd: "C:/b" },
        ]);
      }
      return Promise.resolve();
    });
    render(<ChatWindow />);
    await screen.findByText(/a-file\.ts/);
    fireEvent.click(await screen.findByRole("button", { name: /第二条/ }));
    await screen.findByText(/b-file\.ts/);
    expect(screen.queryByText(/a-file\.ts/)).toBeNull();
    hang = true;
    fireEvent.click(screen.getByRole("button", { name: /第一条/ }));
    await waitFor(() => expect(screen.queryByText(/b-file\.ts/)).toBeNull());
    expect(screen.getByText(/a-file\.ts/)).toBeTruthy();
  });
});
