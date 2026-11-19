import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LeadGateDto, LeadTaskDto } from "../../api";
import { zh } from "../../i18n/zh";
import { visibleText } from "../../test/visibleText";

const snapshot = vi.fn<(cwd: string, session?: string | null) => Promise<LeadTaskDto | null>>();
const answer = vi.fn<(cwd: string, gateId: string, choice: string, note: string, version?: string) => Promise<void>>();
const retryDelivery = vi.fn<(cwd: string, gateId: string) => Promise<void>>();
vi.mock("../../api", async (importActual) => ({
  ...(await importActual<typeof import("../../api")>()),
  leadTaskSnapshot: (cwd: string, session?: string | null) => snapshot(cwd, session),
  answerLeadGate: (...args: Parameters<typeof answer>) => answer(...args),
  retryLeadGateDelivery: (cwd: string, gateId: string) => retryDelivery(cwd, gateId),
}));

const { DockWork } = await import("./DockWork");
const { LeadGateCard, LeadPanel, useLeadTask, gateWaiting, leadLatest, leadProgress, leadTaskTitle } = await import("./LeadPanel");

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
    review: kind === "C" ? { revision: "build-1", checks: [{ id: "business", check: "确认业务范围", passes: false, leader: true, waived: null, blocked: null }] } : undefined,
    version: "frozen-v1",
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

  it("已保存待送达时显式继续使用关卡稳定标识，并展示送达失败原因", async () => {
    retryDelivery.mockRejectedValue("lead/codex_server_unreachable");
    render(<LeadPanel task={task({ gate: gate("Q", { status: "answered" }), delivery: {
      sessionId: 1, cwd: "C:/repo", deliveryId: "stable", state: "saved", reason: null, canRetry: true,
    } })} onClose={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "继续送达" }));
    expect((await screen.findByRole("alert")).textContent).toContain("连不上 Codex 会话服务");
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

  it("旧后端拒绝快照请求时显示异常，不当作确认没有任务", async () => {
    snapshot.mockRejectedValue("lead/no_task");
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    expect(await screen.findByRole("button", { name: /Lead.*数据待更新/ })).toBeTruthy();
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
    // 用户选定的样式（2026-11-09）：没通过的排前面、不显示 #编号。没有证据、说明的条目是普通的一行，不能点开（2026-11-19）。
    expect(screen.queryByText("测试全过")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 1" }));
    const rows = [...document.querySelectorAll(".lead-check")];
    expect(rows.map((row) => row.querySelector(".lead-check-text")?.textContent)).toEqual(["手机能用", "测试全过"]);
    expect(document.body.textContent).not.toContain("#1");
    expect(rows[0].tagName).toBe("DIV");
    expect(rows[0].className).toContain("is-static");
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
    expect(answer).toHaveBeenCalledWith("C:/repo", "A-1", "revise", "验收 3 太宽", "frozen-v1");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("A：点「确认，开工」直接提交", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={() => {}} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "确认，开工" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "A-1", "confirm", "", "frozen-v1");
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
    expect(answer).toHaveBeenCalledWith("C:/repo", "B-1", "2", "", "frozen-v1");
  });

  it("C：驳回必须写意见；后端报错时显示双语映射后的文案，不收卡", async () => {
    answer.mockRejectedValue("lead/gate_closed");
    const onAnswered = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={onAnswered} />);
    fireEvent.click(screen.getByRole("button", { name: "驳回" }));
    fireEvent.change(screen.getByPlaceholderText("写下要改什么（必填）"), { target: { value: "截图对不上" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "发送" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "C-1", "reject", "截图对不上", "frozen-v1");
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
      expect(answer).toHaveBeenCalledWith("C:/repo", `${kind}-1`, id, "", "frozen-v1");
      unmount();
    }
  });

  it("Q 改选后，藏起来的回答不发；可选意见可以收回，收回即清空", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByLabelText("以上都不合适，我自己写回答"));
    fireEvent.change(screen.getByPlaceholderText("写下你的回答（必填）"), { target: { value: "按周统计" } });
    fireEvent.click(screen.getByLabelText(/方案一/));
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "附加意见" }));
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "随便写写" } });
    fireEvent.click(screen.getByRole("button", { name: "不写意见了" }));
    expect(screen.queryByRole("textbox")).toBeNull();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "Q-1", "1", "", "frozen-v1");
  });

  it("一步提交的按钮在请求中显示「提交中…」", async () => {
    let resolve = () => {};
    answer.mockReturnValue(new Promise<void>((r) => { resolve = r; }));
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "验收通过" }));
    expect(screen.getByRole("button", { name: "提交中…" })).toBeTruthy();
    await act(async () => { resolve(); });
  });

  it("旧 C 不允许无范围签署，新 C 展示冻结条目和版本", () => {
    const { unmount } = render(<LeadGateCard cwd="C:/repo" gate={gate("C", { review: undefined })} onAnswered={() => {}} />);
    expect((screen.getByRole("button", { name: "验收通过" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/旧验收卡没有绑定清单和版本/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "验收通过" }));
    expect(answer).not.toHaveBeenCalled();
    unmount();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    expect(screen.getByText("验收版本：build-1")).toBeTruthy();
    const scope = screen.getByRole("list", { name: "本次签署的验收项" });
    expect(scope.textContent).toContain("确认业务范围");
    expect(scope.parentElement?.classList.contains("is-expanded")).toBe(true);
  });

  it("C：「验收通过」一步提交", async () => {
    answer.mockResolvedValue();
    render(<LeadGateCard cwd="C:/repo" gate={gate("C")} onAnswered={() => {}} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "验收通过" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "C-1", "accept", "", "frozen-v1");
  });
});

