//! 已退役 agent 留在用户机器上的 Meowo 接线，一次性清掉。
//!
//! 2026-11 gemini CLI 被 Antigravity CLI 全面替换（用户要求）。插件删掉之后，再没有代码会碰
//! `~/.gemini`，而 Meowo 当年往那里写过：settings.json 里指向 meowo-reporter 的 hooks（普通的与 Lead
//! 的）、`mcpServers.meowo`、Lead 改过的两项设置、`meowo-lead-settings.json`、两份策略文件、
//! Lead 技能与审查角色。留着它们，用户以后在终端里手动跑 gemini 时 hook 还会往看板塞一张状态
//! 不动的卡片；卸载 Meowo 后每个 hook 都会因为 exe 不存在而报错。
//!
//! 只删 Meowo 自己的东西：hook 按「可执行是 meowo-reporter、参数是 `--provider gemini`
//! 或 `lead <模式> --provider gemini`」认；整文件托管的只删带 [`MANAGED_MARK`] 的；设置项只在
//! 值仍是 Meowo 写的那个时才还原。用户自己的 hook（比如别的工具挂的）、自己改过的设置一概不动。
//!
//! 注意 agy 也读 `~/.gemini/skills/` 与 `~/.gemini/agents/`：审查角色只删 gemini 格式
//! （frontmatter 带 `kind: local`）的那份，不误删 antigravity 的 Lead 安装写进去的文件。
//!
//! 宿主每次启动调一次（读一个 JSON、几次 stat，没有要清的就什么都不写）；卸载器也调。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::lead_install::MANAGED_MARK;

const RETIRED_ID: &str = "gemini";

/// gemini 的数据目录：`GEMINI_CLI_HOME` 覆盖的是 home（其下再拼 `.gemini`，0.65 Storage），否则
/// `~/.gemini`。
fn data_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("GEMINI_CLI_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home).join(".gemini"));
    }
    Some(crate::home_dir()?.join(".gemini"))
}

/// 清理并返回做了什么（每处一行，宿主写日志、卸载器写安装日志）。
pub fn sweep() -> Vec<String> {
    data_dir()
        .filter(|dir| dir.is_dir())
        .map(|dir| sweep_in(&dir))
        .unwrap_or_default()
}

