import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LeadGateDto, LeadTaskDto } from "../../api";

const snapshot = vi.fn<(cwd: string, session?: string | null) => Promise<LeadTaskDto | null>>();
const answer = vi.fn<(cwd: string, gateId: string, choice: string, note: string) => Promise<void>>();
vi.mock("../../api", async (importActual) => ({
  ...(await importActual<typeof import("../../api")>()),
  leadTaskSnapshot: (cwd: string, session?: string | null) => snapshot(cwd, session),
  answerLeadGate: (cwd: string, gateId: string, choice: string, note: string) => answer(cwd, gateId, choice, note),
}));

const { DockWork } = await import("./DockWork");
const { LeadGateCard, LeadPanel, gateWaiting, leadLatest, leadProgress, leadTaskTitle } = await import("./LeadPanel");

const history = { plan: undefined, agentModes: [], pendingReview: null, todos: [] };

function task(over: Partial<LeadTaskDto> = {}): LeadTaskDto {
  return {
    slug: "20261109-demo",
    phase: "implement",
    spec: "# SPEC\n目的：演示",
    checks: [
      { id: "1", check: "测试全过", passes: true, waived: null, leader: false, blocked: null },
      { id: "2", check: "手机能用", passes: false, waived: null, leader: false, blocked: null },
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
});
afterEach(cleanup);

describe("Lead 入口", () => {
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
    // 标题是任务名（SPEC 第一个标题），阶段在阶段条上标成当前一步
    expect(document.querySelector(".lead-title-text")?.textContent).toBe("SPEC");
    expect(document.querySelector('[aria-current="step"]')?.textContent).toBe("实施");
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
    expect(await screen.findByText("拍板 · 选机制")).toBeTruthy();
    unmount();
    snapshot.mockResolvedValue(task({ gate: gate("B", { status: "answered", answer: { choice: "1", note: "" } }) }));
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    await screen.findByRole("button", { name: /Lead/ });
    expect(screen.queryByText(/拍板 · 选机制/)).toBeNull();
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
      fireEvent.click(await screen.findByRole("button", { name: "驳回" }));
      fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "截图第二张对不上" } });
      snapshot.mockRejectedValue("network");
      await act(async () => { vi.advanceTimersByTime(4500); });
      await waitFor(() => expect(snapshot.mock.calls.length).toBeGreaterThanOrEqual(2));
      expect((screen.getByPlaceholderText("写下要改什么（必填）") as HTMLTextAreaElement).value).toBe("截图第二张对不上");
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
    // 卡片外壳（ApprovalCard）也有自己的尺寸观察者（正文被裁的提示），所以把每个观察者都叫一遍，
    // 不能只记最后构造的那个。
    const observers: Array<() => void> = [];
    const resize = () => observers.forEach((fire) => fire());
    const original = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(callback: ResizeObserverCallback) { observers.push(() => callback([], this as unknown as ResizeObserver)); }
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

  it("A：「确认，开工」一步提交；「要改」先展开意见框，必填，写完才发送", async () => {
    answer.mockResolvedValue();
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={onAnswered} />);
    // 固定选项不再是单选 + 提交
    expect(screen.queryByRole("radio")).toBeNull();
    expect(screen.queryByRole("button", { name: "提交" })).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "要改" }));
    expect(answer).not.toHaveBeenCalled();
    const note = screen.getByPlaceholderText("写下要改什么（必填）");
    expect(document.activeElement).toBe(note);
    const send = screen.getByRole("button", { name: "发送" }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    // 取消回到两颗按钮，不提交
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "要改" }));
    fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "验收 3 太宽" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "发送" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "A-1", "revise", "验收 3 太宽");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("A：点「确认，开工」直接提交", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={() => {}} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "确认，开工" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "A-1", "confirm", "");
  });

  it("标题说要你做什么，不带关卡代号，也没有重复的「待拍板」徽标", () => {
    for (const [kind, label] of [["A", "确认规格"], ["B", "拍板"], ["C", "验收"], ["D", "授权或操作"], ["Q", "回答问题"]] as const) {
      const { unmount } = render(<LeadGateCard cwd="C:/repo" gate={gate(kind)} onAnswered={() => {}} />);
      expect(screen.getByText(`${label} · 选机制`)).toBeTruthy();
      expect(document.body.textContent).not.toMatch(/关卡 [ABCDQ]/);
      expect(document.querySelector(".chat-approval-badge")).toBeNull();
      unmount();
    }
  });

  it("否定按钮挂 is-deny：键盘 Tab 进卡先落在它上面，它只展开意见框，不会误提交", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    const deny = screen.getByRole("button", { name: "驳回" });
    expect(deny.classList.contains("is-deny")).toBe(true);
    expect(screen.getByRole("button", { name: "验收通过" }).classList.contains("btn-primary")).toBe(true);
    // 「驳回」在 DOM 序里排在「验收通过」前面（与审批卡一致：否定在左、主动作在右）
    const buttons = [...document.querySelectorAll(".chat-approval-actions > button")].map((b) => b.textContent);
    expect(buttons).toEqual(["驳回", "验收通过"]);
    fireEvent.keyDown(deny, { key: "Enter" });
    fireEvent.click(deny);
    expect(answer).not.toHaveBeenCalled();
  });

  it("B：列出 agent 写的方案并标推荐，附意见可选", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("B")} onAnswered={() => {}} />);
    expect(screen.getByText("推荐")).toBeTruthy();
    expect(screen.getByText("持久化")).toBeTruthy();
    // 不预选推荐项，提交先禁用；意见框默认收起，点「附加意见」才出来
    expect(screen.getAllByRole("radio").some((r) => (r as HTMLInputElement).checked)).toBe(false);
    expect((screen.getByRole("button", { name: "提交" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "附加意见" }));
    expect(document.activeElement).toBe(screen.getByPlaceholderText("附加意见（可选）"));
    fireEvent.click(screen.getByLabelText(/方案二/));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "B-1", "2", "");
  });

  it("C：驳回必须写意见；后端报错时显示双语映射后的文案，不收卡", async () => {
    answer.mockRejectedValue("lead/gate_closed");
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={onAnswered} />);
    fireEvent.click(screen.getByRole("button", { name: "驳回" }));
    fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "截图对不上" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "发送" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "C-1", "reject", "截图对不上");
    expect((await screen.findByRole("alert")).textContent).toBe("这个关卡已经回答过了");
    expect(onAnswered).not.toHaveBeenCalled();
    // 失败不收卡、意见还在，可以再发
    expect((screen.getByPlaceholderText("写下要改什么（必填）") as HTMLTextAreaElement).value).toBe("截图对不上");
  });

  it("取消驳回后再点通过：藏起来的驳回意见不跟着发出去（做空审查复现）", async () => {
    answer.mockResolvedValue();
    for (const [kind, deny, ok, id] of [["C", "驳回", "验收通过", "accept"], ["D", "不允许", "允许", "approve"]] as const) {
      answer.mockClear();
      const { unmount } = render(<LeadGateCard cwd="C:/repo" gate={gate(kind)} onAnswered={() => {}} />);
      fireEvent.click(screen.getByRole("button", { name: deny }));
      fireEvent.change(screen.getByRole("textbox"), { target: { value: "别合" } });
      fireEvent.click(screen.getByRole("button", { name: "取消" }));
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: ok })); });
      expect(answer).toHaveBeenCalledWith("C:/repo", `${kind}-1`, id, "");
      unmount();
    }
  });

  it("Q 改选后，藏起来的回答不发；可选意见可以收回，收回即清空", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByLabelText("其他（写在意见里）"));
    fireEvent.change(screen.getByPlaceholderText("写下你的回答（必填）"), { target: { value: "按周统计" } });
    fireEvent.click(screen.getByLabelText(/方案一/));
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "附加意见" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "随便写写" } });
    fireEvent.click(screen.getByRole("button", { name: "不写意见了" }));
    expect(screen.queryByRole("textbox")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "Q-1", "1", "");
  });

  it("一步提交的按钮在请求中显示「提交中…」", async () => {
    let resolve = () => {};
    answer.mockReturnValue(new Promise<void>((r) => { resolve = r; }));
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "验收通过" }));
    expect(screen.getByRole("button", { name: "提交中…" })).toBeTruthy();
    await act(async () => { resolve(); });
  });

  it("C：「验收通过」一步提交", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "验收通过" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "C-1", "accept", "");
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
      { id: "1", check: "a", passes: true, waived: null, leader: false, blocked: null },
      { id: "2", check: "b", passes: false, waived: "leader 选了不做", leader: false, blocked: null },
      { id: "3", check: "由 leader 验收", passes: false, waived: null, leader: true, blocked: null },
    ];
    expect(leadProgress(task({ checks }))).toEqual({ passed: 1, total: 2 });
    // 先豁免、后来又做完的条目按通过算（与后端一致）
    const done = [{ id: "1", check: "a", passes: true, waived: "曾经不做", leader: false, blocked: null }];
    expect(leadProgress(task({ checks: done }))).toEqual({ passed: 1, total: 1 });
  });
});

