//! Meowo MCP 服务器（发图工具 + 面板浏览器）在「只认数据目录里配置文件」的 agent 那边的登记与
//! 撤销（[`crate::McpAttach::DataDirJson`]：kimi 的 `mcp.json` + `config.toml` 审批规则、gemini 的
//! `settings.json` + `policies/meowo-images.toml`）。
//!
//! 放在 meowo-agent 而不是宿主：宿主启动时按设置登记（browser_agent.rs `sync_mcp_files`），
//! 卸载器则调 sidecar `meowo-reporter uninstall-cleanup` 把这些写入全部撤掉（nsis-hooks.nsh
//! 的 PREUNINSTALL）——两边要用同一份增删逻辑，只认 Meowo 自己的那一项 / 那条规则，不碰用户的。
//! 遍历哪些 agent 只看插件声明的能力槽，不按 agent 名分支。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Meowo 在各家那边登记的 MCP 服务器名与发图工具名。
const SERVER_NAME: &str = crate::images::MCP_SERVER;
const SEND_IMAGE: &str = crate::images::SEND_IMAGE;

/// [`sync_mcp_file`] 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFileSync {
    /// 改写了文件。
    Written,
    /// 不需要改（已是目标状态、文件不存在且不用建、读不了）。
    Untouched,
    /// 文件在，但不是 Meowo 能安全改写的 JSON 对象（手写坏了、带注释的 JSONC）：没动它。
    Unparsable,
}

/// 各 agent「配置文件读不懂、没接上发图工具」的记录：agent id → 那份文件的路径。
/// 宿主每次登记时刷新（[`set_unreadable`]），描述符读它（`AgentDescriptor::mcp_config_unreadable`）。
static UNREADABLE: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// 记下（Some）或清掉（None）某 agent 的「配置文件读不懂」标记。
pub fn set_unreadable(agent: &str, file: Option<String>) {
    let mut table = UNREADABLE.lock().unwrap_or_else(|e| e.into_inner());
    table.retain(|(id, _)| id != agent);
    if let Some(file) = file {
        table.push((agent.to_string(), file));
    }
}

/// 某 agent 的配置文件读不懂时返回那份文件的路径。
pub fn unreadable(agent: &str) -> Option<String> {
    UNREADABLE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| id == agent)
        .map(|(_, file)| file.clone())
}

/// 卸载清理：对每个声明了 `DataDirJson` 的 agent，在默认账号与 `<数据根>/profiles/<agent>/*`
/// 下每个账号的数据目录里，摘掉 `mcpServers.meowo`、删掉 Meowo 那条审批规则 / 整个策略文件。
/// 返回做了什么（每处一行，卸载器写进安装日志）。只动 Meowo 自己的项。
pub fn uninstall_cleanup() -> Vec<String> {
    let data_root = crate::meowo_data_root();
    uninstall_cleanup_with(|plugin| {
        let agent = plugin.id().as_str();
        let mut homes: Vec<PathBuf> = plugin
            .default_installation()
            .map(|installation| installation.data_dir)
            .into_iter()
            .collect();
        if let Some(Ok(entries)) = data_root
            .as_deref()
            .map(|root| std::fs::read_dir(root.join("profiles").join(agent)))
        {
            for entry in entries.flatten().filter(|e| e.path().is_dir()) {
                homes.extend(plugin.installation_for_profile(&entry.path()).map(|i| i.data_dir));
            }
        }
        homes
    })
}

