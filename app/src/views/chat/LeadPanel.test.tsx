import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LeadGateDto, LeadTaskDto } from "../../api";

const snapshot = vi.fn<(cwd: string, session?: string | null) => Promise<LeadTaskDto | null>>();
const answer = vi.fn<(cwd: string, gateId: string, choice: string, note: string) => Promise<void>>();
const retryDelivery = vi.fn<(cwd: string, gateId: string) => Promise<void>>();
vi.mock("../../api", async (importActual) => ({
  ...(await importActual<typeof import("../../api")>()),
  leadTaskSnapshot: (cwd: string, session?: string | null) => snapshot(cwd, session),
  answerLeadGate: (cwd: string, gateId: string, choice: string, note: string) => answer(cwd, gateId, choice, note),
  retryLeadGateDelivery: (cwd: string, gateId: string) => retryDelivery(cwd, gateId),
}));

const { DockWork } = await import("./DockWork");
const { LeadGateCard, LeadPanel, gateWaiting, leadProgress } = await import("./LeadPanel");

const history = { plan: undefined, agentModes: [], pendingReview: null, todos: [] };

function task(over: Partial<LeadTaskDto> = {}): LeadTaskDto {
  return {
    slug: "20261109-demo",
    phase: "implement",
    spec: "# SPEC\n目的：演示",
    checks: [
      { id: "1", check: "测试全过", passes: true, waived: null, leader: false },
      { id: "2", check: "手机能用", passes: false, waived: null, leader: false },
    ],
    decisions: "| 决策 |",
    progress: "phase: implement",
    gate: null,
    ...over,
  };
}

function gate(kind: "A" | "B" | "C" | "D" | "Q", over: Partial<LeadGateDto> = {}): LeadGateDto {
  return {
    id: `${kind}-1`,
    kind,
    title: "选机制",
    summary: "**两种做法**",
    options: kind === "B" || kind === "Q" ? [{ id: "1", label: "方案一", detail: "持久化", recommended: true }, { id: "2", label: "方案二", detail: "", recommended: false }] : [],
    status: "pending",
    answer: null,
    ...over,
  };
}

beforeEach(() => {
  snapshot.mockReset();
  answer.mockReset();
  retryDelivery.mockReset();
});
afterEach(cleanup);

