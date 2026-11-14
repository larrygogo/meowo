// 灵动岛「打开」失败的去处（I-1/I-2）、面板键盘（I-3）、额度陈旧标注与登录后重读（backlog 仍存在 #1）。
// 出处：scratchpad/ux2/research-walkthrough.md I-1～I-3、research-backlog.md「仍存在」第 1 条（2026-11-12）。
// 兄弟场景：提醒上的「打开」与面板会话行同一个出口（openIslandSession），两处都验；收起胶囊的额度环
// 与面板额度页同一份新鲜度（meta），两处都验。
import { describe, it, expect, vi, afterEach } from "vitest";

const { invoke, listeners } = vi.hoisted(() => ({
  invoke: vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(null as unknown)),
  listeners: new Map<string, ((event: { payload: unknown }) => void)[]>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(event, [...(listeners.get(event) ?? []), cb]);
    return Promise.resolve(() => {});
  },
}));
import { render, cleanup, fireEvent, screen, act, renderHook, waitFor } from "@testing-library/react";
import { Island } from "../Island";
import { IslandPanel } from "./IslandPanel";
import { UsageRing } from "./islandParts";
import { openIslandSession } from "./openSession";
import { liveIslandUsage, useIslandUsage } from "./useIslandUsage";
import type { Item } from "../sticker/types";
import { zh } from "../../i18n/zh";

afterEach(() => {
  cleanup();
  invoke.mockReset();
  invoke.mockImplementation(() => Promise.resolve(null));
  listeners.clear();
});

let nextId = 1;
const mk = (over: Record<string, unknown> = {}): Item => {
  const id = nextId++;
  return {
    session: { id, project_id: 1, cc_session_id: `s-${id}`, status: "running", started_at: id, last_event_at: 1_000, ended_at: null },
    project_name: "proj", task_title: `会话 ${id}`, current_activity: null, column: "doing",
    todo_done: 0, todo_total: 0, todos: [], pid: 4242, archived: false, cwd: "C:/work/proj",
    connected: true, pty_managed: false, endable: true, background: false,
    errored: false, error_label: null, error_raw: null, busy_subagents: 0,
    preview: null, profile_name: null, foreign: false, pending_review: null,
    provider: "claude", screen_state: "working",
    ...over,
  } as unknown as Item;
};
const noop = () => {};

describe("I-1/I-2 openIslandSession", () => {
  it("对话窗没开成：返回失败与映射后的原因，不再只打日志", async () => {
    vi.spyOn(console, "error").mockImplementation(noop);
    invoke.mockImplementation((cmd: string) => (cmd === "open_chat_window" ? Promise.reject("会话不存在") : Promise.resolve(null)));
    const result = await openIslandSession({ sessionId: 7, chatEnabled: true, item: undefined, locale: "zh-CN" });
    expect(result).toEqual({ sessionId: 7, kind: "failed", reason: "会话不存在" });
  });
  it("开成了返回 null", async () => {
    expect(await openIslandSession({ sessionId: 7, chatEnabled: true, item: undefined, locale: "zh-CN" })).toBeNull();
    expect(invoke).toHaveBeenCalledWith("open_chat_window", { sessionId: 7 });
  });
  it("轻量模式：不开对话窗（后端会改开设置窗），改为把外部终端拉到前台", async () => {
    const item = mk();
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "focus_session" ? "focused" : null));
    expect(await openIslandSession({ sessionId: item.session.id, chatEnabled: false, item, locale: "zh-CN" })).toBeNull();
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(["focus_session"]);
    expect(invoke.mock.calls[0][1]).toMatchObject({ pid: 4242, sessionId: item.session.cc_session_id, provider: "claude", cwd: "C:/work/proj" });
  });
  it("轻量模式跳不过去（进程已结束 / 没有 pid）：说明轻量模式没有对话窗", async () => {
    const item = mk();
    invoke.mockImplementation(() => Promise.resolve("process_ended"));
    expect(await openIslandSession({ sessionId: item.session.id, chatEnabled: false, item, locale: "zh-CN" })).toEqual({ sessionId: item.session.id, kind: "lite" });
    invoke.mockClear();
    const noPid = mk({ pid: null });
    expect(await openIslandSession({ sessionId: noPid.session.id, chatEnabled: false, item: noPid, locale: "zh-CN" })).toEqual({ sessionId: noPid.session.id, kind: "lite" });
    expect(invoke).not.toHaveBeenCalled();
  });
});

