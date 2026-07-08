//! codex（OpenAI Codex CLI）自动接线：幂等合并 `~/.codex/hooks.json`。
//! codex hooks 格式 Claude 同款但顶层只允许 {"hooks": {...}}（deny_unknown_fields），
//! 条目无 matcher 键；信任机制（trusted_hash）见本模块 hash/apply 部分（Task 3/4）。
use serde_json::{json, Value};

/// 接线事件集：dispatch 消化面 ∩ codex 0.142 支持面。无 SessionEnd（codex 不支持，
/// 会话收尾靠 Stop + liveness）；不配 PreToolUse（其 matcher 目标是 claude 专属工具）。
pub const CODEX_EVENTS: [&str; 5] =
    ["SessionStart", "UserPromptSubmit", "PostToolUse", "Stop", "PermissionRequest"];

/// 从 hooks.json 找出已配置的 meowo-reporter 路径（复用现有路径优先的解析源）。
pub fn reporter_path_from_codex(root: &Value) -> Option<String> {
    let hooks = root.get("hooks")?.as_object()?;
    for (_ev, arr) in hooks {
        for entry in arr.as_array().into_iter().flatten() {
            for h in entry.get("hooks").and_then(|x| x.as_array()).into_iter().flatten() {
                if let Some(p) = h
                    .get("command")
                    .and_then(|c| c.as_str())
                    .and_then(|c| super::claim_provider_cmd(c, "codex"))
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 幂等合并：CODEX_EVENTS 逐事件确保挂上 meowo-reporter。已有认领条目 → 仅路径不符时更新
/// （解析后内容判定，裸路径/引号形态视为等价，不无谓改写用户在用的配置）；缺 → 追加。
/// 返回是否有改动。
pub fn ensure_codex_hooks(root: &mut Value, reporter_native: &str) -> bool {
    let desired_cmd = format!("\"{reporter_native}\" --provider codex");
    let mut changed = false;
    match root.get("hooks") {
        None => {
            // 键不存在：hooks.json 整个文件本就可从空态建（spec 依据见模块头注释），
            // 与 kimi「键不存在也放弃」不同是有意的——此处不存在不代表用户手改过畸形内容。
            root["hooks"] = json!({});
            changed = true;
        }
        Some(h) if !h.is_object() => {
            // 键存在但非 object（如手改坏形状）：对齐 kimi 哲学，直接放弃不写，绝不覆盖用户文件。
            return false;
        }
        Some(_) => {}
    }
    for ev in CODEX_EVENTS {
        let entry_val = root["hooks"]
            .as_object_mut()
            .unwrap()
            .entry(ev.to_string())
            .or_insert_with(|| json!([]));
        let Some(arr) = entry_val.as_array_mut() else {
            // 事件值存在但非 array（畸形形状）：跳过该事件不动，不置空覆盖。
            continue;
        };
        let mut found = false;
        for entry in arr.iter_mut() {
            let Some(hs) = entry.get_mut("hooks").and_then(|x| x.as_array_mut()) else {
                continue;
            };
            for h in hs.iter_mut() {
                let claimed = h
                    .get("command")
                    .and_then(|c| c.as_str())
                    .and_then(|c| super::claim_provider_cmd(c, "codex"));
                if let Some(path) = claimed {
                    found = true;
                    if path != reporter_native {
                        h["command"] = json!(desired_cmd);
                        changed = true;
                    }
                }
            }
        }
        if !found {
            arr.push(json!({ "hooks": [{ "type": "command", "command": desired_cmd, "timeout": 5 }] }));
            changed = true;
        }
    }
    changed
}

/// 我方在 hooks.json 里认领到的一条 hook，附真实位置（group_idx = 组在该事件数组里的下标，
/// handler_idx = handler 在组内 hooks 数组里的下标）——trusted_hash 键必须落在真实位置上，
/// 写死 0:0 会在用户组共存、我方组不在最前时错把哈希覆盖到别人的槽位（详见调用处）。
pub struct ClaimedEntry {
    pub event: String,
    pub command: String,
    pub timeout: u64,
    pub group_idx: usize,
    pub handler_idx: usize,
}

/// 提取合并后全部我方认领条目，供 trusted_hash 按**实际写出的内容 + 真实位置**计算
/// （含既有条目的原样 timeout，如本机 Stop=10）。
///
/// 只认领「整组恰为单 handler 且是我方」的组：trusted_hash 的 canon 公式（见
/// `codex_hook_hash`）本身就是单 handler 形态，组内还有其他 handler 时算出来的哈希对不上
/// codex 真实校验对象——跳过不预信任，退化为 codex 一次 Trust all，无损。
pub fn claimed_codex_entries(root: &Value) -> Vec<ClaimedEntry> {
    let mut out = Vec::new();
    let Some(hooks) = root.get("hooks").and_then(|h| h.as_object()) else {
        return out;
    };
    for (ev, arr) in hooks {
        for (group_idx, entry) in arr.as_array().into_iter().flatten().enumerate() {
            let Some(hs) = entry.get("hooks").and_then(|x| x.as_array()) else {
                continue;
            };
            if hs.len() != 1 {
                continue; // 组内不止一个 handler：canon 公式套不上，跳过不预信任
            }
            let Some(cmd) = hs[0].get("command").and_then(|c| c.as_str()) else {
                continue;
            };
            if super::claim_provider_cmd(cmd, "codex").is_some() {
                // codex 源码中默认超时为 600（`.unwrap_or(600).max(1)`），同步本处默认值。
                let t = hs[0].get("timeout").and_then(|t| t.as_u64()).unwrap_or(600);
                out.push(ClaimedEntry {
                    event: ev.clone(),
                    command: cmd.to_string(),
                    timeout: t,
                    group_idx,
                    handler_idx: 0,
                });
            }
        }
    }
    out
}

/// CamelCase 事件名 → codex hooks.state 键用的 snake_case 标签。未知返回 ""。
pub(crate) fn snake_event(ev: &str) -> &'static str {
    match ev {
        "SessionStart" => "session_start",
        "UserPromptSubmit" => "user_prompt_submit",
        "PostToolUse" => "post_tool_use",
        "Stop" => "stop",
        "PermissionRequest" => "permission_request",
        _ => "",
    }
}

/// codex trusted_hash：对归一化身份对象的 canonical JSON（key 字母序、紧凑）做 SHA-256。
/// 算法为 codex 内部实现（源码 fingerprint.rs），本机 0.142.3 三真实向量验证命中；
/// 漂移时向量测试变红，运行期兜底 = codex TUI 一次 Trust all，无损。
/// 手工 format! 拼串而非 serde_json 对象：杜绝 preserve_order 特性统一导致的键序漂移。
pub(crate) fn codex_hook_hash(event_snake: &str, command: &str, timeout: u64) -> String {
    use sha2::{Digest, Sha256};
    let canon = format!(
        r#"{{"event_name":{ev},"hooks":[{{"async":false,"command":{cmd},"timeout":{timeout},"type":"command"}}]}}"#,
        ev = serde_json::to_string(event_snake).unwrap_or_default(),
        cmd = serde_json::to_string(command).unwrap_or_default(),
    );
    format!("sha256:{:x}", Sha256::digest(canon.as_bytes()))
}

/// 向 config.toml 的 [hooks.state] 写入/更新各认领条目的 trusted_hash（只动该子树）。
/// 键格式：'<hooks.json 绝对路径 display 串>:<snake_case 事件>:<group_idx>:<handler_idx>'——
/// 索引取自 entries 里携带的真实位置，而非写死 0:0：ensure_codex_hooks 只在事件数组末尾
/// 追加我方组、从不重排既有条目，故同一台机器上这个真实索引跨启动稳定，可安全做幂等比较。
pub fn ensure_trusted_hashes(
    doc: &mut toml_edit::DocumentMut,
    hooks_path_display: &str,
    entries: &[ClaimedEntry],
) -> bool {
    let mut changed = false;
    // 逐层确保 hooks / hooks.state 为隐式 table（不打扰同级既有内容）。
    if doc.get("hooks").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc.insert("hooks", toml_edit::Item::Table(t));
    }
    // hooks 键存在但非 table（畸形/误粘 kimi 数组语法）：放弃信任写入，绝不 panic——退化等价于解析失败路径
    let Some(hooks) = doc["hooks"].as_table_mut() else { return changed };
    if hooks.get("state").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        hooks.insert("state", toml_edit::Item::Table(t));
    }
    let Some(state) = hooks["state"].as_table_mut() else { return changed };
    for entry in entries {
        let snake = snake_event(&entry.event);
        if snake.is_empty() {
            continue; // 未知事件不写信任（不该发生：entries 来自我方认领条目）
        }
        let key = format!("{hooks_path_display}:{snake}:{}:{}", entry.group_idx, entry.handler_idx);
        let hash = codex_hook_hash(snake, &entry.command, entry.timeout);
        let cur = state
            .get(&key)
            .and_then(|it| it.get("trusted_hash"))
            .and_then(|v| v.as_str());
        if cur != Some(hash.as_str()) {
            let mut t = toml_edit::Table::new();
            t.insert("trusted_hash", toml_edit::value(hash));
            state.insert(&key, toml_edit::Item::Table(t));
            changed = true;
        }
    }
    changed
}

/// codex 的 ProviderSetup：先 hooks.json 落盘成功，再写 config.toml 预信任（反序会留下
/// 指向不存在配置的信任残渣；正序失败的最坏情形 = codex 弹一次审查提示，无损）。
pub struct CodexSetup;

impl super::ProviderSetup for CodexSetup {
    fn key(&self) -> meowo_store::ProviderKey {
        meowo_store::ProviderKey::Codex
    }
    fn detect(&self) -> bool {
        meowo_reporter::codex::codex_home().is_some_and(|d| d.is_dir())
    }
    fn apply(&self) {
        let Some(home) = meowo_reporter::codex::codex_home() else {
            return;
        };
        let hooks_path = home.join("hooks.json");
        // 1) hooks.json：不存在从空起；存在但读不了/解析失败 → 放弃（绝不覆盖用户文件）。
        let root_text = match std::fs::read_to_string(&hooks_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{\"hooks\":{}}".to_string(),
            Err(_) => return,
        };
        let Ok(mut root) = serde_json::from_str::<serde_json::Value>(root_text.trim_start_matches('\u{feff}')) else {
            return;
        };
        if !root.is_object() {
            return;
        }
        // reporter 路径：复用已配置的当前 meowo-reporter → 否则 app 同目录 sidecar。
        // 历史 cc-reporter 路径已废弃，即使可执行仍存在也不能复用，否则 ensure_codex_hooks
        // 会把旧路径写回去，hooks 仍然指向不存在的 meowo 旧 reporter。
        let reporter = reporter_path_from_codex(&root)
            .filter(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| {
                        let n = n.to_ascii_lowercase();
                        n == "meowo-reporter" || n == "meowo-reporter.exe"
                    })
                    .unwrap_or(false)
            })
            .or_else(super::sibling_reporter);
        let Some(reporter) = reporter else {
            return;
        };
        if ensure_codex_hooks(&mut root, &reporter) {
            let Ok(pretty) = serde_json::to_string_pretty(&root) else {
                return;
            };
            if hooks_path.exists() {
                super::backup_once(&hooks_path);
            }
            if crate::fsutil::write_atomic(&hooks_path, &format!("{pretty}\n")).is_err() {
                return; // hooks.json 没写成，信任步骤跳过（下次启动整段重试）
            }
        }
        // 2) config.toml 预信任：解析失败只跳过信任（hooks 已接上，退化 = codex 弹一次 Trust all）。
        let cfg_path = home.join("config.toml");
        let cfg_text = match std::fs::read_to_string(&cfg_path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => return,
        };
        let Ok(mut doc) = cfg_text.parse::<toml_edit::DocumentMut>() else {
            return;
        };
        let entries = claimed_codex_entries(&root);
        if ensure_trusted_hashes(&mut doc, &hooks_path.display().to_string(), &entries) {
            if cfg_path.exists() {
                super::backup_once(&cfg_path);
            }
            let _ = crate::fsutil::write_atomic(&cfg_path, &doc.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ensure_codex_hooks_adds_all_events_when_empty() {
        let mut v = json!({});
        assert!(ensure_codex_hooks(&mut v, "C:/x/meowo-reporter.exe"));
        for ev in CODEX_EVENTS {
            let cmd = v["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap();
            assert_eq!(cmd, "\"C:/x/meowo-reporter.exe\" --provider codex");
            assert_eq!(v["hooks"][ev][0]["hooks"][0]["timeout"], 5);
        }
        // 幂等：二跑无改动。
        assert!(!ensure_codex_hooks(&mut v, "C:/x/meowo-reporter.exe"));
    }

    #[test]
    fn ensure_codex_hooks_adopts_manual_wiring_and_fills_missing() {
        // 精确复刻本机手工接线形态：裸路径命令、3 事件、Stop timeout=10。
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let entry = |t: u64| json!({ "hooks": [{ "type": "command", "command": format!("{dev} --provider codex"), "timeout": t }] });
        let mut v = json!({ "hooks": {
            "SessionStart": [entry(5)], "UserPromptSubmit": [entry(5)], "Stop": [entry(10)]
        }});
        assert!(ensure_codex_hooks(&mut v, dev)); // 补 PostToolUse/PermissionRequest → 有改动
        // 既有条目原样保留（裸路径不被改写为引号形态、timeout 10 不动）——幂等按解析后内容判定。
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], format!("{dev} --provider codex"));
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["timeout"], 10);
        // 新事件已补齐。
        assert!(v["hooks"]["PostToolUse"][0]["hooks"][0]["command"].as_str().unwrap().contains("--provider codex"));
        assert!(v["hooks"]["PermissionRequest"].is_array());
        assert!(!ensure_codex_hooks(&mut v, dev));
    }

    #[test]
    fn ensure_codex_hooks_updates_stale_path_keeps_user_hooks() {
        let mut v = json!({ "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": "node my-notify.js" }] },
            { "hooks": [{ "type": "command", "command": "\"C:/old/meowo-reporter.exe\" --provider codex", "timeout": 5 }] }
        ]}});
        assert!(ensure_codex_hooks(&mut v, "C:/new/meowo-reporter.exe"));
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "node my-notify.js"); // 用户 hook 不动
        assert_eq!(v["hooks"]["Stop"][1]["hooks"][0]["command"], "\"C:/new/meowo-reporter.exe\" --provider codex");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2); // 不重复追加
    }

    #[test]
    fn ensure_codex_hooks_abandons_when_hooks_key_is_non_object() {
        // I1 回归：hooks 键存在但非 object（如手改坏形状）——对齐 kimi 哲学，放弃不写，
        // 绝不覆盖用户文件（既有实现会整体置 {}，无备份地清掉用户内容）。
        let mut v = json!({ "hooks": 5 });
        assert!(!ensure_codex_hooks(&mut v, "C:/x/meowo-reporter.exe"));
        assert_eq!(v, json!({ "hooks": 5 }));
    }

    #[test]
    fn ensure_codex_hooks_skips_event_with_non_array_value() {
        // I1 回归：某事件值为畸形形状（非 array）——该事件原样跳过不动，其余事件正常补齐。
        let mut v = json!({ "hooks": { "Stop": "oops" } });
        assert!(ensure_codex_hooks(&mut v, "C:/x/meowo-reporter.exe")); // 其余 4 事件补齐 → 有改动
        assert_eq!(v["hooks"]["Stop"], json!("oops")); // 畸形事件原样不动
        for ev in CODEX_EVENTS.iter().filter(|&&e| e != "Stop") {
            let cmd = v["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap();
            assert!(cmd.contains("--provider codex"));
        }
    }

    #[test]
    fn reporter_path_and_claimed_entries_extraction() {
        let mut v = json!({});
        ensure_codex_hooks(&mut v, "C:/x/meowo-reporter.exe");
        assert_eq!(reporter_path_from_codex(&v).as_deref(), Some("C:/x/meowo-reporter.exe"));
        let entries = claimed_codex_entries(&v);
        assert_eq!(entries.len(), 5);
        assert!(entries.iter().all(|e| e.command.contains("--provider codex") && e.timeout == 5));
        assert!(entries.iter().any(|e| e.event == "PermissionRequest"));
        // 全新单 handler 组场景：真实位置就是 0:0。
        assert!(entries.iter().all(|e| e.group_idx == 0 && e.handler_idx == 0));
    }

    #[test]
    fn codex_hook_hash_matches_real_machine_vectors() {
        // 三条向量取自本机 ~/.codex/config.toml 的真实 trusted_hash（codex-cli 0.142.3），
        // 算法：canonical JSON（key 字母序、紧凑）SHA-256。codex 升级若改算法，此测试变红。
        let cmd = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe --provider codex";
        assert_eq!(
            codex_hook_hash("session_start", cmd, 5),
            "sha256:5e68ee84ac2076b424f12a7a1b346f5c1f5907d4829d6a30239bc49c0e76382c"
        );
        assert_eq!(
            codex_hook_hash("user_prompt_submit", cmd, 5),
            "sha256:aef30ddec757deff63f67240a9b859d1a63c669eaee6a5ee6a30404daaa81523"
        );
        assert_eq!(
            codex_hook_hash("stop", cmd, 10),
            "sha256:cd638b7a1c4a91cd28a5946fbbe5d7e7bf1f3c478d85f329ad95323a9323e403"
        );
    }

    #[test]
    fn snake_event_covers_all_codex_events() {
        for ev in CODEX_EVENTS {
            assert!(!snake_event(ev).is_empty(), "{ev} 缺 snake_case 映射");
        }
        assert_eq!(snake_event("PermissionRequest"), "permission_request");
        assert_eq!(snake_event("Unknown"), "");
    }

    #[test]
    fn ensure_trusted_hashes_writes_and_is_idempotent() {
        // 从空 config.toml 起：写入 [hooks.state] 各键；已有等值哈希则不动；不碰无关内容。
        // 单组场景（无其他 handler 共存）：真实位置就是 0:0——本机真实向量断言一字不动。
        let mut doc: toml_edit::DocumentMut = "default_model = \"x\"\n".parse().unwrap();
        let cmd = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe --provider codex";
        let entries = vec![ClaimedEntry {
            event: "SessionStart".to_string(),
            command: cmd.to_string(),
            timeout: 5,
            group_idx: 0,
            handler_idx: 0,
        }];
        let hooks_path = r"C:\Users\larry\.codex\hooks.json";
        assert!(ensure_trusted_hashes(&mut doc, hooks_path, &entries));
        let out = doc.to_string();
        assert!(out.contains("default_model = \"x\"")); // 无关内容原样
        // 键 = <display路径>:<snake事件>:0:0，值 = 本机验证过的真实哈希。
        assert!(out.contains(r"C:\Users\larry\.codex\hooks.json:session_start:0:0"));
        assert!(out.contains("sha256:5e68ee84ac2076b424f12a7a1b346f5c1f5907d4829d6a30239bc49c0e76382c"));
        // 幂等：二跑无改动。
        assert!(!ensure_trusted_hashes(&mut doc, hooks_path, &entries));
    }

    #[test]
    fn ensure_trusted_hashes_tolerates_malformed_hooks_key() {
        // hooks 键存在但非 table（如误粘 kimi 的数组语法）：不得 panic，返回无改动、文档原样。
        let mut doc: toml_edit::DocumentMut = "hooks = \"oops\"\n".parse().unwrap();
        let entries = vec![ClaimedEntry {
            event: "SessionStart".to_string(),
            command: "x --provider codex".to_string(),
            timeout: 5,
            group_idx: 0,
            handler_idx: 0,
        }];
        assert!(!ensure_trusted_hashes(&mut doc, r"C:\h.json", &entries));
        assert_eq!(doc.to_string(), "hooks = \"oops\"\n");
    }

    #[test]
    fn ensure_trusted_hashes_uses_real_group_index_when_user_group_precedes_ours() {
        // C1 回归：用户已有 Stop hook 组在 [0]（如 node my-notify.js），我方组在 [1]——
        // ensure_codex_hooks 只追加不重排，这正是真实机器上会出现的既有组形态。
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let v = json!({ "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": "node my-notify.js" }] },
            { "hooks": [{ "type": "command", "command": format!("{dev} --provider codex"), "timeout": 10 }] }
        ]}});
        let entries = claimed_codex_entries(&v);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].group_idx, 1); // 我方组真实位置是 1，不是 0
        assert_eq!(entries[0].handler_idx, 0);

        let mut doc: toml_edit::DocumentMut = "default_model = \"x\"\n".parse().unwrap();
        let hooks_path = r"C:\Users\larry\.codex\hooks.json";
        assert!(ensure_trusted_hashes(&mut doc, hooks_path, &entries));
        let out = doc.to_string();
        assert!(out.contains(r"C:\Users\larry\.codex\hooks.json:stop:1:0"));
        // 用户组的槽位 :0:0 绝不能被写入/覆盖。
        assert!(!out.contains(r"C:\Users\larry\.codex\hooks.json:stop:0:0"));
        // 幂等：二跑无改动，且 0:0 键始终未被触碰。
        assert!(!ensure_trusted_hashes(&mut doc, hooks_path, &entries));
        assert!(!doc.to_string().contains(r"C:\Users\larry\.codex\hooks.json:stop:0:0"));
    }

    #[test]
    fn claimed_codex_entries_skips_group_shared_with_other_handler() {
        // C1 回归：我方 handler 与他人 handler 挤在同一组里——canon 哈希公式只支持单 handler
        // 形态，算出来的哈希对不上 codex 真实校验对象，必须跳过不预信任。
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let v = json!({ "hooks": { "Stop": [
            { "hooks": [
                { "type": "command", "command": "node my-notify.js" },
                { "type": "command", "command": format!("{dev} --provider codex"), "timeout": 5 }
            ] }
        ]}});
        assert!(claimed_codex_entries(&v).is_empty());
    }

    /// dry-run：CODEX_HOME=<真实 ~/.codex 的副本> 时跑 CodexSetup::apply，人工核对副本产物。
    /// 用法：复制 ~/.codex 到临时目录，CODEX_HOME=<副本> cargo test ... -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dryrun_codex() {
        use crate::setup::ProviderSetup;
        CodexSetup.apply();
        let home = meowo_reporter::codex::codex_home().unwrap();
        eprintln!("=== hooks.json ===\n{}", std::fs::read_to_string(home.join("hooks.json")).unwrap());
        if let Ok(cfg) = std::fs::read_to_string(home.join("config.toml")) {
            eprintln!("=== config.toml [hooks.state] ===\n{cfg}");
        } else {
            eprintln!("=== config.toml not present ===");
        }
    }
}
