import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor, screen, act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

const getLiveSessionsCounts = vi.fn();
const getLiveSessionsPage = vi.fn();
const setSettingsMock = vi.fn((_s: Record<string, unknown>) => Promise.resolve());
let emitBoardChanged: () => void = () => {};
let emitIslandAlert: (e: { payload: unknown }) => void = () => {};
let emitRecallSticker: () => void = () => {};
const unlisten = vi.fn();
// Windows 贴纸已删（2026-11-05），会话列表（Sticker）只剩 macOS 菜单栏面板在用：
// 测列表逻辑的用例切到面板模式跑，其余默认走灵动岛。
let macPanel = false;
vi.mock("./platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./platform")>()),
  isMacPanel: () => macPanel,
}));
// 个别用例要改设置（如灵动岛的提醒开关）：合进 getSettings mock 的返回值。
let extraSettings: Record<string, unknown> = {};

// jsdom 没有真实视口尺寸，@tanstack/react-virtual 会以为 .stk-scroll 高度为 0 而不渲染卡片。
// mock 一个足够大的滚动容器，让测试里的卡片进入可视区并被挂载。
const defaultRect: DOMRect = {
  top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0,
  toJSON: () => ({ top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0 }),
};
// 胶囊的实测宽度（灵动岛测量回调用）；个别用例改它来模拟内容变宽。
let pillRectWidth = 0;
vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement): DOMRect {
  if (this.classList.contains("island-pill")) {
    return { ...defaultRect, width: pillRectWidth, right: pillRectWidth, toJSON: () => ({}) };
  }
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
// 记下所有观察者，个别用例手动再触发一次（模拟窗口改尺寸时的测量回调）。
const resizeObservers: { fire: () => void }[] = [];
class MockResizeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(target: Element) {
    resizeObservers.push({ fire: () => this.cb([{ target } as ResizeObserverEntry], this as unknown as ResizeObserver) });
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
  openNewSessionWindow: () => Promise.resolve(),
  getLiveSessionsCounts: () => getLiveSessionsCounts(),
  // 与真实 api.ts 同款归一化：允许各测试继续用「裸数组」写 mock 数据，这里补上
  // { items, next_cursor } 包装（给满 limit → 用末项当游标；给不满 → 到底）。
  getLiveSessionsPage: (
    filter: "all" | "running" | "waiting" | "archived",
    search: string | null,
    cursor: { last_event_at: number; id: number } | null,
    limit: number
  ) =>
    Promise.resolve(getLiveSessionsPage(filter, search, cursor, limit)).then((res: unknown) => {
      if (!Array.isArray(res)) return res;
      const rows = res as { session: { last_event_at: number; id: number } }[];
      const last = rows[rows.length - 1];
      return {
        items: rows,
        next_cursor: rows.length >= limit && last
          ? { last_event_at: last.session.last_event_at, id: last.session.id }
          : null,
      };
    }),
  setSettings: (s: Record<string, unknown>) => setSettingsMock(s),
  getSettings: () =>
    Promise.resolve({
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
      ...extraSettings,
    }),
  getAccounts: () => Promise.resolve([]),
  firstImportNotice: () => Promise.resolve(null),
  refreshUsage: (_provider: string) => Promise.reject(new Error("USAGE_UNSUPPORTED")),
  // 本 mock 不展开真实模块（工厂里显式列举导出），故 useAgents 用到的两个都得在此提供。
  listAgents: () => Promise.resolve([]),
  agentName: (agents: { id: string; display_name: string }[], id: string) =>
    agents.find((a) => a.id === id)?.display_name ?? id,
}));
// 按事件名分别路由：board-changed / recall-sticker / island-alert 各自记下最新的回调。
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    if (event === "board-changed") emitBoardChanged = cb as () => void;
    if (event === "recall-sticker") emitRecallSticker = cb as unknown as () => void;
    if (event === "island-alert") emitIslandAlert = cb;
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
    onScaleChanged: vi.fn(() => Promise.resolve(() => {})),
  }),
}));
// 更新检查相关插件：jsdom 无后端，给空实现（check 返回无更新）。
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn(() => Promise.resolve()) }));

import { App } from "./App";
import { zh } from "./i18n/zh";

