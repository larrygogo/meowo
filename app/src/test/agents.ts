import type { AgentDescriptor } from "../api";

/** 测试里已知的五家产品名。真实值由后端 `list_agents()` 下发（须与注册表的 display_name 同值）。 */
const NAMES: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  kimi: "Kimi Code",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
};

/**
 * 能被套上代理的那些（＝插件声明了 ProxySpec）。gemini / opencode 尚未声明，故设置页不给它们代理行。
 * 与后端 `AgentPlugin::proxy()` 的能力矩阵同源——那边加了代理能力，这里也要跟上。
 */
const SUPPORTS_PROXY = new Set(["claude", "codex", "kimi"]);

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
  }));
}