describe("关卡卡片：焦点与过时的卡", () => {
  /// 走查 C-3（p2-lead-fail / p2-lead-ok：提交后 activeElement=BODY）：提交在途用 aria-disabled，焦点留在
  /// 提交钮上；失败可原地重按。成功后卡片随轮询收起，焦点经 returnFocusTo 交回输入框（同审批卡）。
  it("提交在途不失焦；失败后焦点仍在提交钮上，可以重按", async () => {
    let reject!: (reason: unknown) => void;
    answer.mockImplementationOnce(() => new Promise<void>((_, no) => { reject = no; }));
    render(<LeadGateCard cwd="C:/repo" gate={gate("B")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByLabelText(/方案一/));
    const submit = screen.getByRole("button", { name: "提交" }) as HTMLButtonElement;
    submit.focus();
    await act(async () => { fireEvent.click(submit); });
    const busy = screen.getByRole("button", { name: zh.chat.leadGateSending }) as HTMLButtonElement;
    expect(busy.disabled).toBe(false);
    expect(busy.getAttribute("aria-disabled")).toBe("true");
    expect(document.activeElement).toBe(busy);
    fireEvent.click(busy);
    expect(answer).toHaveBeenCalledTimes(1);
    await act(async () => { reject("lead/gate_invalid"); });
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "提交" }));
  });

  it("提交成功、卡片收起后焦点回输入框", async () => {
    answer.mockResolvedValue();
    const composer = document.createElement("textarea");
    document.body.appendChild(composer);
    const ref = { current: composer };
    const { unmount } = render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={() => {}} returnFocusTo={ref} />);
    // 合 main（#169）后 A 类关卡是一步提交的动作按钮，不再是单选加「提交」。
    const submit = screen.getByRole("button", { name: zh.chat.leadGateConfirm });
    submit.focus();
    await act(async () => { fireEvent.click(submit); });
    unmount();
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
    expect(document.activeElement).toBe(composer);
    composer.remove();
  });

  /// 走查 C-11：gate_bad_choice 原先只说「请刷新后重选」，对话窗里没有刷新入口。现在卡片就地重读关卡
  /// （onStale），重读后已不在新卡上的选项被清掉，文案说「已重新读取」。gate_changed / missing / closed 同口径。
  it("选项与关卡对不上：就地重读，旧选择作废，文案不叫人刷新", async () => {
    answer.mockRejectedValueOnce("lead/gate_bad_choice");
    const onStale = vi.fn();
    const onAnswered = vi.fn();
    const { rerender } = render(<LeadGateCard cwd="C:/repo" gate={gate("B")} onAnswered={onAnswered} onStale={onStale} />);
    fireEvent.click(screen.getByLabelText(/方案二/));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "提交" })); });
    expect(onStale).toHaveBeenCalledTimes(1);
    expect(onAnswered).not.toHaveBeenCalled();
    const alert = (await screen.findByRole("alert")).textContent ?? "";
    expect(alert).toContain("已重新读取");
    expect(alert).not.toMatch(/刷新/);
    // 重读回来的关卡只剩方案一：选中的方案二作废，提交钮回到不可用。
    rerender(<LeadGateCard cwd="C:/repo" gate={gate("B", { options: [{ id: "1", label: "方案一", detail: "", recommended: true }] })} onAnswered={onAnswered} onStale={onStale} />);
    expect((screen.getByRole("button", { name: "提交" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("与卡片过时无关的报错不重读", async () => {
    answer.mockRejectedValueOnce("lead/gate_invalid");
    const onStale = vi.fn();
    render(<LeadGateCard cwd="C:/repo" gate={gate("A")} onAnswered={() => {}} onStale={onStale} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: zh.chat.leadGateConfirm })); });
    await screen.findByRole("alert");
    expect(onStale).not.toHaveBeenCalled();
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
    fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 2" }));
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
    expect(answer).toHaveBeenCalledWith("C:/repo", "D-1", "done", "", "frozen-v1");
    expect(onAnswered).toHaveBeenCalled();
  });

  it("关卡 D「不允许」意见可空，直接发送", async () => {
    answer.mockResolvedValue(undefined);
    render(<LeadGateCard cwd="C:/repo" gate={gate("D")} onAnswered={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "不允许" }));
    expect(answer).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText("附加意见（可选）")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "发送" })); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "D-1", "deny", "", "frozen-v1");
  });

  it("关卡 Q 用 agent 写的选项", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q")} onAnswered={() => {}} />);
    expect(screen.getByText("回答问题 · 选机制")).toBeTruthy();
    expect(screen.getByLabelText(/方案一/)).toBeTruthy(); // 标签里还含「推荐」和说明
  });

  it("关卡 Q 没有选项时也能选「其他」只写意见作答，意见必填", async () => {
    answer.mockResolvedValue(undefined);
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q", { options: [] })} onAnswered={() => {}} />);
    fireEvent.click(screen.getByLabelText("以上都不合适，我自己写回答"));
    const submit = screen.getByRole("button", { name: "提交" }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.change(screen.getByPlaceholderText("写下你的回答（必填）"), { target: { value: "按周统计" } });
    await act(async () => { fireEvent.click(submit); });
    expect(answer).toHaveBeenCalledWith("C:/repo", "Q-1", "other", "按周统计", "frozen-v1");
  });

  it("agent 自己写了 id 为 other 的选项时不再追加「其他」", () => {
    render(<LeadGateCard cwd="C:/repo" gate={gate("Q", { options: [{ id: "other", label: "别的城市", detail: "", recommended: false }] })} onAnswered={() => {}} />);
    expect(screen.getAllByRole("radio")).toHaveLength(1);
    expect(screen.queryByLabelText("以上都不合适，我自己写回答")).toBeNull();
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
    fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 2" }));
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

  it("胶囊上也报待实测个数；要 leader 验收的条目以 leader 为准，不算待实测", async () => {
    const checks = [
      { id: "1", check: "a", passes: true, waived: null, leader: false, blocked: null },
      { id: "2", check: "等 beta", passes: false, waived: null, leader: false, blocked: "要 beta 开机" },
      { id: "3", check: "看体验", passes: false, waived: null, leader: true, blocked: "要真机" },
    ];
    snapshot.mockResolvedValue(task({ checks }));
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    const pill = await screen.findByRole("button", { name: /Lead.*1\/3.*1 待实测/ });
    fireEvent.click(pill);
    expect(screen.getByText("待你验收")).toBeTruthy();
    expect(screen.queryByText("待实测：要真机")).toBeNull();
    expect(document.querySelector(".lead-blocked-count")?.textContent).toBe("1 条待实测");
  });

  it("没有待实测的条目时不显示个数", () => {
    const { container } = render(<LeadPanel task={task()} onClose={() => {}} />);
    expect(container.querySelector(".lead-blocked-count")).toBeNull();
  });
});


it("切换任务不继承完成项展开状态和文档页签", () => {
  const { rerender } = render(<LeadPanel task={task()} onClose={() => {}} />);
  fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 1" }));
  expect(screen.getByText("测试全过")).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "规格" }));
  rerender(<LeadPanel task={task({ slug: "another-task" })} onClose={() => {}} />);
  expect(screen.getByRole("tab", { name: "验收" }).getAttribute("aria-selected")).toBe("true");
  expect(screen.queryByText("测试全过")).toBeNull();
});

