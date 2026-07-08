import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, waitFor, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

const getLiveSessionsCounts = vi.fn();
const getLiveSessionsPage = vi.fn();
let emitBoardChanged: () => void = () => {};
const unlisten = vi.fn();

// jsdom 没有真实视口尺寸，@tanstack/react-virtual 会以为 .stk-scroll 高度为 0 而不渲染卡片。
// mock 一个足够大的滚动容器，让测试里的卡片进入可视区并被挂载。
const defaultRect: DOMRect = {
  top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0,
  toJSON: () => ({ top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0 }),
};
vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement): DOMRect {
  if (this.classList.contains("stk-scroll")) {
    return {
      ...defaultRect,
      bottom: 600, right: 400, width: 400, height: 600,
      toJSON: () => ({ ...defaultRect, bottom: 600, right: 400, width: 400, height: 600 }),
    };
  }
  if (this.classList.contains("stk-vitem")) {
    return {
      ...defaultRect,
      right: 400, width: 400, height: 82,
      toJSON: () => ({ ...defaultRect, right: 400, width: 400, height: 82 }),
    };
  }
  return defaultRect;
});
const mockScrollRect = { top: 0, left: 0, bottom: 600, right: 400, width: 400, height: 600, x: 0, y: 0 };
const mockItemRect = { top: 0, left: 0, bottom: 82, right: 400, width: 400, height: 82, x: 0, y: 0 };
class MockResizeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(target: Element) {
    const isScroll = target.classList.contains("stk-scroll");
    const rect = isScroll ? mockScrollRect : mockItemRect;
    this.cb([{
      target,
      contentRect: rect as unknown as DOMRectReadOnly,
      borderBoxSize: [{ inlineSize: rect.width, blockSize: rect.height } as unknown as ResizeObserverSize],
      contentBoxSize: [{ inlineSize: rect.width, blockSize: rect.height } as unknown as ResizeObserverSize],
      devicePixelContentBoxSize: [],
    } as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", MockResizeObserver);

vi.mock("./api", () => ({
  getLiveSessionsCounts: () => getLiveSessionsCounts(),
  getLiveSessionsPage: (
    filter: "all" | "running" | "waiting" | "archived",
    cursor: { last_event_at: number; id: number } | null,
    limit: number
  ) => getLiveSessionsPage(filter, cursor, limit),
  getSettings: () =>
    Promise.resolve({
      archive_hide_days: 0,
      notifications_enabled: true,
      theme: "dark",
      opacity: 94,
      ui_scale: 100,
      resume_terminal: "wt",
      language: "auto",
      terminal_open_mode: "card",
      card_menu_mode: "context",
      preview_enabled: true,
      sticker_style: "elevated",
      sticker_color: "classic",
      sticker_quota_providers: ["claude"],
      default_agent: "claude",
    }),
  getAccounts: () => Promise.resolve([]),
  refreshUsage: (_provider: string) => Promise.reject(new Error("USAGE_UNSUPPORTED")),
  availableAgents: () => Promise.resolve([]),
}));
// 按事件名分别路由：board-changed → emitBoardChanged；snap-changed 忽略（Tauri 吸边，无法在 jsdom 中测试）
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: () => void) => {
    if (event === "board-changed") emitBoardChanged = cb;
    return Promise.resolve(unlisten);
  },
  emit: vi.fn(() => Promise.resolve()),
}));
// 吸边相关的 Tauri 命令/窗口 API：jsdom 无后端，给空实现避免报错。
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setAlwaysOnTop: vi.fn(() => Promise.resolve()),
    outerSize: vi.fn(() => Promise.resolve({ width: 340, height: 440 })),
    scaleFactor: vi.fn(() => Promise.resolve(1)),
  }),
}));
// 更新检查相关插件：jsdom 无后端，给空实现（check 返回无更新）。
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(() => Promise.resolve()) }));

import { App } from "./App";

beforeEach(() => {
  getLiveSessionsCounts.mockReset();
  getLiveSessionsCounts.mockResolvedValue({ total: 0, running: 0, waiting: 0, archived: 0 });
  getLiveSessionsPage.mockReset();
  getLiveSessionsPage.mockResolvedValue([]);
  unlisten.mockReset();
  vi.mocked(invoke).mockClear();
  localStorage.clear();
});
afterEach(() => cleanup());

