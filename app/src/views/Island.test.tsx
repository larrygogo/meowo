import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, screen } from "@testing-library/react";
import { Island, byUrgency, focusSession, islandCounts, islandTone } from "./Island";
import { IslandPanel } from "./island/IslandPanel";
import { orderIslandUsage, pickIslandUsage, primaryLane } from "./island/useIslandUsage";
import { UsageRing, usageColor, USAGE_FLIP_MS } from "./island/islandParts";
import { act } from "@testing-library/react";
import type { Item } from "./sticker/types";
import type { ProviderUsage } from "../api";
import { zh } from "../i18n/zh";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

let nextId = 1;
const mk = (over: Partial<Item> & { status?: string; lastEventAt?: number } = {}): Item => {
  const id = nextId++;
  const { status = "running", lastEventAt = 1_000, ...rest } = over;
  return {
    session: { id, project_id: 1, cc_session_id: `s-${id}`, status, started_at: 0, last_event_at: lastEventAt, ended_at: null },
    project_name: "proj", task_title: `会话 ${id}`, current_activity: null, column: "doing",
    todo_done: 0, todo_total: 0, todos: [], pid: null, archived: false,
    connected: true, pty_managed: true, endable: true, background: false,
    errored: false, error_label: null, error_raw: null, busy_subagents: 0,
    preview: null, profile_name: null, foreign: false, pending_review: null,
    provider: "claude",
    screen_state: status === "running" ? "working" : "idle",
    ...rest,
  } as unknown as Item;
};

const noop = () => {};
const renderIsland = (props: Partial<Parameters<typeof Island>[0]> = {}) =>
  render(
    <Island
      sessions={[]}
      compactStyle="counts"
      alert={null}
      usages={[]}
      onMeasure={noop}
      onHoverIntent={noop}
      onActivate={noop}
      onOpenSession={noop}
      onPointerDownDrag={noop}
      {...props}
    />,
  );

const lane = (kind: string, used_pct: number | null) => ({ kind, used_pct, used: null, limit: null, unit: null, resets_at: null });

describe("islandCounts / islandTone", () => {
  it("按状态计数，断开与归档的不算", () => {
    const counts = islandCounts([
      mk(),
      mk(),
      mk({ status: "waiting" }),
      mk({ pending_review: "approval" }),
      mk({ errored: true, status: "waiting" }),
      mk({ connected: false }),
      mk({ archived: true }),
    ]);
    expect(counts).toEqual({ pending: 1, error: 1, running: 2, waiting: 1 });
  });

  it("整体情绪：出错 > 待处理 > 在跑 > 等待输入 > 闲着", () => {
    expect(islandTone({ pending: 1, error: 1, running: 1, waiting: 1 })).toBe("error");
    expect(islandTone({ pending: 1, error: 0, running: 3, waiting: 0 })).toBe("pending");
    expect(islandTone({ pending: 0, error: 0, running: 1, waiting: 2 })).toBe("running");
    expect(islandTone({ pending: 0, error: 0, running: 0, waiting: 2 })).toBe("waiting");
    expect(islandTone({ pending: 0, error: 0, running: 0, waiting: 0 })).toBe("idle");
  });
});

describe("byUrgency / focusSession", () => {
  it("待处理优先，同档取最近有动静的", () => {
    const running = mk({ lastEventAt: 9_000 });
    const olderPending = mk({ pending_review: "question", lastEventAt: 1_000 });
    const newerPending = mk({ pending_review: "approval", lastEventAt: 2_000 });
    expect(focusSession([running, olderPending, newerPending])).toBe(newerPending);
    expect(byUrgency([running, olderPending, newerPending])).toEqual([newerPending, olderPending, running]);
  });

  it("没待处理时，未读的跑完压过在跑的", () => {
    const running = mk({ lastEventAt: 9_000 });
    const unreadDone = mk({ status: "waiting", unread: true, lastEventAt: 1_000 });
    expect(focusSession([running, unreadDone])).toBe(unreadDone);
  });

  it("都断开时没有焦点", () => {
    expect(focusSession([mk({ connected: false })])).toBeNull();
  });
});

