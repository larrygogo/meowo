// 灵动岛提醒可关、清空队列、悬停暂停上限，以及最大化窗口下自动收成细条（UI 优化 acceptance #31 #32）。
// mock 骨架照搬 App.test.tsx（规则：新测试放新文件）。
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor, screen, act } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";

const getLiveSessionsCounts = vi.fn();
const getLiveSessionsPage = vi.fn();
const setSettingsMock = vi.fn((_s: Record<string, unknown>) => Promise.resolve());
let emitIslandAlert: (e: { payload: unknown }) => void = () => {};
let emitRecallSticker: () => void = () => {};
const unlisten = vi.fn();
let emitTitlebar: (e: { payload: unknown }) => void = () => {};
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
    if (event === "recall-sticker") emitRecallSticker = cb as unknown as () => void;
    if (event === "island-alert") emitIslandAlert = cb;
    if (event === "island-titlebar") emitTitlebar = cb;
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

const layoutCalls = () => vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "island_layout").map(([, args]) => args);

const alertOf = (sessionId: number, title: string, kind = "approval") =>
  ({ payload: { sessionId, kind, title, project: "p", detail: null } });
const booted = () => waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("island_layout", expect.anything()));

describe("#31 提醒可关、可清空、悬停有上限", () => {
  it("点 × 关掉当前这条，排队的下一条接着弹", async () => {
    render(<App />);
    await booted();
    act(() => {
      emitIslandAlert(alertOf(1, "第一条"));
      emitIslandAlert(alertOf(2, "第二条"));
    });
    expect(await screen.findByText("第一条")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.island.dismiss }));
    await waitFor(() => expect(screen.queryByText("第一条")).toBeNull());
    expect(await screen.findByText("第二条")).toBeTruthy();
    // 关掉不是打开：没去开对话窗。
    expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("open_chat_window", expect.anything());
  });

  it("「全部忽略」清空队列，后面排着的都不再弹", async () => {
    const { container } = render(<App />);
    await booted();
    act(() => {
      emitIslandAlert(alertOf(1, "一"));
      emitIslandAlert(alertOf(2, "二"));
      emitIslandAlert(alertOf(3, "三"));
    });
    expect(await screen.findByText("一")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.island.dismissAll }));
    await waitFor(() => expect(screen.queryByText("一")).toBeNull());
    await new Promise((r) => setTimeout(r, 450));
    expect(screen.queryByText("二")).toBeNull();
    expect(screen.queryByText("三")).toBeNull();
    expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy();
  });

  it("只有一条时不显示「全部忽略」，× 就够了", async () => {
    render(<App />);
    await booted();
    act(() => { emitIslandAlert(alertOf(1, "独一条")); });
    expect(await screen.findByText("独一条")).toBeTruthy();
    expect(screen.getByRole("button", { name: zh.island.dismiss })).toBeTruthy();
    expect(screen.queryByRole("button", { name: zh.island.dismissAll })).toBeNull();
  });

  it("鼠标一直停在提醒上，最多留 15 秒", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { container } = render(<App />);
      await booted();
      act(() => { emitIslandAlert(alertOf(5, "停太久")); });
      expect(await screen.findByText("停太久")).toBeTruthy();
      fireEvent.pointerEnter(container.querySelector(".isl-notch")!);
      await act(async () => { await vi.advanceTimersByTimeAsync(14_000); });
      expect(screen.getByText("停太久")).toBeTruthy();
      await act(async () => { await vi.advanceTimersByTimeAsync(1_500); });
      await waitFor(() => expect(screen.queryByText("停太久")).toBeNull());
    } finally {
      vi.useRealTimers();
    }
  });

  it("悬停中来了急事也不插队（人在看）", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { container } = render(<App />);
      await booted();
      act(() => { emitIslandAlert(alertOf(1, "跑完了", "waiting")); });
      expect(await screen.findByText("跑完了")).toBeTruthy();
      fireEvent.pointerEnter(container.querySelector(".isl-notch")!);
      act(() => { emitIslandAlert(alertOf(2, "要审批")); });
      await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
      expect(screen.getByText("跑完了")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("#32 最大化窗口的标题栏不被岛挡住", () => {
  it("后端报「岛压在最大化窗口标题栏上」：胶囊收成细条；移开最大化窗口就恢复", async () => {
    const { container } = render(<App />);
    await booted();
    await waitFor(() => expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy());
    await act(async () => { emitTitlebar({ payload: true }); });
    await waitFor(() => expect(container.querySelector(".isl-strip")).toBeTruthy());
    expect(layoutCalls().at(-1)).toMatchObject({ width: 160, height: 8 });
    await act(async () => { emitTitlebar({ payload: false }); });
    await waitFor(() => expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy());
    expect(layoutCalls().at(-1)).toMatchObject({ height: 46 });
  });

  it("自动收起的细条，光标靠近照样变回胶囊", async () => {
    let over = true;
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "cursor_over_window" ? over : undefined) as never);
    const { container } = render(<App />);
    await booted();
    await act(async () => { emitTitlebar({ payload: true }); });
    const strip = await waitFor(() => {
      const el = container.querySelector(".isl-strip");
      expect(el).toBeTruthy();
      return el as HTMLElement;
    });
    fireEvent.pointerEnter(strip);
    await waitFor(() => expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy());
    over = false;
    await waitFor(() => expect(container.querySelector(".isl-strip")).toBeTruthy());
  });

  it("设置里关掉自动收起：最大化窗口在下面也保持胶囊", async () => {
    extraSettings = { island_auto_strip: false };
    const { container } = render(<App />);
    await booted();
    await new Promise((r) => setTimeout(r, 20));
    await act(async () => { emitTitlebar({ payload: true }); });
    await new Promise((r) => setTimeout(r, 50));
    expect(container.querySelector(".isl-strip")).toBeNull();
    expect(container.querySelector(".isl-notch.is-compact")).toBeTruthy();
  });

  it("自动收起不改用户的极简模式开关：面板按钮仍显示「切换到极简模式」", async () => {
    const { container } = render(<App />);
    await booted();
    await act(async () => { emitTitlebar({ payload: true }); });
    await waitFor(() => expect(container.querySelector(".isl-strip")).toBeTruthy());
    await act(async () => { emitRecallSticker(); });
    await waitFor(() => expect(container.querySelector(".isl-panel")).toBeTruthy());
    expect(screen.getByRole("button", { name: zh.island.stripOn }).getAttribute("aria-pressed")).toBe("false");
  });
});
