//! Kimi Code 官方 agents/skills 文档：现代版目录跟随 KIMI_CODE_HOME，旧 Python 变体未取证不安装。
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/skills.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/agents.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/hooks.html
use crate::{
    lead_install::{LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, REVIEW},
    variant::Installation,
};
pub(super) static CAP: KimiLead = KimiLead;
pub(super) struct KimiLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "kimi",
    version_family: (2, 5),
    skill_rel: "skills/lead/SKILL.md",
    reviewer_rel: "agents/lead-redteam.md",
    hook_rel: "config.toml",
    format: LeadHookFormat::Toml,
    hooks: &[
        LeadHook {
            event: "SessionStart",
            mode: "session",
            timeout: 10,
        },
        LeadHook {
            event: "UserPromptSubmit",
            mode: "session",
            timeout: 10,
        },
        LeadHook {
            event: "PostToolUse",
            mode: "claim",
            timeout: 10,
        },
        LeadHook {
            event: "Stop",
            mode: "stop",
            timeout: 10,
        },
    ],
    limitations: &[
        "lead/auto_continue_unverified",
        "lead/offline_resume_unverified",
        "lead/main_agent_identity_unverified",
        "lead/compact_context_unverified",
        "lead/non_lead_io_unverified",
    ],
    powershell: false,
    windows_override: false,
};
impl LeadCap for KimiLead {
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "modern").then_some(&SPEC)
    }
    fn reviewer(&self) -> String {
        format!("---\nname: lead-redteam\ndescription: Independent adversarial review against the Lead specification.\ntools:\n  - Read\n  - Grep\n  - Glob\n  - Bash\nsubagents: []\n---\n\n{REVIEW}")
    }
}