it("最后一项完成后仍保持折叠，完成组可反复展开收起", () => {
  const { rerender } = render(<LeadPanel task={task()} onClose={() => {}} />);
  rerender(<LeadPanel task={task({ checks: task().checks.map((check) => ({ ...check, passes: true })) })} onClose={() => {}} />);
  const toggle = screen.getByRole("button", { name: "已通过与豁免 · 2" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText("测试全过")).toBeNull();
  fireEvent.click(toggle);
  expect(screen.getByText("测试全过")).toBeTruthy();
  fireEvent.click(toggle);
  expect(screen.queryByText("测试全过")).toBeNull();
});

it("首次打开全完成任务保留进度摘要与展开入口", () => {
  render(<LeadPanel task={task({ checks: task().checks.map((check) => ({ ...check, passes: true })) })} onClose={() => {}} />);
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("2");
  fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 2" }));
  expect(screen.getByText("手机能用")).toBeTruthy();
});

describe("Lead 快照异常与会话隔离", () => {
  it("首次失败显示入口，点击重试后可确认没有任务", async () => {
    snapshot.mockRejectedValueOnce(new Error("offline"));
    render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId="cc-1" items={[]} history={history} loading={false} />);
    const retry = await screen.findByRole("button", { name: /Lead.*数据待更新/ });
    snapshot.mockResolvedValue(null);
    fireEvent.click(retry);
    await waitFor(() => expect(screen.queryByRole("button", { name: /Lead/ })).toBeNull());
    expect(snapshot).toHaveBeenCalledTimes(2);
  });

  it("读失败保留上次内容并提示，恢复后清除提示", async () => {
    snapshot.mockResolvedValue(task());
    const { result } = renderHook(() => useLeadTask("C:/repo", "s1"));
    await waitFor(() => expect(result.current[0]?.slug).toBe("20261109-demo"));
    snapshot.mockRejectedValueOnce(new Error("offline"));
    await act(async () => result.current[1]());
    expect(result.current[0]?.checks).toHaveLength(2);
    expect(result.current[0]?.issues).toContain("lead/snapshot_stale");
    await act(async () => result.current[1]());
    expect(result.current[0]?.issues ?? []).toEqual([]);
  });

  it("坏清单保留同任务上次内容，切换会话不保留旧任务", async () => {
    snapshot.mockResolvedValue(task());
    const { result, rerender } = renderHook(({ session }) => useLeadTask("C:/repo", session), { initialProps: { session: "s1" } });
    await waitFor(() => expect(result.current[0]?.checks).toHaveLength(2));
    snapshot.mockResolvedValueOnce(task({ checks: [], issues: ["lead/acceptance_invalid"] }));
    await act(async () => result.current[1]());
    expect(result.current[0]?.checks).toHaveLength(2);
    expect(result.current[0]?.issues).toContain("lead/acceptance_invalid");
    snapshot.mockRejectedValue(new Error("offline"));
    await act(async () => rerender({ session: "s2" }));
    expect(result.current[0]).toBeNull();
  });
});

