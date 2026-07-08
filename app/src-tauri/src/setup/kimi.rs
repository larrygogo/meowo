//! kimi（kimi-code CLI）自动接线：结构保持地合并 `~/.kimi-code/config.toml` 顶层 [[hooks]]。
//! 纪律（源码调研 kimi-code 0.20）：kimi 自身会全量重写此文件（注释全丢）——幂等判定只按
//! (event, command) 内容匹配，绝不依赖注释标记；一条非法 hook 会让 kimi 静默禁用全部
//! hooks——事件名有白名单绊线测试；文件解析失败即放弃（与 kimi 自身写保护一致）。
use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

/// 接线事件集。PermissionRequest = kimi 交互式等待用户审批前触发（官方源码确认，observation-only），
/// 用于卡片「待交互」显示；dispatch 既有 PermissionRequest 分支复用（kimi 无 claude 专属工具，落 Approval）。
pub const KIMI_EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "PostToolUse",
    "Stop",
    "SessionEnd",
    "PermissionRequest",
];

/// kimi-code 0.20 支持的全部 hook 事件（HOOK_EVENT_TYPES，白名单绊线用）。
pub const KIMI_EVENT_WHITELIST: [&str; 16] = [
    "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionResult",
    "UserPromptSubmit", "Stop", "StopFailure", "Interrupt", "SessionStart", "SessionEnd",
    "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "Notification",
];

/// 幂等合并 [[hooks]]：逐 KIMI_EVENTS 找认领条目（event 相符 + command 可认领），
/// 路径不符则更新 command；缺则追加 {event, command, timeout=5}。用户条目一概不动。
pub fn ensure_kimi_hooks(doc: &mut DocumentMut, reporter_native: &str) -> bool {
    let desired_cmd = format!("{reporter_native} --provider kimi");
    let mut changed = false;
    if doc.get("hooks").and_then(|it| it.as_array_of_tables()).is_none() {
        // 不存在或非 array-of-tables（如 hooks = [] 的旧写法残留在新文件：实测 kimi-code
        // 默认无顶层 hooks 键；inline array 形态无法结构保持地转换，直接放弃不写坏）。
        if doc.get("hooks").is_some() {
            return false;
        }
        doc.insert("hooks", Item::ArrayOfTables(ArrayOfTables::new()));
        changed = true;
    }
    // 上面已确保 hooks 键要么本就是 array-of-tables，要么刚被本函数插入为 array-of-tables；
    // 上一分支中「存在但非 array-of-tables」已提前 return，故此处 as_array_of_tables_mut
    // 必不为 None——统一用 let-else 提前返回而非 expect/panic，纪律同 codex.rs 先例。
    let Some(arr) = doc["hooks"].as_array_of_tables_mut() else {
        return changed;
    };
    for ev in KIMI_EVENTS {
        let mut found = false;
        for t in arr.iter_mut() {
            if t.get("event").and_then(|v| v.as_str()) != Some(ev) {
                continue;
            }
            let Some(path) = t
                .get("command")
                .and_then(|v| v.as_str())
                .and_then(|c| super::claim_provider_cmd(c, "kimi"))
            else {
                continue; // 该事件上的用户自有 hook，不动
            };
            found = true;
            if path != reporter_native {
                t.insert("command", value(desired_cmd.clone()));
                changed = true;
            }
        }
        if !found {
            let mut t = Table::new();
            t.insert("event", value(ev));
            t.insert("command", value(desired_cmd.clone()));
            t.insert("timeout", value(5));
            arr.push(t);
            changed = true;
        }
    }
    changed
}

/// kimi 的 ProviderSetup。config.toml 由 kimi login 生成，缺失 → 视为未完成安装，跳过不创建。
pub struct KimiSetup;