describe("验收清单与新关卡", () => {
  it("豁免项沉底并显示原因，待 leader 验收的项有标签", () => {
    const checks = [
      { id: "1", check: "已豁免的", passes: false, waived: "leader 选了不做", leader: false, blocked: null },
      { id: "2", check: "已通过的", passes: true, waived: null, leader: false, blocked: null },
      { id: "3", check: "视觉（由 leader 验收）", passes: false, waived: null, leader: true, blocked: null },
      { id: "4", check: "还要做的", passes: false, waived: null, leader: false, blocked: null },
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
    expect(screen.getByText("授权或操作 · 开一台云主机")).toBeTruthy();
    for (const label of ["允许", "我已完成", "不允许"]) expect(screen.getByRole("button", { name: label })).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "我已完成" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "D-1", "done", "");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("关卡 D「不允许」意见可空，直接发送", async () => {
    answer.mockResolvedValue(undefined);
    render(<LeadGateCard cwd="C:/repo" gate={gate("D")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "不允许" }));
    expect(answer).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("附加意见（可选）")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "发送" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "D-1", "deny", "");
  });

  it("关卡 Q 用 agent 写的选项", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q")} onAnswered={() => {}} />);
    expect(screen.getByText("回答问题 · 选机制")).toBeTruthy();
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

describe("Lead 面板概览", () => {
  it("任务名取 SPEC 第一个标题、去掉末尾括号里的目录名；没有标题用目录名", () => {
    expect(leadTaskTitle({ slug: "20261112-x", spec: "# Lead 面板优化（20261112-x）\n\n## 目的" })).toBe("Lead 面板优化");
    expect(leadTaskTitle({ slug: "20261112-x", spec: "前言\n# 标题 (abc-1)\n" })).toBe("标题");
    expect(leadTaskTitle({ slug: "20261112-x", spec: "## 只有二级标题" })).toBe("20261112-x");
    expect(leadTaskTitle({ slug: "20261112-x", spec: "" })).toBe("20261112-x");
  });

  it("最新进展取最后一条非空行，去掉列表符号，跳过 phase 行和标题", () => {
    expect(leadLatest("phase: verify\n- 第一步\n- 做空第 2 轮：可以交付\n\n")).toBe("做空第 2 轮：可以交付");
    expect(leadLatest("phase: implement\n# 进度\n")).toBeNull();
    expect(leadLatest("")).toBeNull();
    expect(leadLatest("1. 跑测试")).toBe("跑测试");
    // 缩进的子条目、勾选框、代码块围栏、表格行都不算「最近」
    expect(leadLatest("- 做空第 1 轮\n  - 缺口：留白偏大")).toBe("做空第 1 轮");
    expect(leadLatest("- [x] 跑测试")).toBe("跑测试");
    expect(leadLatest("- 写了脚本\n```\nbun test\n```")).toBe("写了脚本");
    expect(leadLatest("- 对比结果\n| a | b |\n|---|---|")).toBe("对比结果");
  });

  it("阶段条六段：走过的标 is-done、当前 aria-current；done 全部走完；blocked 另标红签；unknown 不标当前", () => {
    const { container, rerender } = render(<LeadPanel task={task({ phase: "verify", progress: "phase: verify\n- 第 2 轮做空" })} onClose={() => {}} />);
    const stages = () => [...container.querySelectorAll(".lead-stage")];
    expect(stages().map((n) => n.textContent)).toEqual(["访谈", "规格", "方案", "实施", "验证", "完成"]);
    expect(stages().filter((n) => n.classList.contains("is-done")).length).toBe(4);
    expect(container.querySelector('[aria-current="step"]')?.textContent).toBe("验证");
    expect(container.querySelector(".lead-latest")?.textContent).toBe("最近：第 2 轮做空");
    rerender(<LeadPanel task={task({ phase: "done" })} onClose={() => {}} />);
    expect(stages().every((n) => n.classList.contains("is-done"))).toBe(true);
    expect(container.querySelector('[aria-current="step"]')).toBeNull();
    rerender(<LeadPanel task={task({ phase: "blocked" })} onClose={() => {}} />);
    expect(container.querySelector(".lead-stage-flag.is-blocked")?.textContent).toBe("卡住了");
    rerender(<LeadPanel task={task({ phase: "unknown", progress: "" })} onClose={() => {}} />);
    expect(container.querySelector('[aria-current="step"]')).toBeNull();
    expect(container.querySelector(".lead-stage-flag")?.textContent).toBe("阶段未知");
    rerender(<LeadPanel task={task({ phase: "review" })} onClose={() => {}} />);
    expect(container.querySelector(".lead-stage-flag")?.textContent).toBe("阶段：review");
    expect(container.querySelector(".lead-latest")).toBeNull();
  });
});

describe("待实测的条目", () => {
  /// 2026-11-12 leader：「为什么都已经到了验证阶段进度却才 5/18」——11 条代码写完、只差 beta 实测，看着像没做。
  it("blocked 条目单独标「待实测：原因」，排在还要做的之后；标题行报个数；已通过、已豁免的不受影响", () => {
    const checks = [
      { id: "1", check: "已通过的", passes: true, waived: null, leader: false, blocked: "旧原因" },
      { id: "2", check: "等 beta", passes: false, waived: null, leader: false, blocked: "要 beta 开机" },
      { id: "3", check: "还要做的", passes: false, waived: null, leader: false, blocked: null },
      { id: "4", check: "不做了", passes: false, waived: "leader 定的", leader: false, blocked: "要真机" },
    ];
    const { container } = render(<LeadPanel task={task({ checks })} onClose={() => {}} />);
    const order = [...container.querySelectorAll(".lead-check-text")].map((n) => n.textContent);
    expect(order).toEqual(["还要做的", "等 beta", "已通过的", "不做了"]);
    expect(screen.getByText("待实测：要 beta 开机")).toBeTruthy();
    expect(screen.getByLabelText("待实测")).toBeTruthy();
    expect(screen.queryByText("待实测：旧原因")).toBeNull();
    expect(screen.queryByText("待实测：要真机")).toBeNull();
    expect(container.querySelector(".lead-blocked-count")?.textContent).toBe("1 条待实测");
    // 进度仍只数实测过的：分母不含豁免项
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("1");
  });

  it("没有待实测的条目时不显示个数", () => {
    const { container } = render(<LeadPanel task={task()} onClose={() => {}} />);
    expect(container.querySelector(".lead-blocked-count")).toBeNull();
  });
});
