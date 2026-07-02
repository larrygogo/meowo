//! 无感适配：cc-app 启动时幂等地把 cc-reporter 接入 Claude Code 的 `~/.claude/settings.json`：
//!   1) 确保 8 个 hook 事件指向 cc-reporter（缺则补、路径变则更新）；
//!   2) 把 statusLine 包成「先写库再跑原 statusLine」的脚本，让 Context 百分比自动有准确数据。
//!
//! 全程：解析失败即放弃、先备份、原子写、已正确则一字不改（幂等）。核心合并逻辑为纯函数，便于测试。

use serde_json::{json, Value};

/// cc-reporter 负责的 hook 事件 + matcher。PreToolUse 用 matcher 限定只在两种工具触发,
/// 与用户自有 PreToolUse(如 Bash 预检)按 matcher 区分共存。
/// 注意：此表须与 scripts/install-hooks.mjs 的 SPECS 保持一致。
const HOOK_SPECS: [(&str, &str); 8] = [
    ("SessionStart", "*"),
    ("UserPromptSubmit", "*"),
    ("PostToolUse", "*"),
    ("Stop", "*"),
    ("SessionEnd", "*"),
    ("PermissionRequest", "*"),
    ("PreToolUse", "AskUserQuestion"),
    ("PreToolUse", "ExitPlanMode"),
];

/// Windows 路径转 bash 可用形式：`C:\a\b` -> `C:/a/b`（Git Bash 接受 `C:/...`）。
pub fn to_bash_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// 严格判定一条 hook command 是否是「我们写入的 cc-reporter 可执行」并返回其路径：
/// 必须是单个（可带引号的）可执行路径、**无额外参数**，且文件名恰为 cc-reporter[.exe]。
/// 不用裸 contains —— 否则会误伤用户自有 hook（如 `node tools/cc-reporter-notify.js`，
/// 或路径里恰好含 cc-reporter 目录），把人家的命令静默改写坏。纯函数便于单测。
pub fn reporter_exe_path(cmd: &str) -> Option<String> {
    let c = cmd.trim();
    let path = if let Some(rest) = c.strip_prefix('"') {
        // 形如 "<path>"：取首尾引号之间，引号后不能再有内容（否则是带参数）。
        let inner = rest.strip_suffix('"')?;
        if inner.contains('"') {
            return None;
        }
        inner
    } else {
        // 裸路径：不能含空白（含空白 = 带参数/是命令而非单可执行）。
        if c.chars().any(char::is_whitespace) {
            return None;
        }
        c
    };
    let name = std::path::Path::new(path).file_name()?.to_str()?;
    (name.eq_ignore_ascii_case("cc-reporter") || name.eq_ignore_ascii_case("cc-reporter.exe"))
        .then(|| path.to_string())
}

/// 从 settings 的 hooks 里找出已配置的 cc-reporter 可执行路径。
pub fn reporter_path_from_hooks(settings: &Value) -> Option<String> {
    let hooks = settings.get("hooks")?.as_object()?;
    for (_event, arr) in hooks {
        for entry in arr.as_array().into_iter().flatten() {
            for h in entry.get("hooks").and_then(|x| x.as_array()).into_iter().flatten() {
                if let Some(cmd) = h.get("command").and_then(|x| x.as_str()) {
                    if let Some(p) = reporter_exe_path(cmd) {
                        return Some(p);
                    }
                }
            }
        }
    }
    None
}

/// 在某 hook 事件数组里找「matcher 等于 target_matcher 且含 cc-reporter 命令」的 entry。
/// 返回整个 entry 的可变引用(用于更新其内部 hook 的路径)。matcher 感知:
/// 同一事件下可有多条按 matcher 区分的条目(如 PreToolUse 的 Bash 预检与本程序的 AskUserQuestion)。
fn find_reporter_entry_with_matcher<'a>(
    event_arr: &'a mut [Value],
    target_matcher: &str,
) -> Option<&'a mut Value> {
    for entry in event_arr.iter_mut() {
        if entry.get("matcher").and_then(|m| m.as_str()) != Some(target_matcher) {
            continue;
        }
        let has_reporter = entry
            .get("hooks")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
            .any(|h| {
                h.get("command")
                    .and_then(|x| x.as_str())
                    .and_then(reporter_exe_path)
                    .is_some()
            });
        if has_reporter {
            return Some(entry);
        }
    }
    None
}

