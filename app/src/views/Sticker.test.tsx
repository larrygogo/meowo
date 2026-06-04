import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { Sticker } from "./Sticker";
import type { LiveSession } from "../api";

type Item = LiveSession & { connected: boolean };

function mk(over: Partial<Item> = {}): Item {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
    project_name: "proj",
    task_title: "做点事",
    current_activity: "正在做点事",
    column: "doing", todo_done: 0, todo_total: 0, todos: [],
    pid: 1234, connected: true,
    ...over,
  } as Item;
}

afterEach(() => cleanup());

describe("Sticker", () => {
  it("空数据显示（空）", () => {
    const { container } = render(<Sticker data={[]} />);
    expect(screen.getByText("（空）")).toBeTruthy();
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

  it("connected 时显示 Connected 徽标", () => {
    render(<Sticker data={[mk({ connected: true })]} />);
    expect(screen.getByText("Connected")).toBeTruthy();
  });

  it("disconnected 时显示 Disconnected 徽标", () => {
    render(<Sticker data={[mk({ connected: false })]} />);
    expect(screen.getByText("Disconnected")).toBeTruthy();
  });

  it("stale + disconnected 显示 Disconnected", () => {
    render(<Sticker data={[mk({ session: { id: 2, project_id: 1, cc_session_id: "x", status: "stale", started_at: 0, last_event_at: Date.now(), ended_at: null }, connected: false })]} />);
    expect(screen.getByText("Disconnected")).toBeTruthy();
  });
});
