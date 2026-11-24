//! 安装状态在启动/设置操作时刷新；普通会话启动只读内存，不再遍历CLI配置目录。

use meowo_protocol::ipc::LeadProviderInstallationDto;
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

type InstallationKey = (String, Option<String>);
static INSTALLED: LazyLock<Mutex<HashSet<InstallationKey>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

pub(crate) fn replace_installations(states: &[LeadProviderInstallationDto]) {
    if let Ok(mut cache) = INSTALLED.lock() {
        *cache = states
            .iter()
            .filter(|state| state.installed && state.supported)
            .map(|state| (state.provider.clone(), state.profile_id.clone()))
            .collect();
    }
}

pub(crate) fn update_installation(state: &LeadProviderInstallationDto) {
    if let Ok(mut cache) = INSTALLED.lock() {
        let key = (state.provider.clone(), state.profile_id.clone());
        if state.installed && state.supported {
            cache.insert(key);
        } else {
            cache.remove(&key);
        }
    }
}

pub(crate) fn enabled(provider: &str, env: &[(String, String)]) -> bool {
    let profile = env
        .iter()
        .find(|(key, _)| key == "MEOWO_PROFILE")
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
        .cloned();
    INSTALLED
        .lock()
        .is_ok_and(|cache| cache.contains(&(provider.to_owned(), profile)))
}

/// 让 CLI 能写本项目的 Lead 目录：任务文件改存 Meowo 数据目录后在工作目录之外（任务
/// 20261124-lead-store-in-meowo）。只在这家装了 Lead 时放行，且只放行本项目这一个目录。
/// 返回要追加的启动参数；走内联配置的 agent（opencode）改写 `extra_env` 里的配置变量，与浏览器 MCP
/// 已合并好的内容叠在一起。
pub(crate) fn dir_grant(
    provider: &str,
    env: &[(String, String)],
    cwd: Option<&str>,
    extra_env: &mut Vec<(String, String)>,
) -> Vec<String> {
    use meowo_agent::lead_paths::LeadDirGrant;
    if !enabled(provider, env) {
        return Vec::new();
    }
    let (Some(plugin), Some(cwd)) = (
        meowo_agent::by_id(provider),
        cwd.filter(|c| !c.trim().is_empty()),
    ) else {
        return Vec::new();
    };
    let Some(dir) = meowo_agent::lead_paths::ensure_lead_home(std::path::Path::new(cwd)) else {
        return Vec::new();
    };
    match plugin.lead_dir_grant(&dir) {
        Some(LeadDirGrant::Args(args)) => args,
        Some(LeadDirGrant::InlineConfig(fragment)) => {
            if let Some(meowo_agent::McpAttach::InlineConfigEnv { env: key }) = plugin.mcp_attach() {
                merge_inline(key, &fragment, env, extra_env);
            }
            Vec::new()
        }
        None => Vec::new(),
    }
}

/// 现有值按「本次已合并的 → 会话环境 → 进程环境」取最新的一份；不是 JSON 对象（用户手写坏了）时不碰。
fn merge_inline(
    key: &str,
    fragment: &serde_json::Value,
    env: &[(String, String)],
    extra_env: &mut Vec<(String, String)>,
) {
    let existing = extra_env
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .or_else(|| env.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
        .or_else(|| std::env::var(key).ok());
    let mut root = match existing.filter(|s| !s.trim().is_empty()) {
        Some(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) if value.is_object() => value,
            _ => return,
        },
        None => serde_json::json!({}),
    };
    deep_merge(&mut root, fragment);
    extra_env.retain(|(k, _)| k != key);
    extra_env.push((key.to_owned(), root.to_string()));
}

