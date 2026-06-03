import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { Sticker } from "./Sticker";
import type { LiveSession } from "../api";

function mk(over: Partial<LiveSession> = {}): LiveSession {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: 0, ended_at: null },
    project_name: "proj",
    task_title: "做点事",
    current_activity: "正在做点事",
    column: "doing",
    todo_done: 0,
    todo_total: 0,
    todos: [],
    ...over,
  };
}

afterEach(() => cleanup());

describe("Sticker", () => {
  it("空数据显示无活跃会话", () => {
    const { container } = render(<Sticker data={[]} />);
    expect(screen.getByText("无活跃会话")).toBeTruthy();
    expect(container.querySelector("[data-tauri-drag-region]")).toBeTruthy();
  });

  it("渲染会话行：项目名 + 当前动作", () => {
    render(<Sticker data={[mk()]} />);
    expect(screen.getByText("proj")).toBeTruthy();
    expect(screen.getByText("正在做点事")).toBeTruthy();
  });

  it("unnamed 会话且无动作时显示等待首次输入", () => {
    render(<Sticker data={[mk({ task_title: "(未命名会话)", current_activity: null })]} />);
    expect(screen.getByText("等待首次输入")).toBeTruthy();
  });

  it("stale 会话用灰点", () => {
    const { container } = render(<Sticker data={[mk({ session: { id: 2, project_id: 1, cc_session_id: "x", status: "stale", started_at: 0, last_event_at: 0, ended_at: null } })]} />);
    expect(container.querySelector(".dot-stale")).toBeTruthy();
  });
});
