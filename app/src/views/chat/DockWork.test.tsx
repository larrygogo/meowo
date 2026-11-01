import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { ChatItem } from "../../api";
import { DockWork, dockEntries } from "./DockWork";

afterEach(cleanup);

/// 后台 Bash 的判据是「回执带子任务结局」（与 TasksPanel 同口径）：一条调用 + 一条回执。
const bash = (id: string): ChatItem[] => [
  { type: "tool_use", id, timestamp: null, name: "Bash", summary: "npm run dev", detail: { kind: "command", command: "npm run dev", description: "起开发服务" } },
  { type: "tool_result", id: `r-${id}`, timestamp: null, tool_use_id: id, text: "started", is_error: false, subagent: { running: 1, completed: 0, failed: 0, task_id: `task-${id}` } },
];

describe("dockEntries", () => {
  /// 入口按需出现：没计划、没开计划模式、没派后台任务时，一个入口都没有。
  it("什么都没有时不出入口", () => {
    const entries = dockEntries({ items: [], plan: null, planMode: false, planPending: false });
    expect(entries.showPlan).toBe(false);
    expect(entries.backgroundTasks.bash).toEqual([]);
    expect(entries.backgroundTasks.subagents).toEqual([]);
  });

  it("计划模式开着、或正等计划审批时，即便还没写出计划也给计划入口", () => {
    expect(dockEntries({ items: [], plan: null, planMode: true, planPending: false }).showPlan).toBe(true);
    expect(dockEntries({ items: [], plan: null, planMode: false, planPending: true }).showPlan).toBe(true);
    expect(dockEntries({ items: [], plan: "# 计划", planMode: false, planPending: false }).showPlan).toBe(true);
  });
});

describe("DockWork", () => {
  const history = { plan: "# 计划\n- 第一步", agentModes: [], pendingReview: null, todos: [] };

  it("点胶囊开合面板，Esc 与点面板外都能关", () => {
    render(<DockWork sessionId={1} items={[]} history={history} loading={false} />);
    const pill = screen.getByRole("button", { name: /计划/ });
    fireEvent.click(pill);
    expect(document.querySelector(".dock-work-panel")).toBeTruthy();
    fireEvent.click(pill);
    expect(document.querySelector(".dock-work-panel")).toBeNull();

    fireEvent.click(pill);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(document.querySelector(".dock-work-panel")).toBeNull();

    fireEvent.click(pill);
    fireEvent.pointerDown(document.body);
    expect(document.querySelector(".dock-work-panel")).toBeNull();
  });

  it("派过后台 Bash 才出后台入口", () => {
    const { rerender } = render(<DockWork sessionId={1} items={[]} history={null} loading={false} />);
    expect(screen.queryByRole("button", { name: /后台/ })).toBeNull();
    rerender(<DockWork sessionId={1} items={bash("b1")} history={null} loading={false} />);
    expect(screen.getByRole("button", { name: /后台/ })).toBeTruthy();
  });
});
