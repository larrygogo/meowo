import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ChatItem } from "../../api";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { latestPlan, PlanPanel } from "./PlanPanel";

const items: ChatItem[] = [
  { type: "tool_use", id: "todo-1", timestamp: null, name: "TodoWrite", summary: "拆分待办" },
  { type: "tool_use", id: "plan-1", timestamp: null, name: "ExitPlanMode", summary: "提交计划", plan: "# 改造目标\n\n## 实施步骤\n\n保留完整计划文档。" },
];

afterEach(cleanup);

describe("PlanPanel 计划文档", () => {
  it("展示已通过计划的 Markdown 正文，而不是执行待办", () => {
    render(<PlanPanel plan={latestPlan(items)} pending={false} onClose={() => {}} />);
    expect(screen.getByText("已通过")).toBeTruthy();
    expect(screen.getByRole("heading", { name: "改造目标", level: 1 })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "实施步骤", level: 2 })).toBeTruthy();
    expect(screen.getByText("保留完整计划文档。")).toBeTruthy();
    expect(screen.queryByText("拆分待办")).toBeNull();
  });

  it("待审批时显示待审批状态", () => {
    render(<PlanPanel plan={latestPlan(items)} pending={true} onClose={() => {}} />);
    expect(screen.getByText("待审批")).toBeTruthy();
  });

  it("取最近一次计划，空白文档不能覆盖已有计划", () => {
    const history = [
      ...items,
      { type: "tool_use", id: "plan-2", timestamp: null, name: "ExitPlanMode", summary: "更新计划", plan: "# 最新计划" },
      { type: "tool_use", id: "plan-3", timestamp: null, name: "ExitPlanMode", summary: "空计划", plan: "  " },
    ] satisfies ChatItem[];
    expect(latestPlan(history)).toBe("# 最新计划");
  });

  it("计划已被裁出首屏时使用历史文档，新计划优先于缓存", () => {
    expect(latestPlan(items.slice(0, 1), "# 历史窗口外的计划")).toBe("# 历史窗口外的计划");
    expect(latestPlan(items, "# 历史窗口外的计划")).toContain("# 改造目标");
    expect(latestPlan(items.slice(0, 1), "  ")).toBeNull();
  });

  it("计划模式已开但还没有计划时显示等待文案", () => {
    render(<PlanPanel plan={null} pending={false} onClose={() => {}} />);
    expect(screen.getByText("计划模式已开启，写出的计划会显示在这里")).toBeTruthy();
    expect(screen.queryByText("已通过")).toBeNull();
  });
});