describe("额度", () => {
  it("主泳道取 5 小时窗口，没有就退到周/7 天", () => {
    const usage = { lanes: [lane("seven_day", 30), lane("five_hour", 82)], note: null } as ProviderUsage;
    expect(primaryLane(usage)).toEqual({ pct: 82, kind: "five_hour" });
    expect(primaryLane({ lanes: [lane("weekly", 18)], note: null } as ProviderUsage)).toEqual({ pct: 18, kind: "weekly" });
    expect(primaryLane({ lanes: [lane("five_hour", null)], note: null } as ProviderUsage)).toBeNull();
  });

  it("按「显示配额」设置的顺序挑第一个有数据的 agent", () => {
    const usage = {
      claude: { lanes: [lane("five_hour", null)], note: null },
      codex: { lanes: [lane("weekly", 40)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    expect(pickIslandUsage(usage, ["claude", "codex"])).toEqual({ provider: "codex", pct: 40, kind: "weekly" });
    expect(pickIslandUsage({}, ["claude"])).toBeNull();
  });

  // 用户反馈：设置里 kimi 排第一，会话全是 claude 时环上也固定是 kimi。正在用的 agent 排前面。
  it("正在用的 agent 排在设置顺序前面", () => {
    const usage = {
      kimi: { lanes: [lane("five_hour", 7)], note: null },
      claude: { lanes: [lane("five_hour", 43)], note: null },
      codex: { lanes: [lane("weekly", 18)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    const order = (active: string[]) => orderIslandUsage(usage, ["kimi", "codex", "claude"], active).map((u) => u.provider);
    expect(order([])).toEqual(["kimi", "codex", "claude"]);
    expect(order(["claude"])).toEqual(["claude", "kimi", "codex"]);
    expect(pickIslandUsage(usage, ["kimi", "codex", "claude"], ["claude"])?.provider).toBe("claude");
  });
});

describe("额度环", () => {
  it("颜色随用量变：宽裕绿、过六成琥珀、过八成五红", () => {
    expect(usageColor(20)).toBe("var(--isl-ok)");
    expect(usageColor(60)).toBe("var(--isl-warn)");
    expect(usageColor(90)).toBe("var(--isl-err)");
  });

  it("中间每 4 秒在百分比和 agent logo 之间翻一次", () => {
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 43 }]} />);
    const flip = () => container.querySelector(".isl-usage-flip")!;
    expect(flip().classList.contains("is-logo")).toBe(false);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS); });
    expect(flip().classList.contains("is-logo")).toBe(true);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS); });
    expect(flip().classList.contains("is-logo")).toBe(false);
  });

  it("几家都有额度时轮流显示：A 的数字 → A 的 logo → B 的数字", () => {
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 43 }, { provider: "kimi", pct: 7 }]} />);
    const value = () => container.querySelector(".isl-usage-val")?.textContent;
    expect(value()).toBe("43");
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS * 2); });
    expect(value()).toBe("7");
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS * 2); });
    expect(value()).toBe("43");
  });
});

describe("Island 收起", () => {
  it("概览：额度环、时间、每个会话一颗带环的 agent 图标", () => {
    const { container } = renderIsland({ sessions: [mk(), mk({ pending_review: "approval" })], usages: [{ provider: "claude", pct: 82 }] });
    expect(container.querySelector(".isl-usage-val")?.textContent).toBe("82");
    expect(container.querySelector(".isl-clock")?.textContent).toMatch(/^\d{2}:\d{2}$/);
    expect(container.querySelectorAll(".isl-agent")).toHaveLength(2);
    // 要处理的会话排最前、角上带「!」。
    expect(container.querySelector(".isl-agent")?.getAttribute("data-tone")).toBe("pending");
    expect(container.querySelector(".isl-agent-badge")?.textContent).toBe("!");
  });

  it("概览：没有额度数据就不画额度环；会话多于 4 个折成「+N」", () => {
    const sessions = Array.from({ length: 6 }, () => mk());
    const { container } = renderIsland({ sessions });
    expect(container.querySelector(".isl-usage-val")).toBeNull();
    expect(container.querySelectorAll(".isl-agent")).toHaveLength(4);
    expect(container.querySelector(".isl-more")?.textContent).toBe("+2");
  });

  it("没有会话时眼睛是闲着的样子", () => {
    const { container } = renderIsland();
    expect(container.querySelector(".isl-eyes")?.getAttribute("data-tone")).toBe("idle");
    expect(container.querySelector(".isl-agents")).toBeNull();
  });

  it("最要紧的会话：显示标题与状态", () => {
    const pending = mk({ pending_review: "question", task_title: "改登录页" });
    renderIsland({ sessions: [mk(), pending], compactStyle: "focus" });
    expect(screen.getByText("改登录页")).toBeTruthy();
    expect(screen.getByText(zh.island.toneLabel.pending)).toBeTruthy();
  });

  it("极简：只有色点，最多 6 个", () => {
    const sessions = Array.from({ length: 9 }, () => mk());
    const { container } = renderIsland({ sessions, compactStyle: "minimal" });
    expect(container.querySelectorAll(".isl-dot")).toHaveLength(6);
  });

  // U1-9：屏幕顶边常有鼠标路过，停够 250ms 才展开，路过不展开。
  it("鼠标停够 250ms 才算要展开，提前移开不算", () => {
    vi.useFakeTimers();
    const onHoverIntent = vi.fn();
    const { container } = renderIsland({ onHoverIntent });
    const notch = container.querySelector(".isl-notch")!;
    fireEvent.pointerEnter(notch);
    vi.advanceTimersByTime(200);
    fireEvent.pointerLeave(notch);
    vi.advanceTimersByTime(500);
    expect(onHoverIntent).not.toHaveBeenCalled();
    fireEvent.pointerEnter(notch);
    vi.advanceTimersByTime(260);
    expect(onHoverIntent).toHaveBeenCalledTimes(1);
  });
});

