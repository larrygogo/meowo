import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const apply = vi.fn();
vi.mock("../../api", () => ({ applySessionSettings: (...args: unknown[]) => apply(...args) }));

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

  it("后来的选择覆盖先前的", () => {
    expect(mergeRequests({ modes: { a: "1", b: "1" }, label: "x" }, { modes: { b: "2" }, label: "y" })).toEqual({ model: undefined, modes: { a: "1", b: "2" }, label: "y" });
  });
});
