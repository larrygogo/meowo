import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@testing-library/react";
import { CollapsedStrip } from "./CollapsedStrip";
import type { LiveSession } from "../api";

type Item = LiveSession & { connected: boolean };

function mk(over: Partial<Item> = {}): Item {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: 0, ended_at: null },
    project_name: "proj",
    task_title: "t",
    current_activity: null,
    column: "doing", todo_done: 0, todo_total: 0, todos: [],
    pid: 1, connected: true, archived: false,
    ...over,
  } as Item;
}

afterEach(() => cleanup());

describe("CollapsedStrip", () => {
  it("每个非归档会话渲染一个圆点，按状态给类名", () => {
    const data: Item[] = [
      mk({ session: { id: 1, project_id: 1, cc_session_id: "a", status: "running", started_at: 0, last_event_at: 0, ended_at: null }, connected: true }),
      mk({ session: { id: 2, project_id: 1, cc_session_id: "b", status: "waiting", started_at: 0, last_event_at: 0, ended_at: null }, connected: true }),
      mk({ session: { id: 3, project_id: 1, cc_session_id: "c", status: "ended", started_at: 0, last_event_at: 0, ended_at: null }, connected: false }),
    ];
    const { container } = render(<CollapsedStrip data={data} edge="left" onExpand={() => {}} onLeave={() => {}} />);
    expect(container.querySelectorAll(".cstrip-dot").length).toBe(3);
    expect(container.querySelectorAll(".cstrip-running").length).toBe(1);
    expect(container.querySelectorAll(".cstrip-waiting").length).toBe(1);
    expect(container.querySelectorAll(".cstrip-stop").length).toBe(1);
  });

  it("归档会话不计入竖条", () => {
    const data: Item[] = [mk({ archived: true }), mk({ session: { id: 2, project_id: 1, cc_session_id: "b", status: "running", started_at: 0, last_event_at: 0, ended_at: null } })];
    const { container } = render(<CollapsedStrip data={data} edge="right" onExpand={() => {}} onLeave={() => {}} />);
    expect(container.querySelectorAll(".cstrip-dot").length).toBe(1);
  });

  it("edge 决定容器修饰类", () => {
    const { container } = render(<CollapsedStrip data={[]} edge="right" onExpand={() => {}} onLeave={() => {}} />);
    expect(container.querySelector(".cstrip-right")).toBeTruthy();
  });
});