/// [`uninstall_cleanup`] 本体：`homes_of` 给出某插件要清理的数据目录（测试换成临时目录，
/// 不碰开发者真实的 ~/.gemini、~/.kimi）。遍历哪些插件只看能力槽。
pub fn uninstall_cleanup_with(homes_of: impl Fn(&'static dyn crate::AgentPlugin) -> Vec<PathBuf>) -> Vec<String> {
    let mut report = Vec::new();
    for plugin in crate::all() {
        let Some(crate::McpAttach::DataDirJson { rel, approval, .. }) = plugin.mcp_attach() else {
            continue;
        };
        let mut homes = homes_of(*plugin);
        homes.sort();
        homes.dedup();
        for home in homes.into_iter().filter(|dir| dir.is_dir()) {
            report.extend(cleanup_home(&home, rel, approval));
        }
    }
    report
}

/// 一个数据目录里的撤销（[`uninstall_cleanup`] 与测试共用）。
pub fn cleanup_home(home: &Path, rel: &str, approval: Option<crate::McpApproval>) -> Vec<String> {
    let mut report = Vec::new();
    let file = home.join(rel);
    match sync_mcp_file(&file, None) {
        McpFileSync::Written => report.push(format!("removed mcpServers.{SERVER_NAME}: {}", file.display())),
        McpFileSync::Unparsable => report.push(format!("skipped (unparsable): {}", file.display())),
        McpFileSync::Untouched => {}
    }
    match approval {
        Some(crate::McpApproval::PermissionRules { rel, prefix }) => {
            let path = home.join(rel);
            let before = std::fs::read_to_string(&path).ok();
            sync_permission_rule(&path, prefix, false);
            if before != std::fs::read_to_string(&path).ok() {
                report.push(format!("removed Meowo permission rule: {}", path.display()));
            }
        }
        Some(crate::McpApproval::PolicyFile { rel }) => {
            let path = home.join(rel);
            if path.exists() {
                sync_policy_file(&path, false);
                report.push(format!("removed policy file: {}", path.display()));
            }
        }
        None => {}
    }
    report
}

/// `entry` 为 None（没有 reporter）时摘掉 meowo 一项。
pub fn sync_mcp_file(path: &Path, entry: Option<Value>) -> McpFileSync {
    let path = path.to_path_buf();
    let mut root: Value = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            // 用户手写坏了的文件、带注释的 JSONC（gemini 的 settings.json 允许）不动，免得把
            // 他的配置冲掉；告诉调用方，好让界面说清楚为什么没接上。
            Err(_) => return McpFileSync::Unparsable,
        },
        // 只有「确实不存在」才当空文件新建。拒绝访问、被占用、路径是目录之类的读取错误
        // 若也当空文件，下面会写出只剩 meowo 一项的 mcpServers，把用户的配置冲掉。
        Err(e) if entry.is_some() && e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return McpFileSync::Untouched,
    };
    if !root.is_object() {
        return McpFileSync::Unparsable;
    }
    let servers = root.as_object_mut().and_then(|obj| {
        obj.entry("mcpServers").or_insert_with(|| json!({}));
        obj.get_mut("mcpServers").and_then(Value::as_object_mut)
    });
    let Some(servers) = servers else {
        return McpFileSync::Unparsable;
    };
    let changed = match entry {
        Some(entry) => {
            if servers.get(SERVER_NAME) == Some(&entry) {
                false
            } else {
                servers.insert(SERVER_NAME.into(), entry);
                true
            }
        }
        None => servers.remove(SERVER_NAME).is_some(),
    };
    if !changed {
        return McpFileSync::Untouched;
    }
    let Ok(body) = serde_json::to_string_pretty(&root) else {
        return McpFileSync::Untouched;
    };
    write_user_config(&path, &body);
    McpFileSync::Written
}

/// 同 wiring 的「写前必备份、一律原子写」：这是用户自己的配置文件，覆盖前留一份原件；
/// 备份失败就不写，宁可这次不接线也不冒丢配置的险。
fn write_user_config(path: &Path, body: &str) {
    if path.exists() && crate::wiring::backup_once(path).is_err() {
        return;
    }
    let _ = crate::fsutil::write_atomic(path, body);
}

/// 本条规则的标记（`reason` 字段），增删时按它认自己的那条，不碰用户与 Lead 的规则。
pub const RULE_REASON: &str = "由 Meowo 管理：send_image 只把图片显示给用户，免逐次审批";

