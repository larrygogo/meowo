//! 官方 custom agents 使用独立 TOML；skills/list 隔离探针确认 0.167.2 仍扫描 CODEX_HOME/skills。
//! 取证：https://learn.chatgpt.com/docs/agent-configuration/subagents
//! 取证：https://learn.chatgpt.com/docs/hooks
use crate::lead_install::{
    LeadCap, LeadConfigChange, LeadHook, LeadHookFormat, LeadInstallSpec, MANAGED_MARK, REVIEW,
};
use crate::variant::Installation;
use serde_json::Value;

pub(super) static CAP: CodexLead = CodexLead;
pub(super) struct CodexLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "codex",
    version_family: (0, 167),
    verified: (0, 167, 2),
    skill_rel: "skills/lead/SKILL.md",
    reviewer_rel: "agents/lead-redteam.toml",
    hook_rel: "hooks.json",
    format: LeadHookFormat::Json,
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
        "lead/non_lead_io_unverified",
    ],
    powershell: false,
    windows_override: true,
};
impl LeadCap for CodexLead {
    fn binding_policy(&self) -> crate::lead_install::LeadBindingPolicy {
        crate::lead_install::LeadBindingPolicy::NativeThreadMeta
    }
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "stable").then_some(&SPEC)
    }
    fn activation_issue(&self, inst: &Installation) -> Option<String> {
        let text = std::fs::read_to_string(inst.data_dir.join("config.toml")).ok()?;
        let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
        (doc.get("agents")
            .and_then(|v| v.get("enabled"))
            .and_then(toml_edit::Item::as_bool)
            == Some(false))
        .then(|| "lead/reviewer_disabled".into())
    }
    fn reviewer(&self) -> String {
        let mut doc = toml_edit::DocumentMut::new();
        doc["name"] = toml_edit::value("lead-redteam");
        doc["description"]=toml_edit::value("Independent adversarial review against the Lead specification and acceptance evidence.");
        doc["developer_instructions"] = toml_edit::value(REVIEW);
        format!("# {MANAGED_MARK}\n{doc}")
    }
    fn extra_config(
        &self,
        inst: &Installation,
        before: &str,
        after: &str,
        enabled: bool,
    ) -> Result<Vec<LeadConfigChange>, String> {
        let path = inst.data_dir.join("config.toml");
        let current = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("lead/read_failed".into()),
        };
        let mut doc = current
            .as_deref()
            .unwrap_or("")
            .trim_start_matches('\u{feff}')
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| "lead/settings_invalid")?;
        for key in ["hooks", "hooks.state"] {
            let item = if key == "hooks" {
                doc.get("hooks")
            } else {
                doc.get("hooks").and_then(|item| item.get("state"))
            };
            if item.is_some_and(|item| !item.is_table()) {
                return Err("lead/settings_invalid".into());
            }
        }
        let hooks_path = inst
            .data_dir
            .join(SPEC.hook_rel)
            .to_string_lossy()
            .into_owned();
        for entry in entries(before) {
            let key = format!(
                "{hooks_path}:{}:{}:{}",
                super::setup::snake_event(&entry.event),
                entry.group_idx,
                entry.handler_idx
            );
            let expected = super::setup::codex_hook_hash(
                super::setup::snake_event(&entry.event),
                &entry.command,
                entry.timeout,
            );
            if let Some(state) = doc
                .get_mut("hooks")
                .and_then(|item| item.get_mut("state"))
                .and_then(toml_edit::Item::as_table_mut)
            {
                if state
                    .get(&key)
                    .and_then(|item| item.get("trusted_hash"))
                    .and_then(toml_edit::Item::as_str)
                    == Some(expected.as_str())
                {
                    state.remove(&key);
                }
            }
        }
        if enabled {
            super::setup::ensure_trusted_hashes(&mut doc, &hooks_path, &entries(after));
        }
        let next = doc.to_string();
        if current.as_deref() == Some(next.as_str()) || (current.is_none() && next.is_empty()) {
            Ok(Vec::new())
        } else {
            Ok(vec![LeadConfigChange {
                path,
                before: current,
                after: next,
            }])
        }
    }
}

fn entries(text: &str) -> Vec<super::setup::ClaimedEntry> {
    let root = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    let mut out = Vec::new();
    if let Some(events) = root.get("hooks").and_then(Value::as_object) {
        for (event, groups) in events {
            for (group_idx, group) in groups.as_array().into_iter().flatten().enumerate() {
                let Some(handlers) = group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .filter(|handlers| handlers.len() == 1)
                else {
                    continue;
                };
                let handler = &handlers[0];
                let Some(command) = handler
                    .get("command")
                    .and_then(Value::as_str)
                    .filter(|command| crate::lead_install::managed_command(command, "codex"))
                else {
                    continue;
                };
                let command = if cfg!(windows) {
                    handler
                        .get("commandWindows")
                        .and_then(Value::as_str)
                        .unwrap_or(command)
                } else {
                    command
                };
                out.push(super::setup::ClaimedEntry {
                    event: event.clone(),
                    command: command.into(),
                    timeout: handler
                        .get("timeout")
                        .and_then(Value::as_u64)
                        .unwrap_or(600),
                    group_idx,
                    handler_idx: 0,
                });
            }
        }
    }
    out
}
