// 切换账号后岛上的额度换成新账号的（用户实拍 2026-11-15：切换账号之后灵动岛上的用量没有刷新）。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

const { invoke, listeners } = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

import { ACTIVE_PROFILE_CHANGED, useIslandUsage } from "./useIslandUsage";

const usageOf = (pct: number) => ({ lanes: [{ kind: "five_hour", used_pct: pct, resets_at: null }] });
const account = (provider: string, usage: unknown) => ({
  provider, account: { email: "x" }, usage, usage_supported: true, relay_enabled: false, active_profile_name: null,
});

describe("useIslandUsage 切换账号", () => {
  let current: Record<string, number>;
  let failing = false;
  let staleSnapshot = false;
  beforeEach(() => {
    listeners.clear();
    failing = false;
    staleSnapshot = false;
    current = { claude: 80, codex: 30 };
    invoke.mockReset();
    invoke.mockImplementation((command: string, args?: { provider?: string }) => {
      // 后端在切换时已清缓存：账号快照里不带旧额度。
      if (command === "get_accounts") return Promise.resolve([account("claude", staleSnapshot ? usageOf(80) : null), account("codex", null)]);
      if (command === "refresh_usage") {
        return failing ? Promise.reject("UNAVAILABLE") : Promise.resolve(usageOf(current[args!.provider!]));
      }
      return Promise.resolve(null);
    });
  });

  it("收到账号变更事件：立刻重取，那一家显示新账号的额度，别家不动", async () => {
    const view = renderHook(() => useIslandUsage());
    await waitFor(() => expect(view.result.current.usage.claude?.lanes[0].used_pct).toBe(80));
    await waitFor(() => expect(listeners.has(ACTIVE_PROFILE_CHANGED)).toBe(true));
    const before = invoke.mock.calls.filter(([c, a]) => c === "refresh_usage" && a.provider === "claude").length;

    current.claude = 5; // 新账号
    act(() => listeners.get(ACTIVE_PROFILE_CHANGED)!({ payload: "claude" }));

    await waitFor(() => expect(view.result.current.usage.claude?.lanes[0].used_pct).toBe(5));
    expect(invoke.mock.calls.filter(([c, a]) => c === "refresh_usage" && a.provider === "claude").length).toBeGreaterThan(before);
    expect(view.result.current.usage.codex?.lanes[0].used_pct).toBe(30);
  });

  it("新账号的额度取不到：不留着上一个账号的数字冒充", async () => {
    const view = renderHook(() => useIslandUsage());
    await waitFor(() => expect(view.result.current.usage.claude?.lanes[0].used_pct).toBe(80));
    await waitFor(() => expect(listeners.has(ACTIVE_PROFILE_CHANGED)).toBe(true));

    failing = true;
    act(() => listeners.get(ACTIVE_PROFILE_CHANGED)!({ payload: "claude" }));

    await waitFor(() => expect(view.result.current.meta.claude?.failed).toBe(true));
    expect(view.result.current.usage.claude).toBeUndefined();
  });

  it("账号快照里还带着上一个账号的缓存（后端某条路径没清）：也不拿它预填", async () => {
    const view = renderHook(() => useIslandUsage());
    await waitFor(() => expect(view.result.current.usage.claude?.lanes[0].used_pct).toBe(80));
    await waitFor(() => expect(listeners.has(ACTIVE_PROFILE_CHANGED)).toBe(true));

    staleSnapshot = true;
    failing = true;
    act(() => listeners.get(ACTIVE_PROFILE_CHANGED)!({ payload: "claude" }));

    await waitFor(() => expect(view.result.current.meta.claude?.failed).toBe(true));
    expect(view.result.current.usage.claude).toBeUndefined();
  });
});
