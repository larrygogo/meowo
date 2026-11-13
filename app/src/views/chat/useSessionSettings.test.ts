import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const apply = vi.fn();
const live = vi.fn();
vi.mock("../../api", () => ({
  applySessionSettings: (...args: unknown[]) => apply(...args),
  sessionLiveSettings: (...args: unknown[]) => live(...args),
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
    composerEmpty: async () => true,
    fallbackModel: vi.fn(),
    fallbackMode: () => false,
    modelLabel: (id: string) => id,
    ...overrides,
  };
}

describe("会话里切模型 / 模式的分派", () => {
  beforeEach(() => apply.mockReset());

  it("在跑或有待批审批时不调后端（不写终端、不重启），回合结束再执行", async () => {
    expect(mustDefer("running")).toBe(true);
    expect(mustDefer("pending")).toBe(true);
    expect(mustDefer("waiting")).toBe(false);
    apply.mockResolvedValue({ kind: "restarted", sessionId: 7, confirmed: false, reason: null });
    let current = deps({ tone: "running" });
    const { result, rerender } = renderHook(() => useSessionSettings(current));
    act(() => result.current.request({ model: { id: "kimi-code/k3", label: "K3" }, label: "K3" }));
    // 运行中又选了一档权限：两次合并，只执行一次。
    act(() => result.current.request({ modes: { permission: "yolo" }, label: "Ask When Needed" }));
    expect(apply).not.toHaveBeenCalled();
    expect(result.current.notice).toBe("这一轮结束后切换到 Ask When Needed");
    current = deps({ tone: "pending" });
    rerender();
    expect(apply).not.toHaveBeenCalled();
    current = deps({ tone: "waiting" });
    rerender();
    await waitFor(() => expect(apply).toHaveBeenCalledTimes(1));
    expect(apply.mock.calls[0][0]).toMatchObject({
      sessionId: 7,
      model: "kimi-code/k3",
      modes: { permission: "yolo" },
      current: { permission: "manual" },
      tui: { idle: true, composerEmpty: true, approvalPending: false },
    });
    await waitFor(() => expect(result.current.notice).toBeNull());
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
    await act(async () => result.current.request({ modes: { permission: "manual" }, label: "Always Ask" }));
    expect(setSendError).toHaveBeenCalledWith("接回会话只能放宽权限，收紧要新建会话");
    apply.mockResolvedValueOnce({ kind: "unsupported", sessionId: -3, confirmed: false, reason: null });
    await act(async () => result.current.request({ model: { id: "gpt-5.5", label: "GPT-5.5" }, label: "GPT-5.5" }));
    expect(fallbackModel).toHaveBeenCalledWith("gpt-5.5");
  });

  /// OpenCode 够不着 TUI 服务时 agent 退回 Tab 循环；对话框路线回车后底栏没认出新值，说「下一条消息时核对」。
  it("模式兜底与未确认提示", async () => {
    const fallbackMode = vi.fn(() => true);
    const setSendError = vi.fn();
    const current = deps({ fallbackMode, setSendError });
    const { result } = renderHook(() => useSessionSettings(current));
    apply.mockResolvedValueOnce({ kind: "unsupported", sessionId: 7, confirmed: false, reason: null });
    await act(async () => result.current.request({ modes: { agent: "plan" }, label: "只出方案" }));
    expect(fallbackMode).toHaveBeenCalledWith("agent", "plan");
    expect(setSendError).not.toHaveBeenCalled();
    apply.mockResolvedValueOnce({ kind: "applied", sessionId: 7, confirmed: false, reason: null });
    await act(async () => result.current.request({ modes: { agent: "build" }, label: "构建" }));
    expect(setSendError).toHaveBeenCalledWith("已切换，下一条消息时核对");
  });

  /// 回合结束（running → 空闲）读一次会话此刻的模型 / 模式：用户在 TUI 里自己切过的跟上，不轮询。
  it("回合结束时同步当前模型与模式", async () => {
    live.mockReset();
    live.mockResolvedValue({ model: "gpt-5.5", effort: "high", modes: [{ dimension: "approval", value: "yolo" }] });
    let state: ChatHistory | null = { ...history, sessionId: 7, effort: null } as unknown as ChatHistory;
    const setHistory = vi.fn((update: ChatHistory | null | ((prev: ChatHistory | null) => ChatHistory | null)) => {
      state = typeof update === "function" ? update(state) : update;
    });
    let current = deps({ tone: "running", setHistory, modelLabel: (id) => (id === "gpt-5.5" ? "GPT-5.5" : id) });
    const { rerender } = renderHook(() => useSessionSettings(current));
    expect(live).not.toHaveBeenCalled();
    current = deps({ tone: "waiting", setHistory, modelLabel: (id) => (id === "gpt-5.5" ? "GPT-5.5" : id) });
    rerender();
    await waitFor(() => expect(live).toHaveBeenCalledWith(7));
    await waitFor(() => expect(state?.model).toBe("GPT-5.5"));
    expect(state?.agentModes).toEqual([{ dimension: "permission", value: "manual" }, { dimension: "approval", value: "yolo" }]);
  });

  it("后来的选择覆盖先前的", () => {
    expect(mergeRequests({ modes: { a: "1", b: "1" }, label: "x" }, { modes: { b: "2" }, label: "y" })).toEqual({ model: undefined, modes: { a: "1", b: "2" }, label: "y" });
  });
});
