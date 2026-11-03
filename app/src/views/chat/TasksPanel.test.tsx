import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { collectBackgroundTasks, filterTasks, hasTaskItems, TasksDockPanel, type BackgroundTask } from "./TasksPanel";

function shell(id: string, command: string, outcome: { running: number; failed?: number } | null, times?: [string, string]): ChatItem[] {
  const use: ChatItem = {
    type: "tool_use", id, timestamp: times?.[0] ?? null, name: "Bash", summary: command,
    detail: { kind: "command", command, description: `说明 ${id}` },
  };
  if (!outcome) return [use];
  return [use, {
    type: "tool_result", id: `r-${id}`, timestamp: times?.[1] ?? null, tool_use_id: id, text: "started", is_error: false,
    subagent: { running: outcome.running, completed: outcome.running === 0 ? 1 : 0, failed: outcome.failed ?? 0, task_id: `task-${id}` },
  }];
}

const ordinaryItems: ChatItem[] = [
  { type: "tool_use", id: "read1", timestamp: null, name: "Read", summary: "src/a.ts" },
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
      type: "tool_use", id: "toolu_a", timestamp: "2026-11-03T06:20:00Z", name: "Agent", summary: "rs 导出 schema",
      subagent: { description: "rs 导出 schema", agent_type: "general-purpose", count: 1 },
    };
    const launched: ChatItem = { type: "tool_result", id: "r1", timestamp: "2026-11-03T06:20:01Z", tool_use_id: "toolu_a", text: "Async agent launched", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
    const done = (id: string, ts: string): ChatItem => ({ type: "tool_result", id, timestamp: ts, tool_use_id: "toolu_a", text: "done", is_error: false, subagent: { running: 0, completed: 1, failed: 0 } });
    const send: ChatItem = { type: "tool_use", id: "toolu_s", timestamp: "2026-11-03T06:38:52Z", name: "SendMessage", summary: "纠正字段" };
    const resumed: ChatItem = { type: "tool_result", id: "r3", timestamp: "2026-11-03T06:38:52Z", tool_use_id: "toolu_s", text: "Resuming agent a87", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
    const states = (items: ChatItem[]) => collectBackgroundTasks(items).subagents.map((task) => [task.id, task.state]);

    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z")])).toEqual([["toolu_a", "done"]]);
    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z"), send, resumed])).toEqual([["toolu_a", "run"]]);
    expect(states([use, launched, done("r2", "2026-11-03T06:36:00Z"), send, resumed, done("r4", "2026-11-03T06:45:00Z")])).toEqual([["toolu_a", "done"]]);
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
