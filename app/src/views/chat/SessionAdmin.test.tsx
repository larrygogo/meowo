import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

  // 走查 2026-11-12：S-1（标记完成会结束在跑的会话，先确认）、S-10（撤销提示读完前不消失）、#14（失败带原因、可关、可重试）。
  // 兄弟场景：侧栏单条/批量见 ChatSidebar.actions.test.tsx。
  describe("走查修复", () => {
    let confirms: string[];
    let answer: boolean;
    const running = (id: number) => row(id, `会话${id}`, { connected: true, session: { id, cc_session_id: `cc-${id}`, status: "running", started_at: 1, last_event_at: 1, ended_at: null } } as Partial<LiveSession>);
    const withPage = (items: LiveSession[], extra?: (command: string, args: unknown) => unknown) => {
      confirms = [];
      answer = true;
      invoke.mockImplementation((command: string, args: unknown) => {
        if (command === "confirm_dialog") { confirms.push(String((args as { message: string }).message)); return Promise.resolve(answer); }
        if (command === "set_archived") { archivedCalls.push(args as { sessionId: number; archived: boolean }); return Promise.resolve(extra?.(command, args)); }
        if (command === "get_session_admin_page") return Promise.resolve({ items, total: items.length });
        if (command === "recent_cwds") return Promise.resolve([]);
        return Promise.resolve(extra?.(command, args));
      });
    };

    it("S-1 批量里有在跑的先确认，取消则不动；单条在跑的同样确认", async () => {
      withPage([running(1), row(2, "会话2")]);
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      await screen.findByText("会话2");
      fireEvent.click(screen.getByRole("checkbox", { name: "会话1" }));
      fireEvent.click(screen.getByRole("checkbox", { name: "会话2" }));
      answer = false;
      fireEvent.click(screen.getByRole("button", { name: "标记完成（2）" }));
      await waitFor(() => expect(confirms).toEqual(["其中 1 个还在运行，标记完成会结束它们。"]));
      await new Promise((r) => setTimeout(r, 10));
      expect(archivedCalls).toEqual([]);
      answer = true;
      const first = screen.getByText("会话1").closest("tr")!;
      fireEvent.click(within(first).getByRole("button", { name: "标记完成" }));
      await waitFor(() => expect(archivedCalls).toEqual([{ sessionId: 1, archived: true }]));
      expect(confirms[1]).toBe("Agent 还在运行，标记完成会结束它。");
    });

    // 做空第一轮 #6（2026-11-12）：勾选时记下的「是否在跑」会过期。勾选那刻空闲、点批量前开跑了，
    // 原先按勾选快照判定，不弹确认就结束了进程。改成执行前按 id 现取。单条按钮同理（表格行也是快照）。
    it("#6 勾选后会话开跑：批量与单条标记完成都按现取的状态确认", async () => {
      const items = [row(1, "会话1"), row(2, "会话2")];
      withPage(items);
      // 每次返回一份新数组：页面手里的行不随后端变化（真实后端也是每次新对象）。
      const base = invoke.getMockImplementation()!;
      invoke.mockImplementation((command: string, args: unknown) =>
        command === "get_session_admin_page" ? Promise.resolve({ items: [...items], total: items.length }) : base(command, args));
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      await screen.findByText("会话2");
      fireEvent.click(screen.getByRole("checkbox", { name: "会话1" }));
      fireEvent.click(screen.getByRole("checkbox", { name: "会话2" }));
      // 勾完之后会话 2 开跑（后端此刻的状态），页面上的行还是旧的。
      items[1] = running(2);
      answer = false;
      fireEvent.click(screen.getByRole("button", { name: "标记完成（2）" }));
      await waitFor(() => expect(confirms).toEqual(["其中 1 个还在运行，标记完成会结束它们。"]));
      await new Promise((r) => setTimeout(r, 10));
      expect(archivedCalls).toEqual([]);
      const second = screen.getByText("会话2").closest("tr")!;
      fireEvent.click(within(second).getByRole("button", { name: "标记完成" }));
      await waitFor(() => expect(confirms[1]).toBe("Agent 还在运行，标记完成会结束它。"));
      await new Promise((r) => setTimeout(r, 10));
      expect(archivedCalls).toEqual([]);
    });

    it("#6 现取失败按在跑处理，宁可多问一次", async () => {
      let calls = 0;
      withPage([row(2, "会话2")]);
      const base = invoke.getMockImplementation()!;
      invoke.mockImplementation((command: string, args: unknown) => {
        if (command === "get_session_admin_page" && ++calls > 1) return Promise.reject(new Error("boom"));
        return base(command, args);
      });
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      const target = (await screen.findByText("会话2")).closest("tr")!;
      answer = false;
      fireEvent.click(within(target).getByRole("button", { name: "标记完成" }));
      await waitFor(() => expect(confirms).toEqual(["Agent 还在运行，标记完成会结束它。"]));
      expect(archivedCalls).toEqual([]);
    });

    it("S-1 不在跑的不弹确认", async () => {
      withPage([row(2, "会话2")]);
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      const target = (await screen.findByText("会话2")).closest("tr")!;
      fireEvent.click(within(target).getByRole("button", { name: "标记完成" }));
      await waitFor(() => expect(archivedCalls).toEqual([{ sessionId: 2, archived: true }]));
      expect(confirms).toEqual([]);
    });

    it("S-10 批量完成后焦点交给「撤销」，焦点在条内时 10 秒后仍在", async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      try {
        withPage([row(2, "会话2"), row(3, "会话3")]);
        render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
        await screen.findByText("会话2");
        fireEvent.click(screen.getByRole("checkbox", { name: "会话2" }));
        fireEvent.click(screen.getByRole("button", { name: "标记完成（1）" }));
        await screen.findByText("已标记完成 1 个会话");
        await act(async () => { vi.advanceTimersByTime(10); });
        const undo = screen.getByRole("button", { name: "撤销" });
        expect(document.activeElement).toBe(undo);
        fireEvent.focus(undo);
        await act(async () => { vi.advanceTimersByTime(10_000); });
        expect(screen.getByText("已标记完成 1 个会话")).toBeTruthy();
        // 焦点移出后计满 8 秒才收起。
        fireEvent.blur(undo, { relatedTarget: document.body });
        await act(async () => { vi.advanceTimersByTime(7_000); });
        expect(screen.getByText("已标记完成 1 个会话")).toBeTruthy();
        await act(async () => { vi.advanceTimersByTime(1_500); });
        expect(screen.queryByText("已标记完成 1 个会话")).toBeNull();
      } finally {
        vi.useRealTimers();
      }
    });

    // 做空第二轮 N5（2026-11-12）：确认前要先走网络现取状态，原先没有进行中锁，连点两下弹两个确认、
    // 写两遍；单条按钮也逐页拉全部进行中会话。兄弟场景：批量「标记完成」「恢复进行中」与行内单条按钮共用一把锁。
    it("N5 进行中连点不重入，按钮标 aria-disabled 但焦点留在原处；现取只查目标所在工作区", async () => {
      withPage([row(2, "会话2", { cwd: "C:/w/reverse-bot-gui" }), row(3, "会话3")]);
      const base = invoke.getMockImplementation()!;
      const fetched: Query[] = [];
      let release: () => void = () => {};
      invoke.mockImplementation((command: string, args: unknown) => {
        if (command === "get_session_admin_page" && (args as Query).status === "open") {
          fetched.push(args as Query);
          return new Promise((resolve) => { release = () => resolve(base(command, args)); });
        }
        return base(command, args);
      });
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      const target = (await screen.findByText("会话2")).closest("tr")!;
      const button = within(target).getByRole("button", { name: "标记完成" });
      button.focus();
      fireEvent.click(button);
      await waitFor(() => expect(fetched).toHaveLength(1));
      expect(fetched[0].workspaces).toEqual(["C:/w/reverse-bot-gui"]);
      expect(button.getAttribute("aria-disabled")).toBe("true");
      expect(document.activeElement).toBe(button);
      fireEvent.click(button);
      fireEvent.click(within(screen.getByText("会话3").closest("tr")!).getByRole("button", { name: "标记完成" }));
      await new Promise((r) => setTimeout(r, 10));
      expect(fetched).toHaveLength(1);
      release();
      await waitFor(() => expect(archivedCalls).toEqual([{ sessionId: 2, archived: true }]));
      await new Promise((r) => setTimeout(r, 10));
      expect(archivedCalls).toHaveLength(1);
      await waitFor(() => expect(button.getAttribute("aria-disabled")).toBeNull());
    });

    it("#14 失败提示带原因、不自动消失、× 关闭；加载失败给重试", async () => {
      withPage([row(2, "会话2")], (command) => { if (command === "set_archived") throw new Error("会话不存在"); });
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      const target = (await screen.findByText("会话2")).closest("tr")!;
      fireEvent.click(within(target).getByRole("button", { name: "标记完成" }));
      const alert = await screen.findByRole("alert");
      expect(alert.textContent).toContain("1 个失败");
      expect(alert.textContent).toContain("会话不存在");
      await new Promise((r) => setTimeout(r, 50));
      fireEvent.click(within(alert).getByRole("button", { name: "关闭" }));
      expect(screen.queryByRole("alert")).toBeNull();
      cleanup();

      let fail = true;
      invoke.mockImplementation((command: string) => {
        if (command === "get_session_admin_page") return fail ? Promise.reject(new Error("x")) : Promise.resolve({ items: [row(5, "会话5")], total: 1 });
        return Promise.resolve([]);
      });
      render(<SessionAdmin onBack={() => {}} onOpen={() => {}} />);
      const retry = await screen.findByRole("button", { name: "重试" });
      fail = false;
      fireEvent.click(retry);
      expect(await screen.findByText("会话5")).toBeTruthy();
    });
  });
});
