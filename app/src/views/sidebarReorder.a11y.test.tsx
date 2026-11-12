// 侧栏手动排序的键盘方式与拖到边缘自动滚动（调研 s8）。
// 快捷键用 Ctrl/⌘+Shift+↑/↓：Alt+↑/↓ 是切会话、Alt+Shift+↓ 是跳到下一个待处理会话，都已占用。
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../platform", () => ({ isMac: () => false }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ChatSidebar } from "./ChatSidebar";
import { edgeScrollStep, usePointerReorder } from "./sidebarDrag";
import type { LiveSession } from "../api";

function session(id: number, title: string): LiveSession {
  return { session: { id, cc_session_id: `cc-${id}`, status: "ended" }, project_name: "meowo", task_title: title, connected: false, pending_review: null, cwd: "C:/w/meowo", provider: "claude" } as unknown as LiveSession;
}

afterEach(() => { cleanup(); vi.useRealTimers(); });
beforeEach(() => { invoke.mockReset(); localStorage.clear(); });

describe("键盘调整手动排序", () => {
  const setup = async () => {
    const moves: unknown[] = [];
    invoke.mockImplementation((command: string, args?: unknown) => {
      if (command === "get_live_sessions_page") return Promise.resolve([session(3, "丙"), session(2, "乙"), session(1, "甲")]);
      if (command === "move_session_rank") { moves.push(args); return Promise.resolve(); }
      return Promise.resolve();
    });
    render(<ChatSidebar activeId={3} approvalAwaitingIds={new Set()} onSelect={() => {}} onCollapse={() => {}} />);
    const row = async (name: string) => (await screen.findByRole("button", { name: new RegExp(name) })) as HTMLElement;
    return { moves, row };
  };
  const order = () => Array.from(document.querySelectorAll(".chat-sidebar-item .chat-sidebar-name-inner")).map((n) => n.textContent);

  it("Ctrl+Shift+↑ 上移一位，Ctrl+Shift+↓ 下移一位，焦点跟着条目走", async () => {
    const { moves, row } = await setup();
    const yi = await row("乙");
    yi.focus();
    fireEvent.keyDown(yi, { key: "ArrowUp", ctrlKey: true, shiftKey: true });
    expect(moves).toEqual([{ sessionId: 2, aboveId: null, belowId: 3 }]);
    expect(order()).toEqual(["乙", "丙", "甲"]);
    await act(() => new Promise((r) => setTimeout(r, 10)));
    expect(document.activeElement?.getAttribute("data-session-id")).toBe("2");
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown", ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown", ctrlKey: true, shiftKey: true });
    expect(moves.slice(1)).toEqual([{ sessionId: 2, aboveId: 3, belowId: 1 }, { sessionId: 2, aboveId: 1, belowId: null }]);
    expect(order()).toEqual(["丙", "甲", "乙"]);
  });

  it("到头不动、不写后端；存着已去掉的「按最近排序」旧值时照常能挪", async () => {
    const { moves, row } = await setup();
    fireEvent.keyDown(await row("丙"), { key: "ArrowUp", ctrlKey: true, shiftKey: true });
    expect(moves).toEqual([]);
    cleanup();
    // 旧版本存下的值：这一档排序已经去掉，侧栏按手动排序走，不能因为它把键盘排序关掉。
    localStorage.setItem("meowo-chat-sidebar-sort-mode", "recent");
    const again = await setup();
    fireEvent.keyDown(await again.row("乙"), { key: "ArrowUp", ctrlKey: true, shiftKey: true });
    expect(again.moves).toHaveLength(1);
  });
});

describe("拖到列表边缘自动滚动", () => {
  it("edgeScrollStep：离上下沿 40px 内按距离给速度，越靠边越快，中间为 0", () => {
    expect(edgeScrollStep(100, 0, 200)).toBe(0);
    expect(edgeScrollStep(20, 0, 200)).toBeLessThan(0);
    expect(edgeScrollStep(180, 0, 200)).toBeGreaterThan(0);
    expect(Math.abs(edgeScrollStep(2, 0, 200))).toBeGreaterThan(Math.abs(edgeScrollStep(30, 0, 200)));
    // 指针拖出列表外：按最快速度滚。
    expect(edgeScrollStep(-50, 0, 200)).toBe(edgeScrollStep(0, 0, 200));
    expect(edgeScrollStep(41, 0, 200)).toBe(0);
  });

  it("拖动中指针停在列表底沿附近，列表持续往下滚", () => {
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "setTimeout", "clearTimeout"] });
    function List() {
      const drag = usePointerReorder<number>({
        enabled: true, attr: "rowId", parse: Number, bucketOf: () => "b", listOf: () => [1, 2], onMove: () => {},
      });
      return (
        <div data-testid="scroller" style={{ overflowY: "auto", height: 200 }}>
          {[1, 2].map((id) => <div key={id} data-row-id={id} onPointerDown={(e) => drag.onPointerDown(e, id)}>{id}</div>)}
        </div>
      );
    }
    render(<List />);
    const scroller = screen.getByTestId("scroller");
    scroller.getBoundingClientRect = () => ({ top: 0, bottom: 200, height: 200, left: 0, right: 200, width: 200, x: 0, y: 0, toJSON: () => ({}) });
    Object.defineProperty(scroller, "scrollHeight", { configurable: true, value: 2000 });
    Object.defineProperty(scroller, "clientHeight", { configurable: true, value: 200 });
    const original = document.elementFromPoint;
    document.elementFromPoint = () => null;
    try {
      fireEvent.pointerDown(screen.getByText("1"), { button: 0, pointerType: "mouse", clientX: 10, clientY: 10 });
      fireEvent.pointerMove(window, { clientX: 10, clientY: 195 });
      act(() => { vi.advanceTimersByTime(200); });
      expect(scroller.scrollTop).toBeGreaterThan(0);
      const at = scroller.scrollTop;
      fireEvent.pointerUp(window, { clientX: 10, clientY: 195 });
      act(() => { vi.advanceTimersByTime(200); });
      // 松手即停。
      expect(scroller.scrollTop).toBe(at);
    } finally {
      document.elementFromPoint = original;
    }
  });
});
