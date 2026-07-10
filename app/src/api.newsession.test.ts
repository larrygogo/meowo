import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

import { listAgents, agentName, newSession, recentCwds, checkProviderHooks } from "./api";

beforeEach(() => invokeMock.mockReset());

describe("new-session api", () => {
  it("listAgents 透传后端下发的名单（前端不再自带一份）", async () => {
    invokeMock.mockResolvedValue([
      { id: "claude", display_name: "Claude Code", installed: true },
      { id: "gemini", display_name: "Gemini CLI", installed: false },
    ]);
    const agents = await listAgents();
    expect(invokeMock).toHaveBeenCalledWith("list_agents");
    // 本版本不认识的 agent 也照样透传——不过滤、不改写。
    expect(agents.map((a) => a.id)).toEqual(["claude", "gemini"]);
  });

  it("agentName 未知 id 回退成 id 本身，不冒名成 claude", () => {
    const agents = [{ id: "claude", display_name: "Claude Code", installed: true }];
    expect(agentName(agents, "claude")).toBe("Claude Code");
    expect(agentName(agents, "gemini")).toBe("gemini");
    expect(agentName([], "claude")).toBe("claude");
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