it("验收项展开后显示对应版本的证据，不给旧条目造证据", () => {
  const evidence = { command: "cargo test scoped", result: "3 passed", revision: "build-proof", reference: "evidence/test.log", limitations: "macOS 未验证" };
  render(<LeadPanel task={task({ checks: [{ id: "proof", check: "验证冻结签署", passes: true, leader: false, waived: null, blocked: null, evidence: [evidence] }] })} onClose={() => {}} />);
  expect(screen.queryByText(/cargo test scoped/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "已通过与豁免 · 1" }));
  fireEvent.click(screen.getByRole("button", { name: /验证冻结签署/ }));
  expect(screen.getByText("cargo test scoped")).toBeTruthy();
  expect(screen.getByText("cargo test scoped").closest("button")).toBeNull();
  expect(screen.getByText("验收版本：build-proof")).toBeTruthy();
  expect(screen.getByText("evidence/test.log")).toBeTruthy();
  expect(screen.getByText("macOS 未验证")).toBeTruthy();
});

it("决策按决定卡展示、最新在上，进度不再平铺 phase 行，内容不丢", async () => {
  render(<LeadPanel task={task({
    decisions: "- D-17（publication-2）：采用显式发布，保留旧答复。\n- D-18：继续验证。",
    progress: "phase: verify\n- 完成第一轮验证\n- 修复后复验通过",
  })} onClose={() => {}} />);
  fireEvent.click(screen.getByRole("tab", { name: "决策" }));
  // 单行旧格式：编号、括号说明、正文各归各位；文件末尾的 D-18 是最新的，排最上。
  const cards = [...document.querySelectorAll(".lead-decision")];
  expect(cards.map((card) => card.querySelector(".lead-decision-id")?.textContent)).toEqual(["D-18", "D-17"]);
  expect(cards[1].querySelector(".lead-decision-meta")?.textContent).toBe("publication-2");
  expect(screen.getByText(visibleText("采用显式发布，保留旧答复。"))).toBeTruthy();
  expect(screen.getByText(visibleText("继续验证。"))).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "进度" }));
  // 当前阶段由顶部阶段条显示（二者同源），时间线里不再重复一行「phase: verify」。
  expect(document.querySelector(".lead-notes.is-progress")?.textContent).not.toContain("phase:");
  expect(screen.getByText(visibleText("修复后复验通过"))).toBeTruthy();
});