pub fn sweep_in(dir: &Path) -> Vec<String> {
    let mut report = Vec::new();
    report.extend(clean_settings(dir));
    for rel in ["policies/meowo-images.toml", "policies/meowo-lead.toml"] {
        let path = crate::join_rel(dir, rel);
        // 发图策略文件开头是「由 Meowo 管理」，Lead 策略是「由 Meowo 安装和管理」，都按 Meowo 认。
        if read(&path).is_some_and(|t| t.contains("由 Meowo ")) && std::fs::remove_file(&path).is_ok() {
            report.push(format!("删除 {}", path.display()));
        }
    }
    let skill = dir.join("skills").join("lead").join("SKILL.md");
    if read(&skill).is_some_and(|t| t.contains(MANAGED_MARK)) && std::fs::remove_file(&skill).is_ok() {
        report.push(format!("删除 {}", skill.display()));
        let _ = std::fs::remove_dir(skill.parent().unwrap_or(dir));
    }
    let reviewer = dir.join("agents").join("lead-redteam.md");
    if read(&reviewer).is_some_and(|t| t.contains(MANAGED_MARK) && is_gemini_agent(&t))
        && std::fs::remove_file(&reviewer).is_ok()
    {
        report.push(format!("删除 {}", reviewer.display()));
    }
    report
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// gemini 的子代理 frontmatter 带 `kind: local`；agy 的不带（它要 `subagent: true`）。
fn is_gemini_agent(text: &str) -> bool {
    text.strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---"))
        .is_some_and(|(front, _)| front.lines().any(|l| l.trim() == "kind: local"))
}

/// 这条 hook 命令是 Meowo 给 gemini 写的（普通上报或 Lead）。
fn ours(command: &str) -> bool {
    let Some((exe, args)) = crate::config::parse_hook_command(command) else {
        return false;
    };
    let name = exe.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    if !matches!(name.as_str(), "meowo-reporter" | "meowo-reporter.exe" | "cc-reporter" | "cc-reporter.exe") {
        return false;
    }
    let tail = ["--provider", RETIRED_ID];
    match args.as_slice() {
        [a, b] => [a.as_str(), b.as_str()] == tail,
        [lead, _mode, a, b] => lead == "lead" && [a.as_str(), b.as_str()] == tail,
        _ => false,
    }
}

/// Lead 打开的两项设置（gemini 插件 lead.rs 的 SETTINGS）：（父对象 JSON pointer，键，Meowo 写的值）。
fn lead_settings() -> [(&'static str, &'static str, Value); 2] {
    [
        ("/tools/shell", "backgroundCompletionBehavior", Value::from("inject")),
        ("/experimental", "modelSteering", Value::from(true)),
    ]
}

const SAVED_REL: &str = "meowo-lead-settings.json";

fn clean_settings(dir: &Path) -> Vec<String> {
    let path = dir.join("settings.json");
    let mut report = Vec::new();
    let Some(text) = read(&path) else {
        return report;
    };
    // gemini 允许 JSONC，解析不了的文件不动（宁可留着，也不冲掉用户配置）。
    let Ok(mut root) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return report;
    };
    if !root.is_object() {
        return report;
    }
    let original = root.clone();
    let removed = strip_hooks(&mut root);
    if removed > 0 {
        report.push(format!("{}：摘掉 {removed} 条 Meowo hook", path.display()));
    }
    let mcp_is_ours = root
        .pointer("/mcpServers/meowo/command")
        .and_then(Value::as_str)
        .is_some_and(|cmd| {
            let name = cmd.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
            name.starts_with("meowo-reporter")
        });
    if mcp_is_ours {
        if let Some(servers) = root.pointer_mut("/mcpServers").and_then(Value::as_object_mut) {
            servers.remove("meowo");
            if servers.is_empty() {
                root.as_object_mut().map(|o| o.remove("mcpServers"));
            }
            report.push(format!("{}：摘掉 mcpServers.meowo", path.display()));
        }
    }
    let saved_path = dir.join(SAVED_REL);
    let saved: Value = read(&saved_path)
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    if saved.is_object() {
        for (pointer, key, ours) in lead_settings() {
            let current = root.pointer(&format!("{pointer}/{key}")).cloned();
            if current.as_ref() != Some(&ours) {
                continue;
            }
            if let Some(parent) = root.pointer_mut(pointer).and_then(Value::as_object_mut) {
                match saved.get(key).filter(|v| !v.is_null()) {
                    Some(previous) => {
                        parent.insert(key.into(), previous.clone());
                    }
                    None => {
                        parent.remove(key);
                    }
                }
            }
            prune(&mut root, pointer);
            report.push(format!("{}：还原 {key}", path.display()));
        }
    }
    if root != original {
        let Ok(body) = serde_json::to_string_pretty(&root) else {
            return Vec::new();
        };
        if crate::wiring::backup_once(&path).is_err()
            || crate::fsutil::write_atomic(&path, &format!("{body}\n")).is_err()
        {
            return Vec::new();
        }
    }
    if saved.is_object() && std::fs::remove_file(&saved_path).is_ok() {
        report.push(format!("删除 {}", saved_path.display()));
    }
    report
}

/// `hooks.<事件>[]` 里每组 `{hooks:[{command}]}` 摘掉 Meowo 的 handler；组空了删组，事件空了删事件，
/// hooks 空了删 hooks。返回摘掉的条数。
fn strip_hooks(root: &mut Value) -> usize {
    let mut removed = 0;
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return 0;
    };
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = handlers.len();
                handlers.retain(|h| !h.get("command").and_then(Value::as_str).is_some_and(ours));
                removed += before - handlers.len();
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|handlers| !handlers.is_empty())
        });
    }
    hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
    if hooks.is_empty() {
        root.as_object_mut().map(|o| o.remove("hooks"));
    }
    removed
}

