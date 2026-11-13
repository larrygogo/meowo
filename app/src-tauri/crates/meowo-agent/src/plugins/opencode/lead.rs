//! OpenCode 1.19.3 原生插件与 Markdown subagent；idle 事件不能靠 hook 输出启动下一轮。
//! 取证：https://opencode.ai/v1/docs/skills/
//! 取证：https://opencode.ai/v1/docs/agents/
//! 取证：https://raw.githubusercontent.com/anomalyco/opencode/v1.19.3/packages/plugin/src/index.ts
use crate::{
    lead_install::{
        LeadCap, LeadHookFormat, LeadInstallSpec, NativeContinue, ServerAuth, ServerProtocol,
        REVIEW,
    },
    variant::Installation,
};
pub(super) static CAP: OpenCodeLead = OpenCodeLead;
pub(super) struct OpenCodeLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "opencode",
    version_family: (1, 19),
    verified: (1, 19, 3),
    skill_rel: "skills/lead/SKILL.md",
    reviewer_rel: "agents/lead-redteam.md",
    hook_rel: "plugins/meowo-lead.ts",
    format: LeadHookFormat::Plugin,
    hooks: &[],
    limitations: &[
        "lead/native_stop_unsupported",
        // 在线自动继续由宿主 HTTP 投递闭环（lead_native::opencode_http，1.19.5 隔离真机探针）；
        // TUI 不在时只保存答复，带端口重开后由巡检送达（leader 2026-11-13「不需要离线」）。
        "lead/main_agent_identity_unverified",
        "lead/non_lead_io_unverified",
    ],
    powershell: false,
    windows_override: false,
};
impl LeadCap for OpenCodeLead {
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "stable").then_some(&SPEC)
    }
    /// 原生 TUI 本身是内置 HTTP 服务的客户端：宿主让 TUI 带密码开放端口，答复经 `prompt_async`
    /// 排进同一会话（任务 20261113-lead-tui，1.19.5 隔离真机验证）。TUI 不在时不另起服务，答复等 TUI 重开再送。
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp))
    }
    /// 1.19.5 内嵌源码 `@opencode/ServerAuthConfig`：密码只读 OPENCODE_SERVER_PASSWORD 环境变量，
    /// 用户名默认 "opencode"（OPENCODE_SERVER_USERNAME 可改，宿主不设它）。没有从文件读密码的途径。
    fn server_auth(&self) -> Option<ServerAuth> {
        Some(ServerAuth {
            username: "opencode",
            password_env: "OPENCODE_SERVER_PASSWORD",
        })
    }
    fn reviewer(&self) -> String {
        format!("---\ndescription: Independent adversarial review against the Lead specification.\nmode: subagent\npermission:\n  edit: deny\n  bash: ask\n---\n\n{REVIEW}")
    }
    fn plugin(&self, reporter: &str) -> Option<String> {
        Some(PLUGIN.replace("__REPORTER__", &serde_json::to_string(reporter).ok()?))
    }
}
const PLUGIN: &str = r#"// 由 Meowo 安装和管理。原生插件随 OpenCode 运行，不依赖额外运行时。
const reporter = __REPORTER__
async function lead(mode, payload) {
  let child, timer
  try {
    child = Bun.spawn([reporter, "lead", mode, "--provider", "opencode"], { stdin: "pipe", stdout: "pipe", stderr: "ignore" })
    child.stdin.write(JSON.stringify(payload)); child.stdin.end()
    const completed = Promise.all([new Response(child.stdout).text(), child.exited]).then(([text, code]) => code === 0 && text ? JSON.parse(text) : null)
    return await Promise.race([completed, new Promise(resolve => { timer = setTimeout(() => { try { child.kill() } catch {} resolve(null) }, 5000) })])
  } catch { try { child?.kill() } catch {} return null }
  finally { clearTimeout(timer) }
}
export const MeowoLead = async ({ directory }) => ({
  "chat.message": async (input, output) => {
    await lead("session", { hook_event_name: "chat.message", cwd: directory, input, output })
  },
  "tool.execute.after": async (input, output) => {
    if (!JSON.stringify(input?.args ?? {}).includes(".lead")) return
    await lead("claim", { hook_event_name: "tool.execute.after", cwd: directory, input, output })
  },
  "experimental.chat.system.transform": async (input, output) => {
    const result = await lead("session", { hook_event_name: "experimental.chat.system.transform", cwd: directory, input })
    if (typeof result?.additionalContext === "string") output.system.push(result.additionalContext)
  },
})
"#;