/// 确保 8 条 (event, matcher) 规格都挂上 cc-reporter（`reporter_native` 为本机路径，hook 的 command 用 `"<path>"`）。
/// 返回是否有改动。按 matcher 区分定位/追加，保留同一事件下用户自有的其他 matcher 条目（如 PreToolUse:Bash 预检）。
pub fn ensure_hooks(settings: &mut Value, reporter_native: &str) -> bool {
    let desired_cmd = format!("\"{reporter_native}\"");
    let mut changed = false;

    if !settings.get("hooks").map(|h| h.is_object()).unwrap_or(false) {
        settings["hooks"] = json!({});
        changed = true;
    }
    for (event, matcher) in HOOK_SPECS {
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
        match find_reporter_entry_with_matcher(arr, matcher) {
            Some(entry) => {
                // 升级该 entry 内 cc-reporter hook 的路径(matcher 不动)。
                if let Some(hs) = entry.get_mut("hooks").and_then(|x| x.as_array_mut()) {
                    for h in hs.iter_mut() {
                        let is_reporter = h
                            .get("command")
                            .and_then(|x| x.as_str())
                            .and_then(reporter_exe_path)
                            .is_some();
                        if is_reporter
                            && h.get("command").and_then(|x| x.as_str()) != Some(desired_cmd.as_str())
                        {
                            h["command"] = json!(desired_cmd);
                            changed = true;
                        }
                    }
                }
            }
            None => {
                arr.push(json!({
                    "matcher": matcher,
                    "hooks": [{ "type": "command", "command": desired_cmd, "timeout": 5 }]
                }));
                changed = true;
            }
        }
    }
    changed
}