describe("Lead 入口", () => {
  it("模型回执不冒充关卡接手，不确定投递没有盲重发按钮", () => {
    render(<LeadPanel task={task({ gate: gate("Q", { status: "answered" }), delivery: {
      sessionId: 1, cwd: "C:/repo", deliveryId: "stable", state: "uncertain", reason: null, canRetry: false,
    } })} onClose={() => {}} />);
    expect(screen.getByText("送达结果不确定，请在会话中核对后处理")).toBeTruthy();
    expect(screen.queryByText("Agent 已接手关卡")).toBeNull();
    expect(screen.queryByRole("button", { name: "继续送达" })).toBeNull();
  });

  // 做空指出：关卡归原生终端会话（codex/kimi/gemini/opencode）时，答复只保存不代投，面板此前什么都不显示。
  it("原生终端会话的答复如实说下一步，不给看似能用的重试按钮", () => {
    render(<LeadPanel task={task({ gate: gate("Q", { status: "answered" }), delivery: {
      sessionId: 1, cwd: "C:/repo", deliveryId: "", state: "native_manual", reason: null, canRetry: false,
    } })} onClose={() => {}} />);
    expect(screen.getByText("回答已保存，在会话里发一句话即可继续")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "继续送达" })).toBeNull();
  });

  it("暂停后显式继续使用关卡稳定标识，并展示恢复失败", async () => {
    retryDelivery.mockRejectedValue("conversation/previous_process_unverified");
    render(<LeadPanel task={task({ gate: gate("Q", { status: "answered" }), delivery: {
      sessionId: 1, cwd: "C:/repo", deliveryId: "stable", state: "paused", reason: null, canRetry: true,
    } })} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "继续送达" }));
    expect((await screen.findByRole("alert")).textContent).toContain("尚未确认上次会话进程已退出");
    expect(retryDelivery).toHaveBeenCalledWith("C:/repo", "Q-1");
  });

  it("只有任务文件consumed显示已接手，且不再显示恢复按钮", () => {
    render(<LeadPanel task={task({ gate: gate("Q", { status: "consumed" }), delivery: {
      sessionId: 1, cwd: "C:/repo", deliveryId: "stable", state: "consumed", reason: null, canRetry: true,
    } })} onClose={() => {}} />);
    expect(screen.getByText("Agent 已接手关卡")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "继续送达" })).toBeNull();
  });

  it("项目里没有任务（快照为 null）时不出入口", async () => {
    snapshot.mockResolvedValue(null);
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    // 带上会话 id：认领了的任务只给认领它的会话看（用户反馈 2026-11-09）。
    await waitFor(() => expect(snapshot).toHaveBeenCalledWith("C:/repo", "cc-1"));
    expect(screen.queryByRole("button", { name: /Lead/ })).toBeNull();
  });

  it("还不知道会话 id（历史没到）时不请求：免得别的会话的任务闪一下", () => {
    render(<DockWork sessionId={1} cwd="C:/repo" items={[]} history={history} loading={false} />);
    expect(snapshot).not.toHaveBeenCalled();
  });

  it("没有 cwd 时不请求、不出入口", () => {
    render(<DockWork sessionId={1} cwd={null} items={[]} history={history} loading={false} />);
    expect(snapshot).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /Lead/ })).toBeNull();
  });

  it("读快照失败（文件损坏、目录不是会话目录）按没有任务处理，不崩", async () => {
    snapshot.mockRejectedValue("lead/no_task");
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    await waitFor(() => expect(snapshot).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: /Lead/ })).toBeNull();
  });

  it("有任务时入口显示验收进度，点开面板显示阶段与逐条验收", async () => {
    snapshot.mockResolvedValue(task());
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    const pill = await screen.findByRole("button", { name: /Lead.*1\/2/ });
    fireEvent.click(pill);
    expect(screen.getByText("实施中")).toBeTruthy();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("1");
    expect(screen.getByText("手机能用")).toBeTruthy();
    // 用户选定的样式（2026-11-09）：没通过的排前面、不显示 #编号；点一条展开全文。
    const rows = [...document.querySelectorAll(".lead-check")];
    expect(rows.map((row) => row.textContent)).toEqual(["手机能用", "测试全过"]);
    expect(document.body.textContent).not.toContain("#1");
    fireEvent.click(rows[0]);
    expect(rows[0].className).toContain("is-open");
    fireEvent.click(screen.getByRole("tab", { name: "规格" }));
    expect(screen.getByText(/目的：演示/)).toBeTruthy();
  });

  it("关卡挂起时不用点开面板，卡片直接出现；答完后的关卡不再显示卡片", async () => {
    snapshot.mockResolvedValue(task({ gate: gate("B") }));
    const { unmount } = render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    expect(await screen.findByText(/关卡 B 拍板 · 选机制/)).toBeTruthy();
    unmount();
    snapshot.mockResolvedValue(task({ gate: gate("B", { status: "answered", answer: { choice: "1", note: "" } }) }));
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    await screen.findByRole("button", { name: /Lead/ });
    expect(screen.queryByText(/关卡 B 拍板/)).toBeNull();
    // 已答复、agent 还没接手：入口上要看得出来（第二轮 M2 的降级提示）
    expect(screen.getByText("已答复·等 agent 接手")).toBeTruthy();
  });
});

