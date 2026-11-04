import { describe, it, expect, vi, afterEach } from "vitest";
import { render, cleanup, fireEvent, screen } from "@testing-library/react";
import { Island, focusSession, islandCounts, islandTone } from "./Island";
import type { Item } from "./sticker/types";
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
      onMeasure={noop}
      onHoverIntent={noop}
      onActivate={noop}
      onOpenSession={noop}
      onPointerDownDrag={noop}
      {...props}
    />,
  );

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

describe("focusSession", () => {
  it("最要紧的那个：待处理优先，同档取最近有动静的", () => {
    const running = mk({ lastEventAt: 9_000 });
    const olderPending = mk({ pending_review: "question", lastEventAt: 1_000 });
    const newerPending = mk({ pending_review: "approval", lastEventAt: 2_000 });
    expect(focusSession([running, olderPending, newerPending])).toBe(newerPending);
  });

  it("没待处理时，未读的跑完压过在跑的", () => {
    const running = mk({ lastEventAt: 9_000 });
    const unreadDone = mk({ status: "waiting", unread: true, lastEventAt: 1_000 });
    expect(focusSession([running, unreadDone])).toBe(unreadDone);
  });

  it("都闲着或都断开时没有焦点", () => {
    expect(focusSession([mk({ connected: false })])).toBeNull();
  });
});

describe("Island", () => {
  it("计数样式：只列非零的状态", () => {
    renderIsland({ sessions: [mk(), mk(), mk({ pending_review: "approval" })] });
    expect(screen.getByText(`2 ${zh.island.toneLabel.running}`)).toBeTruthy();
    expect(screen.getByText(`1 ${zh.island.toneLabel.pending}`)).toBeTruthy();
    expect(screen.queryByText(new RegExp(zh.island.toneLabel.error))).toBeNull();
  });

  it("没有会话时显示空态文案，眼睛是闲着的样子", () => {
    const { container } = renderIsland();
    expect(screen.getByText(zh.island.idle)).toBeTruthy();
    expect(container.querySelector(".island-eyes")?.getAttribute("data-tone")).toBe("idle");
  });

  it("焦点样式：显示最要紧那个会话的标题与状态", () => {
    const pending = mk({ pending_review: "question", task_title: "改登录页" });
    renderIsland({ sessions: [mk(), pending], compactStyle: "focus" });
    expect(screen.getByText("改登录页")).toBeTruthy();
    expect(screen.getByText(zh.island.toneLabel.pending)).toBeTruthy();
  });

  it("极简样式：只有色点，最多 6 个", () => {
    const sessions = Array.from({ length: 9 }, () => mk());
    const { container } = renderIsland({ sessions, compactStyle: "minimal" });
    expect(container.querySelectorAll(".island-dot")).toHaveLength(6);
  });

  it("提醒：显示标题、状态与项目，点「打开」只打开会话、不当成点胶囊", () => {
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

  it("未命名会话的提醒显示「等待首次输入」", () => {
    renderIsland({ alert: { sessionId: 1, kind: "waiting", title: "(未命名会话)", project: "p", detail: null } });
    expect(screen.getByText(zh.sticker.waitingFirstInput)).toBeTruthy();
  });

  // U1-9：屏幕顶边常有鼠标路过，停够 250ms 才展开，路过不展开。
  it("鼠标停够 250ms 才算要展开，提前移开不算", () => {
    vi.useFakeTimers();
    const onHoverIntent = vi.fn();
    const { container } = renderIsland({ onHoverIntent });
    const pill = container.querySelector(".island-pill")!;
    fireEvent.pointerEnter(pill);
    vi.advanceTimersByTime(200);
    fireEvent.pointerLeave(pill);
    vi.advanceTimersByTime(500);
    expect(onHoverIntent).not.toHaveBeenCalled();
    fireEvent.pointerEnter(pill);
    vi.advanceTimersByTime(260);
    expect(onHoverIntent).toHaveBeenCalledTimes(1);
  });
});
