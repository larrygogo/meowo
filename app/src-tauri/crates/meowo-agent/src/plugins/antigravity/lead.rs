//! Antigravity CLI 的 Lead 接入。
//!
//! 资产都以 `~/.gemini` 为根（[`LeadCap::asset_root`]）：
//! - 技能 `antigravity-cli/skills/lead/SKILL.md`：CLI 额外读 `~/.gemini/antigravity-cli/skills/`
//!   （skills.md「CLI skill locations」），只给 CLI 用，不进 App/IDE 共用的 `~/.gemini/config/skills`。
//! - 审查角色 `agents/lead-redteam.md`：`agy agents` 列出的就是 `~/.gemini/agents/` 下的定义（本机实测）。
//!   frontmatter 要 `subagent: true`、`mainAgent: false`，不写 tools 只有只读默认集（内置
//!   migrate-agent-settings 技能 Step 3）。
//! - hook 在 `config/hooks.json` 的 `meowo-lead` 一块（[`LeadHookFormat::AgyJson`]）。
//!
//! 续跑走会话内等待器：模型在后台跑 `meowo-reporter lead wait`，leader 答复后它结束；agy 在非 daemon
//! 后台任务结束时会唤醒 agent 开新一轮（changelog：「when a background task or subagent finishes and
//! wakes the agent」）。agy 的 run_command 在 Windows 上是 PowerShell（真机样本 `Get-ChildItem -Force`）。
//!
//! 事件：负载由 [`super::hook::normalize`] 改写成规范形状后交给 reporter 的 Lead 适配：
//! `PreInvocation`（每回合第一次）→ UserPromptSubmit、`PreToolUse` → 写关卡前查等待器、
//! `PostToolUse` → 认领、`Stop` → `{"decision":"continue"}` 续跑（真机探针 probe-hooks-1.md 第 7 条）。
use std::path::PathBuf;

use crate::{
    lead_install::{
        LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, NativeContinue, WaiterShell, REVIEW,
    },
    variant::Installation,
};

pub(super) static CAP: AntigravityLead = AntigravityLead;
pub(super) struct AntigravityLead;

/// hook timeout 是秒（agy 内置 hooks 文档）。
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "antigravity",
    version_family: (1, 5),
    verified: (1, 5, 18),
    skill_rel: "antigravity-cli/skills/lead/SKILL.md",
    reviewer_rel: "agents/lead-redteam.md",
    hook_rel: "config/hooks.json",
    format: LeadHookFormat::AgyJson,
    hooks: &[
        LeadHook {
            event: "SessionStart",
            mode: "session",
            timeout: 10,
        },
        LeadHook {
            event: "PreInvocation",
            mode: "session",
            timeout: 10,
        },
        LeadHook {
            event: "PreToolUse",
            mode: "gate",
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
        "lead/non_lead_io_unverified",
    ],
    // agy 在 Windows 上用 `cmd /c` 执行 hook（不是 PowerShell），命令不带 `& `。
    powershell: false,
    windows_override: false,
};

const SHELL: WaiterShell = WaiterShell::PowerShellOnWindows;

impl LeadCap for AntigravityLead {
    /// 资产根是 `~/.gemini`（数据目录 `~/.gemini/antigravity-cli` 的上一层）：hook 与审查角色都不在
    /// 数据目录里，而相对路径不许带 `..`。
    fn asset_root(&self, installation: &Installation) -> PathBuf {
        installation
            .data_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| installation.data_dir.clone())
    }
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::InSessionWaiter { shell: SHELL })
    }
    /// 后台任务结束唤醒、Stop continue、负载形状都只在 1.5.18 上查证过。
    fn native_continue_floor(&self) -> Option<(u32, u32, u32)> {
        Some((1, 5, 18))
    }
    /// 等待器是 agy 自己 run_command 执行的一条命令：agy 的放行规则在它自己会重写的 settings.json 里，
    /// Meowo 不往里写（decisions 2026-11-26 16:01），default / accept-edits 档下这条命令可能要用户在终端批
    /// 一次；关卡续跑也还没在真机上走过（红队 redteam-1 F1）。设置页要照实说，不能只显示「可自动继续」。
    fn continue_notes(&self) -> &'static [&'static str] {
        &["lead/note_waiter_needs_approval"]
    }
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "stable").then_some(&SPEC)
    }
    fn reviewer(&self) -> String {
        format!(
            "---\nname: lead-redteam\ndescription: Independent adversarial review against the Lead specification.\nsubagent: true\nmainAgent: false\ntools:\n  - read_file\n  - grep_search\n  - find_by_name\n  - list_dir\n  - run_command\n---\n\n{REVIEW}"
        )
    }
}
