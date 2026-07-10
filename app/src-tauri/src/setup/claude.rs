//! claude（Anthropic Claude Code）自动接线。**hooks 合并逻辑已迁入
//! `meowo_agent::config::ConfigFormat::ClaudeJson`**（matcher 感知），本模块只剩两件事：
//!   1) I/O 编排（读 settings.json → 交给格式适配器 → 备份 + 原子写）；
//!   2) 接线**副作用**：把 statusLine 包成「先写库再跑原 statusLine」的脚本，让 Context 百分比
//!      自动有准确数据。它与 hooks 同住 settings.json，故走**写前改写**（`Amend`）而非
//!      `AfterWrite`——要 `db_path()` 与脚本落盘，留在 app 侧。

use meowo_agent::{Installation, RepairReason};
use serde_json::{json, Value};

/// claude 的 ProviderSetup。settings.json 缺失可从 `{}` 建，但**数据目录不存在＝没装**，
/// 不凭空造 `~/.claude`（由 `detect()` / `is_configured()` 在上游拦下）。
pub struct ClaudeSetup;

impl super::ProviderSetup for ClaudeSetup {
    fn id(&self) -> meowo_agent::AgentId {
        meowo_agent::id::CLAUDE
    }
    fn detect(&self) -> bool {
        meowo_agent::installation(meowo_agent::id::CLAUDE).is_some_and(|i| i.is_configured())
    }
    fn apply(&self) -> Option<RepairReason> {
        let Some(inst) = meowo_agent::installation(meowo_agent::id::CLAUDE) else {
            eprintln!("Meowo repair[claude]: 解析不到 claude 安装实况，跳过");
            return Some(RepairReason::NotDetected);
        };
        // `MissingConfig::CreateFrom("{}")` 会在 settings.json 缺失时从空对象建——但仅当
        // ~/.claude 已存在（CC 确实装过）才该发生，否则会在没装 CC 的机器上凭空造目录和文件。
        if !inst.is_configured() {
            eprintln!(
                "Meowo repair[claude]: {} 不存在（未安装），跳过",
                inst.data_dir.display()
            );
            return Some(RepairReason::NotDetected);
        }
        super::wire_hooks(&inst, "claude", Some(statusline_amend), None)
    }
}

/// Windows 路径转 bash 可用形式：`C:\a\b` -> `C:/a/b`（Git Bash 接受 `C:/...`）。
pub fn to_bash_path(p: &str) -> String {
    p.replace('\\', "/")
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

/// 生成包装脚本内容：读 stdin → 喂 meowo-reporter 写库（丢弃其输出）→ 跑原 statusLine（如有）渲染状态栏。
/// `reporter_bash` 为 bash 形式的 meowo-reporter 路径；`inner` 为原 statusLine 命令（空则不渲染）。
pub fn build_script(reporter_bash: &str, inner: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/usr/bin/env bash\n");
    s.push_str("# 本文件由 Meowo 自动生成：写入会话上下文用量 + 渲染状态栏。请勿手改。\n");
    s.push_str("input=$(cat)\n");
    if inner.trim().is_empty() {
        // 无下游 statusLine：meowo-reporter 写库并自渲染极简状态栏（输出即状态栏）。
        s.push_str(&format!(
            "printf '%s' \"$input\" | \"{reporter_bash}\" statusline\n"
        ));
    } else {
        // 有下游（如 claude-hud）：meowo-reporter 只写库（丢弃输出），再跑下游渲染真正的状态栏。
        s.push_str(&format!(
            "printf '%s' \"$input\" | \"{reporter_bash}\" statusline >/dev/null 2>&1\n"
        ));
        s.push_str(&format!("printf '%s' \"$input\" | {inner}\n"));
    }
    s
}

/// 写出 statusline 包装脚本（先建目录）。返回错误供调用方决定是否改写 settings。
fn write_statusline_script(path: &std::path::Path, script: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, script)
}

/// 包装脚本路径：`~/.meowo/statusline.sh`（与 board.db 同目录）。
fn script_path() -> std::path::PathBuf {
    crate::db_path().with_file_name("statusline.sh")
}

