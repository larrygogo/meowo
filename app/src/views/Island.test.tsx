import { describe, it, expect, vi, afterEach } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn((_cmd: string) => Promise.resolve(null as unknown)) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { render, cleanup, fireEvent, screen } from "@testing-library/react";
import { Island, byUrgency, focusSession, islandCounts, islandTone, menubarSegments } from "./Island";
import { IslandPanel } from "./island/IslandPanel";
import { liveIslandUsage, orderIslandUsage, pickIslandUsage, primaryLane } from "./island/useIslandUsage";
import { Eyes, EYE_CUE_MS, EYES_BLUSH_AFTER_MS, EYES_DROWSY_AFTER_MS, EYES_PLEAD_AFTER_MS, EYES_YAWN_MS, EYES_SLEEP_AFTER_MS, UsageRing, usedColor, usedOf, USAGE_FLIP_MS } from "./island/islandParts";
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

  // 用户要求：额度环只轮换岛上正有会话的 agent。
  it("收起胶囊的额度环只取岛上有会话的 agent，没有就不画", () => {
    const usage = {
      kimi: { lanes: [lane("five_hour", 7)], note: null },
      claude: { lanes: [lane("seven_day", 4)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    expect(liveIslandUsage(usage, ["kimi", "claude"], ["claude"]).map((u) => u.provider)).toEqual(["claude"]);
    expect(liveIslandUsage(usage, ["kimi", "claude"], ["codex"])).toEqual([]);
    expect(liveIslandUsage(usage, ["kimi", "claude"], [])).toEqual([]);
  });
});

describe("额度环", () => {
  // 用户 2026-11-05 要回「已用」（此前一度改成剩余）。
  it("显示已用，颜色按已用：六成以下绿、六成起琥珀、八成五起红", () => {
    expect(usedOf(4)).toBe(4);
    expect(usedOf(130)).toBe(100);
    expect(usedColor(4)).toBe("var(--isl-ok)");
    expect(usedColor(60)).toBe("var(--isl-warn)");
    expect(usedColor(90)).toBe("var(--isl-err)");
  });

  it("中间是已用百分比，右下角是哪家的 logo；只有一家时不轮换", () => {
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 4 }]} />);
    expect(container.querySelector(".isl-usage-val")?.textContent).toBe("4");
    expect(container.querySelector(".isl-usage-badge")).toBeTruthy();
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS * 3); });
    expect(container.querySelector(".isl-usage-wrap")?.getAttribute("data-provider")).toBe("claude");
  });

  it("几家都有额度时轮流显示", () => {
    vi.useFakeTimers();
    const { container } = render(<UsageRing entries={[{ provider: "claude", pct: 43 }, { provider: "kimi", pct: 7 }]} />);
    const shown = () => [
      container.querySelector(".isl-usage-wrap")?.getAttribute("data-provider"),
      container.querySelector(".isl-usage-val")?.textContent,
    ];
    expect(shown()).toEqual(["claude", "43"]);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS); });
    expect(shown()).toEqual(["kimi", "7"]);
    act(() => { vi.advanceTimersByTime(USAGE_FLIP_MS); });
    expect(shown()).toEqual(["claude", "43"]);
  });
});

