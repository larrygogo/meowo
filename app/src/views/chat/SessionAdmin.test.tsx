import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { SessionAdmin } from "./SessionAdmin";
import type { LiveSession } from "../../api";

const row = (id: number, title: string, extra: Partial<LiveSession> = {}): LiveSession => ({
  session: { id, cc_session_id: `cc-${id}`, status: "ended", started_at: 1, last_event_at: Date.UTC(2026, 10, 1, 8, 30), ended_at: null },
  project_name: "p", task_title: title, current_activity: null, column: "todo", todo_done: 0, todo_total: 0, todos: [],
  pid: null, archived: false, archived_at: null, cwd: "C:/w/meowo", context_pct: null, context_window: null, model: null,
  pending_review: null, last_ai_text: null, last_user_text: `${title}的最后一句`, provider: "claude", profile: null,
  predecessor_id: null, ...extra,
} as LiveSession);

type Query = { workspaces: string[]; status: string | null; updatedBeforeDays: number | null; page: number; pageSize: number };

describe("会话管理页（对照 Kimi Code）", () => {
  let queries: Query[];
  let archivedCalls: { sessionId: number; archived: boolean }[];
  beforeEach(() => {
    queries = [];
    archivedCalls = [];
    invoke.mockReset();
    invoke.mockImplementation((command: string, args: unknown) => {
      if (command === "recent_cwds") return Promise.resolve(["C:/w/meowo", "C:/w/reverse-bot-gui"]);
      if (command === "set_archived") { archivedCalls.push(args as { sessionId: number; archived: boolean }); return Promise.resolve(); }
      if (command === "get_session_admin_page") {
        const q = args as Query;
        queries.push(q);
        // 共 45 条：20 条一页。
        const all = Array.from({ length: 45 }, (_, i) => row(i + 1, `会话${i + 1}`, i === 0 ? { archived: true, archived_at: Date.UTC(2026, 9, 30, 9, 0) } : {}));
        return Promise.resolve({ items: all.slice(q.page * q.pageSize, (q.page + 1) * q.pageSize), total: 45 });
      }
      return Promise.resolve();
    });
  });
  afterEach(cleanup);

  it("默认查第一页，表格列出状态、标题、工作区、最后一条 prompt，可翻页", async () => {
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
    expect(await screen.findByText("会话2")).toBeTruthy();
    expect(queries[0]).toEqual({ workspaces: [], status: null, updatedBeforeDays: null, page: 0, pageSize: 20 });
    expect(screen.getByText("共 45 条")).toBeTruthy();
    const first = screen.getByText("会话1").closest("tr")!;
    expect(within(first).getByText("已完成")).toBeTruthy();
    expect(within(first).getByText("meowo")).toBeTruthy();
    expect(within(first).getByText("会话1的最后一句")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "3" }));
    expect(await screen.findByText("会话41")).toBeTruthy();
    expect(queries[queries.length - 1].page).toBe(2);
  });

  it("从新建页「查看更多」进来时预选那个工作区", async () => {
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} initialWorkspaces={["C:/w/meowo"]} />);
    expect(await screen.findByText("会话2")).toBeTruthy();
    expect(queries[0].workspaces).toEqual(["C:/w/meowo"]);
  });

  it("筛选改完点「查询」才生效：工作空间多选、状态、N 天以前", async () => {
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
    await screen.findByText("会话2");
    fireEvent.click(screen.getByRole("button", { name: "工作区" }));
    fireEvent.click(await screen.findByRole("option", { name: "reverse-bot-gui" }));
    fireEvent.click(screen.getByRole("button", { name: "状态" }));
    fireEvent.click(await screen.findByRole("option", { name: "进行中" }));
    fireEvent.click(screen.getByRole("button", { name: "更新时间" }));
    fireEvent.click(await screen.findByRole("option", { name: "7 天以前" }));
    expect(queries).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "查询" }));
    await waitFor(() => expect(queries[queries.length - 1]).toEqual({ workspaces: ["C:/w/reverse-bot-gui"], status: "open", updatedBeforeDays: 7, page: 0, pageSize: 20 }));
    fireEvent.click(screen.getByRole("button", { name: "重置" }));
    await waitFor(() => expect(queries[queries.length - 1]).toEqual({ workspaces: [], status: null, updatedBeforeDays: null, page: 0, pageSize: 20 }));
  });

  it("勾选后批量标记完成，提示里的「撤销」把它们恢复", async () => {
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
    await screen.findByText("会话2");
    fireEvent.click(screen.getByRole("checkbox", { name: "会话2" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "会话3" }));
    expect(screen.getByText("已选 2 项")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "标记完成（2）" }));
    await screen.findByText("已标记完成 2 个会话");
    expect(archivedCalls).toEqual([{ sessionId: 2, archived: true }, { sessionId: 3, archived: true }]);
    fireEvent.click(screen.getByRole("button", { name: "撤销" }));
    await waitFor(() => expect(archivedCalls.slice(2)).toEqual([{ sessionId: 2, archived: false }, { sessionId: 3, archived: false }]));
  });

  it("全选本页后可选中当前条件下的全部，逐页拉齐 id", async () => {
    render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
    await screen.findByText("会话2");
    fireEvent.click(screen.getByRole("checkbox", { name: "全选本页" }));
    expect(screen.getByText("已选 20 项")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "选中当前条件下的全部 45 项" }));
    expect(await screen.findByText("已选中全部 45 项")).toBeTruthy();
    // 第 1 条已完成，其余 44 条进行中：两个批量按钮分别计数。
    expect(screen.getByRole("button", { name: "标记完成（44）" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "恢复进行中（1）" })).toBeTruthy();
  });

  it("行内「打开会话」与「返回」交给对话窗", async () => {
    const onOpen = vi.fn();
    const onBack = vi.fn();
    render(<SessionAdmin onBack={onBack} onOpen={onOpen} />);
    const target = (await screen.findByText("会话2")).closest("tr")!;
    fireEvent.click(within(target).getByRole("button", { name: "打开会话" }));
    expect(onOpen).toHaveBeenCalledWith(2);
    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(onBack).toHaveBeenCalled();
  });
});
