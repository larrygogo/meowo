import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const apply = vi.fn();
const live = vi.fn();
const record = vi.fn();
// 宿主侧的排队（session_settings 的 QUEUED）在这里用一张表模拟：按会话记、合并、取走即清。
const queue = new Map<number, Record<string, unknown>>();
// 宿主执行结果事件（session-settings-applied）：测试里直接调监听函数模拟宿主发事件。
const hostEvents = vi.hoisted(() => ({ handler: null as null | ((event: { payload: { sessionId: number; kind: string; reason: string | null } }) => void) }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, handler: typeof hostEvents.handler) => { hostEvents.handler = handler; return () => {}; },
}));
vi.mock("../../api", () => ({
  applySessionSettings: (...args: unknown[]) => apply(...args),
  sessionLiveSettings: (...args: unknown[]) => live(...args),
  recordSessionSettings: (...args: unknown[]) => record(...args),
  queueSessionSettings: async (sessionId: number, request: { modes?: Record<string, string> } & Record<string, unknown>) => {
    const prev = queue.get(sessionId) ?? {};
    const merged = { ...prev, ...request, modes: { ...((prev.modes as Record<string, string>) ?? {}), ...(request.modes ?? {}) } };
    queue.set(sessionId, merged);
    return merged;
  },
  queuedSessionSettings: async (sessionId: number) => queue.get(sessionId) ?? null,
  takeQueuedSessionSettings: async (sessionId: number) => {
    const value = queue.get(sessionId) ?? null;
    if (value?.waitFor) return null; // 宿主标了等待条件的不给前端
    queue.delete(sessionId);
    return value;
  },
}));

import { mergeRequests, mustDefer, useSessionSettings, type SessionSettingsDeps } from "./useSessionSettings";
import type { ChatHistory } from "../../api";

const history = { agentModes: [{ dimension: "permission", value: "manual" }], model: "K3" } as unknown as ChatHistory;

function deps(overrides: Partial<SessionSettingsDeps> = {}): SessionSettingsDeps {
  return {
    sessionId: 7,
    tone: "waiting",
    history,
    setHistory: vi.fn(),
    setSendError: vi.fn(),
    moveTo: vi.fn(),
    rescueDraft: async () => "empty",
    restarts: () => true,
    readCurrent: async () => ({ permission: "manual", model: "kimi-code/k3" }),
    firstTurnPending: false,
    fallbackModel: vi.fn(),
    fallbackMode: () => false,
    modelLabel: (id: string) => id,
    ...overrides,
  };
}

