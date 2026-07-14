import type { AgentDescriptor } from "../api";

/** 测试里已知的五家产品名。真实值由后端 `list_agents()` 下发（须与注册表的 display_name 同值）。 */
const NAMES: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  kimi: "Kimi Code",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
};
const RELAYS: Record<string, AgentDescriptor["relay"]> = {
  claude: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }, { value: "api_key", label: "API Key (x-api-key)" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["claude-sonnet-5"] }] },
  codex: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["gpt-5.4"] }] },
  kimi: { protocols: [{ value: "kimi", label: "Kimi" }, { value: "anthropic", label: "Anthropic Messages" }, { value: "openai", label: "OpenAI Chat Completions" }], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "kimi", default_auth: "bearer", suggestions: [{ protocol: "kimi", models: ["kimi-for-coding"] }] },
  // gemini：只有一种协议（讲 Gemini 自己的 generateContent，故 protocols 空）+ API Key 认证。
  gemini: { protocols: [], auth_modes: [{ value: "api_key", label: "API Key" }], default_protocol: "", default_auth: "api_key", suggestions: [{ protocol: "", models: ["gemini-2.5-pro"] }] },
  // opencode：中转＝往它配置里注入自定义 provider（OpenAI 兼容 / Anthropic）。
  opencode: { protocols: [{ value: "openai", label: "OpenAI 兼容" }, { value: "anthropic", label: "Anthropic Messages" }], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "openai", default_auth: "bearer", suggestions: [{ protocol: "openai", models: ["gpt-5.4"] }] },
};

/**
 * 能被套上代理的那些（＝插件声明了 ProxySpec）。当前**五家都有**（gemini/opencode 也认标准代理
 * 环境变量，实测其 bundle/二进制）。
 * 与后端 `AgentPlugin::proxy()` 的能力矩阵同源——那边加了代理能力，这里也要跟上。
 */
const SUPPORTS_PROXY = new Set(["claude", "codex", "kimi", "gemini", "opencode"]);

/**
 * 有账号概念的那些（＝插件声明了 account 能力槽）。当前**五家都有**，故这里全收。
 *
 * 它仍然是个可选能力槽：为 false 的 agent 不显示登录态、也不给登录入口（它的 `login_argv()`
 * 会是 None，按钮点下去只会报「拉起登录失败」）。要测那条路径，在用例里手写一个
 * `supports_account: false` 的 descriptor——别改这里，这里必须与后端的真实矩阵同源。
 */
const SUPPORTS_ACCOUNT = new Set(["claude", "codex", "kimi", "gemini", "opencode"]);

/**
 * 能有多个账号的那些（＝插件声明了 ProfileSpec）。**gemini 不行**：它的数据目录无法被环境变量
 * 覆盖（`GEMINI_DIR` 实测无效），谎称支持会让两个「账号」静默共用同一份凭据。
 */
const SUPPORTS_PROFILES = new Set(["claude", "codex", "kimi", "opencode"]);

/**
 * meowo 能显示上下文占用的那些。**gemini/opencode 不行**：gemini 的 hook 不带 token，opencode
 * 没声明 telemetry（token 在它自己库里）。与后端 `AgentPlugin::provides_context()` 同源。
 */
const SUPPORTS_CONTEXT = new Set(["claude", "codex", "kimi"]);

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
    supports_proxy: SUPPORTS_PROXY.has(id),
    supports_account: SUPPORTS_ACCOUNT.has(id),
    supports_profiles: SUPPORTS_PROFILES.has(id),
    supports_context: SUPPORTS_CONTEXT.has(id),
    relay: RELAYS[id],
  }));
}