/// kimi `config.toml` 的 `[[permission.rules]]`：开着时确保有一条放行发图工具的规则，
/// 关着时摘掉。文件不存在时不凭空新建（还没用过 kimi，审批模式由它自己定），
/// 解析失败（用户手写坏了）不碰。
pub fn sync_permission_rule(path: &Path, prefix: &str, enabled: bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(mut doc) = text.parse::<toml_edit::DocumentMut>() else {
        return;
    };
    let pattern = format!("{prefix}{SERVER_NAME}__{SEND_IMAGE}");
    let ours =
        |t: &toml_edit::Table| t.get("reason").and_then(|v| v.as_str()) == Some(RULE_REASON);
    let has = doc
        .get("permission")
        .and_then(|p| p.get("rules"))
        .and_then(|r| r.as_array_of_tables())
        .is_some_and(|rules| {
            rules.iter().any(|t| {
                ours(t) && t.get("pattern").and_then(|v| v.as_str()) == Some(pattern.as_str())
            })
        });
    if enabled == has {
        return;
    }
    if enabled {
        if !doc.contains_key("permission") {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            doc.insert("permission", toml_edit::Item::Table(table));
        }
        let Some(permission) = doc["permission"].as_table_mut() else {
            return;
        };
        if !permission.contains_key("rules") {
            permission.insert(
                "rules",
                toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
            );
        }
        let Some(rules) = permission
            .get_mut("rules")
            .and_then(|r| r.as_array_of_tables_mut())
        else {
            return;
        };
        // 旧版本留下的同标记规则（pattern 变了）先清掉。
        rules.retain(|t| !ours(t));
        let mut rule = toml_edit::Table::new();
        rule.insert("decision", toml_edit::value("allow"));
        rule.insert("scope", toml_edit::value("user"));
        rule.insert("pattern", toml_edit::value(pattern));
        rule.insert("reason", toml_edit::value(RULE_REASON));
        rules.push(rule);
    } else if let Some(rules) = doc
        .get_mut("permission")
        .and_then(|p| p.get_mut("rules"))
        .and_then(|r| r.as_array_of_tables_mut())
    {
        rules.retain(|t| !ours(t));
    }
    write_user_config(path, &doc.to_string());
}

/// gemini 用户级策略文件（整个文件归 Meowo 管）：开着时写一条只放行 `meowo` 服务器
/// `send_image` 工具的规则，关着时删掉文件。
pub fn sync_policy_file(path: &Path, enabled: bool) {
    if !enabled {
        let _ = std::fs::remove_file(path);
        return;
    }
    let body = policy_body();
    if std::fs::read_to_string(path).is_ok_and(|old| old == body) {
        return;
    }
    let _ = crate::fsutil::write_atomic(path, &body);
}

