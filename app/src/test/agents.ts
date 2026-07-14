import type { AgentDescriptor } from "../api";

/** 测试里已知的三家产品名。真实值由后端 `list_agents()` 下发。 */
const NAMES: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  kimi: "Kimi Code",
};
const RELAYS: Record<string, AgentDescriptor["relay"]> = {
  claude: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }, { value: "api_key", label: "API Key (x-api-key)" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["claude-sonnet-5"] }] },
  codex: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["gpt-5.4"] }] },
  kimi: { protocols: [{ value: "kimi", label: "Kimi" }, { value: "anthropic", label: "Anthropic Messages" }, { value: "openai", label: "OpenAI Chat Completions" }], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "kimi", default_auth: "bearer", suggestions: [{ protocol: "kimi", models: ["kimi-for-coding"] }] },
};

/**
 * 造一份 `list_agents()` 的返回：列出全部已知 agent，其中 `installed` 里的标记为已装。
 *
 * 各测试此前各自 mock `availableAgents()` 的 id 数组；descriptor 多了展示名与安装态两个字段，
 * 抽到一处免得三个测试文件各写一遍。
 */
export function descriptors(installed: string[]): AgentDescriptor[] {
  return Object.keys(NAMES).map((id) => ({
    id,
    display_name: NAMES[id],
    installed: installed.includes(id),
    relay: RELAYS[id],
  }));
}
