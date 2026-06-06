import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { LiveView } from "./LiveView";
import type { LiveSession } from "../api";

function mk(over: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: 0, ended_at: null },
    project_name: "proj",
    task_title: "做点事",
    current_activity: "正在做点事",
    column: "doing",
    todo_done: 1,
    todo_total: 2,
    todos: [
      { id: 1, task_id: 1, content: "甲", status: "completed", order_idx: 0 },
      { id: 2, task_id: 1, content: "乙", status: "in_progress", order_idx: 1 },
    ],
    pid: null,
    connected: false,
    archived: false,
    archived_at: null,
    cwd: null,
    ...over,
  };
}

beforeEach(() => localStorage.clear());
afterEach(() => cleanup());

describe("LiveView", () => {
  it("空数据显示占位文案", () => {
    render(<LiveView data={[]} />);
    expect(screen.getByText("当前没有活跃会话。")).toBeTruthy();
  });

  it("unnamed 会话显示等待首次输入", () => {
    render(<LiveView data={[mk({ task_title: "(未命名会话)" })]} />);
    expect(screen.getByText("等待首次输入…")).toBeTruthy();
  });

  it("默认进度卡：显示状态标签与进度，不显示 checklist", () => {
    render(<LiveView data={[mk()]} />);
    expect(screen.getByText("运行中")).toBeTruthy();
    expect(screen.getByText("1/2 · 50%")).toBeTruthy();
    // checklist 条目（rich 才有）不应出现
    expect(screen.queryByText("甲")).toBeNull();
  });

  it("切到信息丰富：出现 todo 勾选清单，并写入 localStorage", () => {
    render(<LiveView data={[mk()]} />);
    fireEvent.click(screen.getByText("信息丰富"));
    expect(screen.getByText("甲")).toBeTruthy();
    expect(screen.getByText("乙")).toBeTruthy();
    expect(localStorage.getItem("cc-kanban-density")).toBe("rich");
  });

  it("非法 localStorage 值回退到进度卡（不崩、默认档生效）", () => {
    localStorage.setItem("cc-kanban-density", "garbage");
    render(<LiveView data={[mk()]} />);
    // 进度卡可见进度文本
    expect(screen.getByText("1/2 · 50%")).toBeTruthy();
  });
});