it("历史缺少会话 id 时只查询并展示后端允许的未认领任务", async () => {
  snapshot.mockResolvedValue(task());
  render(<DockWork sessionId={1} cwd="C:/repo" ccSessionId={null} items={[]} history={history} loading={false} />);
  await waitFor(() => expect(snapshot).toHaveBeenCalledWith("C:/repo", undefined));
  expect(await screen.findByRole("button", { name: /Lead.*1\/2/ })).toBeTruthy();
});

it("验收显示稳定编号，旧任务的决定表格拆成卡，进度使用时间轴", () => {
  render(<LeadPanel task={task({
    checks: [
      { id: "L17", check: "恢复未认领任务入口", passes: false, leader: false, blocked: null, waived: null },
      { id: "L18", check: "移动端表格", passes: true, leader: false, blocked: null, waived: null },
    ],
    decisions: "- D-20：保留原始表格\n\n| 日期 | 决策 | 理由 |\n| --- | --- | --- |\n| 11-17 | 时间轴 | 保持顺序 |",
    progress: "phase: verify\n## 2026-11-17 第一节\n- 第一条记录\n\n## 2026-11-18 第二节\n- 第二条记录",
  })} onClose={() => {}} />);
  expect(document.querySelector(".lead-check-id")?.textContent).toBe("L17");
  fireEvent.click(screen.getByRole("tab", { name: "决策" }));
  // 表头有「决策」列的表格每行一张卡：标题取决策列，其余列成要点，原文不丢。
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.getByRole("heading", { name: "时间轴" })).toBeTruthy();
  expect(screen.getByText(visibleText("理由：保持顺序"))).toBeTruthy();
  fireEvent.click(screen.getByRole("tab", { name: "进度" }));
  expect(document.querySelectorAll(".lead-timeline .lead-timeline-marker")).toHaveLength(2);
  expect(document.querySelectorAll(".lead-timeline .lead-entry")[0].textContent).toContain("第二节");
});
