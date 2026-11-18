import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";
import { collectBackgroundTasks, filterTasks, hasTaskItems, TasksDockPanel, type BackgroundTask } from "./TasksPanel";

function shell(id: string, command: string, outcome: { running: number; failed?: number } | null, times?: [string, string]): ChatItem[] {
  const use: ChatItem = {
    type: "tool_use", id, timestamp: times?.[0] ?? null, name: "Bash", kind: "run", summary: command,
    detail: { kind: "command", command, description: `说明 ${id}` },
  };
  if (!outcome) return [use];
  return [use, {
    type: "tool_result", id: `r-${id}`, timestamp: times?.[1] ?? null, tool_use_id: id, text: "started", is_error: false,
    subagent: { running: outcome.running, completed: outcome.running === 0 ? 1 : 0, failed: outcome.failed ?? 0, task_id: `task-${id}` },
  }];
}

const ordinaryItems: ChatItem[] = [
  { type: "tool_use", id: "read1", timestamp: null, name: "Read", kind: "read", summary: "src/a.ts" },
  { type: "tool_result", id: "rr1", timestamp: null, tool_use_id: "read1", text: "ok", is_error: false },
];

afterEach(cleanup);

describe("后台任务", () => {
  it("只收真正的后台 Bash：说明做标题、命令做次要信息、按回执定状态与耗时", () => {
    const items = [...shell("a", "npm run dev", { running: 1 }), ...shell("b", "cargo test", { running: 0 }, ["2026-10-29T10:00:00Z", "2026-10-29T10:01:20Z"]), ...ordinaryItems];
    expect(hasTaskItems(ordinaryItems)).toBe(false);
    const { bash, subagents } = collectBackgroundTasks(items);
    expect(subagents).toEqual([]);
    expect(bash.map((task) => [task.name, task.meta, task.state, task.durationMs])).toEqual([
      ["说明 a", "$ npm run dev", "run", null],
      ["说明 b", "$ cargo test", "done", 80_000],
    ]);
  });

  it("子代理完成后被 SendMessage 续上：变回运行中，下一条完成通知再收尾", () => {
    // 回执形态照 CC 2.1.317 实拍：续上回执挂在 SendMessage 自己的调用上，靠 task_id
    // 归回最初那次委派；之后的完成通知仍挂在最初的委派上。
    const use: ChatItem = {
      type: "tool_use", id: "toolu_a", timestamp: "2026-11-03T06:20:00Z", name: "Agent", kind: "agent", summary: "rs 导出 schema",
      subagent: { description: "rs 导出 schema", agent_type: "general-purpose", count: 1 },
    };
    const launched: ChatItem = { type: "tool_result", id: "r1", timestamp: "2026-11-03T06:20:01Z", tool_use_id: "toolu_a", text: "Async agent launched", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
    const done = (id: string, ts: string): ChatItem => ({ type: "tool_result", id, timestamp: ts, tool_use_id: "toolu_a", text: "done", is_error: false, subagent: { running: 0, completed: 1, failed: 0 } });
    const send: ChatItem = { type: "tool_use", id: "toolu_s", timestamp: "2026-11-03T06:38:52Z", name: "SendMessage", kind: "other", summary: "纠正字段" };
    const resumed: ChatItem = { type: "tool_result", id: "r3", timestamp: "2026-11-03T06:38:52Z", tool_use_id: "toolu_s", text: "Resuming agent a87", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
    const states = (items: ChatItem[]) => collectBackgroundTasks(items).subagents.map((task) => [task.id, task.state]);

    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z")])).toEqual([["toolu_a", "done"]]);
    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z"), send, resumed])).toEqual([["toolu_a", "run"]]);
    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z"), send, resumed, done("r4", "2026-11-03T06:45:00Z")])).toEqual([["toolu_a", "done"]]);
  });

  // 实拍 2026-11-18（本机 transcript 解析结果，脱敏）：子代理 04:01 派出，之后被 SendMessage 续上四次，
  // 最近一次 05:25:33 续上、05:29:20 收工。对话页首屏只载入末尾 200 条，最初那次委派（和它的启动回执）
  // 不在已载入的窗口里：续上回执的 task_id 找不到归属，面板里看不到它在跑（终端里两个子代理在跑，面板
  // 「1 运行中」）。找不到最初委派时，就由续上它的那条 SendMessage 代表这个子代理。
  it("最初的委派不在已载入窗口里：SendMessage 续上的子代理照样算运行中，收工后算完成", () => {
    const send: ChatItem = {
      type: "tool_use", id: "toolu_01Wm61", timestamp: "2026-11-18T05:25:32.636Z", name: "SendMessage", kind: "other",
      summary: JSON.stringify({ to: "acb4b19", message: "CI 全绿，请做只读真机核对" }),
    };
    const resumed: ChatItem = {
      type: "tool_result", id: "441993b6:0", timestamp: "2026-11-18T05:25:33.178Z", tool_use_id: "toolu_01Wm61",
      text: "{\"success\":true,\"message\":\"Resuming agent acb4b19\",\"resumedAgentId\":\"acb4b19\"}", is_error: false,
      subagent: { running: 1, completed: 0, failed: 0, task_id: "acb4b19" },
    };
    const finished: ChatItem = {
      type: "tool_result", id: "queued-notice:2026-11-18T05:29:20.098Z", timestamp: "2026-11-18T05:29:20.098Z", tool_use_id: "toolu_01Wm61",
      text: "Agent \"Implement GitHub connection feature\" finished", is_error: false,
      subagent: { running: 0, completed: 1, failed: 0, task_id: "acb4b19" },
    };
    const live = { activity: "idle" as const, alive: true };
    const running = collectBackgroundTasks([send, resumed], live).subagents;
    expect(running.map((task) => [task.id, task.name, task.state, task.probeId])).toEqual([["toolu_01Wm61", "CI 全绿，请做只读真机核对", "run", "acb4b19"]]);
    // 运行时长从这一次续上算起（本轮），不是从最初派出算。
    expect(running[0].startedAt).toBe(Date.parse("2026-11-18T05:25:33.178Z"));
    const done = collectBackgroundTasks([send, resumed, finished], live).subagents;
    expect(done.map((task) => [task.state, task.durationMs])).toEqual([["done", 227_000 - 80]]);
  });

  // 实拍 2026-11-18（本会话 transcript 解析结果，脱敏）：子代理 a9441de 04:39 派出，05:48 被续上，
  // 06:15 收工（完成通知挂在 05:48 那次续上的调用 id 上），06:49 又被续上。首屏窗口从 05:48 之后开始：
  // 窗口里第一条带这个 task_id 的是那条完成通知，它的 tool_use_id 指向窗口外的调用——此前结局归到了
  // 这个看不见的 id 上，06:49 的续上找不到行，面板「1 运行中」里没有它。名称也不能是参数 JSON。
  it("续上它的那次委派与上一次续上都在窗口外：按子代理归并成一行，算运行中，名称取参数里的话", () => {
    const notice: ChatItem = {
      type: "tool_result", id: "queued-notice:2026-11-18T06:15:37.956Z", timestamp: "2026-11-18T06:15:37.956Z", tool_use_id: "toolu_019uC",
      text: "Agent \"会话状态修复\" finished", is_error: false, subagent: { running: 0, completed: 1, failed: 0, task_id: "a9441de" },
    };
    const send = (id: string, ts: string, args: string): ChatItem => ({ type: "tool_use", id, timestamp: ts, name: "SendMessage", kind: "other", summary: args });
    const resumed = (id: string, ts: string): ChatItem => ({
      type: "tool_result", id: `${id}:0`, timestamp: ts, tool_use_id: id, is_error: false,
      text: "{\"success\":true,\"message\":\"Resuming agent a9441de\",\"resumedAgentId\":\"a9441de\"}",
      subagent: { running: 1, completed: 0, failed: 0, task_id: "a9441de" },
    });
    // 摘要是截断过的参数 JSON（实拍形状），整段 JSON.parse 会失败。
    const truncated = "{\"to\":\"a9441de\",\"message\":\"新现象：用户装的是含你全部修复的本地集成包\\n仍出现「主会话空闲、对话页显示工作中」…";
    const items = [notice, send("toolu_01GBz", "2026-11-18T06:49:31.056Z", truncated), resumed("toolu_01GBz", "2026-11-18T06:49:31.226Z")];
    const live = { activity: "idle" as const, alive: true };
    expect(collectBackgroundTasks(items, live).subagents.map((task) => [task.id, task.name, task.state, task.probeId, task.resumed]))
      .toEqual([["toolu_01GBz", "新现象：用户装的是含你全部修复的本地集成包", "run", "a9441de", true]]);

    // 同一个子代理在窗口里被续上两次：只一行，状态取最近一次，由最近一次续上的调用代表。
    const twice = [
      send("toolu_A", "2026-11-18T05:48:05.205Z", "{\"to\":\"a9441de\",\"summary\":\"继续修会话状态\",\"message\":\"会话中断过\"}"), resumed("toolu_A", "2026-11-18T05:48:05.453Z"),
      { ...notice, tool_use_id: "toolu_A" },
      ...items.slice(1),
    ];
    const rows = collectBackgroundTasks(twice, live).subagents;
    expect(rows.map((task) => [task.id, task.state])).toEqual([["toolu_01GBz", "run"]]);
    expect(rows[0].startedAt).toBe(Date.parse("2026-11-18T06:49:31.226Z"));
  });

  // 用户要求（2026-11-18）：运行中的子代理行实时显示已运行时间与已用 token（照终端
  // 「2m 3s · ↓ 387.8k tokens」），完成的行也显示总 token；面板关着或没有运行中的任务时不起定时器。
  it("子代理面板：运行中的行每秒走表并显示 token，完成的行显示总时长与总 token", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    try {
      vi.setSystemTime(Date.parse("2026-11-18T05:27:36Z"));
      const tasks: BackgroundTask[] = [
        { id: "toolu_run", name: "核对 SourceControl.tsx", meta: null, state: "run", durationMs: null, sortKey: "2", startedAt: Date.parse("2026-11-18T05:25:33Z"), probeId: "acb4b19" },
        { id: "toolu_done", name: "独立审查", meta: null, state: "done", durationMs: 30_000, sortKey: "1" },
      ];
      const probe = vi.fn(async (ids: string[]) => {
        expect(ids).toEqual(["acb4b19", "toolu_done"]);
        return { acb4b19: { tokens: 540_812 }, toolu_done: { tokens: 26_548 } };
      });
      render(<TasksDockPanel title="子代理" icon={<span />} tasks={tasks} onClose={() => {}} probe={probe} />);
      await act(async () => { await Promise.resolve(); });
      const row = (name: string) => screen.getByText(name).closest("li")!.querySelector(".chat-task-time")!.textContent;
      expect(row("核对 SourceControl.tsx")).toBe("2 分 3 秒 · ↓ 540.8k tokens");
      expect(row("独立审查")).toBe("30 秒 · ↓ 26.5k tokens");
      act(() => { vi.advanceTimersByTime(1000); });
      expect(row("核对 SourceControl.tsx")).toBe("2 分 4 秒 · ↓ 540.8k tokens");
    } finally {
      vi.useRealTimers();
    }
  });

  // 用户截图：续上的子代理行名称显示成原始参数 JSON。侧车元数据给得出最初委派的说明时用它（与终端
  // 子代理行同名），否则用续上时那句话；普通委派不受影响。
  it("续上的子代理：名称换成侧车元数据里最初委派的说明", async () => {
    const tasks: BackgroundTask[] = [
      { id: "toolu_s", name: "CI 全绿，请做只读真机核对", meta: null, state: "done", durationMs: 1_000, sortKey: "2", probeId: "acb4b19", resumed: true },
      { id: "toolu_a", name: "跑后端测试", meta: null, state: "done", durationMs: 1_000, sortKey: "1", probeId: "toolu_a" },
    ];
    render(<TasksDockPanel title="子代理" icon={<span />} tasks={tasks} onClose={() => {}} probe={async () => ({ acb4b19: { description: "Implement GitHub connection feature" }, toolu_a: { description: "别的说明" } })} />);
    expect(await screen.findByText("Implement GitHub connection feature")).toBeTruthy();
    expect(screen.queryByText("CI 全绿，请做只读真机核对")).toBeNull();
    expect(screen.getByText("跑后端测试")).toBeTruthy();
  });

  it("没有运行中的任务时不起走表定时器；拿不到 token 的不显示数字", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    try {
      const tasks: BackgroundTask[] = [{ id: "d", name: "已完成的", meta: null, state: "done", durationMs: 5_000, sortKey: "1" }];
      render(<TasksDockPanel title="子代理" icon={<span />} tasks={tasks} onClose={() => {}} probe={async () => ({})} />);
      await act(async () => { await Promise.resolve(); });
      expect(vi.getTimerCount()).toBe(0);
      expect(screen.getByText("已完成的").closest("li")!.querySelector(".chat-task-time")!.textContent).toBe("5 秒");
    } finally {
      vi.useRealTimers();
    }
  });

  it("「最近」= 全部运行中 + 最近结束的 5 条，结束的按完成时刻倒序", () => {
    const task = (id: string, state: BackgroundTask["state"], sortKey: string): BackgroundTask => ({ id, name: id, meta: null, state, durationMs: null, sortKey });
    const tasks = [task("old", "done", "01"), task("run", "run", "02"), ...["03", "04", "05", "06", "07"].map((key) => task(key, "done", key)), task("bad", "fail", "08")];
    expect(filterTasks(tasks, "recent").map((item) => item.id)).toEqual(["run", "bad", "07", "06", "05", "04"]);
    expect(filterTasks(tasks, "running").map((item) => item.id)).toEqual(["run"]);
    expect(filterTasks(tasks, "done")).toHaveLength(7);
    expect(filterTasks(tasks, "all")).toHaveLength(8);
  });

  it("面板头显示运行数，切到「已完成」只剩结束的任务", () => {
    const { bash } = collectBackgroundTasks([...shell("a", "npm run dev", { running: 1 }), ...shell("b", "cargo test", { running: 0, failed: 1 })]);
    render(<TasksDockPanel title="后台 Bash" icon={<span />} tasks={bash} onClose={() => {}} />);
    expect(screen.getByText("1 运行中")).toBeTruthy();
    expect(screen.getByText("说明 a")).toBeTruthy();
    fireEvent.click(screen.getByRole("tab", { name: "已完成" }));
    expect(screen.queryByText("说明 a")).toBeNull();
    expect(screen.getByRole("img", { name: "失败" })).toBeTruthy();
  });
});