describe("Island 提醒", () => {
  it("显示标题、状态与项目，点「打开」只打开会话、不当成点胶囊", () => {
    const onOpenSession = vi.fn();
    const onActivate = vi.fn();
    const onPointerDownDrag = vi.fn();
    renderIsland({
      alert: { sessionId: 9, kind: "error", title: "跑测试", project: "api", detail: "API 请求出错" },
      onOpenSession,
      onActivate,
      onPointerDownDrag,
    });
    expect(screen.getByText("跑测试")).toBeTruthy();
    expect(screen.getByText("API 请求出错")).toBeTruthy();
    expect(screen.getByText("api")).toBeTruthy();
    const open = screen.getByRole("button", { name: zh.island.open });
    fireEvent.pointerDown(open);
    fireEvent.click(open);
    expect(onOpenSession).toHaveBeenCalledWith(9);
    expect(onPointerDownDrag).not.toHaveBeenCalled();
    expect(onActivate).not.toHaveBeenCalled();
  });

  // 用户反馈：鼠标移上提醒，提醒就被换成展开面板，还没看完就没了。提醒在场时悬停只留住它。
  it("鼠标停在提醒上只报告「在看」，不触发展开", () => {
    vi.useFakeTimers();
    const onHoverIntent = vi.fn();
    const onAlertHover = vi.fn();
    const { container } = renderIsland({
      alert: { sessionId: 3, kind: "approval", title: "批一下", project: "p", detail: null },
      onHoverIntent,
      onAlertHover,
    });
    const notch = container.querySelector(".isl-notch")!;
    fireEvent.pointerEnter(notch);
    vi.advanceTimersByTime(1_000);
    expect(onAlertHover).toHaveBeenLastCalledWith(true);
    expect(onHoverIntent).not.toHaveBeenCalled();
    fireEvent.pointerLeave(notch);
    expect(onAlertHover).toHaveBeenLastCalledWith(false);
  });

  it("未命名会话的提醒显示「等待首次输入」", () => {
    renderIsland({ alert: { sessionId: 1, kind: "waiting", title: "(未命名会话)", project: "p", detail: null } });
    expect(screen.getByText(zh.sticker.waitingFirstInput)).toBeTruthy();
  });
});

describe("IslandPanel 展开面板", () => {
  const renderPanel = (props: Partial<Parameters<typeof IslandPanel>[0]> = {}) =>
    render(
      <IslandPanel
        sessions={[]}
        usage={{}}
        quotaProviders={["claude"]}
        onOpenSession={noop}
        onShowAll={noop}
        onMeasure={noop}
        onPointerDownDrag={noop}
        {...props}
      />,
    );

  it("最多列 3 个最要紧的会话，点一行打开它", () => {
    const onOpenSession = vi.fn();
    const pending = mk({ pending_review: "approval", task_title: "要批的" });
    const sessions = [mk(), mk(), mk(), pending];
    const { container } = renderPanel({ sessions, onOpenSession });
    const rows = container.querySelectorAll(".isl-row");
    expect(rows).toHaveLength(3);
    expect(rows[0].textContent).toContain("要批的");
    fireEvent.click(rows[0]);
    expect(onOpenSession).toHaveBeenCalledWith(pending.session.id);
  });

  it("「全部」打开完整列表；没有额度数据时只有一页、不出翻页点", () => {
    const onShowAll = vi.fn();
    const { container } = renderPanel({ onShowAll });
    expect(screen.getByText(zh.island.idle)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: zh.island.showAll }));
    expect(onShowAll).toHaveBeenCalled();
    expect(container.querySelector(".isl-dots-nav")).toBeNull();
  });

  it("有额度数据：会话页带额度大环，翻到第二页看各家额度", () => {
    const usage = {
      claude: { lanes: [lane("five_hour", 43)], note: null },
      codex: { lanes: [lane("weekly", 18)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    const { container } = renderPanel({ usage });
    expect(container.querySelector(".isl-big-val")?.textContent).toContain("43");
    fireEvent.click(screen.getByRole("tab", { name: zh.island.page(2) }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(2);
    expect(screen.getByText("Codex")).toBeTruthy();
  });

  it("左右箭头翻页，到头绕回", () => {
    const usage = { claude: { lanes: [lane("five_hour", 43)], note: null } } as unknown as Record<string, ProviderUsage>;
    const { container } = renderPanel({ usage });
    fireEvent.click(screen.getByRole("button", { name: zh.island.nextPage }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: zh.island.nextPage }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: zh.island.prevPage }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(1);
  });

  it("只有一页时不出箭头", () => {
    renderPanel();
    expect(screen.queryByRole("button", { name: zh.island.nextPage })).toBeNull();
  });
});