/// `Amend`：在 hooks 合并后、落盘前把 statusLine 指向包装脚本。
///
/// **顺序纪律（勿改）**：脚本先落盘，成功后 settings 才指向它。写失败（目录不可写/杀软拦截/磁盘满）
/// 时 settings 原样不动——否则 Claude Code 状态栏会指向不存在的脚本，用户原 statusLine 命令
/// （inner）只存在于没写出去的脚本里而永久丢失，且后续启动因幂等判定命中 marker 而跳过重建、
/// 永不自愈。settings 未动则下次启动整段重试。
///
/// 无改动时**原样返回入参文本**（不重新序列化），否则 `wire_hooks` 的幂等判定会误判为有改动。
fn statusline_amend(
    _inst: &Installation,
    text: &str,
    reporter: &str,
) -> Result<String, RepairReason> {
    let Some(mut settings) = meowo_agent::config::parse_json_config(text) else {
        return Err(RepairReason::ConfigUnreadable);
    };
    let script_path = script_path();
    let script_bash = to_bash_path(&script_path.to_string_lossy());

    match probe_statusline(&settings, &script_bash) {
        Some(inner) => {
            let script = build_script(&to_bash_path(reporter), &inner);
            if write_statusline_script(&script_path, &script).is_err() {
                eprintln!("Meowo repair[claude]: statusline 脚本写入失败，settings 原样不动（下次启动重试）");
                return Ok(text.to_string());
            }
            settings["statusLine"] =
                json!({ "type": "command", "command": format!("bash \"{script_bash}\"") });
            let pretty =
                serde_json::to_string_pretty(&settings).map_err(|_| RepairReason::WriteFailed)?;
            Ok(format!("{pretty}\n"))
        }
        None => {
            // 幂等命中（settings 已指向我们的脚本）但脚本文件缺失：用户删 ~/.meowo 重置数据时
            // board.db 会被 Store::open 自动重建，本脚本却不会——不补建则状态栏每次渲染报
            // No such file、Context% 永久断供。原 inner 已无从恢复，退化为自渲染版兜底。
            if !script_path.exists() {
                let script = build_script(&to_bash_path(reporter), "");
                let _ = write_statusline_script(&script_path, &script);
            }
            Ok(text.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_statusline_wraps_existing_and_is_idempotent() {
        let mut v =
            json!({ "statusLine": { "type": "command", "command": "bash -c 'claude-hud'" } });
        let marker = "C:/Users/me/.meowo/statusline.sh";
        let inv = format!("bash \"{marker}\"");
        let inner = probe_statusline(&v, marker).expect("应需要生成脚本");
        assert_eq!(inner, "bash -c 'claude-hud'"); // 捕获到原命令
        assert_eq!(v["statusLine"]["command"], "bash -c 'claude-hud'"); // 探测不改写
                                                                        // 模拟 amend：脚本落盘成功后才改写 settings。
        v["statusLine"] = json!({ "type": "command", "command": inv });
        // 再探测：已引用我们的脚本 → None（幂等，不再重复捕获/递归）
        assert!(probe_statusline(&v, marker).is_none());
    }

    #[test]
    fn probe_statusline_handles_absent() {
        let v = json!({});
        let marker = "/home/me/.meowo/statusline.sh";
        let inner = probe_statusline(&v, marker).expect("无 statusLine 也应接线");
        assert_eq!(inner, ""); // 无原命令
    }

    #[test]
    fn build_script_with_and_without_inner() {
        let with = build_script("C:/x/meowo-reporter.exe", "bash -c 'hud'");
        assert!(with.contains("\"C:/x/meowo-reporter.exe\" statusline >/dev/null"));
        assert!(with.contains("| bash -c 'hud'\n"));
        let without = build_script("C:/x/meowo-reporter.exe", "  ");
        // 无 inner：让 meowo-reporter 自渲染（不丢弃输出）
        assert!(without.contains("| \"C:/x/meowo-reporter.exe\" statusline\n"));
        assert!(!without.contains(">/dev/null"));
    }

    #[test]
    fn to_bash_path_normalizes_windows_separators() {
        assert_eq!(
            to_bash_path(r"C:\Users\me\.meowo\statusline.sh"),
            "C:/Users/me/.meowo/statusline.sh"
        );
        assert_eq!(to_bash_path("/home/me/x"), "/home/me/x");
    }

    /// 从 install-hooks.mjs 里抠出 `const SPECS = [...]` 的 `["事件", "matcher"],` 各行。
    fn parse_mjs_specs(src: &str) -> Vec<(String, String)> {
        let start = src
            .find("const SPECS = [")
            .expect("install-hooks.mjs 里找不到 const SPECS");
        let block = &src[start..];
        let end = block.find("];").expect("SPECS 数组未闭合");
        block[..end]
            .lines()
            .filter_map(|l| {
                let inner = l.trim().strip_prefix('[')?.split(']').next()?;
                let mut it = inner.split(',').map(|s| s.trim().trim_matches('"'));
                Some((it.next()?.to_string(), it.next()?.to_string()))
            })
            .collect()
    }

    /// 绊线：`plugins/claude.rs` 的 EVENTS 与 `scripts/install-hooks.mjs` 的 SPECS 必须逐条一致。
    /// 两处各维护一份（一个给 app 无感接线，一个给手动脚本），此前只有注释在提醒，靠不住——
    /// 漏同步会让脚本装出的 hooks 与 app 认领的规格对不上，出现「装了却显示未接入」。
    #[test]
    fn hook_specs_match_install_hooks_mjs() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/install-hooks.mjs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读不到 {}：{e}", path.display()));
        let from_js = parse_mjs_specs(&src);
        assert_eq!(
            from_js.len(),
            8,
            "解析出的 SPECS 条数不对，脚本格式可能变了"
        );

        let events = meowo_agent::by_id("claude")
            .expect("claude 应已注册")
            .variants()[0]
            .hooks
            .events;
        let from_rs: Vec<(String, String)> = events
            .iter()
            .map(|e| {
                (
                    e.name.to_string(),
                    e.matcher.unwrap_or_default().to_string(),
                )
            })
            .collect();

        assert_eq!(
            from_rs, from_js,
            "plugins/claude.rs 的 EVENTS 与 install-hooks.mjs 的 SPECS 不一致"
        );
    }

    /// dry-run：对 CLAUDE_CONFIG_DIR/settings.json（真实文件的副本）跑 ClaudeSetup::apply，核对产物。
    /// 用法：复制 ~/.claude 到临时目录，
    ///       CLAUDE_CONFIG_DIR=<副本> MEOWO_DB=<副本/board.db> \
    ///       cargo test -p meowo-app dryrun_claude -- --ignored --nocapture
    ///
    /// 只打印结构性摘要，**绝不 dump 配置原文**——真实 settings.json 可能含 env 里的密钥。
    #[test]
    #[ignore]
    fn dryrun_claude() {
        use crate::setup::ProviderSetup;
        let reason = super::ClaudeSetup.apply();
        let inst = meowo_agent::installation(meowo_agent::id::CLAUDE).expect("应解析出实况");
        let text = std::fs::read_to_string(inst.config_path()).expect("读不回 settings.json");
        let v: Value = serde_json::from_str(&text).expect("产物应为合法 JSON");

        eprintln!(
            "变体={} 配置={}",
            inst.variant_tag,
            inst.config_path().display()
        );
        eprintln!("apply reason={reason:?}");
        eprintln!(
            "顶层键（应全部保留）={:?}",
            v.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        eprintln!(
            "hooks 事件={:?}",
            v["hooks"].as_object().unwrap().keys().collect::<Vec<_>>()
        );
        eprintln!(
            "SessionStart 已接线={}",
            inst.hooks.has_reporter(&text, "claude")
        );

        // 8 条 (event, matcher) 齐。
        let want: [(&str, &str); 8] = [
            ("SessionStart", "*"),
            ("UserPromptSubmit", "*"),
            ("PostToolUse", "*"),
            ("Stop", "*"),
            ("SessionEnd", "*"),
            ("PermissionRequest", "*"),
            ("PreToolUse", "AskUserQuestion"),
            ("PreToolUse", "ExitPlanMode"),
        ];
        for (ev, m) in want {
            let arr = v["hooks"][ev]
                .as_array()
                .unwrap_or_else(|| panic!("缺事件 {ev}"));
            assert!(arr.iter().any(|e| e["matcher"] == m), "缺 ({ev}, {m})");
        }
        // statusLine 指向脚本且脚本存在。
        let sl = v["statusLine"]["command"].as_str().unwrap();
        assert!(sl.contains("statusline.sh"), "statusLine 未指向脚本：{sl}");
        assert!(super::script_path().exists(), "statusLine 脚本未落盘");
        eprintln!("statusLine 指向脚本且脚本存在 ✓");
    }
}