describe("眼睛", () => {
  const mode = (container: HTMLElement) => container.querySelector(".isl-eyes")?.getAttribute("data-mode");

  it("每种状态一个表情：在跑睁眼、要处理睁大、出错叉叉、跑完 ^ ^", () => {
    expect(mode(render(<Eyes tone="running" />).container)).toBe("open");
    cleanup();
    expect(mode(render(<Eyes tone="pending" />).container)).toBe("wide");
    cleanup();
    expect(mode(render(<Eyes tone="error" />).container)).toBe("dead");
    cleanup();
    expect(mode(render(<Eyes tone="waiting" />).container)).toBe("happy");
  });

  it("闲着 6 分钟犯困、10 分钟睡着冒 z；一有会话就醒，醒时吓一跳", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 10, 6, 14, 0));
    const { container, rerender } = render(<Eyes tone="idle" />);
    expect(mode(container)).toBe("open");
    act(() => { vi.advanceTimersByTime(EYES_DROWSY_AFTER_MS + 1_000); });
    expect(mode(container)).toBe("drowsy");
    act(() => { vi.advanceTimersByTime(EYES_SLEEP_AFTER_MS - EYES_DROWSY_AFTER_MS); });
    expect(mode(container)).toBe("yawn");
    act(() => { vi.advanceTimersByTime(EYES_YAWN_MS); });
    expect(mode(container)).toBe("sleep");
    expect(container.querySelector(".isl-zz")).toBeTruthy();
    rerender(<Eyes tone="running" />);
    expect(mode(container)).toBe("surprised");
    act(() => { vi.advanceTimersByTime(1_000); });
    expect(mode(container)).toBe("open");
  });

  it("睡着前打个哈欠", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 10, 6, 14, 0));
    const { container } = render(<Eyes tone="idle" />);
    act(() => { vi.advanceTimersByTime(EYES_SLEEP_AFTER_MS + 100); });
    expect(mode(container)).toBe("yawn");
    expect(container.querySelector(".isl-tear")).toBeTruthy();
    act(() => { vi.advanceTimersByTime(EYES_YAWN_MS); });
    expect(mode(container)).toBe("sleep");
  });

  it("打哈欠时开了新会话：醒来吓一跳后回到睁眼，不会一直打哈欠", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 10, 6, 14, 0));
    const { container, rerender } = render(<Eyes tone="idle" />);
    act(() => { vi.advanceTimersByTime(EYES_SLEEP_AFTER_MS + 100); });
    expect(mode(container)).toBe("yawn");
    rerender(<Eyes tone="running" />);
    expect(mode(container)).toBe("surprised");
    act(() => { vi.advanceTimersByTime(EYES_YAWN_MS + 1_000); });
    expect(mode(container)).toBe("open");
  });

  it("多个出错抓狂；待处理放一分钟没人管就可怜巴巴", () => {
    vi.useFakeTimers();
    const { container, rerender } = render(<Eyes tone="error" errors={1} />);
    expect(mode(container)).toBe("dead");
    rerender(<Eyes tone="error" errors={2} />);
    expect(mode(container)).toBe("frustrated");
    rerender(<Eyes tone="pending" />);
    expect(mode(container)).toBe("wide");
    act(() => { vi.advanceTimersByTime(EYES_PLEAD_AFTER_MS + 100); });
    expect(mode(container)).toBe("plead");
    rerender(<Eyes tone="running" />);
    expect(mode(container)).toBe("open");
  });

  it("后半夜一闲下来就犯困", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 10, 6, 2, 30));
    expect(mode(render(<Eyes tone="idle" />).container)).toBe("drowsy");
  });

  it("额度到八成五发愁挂汗，用光转蚊香圈；要处理和出错仍然优先", () => {
    const { container, rerender } = render(<Eyes tone="running" quotaPct={90} />);
    expect(mode(container)).toBe("worried");
    expect(container.querySelector(".isl-sweat")).toBeTruthy();
    rerender(<Eyes tone="running" quotaPct={100} />);
    expect(mode(container)).toBe("dizzy");
    rerender(<Eyes tone="pending" quotaPct={100} />);
    expect(mode(container)).toBe("wide");
    rerender(<Eyes tone="running" quotaPct={40} busy />);
    expect(mode(container)).toBe("focus");
  });

  it("一闪而过的表情：星星眼、吓一跳压过常驻神态，但压不过要处理", () => {
    const { container, rerender } = render(<Eyes tone="waiting" cue="star" />);
    expect(mode(container)).toBe("star");
    rerender(<Eyes tone="idle" cue="surprised" />);
    expect(mode(container)).toBe("surprised");
    rerender(<Eyes tone="pending" cue="star" />);
    expect(mode(container)).toBe("wide");
  });

  it("瞳孔朝鼠标的方向转", async () => {
    invoke.mockImplementation((cmd: string) => Promise.resolve(cmd === "island_cursor" ? [300, 120] : null));
    const { container } = render(<Eyes tone="running" />);
    // 鼠标在右下方远处：瞳孔往右下挪到头。
    await vi.waitFor(() => {
      const transform = (container.querySelector(".isl-pupil") as SVGElement | null)?.style.transform ?? "";
      expect(transform).toBe("translate(2.2px, 2.6px)");
    });
    invoke.mockImplementation(() => Promise.resolve(null));
  });
});

