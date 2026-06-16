import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";
import { Sticker, EmptyState } from "./Sticker";
import type { LiveSession } from "../api";
import { zh } from "../i18n/zh";

type Item = LiveSession & { connected: boolean };

function mk(over: Partial<Item> = {}): Item {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
    project_name: "proj",
    task_title: "做点事",
    current_activity: "正在做点事",
    column: "doing", todo_done: 0, todo_total: 0, todos: [],
    pid: 1234, connected: true, archived: false, cwd: null, errored: false, error_label: null, error_raw: null,
    ...over,
  } as Item;
}

afterEach(() => {
  cleanup();
  localStorage.clear(); // 防 tab/star 等持久化状态跨用例泄漏
});

describe("Sticker", () => {
  it("空数据显示 all 空态主文案", () => {
    const { container } = render(<Sticker data={[]} />);
    expect(screen.getByText(zh.empty.allTitle)).toBeTruthy();
    expect(container.querySelector("[data-tauri-drag-region]")).toBeTruthy();
  });

  it("渲染会话行：项目名 + 当前动作", () => {
    render(<Sticker data={[mk()]} />);
    expect(screen.getByText("proj")).toBeTruthy();
    expect(screen.getByText("正在做点事")).toBeTruthy();
  });

  it("hover 停留后浮现轻推预览块（最近一条 AI 正文）", () => {
    vi.useFakeTimers();
    try {
      const { container } = render(<Sticker data={[mk({ preview: "需要你确认下一步" })]} />);
      expect(container.querySelector(".stk-preview")).toBeNull(); // 未停留不显示
      fireEvent.mouseEnter(container.querySelector(".stk-card")!);
      act(() => { vi.advanceTimersByTime(300); });
      expect(screen.getByText("需要你确认下一步")).toBeTruthy();
      expect(screen.getByText(zh.sticker.previewMark)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("快速划过（停留不足 PREVIEW_DELAY）不浮现预览", () => {
    vi.useFakeTimers();
    try {
      const { container } = render(<Sticker data={[mk({ preview: "x" })]} />);
      const card = container.querySelector(".stk-card")!;
      fireEvent.mouseEnter(card);
      act(() => { vi.advanceTimersByTime(100); });
      fireEvent.mouseLeave(card);
      act(() => { vi.advanceTimersByTime(300); });
      expect(container.querySelector(".stk-preview")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("无 preview 时即便停留也不渲染预览块", () => {
    vi.useFakeTimers();
    try {
      const { container } = render(<Sticker data={[mk({ preview: null })]} />);
      fireEvent.mouseEnter(container.querySelector(".stk-card")!);
      act(() => { vi.advanceTimersByTime(300); });
      expect(container.querySelector(".stk-preview")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("点击星标切换状态并持久化到 localStorage", () => {
    localStorage.removeItem("cc-kanban-starred");
    const { container } = render(<Sticker data={[mk({ session: { id: 7, project_id: 1, cc_session_id: "star-me", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } })]} />);
    fireEvent.click(screen.getByTitle(zh.sticker.star));
    expect(container.querySelector(".stk-card.is-star")).toBeTruthy();
    expect(JSON.parse(localStorage.getItem("cc-kanban-starred") ?? "[]")).toContain("star-me");
    localStorage.removeItem("cc-kanban-starred");
  });

  it("待交互标签页按等待最久优先排序", () => {
    localStorage.setItem("cc-kanban-tab", "waiting");
    const base = (id: number, cc: string, last: number) =>
      mk({ task_title: cc, current_activity: null, connected: true,
        session: { id, project_id: 1, cc_session_id: cc, status: "waiting", started_at: 0, last_event_at: last, ended_at: null } });
    const now = Date.now();
    const { container } = render(<Sticker data={[
      base(1, "新", now - 60_000),   // 1 分钟前
      base(2, "旧", now - 600_000),  // 10 分钟前(等待最久)
    ]} />);
    const cards = container.querySelectorAll(".stk-card");
    expect(cards[0].querySelector(".stk-title")?.textContent).toBe("旧");
  });

  it("已星标会话排到列表最前", () => {
    localStorage.setItem("cc-kanban-starred", JSON.stringify(["b"]));
    const { container } = render(<Sticker data={[
      mk({ task_title: "甲", current_activity: null, session: { id: 1, project_id: 1, cc_session_id: "a", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } }),
      mk({ task_title: "乙", current_activity: null, session: { id: 2, project_id: 1, cc_session_id: "b", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } }),
    ]} />);
    const cards = container.querySelectorAll(".stk-card");
    expect(cards[0].querySelector(".stk-title")?.textContent).toBe("乙");
    expect(cards[0].classList.contains("is-star")).toBe(true);
    localStorage.removeItem("cc-kanban-starred");
  });

  it("有便签时渲染便签块", () => {
    const { container } = render(<Sticker data={[mk({ note: "记得 review PR" })]} />);
    expect(screen.getByText("记得 review PR")).toBeTruthy();
    expect(container.querySelector(".stk-note")).toBeTruthy();
  });

  it("无便签时点击便签按钮打开编辑框", () => {
    const { container } = render(<Sticker data={[mk({ note: null })]} />);
    expect(container.querySelector(".stk-note-edit")).toBeNull();
    fireEvent.click(screen.getByTitle(zh.sticker.noteAdd));
    const input = container.querySelector(".stk-note-edit") as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.placeholder).toBe(zh.sticker.notePlaceholder);
  });

  it("编辑已有便签时预填原文", () => {
    const { container } = render(<Sticker data={[mk({ note: "旧便签" })]} />);
    fireEvent.click(container.querySelector(".stk-noteb")!);
    const input = container.querySelector(".stk-note-edit") as HTMLInputElement;
    expect(input.value).toBe("旧便签");
  });

  it("unnamed 会话且无动作时显示等待首次输入", () => {
    render(<Sticker data={[mk({ task_title: "(未命名会话)", current_activity: null })]} />);
    expect(screen.getByText(zh.sticker.waitingFirstInput)).toBeTruthy();
  });

  it("connected 时显示已连接徽标", () => {
    render(<Sticker data={[mk({ connected: true })]} />);
    expect(screen.getByText(zh.conn.on)).toBeTruthy();
  });

  it("disconnected 时显示已断开徽标", () => {
    render(<Sticker data={[mk({ connected: false })]} />);
    expect(screen.getByText(zh.conn.off)).toBeTruthy();
  });

  it("stale + disconnected 显示已断开", () => {
    render(<Sticker data={[mk({ session: { id: 2, project_id: 1, cc_session_id: "x", status: "stale", started_at: 0, last_event_at: Date.now(), ended_at: null }, connected: false })]} />);
    expect(screen.getByText(zh.conn.off)).toBeTruthy();
  });

  it.each([
    ["all", zh.empty.allTitle, zh.empty.allHint],
    ["waiting", zh.empty.waitingTitle, zh.empty.waitingHint],
    ["running", zh.empty.runningTitle, null],
    ["archived", zh.empty.archivedTitle, zh.empty.archivedHint],
  ] as const)("EmptyState[%s] 渲染主文案与提示", (tab, title, hint) => {
    render(<EmptyState tab={tab} />);
    expect(screen.getByText(title)).toBeTruthy();
    if (hint) {
      expect(screen.getByText(hint)).toBeTruthy();
    }
  });

  it("EmptyState[running] 不渲染提示文案", () => {
    const { container } = render(<EmptyState tab="running" />);
    expect(container.querySelector(".stk-empty-hint")).toBeNull();
  });

  it("errored 会话归入待交互、显示红点与错误文案", () => {
    const item = mk({
      session: { id: 9, project_id: 1, cc_session_id: "s9", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
      errored: true, error_label: "工具调用解析失败", error_raw: "The model's tool call could not be parsed (retry also failed).",
    });
    const { container } = render(<Sticker data={[item]} />);
    const waitingTab = screen.getByText(zh.tabs.waiting).closest(".stab")!;
    expect(waitingTab.querySelector(".stab-n")!.textContent).toBe("1");
    const runningTab = screen.getByText(zh.tabs.running).closest(".stab")!;
    expect(runningTab.querySelector(".stab-n")!.textContent).toBe("0");
    expect(container.querySelector(".needs-error")).toBeTruthy();
    expect(screen.getByText("工具调用解析失败")).toBeTruthy();
    expect(screen.getByText("工具调用解析失败").closest(".stk-sub-err")).toBeTruthy();
  });

  it("运行中卡片在徽标圆内显示 Content 已用百分比", () => {
    const { container } = render(<Sticker data={[mk({ context_pct: 47 })]} />);
    expect(container.querySelector(".run-badge")).toBeTruthy();
    expect(screen.getByText("47%")).toBeTruthy();
  });

  it("无 context_pct 时只渲染绿圆、不渲染百分比文字", () => {
    const { container } = render(<Sticker data={[mk({ context_pct: null })]} />);
    expect(container.querySelector(".run-badge")).toBeTruthy();
    expect(container.querySelector(".run-core")?.textContent).toBe("");
  });

  it("待交互卡片用黄色徽标 run-badge--waiting，且同样显示百分比", () => {
    const { container } = render(<Sticker data={[mk({
      session: { id: 3, project_id: 1, cc_session_id: "w", status: "waiting", started_at: 0, last_event_at: Date.now(), ended_at: null },
      connected: true, context_pct: 30,
    })]} />);
    expect(container.querySelector(".run-badge--waiting")).toBeTruthy();
    expect(screen.getByText("30%")).toBeTruthy();
  });

  it("断开优先于 errored：只显示断开环", () => {
    const item = mk({ connected: false, errored: true, error_label: "认证失败" });
    const { container } = render(<Sticker data={[item]} />);
    expect(container.querySelector(".ring-stop")).toBeTruthy();
    expect(container.querySelector(".needs-error")).toBeFalsy();
  });
});
