// 页签组键盘规范（调研 i9）：侧栏三页签、后台任务筛选、Lead 面板页签共用 useTablist，
// 每处验 ←/→/Home/End、roving tabindex、aria-controls 指向真实存在的面板。
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { useTablist } from "./useTablist";
import { ChatSidebar } from "./ChatSidebar";
import { TasksDockPanel } from "./chat/TasksPanel";
import { LeadPanel } from "./chat/LeadPanel";
import type { LeadTaskDto } from "../api";

afterEach(cleanup);
beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue([]);
  localStorage.clear();
});

/// 每组通用断言：只有选中页签 tabIndex=0；aria-controls 指向 DOM 里存在的元素；
/// → 选中并聚焦下一个、End 到最后、Home 回第一个、← 到头不回绕。
function expectTablistKeys(tabs: () => HTMLElement[]) {
  const all = tabs();
  expect(all.length).toBeGreaterThan(1);
  expect(all.filter((tab) => tab.tabIndex === 0)).toHaveLength(1);
  for (const tab of all) {
    const id = tab.getAttribute("aria-controls");
    expect(id, tab.textContent ?? "").toBeTruthy();
    expect(document.getElementById(id!), `aria-controls=${id}`).not.toBeNull();
  }
  const selected = () => tabs().findIndex((tab) => tab.getAttribute("aria-selected") === "true");
  const start = selected();
  tabs()[start].focus();
  fireEvent.keyDown(tabs()[start], { key: "ArrowRight" });
  expect(selected()).toBe(start + 1);
  expect(document.activeElement).toBe(tabs()[start + 1]);
  expect(tabs()[start + 1].tabIndex).toBe(0);
  fireEvent.keyDown(tabs()[start + 1], { key: "End" });
  expect(selected()).toBe(tabs().length - 1);
  expect(document.activeElement).toBe(tabs()[tabs().length - 1]);
  fireEvent.keyDown(tabs()[tabs().length - 1], { key: "Home" });
  expect(selected()).toBe(0);
  expect(document.activeElement).toBe(tabs()[0]);
  fireEvent.keyDown(tabs()[0], { key: "ArrowLeft" });
  expect(selected()).toBe(0);
}

describe("useTablist", () => {
  function Demo({ vertical }: { vertical?: boolean }) {
    const keys = ["a", "b", "c"] as const;
    const [active, setActive] = useState<(typeof keys)[number]>("a");
    const tabs = useTablist({ keys, active, onSelect: setActive, orientation: vertical ? "vertical" : "horizontal" });
    return (
      <>
        <div {...tabs.listProps} aria-label="演示">
          {keys.map((k) => <button key={k} type="button" {...tabs.tabProps(k)}>{k}</button>)}
        </div>
        <div {...tabs.panelProps}>{active}</div>
      </>
    );
  }
  it("横向：方向键、Home/End、roving tabindex、面板 labelledby 当前页签", () => {
    render(<Demo />);
    expectTablistKeys(() => screen.getAllByRole("tab"));
    const panel = screen.getByRole("tabpanel");
    expect(panel.getAttribute("aria-labelledby")).toBe(screen.getAllByRole("tab")[0].id);
  });
  it("纵向用 ↑/↓，带修饰键的方向键不处理（留给 Alt+↑/↓ 等快捷键）", () => {
    render(<Demo vertical />);
    const tabs = screen.getAllByRole("tab");
    expect(screen.getByRole("tablist").getAttribute("aria-orientation")).toBe("vertical");
    fireEvent.keyDown(tabs[0], { key: "ArrowRight" });
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(tabs[0], { key: "ArrowDown", altKey: true });
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(tabs[0], { key: "ArrowDown" });
    expect(screen.getAllByRole("tab")[1].getAttribute("aria-selected")).toBe("true");
  });
});

describe("用到 useTablist 的页签组", () => {
  it("侧栏「进行中 / 已完成 / 工作区」", async () => {
    render(<ChatSidebar activeId={1} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    const list = await screen.findByRole("tablist");
    await waitFor(() => expect(list.querySelectorAll('[role="tab"]').length).toBe(3));
    expectTablistKeys(() => Array.from(list.querySelectorAll<HTMLElement>('[role="tab"]')));
  });

  it("后台任务筛选", () => {
    render(<TasksDockPanel title="后台任务" icon={<span />} tasks={[]} onClose={() => {}} />);
    expectTablistKeys(() => screen.getAllByRole("tab"));
  });

  it("Lead 面板页签", () => {
    const task = { slug: "s", phase: "implement", spec: "# SPEC", checks: [], decisions: "", progress: "", gate: null } as unknown as LeadTaskDto;
    render(<LeadPanel task={task} onClose={() => {}} />);
    expectTablistKeys(() => screen.getAllByRole("tab"));
  });
});