beforeEach(() => {
  getLiveSessionsCounts.mockReset();
  getLiveSessionsCounts.mockResolvedValue({ total: 0, running: 0, waiting: 0, archived: 0 });
  getLiveSessionsPage.mockReset();
  getLiveSessionsPage.mockResolvedValue([]);
  unlisten.mockReset();
  // mockReset 连实现一起清（防上个用例的 mockImplementation 泄漏），再装回默认空实现。
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(() => Promise.resolve(undefined));
  localStorage.clear();
  extraSettings = {};
  pillRectWidth = 0;
  resizeObservers.length = 0;
  macPanel = false;
});
afterEach(() => cleanup());

/** W-17：贴纸几何（尺寸/吸附边/置顶/位置）在 settings.json——用 invoke mock 播种读取值。 */
const seedWindowState = (s: {
  normal_width: number | null;
  normal_height: number | null;
  snap_edge: "left" | "right" | "top" | null;
  pinned: boolean;
  x: number | null;
  y: number | null;
  island_center?: number | null;
}) => {
  vi.mocked(invoke).mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "get_sticker_window_state" ? s : undefined)
  );
};
const EMPTY_WS = { normal_width: null, normal_height: null, snap_edge: null, pinned: false, x: null, y: null, island_center: null };

/** 在 macOS 面板模式下渲染会话列表（Windows 已没有列表，见 macPanel 注释）。 */
const renderList = async () => {
  macPanel = true;
  const result = render(<App />);
  await waitFor(() => expect(document.querySelector(".sticker")).toBeTruthy());
  return result;
};
/** 托盘找回 = 展开灵动岛面板。 */
const expandIsland = async () => {
  await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
  await act(async () => { emitRecallSticker(); });
  await waitFor(() => expect(document.querySelector(".isl-panel")).toBeTruthy());
};
const layoutCalls = () => vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "island_layout").map(([, args]) => args);