impl super::ProviderSetup for KimiSetup {
    fn key(&self) -> meowo_store::ProviderKey {
        meowo_store::ProviderKey::Kimi
    }
    fn detect(&self) -> bool {
        meowo_reporter::kimi::kimi_share_dir().is_some_and(|d| d.is_dir())
    }
    fn apply(&self) {
        let Some(dir) = meowo_reporter::kimi::kimi_share_dir() else {
            return;
        };
        let cfg = dir.join("config.toml");
        let Ok(text) = std::fs::read_to_string(&cfg) else {
            return;
        };
        let Ok(mut doc) = text.parse::<DocumentMut>() else {
            return; // 解析失败绝不写（kimi 自身对坏文件同样拒写）
        };
        // reporter 路径：复用 [[hooks]] 里已认领的当前 meowo-reporter → 否则 sidecar。
        // 历史 cc-reporter 路径已废弃，必须改用当前 meowo-reporter，否则 ensure_kimi_hooks
        // 会把旧路径当成目标写回去，导致 hooks 仍然失效。
        let existing = doc
            .get("hooks")
            .and_then(|it| it.as_array_of_tables())
            .into_iter()
            .flat_map(|a| a.iter())
            .find_map(|t| {
                t.get("command")
                    .and_then(|v| v.as_str())
                    .and_then(|c| super::claim_provider_cmd(c, "kimi"))
            })
            .filter(|p| {
                std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| {
                        let n = n.to_ascii_lowercase();
                        n == "meowo-reporter" || n == "meowo-reporter.exe"
                    })
                    .unwrap_or(false)
            });
        let Some(reporter) = existing.or_else(super::sibling_reporter) else {
            return;
        };
        if ensure_kimi_hooks(&mut doc, &reporter) {
            super::backup_once(&cfg);
            let _ = crate::fsutil::write_atomic(&cfg, &doc.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kimi_events_all_in_upstream_whitelist() {
        // 防连坐绊线：一条非法 event 会让 kimi 静默禁用全部 hooks（源码 salvageConfigData）。
        for ev in KIMI_EVENTS {
            assert!(KIMI_EVENT_WHITELIST.contains(&ev), "{ev} 不在 kimi 0.20 事件白名单");
        }
    }

    #[test]
    fn ensure_kimi_hooks_adds_all_when_absent_and_preserves_content() {
        let src = "default_model = \"kimi-code/kimi-for-coding\"\n# 用户注释\n[loop_control]\nmax_steps_per_turn = 100\n";
        let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
        assert!(ensure_kimi_hooks(&mut doc, "C:/x/meowo-reporter.exe"));
        let out = doc.to_string();
        assert!(out.contains("# 用户注释")); // 结构保持：注释仍在
        assert!(out.contains("max_steps_per_turn = 100"));
        for ev in KIMI_EVENTS {
            assert!(out.contains(&format!("event = \"{ev}\"")), "{ev} 未写入");
        }
        assert!(out.contains(r#"command = "C:/x/meowo-reporter.exe --provider kimi""#));
        assert!(!ensure_kimi_hooks(&mut doc, "C:/x/meowo-reporter.exe")); // 幂等
    }

    #[test]
    fn ensure_kimi_hooks_adopts_manual_and_updates_stale_path() {
        // 复刻本机手工接线形态：裸路径命令、6 事件、timeout 5。
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!("[[hooks]]\nevent = \"{ev}\"\ncommand = \"{dev} --provider kimi\"\ntimeout = 5\n\n"));
        }
        let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
        // 路径仍存在时（解析等价）：无改动。
        assert!(!ensure_kimi_hooks(&mut doc, dev));
        // 路径失效换 sidecar：6 条 command 全部更新，用户键 theme 不动。
        assert!(ensure_kimi_hooks(&mut doc, "C:/app/meowo-reporter.exe"));
        let out = doc.to_string();
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
    }

    #[test]
    fn ensure_kimi_hooks_updates_legacy_cc_reporter_paths() {
        // 项目改名后，旧 hooks 仍指向 cc-reporter.exe；应被认领并更新为 meowo-reporter。
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!(
                "[[hooks]]\nevent = \"{ev}\"\ncommand = \"C:/x/cc-reporter.exe --provider kimi\"\ntimeout = 5\n\n"
            ));
        }
        let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
        assert!(ensure_kimi_hooks(&mut doc, "C:/app/meowo-reporter.exe"));
        let out = doc.to_string();
        assert_eq!(out.matches("cc-reporter").count(), 0);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
        assert!(!ensure_kimi_hooks(&mut doc, "C:/app/meowo-reporter.exe")); // 幂等
    }

    #[test]
    fn ensure_kimi_hooks_keeps_user_hook_entries() {
        let src = "[[hooks]]\nevent = \"Notification\"\ncommand = \"my-notify --ding\"\ntimeout = 3\n";
        let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
        assert!(ensure_kimi_hooks(&mut doc, "C:/x/meowo-reporter.exe"));
        let out = doc.to_string();
        assert!(out.contains("my-notify --ding")); // 用户 hook 原样
        assert_eq!(out.matches("--provider kimi").count(), 6); // 我方 6 条已加
    }

    #[test]
    fn ensure_kimi_hooks_abandons_on_non_array_hooks() {
        // hooks 键存在但非 array-of-tables（inline array / 字符串）：放弃不写坏，返回 false、文档原样。
        for src in ["hooks = []\n", "hooks = \"oops\"\n"] {
            let mut doc: toml_edit::DocumentMut = src.parse().unwrap();
            assert!(!ensure_kimi_hooks(&mut doc, "C:/x/meowo-reporter.exe"), "src={src:?}");
            assert_eq!(doc.to_string(), src);
        }
    }

    /// dry-run：KIMI_SHARE_DIR=<真实 ~/.kimi-code 的副本> 时跑 KimiSetup::apply，人工核对副本产物。
    /// 用法：复制 ~/.kimi-code 到临时目录，KIMI_SHARE_DIR=<副本> cargo test ... -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dryrun_kimi() {
        use crate::setup::ProviderSetup;
        KimiSetup.apply();
        let dir = meowo_reporter::kimi::kimi_share_dir().unwrap();
        eprintln!("=== config.toml ===\n{}", std::fs::read_to_string(dir.join("config.toml")).unwrap());
    }
}