describe("岛上一闪而过的表情", () => {
  const eyeMode = (container: HTMLElement) => container.querySelector(".isl-eyes")?.getAttribute("data-mode");
  const islandWith = (sessions: Item[]) => (
    <Island sessions={sessions} compactStyle="counts" alert={null} usages={[]} onMeasure={noop} onActivate={noop} onOpenSession={noop} onPointerDownDrag={noop} />
  );

  it("有会话跑完冒星星眼，过一会儿回到 ^ ^", () => {
    vi.useFakeTimers();
    const a = mk();
    const { container, rerender } = render(islandWith([a]));
    expect(eyeMode(container)).toBe("open");
    const done = { ...a, session: { ...a.session, status: "waiting" }, screen_state: "idle" } as unknown as Item;
    rerender(islandWith([done]));
    expect(eyeMode(container)).toBe("star");
    act(() => { vi.advanceTimersByTime(EYE_CUE_MS.star + 100); });
    expect(eyeMode(container)).toBe("happy");
  });

  it("新开了会话吓一跳；第一次拿到列表不算", () => {
    vi.useFakeTimers();
    const a = mk();
    const { container, rerender } = render(islandWith([a]));
    expect(eyeMode(container)).toBe("open");
    rerender(islandWith([a, mk()]));
    expect(eyeMode(container)).toBe("surprised");
  });

  it("启动时列表从空到有不算新开；拉取偶尔回空也不重置基线", () => {
    vi.useFakeTimers();
    const a = mk();
    const b = mk();
    // App 的 stripSessions 初值是 []，第一次真实拉取才拿到会话。
    const { container, rerender } = render(islandWith([]));
    rerender(islandWith([a, b]));
    expect(eyeMode(container)).toBe("open");
    rerender(islandWith([]));
    rerender(islandWith([a, b]));
    expect(eyeMode(container)).toBe("open");
    rerender(islandWith([a, b, mk()]));
    expect(eyeMode(container)).toBe("surprised");
  });

  it("鼠标在岛上停一会儿就害羞，移开前不点就一直等", () => {
    vi.useFakeTimers();
    const { container } = render(islandWith([mk()]));
    const notch = container.querySelector(".isl-notch")!;
    fireEvent.pointerEnter(notch);
    act(() => { vi.advanceTimersByTime(EYES_BLUSH_AFTER_MS + 100); });
    expect(eyeMode(container)).toBe("blush");
    expect(container.querySelector(".isl-cheeks")).toBeTruthy();
    // 移开得早就不脸红。
    act(() => { vi.advanceTimersByTime(EYE_CUE_MS.blush + 100); });
    fireEvent.pointerLeave(notch);
    fireEvent.pointerEnter(notch);
    fireEvent.pointerLeave(notch);
    act(() => { vi.advanceTimersByTime(EYES_BLUSH_AFTER_MS + 100); });
    expect(eyeMode(container)).toBe("open");
  });

  it("鼠标移上岛眨一只眼", () => {
    vi.useFakeTimers();
    const { container } = render(islandWith([mk()]));
    fireEvent.pointerEnter(container.querySelector(".isl-notch")!);
    expect(container.querySelectorAll(".isl-eye-open ellipse")).toHaveLength(1);
    act(() => { vi.advanceTimersByTime(EYE_CUE_MS.wink + 100); });
    expect(container.querySelectorAll(".isl-eye-open ellipse")).toHaveLength(2);
  });
});

