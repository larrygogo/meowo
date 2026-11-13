//! OpenCode 1.19.3 原生插件与 Markdown subagent；idle 事件不能靠 hook 输出启动下一轮。
//! 取证：https://opencode.ai/v1/docs/skills/
//! 取证：https://opencode.ai/v1/docs/agents/
//! 取证：https://raw.githubusercontent.com/anomalyco/opencode/v1.19.3/packages/plugin/src/index.ts
use crate::{
    lead_install::{
        LeadCap, LeadHookFormat, LeadInstallSpec, NativeContinue, ServerProtocol, REVIEW,
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
        // 自动继续与离线恢复已由宿主 HTTP 投递闭环（lead_native::opencode_http，1.19.5 隔离真机探针）。
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
    /// 排进同一会话；TUI 退出后宿主起 serve 投递、打开时 attach（任务 20261113-lead-tui，1.19.5 隔离真机验证）。
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp))
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
