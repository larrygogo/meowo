import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

const invokeMock = vi.hoisted(() =>
  vi.fn((cmd: string, _args?: unknown) => {
    if (cmd === "get_settings") {
      return Promise.resolve({
        archive_hide_days: 0,
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
      });
    }
    if (cmd === "available_agents") return Promise.resolve(["claude"]);
    return Promise.resolve();
  })
);
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
}));

import { Sticker, EmptyState, UsageScreen } from "./Sticker";
import type { LiveSession, ProviderUsage } from "../api";
import { zh } from "../i18n/zh";

// jsdom 没有真实视口尺寸，@tanstack/react-virtual 会以为 .stk-scroll 高度为 0 而不渲染卡片。
// mock 一个足够大的滚动容器，让测试里的卡片进入可视区并被挂载。
const defaultRect: DOMRect = {
  top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0,
  toJSON: () => ({ top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, x: 0, y: 0 }),
};
vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement): DOMRect {
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
// 用同步触发 rect 的 ResizeObserver 替换原生实现，确保虚拟列表在测试查询前已完成尺寸计算。
const mockScrollRect = { top: 0, left: 0, bottom: 600, right: 400, width: 400, height: 600, x: 0, y: 0 };
const mockItemRect = { top: 0, left: 0, bottom: 82, right: 400, width: 400, height: 82, x: 0, y: 0 };
class MockResizeObserver {
  constructor(private cb: ResizeObserverCallback) {}
  observe(target: Element) {
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

type Item = LiveSession & { connected: boolean };

function mk(over: Partial<Item> = {}): Item {
  return {
    session: { id: 1, project_id: 1, cc_session_id: "s", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
    project_name: "proj",
    task_title: "做点事",
    current_activity: "正在做点事",
    column: "doing", todo_done: 0, todo_total: 0, todos: [],
    pid: 1234, connected: true, archived: false, cwd: null, errored: false, error_label: null, error_raw: null,
    provider: "claude",
    ...over,
  } as Item;
}

afterEach(() => {
  cleanup();
  localStorage.clear(); // 防 tab/star 等持久化状态跨用例泄漏
});
beforeEach(() => {
  invokeMock.mockClear();
});

describe("Sticker", () => {
  it("用量选择在卸载重挂后保留（记住上次选择，找不到才退第一个）", () => {
    const usageMap: Record<string, ProviderUsage> = {
      claude: { lanes: [], note: null } as ProviderUsage,
      codex: { lanes: [], note: null } as ProviderUsage,
    };
    const props = { quotaProviders: ["claude", "codex"], usageMap };
    const { unmount, container } = render(<UsageScreen {...props} />);
    const tabs = container.querySelectorAll(".stk-utab");
    expect(tabs.length).toBe(2);
    expect(tabs[0].classList.contains("on")).toBe(true); // 默认第一个 claude
    fireEvent.click(tabs[1]); // 选 codex
    expect(tabs[1].classList.contains("on")).toBe(true);
    unmount(); // 折叠 → 卸载
    const { container: c2 } = render(<UsageScreen {...props} />); // 展开 → 重挂
    const tabs2 = c2.querySelectorAll(".stk-utab");
    expect(tabs2[1].classList.contains("on")).toBe(true); // 应记住 codex
    expect(tabs2[0].classList.contains("on")).toBe(false);
  });

  it("待交互/运行中角标数字上限 99+", () => {
    const counts = { total: 0, running: 150, waiting: 3, archived: 0 };
    const { container } = render(<Sticker filter="all" data={[]} counts={counts} />);
    const badges = Array.from(container.querySelectorAll(".stab-n")).map((e) => e.textContent);
    expect(badges).toContain("99+"); // running=150 → 99+
    expect(badges).toContain("3"); // waiting=3 → 原样
  });

  it("空数据显示 all 空态主文案", () => {
    const { container } = render(<Sticker filter="all" data={[]} />);
    expect(screen.getByText(zh.empty.allTitle)).toBeTruthy();
    expect(container.querySelector("[data-tauri-drag-region]")).toBeTruthy();
  });

  it("渲染会话行：文件夹名 + 最近 AI 正文", () => {
    render(<Sticker filter="all" data={[mk({ cwd: "C:\\dev\\my-project", preview: "最近这条 AI 正文" })]} />);
    expect(screen.getByText("my-project")).toBeTruthy();
    expect(screen.getByText("最近这条 AI 正文")).toBeTruthy();
  });

  it("活动行常显最近 AI 正文(preview)，data-tip 带完整文本", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ preview: "需要你确认下一步" })]} />);
    const subEl = container.querySelector(".stk-sub") as HTMLElement;
    expect(subEl?.textContent).toBe("需要你确认下一步");
    expect(subEl?.getAttribute("data-tip")).toBe("需要你确认下一步");
  });

  it("无 preview 且无错误时不渲染活动行", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ preview: null })]} />);
    expect(container.querySelector(".stk-sub")).toBeNull();
  });

  it("右键菜单星标切换状态并持久化到 localStorage,操作后菜单关闭", () => {
    localStorage.removeItem("cc-kanban-starred");
    const { container } = render(<Sticker filter="all" data={[mk({ session: { id: 7, project_id: 1, cc_session_id: "star-me", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.star));
    expect(container.querySelector(".stk-card.is-star")).toBeTruthy();
    expect(JSON.parse(localStorage.getItem("cc-kanban-starred") ?? "[]")).toContain("star-me");
    expect(document.querySelector(".ctx-menu")).toBeNull(); // 任一菜单项执行后菜单关闭
    localStorage.removeItem("cc-kanban-starred");
  });

  it("右键打开菜单:含星标/便签/重命名/归档四项,Escape 关闭", () => {
    const { container } = render(<Sticker filter="all" data={[mk()]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    const menu = document.querySelector(".ctx-menu")!;
    expect(menu).toBeTruthy();
    const labels = Array.from(menu.querySelectorAll(".ctx-item")).map((el) => el.textContent);
    expect(labels).toEqual([zh.sticker.star, zh.sticker.noteAdd, zh.sticker.renameTitle, zh.sticker.archive, zh.sticker.newSession]);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector(".ctx-menu")).toBeNull();
  });

  it("点击菜单外部关闭菜单,且该次点击不触发卡片点击", () => {
    const { container } = render(<Sticker filter="all" data={[mk()]} />);
    // 先打开重命名编辑器作观察哨:卡片 onClick 若被触发会关闭编辑器。
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.renameTitle));
    expect(container.querySelector(".stk-edit")).toBeTruthy();
    // 再开菜单,点击卡片(菜单外部)——菜单应关闭,但编辑器保持打开,证明点击被捕获相拦下。
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    expect(document.querySelector(".ctx-menu")).toBeTruthy();
    fireEvent.click(container.querySelector(".stk-card")!);
    expect(document.querySelector(".ctx-menu")).toBeNull();
    expect(container.querySelector(".stk-edit")).toBeTruthy(); // 编辑器未被误关
  });

  it("默认(右键菜单模式)不渲染卡片菜单按钮", () => {
    // card_menu_mode=button 时按钮与右键二选一;按钮模式依赖设置注入,与 terminal_open_mode
    // 的按钮模式一样走手动验证(测试环境 getSettings 不可用,仅锁默认形态)。
    const { container } = render(<Sticker filter="all" data={[mk()]} />);
    expect(container.querySelector(".stk-menu-btn")).toBeNull();
  });

  it("有 cwd 的会话菜单末尾多出「打开项目目录」,无 cwd 则隐藏", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ cwd: "C:\\proj" })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    expect(screen.getByText(zh.sticker.openProjectDir)).toBeTruthy();
    expect(document.querySelector(".ctx-sep")).toBeTruthy(); // 与卡片管理操作以分隔线分组
    fireEvent.keyDown(document, { key: "Escape" });

    cleanup();
    const { container: c2 } = render(<Sticker filter="all" data={[mk({ cwd: null })]} />);
    fireEvent.contextMenu(c2.querySelector(".stk-card")!);
    expect(screen.queryByText(zh.sticker.openProjectDir)).toBeNull();
  });

  it("已星标/有便签/已归档的会话,菜单项显示反向文案", () => {
    localStorage.setItem("cc-kanban-starred", JSON.stringify(["s"]));
    const { container } = render(<Sticker filter="archived" data={[mk({ archived: true, note: "有便签" })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    const labels = Array.from(document.querySelectorAll(".ctx-item")).map((el) => el.textContent);
    expect(labels).toEqual([zh.sticker.unstar, zh.sticker.noteEdit, zh.sticker.renameTitle, zh.sticker.unarchive, zh.sticker.newSession]);
    localStorage.removeItem("cc-kanban-starred");
  });

  it("菜单「新建会话」用当前会话的 cwd 和 provider 打开新建窗口", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ cwd: "C:\\\\proj", provider: "kimi" })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.newSession));
    expect(invokeMock).toHaveBeenCalledWith("open_new_session_window", { cwd: "C:\\\\proj", provider: "kimi" });
  });

  it("待交互 tab 保留后端顺序，不客户端重排", () => {
    const base = (id: number, cc: string, last: number) =>
      mk({ task_title: cc, current_activity: null, connected: true,
        session: { id, project_id: 1, cc_session_id: cc, status: "waiting", started_at: 0, last_event_at: last, ended_at: null } });
    const now = Date.now();
    // 故意传入「非等待最久优先」的顺序（更近的排前面、等待更久的排后面）——
    // 若组件仍客户端按 last_event_at ASC 重排（旧实现），会把顺序翻成 [旧, 新]，断言会失败；
    // 新实现应原样保留后端给的顺序（只做 starred 浮顶），断言才会通过。
    const { container } = render(<Sticker filter="waiting" data={[
      base(1, "新", now - 60_000),   // 1 分钟前(更近)
      base(2, "旧", now - 600_000),  // 10 分钟前(等待更久)
    ]} />);
    const cards = container.querySelectorAll(".stk-card");
    expect(cards[0].querySelector(".stk-title")?.textContent).toBe("新");
    expect(cards[1].querySelector(".stk-title")?.textContent).toBe("旧");
  });

  it("已星标会话排到列表最前", () => {
    localStorage.setItem("cc-kanban-starred", JSON.stringify(["b"]));
    const { container } = render(<Sticker filter="all" data={[
      mk({ task_title: "甲", current_activity: null, session: { id: 1, project_id: 1, cc_session_id: "a", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } }),
      mk({ task_title: "乙", current_activity: null, session: { id: 2, project_id: 1, cc_session_id: "b", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null } }),
    ]} />);
    const cards = container.querySelectorAll(".stk-card");
    expect(cards[0].querySelector(".stk-title")?.textContent).toBe("乙");
    expect(cards[0].classList.contains("is-star")).toBe(true);
    localStorage.removeItem("cc-kanban-starred");
  });

  it("有便签时渲染便签块", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ note: "记得 review PR" })]} />);
    expect(screen.getByText("记得 review PR")).toBeTruthy();
    expect(container.querySelector(".stk-note")).toBeTruthy();
  });

  it("无便签时经右键菜单打开编辑框", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ note: null })]} />);
    expect(container.querySelector(".stk-note-edit")).toBeNull();
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.noteAdd));
    const input = container.querySelector(".stk-note-edit") as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.placeholder).toBe(zh.sticker.notePlaceholder);
  });

  it("点击便签块进入编辑并预填原文", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ note: "旧便签" })]} />);
    fireEvent.click(container.querySelector(".stk-note")!);
    const input = container.querySelector(".stk-note-edit") as HTMLInputElement;
    expect(input.value).toBe("旧便签");
  });

  it("便签编辑框有保存/取消按钮，点取消关闭且保留原文", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ note: "保留我" })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.noteEdit));
    expect(screen.getByLabelText(zh.sticker.noteSave)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(zh.sticker.noteCancel));
    expect(container.querySelector(".stk-note-edit")).toBeNull();
    expect(screen.getByText("保留我")).toBeTruthy(); // 便签块仍在
  });

  it("点便签保存按钮关闭编辑框", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ note: null })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.noteAdd));
    fireEvent.change(container.querySelector(".stk-note-edit") as HTMLInputElement, { target: { value: "新便签" } });
    fireEvent.click(screen.getByLabelText(zh.sticker.noteSave));
    expect(container.querySelector(".stk-note-edit")).toBeNull();
  });

  it("重命名编辑器有保存/取消按钮，点取消关闭", () => {
    const { container } = render(<Sticker filter="all" data={[mk()]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.renameTitle));
    expect(container.querySelector(".stk-editbox")).toBeTruthy();
    expect(screen.getByLabelText(zh.sticker.noteSave)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(zh.sticker.noteCancel));
    expect(container.querySelector(".stk-edit")).toBeNull();
  });

  it("编辑态下点击卡片只关闭编辑器、不导航开终端", () => {
    // 守卫成立的可观察证据：点击卡片后编辑器关闭（setEditingId(null) 只在早返回分支执行）；
    // 若无守卫，onClick 会走 focus_session 分支、editingId 不变、编辑器仍在。
    const { container } = render(<Sticker filter="all" data={[mk({ connected: true })]} />);
    fireEvent.contextMenu(container.querySelector(".stk-card")!);
    fireEvent.click(screen.getByText(zh.sticker.renameTitle));
    expect(container.querySelector(".stk-edit")).toBeTruthy();
    fireEvent.click(container.querySelector(".stk-card")!);
    expect(container.querySelector(".stk-edit")).toBeNull();
  });

  it("默认(点击卡片模式)不渲染独立打开按钮", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ connected: true })]} />);
    expect(container.querySelector(".stk-open")).toBeNull();
  });

  it("unnamed 会话且无动作时显示等待首次输入", () => {
    render(<Sticker filter="all" data={[mk({ task_title: "(未命名会话)", current_activity: null })]} />);
    expect(screen.getByText(zh.sticker.waitingFirstInput)).toBeTruthy();
  });

  it("connected 时 agent 图标高亮（非灰）", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ connected: true })]} />);
    const agent = container.querySelector(".stk-agent");
    expect(agent).toBeTruthy();
    expect(agent?.classList.contains("stk-agent-off")).toBe(false);
  });

  it("disconnected 时 agent 图标变灰（stk-agent-off）", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ connected: false })]} />);
    expect(container.querySelector(".stk-agent.stk-agent-off")).toBeTruthy();
  });

  it("stale + disconnected 时 agent 图标变灰", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ session: { id: 2, project_id: 1, cc_session_id: "x", status: "stale", started_at: 0, last_event_at: Date.now(), ended_at: null }, connected: false })]} />);
    expect(container.querySelector(".stk-agent.stk-agent-off")).toBeTruthy();
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

  it("errored running 会话归入运行中、显示红点与错误文案", () => {
    const item = mk({
      session: { id: 9, project_id: 1, cc_session_id: "s9", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
      errored: true, error_label: "工具调用解析失败", error_raw: "The model's tool call could not be parsed (retry also failed).",
    });
    const { container } = render(<Sticker filter="all" data={[item]} />);
    const waitingTab = screen.getByText(zh.tabs.waiting).closest(".stab")!;
    expect(waitingTab.querySelector(".stab-n")!.textContent).toBe("0");
    const runningTab = screen.getByText(zh.tabs.running).closest(".stab")!;
    expect(runningTab.querySelector(".stab-n")!.textContent).toBe("1");
    expect(container.querySelector(".needs-error")).toBeTruthy();
    expect(screen.getByText("工具调用解析失败")).toBeTruthy();
    expect(screen.getByText("工具调用解析失败").closest(".stk-sub-err")).toBeTruthy();
  });

  it("运行中卡片在徽标圆内显示 Content 已用百分比", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ context_pct: 47 })]} />);
    expect(container.querySelector(".run-badge")).toBeTruthy();
    expect(screen.getByText("47%")).toBeTruthy();
  });

  it("无 context_pct 时只渲染绿圆、不渲染百分比文字", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ context_pct: null })]} />);
    expect(container.querySelector(".run-badge")).toBeTruthy();
    expect(container.querySelector(".run-core")?.textContent).toBe("");
  });

  it("待交互卡片用黄色徽标 run-badge--waiting，且同样显示百分比", () => {
    const { container } = render(<Sticker filter="all" data={[mk({
      session: { id: 3, project_id: 1, cc_session_id: "w", status: "waiting", started_at: 0, last_event_at: Date.now(), ended_at: null },
      connected: true, context_pct: 30,
    })]} />);
    expect(container.querySelector(".run-badge--waiting")).toBeTruthy();
    expect(screen.getByText("30%")).toBeTruthy();
  });

  it("断开优先于 errored：只显示断开环", () => {
    const item = mk({ connected: false, errored: true, error_label: "认证失败" });
    const { container } = render(<Sticker filter="all" data={[item]} />);
    expect(container.querySelector(".ring-stop")).toBeTruthy();
    expect(container.querySelector(".needs-error")).toBeFalsy();
  });

  it("pending_review running 会话归入待交互并正常排序", () => {
    const sess = (id: number, cc: string, status: "running" | "waiting", last: number) =>
      ({ id, project_id: 1, cc_session_id: cc, status, started_at: 0, last_event_at: last, ended_at: null });
    const now = Date.now();
    const items = [
      mk({ task_title: "运行更久的", connected: true, session: sess(1, "r1", "running", now - 600_000) }),
      mk({ task_title: "待批准", connected: true, pending_review: "approval", session: sess(2, "p1", "running", now - 60_000) }),
    ];
    const { container } = render(<Sticker filter="waiting" data={items} />);
    // waiting tab 计数：status=waiting 与 pending_review 都计入
    const waitingTab = screen.getByText(zh.tabs.waiting).closest(".stab")!;
    expect(waitingTab.querySelector(".stab-n")!.textContent).toBe("1");
    // pending_review 会话显示在 waiting tab 下
    expect(container.querySelector(".stk-title")?.textContent).toBe("待批准");
    // running tab 计数：只含无需用户介入的纯 running
    const runningTab = screen.getByText(zh.tabs.running).closest(".stab")!;
    expect(runningTab.querySelector(".stab-n")!.textContent).toBe("1");
  });

  it("pending 会话显示琥珀 pill 与 pending 徽标", () => {
    const item = mk({
      task_title: "审批中",
      connected: true,
      pending_review: "approval",
      context_pct: 30,
      session: { id: 5, project_id: 1, cc_session_id: "pp", status: "running", started_at: 0, last_event_at: Date.now(), ended_at: null },
    });
    const { container } = render(<Sticker filter="all" data={[item]} />);
    expect(screen.getByText(zh.pending.approval)).toBeTruthy();     // pill 文字「待批准」
    expect(container.querySelector(".pending-pill")).toBeTruthy();  // pill 元素
    expect(container.querySelector(".run-badge--pending")).toBeTruthy(); // 琥珀徽标
  });

  it("卡片优先显示 last_ai_text,并显示用户消息行", () => {
    const item = mk({
      connected: true,
      preview: "transcript 兜底的旧预览",
      last_ai_text: "调研完成,结论更微妙",
      last_user_text: "切到这个任务",
      session: { id: 7, project_id: 1, cc_session_id: "uai", status: "waiting", started_at: 0, last_event_at: Date.now(), ended_at: null },
    });
    render(<Sticker filter="all" data={[item]} />);
    expect(screen.getByText("调研完成,结论更微妙")).toBeTruthy(); // AI 行用 last_ai_text 而非 preview
    expect(screen.queryByText("transcript 兜底的旧预览")).toBeNull();
    expect(screen.getByText("切到这个任务")).toBeTruthy();         // 用户消息行
    expect(screen.getByText(zh.sticker.youPrefix)).toBeTruthy();   // 「你」前缀
  });

  it("显示 AI 正文时有 aiPrefix 标签，与用户行对称", () => {
    const item = mk({
      connected: true,
      last_ai_text: "完成了代码审查",
      last_user_text: "帮我看这个 PR",
    });
    const { container } = render(<Sticker filter="all" data={[item]} />);
    // 「AI」前缀标签存在
    const tags = container.querySelectorAll(".stk-msg-tag");
    const tagTexts = Array.from(tags).map((el) => el.textContent);
    expect(tagTexts).toContain(zh.sticker.aiPrefix);  // AI 前缀
    expect(tagTexts).toContain(zh.sticker.youPrefix); // 你 前缀，两行对称
    // AI 标签带品牌色 is-ai 修饰类，用户标签不带（视觉区分主角/用户）
    const aiTag = Array.from(tags).find((el) => el.textContent === zh.sticker.aiPrefix)!;
    const youTag = Array.from(tags).find((el) => el.textContent === zh.sticker.youPrefix)!;
    expect(aiTag.classList.contains("is-ai")).toBe(true);
    expect(youTag.classList.contains("is-ai")).toBe(false);
  });

  it("errored 活动行不显示 aiPrefix 标签", () => {
    const item = mk({
      connected: true,
      errored: true,
      error_label: "工具调用解析失败",
      error_raw: "parse error",
    });
    const { container } = render(<Sticker filter="all" data={[item]} />);
    // 错误标签行存在（红色错误文案），但无 aiPrefix
    expect(container.querySelector(".stk-sub-err")).toBeTruthy();
    const tags = container.querySelectorAll(".stk-msg-tag");
    const tagTexts = Array.from(tags).map((el) => el.textContent);
    expect(tagTexts).not.toContain(zh.sticker.aiPrefix);
  });

  it("有 model 时渲染模型胶囊与 agent 图标", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ model: "Opus" })]} />);
    expect(container.querySelector(".stk-model")?.textContent).toBe("Opus");
    expect(container.querySelector(".stk-agent")).toBeTruthy();
  });

  it("无 model 时只渲染 agent 图标、不渲染模型胶囊", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ model: null })]} />);
    expect(container.querySelector(".stk-agent")).toBeTruthy();
    expect(container.querySelector(".stk-model")).toBeNull();
  });

  it("项目名使用 cwd 的文件夹名，data-tip 显示完整路径", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ cwd: "C:\\Users\\larry\\projects\\autopilot", project_name: "larrygogo/autopilot" })]} />);
    const repo = container.querySelector(".stk-repo") as HTMLElement;
    expect(repo?.textContent).toBe("autopilot");
    expect(repo?.getAttribute("data-tip")).toBe("C:\\Users\\larry\\projects\\autopilot");
  });

  it("无 cwd 时不显示项目名", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ cwd: null, project_name: "larrygogo/autopilot" })]} />);
    expect(container.querySelector(".stk-repo")).toBeNull();
  });

  it("kimi 会话用 Kimi Code 标签与 kimi 徽标(黑方块)，claude 用 Claude Code 标签且无方块", () => {
    const { container } = render(<Sticker filter="all" data={[mk({ provider: "kimi", project_name: "kimi-proj" })]} />);
    const agent = container.querySelector(".stk-agent") as HTMLElement;
    expect(agent.getAttribute("data-tip")).toBe(zh.sticker.agentKimiCode);
    expect(agent.getAttribute("aria-label")).toBe(zh.sticker.agentKimiCode);
    expect(agent.querySelector("img")).toBeTruthy(); // kimi 徽标内嵌官方 PNG（黑圆角方块已在图内）

    cleanup();
    const { container: c2 } = render(<Sticker filter="all" data={[mk({ provider: "claude" })]} />);
    const a2 = c2.querySelector(".stk-agent") as HTMLElement;
    expect(a2.getAttribute("data-tip")).toBe(zh.sticker.agentClaudeCode);
    expect(a2.querySelector("svg rect")).toBeNull(); // Claude logomark 无方块
  });

  it("搜索走后端：输入调用 onSearchChange，且不客户端过滤已加载数据", () => {
    const onSearchChange = vi.fn();
    const { container } = render(
      <Sticker filter="all" data={[mk({ task_title: "任务甲" })]} search="" onSearchChange={onSearchChange} />
    );
    const before = container.querySelectorAll(".stk-vitem").length;
    expect(before).toBeGreaterThan(0);
    // 打开搜索框并输入一个不匹配已加载标题的词
    fireEvent.click(screen.getByLabelText(zh.sticker.search));
    const input = container.querySelector(".stk-search-in") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "不匹配任何标题zzz" } });
    // 搜索词经回调交后端处理
    expect(onSearchChange).toHaveBeenCalledWith("不匹配任何标题zzz");
    // 前端不再按搜索词过滤已加载数据（过滤由后端负责）→ 卡片数不变
    expect(container.querySelectorAll(".stk-vitem").length).toBe(before);
  });
});
