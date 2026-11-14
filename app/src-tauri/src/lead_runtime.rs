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
