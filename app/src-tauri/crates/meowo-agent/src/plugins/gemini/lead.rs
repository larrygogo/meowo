//! Gemini 官方 subagents 格式为 kind: local 与本家工具名；不改用户已禁用的 agents 设置。
//! 取证：https://geminicli.com/docs/cli/skills/
//! 取证：https://geminicli.com/docs/core/subagents/
//! 取证：https://geminicli.com/docs/hooks/reference/
use crate::{
    lead_install::{
        waiter_command_prefix, LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, NativeContinue,
        OfflineResume, MANAGED_MARK, REVIEW,
    },
    variant::Installation,
};
use serde_json::{json, Value};
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

/// 自动续跑依赖的两项设置（0.68 源码）：后台命令结束时把输出注入会话（默认 silent），
/// 以及空闲时自动提交待处理注入（只在 modelSteering 打开时发生，interactiveCli 空闲 effect）。
/// leader 2026-11-13 同意打开 modelSteering；副作用是运行中回车从「排队」变「插话」（见 mod.rs）。
/// （父对象的 JSON pointer，键，Meowo 要的值）。
type Setting = (&'static str, &'static str, fn() -> Value);
const SETTINGS: [Setting; 2] = [
    ("/tools/shell", "backgroundCompletionBehavior", || {
        json!("inject")
    }),
    ("/experimental", "modelSteering", || json!(true)),
];
pub(super) const SAVED_REL: &str = "meowo-lead-settings.json";
const POLICY_REL: &str = "policies/meowo-lead.toml";

fn parent_mut<'a>(
    root: &'a mut Value,
    pointer: &str,
) -> Option<&'a mut serde_json::Map<String, Value>> {
    let mut node = root;
    for key in pointer.split('/').filter(|k| !k.is_empty()) {
        let map = node.as_object_mut()?;
        node = map.entry(key.to_owned()).or_insert_with(|| json!({}));
    }
    node.as_object_mut()
}

/// 卸载后删掉只剩空壳的父对象（tools.shell → tools，experimental），配置还原成安装前的样子。
fn prune(root: &mut Value, pointer: &str) {
    let keys: Vec<&str> = pointer.split('/').filter(|k| !k.is_empty()).collect();
    for depth in (1..=keys.len()).rev() {
        let path = format!("/{}", keys[..depth].join("/"));
        if root
            .pointer(&path)
            .is_some_and(|v| v.as_object().is_some_and(|m| m.is_empty()))
        {
            let parent = format!("/{}", keys[..depth - 1].join("/"));
            let parent = if depth == 1 {
                root.as_object_mut()
            } else {
                root.pointer_mut(&parent).and_then(Value::as_object_mut)
            };
            if let Some(parent) = parent {
                parent.remove(keys[depth - 1]);
            }
        }
    }
}

/// gemini 打开了 modelSteering 吗（Lead 安装会打开；用户也可能自己开）。对话页据此决定运行中回车的说法。
pub(super) fn model_steering(data_dir: &std::path::Path) -> bool {
    std::fs::read_to_string(data_dir.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')).ok())
        .and_then(|doc| {
            doc.pointer("/experimental/modelSteering")
                .and_then(Value::as_bool)
        })
        == Some(true)
}

impl LeadCap for GeminiLead {
    /// 关卡写入后 AfterTool 尾调用在后台启动等待器（不依赖模型），结束时 inject + modelSteering 让空闲
    /// TUI 自己开新回合。TUI 已退出时在 Meowo 终端里 `--resume <id> -i <答复>` 重新打开：初始提示在
    /// 初始化完成后自动提交（0.68 interactiveCli），正常交互模式与审批，不走 ACP。
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::InSessionWaiter {
            offline: OfflineResume::InteractiveInitialPrompt(&["-i", "{prompt}"]),
        })
    }
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
    /// 打开 SETTINGS 两项；首次安装记下用户原值，卸载时只在值仍是 Meowo 写的那个时还原，
    /// 用户之后自己改过的不动。其余设置一概不碰。
    fn edit_hook_file(
        &self,
        _inst: &Installation,
        text: &str,
        _reporter: &str,
        enabled: bool,
        saved: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let mut root: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| "lead/settings_invalid")?;
        if !root.is_object() {
            return Err("lead/settings_invalid".into());
        }
        let original = root.clone();
        let saved: Value = saved
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or_else(|| json!({}));
        let mut record = serde_json::Map::new();
        for (pointer, key, ours) in SETTINGS {
            let current = root.pointer(&format!("{pointer}/{key}")).cloned();
            if enabled {
                record.insert(key.into(), current.clone().unwrap_or(Value::Null));
                if current.as_ref() != Some(&ours()) {
                    parent_mut(&mut root, pointer)
                        .ok_or("lead/settings_invalid")?
                        .insert(key.into(), ours());
                }
            } else if current.as_ref() == Some(&ours()) {
                let parent = parent_mut(&mut root, pointer).ok_or("lead/settings_invalid")?;
                match saved.get(key).filter(|v| !v.is_null()) {
                    Some(previous) => {
                        parent.insert(key.into(), previous.clone());
                    }
                    None => {
                        parent.remove(key);
                    }
                }
                prune(&mut root, pointer);
            }
        }
        let record = enabled.then(|| Value::Object(record).to_string());
        if root == original {
            return Ok((text.to_owned(), record));
        }
        let text = serde_json::to_string_pretty(&root)
            .map(|text| format!("{text}\n"))
            .map_err(|_| "lead/settings_invalid")?;
        Ok((text, record))
    }
    fn saved_settings_rel(&self) -> Option<&'static str> {
        Some(SAVED_REL)
    }
    fn managed_file_rels(&self) -> &'static [&'static str] {
        &[POLICY_REL]
    }
    /// 放行等待器的用户级策略（~/.gemini/policies）：AfterTool 尾调用照常走审批策略（0.68 源码
    /// _validateAndCreateToolCall），不放行时每个关卡都要用户在 TUI 里点一次「允许」。
    /// commandPrefix 只匹配本机 reporter 的 `lead wait --provider gemini`。
    fn managed_file_body(
        &self,
        _inst: &Installation,
        relative: &str,
        reporter: &str,
    ) -> Result<String, String> {
        if relative != POLICY_REL || reporter.is_empty() {
            return Err("lead/asset_path_invalid".into());
        }
        let prefix = waiter_command_prefix("gemini", reporter);
        let mut doc = toml_edit::DocumentMut::new();
        let mut rule = toml_edit::Table::new();
        rule.insert("toolName", toml_edit::value("run_shell_command"));
        rule.insert("commandPrefix", toml_edit::value(prefix));
        rule.insert("decision", toml_edit::value("allow"));
        rule.insert("priority", toml_edit::value(900));
        let mut rules = toml_edit::ArrayOfTables::new();
        rules.push(rule);
        doc.insert("rule", toml_edit::Item::ArrayOfTables(rules));
        Ok(format!(
            "# {MANAGED_MARK}：放行 Lead 关卡等待器，卸载 Lead 时删除。\n{doc}"
        ))
    }
}
