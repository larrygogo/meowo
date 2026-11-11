import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { zh } from "../../i18n/zh";
import { outcomeBadge } from "./SubagentBlock";
import { collectBackgroundTasks, filterTasks, TasksPanel } from "./TasksPanel";

// 子代理「is waiting on its own background work」（做空第三轮 #7，coordinator 11-11 定）：后端给出
// waiting=1 的结局，徽标单独显示「等待后台」——不是完成，也不是运行中的转圈；之后的 finished 再变完成。

afterEach(cleanup);

const use: ChatItem = {
  type: "tool_use", id: "toolu_a", timestamp: "2026-11-11T05:00:00Z", name: "Agent", summary: "查误报",
  subagent: { description: "查误报", agent_type: "general-purpose", count: 1 },
};
const launched: ChatItem = { type: "tool_result", id: "r1", timestamp: "2026-11-11T05:00:01Z", tool_use_id: "toolu_a", text: "Async agent launched", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
const waiting: ChatItem = { type: "tool_result", id: "r2", timestamp: "2026-11-11T05:26:55Z", tool_use_id: "toolu_a", text: "waiting", is_error: false, subagent: { running: 0, completed: 0, failed: 0, waiting: 1, task_id: "a87" } };
const done: ChatItem = { type: "tool_result", id: "r3", timestamp: "2026-11-11T05:34:25Z", tool_use_id: "toolu_a", text: "done", is_error: false, subagent: { running: 0, completed: 1, failed: 0, task_id: "a87" } };

describe("子代理等待后台", () => {
  it("徽标：等待后台独立成一档，不算完成也不算运行中", () => {
    expect(outcomeBadge({ running: 0, completed: 0, failed: 0, waiting: 1 }, zh)).toEqual({ tone: "waiting", text: "等待后台" });
    expect(outcomeBadge({ running: 0, completed: 1, failed: 0 }, zh)).toEqual({ tone: "completed", text: "已完成" });
  });

  it("进度面板：等待后台单独显示，算未结束；finished 到了变完成", () => {
    const states = (items: ChatItem[]) => collectBackgroundTasks(items).subagents.map((task) => task.state);
    expect(states([use, launched, waiting])).toEqual(["wait"]);
    expect(states([use, launched, waiting, done])).toEqual(["done"]);
    const tasks = collectBackgroundTasks([use, launched, waiting]).subagents;
    expect(filterTasks(tasks, "running")).toHaveLength(1);
    expect(filterTasks(tasks, "done")).toHaveLength(0);
    render(<TasksPanel tasks={tasks} filter="recent" />);
    expect(screen.getByRole("img", { name: "等待后台" })).toBeTruthy();
  });
});
