//! hooks 配置格式适配器：统一「确保接入 / 判断已接入 / 认领既有条目」三件事，全是纯函数
//! （输入配置文本，输出新文本或判定），落盘由调用方负责——便于单测，也保证「解析失败绝不写」。
//!
//! 迁移状态：只有 [`ConfigFormat::KimiToml`] 已迁入。claude/codex 的 JSON 形态仍在 meowo-app 的
//! `setup/` 里，待各自变体表落地后再收进来——刻意不预留 `todo!()` 变体，免得留下会 panic 的空壳。

use toml_edit::{value, ArrayOfTables, DocumentMut, Item, Table};

/// 接线失败的机器可读原因，回传前端以给出精准提示（如未登录 → 「请先登录」）。
/// 序列化为 kebab-case 字符串；`None` 表示成功或已是目标状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairReason {
    /// agent 数据目录不存在（视为未安装）。
    NotDetected,
    /// 承载 hooks 的配置文件尚未生成（如 kimi 的 config.toml 需先 `kimi login`）。
    NeedLogin,
    /// 找不到 meowo-reporter 二进制（既有 hooks 无有效路径且 app 同目录无 sidecar）。
    ReporterNotFound,
    /// 配置文件读取或解析失败（权限/编码/畸形），为保护用户文件放弃写入。
    ConfigUnreadable,
    /// 写入失败（目录不可写/磁盘满/杀软拦截）。
    WriteFailed,
}

/// `ensure_hooks` 的三态结果。`Changed` 携带待落盘的新文本。
#[derive(Debug, PartialEq, Eq)]
pub enum EnsureOutcome {
    Changed(String),
    Unchanged,
    /// 配置形态无法安全改写 → 放弃，绝不写坏用户文件。
    Abandon(RepairReason),
}

/// hooks 配置的落盘格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// kimi（新旧版一致）：`config.toml` 顶层 `[[hooks]]` array-of-tables。
    KimiToml,
}

impl ConfigFormat {
    /// 配置文件相对 `data_dir` 的路径。
    pub fn config_rel(self) -> &'static str {
        match self {
            Self::KimiToml => "config.toml",
        }
    }

    /// 幂等接入 meowo-reporter hooks。用户自有的 hook 条目一概不动。
    pub fn ensure_hooks(self, cur_text: &str, reporter: &str, provider: &str) -> EnsureOutcome {
        match self {
            Self::KimiToml => kimi_toml::ensure_hooks(cur_text, reporter, provider),
        }
    }

    /// 新会话是否真能入库：**只看 SessionStart**。仅在别的事件（如 Stop）挂了 reporter，
    /// 不能保证新会话被记录，不应误判成已接入。
    pub fn has_reporter(self, cur_text: &str, provider: &str) -> bool {
        match self {
            Self::KimiToml => kimi_toml::has_reporter(cur_text, provider),
        }
    }

    /// 从既有配置里取出已认领的**当前** meowo-reporter 绝对路径（历史 cc-reporter 不算：
    /// 它已废弃，若当成目标写回去，hooks 依旧失效）。用于复用用户已有的 reporter 位置。
    pub fn claimed_reporter(self, cur_text: &str, provider: &str) -> Option<String> {
        match self {
            Self::KimiToml => kimi_toml::claimed_reporter(cur_text, provider),
        }
    }
}

// ═══ 命令行认领（各格式共用） ═══

/// 解析 hook command 为（可执行路径, 余参）。首 token 支持带双引号或裸路径。
pub fn parse_hook_command(cmd: &str) -> Option<(String, Vec<String>)> {
    let c = cmd.trim();
    let (path, rest) = if let Some(r) = c.strip_prefix('"') {
        let end = r.find('"')?;
        (r[..end].to_string(), r[end + 1..].trim())
    } else {
        match c.split_once(char::is_whitespace) {
            Some((p, r)) => (p.to_string(), r.trim()),
            None => (c.to_string(), ""),
        }
    };
    let args = rest.split_whitespace().map(str::to_string).collect();
    Some((path, args))
}

