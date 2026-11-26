use super::*;

#[test]
fn process_scope_owns_agy_and_shares_acp() {
    use crate::registry::ProcessScope;
    assert_eq!(Antigravity.process_scope(&[]), ProcessScope::Unknown);
    assert_eq!(
        Antigravity.process_scope(&["node".into(), "agy.js".into()]),
        ProcessScope::Unknown
    );
    let argv: Vec<String> = vec![r"C:\Users\u\AppData\Local\agy\bin\agy.exe".into(), "--conversation".into()];
    assert_eq!(Antigravity.process_scope(&argv), ProcessScope::Exclusive);
    assert_eq!(
        Antigravity.process_scope(&["agy".into(), "acp".into()]),
        ProcessScope::Shared
    );
}

/// 防连坐绊线：只写 agy 认识的事件。
#[test]
fn events_all_in_upstream_whitelist() {
    for ev in EVENTS {
        assert!(EVENT_WHITELIST.contains(&ev.name), "{} 不在 agy 事件白名单", ev.name);
    }
}

/// agy 的 timeout 是秒（默认 30）。照抄 gemini 的毫秒值会让每个 hook 都挂 1 个多小时。
#[test]
fn hook_timeouts_are_seconds() {
    for ev in EVENTS {
        assert!((1..=60).contains(&ev.timeout), "{} 的 timeout={}", ev.name, ev.timeout);
    }
}

/// Windows 上 agy 经 cmd /c 执行：不能带 PowerShell 的 `& `。
#[test]
fn hook_command_is_cmd_invocable() {
    let cmd = HOOKS.command.render(r"C:\x\meowo-reporter.exe", "antigravity");
    assert_eq!(cmd, r#""C:\x\meowo-reporter.exe" --provider antigravity"#);
}

/// hooks.json 只占顶层 `meowo` 一块，用户自己的命名 hook 原样保留；工具事件 matcher 分组，
/// 生命周期事件直接是 handler 列表（agy 内置 hooks 文档的结构）；再接一次不改文件。
#[test]
fn hooks_json_keeps_user_hooks_and_is_idempotent() {
    let user = r#"{"mine":{"enabled":true,"Stop":[{"command":"echo done"}]}}"#;
    let crate::config::EnsureOutcome::Changed(text) =
        HOOKS.ensure_hooks(user, r"C:\x\meowo-reporter.exe", "antigravity")
    else {
        panic!("应写入");
    };
    let root: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(root["mine"]["Stop"][0]["command"], "echo done");
    let ours = &root[crate::config::AGY_HOOK_NAME];
    assert_eq!(ours["enabled"], true);
    assert_eq!(ours["Stop"][0]["type"], "command");
    assert_eq!(ours["PostToolUse"][0]["matcher"], "*");
    assert!(ours["PostToolUse"][0]["hooks"][0]["command"].is_string());
    assert!(HOOKS.has_reporter(&text, "antigravity"));
    assert_eq!(
        HOOKS.ensure_hooks(&text, r"C:\x\meowo-reporter.exe", "antigravity"),
        crate::config::EnsureOutcome::Unchanged
    );
    // agy 会先建 0 字节的配置文件。
    assert!(matches!(
        HOOKS.ensure_hooks("", r"C:\x\meowo-reporter.exe", "antigravity"),
        crate::config::EnsureOutcome::Changed(_)
    ));
}

#[test]
fn hooks_file_is_the_global_config_next_to_the_data_dir() {
    let home = std::env::temp_dir().join(format!("meowo-agy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(crate::join_rel(&home, DATA_DIR_REL)).unwrap();
    let inst = VARIANTS[0].probe(id::ANTIGRAVITY, &home).expect("数据目录在 → 应命中");
    let config = inst.config_path();
    assert!(config.ends_with("config/hooks.json") || config.ends_with(r"config\hooks.json"));
    let normalized = std::path::Path::new(&home).join(".gemini").join("config").join("hooks.json");
    assert_eq!(
        std::fs::canonicalize(config.parent().unwrap().parent().unwrap()).ok(),
        std::fs::canonicalize(normalized.parent().unwrap().parent().unwrap()).ok()
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn resume_uses_conversation_flag() {
    assert_eq!(Antigravity.resume_args(), ["--conversation"]);
}

/// Lead 目录用 `--add-dir` 加进工作区，否则写任务文件要逐个批（#7 真机测试 2026-11-26）；
/// 参数只在 1.5.18 上查证过，更老的版本不加。
#[test]
fn lead_dir_joins_workspace_via_add_dir() {
    let dir = std::path::Path::new("C:/Users/user/.meowo/projects/p/.lead");
    match Antigravity.lead_dir_grant(dir, &[]) {
        Some(crate::lead_paths::LeadDirGrant::Args(args)) => {
            assert_eq!(args, ["--add-dir", "C:/Users/user/.meowo/projects/p/.lead"]);
        }
        _ => panic!("应当给 --add-dir 参数"),
    }
    assert_eq!(Antigravity.lead_dir_grant_floor(), Some((1, 5, 18)));
}
