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
let emitTrayToggle: () => void = () => {};
// 贴纸 2026-11-05 在两个平台都删了，App 只剩灵动岛。macOS 用例把 isMac 打开，落点由
// island_host 的 mock 返回值决定（刘海 / 菜单栏）。
let mac = false;
vi.mock("./platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./platform")>()),
  isMac: () => mac,
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

const openNewSessionWindowMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("./api", () => ({
  openNewSessionWindow: () => openNewSessionWindowMock(),
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
    if (event === "island-tray-toggle") emitTrayToggle = cb as unknown as () => void;
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
  mac = false;
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
  island_monitor?: string | null;
}) => {
  vi.mocked(invoke).mockImplementation((cmd: string) =>
    Promise.resolve(cmd === "get_sticker_window_state" ? s : undefined)
  );
};
const EMPTY_WS = { normal_width: null, normal_height: null, snap_edge: null, pinned: false, x: null, y: null, island_center: null, island_monitor: null };

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

  it("收到 board-changed 后重新拉取 counts 和第 0 页", async () => {
    render(<App />);
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(1));
    emitBoardChanged();
    await waitFor(() => expect(getLiveSessionsCounts).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(getLiveSessionsPage).toHaveBeenCalledWith("all", "", null, 100));
  });

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

  // 审查 2026-11-09：collapse 防住了重复收起，但动作本身会再执行一次，双击「新建会话」就开两次窗口。
  it("面板上快速连点两次新建会话，只开一次窗口", async () => {
    openNewSessionWindowMock.mockClear();
    const { container } = render(<App />);
    const pill = await waitFor(() => {
      const el = container.querySelector(".isl-notch");
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    fireEvent.pointerDown(pill, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(pill, { button: 0, pointerId: 1, screenX: 101 });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    const button = screen.getByRole("button", { name: zh.sticker.newSession });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(openNewSessionWindowMock).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeNull());
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

  const pillOf = async (container: HTMLElement) => waitFor(() => {
    const el = container.querySelector(".isl-notch");
    expect(el).toBeTruthy();
    return el as HTMLElement;
  });
  const clickPill = (pill: HTMLElement) => {
    fireEvent.pointerDown(pill, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(pill, { button: 0, pointerId: 1, screenX: 100 });
  };
  /// 让展开（628 宽的 island_layout）挂起，由用例决定何时成功或失败。
  const holdGrow = () => {
    const base = vi.mocked(invoke).getMockImplementation()!;
    const gate = { resolve: () => {}, reject: () => {} } as { resolve: () => void; reject: () => void };
    vi.mocked(invoke).mockImplementation(((cmd: string, args?: Record<string, unknown>) =>
      cmd === "island_layout" && (args as { width: number }).width === 628
        ? new Promise<void>((resolve, reject) => { gate.resolve = resolve; gate.reject = () => reject(new Error("boom")); })
        : base(cmd, args)) as typeof invoke);
    return gate;
  };

  // 审查复现：失去指针捕获 / 触控手势被系统接管（pointercancel）不是点击。
  it("指针被系统取消时不当成点击，胶囊不展开", async () => {
    const { container } = render(<App />);
    const pill = await pillOf(container);
    fireEvent.pointerDown(pill, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerCancel(pill, { button: 0, pointerId: 1, screenX: 100 });
    await new Promise((r) => setTimeout(r, 30));
    expect(container.querySelector(".isl-panel")).toBeNull();
    expect(layoutCalls()).not.toContainEqual({ width: 628, height: 250, centerRatio: null });
  });

  // 审查复现：展开在等窗口长大时来了提醒，提醒尺寸盖掉展开尺寸，面板被截在提醒高度里。
  it("展开途中到达的提醒不弹，窗口停在面板尺寸", async () => {
    const gate = holdGrow();
    const { container } = render(<App />);
    clickPill(await pillOf(container));
    await waitFor(() => expect(layoutCalls()).toContainEqual({ width: 628, height: 250, centerRatio: null }));
    act(() => {
      emitIslandAlert({ payload: { sessionId: 7, kind: "approval", title: "修登录页", project: "web", detail: null } });
    });
    await act(async () => { gate.resolve(); });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    expect(layoutCalls().at(-1)).toEqual({ width: 628, height: 250, centerRatio: null });
    expect(layoutCalls().some((args) => (args as { width: number }).width === 548)).toBe(false);
  });

  it("展开失败时，途中到达的提醒回到胶囊上补弹", async () => {
    const gate = holdGrow();
    const { container } = render(<App />);
    clickPill(await pillOf(container));
    await waitFor(() => expect(layoutCalls()).toContainEqual({ width: 628, height: 250, centerRatio: null }));
    act(() => {
      emitIslandAlert({ payload: { sessionId: 7, kind: "approval", title: "修登录页", project: "web", detail: null } });
    });
    await act(async () => { gate.reject(); });
    expect(await screen.findByText("修登录页")).toBeTruthy();
  });

  // 审查复现：收起动画途中点托盘找回（= 展开），旧的收起定时器到点照样把面板缩回胶囊。
  it("收起途中托盘找回，面板不被旧的收起定时器缩回去", async () => {
    const { container } = render(<App />);
    clickPill(await pillOf(container));
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    fireEvent.keyDown(window, { key: "Escape" });
    await act(async () => { emitRecallSticker(); });
    await new Promise((r) => setTimeout(r, 400));
    expect(container.querySelector(".isl-panel")).toBeTruthy();
    expect(layoutCalls().at(-1)).toEqual({ width: 628, height: 250, centerRatio: null });
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

  // 二期 #1（2026-11-10 定）：提醒在场时点正文 = 打开那条会话，与系统通知横幅一致；排在后面的提醒照常弹。
  it("提醒显示时点正文打开该会话，排队的提醒不被清空", async () => {
    const { container } = render(<App />);
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
    act(() => {
      emitIslandAlert({ payload: { sessionId: 11, kind: "approval", title: "正文点我", project: "p", detail: null } });
      emitIslandAlert({ payload: { sessionId: 12, kind: "waiting", title: "排在后面", project: "p", detail: null } });
    });
    expect(await screen.findByText("正文点我")).toBeTruthy();
    const notch = container.querySelector(".isl-notch") as HTMLElement;
    fireEvent.pointerDown(notch, { button: 0, pointerId: 1, screenX: 100 });
    fireEvent.pointerUp(notch, { button: 0, pointerId: 1, screenX: 100 });
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_chat_window", { sessionId: 11 }));
    expect(container.querySelector(".isl-panel")).toBeNull();
    // 300ms 间隔后排队的那条接着弹。
    expect(await screen.findByText("排在后面")).toBeTruthy();
  });

  // 二期 #2：审批/提问/出错插到队头；正在显示的「跑完」顶多再留一小会儿就让位（原来要等约 19s）。
  it("连发 3 条跑完再来审批，1.5s 内显示审批", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<App />);
      await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));
      const waiting = (sessionId: number, title: string) =>
        ({ payload: { sessionId, kind: "waiting", title, project: "p", detail: null } });
      act(() => {
        emitIslandAlert(waiting(1, "跑完一"));
        emitIslandAlert(waiting(2, "跑完二"));
        emitIslandAlert(waiting(3, "跑完三"));
      });
      expect(await screen.findByText("跑完一")).toBeTruthy();
      act(() => {
        emitIslandAlert({ payload: { sessionId: 4, kind: "approval", title: "要审批", project: "p", detail: null } });
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(1_500); });
      expect(screen.getByText("要审批")).toBeTruthy();
      // 被插队的跑完提醒没丢：审批后面还排着两条。
      expect(screen.getByText(zh.island.moreAlerts(2))).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
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
        state: { normal_width: 420, normal_height: 500, snap_edge: "top", pinned: true, x: null, y: null, island_center: null, island_monitor: null },
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

});

describe("App · macOS 灵动岛", () => {
  const seedHost = (host: { kind: string; notchWidth: number; notchHeight: number }) => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "get_sticker_window_state" ? EMPTY_WS : cmd === "island_host" ? host : undefined)
    );
  };
  const visibleCalls = () => vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "island_set_visible").map(([, args]) => args);

  it("有刘海：胶囊高度等于刘海高，内容分到刘海两侧，摆好后不抢焦点地显示", async () => {
    mac = true;
    seedHost({ kind: "notch", notchWidth: 180, notchHeight: 32 });
    const { container } = render(<App />);
    await waitFor(() => expect(layoutCalls().some((args) => (args as { height: number }).height === 32)).toBe(true));
    await waitFor(() => expect(visibleCalls()).toContainEqual({ visible: true, focus: false }));
    await waitFor(() => expect(container.querySelector(".isl-notch.is-split")).toBeTruthy());
    expect(container.querySelector(".isl-notch-gap")).toBeTruthy();
    // 刘海落点不能拖，也没有极简模式。
    expect(container.querySelector(".isl-strip")).toBeNull();
    // 菜单栏图标的神态与文字由这里推（没会话：空闲、无文字、眼睛睁着，表情与岛上同一套）。
    await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("set_menubar_status", { tone: "idle", segments: [], mood: "open" }));
  });

  it("没刘海：平时藏着，点菜单栏的迷你岛在图标下展开面板，再点收起", async () => {
    mac = true;
    seedHost({ kind: "menubar", notchWidth: 0, notchHeight: 0 });
    const { container } = render(<App />);
    await waitFor(() => expect(visibleCalls()).toContainEqual({ visible: false, focus: false }));
    expect(layoutCalls()).toHaveLength(0);
    await act(async () => { emitTrayToggle(); });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    await waitFor(() => expect(visibleCalls()).toContainEqual({ visible: true, focus: true }));
    // 面板上没有极简模式按钮（只有 Windows 有）。
    expect(screen.queryByRole("button", { name: zh.island.stripOn })).toBeNull();
    // 收起 = 藏窗口；面板内容留在窗口里，下次一显示就是画好的（不闪空玻璃）。
    await act(async () => { emitTrayToggle(); });
    await waitFor(() => expect(visibleCalls().at(-1)).toEqual({ visible: false, focus: false }));
    expect(container.querySelector(".isl-panel")).toBeTruthy();
    // 刚收起的半秒内再点图标（按下时已经算「点了外面」收起过），不再弹开。
    const shown = visibleCalls().length;
    await act(async () => { emitTrayToggle(); });
    await new Promise((r) => setTimeout(r, 120));
    expect(visibleCalls()).toHaveLength(shown);
  });
});