/// 严格认领带 provider 参数的命令（codex/kimi 形态）：可执行文件名恰为 meowo-reporter[.exe]
///（或历史遗留 cc-reporter[.exe]）且余参恰为 `["--provider", provider]`。返回可执行路径。
/// 不裸 contains，不误伤用户 hook。
pub fn claim_provider_cmd(cmd: &str, provider: &str) -> Option<String> {
    let (path, args) = parse_hook_command(cmd)?;
    let name = std::path::Path::new(&path).file_name()?.to_str()?;
    let is_reporter = matches!(
        name.to_ascii_lowercase().as_str(),
        "meowo-reporter" | "meowo-reporter.exe" | "cc-reporter" | "cc-reporter.exe"
    );
    (is_reporter && args == ["--provider", provider]).then_some(path)
}

/// 该路径的文件名是否为**当前** meowo-reporter（排除历史 cc-reporter）。
fn is_current_reporter(path: &str) -> bool {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n.to_ascii_lowercase().as_str(), "meowo-reporter" | "meowo-reporter.exe"))
}

// ═══ KimiToml ═══

mod kimi_toml {
    use super::*;

    /// 接线事件集。PermissionRequest = kimi 交互式等待用户审批前触发（官方源码确认，observation-only），
    /// 用于卡片「待交互」显示；dispatch 既有 PermissionRequest 分支复用。
    pub const EVENTS: [&str; 6] = [
        "SessionStart",
        "UserPromptSubmit",
        "PostToolUse",
        "Stop",
        "SessionEnd",
        "PermissionRequest",
    ];

    /// kimi 0.20 支持的全部 hook 事件（HOOK_EVENT_TYPES）。一条非法 event 会让 kimi **静默禁用全部**
    /// hooks（源码 salvageConfigData），故 EVENTS 有针对本表的绊线测试。
    pub const EVENT_WHITELIST: [&str; 16] = [
        "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionResult",
        "UserPromptSubmit", "Stop", "StopFailure", "Interrupt", "SessionStart", "SessionEnd",
        "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "Notification",
    ];

    pub fn ensure_hooks(cur_text: &str, reporter: &str, provider: &str) -> EnsureOutcome {
        // 解析失败绝不写（kimi 自身对坏文件同样拒写）。
        let Ok(mut doc) = cur_text.parse::<DocumentMut>() else {
            return EnsureOutcome::Abandon(RepairReason::ConfigUnreadable);
        };
        match merge(&mut doc, reporter, provider) {
            Merge::Changed => EnsureOutcome::Changed(doc.to_string()),
            Merge::Unchanged => EnsureOutcome::Unchanged,
            Merge::Abandon => EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
        }
    }

    enum Merge {
        Changed,
        Unchanged,
        Abandon,
    }