/// 删掉只剩空壳的父对象（tools.shell → tools），配置回到安装 Lead 前的样子。
fn prune(root: &mut Value, pointer: &str) {
    let keys: Vec<&str> = pointer.split('/').filter(|k| !k.is_empty()).collect();
    for depth in (1..=keys.len()).rev() {
        let path = format!("/{}", keys[..depth].join("/"));
        if !root
            .pointer(&path)
            .is_some_and(|v| v.as_object().is_some_and(|m| m.is_empty()))
        {
            continue;
        }
        let parent = if depth == 1 {
            root.as_object_mut()
        } else {
            root.pointer_mut(&format!("/{}", keys[..depth - 1].join("/")))
                .and_then(Value::as_object_mut)
        };
        if let Some(parent) = parent {
            parent.remove(keys[depth - 1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-retired-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 本机 ~/.gemini/settings.json 的真实形态（路径换成占位）：Meowo 的普通 hook、Lead hook 与别的
    /// 工具（orca）挂的 hook 混在同一个事件下。
    #[test]
    fn strips_only_meowo_hooks_and_restores_lead_settings() {
        let dir = temp("settings");
        let reporter = r#"& "C:\Users\u\AppData\Local\Meowo\meowo-reporter.exe""#;
        let settings = json!({
            "experimental": {"modelSteering": true},
            "tools": {"shell": {"backgroundCompletionBehavior": "inject"}},
            "theme": "dark",
            "hooks": {
                "AfterAgent": [
                    {"hooks": [{"command": format!("{reporter} --provider gemini"), "timeout": 5000, "type": "command"}]},
                    {"hooks": [{"command": format!("{reporter} lead stop --provider gemini"), "timeout": 5000, "type": "command"}]},
                    {"hooks": [{"type": "command", "command": "C:/Users/u/.orca/agent-hooks/gemini-hook.cmd", "timeout": 10000}]}
                ],
                "BeforeModel": [
                    {"hooks": [{"command": format!("{reporter} lead session --provider gemini"), "type": "command"}]}
                ]
            },
            "mcpServers": {
                "meowo": {"args": ["mcp"], "command": r"C:\Users\u\AppData\Local\Meowo\meowo-reporter.exe", "type": "stdio"},
                "other": {"command": "npx"}
            }
        });
        std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
        std::fs::write(dir.join(SAVED_REL), r#"{"backgroundCompletionBehavior":null,"modelSteering":null}"#).unwrap();
        let report = sweep_in(&dir);
        assert!(!report.is_empty());
        let after: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(
            after["hooks"],
            json!({"AfterAgent": [{"hooks": [{"type": "command", "command": "C:/Users/u/.orca/agent-hooks/gemini-hook.cmd", "timeout": 10000}]}]}),
            "只留下别的工具挂的 hook"
        );
        assert!(after.get("experimental").is_none(), "Lead 打开的设置还原后父对象一并删掉");
        assert!(after.get("tools").is_none());
        assert_eq!(after["theme"], "dark");
        assert!(after["mcpServers"].get("meowo").is_none());
        assert!(after["mcpServers"].get("other").is_some());
        assert!(!dir.join(SAVED_REL).exists());
        // 再跑一次什么都不做。
        assert!(sweep_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 用户自己改过的设置（不是 Meowo 写的值）不还原。
    #[test]
    fn user_changed_settings_are_left_alone() {
        let dir = temp("user");
        std::fs::write(dir.join("settings.json"), r#"{"experimental":{"modelSteering":false}}"#).unwrap();
        std::fs::write(dir.join(SAVED_REL), r#"{"modelSteering":null}"#).unwrap();
        sweep_in(&dir);
        let after: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
        assert_eq!(after["experimental"]["modelSteering"], false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 托管文件只删 Meowo 的；审查角色只删 gemini 格式的（agy 也读 ~/.gemini/agents）。
    #[test]
    fn managed_files_are_removed_but_agy_reviewer_is_kept() {
        let dir = temp("files");
        std::fs::create_dir_all(dir.join("policies")).unwrap();
        std::fs::create_dir_all(dir.join("skills/lead")).unwrap();
        std::fs::create_dir_all(dir.join("agents")).unwrap();
        std::fs::write(dir.join("policies/meowo-images.toml"), "# 由 Meowo 管理：…\n[[rule]]\n").unwrap();
        std::fs::write(dir.join("policies/mine.toml"), "[[rule]]\n").unwrap();
        std::fs::write(dir.join("skills/lead/SKILL.md"), format!("---\nname: lead\n---\n<!-- {MANAGED_MARK} -->\n")).unwrap();
        std::fs::write(
            dir.join("agents/lead-redteam.md"),
            format!("---\nname: lead-redteam\nsubagent: true\n---\n<!-- {MANAGED_MARK} -->\n"),
        )
        .unwrap();
        sweep_in(&dir);
        assert!(!dir.join("policies/meowo-images.toml").exists());
        assert!(dir.join("policies/mine.toml").exists());
        assert!(!dir.join("skills/lead").exists());
        assert!(dir.join("agents/lead-redteam.md").exists(), "agy 格式的审查角色不删");
        std::fs::write(
            dir.join("agents/lead-redteam.md"),
            format!("---\nname: lead-redteam\nkind: local\n---\n<!-- {MANAGED_MARK} -->\n"),
        )
        .unwrap();
        sweep_in(&dir);
        assert!(!dir.join("agents/lead-redteam.md").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hook_claim_requires_reporter_and_gemini_provider() {
        assert!(ours(r#"& "C:\x\meowo-reporter.exe" --provider gemini"#));
        assert!(ours(r#""C:\x\meowo-reporter.exe" lead claim --provider gemini"#));
        assert!(!ours(r#""C:\x\meowo-reporter.exe" --provider antigravity"#));
        assert!(!ours("node tools/meowo-reporter-notify.js --provider gemini"));
        assert!(!ours("C:/Users/u/.orca/agent-hooks/gemini-hook.cmd"));
    }
}
