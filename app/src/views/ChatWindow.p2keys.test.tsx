import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { MutableRefObject } from "react";

// 二期 #5 / #8：对话窗的跳转与找字快捷键。用独立文件（ChatWindow.test.tsx 正在被 infra 车道拆分）。
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path, Channel: class { onmessage = () => {}; } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    close: () => Promise.resolve(),
    setTitle: () => Promise.resolve(),
    onDragDropEvent: () => Promise.resolve(() => {}),
    onResized: () => Promise.resolve(() => {}),
    isMaximized: () => Promise.resolve(false),
    onCloseRequested: () => Promise.resolve(() => {}),
  }),
}));
// 终端换成一个带 .managed-terminal 的替身：焦点放进去可以模拟「焦点在终端里」，
// 并把对话窗传下来的找字入口接到 spy 上。
const terminalFind = vi.hoisted(() => vi.fn());
vi.mock("./ManagedTerminal", () => ({
  ManagedTerminal: ({ findRef }: { findRef?: MutableRefObject<(() => void) | null> }) => {
    if (findRef) findRef.current = terminalFind;
    return <div className="managed-terminal"><textarea aria-label="xterm" /></div>;
  },
}));
vi.mock("./chat/SidePanel", () => ({ SidePanel: () => null }));

import { ChatWindow } from "./ChatWindow";
import { zh } from "../i18n/zh";

type Row = { id: number; title: string; status?: string; pending?: string | null; errored?: boolean };
let rows: Row[] = [];

const live = (row: Row, index: number) => ({
  session: { id: row.id, project_id: 1, cc_session_id: `cc-${row.id}`, status: row.status ?? "waiting", started_at: 0, last_event_at: 1000 - index, ended_at: null },
  project_name: "proj", task_title: row.title, current_activity: null, column: "doing", todo_done: 0, todo_total: 0, todos: [],
  pid: 1, archived: false, archived_at: null, cwd: "C:/repo", context_pct: null, context_window: null, model: null, note: null,
  pending_review: row.pending ?? null, last_ai_text: null, last_user_text: null, provider: "claude", profile: null, predecessor_id: null,
  // 侧栏默认手动排序：manual_key 越大越靠前，按 rows 的顺序给。
  manual_key: 10_000 - index,
  connected: true, pty_managed: true, endable: true, background: false, errored: row.errored ?? false,
  error_label: row.errored ? "出错" : null, error_raw: null, busy_subagents: 0, preview: null, profile_name: null, foreign: false,
  screen_state: row.status === "running" ? "working" : "idle",
});

const historyOf = (sessionId: number) => ({
  sessionId, provider: "claude", status: "waiting", title: rows.find((r) => r.id === sessionId)?.title ?? "会话", cwd: "C:/repo",
  connected: true, supported: true, offset: 0, reset: false, pendingReview: null, ptyManaged: true,
  items: [{ type: "assistant_text", id: `a-${sessionId}`, timestamp: null, text: "这里有一段可以搜的正文" }],
});

beforeEach(() => {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
  });
  window.history.replaceState({}, "", "/?sessionId=1");
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "get_chat_history") return Promise.resolve(historyOf(Number(args?.sessionId)));
    if (command === "get_live_sessions_page") return Promise.resolve({ items: rows.map(live), next_cursor: null });
    if (command === "get_live_sessions_counts") return Promise.resolve({ total: rows.length, running: 0, waiting: 0, archived: 0 });
    if (command === "pending_interaction") return Promise.resolve({ approval: null, question: null });
    if (command === "shell_terminal_list" || command === "browser_pane_list") return Promise.resolve([]);
    return Promise.resolve();
  });
});
afterEach(() => {
  cleanup();
  localStorage.clear();
  invoke.mockReset();
  terminalFind.mockReset();
});

const historyPulls = (sessionId: number) =>
  invoke.mock.calls.filter(([name, args]) => name === "get_chat_history" && (args as { sessionId: number }).sessionId === sessionId).length;
const nextPending = (target: Element | Window = window) =>
  fireEvent.keyDown(target, { key: "ArrowDown", code: "ArrowDown", altKey: true, shiftKey: true });