    /// 幂等合并 `[[hooks]]`：逐 EVENTS 找认领条目（event 相符 + command 可认领），路径不符则更新
    /// command；缺则追加 `{event, command, timeout=5}`。用户条目一概不动。
    ///
    /// 纪律（源码调研 kimi 0.20）：kimi 自身会全量重写此文件（注释全丢）——幂等判定只按
    /// (event, command) 内容匹配，绝不依赖注释标记。
    fn merge(doc: &mut DocumentMut, reporter: &str, provider: &str) -> Merge {
        let desired_cmd = format!("{reporter} --provider {provider}");
        let mut changed = false;

        if doc.get("hooks").and_then(|it| it.as_array_of_tables()).is_none() {
            // 不是 array-of-tables。可安全替换的只有两种：
            //   - 不存在：新版 kimi-code 默认无顶层 hooks 键。
            //   - `hooks = []`：旧 Python 版 kimi-cli 的默认**空内联数组**——语义等价于「无 hooks」，
            //     与 `[[hooks]]` array-of-tables 是同一 TOML 类型（空列表），替换无损，kimi 照常读取。
            // 非空内联数组 / 字符串等畸形形态则保守放弃：无法结构保持地转换，绝不写坏用户配置。
            let replaceable = match doc.get("hooks") {
                None => true,
                Some(it) => it.as_array().is_some_and(|a| a.is_empty()),
            };
            if !replaceable {
                return Merge::Abandon;
            }
            doc.remove("hooks");
            doc.insert("hooks", Item::ArrayOfTables(ArrayOfTables::new()));
            changed = true;
        }
        // 上面已确保 hooks 是 array-of-tables（否则已提前返回），故此处必不为 None——
        // 仍用 let-else 而非 expect/panic，纪律同 codex 先例。
        let Some(arr) = doc["hooks"].as_array_of_tables_mut() else {
            return Merge::Abandon;
        };

        for ev in EVENTS {
            let mut found = false;
            for t in arr.iter_mut() {
                if t.get("event").and_then(|v| v.as_str()) != Some(ev) {
                    continue;
                }
                let Some(path) = t
                    .get("command")
                    .and_then(|v| v.as_str())
                    .and_then(|c| claim_provider_cmd(c, provider))
                else {
                    continue; // 该事件上的用户自有 hook，不动
                };
                found = true;
                if path != reporter {
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
        if changed {
            Merge::Changed
        } else {
            Merge::Unchanged
        }
    }

    /// SessionStart 的 `[[hooks]]` 条目里是否有当前 meowo-reporter。解析失败 → false（Missing）——
    /// 调用方对「读不出来」另有 Unknown 通道，此处只回答「确定接上了吗」。
    pub fn has_reporter(cur_text: &str, provider: &str) -> bool {
        session_start_reporter(cur_text, provider).is_some_and(|p| is_current_reporter(&p))
    }

    /// 任一 `[[hooks]]` 条目中已认领的当前 reporter 路径（不限事件：接线时只需知道二进制在哪）。
    pub fn claimed_reporter(cur_text: &str, provider: &str) -> Option<String> {
        // 迭代器借用 doc；必须先收集出 owned String 再让 doc 出作用域。
        let doc = cur_text.parse::<DocumentMut>().ok()?;
        let found = hook_tables(&doc)
            .filter_map(|t| t.get("command").and_then(|v| v.as_str()).and_then(|c| claim_provider_cmd(c, provider)))
            .find(|p| is_current_reporter(p));
        found
    }

    /// SessionStart 条目上被认领的 reporter 路径（可能是历史 cc-reporter）。
    fn session_start_reporter(cur_text: &str, provider: &str) -> Option<String> {
        let doc = cur_text.parse::<DocumentMut>().ok()?;
        let found = hook_tables(&doc)
            .filter(|t| t.get("event").and_then(|v| v.as_str()) == Some("SessionStart"))
            .find_map(|t| t.get("command").and_then(|v| v.as_str()).and_then(|c| claim_provider_cmd(c, provider)));
        found
    }

    /// 顶层 `[[hooks]]` 的各条目（键缺失或非 array-of-tables → 空迭代）。
    fn hook_tables(doc: &DocumentMut) -> impl Iterator<Item = &Table> {
        doc.get("hooks")
            .and_then(|it| it.as_array_of_tables())
            .into_iter()
            .flat_map(|a| a.iter())
    }
}

pub use kimi_toml::{EVENTS as KIMI_EVENTS, EVENT_WHITELIST as KIMI_EVENT_WHITELIST};

#[cfg(test)]
mod tests {
    use super::*;

    const KT: ConfigFormat = ConfigFormat::KimiToml;

    fn changed(text: &str, reporter: &str) -> String {
        match KT.ensure_hooks(text, reporter, "kimi") {
            EnsureOutcome::Changed(s) => s,
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    // ── claim_provider_cmd ──

    #[test]
    fn claim_provider_cmd_strict() {
        // 认领：带引号/裸路径两种形态。
        assert_eq!(
            claim_provider_cmd("\"C:/x/meowo-reporter.exe\" --provider codex", "codex").as_deref(),
            Some("C:/x/meowo-reporter.exe")
        );
        assert_eq!(
            claim_provider_cmd("C:/x/meowo-reporter.exe --provider kimi", "kimi").as_deref(),
            Some("C:/x/meowo-reporter.exe")
        );
        // 历史遗留 cc-reporter 也认领，便于升级时替换旧 hooks。
        assert_eq!(
            claim_provider_cmd("C:/x/cc-reporter.exe --provider kimi", "kimi").as_deref(),
            Some("C:/x/cc-reporter.exe")
        );
        // 拒绝：provider 不符 / 无参数 / 多余参数 / 别的可执行 / 子串陷阱。
        assert!(claim_provider_cmd("C:/x/meowo-reporter.exe --provider codex", "kimi").is_none());
        assert!(claim_provider_cmd("\"C:/x/meowo-reporter.exe\"", "codex").is_none());
        assert!(claim_provider_cmd("C:/x/meowo-reporter.exe --provider codex --v", "codex").is_none());
        assert!(claim_provider_cmd("node meowo-reporter-notify.js --provider codex", "codex").is_none());
        assert!(claim_provider_cmd("C:/x/cc-reporter-not-us.exe --provider codex", "codex").is_none());
    }

    // ── KimiToml::ensure_hooks ──

    #[test]
    fn kimi_events_all_in_upstream_whitelist() {
        // 防连坐绊线：一条非法 event 会让 kimi 静默禁用全部 hooks。
        for ev in KIMI_EVENTS {
            assert!(KIMI_EVENT_WHITELIST.contains(&ev), "{ev} 不在 kimi 0.20 事件白名单");
        }
    }

    #[test]
    fn adds_all_when_absent_and_preserves_content() {
        let src = "default_model = \"kimi-code/kimi-for-coding\"\n# 用户注释\n[loop_control]\nmax_steps_per_turn = 100\n";
        let out = changed(src, "C:/x/meowo-reporter.exe");
        assert!(out.contains("# 用户注释")); // 结构保持：注释仍在
        assert!(out.contains("max_steps_per_turn = 100"));
        for ev in KIMI_EVENTS {
            assert!(out.contains(&format!("event = \"{ev}\"")), "{ev} 未写入");
        }
        assert!(out.contains(r#"command = "C:/x/meowo-reporter.exe --provider kimi""#));
        // 幂等
        assert_eq!(KT.ensure_hooks(&out, "C:/x/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn adopts_manual_and_updates_stale_path() {
        // 复刻手工接线形态：裸路径命令、6 事件、timeout 5。
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!("[[hooks]]\nevent = \"{ev}\"\ncommand = \"{dev} --provider kimi\"\ntimeout = 5\n\n"));
        }
        // 路径一致：无改动。
        assert_eq!(KT.ensure_hooks(&src, dev, "kimi"), EnsureOutcome::Unchanged);
        // 路径失效换 sidecar：6 条 command 全部更新，用户键 theme 不动。
        let out = changed(&src, "C:/app/meowo-reporter.exe");
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
    }

    #[test]
    fn updates_legacy_cc_reporter_paths() {
        // 项目改名后，旧 hooks 仍指向 cc-reporter.exe；应被认领并更新为 meowo-reporter。
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!(
                "[[hooks]]\nevent = \"{ev}\"\ncommand = \"C:/x/cc-reporter.exe --provider kimi\"\ntimeout = 5\n\n"
            ));
        }
        // 接线前：SessionStart 上挂的是废弃的 cc-reporter → 不算已接入。
        assert!(!KT.has_reporter(&src, "kimi"));
        assert_eq!(KT.claimed_reporter(&src, "kimi"), None);

        let out = changed(&src, "C:/app/meowo-reporter.exe");
        assert_eq!(out.matches("cc-reporter").count(), 0);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        assert!(out.contains("theme = \"light\""));
        assert!(KT.has_reporter(&out, "kimi"));
        assert_eq!(KT.claimed_reporter(&out, "kimi").as_deref(), Some("C:/app/meowo-reporter.exe"));
        assert_eq!(KT.ensure_hooks(&out, "C:/app/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn keeps_user_hook_entries() {
        let src = "[[hooks]]\nevent = \"Notification\"\ncommand = \"my-notify --ding\"\ntimeout = 3\n";
        let out = changed(src, "C:/x/meowo-reporter.exe");
        assert!(out.contains("my-notify --ding")); // 用户 hook 原样
        assert_eq!(out.matches("--provider kimi").count(), 6);
    }

    #[test]
    fn abandons_on_nonempty_or_malformed_hooks() {
        // hooks 键存在但非 array-of-tables 且不可安全替换（非空内联数组 / 字符串）：放弃不写坏。
        for src in ["hooks = [1, 2]\n", "hooks = \"oops\"\n"] {
            assert_eq!(
                KT.ensure_hooks(src, "C:/x/meowo-reporter.exe", "kimi"),
                EnsureOutcome::Abandon(RepairReason::ConfigUnreadable),
                "src={src:?}"
            );
        }
    }

    #[test]
    fn abandons_on_invalid_toml() {
        assert_eq!(
            KT.ensure_hooks("this is not = = toml\n", "C:/x/meowo-reporter.exe", "kimi"),
            EnsureOutcome::Abandon(RepairReason::ConfigUnreadable)
        );
    }

    #[test]
    fn replaces_empty_inline_array_from_legacy_kimi() {
        // 旧 Python 版 kimi-cli 的真实结构：顶层标量 + `hooks = []` 空内联数组 + 各 [section] 表。
        // 空数组语义等价于无 hooks，应被替换为 [[hooks]] 并写入 6 条，且其余键/表原样保留。
        let src = "\
default_model = \"kimi-code/kimi-for-coding\"
theme = \"dark\"
hooks = []
merge_all_available_skills = true

[models.\"kimi-code/kimi-for-coding\"]
provider = \"managed:kimi-code\"

[providers.\"managed:kimi-code\"]
type = \"managed\"
api_key = \"secret-should-survive\"

[loop_control]
max_steps_per_turn = 100
";
        let out = changed(src, "C:/app/meowo-reporter.exe");
        // 输出必须仍是合法 TOML（array-of-tables 不能错位插到 section 里）。
        let reparsed: toml_edit::DocumentMut = out.parse().expect("产物应为合法 TOML");
        let arr = reparsed["hooks"].as_array_of_tables().expect("hooks 应为 array-of-tables");
        assert_eq!(arr.len(), 6);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        for ev in KIMI_EVENTS {
            assert!(out.contains(&format!("event = \"{ev}\"")), "{ev} 未写入");
        }
        // 其余键与 [section] 表原样保留（含 api_key，绝不能丢）。
        assert_eq!(reparsed["default_model"].as_str(), Some("kimi-code/kimi-for-coding"));
        assert_eq!(reparsed["merge_all_available_skills"].as_bool(), Some(true));
        assert_eq!(reparsed["providers"]["managed:kimi-code"]["api_key"].as_str(), Some("secret-should-survive"));
        assert_eq!(reparsed["loop_control"]["max_steps_per_turn"].as_integer(), Some(100));
        assert_eq!(KT.ensure_hooks(&out, "C:/app/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    // ── KimiToml::has_reporter ──

    #[test]
    fn has_reporter_only_counts_session_start() {
        let session_start = "[[hooks]]\nevent = \"SessionStart\"\ncommand = \"/home/u/.local/meowo-reporter --provider kimi\"\ntimeout = 5\n";
        assert!(KT.has_reporter(session_start, "kimi"));
        assert!(!KT.has_reporter(session_start, "codex")); // provider 不符

        // 只在 Stop 挂了 reporter：不能保证新会话入库，不应判定为已接入。
        let stop_only = "[[hooks]]\nevent = \"Stop\"\ncommand = \"/home/u/.local/meowo-reporter --provider kimi\"\ntimeout = 5\n";
        assert!(!KT.has_reporter(stop_only, "kimi"));
        // 但接线时仍能从中取到 reporter 位置（不限事件）。
        assert_eq!(KT.claimed_reporter(stop_only, "kimi").as_deref(), Some("/home/u/.local/meowo-reporter"));

        // Stop 块在前、SessionStart 块在后：不串块。
        let both = format!("{stop_only}\n{session_start}");
        assert!(KT.has_reporter(&both, "kimi"));

        // 非 hooks 结构 / 用户自有命令 / 畸形 TOML → false。
        assert!(!KT.has_reporter("event = \"SessionStart\"\ncommand = \"node a.js\"\n", "kimi"));
        assert!(!KT.has_reporter("[[hooks]]\nevent = \"SessionStart\"\ncommand = \"node a.js\"\n", "kimi"));
        assert!(!KT.has_reporter("= = 非法 toml", "kimi"));
    }
}