describe("Island 收起", () => {
  it("概览：额度环、时间、每个会话一颗带环的 agent 图标", () => {
    const { container } = renderIsland({ sessions: [mk(), mk({ pending_review: "approval" })], usages: [{ provider: "claude", pct: 82 }] });
    // 额度环显示已用 82%。
    expect(container.querySelector(".isl-usage-val")?.textContent).toBe("82");
    expect(container.querySelector(".isl-clock")?.textContent).toMatch(/^\d{2}:\d{2}$/);
    expect(container.querySelectorAll(".isl-agent")).toHaveLength(2);
    // 要处理的会话排最前、角上带「!」。
    expect(container.querySelector(".isl-agent")?.getAttribute("data-tone")).toBe("pending");
    expect(container.querySelector(".isl-agent-badge")?.textContent).toBe("!");
  });

  // 用户嫌整圈亮色环又重又看不出状态：跑完的不画亮弧，亮一个琥珀等待点（与对话窗侧栏同规则）；在跑才有亮弧。
  it("会话图标：跑完不画亮弧、亮等待点；在跑才有亮弧", () => {
    const done = mk({ status: "waiting", unread: true });
    const seen = mk({ status: "waiting", unread: false });
    const running = mk({ todo_done: 1, todo_total: 4 });
    const { container } = renderIsland({ sessions: [done, seen, running] });
    const agents = [...container.querySelectorAll(".isl-agent")];
    const arcStroke = (el: Element) => el.querySelectorAll("circle")[1]?.getAttribute("stroke");
    const byTone = (tone: string) => agents.filter((a) => a.getAttribute("data-tone") === tone);
    expect(byTone("waiting").map(arcStroke)).toEqual(["transparent", "transparent"]);
    expect(container.querySelectorAll(".isl-agent-badge.is-waiting")).toHaveLength(2);
    expect(arcStroke(byTone("running")[0])).not.toBe("transparent");
  });

  it("会话图标带「标题 · 状态」的悬停提示与会话 id", () => {
    const item = mk({ task_title: "改登录页", pending_review: "approval" });
    const { container } = renderIsland({ sessions: [item] });
    const icon = container.querySelector(".isl-agent")!;
    expect(icon.getAttribute("title")).toBe(`改登录页 · ${zh.island.toneLabel.pending}`);
    expect(icon.getAttribute("data-session-id")).toBe(String(item.session.id));
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

  // 用户要求（2026-11-04）：只点击展开，鼠标停上去不展开。
  it("鼠标停在胶囊上不展开", () => {
    vi.useFakeTimers();
    const onActivate = vi.fn();
    const onPointerDownDrag = vi.fn();
    const { container } = renderIsland({ onActivate, onPointerDownDrag });
    const notch = container.querySelector(".isl-notch")!;
    fireEvent.pointerEnter(notch);
    vi.advanceTimersByTime(2_000);
    expect(onActivate).not.toHaveBeenCalled();
    expect(onPointerDownDrag).not.toHaveBeenCalled();
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
    const onActivate = vi.fn();
    const onAlertHover = vi.fn();
    const { container } = renderIsland({
      alert: { sessionId: 3, kind: "approval", title: "批一下", project: "p", detail: null },
      onActivate,
      onAlertHover,
    });
    const notch = container.querySelector(".isl-notch")!;
    fireEvent.pointerEnter(notch);
    vi.advanceTimersByTime(1_000);
    expect(onAlertHover).toHaveBeenLastCalledWith(true);
    expect(onActivate).not.toHaveBeenCalled();
    fireEvent.pointerLeave(notch);
    expect(onAlertHover).toHaveBeenLastCalledWith(false);
  });

  it("跑完的提醒带 agent 最后一句话；没有时显示项目名", () => {
    renderIsland({ alert: { sessionId: 4, kind: "waiting", title: "改登录页", project: "web", detail: "已经改好了，测试全过" } });
    expect(screen.getByText("已经改好了，测试全过")).toBeTruthy();
    expect(screen.queryByText("web")).toBeNull();
    cleanup();
    renderIsland({ alert: { sessionId: 4, kind: "waiting", title: "改登录页", project: "web", detail: null } });
    expect(screen.getByText("web")).toBeTruthy();
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
        onMeasure={noop}
        onPointerDownDrag={noop}
        {...props}
      />,
    );

  it("会话多于 3 个时翻页看：每页 3 个，最后是用量页", () => {
    const sessions = Array.from({ length: 5 }, (_, i) => mk({ task_title: `第${i + 1}个`, lastEventAt: 10_000 - i }));
    const usage = { claude: { lanes: [lane("five_hour", 43)], note: null } } as unknown as Record<string, ProviderUsage>;
    const { container } = renderPanel({ sessions, usage });
    const titles = () => [...container.querySelectorAll(".isl-row-title")].map((el) => el.textContent);
    expect(titles()).toEqual(["第1个", "第2个", "第3个"]);
    expect(container.querySelectorAll(".isl-dots-nav button")).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: zh.island.nextPage }));
    expect(titles()).toEqual(["第4个", "第5个"]);
    fireEvent.click(screen.getByRole("button", { name: zh.island.nextPage }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(1);
    expect(screen.getByText(zh.island.panelUsage)).toBeTruthy();
  });

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

  it("新建会话：标题旁的按钮与空态按钮都能开", () => {
    const onNewSession = vi.fn();
    renderPanel({ onNewSession });
    const buttons = screen.getAllByRole("button", { name: new RegExp(zh.sticker.newSession) });
    // 空态时两个：标题旁一个、空态里一个。
    expect(buttons).toHaveLength(2);
    buttons.forEach((button) => fireEvent.click(button));
    expect(onNewSession).toHaveBeenCalledTimes(2);
  });

  it("面板上不放「全部」按钮（用户要求）；没有额度数据时只有一页、不出翻页点", () => {
    const { container } = renderPanel();
    expect(screen.getByText(zh.island.idle)).toBeTruthy();
    expect(screen.queryByRole("button", { name: zh.island.showAll })).toBeNull();
    expect(container.querySelector(".isl-dots-nav")).toBeNull();
  });

  it("会话页不放额度（用户要求），翻到第二页看各家额度，按设置顺序排", () => {
    const usage = {
      claude: { lanes: [lane("five_hour", 43)], note: null },
      codex: { lanes: [lane("weekly", 18)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    const { container } = renderPanel({ usage });
    expect(container.querySelector(".isl-big-val")).toBeNull();
    expect(container.querySelector(".isl-side")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: zh.island.page(2) }));
    expect(container.querySelectorAll(".isl-usage-cell")).toHaveLength(2);
    expect(screen.getByText("Codex")).toBeTruthy();
  });

  // 用户问「为什么只有一个用量」：每家的每档额度都要列出来，不只主泳道。
  it("用量页每家每档额度一个环，叫法与贴纸底栏一致", () => {
    const usage = {
      codex: { lanes: [lane("weekly", 79), lane("five_hour", 30), lane("other", null)], note: null },
    } as unknown as Record<string, ProviderUsage>;
    const { container } = renderPanel({ usage });
    fireEvent.click(screen.getByRole("button", { name: zh.island.nextPage }));
    const cells = container.querySelectorAll(".isl-usage-group .isl-usage-cell");
    // 没有百分比的那档不画；短窗口排在前面。
    expect(cells).toHaveLength(2);
    expect(cells[0].textContent).toContain(zh.account.laneFiveHour);
    expect(cells[1].textContent).toContain(zh.account.laneWeekly);
    // 显示已用 79%。
    expect(cells[1].textContent).toContain("79");
    // 一家只画一个叠环，每档一圈（用户要求，仿 Apple 健康三环）。
    expect(container.querySelectorAll(".isl-usage-stack")).toHaveLength(1);
    expect(container.querySelectorAll(".isl-usage-stack > .isl-ring")).toHaveLength(2);
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

  it("面板标题栏有打开对话窗口和设置；轻量模式不传对话回调就不出那颗", () => {
    const onOpenChat = vi.fn();
    const onOpenSettings = vi.fn();
    renderPanel({ onOpenChat, onOpenSettings });
    // 没有会话时空态也有一颗同名主按钮，这里点标题栏那颗。
    fireEvent.click(screen.getAllByRole("button", { name: zh.sticker.openChatWindow })[0]);
    fireEvent.click(screen.getByRole("button", { name: zh.sticker.openSettings }));
    expect(onOpenChat).toHaveBeenCalledOnce();
    expect(onOpenSettings).toHaveBeenCalledOnce();
    cleanup();
    renderPanel({ onOpenSettings });
    expect(screen.queryByRole("button", { name: zh.sticker.openChatWindow })).toBeNull();
  });

  it("没有会话时空态主按钮是打开对话窗口；轻量模式退回新建会话", () => {
    const onOpenChat = vi.fn();
    const onNewSession = vi.fn();
    const { container } = renderPanel({ onOpenChat, onNewSession });
    fireEvent.click(container.querySelector(".isl-empty .isl-btn")!);
    expect(onOpenChat).toHaveBeenCalledOnce();
    expect(onNewSession).not.toHaveBeenCalled();
    cleanup();
    const lite = renderPanel({ onNewSession });
    expect(lite.container.querySelector(".isl-empty .isl-btn")?.textContent).toContain(zh.sticker.newSession);
  });

  it("只有一页时不出箭头", () => {
    renderPanel();
    expect(screen.queryByRole("button", { name: zh.island.nextPage })).toBeNull();
  });
});

describe("macOS 菜单栏富文本", () => {
  const labels = { toneLabel: zh.island.toneLabel, untitled: "未命名" };
  it("显示决定眼睛神态的那个会话：标题 · 状态词，其余折成 +N，只用主文字色", () => {
    const sessions = [mk({ task_title: "跑着的" }), mk({ task_title: "跑完的", status: "waiting", unread: true })];
    // 眼睛按 islandTone 是「在跑」（绿）：文字就得是在跑的那个，不是排在前面的未读跑完。
    expect(menubarSegments(sessions, "running", 12, labels)).toEqual([
      { kind: "text", text: `跑着的 · ${zh.island.toneLabel.running}  +1`, color: "label" },
    ]);
  });
  it("标题太长省略；没会话显示额度；都没有就空", () => {
    const long = menubarSegments([mk({ task_title: "一二三四五六七八九十一二三四五六七八" })], "running", null, labels);
    expect(long[0].kind === "text" && long[0].text.startsWith("一二三四五六七八九十一二三四五六… · ")).toBe(true);
    expect(menubarSegments([], "idle", 90.4, labels)).toEqual([
      { kind: "symbol", name: "gauge.medium", color: "label" },
      { kind: "text", text: " 90%", color: "label" },
    ]);
    expect(menubarSegments([], "idle", null, labels)).toEqual([]);
  });
});