describe("网络抖动", () => {
  /// 做空审查 M3：手机远程中继一次失败，卡片不能卸载——leader 写了一半的驳回意见会丢。
  it("快照读失败时保留上一次的数据和卡片里没提交的意见", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      snapshot.mockResolvedValue(task({ gate: gate("C") }));
      render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
      const note = await screen.findByPlaceholderText("附加意见（可选）");
      fireEvent.change(note, { target: { value: "截图第二张对不上" } });
      snapshot.mockRejectedValue("network");
      await act(async () => { vi.advanceTimersByTime(4500); });
      await waitFor(() => expect(snapshot.mock.calls.length).toBeGreaterThanOrEqual(2));
      expect((screen.getByPlaceholderText("附加意见（可选）") as HTMLTextAreaElement).value).toBe("截图第二张对不上");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("关卡卡片", () => {
  it("摘要只在真被截断时渐隐并给「展开全部」；卡片宽度变了会重新量", () => {
    // 实拍：摘要刚好放得下时最后一行也被淡掉，又没有「展开全部」，那行字读不全。
    let contentHeight = 100;
    const scroll = vi.spyOn(Element.prototype, "scrollHeight", "get").mockImplementation(() => contentHeight);
    const client = vi.spyOn(Element.prototype, "clientHeight", "get").mockImplementation(() => 100);
    let resize = () => {};
    const original = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(callback: ResizeObserverCallback) { resize = () => callback([], this as unknown as ResizeObserver); }
      observe() {}
      unobserve() {}
      disconnect() {}
    } as unknown as typeof ResizeObserver;
    try {
      const { container } = render(<LeadGateCard cwd="C:/repo" gate={gate("B")} onAnswered={() => {}} />);
      const summary = container.querySelector(".lead-gate-summary")!;
      expect(summary.classList.contains("is-clipped")).toBe(false);
      expect(screen.queryByRole("button", { name: "展开全部" })).toBeNull();
      // 卡片变窄，摘要折成更多行，超出了。
      contentHeight = 300;
      act(() => resize());
      expect(summary.classList.contains("is-clipped")).toBe(true);
      fireEvent.click(screen.getByRole("button", { name: "展开全部" }));
      expect(summary.classList.contains("is-clipped")).toBe(false);
      expect(summary.classList.contains("is-expanded")).toBe(true);
    } finally {
      scroll.mockRestore();
      client.mockRestore();
      globalThis.ResizeObserver = original;
    }
  });

  it("A：确认直接提交；选「要改」必须写意见", async () => {
    answer.mockResolvedValue();
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={onAnswered} />);
    const submit = screen.getByRole("button", { name: "提交" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.click(screen.getByLabelText("要改"));
    expect(submit.disabled).toBe(true);
    fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "验收 3 太宽" } });
    expect(submit.disabled).toBe(false);
    fireEvent.click(screen.getByLabelText("确认，开工"));
    await act(async () => { fireEvent.click(submit); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "A-1", "confirm", "验收 3 太宽");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("B：列出 agent 写的方案并标推荐，附意见可选", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("B")} onAnswered={() => {}} />);
    expect(screen.getByText("推荐")).toBeTruthy();
    expect(screen.getByText("持久化")).toBeTruthy();
    fireEvent.click(screen.getByLabelText(/方案二/));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "B-1", "2", "");
  });

  it("C：驳回必须写意见；后端报错时显示双语映射后的文案，不收卡", async () => {
    answer.mockRejectedValue("lead/gate_closed");
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={onAnswered} />);
    fireEvent.click(screen.getByLabelText("驳回"));
    fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "截图对不上" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "C-1", "reject", "截图对不上");
    expect((await screen.findByRole("alert")).textContent).toBe("这个关卡已经回答过了");
    expect(onAnswered).toHaveBeenCalledOnce();
  });
});

describe("纯函数", () => {
  it("只有 pending 算在等 leader；进度按 passes 计", () => {
    expect(gateWaiting(task({ gate: gate("A") }))).toBe(true);
    expect(gateWaiting(task({ gate: gate("A", { status: "answered" }) }))).toBe(false);
    expect(gateWaiting(null)).toBe(false);
    expect(leadProgress(task())).toEqual({ passed: 1, total: 2 });
  });

  it("豁免项不进进度分母（实测三个任务带着没通过的项收工）", () => {
    const checks = [
      { id: "1", check: "a", passes: true, waived: null, leader: false },
      { id: "2", check: "b", passes: false, waived: "leader 选了不做", leader: false },
      { id: "3", check: "由 leader 验收", passes: false, waived: null, leader: true },
    ];
    expect(leadProgress(task({ checks }))).toEqual({ passed: 1, total: 2 });
    // 先豁免、后来又做完的条目按通过算（与后端一致）
    const done = [{ id: "1", check: "a", passes: true, waived: "曾经不做", leader: false }];
    expect(leadProgress(task({ checks: done }))).toEqual({ passed: 1, total: 1 });
  });
});