describe("App", () => {
  it("挂载时拉取 counts 和第 0 页", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", null, 100));
  });

  it("收到 board-changed 后重新拉取 counts 和第 0 页", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(1));
    emitBoardChanged();
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", null, 100));
  });

  // 单一真相源：window-state 不再恢复尺寸(lib.rs)，main 窗口尺寸由 SIZE_KEY 持有。非吸附态启动
  // 无条件按 SIZE_KEY(默认 {360,440}) snap_restore 还原正常尺寸，且不走折叠分支。
  it("非吸附态启动按 SIZE_KEY 还原正常尺寸(snap_restore)，不折叠", async () => {
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("snap_collapse", expect.anything());
  });

  it("吸附态启动(SNAP_KEY 有边)走折叠分支，不触发尺寸还原", async () => {
    localStorage.setItem("cc-kanban-snap-edge", "left");
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("snap_collapse", expect.anything())
    );
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("snap_restore", expect.anything());
  });

  // 回归：SIZE_KEY 曾被「吸附态拖角缩成细条」的尺寸毒化(实测 {80,240}/{136,20})。loadSize 须把低于
  // 最小尺寸的值回落默认 {360,440}，否则还原会用毒化的细条尺寸、把窗口缩成细条。
  it("SIZE_KEY 被细条尺寸毒化时，loadSize 回落默认 {360,440} 再 snap_restore", async () => {
    localStorage.setItem("cc-kanban-normal-size", JSON.stringify({ w: 80, h: 37 }));
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
  });

  // 回归：SIZE_KEY 异常大值/非有限数(localStorage 被改坏)不能直接喂给 set_size，否则设出极端窗口。
  // loadSize 须校验上界(<=20000)与有限数，超界则回落默认 {360,440}。
  it("SIZE_KEY 异常大值时，loadSize 回落默认 {360,440}，不设出极端窗口", async () => {
    localStorage.setItem("cc-kanban-normal-size", JSON.stringify({ w: 999999, h: 999999 }));
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith(
        "snap_restore",
        expect.objectContaining({ width: 360, height: 440 })
      )
    );
  });

  // 回归：running tab 下刷新时，若某会话已从 running 迁移到 waiting，后端 running 分页不再返回它；
  // 旧合并逻辑会保留 prev 中的该会话（状态仍是旧 running），导致它错误地继续显示在 running tab 下。
  it("running tab 刷新时，状态迁出 running 的会话应从列表移除", async () => {
    localStorage.setItem("cc-kanban-tab", "running");
    const mk = (id: number, status: "running" | "waiting", title: string) => ({
      session: {
        id,
        cc_session_id: `s-${id}`,
        status,
        last_event_at: 1000 - id,
        started_at: 0,
        ended_at: null,
        project_id: 1,
      },
      project_name: "p",
      task_title: title,
      current_activity: null,
      column: "todo" as const,
      todo_done: 0,
      todo_total: 0,
      todos: [],
      pid: null,
      connected: true,
      archived: false,
      archived_at: null,
      cwd: "/p",
      errored: false,
      error_label: null,
      error_raw: null,
      preview: null,
      note: null,
      context_pct: null,
      context_window: null,
      model: null,
      pending_review: null,
      last_ai_text: null,
      last_user_text: null,
      provider: "claude" as const,
    });

    getLiveSessionsCounts.mockResolvedValue({ total: 2, running: 2, waiting: 0, archived: 0 });
    getLiveSessionsPage
      .mockResolvedValueOnce([mk(1, "running", "A"), mk(2, "running", "B")])
      .mockResolvedValueOnce([mk(1, "running", "A")]); // B 已迁出 running

    render(<App />);
    await waitFor(() => expect(screen.getByText("A")).toBeTruthy());
    await waitFor(() => expect(screen.getByText("B")).toBeTruthy());

    emitBoardChanged();
    await waitFor(() => expect(screen.queryByText("B")).toBeFalsy());
    expect(screen.getByText("A")).toBeTruthy();
  });

  // 回归：已存在会话的 last_event_at 变化时（如恢复断开的旧会话），board-changed 刷新应按
  // 新顺序重排；旧合并逻辑按 prev 数组的原位置合并，只有全新会话才 prepend 到最前，已存在
  // 会话不会因排序键变化而移动——用户须手动切 tab 才能看到它跳到最前。
  it("board-changed 刷新时，已存在会话应按新 last_event_at 重新排序（如恢复的旧会话跳到最前）", async () => {
    localStorage.setItem("cc-kanban-tab", "all");
    const mk = (id: number, lastEventAt: number, title: string) => ({
      session: {
        id,
        cc_session_id: `s-${id}`,
        status: "running" as const,
        last_event_at: lastEventAt,
        started_at: 0,
        ended_at: null,
        project_id: 1,
      },
      project_name: "p",
      task_title: title,
      current_activity: null,
      column: "todo" as const,
      todo_done: 0,
      todo_total: 0,
      todos: [],
      pid: null,
      connected: true,
      archived: false,
      archived_at: null,
      cwd: "/p",
      errored: false,
      error_label: null,
      error_raw: null,
      preview: null,
      note: null,
      context_pct: null,
      context_window: null,
      model: null,
      pending_review: null,
      last_ai_text: null,
      last_user_text: null,
      provider: "claude" as const,
    });

    getLiveSessionsCounts.mockResolvedValue({ total: 2, running: 2, waiting: 0, archived: 0 });
    // 首页：B（last_event_at 更晚）排前，A（很久没活动，模拟断开的旧会话）排后。
    getLiveSessionsPage
      .mockResolvedValueOnce([mk(2, 2000, "B"), mk(1, 1000, "A")])
      // A 被恢复：last_event_at 刷新为最新，后端按新顺序把 A 排到最前。
      .mockResolvedValueOnce([mk(1, 3000, "A"), mk(2, 2000, "B")]);

    render(<App />);
    await waitFor(() => {
      const titles = Array.from(document.querySelectorAll(".stk-title")).map((el) => el.textContent);
      expect(titles).toEqual(["B", "A"]);
    });

    emitBoardChanged();
    await waitFor(() => {
      const titles = Array.from(document.querySelectorAll(".stk-title")).map((el) => el.textContent);
      expect(titles).toEqual(["A", "B"]);
    });
  });
});
