import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { zh } from "../../i18n/zh";
import { outcomeBadge } from "./SubagentBlock";
import { collectBackgroundTasks, filterTasks, TasksPanel } from "./TasksPanel";

// 子代理「is waiting on its own background work」（做空第二轮定）：结论已交，按完成算，次行小字
// 「后台仍在运行」；不算进行中——本机实拍 9 次里 4 次之后再无 finished，挂进行中会永远占着面板。
// 之后 finished 到了去掉次行。

afterEach(cleanup);

const use: ChatItem = {
  type: "tool_use", id: "toolu_a", timestamp: "2026-11-11T05:00:00Z", name: "Agent", kind: "agent", summary: "查误报",
  subagent: { description: "查误报", agent_type: "general-purpose", count: 1 },
};
const launched: ChatItem = { type: "tool_result", id: "r1", timestamp: "2026-11-11T05:00:01Z", tool_use_id: "toolu_a", text: "Async agent launched", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: "a87" } };
const waiting: ChatItem = { type: "tool_result", id: "r2", timestamp: "2026-11-11T05:26:55Z", tool_use_id: "toolu_a", text: "waiting", is_error: false, subagent: { running: 0, completed: 1, failed: 0, background_running: true, task_id: "a87" } };
const done: ChatItem = { type: "tool_result", id: "r3", timestamp: "2026-11-11T05:34:25Z", tool_use_id: "toolu_a", text: "done", is_error: false, subagent: { running: 0, completed: 1, failed: 0, task_id: "a87" } };

describe("子代理交了结论但后台仍在运行", () => {
  it("徽标：完成，次行「后台仍在运行」；finished 后次行消失", () => {
    expect(outcomeBadge({ running: 0, completed: 1, failed: 0, background_running: true }, zh)).toEqual({ tone: "completed", text: "已完成", note: "后台仍在运行" });
    expect(outcomeBadge({ running: 0, completed: 1, failed: 0 }, zh)).toEqual({ tone: "completed", text: "已完成" });
  });

  it("进度面板：按完成算，不占「进行中」，次行提示；再无 finished 时保持，finished 后提示消失", () => {
    const tasks = collectBackgroundTasks([use, launched, waiting]).subagents;
    expect(tasks.map((task) => [task.state, task.backgroundRunning])).toEqual([["done", true]]);
    expect(filterTasks(tasks, "running")).toHaveLength(0);
    expect(filterTasks(tasks, "done")).toHaveLength(1);
    render(<TasksPanel tasks={tasks} filter="recent" />);
    expect(screen.getByText("后台仍在运行")).toBeTruthy();
    expect(collectBackgroundTasks([use, launched, waiting, done]).subagents.map((task) => [task.state, task.backgroundRunning])).toEqual([["done", false]]);
  });
});