const alertOf = (sessionId: number) => ({ sessionId, kind: "approval", title: "修登录", project: "proj", detail: null }) as never;
const renderAlert = (props: Partial<Parameters<typeof Island>[0]>) =>
  render(<Island sessions={[]} compactStyle="counts" alert={alertOf(5)} usages={[]} onMeasure={noop} onActivate={noop} onOpenSession={noop} onPointerDownDrag={noop} {...props} />);

describe("I-1/I-2 提醒上的失败提示", () => {
  it("开窗失败：提醒换成出错样式写明原因，主按钮变「重试」并再次打开同一条", () => {
    const onOpenSession = vi.fn();
    renderAlert({ onOpenSession, openError: { sessionId: 5, kind: "failed", reason: "会话不存在" } });
    expect(document.querySelector(".isl-alert-sub.is-error")?.textContent).toContain(zh.island.openFailed);
    expect(document.querySelector(".isl-alert-sub.is-error")?.textContent).toContain("会话不存在");
    expect(screen.getByRole("alert").textContent).toContain(zh.island.openFailed);
    fireEvent.click(screen.getByRole("button", { name: zh.island.retry }));
    expect(onOpenSession).toHaveBeenCalledWith(5);
  });
  it("轻量模式跳不过去：说明原因，主按钮是「打开设置」", () => {
    const onOpenSettingsFromError = vi.fn();
    renderAlert({ openError: { sessionId: 5, kind: "lite" }, onOpenSettingsFromError });
    expect(document.querySelector(".isl-alert-sub.is-error")?.textContent).toBe(zh.island.liteNoChat);
    fireEvent.click(screen.getByRole("button", { name: zh.island.openSettings }));
    expect(onOpenSettingsFromError).toHaveBeenCalled();
  });
  it("别的会话的失败不串到这条提醒上", () => {
    renderAlert({ openError: { sessionId: 99, kind: "lite" } });
    expect(document.querySelector(".isl-alert-sub.is-error")).toBeNull();
    expect(screen.getByRole("button", { name: zh.island.open })).toBeTruthy();
  });
});

const renderPanel = (sessions: Item[], props: Partial<Parameters<typeof IslandPanel>[0]> = {}) =>
  render(<IslandPanel sessions={sessions} usage={{}} quotaProviders={[]} onOpenSession={noop} onMeasure={noop} onPointerDownDrag={noop} {...props} />);

describe("I-1/I-2 面板里的失败提示", () => {
  it("点会话失败：面板留着，顶上有原因、「重试」和关闭", () => {
    const onRetryOpen = vi.fn();
    const onDismissOpenError = vi.fn();
    renderPanel([mk()], { openError: { sessionId: 3, kind: "failed", reason: "会话不存在" }, onRetryOpen, onDismissOpenError });
    const bar = screen.getByRole("alert");
    expect(bar.textContent).toContain(zh.island.openFailed);
    expect(bar.textContent).toContain("会话不存在");
    fireEvent.click(screen.getByRole("button", { name: zh.island.retry }));
    expect(onRetryOpen).toHaveBeenCalledWith(3);
    fireEvent.click(screen.getByRole("button", { name: zh.island.dismissError }));
    expect(onDismissOpenError).toHaveBeenCalled();
  });
  it("轻量模式：给「打开设置」", () => {
    const onOpenSettingsFromError = vi.fn();
    renderPanel([mk()], { openError: { sessionId: 3, kind: "lite" }, onOpenSettingsFromError });
    expect(screen.getByRole("alert").textContent).toContain(zh.island.liteNoChat);
    fireEvent.click(screen.getByRole("button", { name: zh.island.openSettings }));
    expect(onOpenSettingsFromError).toHaveBeenCalled();
  });
});