describe("验收清单与新关卡", () => {
  it("豁免项沉底并显示原因，待 leader 验收的项有标签", () => {
    const checks = [
      { id: "1", check: "已豁免的", passes: false, waived: "leader 选了不做", leader: false },
      { id: "2", check: "已通过的", passes: true, waived: null, leader: false },
      { id: "3", check: "视觉（由 leader 验收）", passes: false, waived: null, leader: true },
      { id: "4", check: "还要做的", passes: false, waived: null, leader: false },
    ];
    const { container } = render(<LeadPanel task={task({ checks })} onClose={() => {}} />);
    const order = [...container.querySelectorAll(".lead-check-text")].map((n) => n.textContent);
    expect(order).toEqual(["还要做的", "视觉（由 leader 验收）", "已通过的", "已豁免的"]);
    expect(screen.getByText("已豁免：leader 选了不做")).toBeTruthy();
    expect(screen.getByText("待你验收")).toBeTruthy();
    expect(screen.getByLabelText("已豁免")).toBeTruthy();
  });

  it("关卡 D 给固定三个选项并按 id 作答", async () => {
    answer.mockResolvedValue(undefined);
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("D", { title: "开一台云主机" })} onAnswered={onAnswered} />);
    expect(screen.getByText("关卡 D 授权或操作 · 开一台云主机")).toBeTruthy();
    for (const label of ["允许", "我已完成", "不允许"]) expect(screen.getByLabelText(label)).toBeTruthy();
    fireEvent.click(screen.getByLabelText("我已完成"));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "D-1", "done", "");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("关卡 Q 用 agent 写的选项", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q")} onAnswered={() => {}} />);
    expect(screen.getByText("关卡 Q 回答问题 · 选机制")).toBeTruthy();
    expect(screen.getByLabelText(/方案一/)).toBeTruthy(); // 标签里还含「推荐」和说明
  });

  it("关卡 Q 没有选项时也能选「其他」只写意见作答，意见必填", async () => {
    answer.mockResolvedValue(undefined);
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q", { options: [] })} onAnswered={() => {}} />);
    fireEvent.click(screen.getByLabelText("其他（写在意见里）"));
    const submit = screen.getByRole("button", { name: "提交" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.change(screen.getByPlaceholderText("写下你的回答（必填）"), { target: { value: "按周统计" } });
    await act(async () => { fireEvent.click(submit); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "Q-1", "other", "按周统计");
  });

  it("agent 自己写了 id 为 other 的选项时不再追加「其他」", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q", { options: [{ id: "other", label: "别的城市", detail: "", recommended: false }] })} onAnswered={() => {}} />);
    expect(screen.getAllByRole("radio")).toHaveLength(1);
    expect(screen.queryByLabelText("其他（写在意见里）")).toBeNull();
  });
});

// jsdom 不排版，量不了「卡片是否超出屏幕」；这里按文本守住三条让关卡卡片在手机上不出屏的规则。
// 实际效果靠 Playwright iPhone 仿真验证过（修复前视口 664、dock 619 从 315 起，卡底和提交按钮在屏外）。
describe("关卡卡片在底部 dock 里的高度", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const css = readFileSync(join(here, "../../styles.css"), "utf8");
  /// 取某条规则大括号里的声明文本。只认写在一行、选择器顶格的规则（styles.css 的惯例）；
  /// 规则改成多行会返回空串，断言随之失败——那时改这里，别放宽断言。
  const ruleBody = (selector: string) => {
    const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return css.match(new RegExp(`^${escaped} \\{([^}]*)\\}`, "m"))?.[1] ?? "";
  };

  it("dock 可收缩且是纵向 flex，卡片能让出高度、其余子项不让", () => {
    expect(ruleBody(".chat-dock")).toMatch(/flex: 0 1 auto/);
    expect(ruleBody(".chat-dock")).toMatch(/min-height: 0/);
    expect(ruleBody(".chat-dock")).toMatch(/flex-direction: column/);
    expect(ruleBody(".chat-dock > *")).toMatch(/flex-shrink: 0/);
    expect(ruleBody(".chat-dock > .lead-gate-card")).toMatch(/flex-shrink: 1/);
    // 下限只能是固定的小值（标题 + 按钮行）：按视口比例的下限会把输入框挤出窗口（桌面版实拍），
    // 0 又会让提交按钮伸出卡外、被输入框盖住。
    expect(ruleBody(".chat-dock > .lead-gate-card")).toMatch(/min-height: 104px;/);
    // 浮层有卡时放弃下限并裁掉溢出：下限会把 iPhone SE 的输入框挤出屏幕。
    const withOverlay = ruleBody(".chat-main:has(> .chat-approval-overlay.is-active) > .chat-dock > .lead-gate-card");
    expect(withOverlay).toMatch(/min-height: 0;/);
    expect(withOverlay).toMatch(/overflow: hidden;/);
  });
});
