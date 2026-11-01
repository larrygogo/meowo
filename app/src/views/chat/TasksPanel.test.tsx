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
