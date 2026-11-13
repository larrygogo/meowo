//! Kimi Code 官方 agents/skills 文档：现代版目录跟随 KIMI_CODE_HOME，旧 Python 变体未取证不安装。
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/skills.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/agents.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/hooks.html
use crate::{
    lead_install::{
        waiter_command_prefix, LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, NativeContinue,
        OfflineResume, MANAGED_MARK, REVIEW,
    },
    variant::Installation,
};
pub(super) static CAP: KimiLead = KimiLead;
pub(super) struct KimiLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "kimi",
    version_family: (2, 5),
    verified: (2, 5, 3),
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
/// kimi permission 规则（picomatch）里需要转义的字符，与 2.6.0 源码 GLOB_LITERAL_SPECIAL 一致。
fn glob_literal(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            let special = r"\*?[]{}()!+@|".contains(c);
            special
                .then_some('\\')
                .into_iter()
                .chain(std::iter::once(c))
        })
        .collect()
}

/// 放行等待器的 permission 规则：只匹配本机 reporter 的 `lead wait --provider kimi --session <id>`，
/// `*` 在 picomatch 里不跨 `/`，会话 id 里也没有 `/`，所以放不进别的命令。
fn waiter_rule(reporter: &str) -> String {
    format!(
        "Bash({} --session *)",
        glob_literal(&waiter_command_prefix("kimi", reporter))
    )
}

fn is_waiter_rule(item: &dyn toml_edit::TableLike) -> bool {
    item.get("pattern")
        .and_then(toml_edit::Item::as_str)
        .or_else(|| {
            item.get("pattern")
                .and_then(|i| i.as_value())
                .and_then(|v| v.as_str())
        })
        .is_some_and(|pattern| {
            pattern.starts_with("Bash(")
                && pattern.contains("meowo-reporter")
                && pattern.contains(" lead wait --provider kimi ")
        })
}

impl LeadCap for KimiLead {
    /// 关卡挂起时 TUI 里后台 Bash 跑等待器（Stop hook 给出原样命令）；完成通知让空闲的 TUI 自己开新回合
    /// （2.6.0 源码 completeDetached → loop.notify，research-kimi-gemini.md 假模型实测）。
    /// TUI 已退出时用 `-p` 续写同一会话：2026-11-13 假端点对照，`-p` 与 TUI 的请求头逐字一致
    /// （UA kimi-code-cli/2.6.0、X-Msh-Platform kimi_code_cli、turn_trigger user），计费路径相同；
    /// 但 `-p` 会把这一轮的权限模式强制成 auto（runV2Print → setMode("auto")），宿主据此只让它做
    /// 「确认答复、消费关卡」这一步（lead_native::waiter）。会话 id 已带 `session_` 前缀。
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::InSessionWaiter {
            offline: OfflineResume::PrintMode(&["-S", "{session}", "-p", "{prompt}"]),
        })
    }
    fn edit_hook_file(
        &self,
        _inst: &Installation,
        text: &str,
        reporter: &str,
        enabled: bool,
        _saved: Option<&str>,
    ) -> Result<(String, Option<String>), String> {
        let mut doc = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| "lead/settings_invalid")?;
        let desired = if enabled {
            Some(waiter_rule(reporter))
        } else {
            None
        };
        let mut ours = Vec::new();
        if let Some(rules) = doc.get("permission").and_then(|p| p.get("rules")) {
            if let Some(tables) = rules.as_array_of_tables() {
                for table in tables.iter() {
                    if is_waiter_rule(table) {
                        ours.push(
                            table
                                .get("pattern")
                                .and_then(toml_edit::Item::as_str)
                                .map(str::to_owned),
                        );
                    }
                }
            } else if let Some(array) = rules.as_array() {
                for value in array.iter() {
                    match value.as_inline_table() {
                        Some(table) if is_waiter_rule(table) => ours.push(
                            table
                                .get("pattern")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned),
                        ),
                        _ => {}
                    }
                }
            } else {
                return Err("lead/settings_invalid".into());
            }
        }
        // 已经是想要的样子就原样返回：inspect 靠幂等判断已安装，也不打乱用户规则的顺序。
        if ours.len() == usize::from(desired.is_some())
            && ours.first().cloned().flatten() == desired
        {
            return Ok((text.to_owned(), None));
        }
        if !doc.contains_key("permission") {
            if desired.is_none() {
                return Ok((text.to_owned(), None));
            }
            doc["permission"] = toml_edit::table();
        }
        let permission = doc["permission"]
            .as_table_like_mut()
            .ok_or("lead/settings_invalid")?;
        if !permission.contains_key("rules") {
            permission.insert(
                "rules",
                toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
            );
        }
        let rules = permission.get_mut("rules").ok_or("lead/settings_invalid")?;
        if let Some(tables) = rules.as_array_of_tables_mut() {
            tables.retain(|table| !is_waiter_rule(table));
            if let Some(pattern) = &desired {
                let mut table = toml_edit::Table::new();
                table.insert("decision", toml_edit::value("allow"));
                table.insert("scope", toml_edit::value("user"));
                table.insert("pattern", toml_edit::value(pattern.as_str()));
                table.insert(
                    "reason",
                    toml_edit::value(format!("{MANAGED_MARK}：Lead 关卡等待器")),
                );
                tables.push(table);
            }
        } else if let Some(array) = rules.as_array_mut() {
            array.retain(|value| !value.as_inline_table().is_some_and(|t| is_waiter_rule(t)));
            if let Some(pattern) = &desired {
                let mut table = toml_edit::InlineTable::new();
                table.insert("decision", "allow".into());
                table.insert("scope", "user".into());
                table.insert("pattern", pattern.as_str().into());
                table.insert("reason", format!("{MANAGED_MARK}：Lead 关卡等待器").into());
                array.push(table);
            }
        }
        // 卸载后只剩 Meowo 建的空壳时一并去掉，配置还原成安装前的样子。
        let empty_rules = permission.get("rules").is_some_and(|rules| {
            rules.as_array_of_tables().is_some_and(|t| t.is_empty())
                || rules.as_array().is_some_and(|a| a.is_empty())
        });
        if empty_rules {
            permission.remove("rules");
        }
        if permission.is_empty() {
            doc.remove("permission");
        }
        Ok((doc.to_string(), None))
    }
    fn spec(&self, inst: &Installation) -> Option<&'static LeadInstallSpec> {
        (inst.variant_tag == "modern").then_some(&SPEC)
    }
    fn reviewer(&self) -> String {
        format!("---\nname: lead-redteam\ndescription: Independent adversarial review against the Lead specification.\ntools:\n  - Read\n  - Grep\n  - Glob\n  - Bash\nsubagents: []\n---\n\n{REVIEW}")
    }
}