/// 探测 statusLine 接线状态（只读不改）：
///   - Some(inner)：尚未指向我们的脚本，需要生成脚本并改写 settings；inner 是要内嵌的原
///     statusLine 命令（无则空串）；
///   - None：已是我们的包装，幂等跳过，不重生成脚本（避免把包装再包一层导致递归）。
///
/// 只探测不改写——settings 的实际改写由调用方在**脚本落盘成功之后**执行：先改 settings 再写脚本、
/// 写失败再回滚的顺序会在回滚代码里反向编码本函数的副作用，脆弱且曾造成「settings 指向不存在的
/// 脚本、原 statusLine 命令永久丢失」。
///
/// `script_marker` 为我们脚本的实际路径；用它判定幂等，杜绝「把自己的包装再当 inner 捕获」的递归。
pub fn probe_statusline(settings: &Value, script_marker: &str) -> Option<String> {
    let cur = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(|x| x.as_str());
    if let Some(c) = cur {
        if c.contains(script_marker) {
            return None; // 已引用我们的脚本 → 幂等，不动（也避免递归）
        }
    }
    Some(cur.unwrap_or("").to_string())
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

/// 写出 statusline 包装脚本（先建目录）。返回错误供调用方回滚 settings 的 statusLine 改动。
fn write_statusline_script(path: &std::path::Path, script: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, script)
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !settings_path.parent().is_some_and(|p| p.is_dir()) {
                return;
            }
            "{}".to_string()
        }
        // 其它读取失败（权限、非 UTF-8 编码如 UTF-16 等）：文件存在但读不了，
        // 绝不能当「不存在」处理——否则会拿空配置覆盖用户文件。
        Err(_) => return,
    };
    let Some(mut settings) = parse_settings(&text) else {
        return; // 解析失败 → 绝不覆盖用户文件
    };
    if !settings.is_object() {
        return; // 顶层不是对象（数组/标量）：后续按键索引赋值会 panic，直接放弃
    }
    let orig = settings.clone();

    let Some(reporter_native) = resolve_reporter_native(&settings) else {
        return; // 找不到 cc-reporter 二进制 → 无法接线
    };

    ensure_hooks(&mut settings, &reporter_native);

    // 包装脚本路径：~/.cc-kanban/statusline.sh（与 board.db 同目录）。
    let script_path = crate::db_path().with_file_name("statusline.sh");
    let script_bash = to_bash_path(&script_path.to_string_lossy());
    let invocation = format!("bash \"{script_bash}\"");
    match probe_statusline(&settings, &script_bash) {
        Some(inner) => {
            // 顺序关键：脚本先落盘，成功后 settings 才指向它。写失败（目录不可写/杀软拦截/磁盘满）
            // 时 settings 原样不动——否则 Claude Code 状态栏会指向不存在的脚本，用户原 statusLine
            // 命令（inner）只存在于没写出去的脚本里而永久丢失，且后续启动因幂等判定命中 marker
            // 而跳过重建、永不自愈。settings 未动则下次启动整段重试。
            let script = build_script(&to_bash_path(&reporter_native), &inner);
            if write_statusline_script(&script_path, &script).is_ok() {
                settings["statusLine"] = json!({ "type": "command", "command": invocation });
            }
        }
        None => {
            // 幂等命中（settings 已指向我们的脚本）但脚本文件缺失：用户删 ~/.cc-kanban 重置数据时
            // board.db 会被 Store::open 自动重建，本脚本却不会——不补建则状态栏每次渲染报
            // No such file、Context% 永久断供。原 inner 已无从恢复，退化为自渲染版兜底。
            if !script_path.exists() {
                let script = build_script(&to_bash_path(&reporter_native), "");
                let _ = write_statusline_script(&script_path, &script);
            }
        }
    }

    if settings == orig {
        return; // 已是目标状态 → 一字不改
    }

    // 备份原文件，再原子写（先写临时文件后 rename）。备份只在不存在时写一次——
    // 保留**最初**那份用户原始配置，避免连续启动用（可能已被我们改过的）当前文件覆盖原始备份。
    let backup = settings_path.with_extension("json.cckb-bak");
    if !backup.exists() {
        let _ = std::fs::copy(&settings_path, &backup);
    }
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
    fn reporter_exe_path_strict_matches_only_our_exe() {
        // 我们写入的形态：带引号的单可执行路径，无参数。
        // 路径用正斜杠：Windows/macOS 均把 `/` 当分隔符，file_name 跨平台一致（避免 macOS 上 `\` 不被当分隔符）。
        assert_eq!(
            reporter_exe_path("\"C:/x/cc-reporter.exe\"").as_deref(),
            Some("C:/x/cc-reporter.exe")
        );
        assert_eq!(reporter_exe_path("/usr/local/bin/cc-reporter").as_deref(), Some("/usr/local/bin/cc-reporter"));
        // 不能误伤用户自有 hook：带参数、是别的脚本、或只是路径里含子串。
        assert_eq!(reporter_exe_path("node tools/cc-reporter-notify.js"), None);
        assert_eq!(reporter_exe_path("\"C:/x/cc-reporter.exe\" --flag"), None);
        assert_eq!(reporter_exe_path("/opt/cc-reporter/run.sh"), None);
        assert_eq!(reporter_exe_path("cc-reporter-wrapper"), None);
        assert_eq!(reporter_exe_path(""), None);
        // find_reporter_entry_with_matcher 不应认领「事件内、命令含 cc-reporter 子串的用户 hook」。
        let mut arr = vec![json!({"hooks":[{"type":"command","command":"node tools/cc-reporter-notify.js"}]})];
        assert!(find_reporter_entry_with_matcher(&mut arr, "*").is_none());
    }

    #[test]
    fn ensure_hooks_adds_all_events_when_empty() {
        let mut v = json!({});
        assert!(ensure_hooks(&mut v, "C:/x/cc-reporter.exe"));
        for e in ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "SessionEnd"] {
            let cmd = v["hooks"][e][0]["hooks"][0]["command"].as_str().unwrap();
            assert_eq!(cmd, "\"C:/x/cc-reporter.exe\"");
        }
        // 再跑一次：幂等，无改动。
        assert!(!ensure_hooks(&mut v, "C:/x/cc-reporter.exe"));
    }

    #[test]
    fn ensure_hooks_updates_changed_path_and_keeps_other_hooks() {
        // 某事件上已有一个别的 hook + 一个旧路径的 cc-reporter。
        let mut v = json!({
            "hooks": {
                "SessionStart": [
                    { "matcher": "*", "hooks": [{ "type": "command", "command": "node other.js" }] },
                    { "matcher": "*", "hooks": [{ "type": "command", "command": "\"C:/old/cc-reporter.exe\"", "timeout": 5 }] }
                ]
            }
        });
        assert!(ensure_hooks(&mut v, "C:/new/cc-reporter.exe"));
        // 别的 hook 还在
        assert_eq!(v["hooks"]["SessionStart"][0]["hooks"][0]["command"], "node other.js");
        // cc-reporter 路径被更新
        assert_eq!(
            v["hooks"]["SessionStart"][1]["hooks"][0]["command"],
            "\"C:/new/cc-reporter.exe\""
        );
        // 没有重复追加（该事件仍是 2 条）
        assert_eq!(v["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn probe_statusline_wraps_existing_and_is_idempotent() {
        let mut v = json!({ "statusLine": { "type": "command", "command": "bash -c 'claude-hud'" } });
        let marker = "C:/Users/me/.cc-kanban/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        let inner = probe_statusline(&v, marker).expect("应需要生成脚本");
        assert_eq!(inner, "bash -c 'claude-hud'"); // 捕获到原命令
        assert_eq!(v["statusLine"]["command"], "bash -c 'claude-hud'"); // 探测不改写
        // 模拟 apply：脚本落盘成功后才改写 settings。
        v["statusLine"] = json!({ "type": "command", "command": inv });
        // 再探测：已引用我们的脚本 → None（幂等，不再重复捕获/递归）
        assert!(probe_statusline(&v, marker).is_none());
    }

    #[test]
    fn probe_statusline_handles_absent() {
        let v = json!({});
        let marker = "/home/me/.cc-kanban/statusline.sh";
        let inner = probe_statusline(&v, marker).expect("无 statusLine 也应接线");
        assert_eq!(inner, ""); // 无原命令
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
        let ccr = "C:/Users/larry/Desktop/workspace/cc-kanban/target/release/cc-reporter.exe";
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
        // fixture 缺少 PermissionRequest / PreToolUse(AskUserQuestion|ExitPlanMode) → 首次追加 3 条，返回 true
        assert!(ensure_hooks(&mut v, ccr));
        // PreToolUse 的 node 预检原封不动
        assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "node \"x/pre-commit-check.cjs\"");
        // PreToolUse 下已追加 AskUserQuestion / ExitPlanMode 两条 cc-reporter
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(pre.iter().any(|e| e["matcher"] == "AskUserQuestion"));
        assert!(pre.iter().any(|e| e["matcher"] == "ExitPlanMode"));
        // PermissionRequest 也已追加
        assert_eq!(v["hooks"]["PermissionRequest"][0]["matcher"], "*");
        // 再跑一次：此时才幂等
        assert!(!ensure_hooks(&mut v, ccr));
        // statusLine 探测捕获到原 claude-hud;模拟 apply 在脚本落盘成功后改写
        let marker = "C:/Users/larry/.cc-kanban/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        assert_eq!(probe_statusline(&v, marker).as_deref(), Some("bash -c 'claude-hud stuff'"));
        v["statusLine"] = json!({ "type": "command", "command": inv });
        // 再探测幂等：不再重复捕获
        assert!(probe_statusline(&v, marker).is_none());
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
                { "type": "command", "command": "\"C:/a/b/cc-reporter.exe\"", "timeout": 5 }
            ] }] }
        });
        assert_eq!(reporter_path_from_hooks(&v).as_deref(), Some("C:/a/b/cc-reporter.exe"));
    }

    #[test]
    fn ensure_hooks_adds_all_specs_including_pretooluse_matchers() {
        let mut v = json!({});
        assert!(ensure_hooks(&mut v, "C:/x/cc-reporter.exe"));
        // 5 个老事件 + PermissionRequest:matcher "*"。
        for e in ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "SessionEnd", "PermissionRequest"] {
            assert_eq!(v["hooks"][e][0]["matcher"], "*", "{e} matcher");
            assert_eq!(v["hooks"][e][0]["hooks"][0]["command"], "\"C:/x/cc-reporter.exe\"");
        }
        // PreToolUse:两条,matcher 分别 AskUserQuestion / ExitPlanMode。
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        let matchers: Vec<&str> = pre.iter().map(|e| e["matcher"].as_str().unwrap()).collect();
        assert!(matchers.contains(&"AskUserQuestion"));
        assert!(matchers.contains(&"ExitPlanMode"));
        // 幂等。
        assert!(!ensure_hooks(&mut v, "C:/x/cc-reporter.exe"));
    }

    #[test]
    fn ensure_hooks_preserves_user_pretooluse_bash() {
        // 用户自有 PreToolUse:Bash node 预检,不是 cc-reporter。
        let mut v = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "node \"x/pre-check.cjs\"" }] }
            ]}
        });
        ensure_hooks(&mut v, "C:/x/cc-reporter.exe");
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        // 原 Bash 条目原封保留。
        let bash = pre.iter().find(|e| e["matcher"] == "Bash").unwrap();
        assert_eq!(bash["hooks"][0]["command"], "node \"x/pre-check.cjs\"");
        // 且新增了 AskUserQuestion / ExitPlanMode 两条 cc-reporter。
        assert!(pre.iter().any(|e| e["matcher"] == "AskUserQuestion"));
        assert!(pre.iter().any(|e| e["matcher"] == "ExitPlanMode"));
    }
}
