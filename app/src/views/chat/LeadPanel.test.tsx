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
    expect(onAnswered).not.toHaveBeenCalled();
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