/// 对象逐键合并；目标那一格不是对象（如 `permission: "allow"` 这种用户定的全局值）时保留用户的，不改写。
fn deep_merge(target: &mut serde_json::Value, fragment: &serde_json::Value) {
    let (Some(target), Some(fragment)) = (target.as_object_mut(), fragment.as_object()) else {
        return;
    };
    for (key, value) in fragment {
        match target.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => deep_merge(existing, value),
            Some(_) => {}
            None => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

pub(crate) fn launch_args(
    provider: &str,
    env: &[(String, String)],
    session: Option<crate::browser_agent::McpSessionEnv<'_>>,
) -> Vec<String> {
    if !enabled(provider, env) {
        return Vec::new();
    }
    let Some(plugin) = meowo_agent::by_id(provider) else {
        return Vec::new();
    };
    let Some(cap) = plugin.lead() else {
        return Vec::new();
    };
    if cap.binding_policy() != meowo_agent::lead_install::LeadBindingPolicy::NativeThreadMeta {
        return Vec::new();
    }
    let (Some(meowo_agent::McpAttach::TomlOverride { flag }), Some(session), Some(reporter)) = (
        plugin.mcp_attach(),
        session,
        crate::setup::sibling_reporter(),
    ) else {
        return Vec::new();
    };
    toml_args(flag, &reporter, session)
}

fn toml_args(
    flag: &str,
    reporter: &str,
    session: crate::browser_agent::McpSessionEnv<'_>,
) -> Vec<String> {
    let mut env = toml_edit::InlineTable::new();
    env.insert("MEOWO_PTY_ENDPOINT", session.endpoint.into());
    env.insert("MEOWO_PTY_TOKEN", session.token.into());
    env.insert("MEOWO_PTY_PROTOCOL", session.protocol.to_string().into());
    // 只给本次原生thread设置，不能继承共享daemon的旧令牌；8天超时容纳手机离线等待。
    [
        format!(
            "mcp_servers.meowo_lead.command={}",
            toml_edit::Value::from(reporter)
        ),
        "mcp_servers.meowo_lead.args=['lead-mcp']".to_owned(),
        format!("mcp_servers.meowo_lead.env={env}"),
        "mcp_servers.meowo_lead.tool_timeout_sec=691200".to_owned(),
    ]
    .into_iter()
    .flat_map(|value| [flag.to_owned(), value])
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_binding_survives_quoted_paths_and_sets_a_long_native_timeout() {
        let args = toml_args(
            "-c",
            "C:\\O'Neil\\reporter.exe",
            crate::browser_agent::McpSessionEnv {
                endpoint: "127.0.0.1:1234",
                token: "test-'quoted",
                protocol: 2,
            },
        );
        let body = args
            .chunks_exact(2)
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let doc = body.parse::<toml_edit::DocumentMut>().unwrap();
        let server = &doc["mcp_servers"]["meowo_lead"];
        assert_eq!(server["command"].as_str(), Some("C:\\O'Neil\\reporter.exe"));
        assert_eq!(
            server["env"]["MEOWO_PTY_TOKEN"].as_str(),
            Some("test-'quoted")
        );
        assert_eq!(server["args"][0].as_str(), Some("lead-mcp"));
        assert_eq!(server["tool_timeout_sec"].as_integer(), Some(691200));
    }
}

#[cfg(test)]
mod dir_grant_tests {
    use super::*;

    #[test]
    fn inline_merge_keeps_user_values_and_adds_lead_dir() {
        let fragment = serde_json::json!({"permission": {"external_directory": {"C:/d/**": "allow"}}});
        let mut extra = vec![(
            "OPENCODE_CONFIG_CONTENT".to_owned(),
            r#"{"mcp":{"meowo":{}},"permission":{"edit":"ask","external_directory":{"/x/**":"deny"}}}"#.to_owned(),
        )];
        merge_inline("OPENCODE_CONFIG_CONTENT", &fragment, &[], &mut extra);
        assert_eq!(extra.len(), 1);
        let v: serde_json::Value = serde_json::from_str(&extra[0].1).unwrap();
        assert_eq!(v["mcp"]["meowo"], serde_json::json!({}));
        assert_eq!(v["permission"]["edit"], "ask");
        assert_eq!(v["permission"]["external_directory"]["/x/**"], "deny");
        assert_eq!(v["permission"]["external_directory"]["C:/d/**"], "allow");
    }

    #[test]
    fn inline_merge_respects_global_permission_string_and_broken_json() {
        let fragment = serde_json::json!({"permission": {"external_directory": {"C:/d/**": "allow"}}});
        let mut extra = vec![("K".to_owned(), r#"{"permission":"allow"}"#.to_owned())];
        merge_inline("K", &fragment, &[], &mut extra);
        assert_eq!(extra[0].1, r#"{"permission":"allow"}"#);
        let mut broken = Vec::new();
        merge_inline("K", &fragment, &[("K".to_owned(), "{oops".to_owned())], &mut broken);
        assert!(broken.is_empty());
    }
}
