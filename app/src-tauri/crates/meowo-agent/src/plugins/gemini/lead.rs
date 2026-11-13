//! Gemini 官方 subagents 格式为 kind: local 与本家工具名；不改用户已禁用的 agents 设置。
//! 取证：https://geminicli.com/docs/cli/skills/
//! 取证：https://geminicli.com/docs/core/subagents/
//! 取证：https://geminicli.com/docs/hooks/reference/
use crate::{
    lead_install::{LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, REVIEW},
    variant::Installation,
};
pub(super) static CAP: GeminiLead = GeminiLead;
pub(super) struct GeminiLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "gemini",
    version_family: (0, 65),
    verified: (0, 65, 0),
    skill_rel: "skills/lead/SKILL.md",
    reviewer_rel: "agents/lead-redteam.md",
    hook_rel: "settings.json",
    format: LeadHookFormat::Json,
    hooks: &[
        LeadHook {
            event: "SessionStart",
            mode: "session",
            timeout: 5000,
        },
        LeadHook {
            event: "BeforeAgent",
            mode: "session",
            timeout: 5000,
        },
        LeadHook {
            event: "BeforeModel",
            mode: "session",
            timeout: 5000,
        },
        LeadHook {
            event: "AfterTool",
            mode: "claim",
            timeout: 5000,
        },
        LeadHook {
            event: "AfterAgent",
            mode: "stop",
            timeout: 5000,
        },
    ],
    limitations: &[
        "lead/auto_continue_unverified",
        "lead/offline_resume_unverified",
        "lead/main_agent_identity_unverified",
        "lead/non_lead_io_unverified",
    ],
    powershell: true,
    windows_override: false,
};
impl LeadCap for GeminiLead {
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "stable").then_some(&SPEC)
    }
    fn activation_issue(&self, inst: &Installation) -> Option<String> {
        let text = std::fs::read_to_string(inst.data_dir.join("settings.json")).ok()?;
        let doc = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        (doc.pointer("/experimental/enableAgents")
            .and_then(serde_json::Value::as_bool)
            == Some(false))
        .then(|| "lead/reviewer_disabled".into())
    }
    fn reviewer(&self) -> String {
        format!("---\nname: lead-redteam\ndescription: Independent adversarial review against the Lead specification.\nkind: local\ntools:\n  - read_file\n  - grep_search\n  - glob\n  - list_directory\n  - run_shell_command\n---\n\n{REVIEW}")
    }
}