describe("I-3 面板键盘", () => {
  const focusedTitle = () => document.activeElement?.querySelector(".isl-row-title")?.textContent;
  it("展开即聚焦第一条会话，↓↑ 在行间移动，到页尾自动翻到下一页、到页首翻回上一页", () => {
    renderPanel([mk(), mk(), mk(), mk(), mk()]);
    const first = document.activeElement as HTMLElement;
    expect(first.classList.contains("isl-row")).toBe(true);
    const page1 = [...document.querySelectorAll(".isl-row-title")].map((el) => el.textContent);
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(focusedTitle()).toBe(page1[1]);
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
    // 第 2 页第一条：已自动翻页，当前页圆点跟着走。
    expect(document.activeElement?.classList.contains("isl-row")).toBe(true);
    expect(screen.getByRole("button", { name: zh.island.page(2) }).getAttribute("aria-current")).toBe("page");
    expect(page1).not.toContain(focusedTitle());
    fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
    expect(screen.getByRole("button", { name: zh.island.page(1) }).getAttribute("aria-current")).toBe("page");
    expect(focusedTitle()).toBe(page1[2]);
  });
  it("菜单栏落点收起时面板仍挂着：不抢焦点", () => {
    renderPanel([mk()], { focusOnOpen: false });
    expect(document.activeElement).toBe(document.body);
  });
});

describe("backlog #1 额度陈旧标注", () => {
  const usage = { claude: { lanes: [{ kind: "five_hour", used_pct: 40, label: null, resets_at: null }], note: null } } as never;
  it("刷新失败的那家在收起环上降饱和，悬停写上次更新时间", () => {
    const at = new Date(2026, 10, 12, 9, 5).getTime();
    const entries = liveIslandUsage(usage, ["claude"], ["claude"], { claude: { fetchedAt: at, failed: true } });
    expect(entries[0]).toMatchObject({ provider: "claude", stale: true, fetchedAt: at });
    render(<UsageRing entries={entries} staleTip={(when) => `tip:${when}`} />);
    const wrap = document.querySelector(".isl-usage-wrap")!;
    expect(wrap.classList.contains("is-stale")).toBe(true);
    expect(wrap.getAttribute("title")).toBe(`tip:${at}`);
  });
  it("新鲜的不标", () => {
    expect(liveIslandUsage(usage, ["claude"], ["claude"], { claude: { fetchedAt: 1, failed: false } })[0].stale).toBeUndefined();
  });
  it("面板额度页：陈旧的那家写明「上次更新」，从没成功过的写「显示的是缓存」", () => {
    const at = new Date(2026, 10, 12, 9, 5).getTime();
    renderPanel([], { usage, quotaProviders: ["claude"], usageMeta: { claude: { fetchedAt: at, failed: true } }, initialPage: 1 });
    const group = document.querySelector(".isl-usage-group")!;
    expect(group.classList.contains("is-stale")).toBe(true);
    expect(group.querySelector(".isl-usage-stale")?.textContent).toBe(zh.sticker.usageStale("09:05"));
    cleanup();
    renderPanel([], { usage, quotaProviders: ["claude"], usageMeta: { claude: { fetchedAt: null, failed: true } }, initialPage: 1 });
    expect(document.querySelector(".isl-usage-stale")?.textContent).toBe(zh.sticker.staleNotice);
  });
});

describe("backlog #1 useIslandUsage", () => {
  const lane = (pct: number) => ({ lanes: [{ kind: "five_hour", used_pct: pct, label: null, resets_at: null }], note: null });
  it("刷新失败记成陈旧（旧值保留），登录完成后重读账号、新登录的那家不重启就出额度", async () => {
    let accounts: Record<string, unknown>[] = [
      { provider: "claude", account: { email: "a" }, usage: lane(10), usage_supported: true, relay_enabled: false },
      { provider: "codex", account: null, usage: null, usage_supported: true, relay_enabled: false },
    ];
    invoke.mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "get_accounts") return Promise.resolve(accounts);
      if (cmd === "refresh_usage") {
        const provider = (args as { provider: string }).provider;
        return provider === "claude" ? Promise.reject("网络断了") : Promise.resolve(lane(55));
      }
      return Promise.resolve(null);
    });
    const { result } = renderHook(() => useIslandUsage());
    await waitFor(() => expect(result.current.meta.claude?.failed).toBe(true));
    expect(result.current.usage.claude.lanes[0].used_pct).toBe(10);
    expect(result.current.meta.claude.fetchedAt).toBeNull();
    expect(result.current.usage.codex).toBeUndefined();
    // codex 登录完成：login-done 到达，岛重读账号并开始刷新它。
    accounts = accounts.map((a) => (a.provider === "codex" ? { ...a, account: { email: "b" } } : a));
    await act(async () => { listeners.get("login-done")?.forEach((cb) => cb({ payload: { provider: "codex" } })); });
    await waitFor(() => expect(result.current.usage.codex?.lanes[0].used_pct).toBe(55));
    expect(result.current.meta.codex).toMatchObject({ failed: false });
  });
});
