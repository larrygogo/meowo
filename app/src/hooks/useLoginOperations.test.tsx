import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  createLoginOperationId: vi.fn((provider: string) => `op-${provider}`),
  loginAgent: vi.fn(),
  cancelLogin: vi.fn(),
}));
const events = vi.hoisted(() => ({
  callback: undefined as undefined | ((event: unknown) => void),
}));

vi.mock("../api", () => api);
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, callback: (event: unknown) => void) => {
    events.callback = callback;
    return Promise.resolve(() => {});
  },
}));

import { useLoginOperations } from "./useLoginOperations";

beforeEach(() => {
  api.createLoginOperationId.mockClear();
  api.loginAgent.mockReset().mockResolvedValue(undefined);
  api.cancelLogin.mockReset().mockResolvedValue(undefined);
  events.callback = undefined;
});

describe("useLoginOperations", () => {
  it("tracks providers independently and passes profile plus operationId to IPC", async () => {
    const hook = renderHook(() => useLoginOperations());
    await act(async () => {
      await Promise.all([
        hook.result.current.start("claude", { profile: "work" }),
        hook.result.current.start("codex"),
      ]);
    });

    expect(api.loginAgent).toHaveBeenCalledWith("claude", undefined, "work", "op-claude");
    expect(api.loginAgent).toHaveBeenCalledWith("codex", undefined, undefined, "op-codex");
    expect(hook.result.current.isPending("claude")).toBe(true);
    expect(hook.result.current.isPending("codex")).toBe(true);
    expect(hook.result.current.states.get("claude")).toEqual({
      phase: "pending",
      operationId: "op-claude",
      profile: "work",
    });
  });

  it("ignores stale completion events and cancels with the current operationId", async () => {
    const onDone = vi.fn();
    const hook = renderHook(() => useLoginOperations(onDone));
    await act(async () => { await hook.result.current.start("claude"); });

    act(() => events.callback?.({
      payload: { provider: "claude", operationId: "old-op", outcome: "timeout" },
    }));
    expect(hook.result.current.isPending("claude")).toBe(true);
    expect(onDone).not.toHaveBeenCalled();

    await act(async () => { await hook.result.current.cancel("claude"); });
    expect(api.cancelLogin).toHaveBeenCalledWith("claude", "op-claude");

    act(() => events.callback?.({
      payload: { provider: "claude", operationId: "op-claude", outcome: "cancelled" },
    }));
    expect(hook.result.current.isPending("claude")).toBe(false);
    expect(hook.result.current.states.get("claude")).toEqual({ phase: "done", outcome: "cancelled" });
    expect(onDone).toHaveBeenCalledWith({
      provider: "claude",
      operationId: "op-claude",
      outcome: "cancelled",
    });
  });
});
