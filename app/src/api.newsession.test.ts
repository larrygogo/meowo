import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { PROVIDER_KEYS, newSession, recentCwds, checkProviderHooks } from "./api";

beforeEach(() => invokeMock.mockReset());

describe("new-session api", () => {
  it("PROVIDER_KEYS 覆盖三个 provider", () => {
    expect([...PROVIDER_KEYS].sort()).toEqual(["claude", "codex", "kimi"]);
  });

  it("newSession 透传参数", () => {
    invokeMock.mockResolvedValue(undefined);
    newSession("C:/p", "claude", "wt");
    expect(invokeMock).toHaveBeenCalledWith("new_session", { cwd: "C:/p", provider: "claude", terminal: "wt" });
  });

  it("checkProviderHooks 传 provider", () => {
    invokeMock.mockResolvedValue("missing");
    checkProviderHooks("codex");
    expect(invokeMock).toHaveBeenCalledWith("check_provider_hooks", { provider: "codex" });
  });

  it("recentCwds 传 limit", () => {
    invokeMock.mockResolvedValue([]);
    recentCwds(8);
    expect(invokeMock).toHaveBeenCalledWith("recent_cwds", { limit: 8 });
  });
});