describe("App", () => {
  it("挂载时拉取 counts 和第 0 页", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(1));
    // search 初值为空字符串（App 自持有的真实搜索状态，非固定 null）。
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", "", null, 100));
  });

  // 回归：冷启动首页未落地前不能闪「还没有会话」假空态，必须显示加载占位。
  it("初始加载中显示加载占位，落地后再切到正常空态", async () => {
    let resolvePage: (v: unknown[]) => void = () => {};
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) => {
      if (search === null) return Promise.resolve([]); // 折叠条查询不挂起
      return new Promise((r) => { resolvePage = r as (v: unknown[]) => void; });
    });
    await renderList();
    await waitFor(() => expect(screen.getByText(zh.sticker.loading)).toBeTruthy());
    expect(screen.queryByText(zh.empty.allTitle)).toBeNull();
    resolvePage([]);
    await waitFor(() => expect(screen.getByText(zh.empty.allTitle)).toBeTruthy());
    expect(screen.queryByText(zh.sticker.loading)).toBeNull();
  });

  // 回归：首页加载失败曾只 console.error，用户看到「还没有会话」。现在必须显示
  // 「加载失败 + 重试」，且点重试会重新发起首页加载，成功后回到正常空态。
  it("初始加载失败显示错误与重试，重试成功后恢复正常", async () => {
    let failFirst = true;
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) => {
      if (search === null) return Promise.resolve([]);
      if (failFirst) {
        failFirst = false;
        return Promise.reject(new Error("boom"));
      }
      return Promise.resolve([]);
    });
    await renderList();
    await waitFor(() => expect(screen.getByText(zh.sticker.loadFailed)).toBeTruthy());
    expect(screen.queryByText(zh.empty.allTitle)).toBeNull();

    const callsBefore = getLiveSessionsPage.mock.calls.filter(([, s]) => s === "").length;
    fireEvent.click(screen.getByTestId("empty-retry-cta"));
    await waitFor(() =>
      expect(getLiveSessionsPage.mock.calls.filter(([, s]) => s === "").length).toBeGreaterThan(callsBefore)
    );
    await waitFor(() => expect(screen.getByText(zh.empty.allTitle)).toBeTruthy());
    expect(screen.queryByText(zh.sticker.loadFailed)).toBeNull();
  });

  it("收到 board-changed 后重新拉取 counts 和第 0 页", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(1));
    emitBoardChanged();
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", "", null, 100));
  });

  // T-12：「正在启动」占位卡（后端合成的负 id 占位项）随首页进列表；同 provider+cwd
  // 且 started_at 不早于占位的真实会话出现时对账撤卡（增量刷新也不留残影）。
  it("pending 占位卡（负 id）在真实会话出现时对账撤下", async () => {
    const startedAt = Date.now();
    const pendingRow = {
      session: { id: -1, project_id: 0, cc_session_id: "", status: "running", started_at: startedAt, last_event_at: startedAt, ended_at: null },
      project_name: "proj", task_title: "", current_activity: null, column: "doing",
      todo_done: 0, todo_total: 0, todos: [], pid: null, archived: false, archived_at: null,
      cwd: "C:/work/proj", context_pct: null, context_window: null, model: null, note: null,
      pending_review: null, last_ai_text: null, last_user_text: null, provider: "codex",
      profile: null, predecessor_id: null,
      connected: true, pty_managed: true, endable: true, background: false, errored: false,
      error_label: null, error_raw: null, busy_subagents: 0, preview: null,
      profile_name: null, foreign: false,
    };
    const realRow = {
      ...pendingRow,
      session: { ...pendingRow.session, id: 42, cc_session_id: "real-42", started_at: startedAt + 500, last_event_at: startedAt + 500 },
      task_title: "真实会话",
    };
    // 折叠条查询 search=null → 空；看板查询 search="" → 先只给占位行。
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) =>
      Promise.resolve(search === null ? [] : [pendingRow]));
    await renderList();
    await screen.findByTestId("stk-starting-card");
    // 认领完成：后端不再上报占位，真实行（同 provider+cwd、started_at 更晚）进首页。
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) =>
      Promise.resolve(search === null ? [] : [realRow]));
    emitBoardChanged();
    await waitFor(() => expect(screen.queryByTestId("stk-starting-card")).toBeNull());
    await screen.findByText("真实会话");
  });

  // 回归（DEV 实拍）：展开时窗口先长大，胶囊的测量回调恰在这时触发，按胶囊宽度把窗口又缩了
  // 回去，列表被挤进 36px 高的窗口里。形态切换进行中，测量不得改窗口尺寸。
  it("展开途中胶囊测量回调触发，不会把窗口缩回胶囊大小", async () => {
    let finishExpand: () => void = () => {};
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      const a = args as { height?: number } | undefined;
      if (cmd === "island_layout" && a?.height === 250) {
        return new Promise<void>((r) => { finishExpand = r; }) as never;
      }
      return Promise.resolve(undefined) as never;
    });
    render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    await act(async () => { emitRecallSticker(); });
    await waitFor(() => expect(layoutCalls()).toContainEqual({ width: 628, height: 250, centerRatio: null }));
    // 窗口改尺寸途中：胶囊被测到新宽度。
    pillRectWidth = 195;
    act(() => resizeObservers.forEach((o) => o.fire()));
    await act(async () => { finishExpand(); });
    await waitFor(() => expect(document.querySelector(".island-shell")).toBeTruthy());
    const calls = layoutCalls();
    expect(calls[calls.length - 1]).toEqual({ width: 628, height: 250, centerRatio: null });
  });

  it("点一下胶囊（没拖动）就展开面板", async () => {
    const { container } = render(<App />);
    const pill = await waitFor(() => {
      const el = container.querySelector(".isl-notch");
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    fireEvent.pointerDown(pill, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(pill, { button: 0, pointerId: 1, screenX: 101 });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("island_drag_begin");
    expect(layoutCalls()).toContainEqual({ width: 628, height: 250, centerRatio: null });
  });

  // 点胶囊上的会话图标直接打开那个会话；点胶囊别处才展开面板。
  it("点胶囊上的会话图标直接打开它，不展开面板", async () => {
    const row = {
      session: { id: 77, cc_session_id: "s-77", status: "running", last_event_at: 1000, started_at: 0, ended_at: null, project_id: 1 },
      project_name: "p", task_title: "点我打开", current_activity: null, column: "todo", todo_done: 0, todo_total: 0,
      todos: [], pid: null, connected: true, archived: false, archived_at: null, cwd: "/p", errored: false,
      error_label: null, error_raw: null, preview: null, note: null, context_pct: null, context_window: null,
      model: null, pending_review: null, last_ai_text: null, last_user_text: null, provider: "claude",
      screen_state: "working",
    };
    getLiveSessionsPage.mockImplementation((filter: string, search: string | null) =>
      Promise.resolve(search === null && filter === "running" ? [row] : []));
    const { container } = render(<App />);
    const icon = await waitFor(() => {
      const el = container.querySelector('[data-session-id="77"]');
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    expect(icon.getAttribute("title")).toContain("点我打开");
    const notch = container.querySelector(".isl-notch") as HTMLElement;
    fireEvent.pointerDown(icon, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(notch, { button: 0, pointerId: 1, screenX: 100 });
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_chat_window", { sessionId: 77 }));
    expect(container.querySelector(".isl-panel")).toBeNull();
  });

  it("收到提醒时胶囊弹出会话标题与状态，点「打开」进对话窗", async () => {
    render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    act(() => {
      emitIslandAlert({ payload: { sessionId: 7, kind: "approval", title: "修登录页", project: "web", detail: null } });
    });
    expect(await screen.findByText("修登录页")).toBeTruthy();
    expect(screen.getByText(zh.pending.approval)).toBeTruthy();
    expect(layoutCalls()).toContainEqual({ width: 548, height: 68, centerRatio: null });
    fireEvent.click(screen.getByRole("button", { name: zh.island.open }));
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_chat_window", { sessionId: 7 }));
    await waitFor(() => expect(screen.queryByText("修登录页")).toBeNull());
  });

  it("提醒按设置开关过滤：关掉「会话跑完」就不弹它，计划确认算在「等待审批」里", async () => {
    extraSettings = { island_alerts: ["approval"] };
    render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    // 等设置读进来（getSettings 是异步的）。
    await new Promise((r) => setTimeout(r, 20));
    act(() => {
      emitIslandAlert({ payload: { sessionId: 1, kind: "waiting", title: "跑完了", project: "p", detail: null } });
    });
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.queryByText("跑完了")).toBeNull();
    act(() => {
      emitIslandAlert({ payload: { sessionId: 2, kind: "plan", title: "计划待确认", project: "p", detail: null } });
    });
    expect(await screen.findByText("计划待确认")).toBeTruthy();
  });

  it("鼠标停在提醒上提醒不走；移开后几秒才缩回", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { container } = render(<App />);
      await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
      act(() => {
        emitIslandAlert({ payload: { sessionId: 5, kind: "approval", title: "停住看看", project: "p", detail: null } });
      });
      expect(await screen.findByText("停住看看")).toBeTruthy();
      const notch = container.querySelector(".isl-notch")!;
      fireEvent.pointerEnter(notch);
      // 远超默认停留 6 秒：人在看，提醒不走，也没被换成展开面板。
      await act(async () => { vi.advanceTimersByTime(10_000); });
      expect(screen.getByText("停住看看")).toBeTruthy();
      expect(container.querySelector(".isl-panel")).toBeNull();
      fireEvent.pointerLeave(notch);
      await act(async () => { vi.advanceTimersByTime(3_500); });
      await waitFor(() => expect(screen.queryByText("停住看看")).toBeNull());
    } finally {
      vi.useRealTimers();
    }
  });

  it("几条提醒一起来：排队一条条弹、显示还剩几条；同一会话只保留最新的", async () => {
    render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    const alertOf = (sessionId: number, title: string) =>
      ({ payload: { sessionId, kind: "waiting", title, project: "p", detail: null } });
    act(() => {
      emitIslandAlert(alertOf(1, "第一条"));
      emitIslandAlert(alertOf(2, "第二条"));
      emitIslandAlert(alertOf(3, "第三条"));
    });
    expect(await screen.findByText("第一条")).toBeTruthy();
    expect(screen.getByText(zh.island.moreAlerts(2))).toBeTruthy();
    // 会话 2 又来一条：替换排队中的旧的，不再多排一条。
    act(() => { emitIslandAlert(alertOf(2, "第二条（新）")); });
    expect(screen.getByText(zh.island.moreAlerts(2))).toBeTruthy();
  });

  // 用户 2026-11-05 要求：点开后光标移开不收，点岛外面才收。点到别的窗口靠失焦；
  // 点桌面空白这类不夺焦的地方，靠后端「鼠标按着且在窗口外」。
  it("展开的面板光标移开不收，点外面才收", async () => {
    let pressedOutside = false;
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "mouse_pressed_outside_window" ? pressedOutside : undefined) as never);
    const { container } = render(<App />);
    await expandIsland();
    await new Promise((r) => setTimeout(r, 250));
    expect(container.querySelector(".isl-panel")).toBeTruthy();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("cursor_over_window");
    pressedOutside = true;
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeNull());
    // 失焦同样收起。
    pressedOutside = false;
    await expandIsland();
    act(() => { window.dispatchEvent(new Event("blur")); });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeNull());
  });

  // 极简模式（用户 2026-11-05 要求）：收起时只在顶边留一条细条，点它展开；面板标题栏的按钮切换。
  it("极简模式收起成顶边细条，点它展开；面板按钮能切回来", async () => {
    extraSettings = { island_strip: true };
    setSettingsMock.mockClear();
    const { container } = render(<App />);
    const strip = await waitFor(() => {
      const el = container.querySelector(".isl-strip");
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    expect(layoutCalls()).toContainEqual({ width: 160, height: 8, centerRatio: 0.5 });
    expect(container.querySelector(".isl-notch.is-compact")).toBeNull();
    fireEvent.pointerDown(strip, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(strip, { button: 0, pointerId: 1, screenX: 100 });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: zh.island.stripOff }));
    await waitFor(() => expect(setSettingsMock).toHaveBeenCalledWith(expect.objectContaining({ island_strip: false })));
    expect(screen.getByRole("button", { name: zh.island.stripOn })).toBeTruthy();
  });

  it("极简模式：鼠标移上细条变回胶囊，移开缩回细条", async () => {
    extraSettings = { island_strip: true };
    let over = true;
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "cursor_over_window" ? over : undefined) as never);
    const { container } = render(<App />);
    const strip = await waitFor(() => {
      const el = container.querySelector(".isl-strip");
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    fireEvent.pointerEnter(strip);
    await waitFor(() => expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy());
    expect(container.querySelector(".isl-strip")).toBeNull();
    expect(layoutCalls().at(-1)).toMatchObject({ height: 46 });
    // 光标还在上面：一直是胶囊。
    await new Promise((r) => setTimeout(r, 450));
    expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy();
    over = false;
    await waitFor(() => expect(container.querySelector(".isl-strip")).toBeTruthy());
    expect(layoutCalls().at(-1)).toMatchObject({ width: 160, height: 8 });
  });

  it("展开着的时候不弹提醒（人就在看面板）", async () => {
    render(<App />);
    await expandIsland();
    act(() => {
      emitIslandAlert({ payload: { sessionId: 7, kind: "question", title: "不该弹出来", project: "web", detail: null } });
    });
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByText("不该弹出来")).toBeNull();
  });

  it("清空搜索时恢复搜索前的列表顺序", async () => {
    const mk = (id: number, title: string, lastEventAt: number) => ({
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
    const original = [mk(2, "B", 2000), mk(1, "A", 1000)];
    getLiveSessionsCounts.mockResolvedValue({ total: 2, running: 2, waiting: 0, archived: 0 });
    let emptySearchCalls = 0;
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) => {
      if (search === null) return Promise.resolve([]);
      if (search === "A") return Promise.resolve([original[1]]);
      emptySearchCalls += 1;
      if (emptySearchCalls === 1) return Promise.resolve(original);
      // 模拟清空后的刷新尚未返回：原列表应从缓存立即恢复，不能依赖这次响应。
      return new Promise(() => {});
    });

    const { container } = await renderList();
    const titles = () =>
      Array.from(container.querySelectorAll(".stk-title")).map((el) => el.textContent);
    await waitFor(() => expect(titles()).toEqual(["B", "A"]));

    // 按 aria-label 定位搜索按钮，不吃底栏按钮增减的位置变化。
    fireEvent.click(container.querySelector('.stk-bar-actions [aria-label="搜索会话"]')!);
    fireEvent.change(container.querySelector(".stk-search-in")!, { target: { value: "A" } });
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", "A", null, 100));
    await waitFor(() => expect(titles()).toEqual(["A"]));

    fireEvent.click(container.querySelector(".stk-search-x")!);
    await waitFor(() => expect(titles()).toEqual(["B", "A"]));
  });

  // 启动：按 settings 记住的横向位置把胶囊贴顶摆好（缺省居中 0.5），再显示窗口。
  it("启动把胶囊贴顶摆放：没记过位置时居中", async () => {
    render(<App />);
    await waitFor(() =>
      expect(layoutCalls()).toContainEqual({ width: 148, height: 46, centerRatio: 0.5 })
    );
  });

  it("启动按记住的横向位置摆放；旧的吸附边不再生效", async () => {
    seedWindowState({ ...EMPTY_WS, snap_edge: "left", island_center: 0.3 });
    render(<App />);
    await waitFor(() =>
      expect(layoutCalls()).toContainEqual({ width: 148, height: 46, centerRatio: 0.3 })
    );
  });

  // W-17 一次性迁移：旧版三套 localStorage 键（尺寸/吸附边/置顶）在 settings 字段缺席时
  // 并入 sticker_window 原子落盘，旧键随即删除；此后 settings.json 是唯一真相源。
  it("W-17：localStorage 旧键一次性迁移进 settings 并删除旧键", async () => {
    localStorage.setItem("meowo-normal-size", JSON.stringify({ w: 420, h: 500 }));
    localStorage.setItem("meowo-snap-edge", "top");
    localStorage.setItem("meowo-pinned", "1");
    render(<App />);
    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_sticker_window_state", {
        state: { normal_width: 420, normal_height: 500, snap_edge: "top", pinned: true, x: null, y: null, island_center: null },
      })
    );
    expect(localStorage.getItem("meowo-normal-size")).toBeNull();
    expect(localStorage.getItem("meowo-snap-edge")).toBeNull();
    expect(localStorage.getItem("meowo-pinned")).toBeNull();
  });

  it("W-17：settings 已有几何值时，残留旧键不覆盖（仅清除），不触发迁移写", async () => {
    seedWindowState({ ...EMPTY_WS, normal_width: 400, normal_height: 460 });
    localStorage.setItem("meowo-normal-size", JSON.stringify({ w: 999, h: 999 }));
    render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    expect(localStorage.getItem("meowo-normal-size")).toBeNull();
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("set_sticker_window_state", expect.anything());
  });

  // 回归：running tab 下刷新时，若某会话已从 running 迁移到 waiting，后端 running 分页不再返回它；
  // 旧合并逻辑会保留 prev 中的该会话（状态仍是旧 running），导致它错误地继续显示在 running tab 下。
  it("running tab 刷新时，状态迁出 running 的会话应从列表移除", async () => {
    localStorage.setItem("meowo-tab", "running");
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
    // tab 页按序返回；折叠条调用（search===null，独立于 tab）返回 []，不消耗序列。
    const tabPages = [
      [mk(1, "running", "A"), mk(2, "running", "B")],
      [mk(1, "running", "A")], // B 已迁出 running
    ];
    let ti = 0;
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) => {
      if (search === null) return Promise.resolve([]);
      return Promise.resolve(tabPages[Math.min(ti++, tabPages.length - 1)]);
    });

    await renderList();
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
    localStorage.setItem("meowo-tab", "all");
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
    // 首页：B（last_event_at 更晚）排前，A（旧）排后；board-changed 后 A 恢复排最前。
    // tab 页按序返回；折叠条调用（search===null）返回 []，不消耗序列。
    const tabPages = [
      [mk(2, 2000, "B"), mk(1, 1000, "A")],
      [mk(1, 3000, "A"), mk(2, 2000, "B")],
    ];
    let ti = 0;
    getLiveSessionsPage.mockImplementation((_f: string, search: string | null) => {
      if (search === null) return Promise.resolve([]);
      return Promise.resolve(tabPages[Math.min(ti++, tabPages.length - 1)]);
    });

    await renderList();
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
