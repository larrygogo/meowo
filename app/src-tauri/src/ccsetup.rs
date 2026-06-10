//! 无感适配：cc-app 启动时幂等地把 cc-reporter 接入 Claude Code 的 `~/.claude/settings.json`：
//!   1) 确保 5 个 hook 事件指向 cc-reporter（缺则补、路径变则更新）；
//!   2) 把 statusLine 包成「先写库再跑原 statusLine」的脚本，让 Context 百分比自动有准确数据。
//!
//! 全程：解析失败即放弃、先备份、原子写、已正确则一字不改（幂等）。核心合并逻辑为纯函数，便于测试。

use serde_json::{json, Value};

/// cc-reporter 负责的 5 个 hook 事件（PreToolUse 等其它 hook 一律不碰）。
const HOOK_EVENTS: [&str; 5] = [
    "SessionStart",
    "UserPromptSubmit",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

/// Windows 路径转 bash 可用形式：`C:\a\b` -> `C:/a/b`（Git Bash 接受 `C:/...`）。
pub fn to_bash_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// 从 settings 的 hooks 里找出已配置的 cc-reporter 可执行路径（去掉外层引号）。
pub fn reporter_path_from_hooks(settings: &Value) -> Option<String> {
    let hooks = settings.get("hooks")?.as_object()?;
    for (_event, arr) in hooks {
        for entry in arr.as_array().into_iter().flatten() {
            for h in entry.get("hooks").and_then(|x| x.as_array()).into_iter().flatten() {
                if let Some(cmd) = h.get("command").and_then(|x| x.as_str()) {
                    if cmd.contains("cc-reporter") {
                        // hook 的 command 就是带引号的可执行路径（无参数）。
                        let p = cmd.trim().trim_matches('"').to_string();
                        if !p.is_empty() {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    None
}

/// 某 hook 事件的数组里是否已有指向 cc-reporter 的条目。返回该 hook 的可变引用（用于更新路径）。
fn find_reporter_hook(event_arr: &mut [Value]) -> Option<&mut Value> {
    for entry in event_arr.iter_mut() {
        if let Some(hs) = entry.get_mut("hooks").and_then(|x| x.as_array_mut()) {
            for h in hs.iter_mut() {
                if h.get("command").and_then(|x| x.as_str()).is_some_and(|c| c.contains("cc-reporter")) {
                    return Some(h);
                }
            }
        }
    }
    None
}

/// 确保 5 个事件都挂上 cc-reporter（`reporter_native` 为本机路径，hook 的 command 用 `"<path>"`）。
/// 返回是否有改动。保留事件上的其它 hook（如 PreToolUse 的 node 预检不在管辖事件内，天然不动）。
pub fn ensure_hooks(settings: &mut Value, reporter_native: &str) -> bool {
    let desired_cmd = format!("\"{reporter_native}\"");
    let mut changed = false;

    if !settings.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        settings["hooks"] = json!({});
        changed = true;
    }
    for event in HOOK_EVENTS {
        let arr = settings["hooks"]
            .as_object_mut()
            .unwrap()
            .entry(event.to_string())
            .or_insert_with(|| json!([]));
        let arr = match arr.as_array_mut() {
            Some(a) => a,
            None => {
                *arr = json!([]);
                arr.as_array_mut().unwrap()
            }
        };
        match find_reporter_hook(arr) {
            Some(h) => {
                if h.get("command").and_then(|x| x.as_str()) != Some(desired_cmd.as_str()) {
                    h["command"] = json!(desired_cmd);
                    changed = true;
                }
            }
            None => {
                arr.push(json!({
                    "matcher": "*",
                    "hooks": [{ "type": "command", "command": desired_cmd, "timeout": 5 }]
                }));
                changed = true;
            }
        }
    }
    changed
}

/// 确保 statusLine 包成我们的脚本。`script_invocation` 形如 `bash "C:/Users/.../statusline.sh"`。
/// 返回值：
///   - Some(inner)：本次需要（重新）生成脚本，inner 是要内嵌的原 statusLine 命令（无则空串）；
///   - None：已是我们的包装，幂等跳过，不重生成脚本（避免把包装再包一层导致递归）。
///
/// `script_marker` 为我们脚本的实际路径（必出现在 `script_invocation` 里）；用它判定幂等，
/// 杜绝「把自己的包装再当 inner 捕获」的递归。
pub fn ensure_statusline(settings: &mut Value, script_invocation: &str, script_marker: &str) -> Option<String> {
    let cur = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(|x| x.as_str());
    if let Some(c) = cur {
        if c.contains(script_marker) {
            return None; // 已引用我们的脚本 → 幂等，不动（也避免递归）
        }
    }
    let inner = cur.unwrap_or("").to_string();
    settings["statusLine"] = json!({ "type": "command", "command": script_invocation });
    Some(inner)
}

/// 生成包装脚本内容：读 stdin → 喂 cc-reporter 写库（丢弃其输出）→ 跑原 statusLine（如有）渲染状态栏。
/// `reporter_bash` 为 bash 形式的 cc-reporter 路径；`inner` 为原 statusLine 命令（空则不渲染）。
pub fn build_script(reporter_bash: &str, inner: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str("# 本文件由 cc-kanban 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n");
    s.push_str("input=$(cat)\n");
    if inner.trim().is_empty() {
        // 无下游 statusLine：cc-reporter 写库并自渲染极简状态栏（输出即状态栏）。
        s.push_str(&format!("printf '%s' \"$input\" | \"{reporter_bash}\" statusline\n"));
    } else {
        // 有下游（如 claude-hud）：cc-reporter 只写库（丢弃输出），再跑下游渲染真正的状态栏。
        s.push_str(&format!(
            "printf '%s' \"$input\" | \"{reporter_bash}\" statusline >/dev/null 2>&1\n"
        ));
        s.push_str(&format!("printf '%s' \"$input\" | {inner}\n"));
    }
    s
}

/// 解析 settings.json 文本。容忍 UTF-8 BOM——Windows 上不少编辑器/PowerShell 写出的
/// JSON 带 BOM，serde_json 会直接报错，曾导致无感接线静默失败。
fn parse_settings(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// `~/.claude/settings.json`（尊重 CLAUDE_CONFIG_DIR）。
fn claude_settings_path() -> std::path::PathBuf {
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".into());
            std::path::PathBuf::from(home).join(".claude")
        });
    base.join("settings.json")
}

/// 解析 cc-reporter 本机路径：优先 app 可执行同目录（打包态会把二进制放一起），
/// 其次复用现有 hooks 里已配置的路径（dev / 已安装）。两者都没有则放弃自动接线。
fn resolve_reporter_native(settings: &Value) -> Option<String> {
    // 1) 优先复用现有 hooks 里已配置且仍存在的路径——不折腾用户正在工作的配置（幂等关键）。
    if let Some(p) = reporter_path_from_hooks(settings) {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    // 2) 否则用 app 可执行同目录（全新安装/打包态：cc-reporter 与 app 放一起）。
    let bin = if cfg!(windows) { "cc-reporter.exe" } else { "cc-reporter" };
    if let Ok(exe) = std::env::current_exe() {
        let sib = exe.with_file_name(bin);
        if sib.exists() {
            return Some(sib.to_string_lossy().into_owned());
        }
    }
    None
}

/// 启动时调用：幂等地把 cc-reporter 接入 Claude Code 的 settings.json（hooks + statusLine）。
/// 全程 best-effort：读不到 / 解析失败 / 找不到二进制都静默返回，绝不影响应用启动，绝不写坏文件。
pub fn apply() {
    let settings_path = claude_settings_path();
    let text = match std::fs::read_to_string(&settings_path) {
        Ok(t) => t,
        // 没有 settings.json：从空配置创建（刚装 Claude Code、没改过设置的用户就没有这个文件，
        // 以前直接放弃导致接线永远不发生）。仅当 ~/.claude 目录已存在（CC 确实装过）才创建，
        // 避免在没装 CC 的机器上凭空造目录和文件。
        Err(_) => {
            if !settings_path.parent().is_some_and(|p| p.is_dir()) {
                return;
            }
            "{}".to_string()
        }
    };
    let Some(mut settings) = parse_settings(&text) else {
        return; // 解析失败 → 绝不覆盖用户文件
    };
    let orig = settings.clone();

    let Some(reporter_native) = resolve_reporter_native(&settings) else {
        return; // 找不到 cc-reporter 二进制 → 无法接线
    };

    ensure_hooks(&mut settings, &reporter_native);

    // 包装脚本路径：~/.cc-kanban/statusline.sh（与 board.db 同目录）。
    let script_path = crate::db_path().with_file_name("statusline.sh");
    let script_bash = to_bash_path(&script_path.to_string_lossy());
    let invocation = format!("bash \"{script_bash}\"");
    if let Some(inner) = ensure_statusline(&mut settings, &invocation, &script_bash) {
        let script = build_script(&to_bash_path(&reporter_native), &inner);
        if let Some(parent) = script_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let _ = std::fs::write(&script_path, script);
    }

    if settings == orig {
        return; // 已是目标状态 → 一字不改
    }

    // 备份原文件，再原子写（先写临时文件后 rename）。
    let _ = std::fs::copy(&settings_path, settings_path.with_extension("json.cckb-bak"));
    if let Ok(pretty) = serde_json::to_string_pretty(&settings) {
        let tmp = settings_path.with_extension("json.cckb-tmp");
        if std::fs::write(&tmp, format!("{pretty}\n")).is_ok() {
            let _ = std::fs::rename(&tmp, &settings_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_hooks_adds_all_events_when_empty() {
        let mut v = json!({});
        assert!(ensure_hooks(&mut v, r"C:\x\cc-reporter.exe"));
        for e in HOOK_EVENTS {
            let cmd = v["hooks"][e][0]["hooks"][0]["command"].as_str().unwrap();
            assert_eq!(cmd, "\"C:\\x\\cc-reporter.exe\"");
        }
        // 再跑一次：幂等，无改动。
        assert!(!ensure_hooks(&mut v, r"C:\x\cc-reporter.exe"));
    }

    #[test]
    fn ensure_hooks_updates_changed_path_and_keeps_other_hooks() {
        // 某事件上已有一个别的 hook + 一个旧路径的 cc-reporter。
        let mut v = json!({
            "hooks": {
                "SessionStart": [
                    { "matcher": "*", "hooks": [{ "type": "command", "command": "node other.js" }] },
                    { "matcher": "*", "hooks": [{ "type": "command", "command": "\"C:\\old\\cc-reporter.exe\"", "timeout": 5 }] }
                ]
            }
        });
        assert!(ensure_hooks(&mut v, r"C:\new\cc-reporter.exe"));
        // 别的 hook 还在
        assert_eq!(v["hooks"]["SessionStart"][0]["hooks"][0]["command"], "node other.js");
        // cc-reporter 路径被更新
        assert_eq!(
            v["hooks"]["SessionStart"][1]["hooks"][0]["command"],
            "\"C:\\new\\cc-reporter.exe\""
        );
        // 没有重复追加（该事件仍是 2 条）
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn ensure_statusline_wraps_existing_and_is_idempotent() {
        let mut v = json!({ "statusLine": { "type": "command", "command": "bash -c 'claude-hud'" } });
        let marker = "C:/Users/me/.cc-kanban/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        let inner = ensure_statusline(&mut v, &inv, marker).expect("应需要生成脚本");
        assert_eq!(inner, "bash -c 'claude-hud'"); // 捕获到原命令
        assert_eq!(v["statusLine"]["command"], inv);
        // 再跑一次：已引用我们的脚本 → None（幂等，不再重复捕获/递归）
        assert!(ensure_statusline(&mut v, &inv, marker).is_none());
    }

    #[test]
    fn ensure_statusline_handles_absent() {
        let mut v = json!({});
        let marker = "/home/me/.cc-kanban/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        let inner = ensure_statusline(&mut v, &inv, marker).expect("无 statusLine 也应设置");
        assert_eq!(inner, ""); // 无原命令
        assert_eq!(v["statusLine"]["command"], inv);
    }

    #[test]
    fn build_script_with_and_without_inner() {
        let with = build_script("C:/x/cc-reporter.exe", "bash -c 'hud'");
        assert!(with.contains("\"C:/x/cc-reporter.exe\" statusline >/dev/null"));
        assert!(with.contains("| bash -c 'hud'\n"));
        let without = build_script("C:/x/cc-reporter.exe", "  ");
        // 无 inner：让 cc-reporter 自渲染（不丢弃输出）
        assert!(without.contains("| \"C:/x/cc-reporter.exe\" statusline\n"));
        assert!(!without.contains(">/dev/null"));
    }

    /// dry-run：对 CLAUDE_CONFIG_DIR/settings.json（真实文件的副本）跑 apply()，打印结果。
    /// 用法：CLAUDE_CONFIG_DIR=<tmp> CC_KANBAN_DB=<tmp/board.db> \
    ///       cargo test -p cc-app ccsetup::tests::dryrun_against_copy -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dryrun_against_copy() {
        super::apply();
        let p = super::claude_settings_path();
        let after = std::fs::read_to_string(&p).expect("读不回 settings.json");
        let v: Value = serde_json::from_str(&after).expect("结果不是合法 JSON");
        eprintln!("=== 顶层键（应全部保留）===\n{:?}", v.as_object().unwrap().keys().collect::<Vec<_>>());
        eprintln!("=== statusLine ===\n{}", serde_json::to_string_pretty(&v["statusLine"]).unwrap());
        eprintln!("=== hooks 事件 ===\n{:?}", v["hooks"].as_object().unwrap().keys().collect::<Vec<_>>());
        eprintln!("=== PreToolUse（应原封不动）===\n{}", v["hooks"]["PreToolUse"][0]["hooks"][0]["command"]);
        eprintln!("=== SessionStart cc-reporter（应不变）===\n{}", v["hooks"]["SessionStart"][0]["hooks"][0]["command"]);
        assert!(v["statusLine"]["command"].as_str().unwrap().contains("statusline.sh"));
        assert!(v["hooks"]["PreToolUse"].is_array());
    }

    #[test]
    fn real_shape_user_settings_merge() {
        // 精确复刻用户 settings.json 结构：PreToolUse(node) + 5 个 cc-reporter 事件 + claude-hud statusLine。
        let ccr = r"C:\Users\larry\Desktop\workspace\cc-kanban\target\release\cc-reporter.exe";
        let ccr_cmd = format!("\"{ccr}\"");
        let mut v = json!({
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type":"command", "command":"node \"x/pre-commit-check.cjs\"", "timeout":5000 }] }],
                "SessionStart": [{ "matcher":"*", "hooks":[{ "type":"command", "command": ccr_cmd, "timeout":5 }] }],
                "UserPromptSubmit": [{ "matcher":"*", "hooks":[{ "type":"command", "command": ccr_cmd, "timeout":5 }] }],
                "PostToolUse": [{ "matcher":"*", "hooks":[{ "type":"command", "command": ccr_cmd, "timeout":5 }] }],
                "Stop": [{ "matcher":"*", "hooks":[{ "type":"command", "command": ccr_cmd, "timeout":5 }] }],
                "SessionEnd": [{ "matcher":"*", "hooks":[{ "type":"command", "command": ccr_cmd, "timeout":5 }] }]
            },
            "statusLine": { "type":"command", "command":"bash -c 'claude-hud stuff'" }
        });
        // hooks 已全部正确 → 幂等无改动
        assert!(!ensure_hooks(&mut v, ccr));
        // PreToolUse 的 node 预检原封不动
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "node \"x/pre-commit-check.cjs\"");
        // statusLine 被包装，捕获到原 claude-hud
        let marker = "C:/Users/larry/.cc-kanban/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        assert_eq!(ensure_statusline(&mut v, &inv, marker).as_deref(), Some("bash -c 'claude-hud stuff'"));
        assert_eq!(v["statusLine"]["command"], inv);
        // 再跑一次幂等：不再重复捕获
        assert!(ensure_statusline(&mut v, &inv, marker).is_none());
    }

    #[test]
    fn parse_settings_tolerates_utf8_bom() {
        let with_bom = "\u{feff}{\"hooks\":{}}";
        let v = parse_settings(with_bom).expect("带 BOM 的 JSON 应能解析");
        assert!(v["hooks"].is_object());
        // 无 BOM 照常
        assert!(parse_settings("{}").is_some());
        // 真正的坏 JSON 仍拒绝
        assert!(parse_settings("{not json").is_none());
    }

    #[test]
    fn reporter_path_extracted_from_hooks() {
        let v = json!({
            "hooks": { "Stop": [{ "matcher": "*", "hooks": [
                { "type": "command", "command": "\"C:\\a\\b\\cc-reporter.exe\"", "timeout": 5 }
            ] }] }
        });
        assert_eq!(reporter_path_from_hooks(&v).as_deref(), Some(r"C:\a\b\cc-reporter.exe"));
    }
}