describe("会话里切模型 / 模式的分派", () => {
  beforeEach(() => {
    apply.mockReset();
    live.mockReset();
    record.mockReset();
    // 真实接口一律返回 Promise（做空 N8：reset 后返回 undefined 曾在实现里 .catch 出 Unhandled Rejection）。
    live.mockResolvedValue({ model: null, effort: null, modes: [] });
    record.mockResolvedValue(undefined);
    queue.clear();
  });

  /// 做空 N8：后端调用同步抛错或返回 undefined 时不留未处理的 rejection，流程照常走完。
  it("后端返回 undefined 或同步抛错不留 Unhandled Rejection", async () => {
    live.mockReturnValue(undefined);
    record.mockImplementation(() => { throw new Error("boom"); });
    let current = deps({ tone: "running" });
    const { rerender } = renderHook(() => useSessionSettings(current));
    current = deps({ tone: "waiting" });
    rerender();
    await waitFor(() => expect(live).toHaveBeenCalled());
    await waitFor(() => expect(record).toHaveBeenCalled());
  });

  /// 做空 N9：不重启的路线只读不搬草稿；有字时如实报给后端（对话框路线见字不动手）。
  it("不重启的路线不搬草稿", async () => {
    apply.mockResolvedValue({ kind: "not_now", sessionId: 7, confirmed: false, reason: "lead/opencode_composer_not_empty" });
    const rescueDraft = vi.fn(async (move: boolean) => (move ? "moved" as const : "occupied" as const));
    const { result } = renderHook(() => useSessionSettings(deps({ rescueDraft, restarts: () => false })));
    await act(async () => result.current.request({ modes: { agent: "plan" }, label: "计划" }));
    expect(rescueDraft).toHaveBeenCalledWith(false);
    expect(apply.mock.calls[0][0].tui.composerEmpty).toBe(false);
  });

  /// 做空 N3 / N6：宿主收进队列（Lead 在等关卡答复 / 会话没在运行），提示说等的是什么；前端不去取走执行。
  it("宿主收进队列时说清等什么，前端不抢着执行", async () => {
    // 宿主收进队列的条目没有提示名（apply 不带 label），前端随后合并进来。
    apply.mockImplementation(async (args: { sessionId: number }) => {
      queue.set(args.sessionId, { model: { id: "fake/b", effort: null }, modes: {}, waitFor: "lead" });
      return { kind: "queued", sessionId: 7, confirmed: false, reason: "settings/lead_waiting" };
    });
    let current = deps();
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    await act(async () => result.current.request({ model: { id: "fake/b", label: "Fake B" }, label: "Fake B" }));
    await waitFor(() => expect(result.current.notice).toBe("关卡答复后那一轮跑完再切换到「Fake B」（关闭应用会取消）"));
    current = deps({ tone: "running" });
    rerender();
    current = deps({ tone: "waiting" });
    rerender();
    expect(apply).toHaveBeenCalledTimes(1);
    expect(queue.get(7)).toMatchObject({ label: "Fake B", waitFor: "lead" });
    queue.set(7, { label: "GPT-6", waitFor: "open" });
    const reopened = renderHook(() => useSessionSettings(deps()));
    await waitFor(() => expect(reopened.result.current.notice).toBe("打开会话后切换到「GPT-6」（关闭应用会取消）"));
  });

  /// 搬了草稿最后没重启（宿主排进队列 / 被拒）：撤回对话页那份，TUI 原稿还在；真重启了不撤。
  it("搬了草稿却没重启时撤回", async () => {
    const undoRescue = vi.fn();
    const rescueDraft = async () => "moved" as const;
    apply.mockImplementation(async () => {
      queue.set(7, { modes: {}, waitFor: "lead" });
      return { kind: "queued", sessionId: 7, confirmed: false, reason: "settings/lead_waiting" };
    });
    const first = renderHook(() => useSessionSettings(deps({ rescueDraft, undoRescue })));
    await act(async () => first.result.current.request({ model: { id: "fake/b", label: "Fake B" }, label: "Fake B" }));
    expect(undoRescue).toHaveBeenCalledTimes(1);
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: false, reason: null });
    const second = renderHook(() => useSessionSettings(deps({ rescueDraft, undoRescue })));
    await act(async () => second.result.current.request({ model: { id: "fake/c", label: "Fake C" }, label: "Fake C" }));
    expect(undoRescue).toHaveBeenCalledTimes(1);
  });

  /// 做空 R3 回归（原样移植 redteam-3 的 vt/undo.test.ts，期望反过来）：进程可能已被杀的失败路径上，
  /// 对话页那份草稿必须留着——TUI 里的原稿已经随进程没了。rescueDraft / undoRescue 照抄 ChatWindow.tsx。
  describe("重启失败时保住草稿", () => {
    function run(outcome: () => Promise<unknown>) {
      let prompt = "";
      let rescued: string | null = null;
      const tuiDraft = "这段只在 TUI 输入框里";
      apply.mockImplementation(outcome);
      const d = deps({
        rescueDraft: async (move: boolean) => {
          if (!move) return "occupied" as const;
          rescued = tuiDraft;
          prompt = prompt.trim() ? prompt + "\n" + tuiDraft : tuiDraft;
          return "moved" as const;
        },
        undoRescue: () => {
          const text = rescued;
          rescued = null;
          if (!text) return;
          prompt = prompt === text ? "" : prompt.endsWith("\n" + text) ? prompt.slice(0, -(text.length + 1)) : prompt;
        },
      });
      return { d, get prompt() { return prompt; } };
    }
    it("restart_stuck（stop 之后没收掉）：草稿留着", async () => {
      const r = run(async () => { throw "terminal/restart_stuck"; });
      const { result } = renderHook(() => useSessionSettings(r.d));
      await act(async () => result.current.request({ model: { id: "kimi-code/k2", label: "K2" }, label: "K2" }));
      expect(r.prompt).toBe("这段只在 TUI 输入框里");
    });
    it("stop 之后恰好退出被记成 next_open：草稿留着", async () => {
      const r = run(async () => ({ kind: "next_open", sessionId: 7, confirmed: false, reason: null }));
      const { result } = renderHook(() => useSessionSettings(r.d));
      await act(async () => result.current.request({ model: { id: "kimi-code/k2", label: "K2" }, label: "K2" }));
      expect(r.prompt).toBe("这段只在 TUI 输入框里");
    });
    it("新进程起来马上退出（后端已报 restarted，之后进程没了）：草稿留着", async () => {
      // 真机第四轮 R3 补测：后端只看得到「起来了」，紧接着的退出由 PTY 退出事件另报；对话页这份是唯一副本。
      const r = run(async () => ({ kind: "restarted", sessionId: 7, confirmed: false, reason: null }));
      const { result } = renderHook(() => useSessionSettings(r.d));
      await act(async () => result.current.request({ model: { id: "kimi-code/k2", label: "K2" }, label: "K2" }));
      expect(r.prompt).toBe("这段只在 TUI 输入框里");
    });
    it("起新进程前报错（项目目录不存在 / 未安装）：草稿留着", async () => {
      const r = run(async () => { throw "项目目录已不存在：C:/gone。目录被移动或删除后无法在原位置恢复会话"; });
      const { result } = renderHook(() => useSessionSettings(r.d));
      await act(async () => result.current.request({ model: { id: "kimi-code/k2", label: "K2" }, label: "K2" }));
      expect(r.prompt).toBe("这段只在 TUI 输入框里");
    });
    it("后端明确没动进程（not_now）：撤回对话页那份", async () => {
      const r = run(async () => ({ kind: "not_now", sessionId: 7, confirmed: false, reason: "settings/composer_not_empty" }));
      const { result } = renderHook(() => useSessionSettings(r.d));
      await act(async () => result.current.request({ model: { id: "kimi-code/k2", label: "K2" }, label: "K2" }));
      expect(r.prompt).toBe("");
    });
  });

  /// 做空 R5：对话页执行时服务没就绪（刚打开的 Codex），放回宿主队列等就绪，不丢、不报错，前端也不再抢。
  /// SPEC 终态 2（第四轮补）：Kimi 工作模式这类要往终端打字的切换，运行中和模型一样排到这一轮之后、
  /// 同一套提示；回合结束由对话页照原样执行（不走后端），不再报「请等待当前任务结束后切换」。
  it("运行中切终端项排到这一轮之后，回合结束照原样执行", async () => {
    const applyTerminal = vi.fn(async () => {});
    const action = { kind: "mode" as const, dimension: "work", value: "plan", inputs: [{ data: "/plan on", submit: true }], cycleInput: null, markers: [] };
    let current = deps({ tone: "running", applyTerminal });
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    act(() => result.current.request({ modes: { work: "plan" }, label: "计划：开", terminal: [action] }));
    await waitFor(() => expect(result.current.notice).toBe("这一轮结束后切换到「计划：开」（关闭应用会取消）"));
    expect(applyTerminal).not.toHaveBeenCalled();
    current = deps({ tone: "waiting", applyTerminal });
    rerender();
    await waitFor(() => expect(applyTerminal).toHaveBeenCalledWith(action));
    expect(apply).not.toHaveBeenCalled();
    await waitFor(() => expect(result.current.notice).toBeNull());
  });

  /// 真机第四轮问题 3：OpenCode 只切 agent，后端当场读回模型，界面立刻显示并钉住。
  it("只切 agent 后显示读回的模型", async () => {
    const setHistory = vi.fn();
    apply.mockResolvedValue({ kind: "applied", sessionId: 7, confirmed: true, reason: null, model: "Fake One" });
    const { result } = renderHook(() => useSessionSettings(deps({ setHistory, restarts: () => false })));
    await act(async () => result.current.request({ modes: { agent: "plan" }, label: "计划" }));
    const update = setHistory.mock.calls.at(-1)![0] as (h: unknown) => { model: string };
    expect(update({ sessionId: 7, model: "Fake Two", agentModes: [] }).model).toBe("Fake One");
    // 最终做空 P2：接着打开菜单（syncLive）。宿主 live 已按钉子给出读回的模型，显示不退回旧值，
    // 写回存档的也是它。
    live.mockResolvedValue({ model: "fake/fake-model", effort: null, modes: [{ dimension: "agent", value: "plan" }] });
    setHistory.mockClear();
    await act(async () => result.current.syncLive());
    const synced = setHistory.mock.calls.at(0)![0] as (h: unknown) => { model: string };
    expect(synced({ sessionId: 7, model: "Fake Two", agentModes: [] }).model).toBe("fake/fake-model");
    await waitFor(() => expect(record).toHaveBeenCalledWith(7, "fake/fake-model", expect.objectContaining({ agent: "plan" })));
  });

  /// leader 2026-11-14：混合排队宿主先做经后端的部分（模型），终端项留给对话页；拆分后提示仍是同一句。
  it("混合排队宿主做完模型后，剩下的终端项提示仍是同一句", async () => {
    const action = { kind: "mode" as const, dimension: "work", value: "plan", inputs: [{ data: "/plan on", submit: true }], cycleInput: null, markers: [] };
    const applyTerminal = vi.fn(async () => {});
    let current = deps({ tone: "running", applyTerminal });
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    await waitFor(() => expect(hostEvents.handler).not.toBeNull());
    act(() => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    act(() => result.current.request({ modes: { work: "plan" }, label: "计划：开", terminal: [action] }));
    await waitFor(() => expect(result.current.notice).toBe("这一轮结束后切换到「计划：开」（关闭应用会取消）"));
    // 宿主在回合结束时做掉了模型（restarted），把终端项放回队列并发 queued。
    queue.set(7, { label: "计划：开", modes: { work: "plan" }, terminal: [action] });
    act(() => hostEvents.handler!({ payload: { sessionId: 7, kind: "restarted", reason: null } }));
    act(() => hostEvents.handler!({ payload: { sessionId: 7, kind: "queued", reason: null } }));
    await waitFor(() => expect(result.current.notice).toBe("这一轮结束后切换到「计划：开」（关闭应用会取消）"));
    // 回合结束：对话页照原样执行终端项，不再走后端。
    current = deps({ tone: "waiting", applyTerminal });
    rerender();
    await waitFor(() => expect(applyTerminal).toHaveBeenCalledWith(action));
    expect(apply).not.toHaveBeenCalled();
  });

  /// 最终做空 P6：对话页执行混合条目与宿主同一顺序——先后端（重启 / 服务 / 模型），再终端项；后端那部分
  /// 又排进队列时终端项跟着等，不抢先打字。
  it("混合条目先后端、再终端项；后端排队时终端项一起等", async () => {
    const order: string[] = [];
    const action = { kind: "mode" as const, dimension: "work", value: "plan", inputs: [{ data: "/plan on", submit: true }], cycleInput: null, markers: [] };
    const applyTerminal = vi.fn(async () => { order.push("terminal"); });
    apply.mockImplementation(async () => { order.push("backend"); return { kind: "restarted", sessionId: 7, confirmed: false, reason: null }; });
    const { result } = renderHook(() => useSessionSettings(deps({ applyTerminal })));
    await act(async () => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, modes: { work: "plan" }, label: "计划：开", terminal: [action] }));
    await waitFor(() => expect(order).toEqual(["backend", "terminal"]));
    expect(apply.mock.calls[0][0].modes).toEqual({});
    // 后端被宿主收进队列（Lead 在等）：终端项不执行，跟着排队。
    order.length = 0;
    apply.mockImplementation(async (args: { sessionId: number }) => {
      order.push("backend");
      queue.set(args.sessionId, { modes: {}, model: { id: "kimi-code/k3" }, waitFor: "lead" });
      return { kind: "queued", sessionId: 7, confirmed: false, reason: "settings/lead_waiting" };
    });
    await act(async () => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, modes: { work: "plan" }, label: "计划：开", terminal: [action] }));
    await waitFor(() => expect(queue.get(7)).toMatchObject({ waitFor: "lead", terminal: [action], modes: { work: "plan" } }));
    expect(order).toEqual(["backend"]);
  });

  /// 第六轮 L2：前端按「可能重启」先搬了草稿，后端走会话服务没重启（applied）——进程没动，撤回对话页那份。
  it("预搬草稿后后端没重启（applied）时撤回", async () => {
    const undoRescue = vi.fn();
    apply.mockResolvedValue({ kind: "applied", sessionId: 7, confirmed: true, reason: null });
    const { result } = renderHook(() => useSessionSettings(deps({ rescueDraft: async () => "moved" as const, undoRescue })));
    await act(async () => result.current.request({ modes: { approval: "ask" }, label: "必要时询问" }));
    expect(undoRescue).toHaveBeenCalledTimes(1);
  });

  /// 第六轮 L3：混合条目后端那部分失败，终端项不执行，不只生效一半。
  it("混合条目后端失败时终端项不执行", async () => {
    const applyTerminal = vi.fn(async () => {});
    const setSendError = vi.fn();
    const action = { kind: "mode" as const, dimension: "work", value: "plan", inputs: [{ data: "/plan on", submit: true }], cycleInput: null, markers: [] };
    apply.mockImplementation(async () => { throw "terminal/restart_stuck"; });
    const { result } = renderHook(() => useSessionSettings(deps({ applyTerminal, setSendError })));
    await act(async () => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, modes: { work: "plan" }, label: "计划：开", terminal: [action] }));
    await waitFor(() => expect(setSendError).toHaveBeenCalled());
    expect(applyTerminal).not.toHaveBeenCalled();
    expect(queue.has(7)).toBe(false);
  });

  /// 第五轮 P3 低危：Codex 为沙箱带参重开会话时说明原因。
  it("为沙箱重开会话时提示原因", async () => {
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: true, reason: "settings/sandbox_reopen" });
    const { result } = renderHook(() => useSessionSettings(deps({ restarts: () => true })));
    await act(async () => result.current.request({ modes: { approval: "ask" }, label: "必要时询问" }));
    await waitFor(() => expect(result.current.notice).toBe("已带沙箱重开会话，切到「必要时询问」"));
  });

  /// 第七轮 R3：用户就在这个会话里，只含终端项的排队显示通常的「这一轮结束后切换」（离开会话时才由灵动岛
  /// 提醒「打开这个会话后切换」），回合结束空闲时照原样执行。
  it("会话里只含终端项的排队显示通常提示，空闲时执行", async () => {
    const action = { kind: "mode", dimension: "work", value: "plan", inputs: [{ data: "/plan on", submit: true }] };
    queue.set(7, { label: "计划：开", modes: { work: "plan" }, terminal: [action] });
    const applyTerminal = vi.fn(async () => {});
    let current = deps({ tone: "running", applyTerminal });
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    await waitFor(() => expect(result.current.notice).toBe("这一轮结束后切换到「计划：开」（关闭应用会取消）"));
    current = deps({ tone: "waiting", applyTerminal });
    rerender();
    await waitFor(() => expect(applyTerminal).toHaveBeenCalledWith(action));
  });

  it("服务没就绪时交给宿主等就绪", async () => {
    apply.mockImplementation(async () => { throw "settings/server_not_ready"; });
    const setSendError = vi.fn();
    const { result } = renderHook(() => useSessionSettings(deps({ setSendError, restarts: () => false })));
    await act(async () => result.current.request({ model: { id: "gpt-6", label: "GPT-6" }, label: "GPT-6" }));
    expect(queue.get(7)).toMatchObject({ label: "GPT-6", waitFor: "ready" });
    expect(setSendError).not.toHaveBeenCalled();
    await waitFor(() => expect(result.current.notice).toBe("会话就绪后切换到「GPT-6」（关闭应用会取消）"));
    expect(apply).toHaveBeenCalledTimes(1);
  });

  it("在跑或有待批审批时不调后端（不写终端、不重启），回合结束再执行", async () => {
    expect(mustDefer("running")).toBe(true);
    expect(mustDefer("pending")).toBe(true);
    expect(mustDefer("waiting")).toBe(false);
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: false, reason: null });
    let current = deps({ tone: "running" });
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    await act(async () => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    // 运行中又选了一档权限：两次合并，只执行一次。
    await act(async () => result.current.request({ modes: { permission: "yolo" }, label: "必要时询问" }));
    expect(apply).not.toHaveBeenCalled();
    expect(result.current.notice).toBe("这一轮结束后切换到「必要时询问」（关闭应用会取消）");
    current = deps({ tone: "pending" });
    rerender();
    expect(apply).not.toHaveBeenCalled();
    current = deps({ tone: "waiting" });
    rerender();
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
    // 带上会话当前实际值，后端按它拼参数（做空 H1）。
    expect(apply.mock.calls[0][0]).toMatchObject({
      sessionId: 7,
      model: "kimi-code/k3",
      modes: { permission: "yolo" },
      current: { permission: "manual", model: "kimi-code/k3" },
      tui: { idle: true, composerEmpty: true, approvalPending: false },
    });
    await waitFor(() => expect(result.current.notice).toBeNull());
  });

  /// 做空 M3：排队存在宿主里、按会话记——换会话再回来（对话窗重挂）照样显示并在空闲时执行。
  it("排队的切换换会话后还在", async () => {
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: false, reason: null });
    let current = deps({ tone: "running" });
    const first = renderHook(() => useSessionSettings(current));
    await act(async () => first.result.current.request({ modes: { permission: "auto" }, label: "都不问" }));
    first.unmount();
    current = deps({ tone: "running" });
    const again = renderHook(() => useSessionSettings(current));
    await waitFor(() => expect(again.result.current.notice).toBe("这一轮结束后切换到「都不问」（关闭应用会取消）"));
    current = deps({ tone: "waiting" });
    again.rerender();
    await waitFor(() => expect(apply).toHaveBeenCalledWith(expect.objectContaining({ modes: { permission: "auto" } })));
  });

  /// 做空 H2：重启前 TUI 输入框里的字先搬进对话页；认不出输入框就不重启、说原因。
  it("重启前保住 TUI 草稿，认不出输入框不重启", async () => {
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: false, reason: null });
    const rescueDraft = vi.fn(async () => "moved" as const);
    const moved = renderHook(() => useSessionSettings(deps({ rescueDraft })));
    await act(async () => moved.result.current.request({ model: { id: "gemini-2.5-pro", label: "gemini-2.5-pro" }, label: "gemini-2.5-pro" }));
    expect(rescueDraft).toHaveBeenCalled();
    expect(apply.mock.calls[0][0].tui.composerEmpty).toBe(true);
    apply.mockClear();
    const setSendError = vi.fn();
    const unknown = renderHook(() => useSessionSettings(deps({ rescueDraft: async () => "unknown", setSendError })));
    await act(async () => unknown.result.current.request({ modes: { permission: "yolo" }, label: "必要时询问" }));
    expect(apply).not.toHaveBeenCalled();
    expect(setSendError).toHaveBeenCalledWith("认不出终端输入框，为免丢字没有重启");
  });

  /// 做空 L4 / M4：未认领会话第一条消息已发出时不重开；会话没在跑时如实说「下次打开时生效」。
  it("第一轮在跑不重开，离线会话说下次打开生效", async () => {
    const setSendError = vi.fn();
    const pending = renderHook(() => useSessionSettings(deps({ sessionId: -3, firstTurnPending: true, setSendError })));
    await act(async () => pending.result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    expect(apply).not.toHaveBeenCalled();
    expect(setSendError).toHaveBeenCalledWith("第一条消息正在处理，会话建好后再切");
    apply.mockResolvedValueOnce({ kind: "next_open", sessionId: 7, confirmed: false, reason: null });
    const offline = renderHook(() => useSessionSettings(deps()));
    await act(async () => offline.result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    expect(offline.result.current.notice).toBe("会话没在运行，下次打开时生效");
  });

  it("未建会话重开后切到新的临时 id；收紧做不到时如实说；够不着服务时退回终端菜单", async () => {
    const moveTo = vi.fn();
    const setSendError = vi.fn();
    const fallbackModel = vi.fn();
    const current = deps({ sessionId: -3, moveTo, setSendError, fallbackModel });
    const { result } = renderHook(() => useSessionSettings(current));
    apply.mockResolvedValueOnce({ kind: "reopened", sessionId: -4, confirmed: false, reason: null });
    await act(async () => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    expect(moveTo).toHaveBeenCalledWith(-4);
    apply.mockResolvedValueOnce({ kind: "needs_new_session", sessionId: -3, confirmed: false, reason: null });
    await act(async () => result.current.request({ modes: { permission: "manual" }, label: "每步都问" }));
    expect(setSendError).toHaveBeenCalledWith("接回会话只能放宽权限，收紧要新建会话");
    apply.mockResolvedValueOnce({ kind: "unsupported", sessionId: -3, confirmed: false, reason: null });
    await act(async () => result.current.request({ model: { id: "gpt-5.5", label: "GPT-5.5" }, label: "GPT-5.5" }));
    expect(fallbackModel).toHaveBeenCalledWith("gpt-5.5");
  });

  /// OpenCode 够不着 TUI 服务时 agent 退回 Tab 循环；对话框路线回车后底栏没认出新值，说「下一条消息时核对」。
  it("模式兜底与未确认提示", async () => {
    const fallbackMode = vi.fn(() => true);
    const setSendError = vi.fn();
    const current = deps({ fallbackMode, setSendError, restarts: () => false });
    const { result } = renderHook(() => useSessionSettings(current));
    apply.mockResolvedValueOnce({ kind: "unsupported", sessionId: 7, confirmed: false, reason: null });
    await act(async () => result.current.request({ modes: { agent: "plan" }, label: "计划" }));
    expect(fallbackMode).toHaveBeenCalledWith("agent", "plan");
    expect(setSendError).not.toHaveBeenCalled();
    apply.mockResolvedValueOnce({ kind: "applied", sessionId: 7, confirmed: false, reason: null });
    await act(async () => result.current.request({ modes: { agent: "build" }, label: "必要时询问" }));
    expect(setSendError).toHaveBeenCalledWith("已切换，下一条消息时核对");
  });

  /// 回合结束（running → 空闲）读一次会话此刻的模型 / 模式并写回存档（做空 M5），不轮询。
  it("回合结束时同步当前模型与模式并写回存档", async () => {
    live.mockResolvedValue({ model: "gpt-5.5", effort: "high", modes: [{ dimension: "approval", value: "yolo" }] });
    let state: ChatHistory | null = { ...history, sessionId: 7, effort: null } as unknown as ChatHistory;
    const setHistory = vi.fn((update: ChatHistory | null | ((prev: ChatHistory | null) => ChatHistory | null)) => {
      state = typeof update === "function" ? update(state) : update;
    });
    const label = (id: string) => (id === "gpt-5.5" ? "GPT-5.5" : id);
    let current = deps({ tone: "running", setHistory, modelLabel: label, readCurrent: async () => ({}) });
    const { rerender } = renderHook(() => useSessionSettings(current));
    expect(live).not.toHaveBeenCalled();
    current = deps({ tone: "waiting", setHistory, modelLabel: label, readCurrent: async () => ({}) });
    rerender();
    await waitFor(() => expect(live).toHaveBeenCalledWith(7));
    await waitFor(() => expect(state?.model).toBe("GPT-5.5"));
    expect(state?.agentModes).toEqual([{ dimension: "permission", value: "manual" }, { dimension: "approval", value: "yolo" }]);
    await waitFor(() => expect(record).toHaveBeenCalledWith(7, "gpt-5.5", { approval: "yolo" }));
  });

  it("后来的选择覆盖先前的", () => {
    expect(mergeRequests({ modes: { a: "1", b: "1" }, label: "x" }, { modes: { b: "2" }, label: "y" })).toEqual({ model: undefined, modes: { a: "1", b: "2" }, label: "y" });
  });
});
