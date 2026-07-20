import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { listAgents, agentName, newSession, recentCwds, checkProviderHooks } from "./api";

beforeEach(() => invokeMock.mockReset());

describe("new-session api", () => {
  it("listAgents 透传后端下发的名单（前端不再自带一份）", async () => {
    invokeMock.mockResolvedValue([
      { id: "claude", display_name: "Claude Code", installed: true, supports_proxy: true },
      // 一个本版本不认识的 id（曾用 "gemini" 举例，而它后来真成了 agent）。
      { id: "not-an-agent", display_name: "Not An Agent", installed: false, supports_proxy: false },
    ]);
    const agents = await listAgents();
    expect(invokeMock).toHaveBeenCalledWith("list_agents");
    // 本版本不认识的 agent 也照样透传——不过滤、不改写。
    expect(agents.map((a) => a.id)).toEqual(["claude", "not-an-agent"]);
  });

  it("agentName 未知 id 回退成 id 本身，不冒名成 claude", () => {
    const agents = [
      { id: "claude", display_name: "Claude Code", installed: true, supports_proxy: true, supports_account: true, supports_profiles: true },
    ];
    expect(agentName(agents, "claude")).toBe("Claude Code");
    // 名单里没有的 id → 回退成 id 本身（显示 "not-an-agent" 好过显示 "Claude Code"）。
    expect(agentName(agents, "not-an-agent")).toBe("not-an-agent");
    expect(agentName([], "claude")).toBe("claude");
  });

  it("newSession 透传参数（含启动选项的 choice id）", () => {
    invokeMock.mockResolvedValue(undefined);
    newSession("C:/p", "claude", { model: "opus" }, "wt");
    expect(invokeMock).toHaveBeenCalledWith("new_session", {
      cwd: "C:/p", provider: "claude", terminal: "wt", options: { model: "opus" },
    });
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