describe("Alt+Shift+↓ 跳到下一个等你处理的会话（二期 #5）", () => {
  it("按侧栏顺序跳过运行中，到头循环", async () => {
    rows = [
      { id: 1, title: "当前" },
      { id: 2, title: "在跑", status: "running" },
      { id: 3, title: "待审批", pending: "approval" },
      { id: 4, title: "出错了", errored: true },
    ];
    render(<ChatWindow />);
    await screen.findByRole("button", { name: /待审批/ });
    act(() => { nextPending(); });
    await waitFor(() => expect(historyPulls(3)).toBeGreaterThan(0));
    act(() => { nextPending(); });
    await waitFor(() => expect(historyPulls(4)).toBeGreaterThan(0));
    const before = historyPulls(3);
    // 4 之后循环回头：1 只是跑完、2 在跑，都跳过，落到 3。
    act(() => { nextPending(); });
    await waitFor(() => expect(historyPulls(3)).toBeGreaterThan(before));
    expect(historyPulls(2)).toBe(0);
  });

  it("没有等你处理的会话时提示一句", async () => {
    rows = [{ id: 1, title: "当前" }, { id: 2, title: "在跑", status: "running" }];
    render(<ChatWindow />);
    await screen.findByRole("button", { name: /在跑/ });
    act(() => { nextPending(); });
    expect(await screen.findByText(zh.chat.noPendingSession)).toBeTruthy();
    expect(historyPulls(2)).toBe(0);
  });

  it("焦点在终端里时不抢这个键", async () => {
    rows = [{ id: 1, title: "当前" }, { id: 3, title: "待审批", pending: "approval" }];
    render(<ChatWindow />);
    await screen.findByRole("button", { name: /待审批/ });
    // 切到终端视图让替身挂上，再把焦点放进去。
    fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
    const xterm = await screen.findByLabelText("xterm");
    xterm.focus();
    act(() => { nextPending(xterm); });
    await new Promise((r) => setTimeout(r, 50));
    expect(historyPulls(3)).toBe(0);
  });
});

describe("Ctrl+F 在当前内容里找字（二期 #8）", () => {
  beforeEach(() => {
    rows = [{ id: 1, title: "当前" }, { id: 2, title: "别的" }];
  });
  const sidebarSearch = () => document.querySelector(".chat-sidebar-search input") as HTMLInputElement | null;

  it("对话视图 Ctrl+F 打开对话内搜索，不去聚焦侧栏搜会话", async () => {
    render(<ChatWindow />);
    await screen.findByText("这里有一段可以搜的正文");
    fireEvent.keyDown(window, { key: "f", code: "KeyF", ctrlKey: true });
    expect(await screen.findByRole("search")).toBeTruthy();
    await new Promise((r) => requestAnimationFrame(() => r(null)));
    expect(document.activeElement).not.toBe(sidebarSearch());
  });

  it("Ctrl+Shift+F 是找字的别名，不再连带展开侧栏搜索", async () => {
    // 侧栏收起：旧实现会先把它展开再聚焦搜索框。
    localStorage.setItem("meowo-chat-sidebar-collapsed", "1");
    render(<ChatWindow />);
    await screen.findByText("这里有一段可以搜的正文");
    expect(sidebarSearch()).toBeNull();
    fireEvent.keyDown(window, { key: "F", code: "KeyF", ctrlKey: true, shiftKey: true });
    expect(await screen.findByRole("search")).toBeTruthy();
    await new Promise((r) => setTimeout(r, 50));
    expect(sidebarSearch()).toBeNull();
  });

  it("终端视图里焦点不在 xterm 上时，Ctrl+F 打开终端找字", async () => {
    render(<ChatWindow />);
    await screen.findByText("这里有一段可以搜的正文");
    fireEvent.keyDown(window, { key: "2", code: "Digit2", ctrlKey: true });
    await screen.findByLabelText("xterm");
    (document.activeElement as HTMLElement | null)?.blur();
    fireEvent.keyDown(window, { key: "f", code: "KeyF", ctrlKey: true });
    expect(terminalFind).toHaveBeenCalledTimes(1);
  });
});