pub fn policy_body() -> String {
    let mut doc = toml_edit::DocumentMut::new();
    let mut rule = toml_edit::Table::new();
    rule.insert("mcpName", toml_edit::value(SERVER_NAME));
    rule.insert("toolName", toml_edit::value(SEND_IMAGE));
    rule.insert("decision", toml_edit::value("allow"));
    rule.insert("priority", toml_edit::value(900));
    let mut rules = toml_edit::ArrayOfTables::new();
    rules.push(rule);
    doc.insert("rule", toml_edit::Item::ArrayOfTables(rules));
    format!(
        "# 由 Meowo 管理：Meowo 的发图工具 send_image 只把图片显示给用户，免逐次确认。\n{doc}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("meowo-mcp-config-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 卸载撤销：对每个声明了「数据目录配置文件」的插件，摘掉 mcpServers.meowo、删掉 Meowo 的审批
    /// 规则 / 策略文件，用户自己的服务器、设置、规则原样保留。数据目录换成临时目录，不碰真实配置。
    #[test]
    fn uninstall_cleanup_removes_only_meowo_entries() {
        let base = temp("uninstall");
        let user_rule = "default_model = 'keep'\n\n[[permission.rules]]\ndecision = 'deny'\npattern = 'Bash(rm *)'\n";
        let mut covered = Vec::new();
        for plugin in crate::all() {
            let Some(crate::McpAttach::DataDirJson { rel, approval, .. }) = plugin.mcp_attach() else {
                continue;
            };
            covered.push(plugin.id().as_str());
            let home = base.join(plugin.id().as_str());
            std::fs::create_dir_all(home.join(rel).parent().unwrap()).unwrap();
            std::fs::write(
                home.join(rel),
                r#"{"theme":"dark","mcpServers":{"meowo":{"command":"C:/m/meowo-reporter.exe","args":["mcp"]},"mine":{"command":"x"}}}"#,
            )
            .unwrap();
            match approval {
                Some(crate::McpApproval::PermissionRules { rel, prefix }) => {
                    std::fs::write(home.join(rel), user_rule).unwrap();
                    sync_permission_rule(&home.join(rel), prefix, true);
                    assert!(std::fs::read_to_string(home.join(rel)).unwrap().contains(RULE_REASON));
                }
                Some(crate::McpApproval::PolicyFile { rel }) => {
                    std::fs::create_dir_all(home.join(rel).parent().unwrap()).unwrap();
                    sync_policy_file(&home.join(rel), true);
                    assert!(home.join(rel).exists());
                }
                None => {}
            }
        }
        assert!(covered.contains(&"gemini") && covered.contains(&"kimi"), "{covered:?}");

        let report = uninstall_cleanup_with(|plugin| vec![base.join(plugin.id().as_str())]);
        assert!(!report.is_empty());
        for plugin in crate::all() {
            let Some(crate::McpAttach::DataDirJson { rel, approval, .. }) = plugin.mcp_attach() else {
                continue;
            };
            let home = base.join(plugin.id().as_str());
            let config: Value = serde_json::from_str(&std::fs::read_to_string(home.join(rel)).unwrap()).unwrap();
            assert!(config["mcpServers"].get("meowo").is_none(), "{config}");
            assert_eq!(config["mcpServers"]["mine"]["command"], "x");
            assert_eq!(config["theme"], "dark");
            match approval {
                Some(crate::McpApproval::PermissionRules { rel, .. }) => {
                    let text = std::fs::read_to_string(home.join(rel)).unwrap();
                    assert!(!text.contains(RULE_REASON), "{text}");
                    assert!(text.contains("Bash(rm *)") && text.contains("default_model"), "{text}");
                }
                Some(crate::McpApproval::PolicyFile { rel }) => assert!(!home.join(rel).exists()),
                None => {}
            }
        }
        // 再跑一次：没什么可做的，什么都不改。
        assert!(uninstall_cleanup_with(|plugin| vec![base.join(plugin.id().as_str())]).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// gemini 的 settings.json 允许注释（JSONC），serde_json 读不懂：不改写、原样保留，并告诉
    /// 调用方「读不懂」（宿主据此写运行日志、在描述符里给前端标记）。
    #[test]
    fn jsonc_config_is_left_alone_and_reported() {
        let dir = temp("jsonc");
        let path = dir.join("settings.json");
        let original = "{\n  // 用户写的注释\n  \"theme\": \"dark\"\n}\n";
        std::fs::write(&path, original).unwrap();
        assert_eq!(sync_mcp_file(&path, Some(json!({"command": "x"}))), McpFileSync::Unparsable);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(sync_mcp_file(&path, None), McpFileSync::Unparsable);
        assert_eq!(
            cleanup_home(&dir, "settings.json", None),
            vec![format!("skipped (unparsable): {}", path.display())]
        );

        set_unreadable("gemini-test", Some(path.to_string_lossy().into_owned()));
        assert_eq!(unreadable("gemini-test").as_deref(), Some(path.to_string_lossy().as_ref()));
        set_unreadable("gemini-test", None);
        assert_eq!(unreadable("gemini-test"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
