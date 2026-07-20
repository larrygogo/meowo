// App 内 loadMore 的重入守卫：loadingMore 是 state，set 后到下次渲染落地之间同一 tick 仍可重入
// （Sticker 触底 effect 在一个渲染批内连发），会以相同游标重复请求下一页。这里 mock 掉 Sticker
// 直接捕获 loadMore prop，同 tick 连调两次来钉死这个竞态——真实滚动路径在 jsdom 里无法稳定复现。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, render, cleanup, waitFor } from "@testing-library/react";

const getLiveSessionsCounts = vi.fn();
const getLiveSessionsPage = vi.fn();

// 捕获最近一次渲染传给 Sticker 的 props（只关心 loadMore 与 data）。
let stickerProps: { loadMore?: () => void; data?: unknown[] } = {};
vi.mock("./views/Sticker", () => ({
  Sticker: (props: { loadMore?: () => void; data?: unknown[] }) => {
    stickerProps = props;
    return null;
  },
}));

vi.mock("./api", () => ({
  getLiveSessionsCounts: () => getLiveSessionsCounts(),
  getLiveSessionsPage: (
    filter: "all" | "running" | "waiting" | "archived",
    search: string | null,
    cursor: { last_event_at: number; id: number } | null,
    limit: number
  ) => getLiveSessionsPage(filter, search, cursor, limit),
  getSettings: () => Promise.resolve({}),
  getAccounts: () => Promise.resolve([]),
  refreshUsage: () => Promise.reject(new Error("USAGE_UNSUPPORTED")),
  listAgents: () => Promise.resolve([]),
  agentName: (agents: { id: string; display_name: string }[], id: string) =>
    agents.find((a) => a.id === id)?.display_name ?? id,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
  emit: vi.fn(() => Promise.resolve()),
}));
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
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(() => Promise.resolve()) }));

import { App } from "./App";

const mk = (id: number) => ({
  session: {
    id,
    cc_session_id: `s-${id}`,
    status: "running" as const,
    last_event_at: 10_000 - id,
    started_at: 0,
    ended_at: null,
    project_id: 1,
  },
  project_name: "p",
  task_title: `t-${id}`,
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

const cursorCalls = () => getLiveSessionsPage.mock.calls.filter(([, , cursor]) => cursor !== null);

beforeEach(() => {
  stickerProps = {};
  getLiveSessionsCounts.mockReset();
  getLiveSessionsCounts.mockResolvedValue({ total: 300, running: 300, waiting: 0, archived: 0 });
  getLiveSessionsPage.mockReset();
  localStorage.clear();
});
afterEach(() => cleanup());

describe("App loadMore 重入守卫", () => {
  it("同一 tick 重复触发 loadMore 只发一次游标分页请求", async () => {
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null, cursor: unknown) => {
      if (search === null) return Promise.resolve([]); // 折叠条查询，与本测试无关
      // 游标请求挂起不落地：守卫若在重入前没同步置位，第二次调用会再发一发同样的请求。
      if (cursor) return new Promise(() => {});
      return Promise.resolve(Array.from({ length: 100 }, (_, i) => mk(i + 1)));
    });

    render(<App />);
    // 首屏 100 条落定（page.length == limit，reachedEnd 不置位，loadMore 可用）。
    await waitFor(() => expect(stickerProps.data).toHaveLength(100));

    act(() => {
      stickerProps.loadMore!();
      stickerProps.loadMore!(); // 同 tick 重入：state 尚未落地，必须被 ref 守卫当场拦下
    });

    expect(cursorCalls()).toHaveLength(1);
    expect(cursorCalls()[0][2]).toEqual({ last_event_at: 9_900, id: 100 });
  });

  it("上一页请求完成后守卫释放，可继续加载下一页", async () => {
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null, cursor: unknown) => {
      if (search === null) return Promise.resolve([]);
      // 每次游标请求都回满一页（100 条）：reachedEnd 不置位，可继续翻页。
      if (cursor) return Promise.resolve(Array.from({ length: 100 }, (_, i) => mk(i + 101)));
      return Promise.resolve(Array.from({ length: 100 }, (_, i) => mk(i + 1)));
    });

    render(<App />);
    await waitFor(() => expect(stickerProps.data).toHaveLength(100));

    await act(async () => {
      await stickerProps.loadMore!();
    });
    await act(async () => {
      await stickerProps.loadMore!();
    });

    expect(cursorCalls()).toHaveLength(2);
    expect(cursorCalls()[0][2]).toEqual({ last_event_at: 9_900, id: 100 });
    expect(cursorCalls()[1][2]).toEqual({ last_event_at: 9_800, id: 200 });
  });
});
