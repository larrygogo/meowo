//! hooks 接线的**声明**与**纯函数实现**：一个 [`HookSpec`] 描述某个变体「hooks 写在哪个文件、
//! 什么格式、挂哪些事件、命令长什么样」；三个方法回答「确保接入 / 是否已接入 / 二进制在哪」。
//!
//! 全是纯函数（输入配置文本，输出新文本或判定），落盘由调用方负责——便于单测，也保证
//! 「解析失败绝不写」。带副作用的接线步骤（claude 的 statusline 脚本、codex 的 trusted_hash）
//! 不在这里：它们要 `fsutil`/`sha2`/`db_path`，由 meowo-app 的 `SetupBehavior` 承接。

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
    /// kimi：`config.toml` 顶层 `[[hooks]]` array-of-tables。
    KimiToml,
}

/// 配置文件不存在时怎么办。声明式，避免每个 agent 的 `apply` 各写一遍 NotFound 分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingConfig {
    /// 从这段初始文本起（如 claude 的 `{}`、codex 的 `{"hooks":{}}`）。
    CreateFrom(&'static str),
    /// 不创建，直接失败（如 kimi 的 config.toml 需先 `kimi login`）。
    Fail(RepairReason),
}

/// hook command 的书写形态。三家各不相同，且**不能随意统一**：
/// 引号与否影响既有配置的幂等判定，`--provider` 与否影响认领规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// 可执行路径是否加双引号（claude/codex 加，kimi 不加——保持与各自现存配置一致）。
    pub quote_exe: bool,
    /// 是否追加 `--provider <id>`（codex/kimi 加；claude 靠 settings 里的位置区分，不带参）。
    pub with_provider: bool,
}

impl CommandSpec {
    /// 写出的 command 串。
    pub fn render(self, reporter: &str, agent_id: &str) -> String {
        let exe = if self.quote_exe { format!("\"{reporter}\"") } else { reporter.to_string() };
        if self.with_provider {
            format!("{exe} --provider {agent_id}")
        } else {
            exe
        }
    }

    /// 严格认领：可执行文件名恰为 meowo-reporter[.exe]（或历史遗留 cc-reporter[.exe]，以便升级时
    /// 替换旧路径），且余参**恰好**符合本形态。返回可执行路径。不裸 `contains`，不误伤用户 hook
    /// （如 `node tools/meowo-reporter-notify.js`）。
    pub fn claim(self, cmd: &str, agent_id: &str) -> Option<String> {
        let (path, args) = parse_hook_command(cmd)?;
        if !is_any_reporter(&path) {
            return None;
        }
        let args_ok = if self.with_provider {
            args == ["--provider", agent_id]
        } else {
            args.is_empty()
        };
        args_ok.then_some(path)
    }
}

/// 一个 hook 事件。`matcher` 仅 claude 有（同一事件下按 matcher 区分多条，与用户自有 hook 共存）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookEvent {
    pub name: &'static str,
    pub matcher: Option<&'static str>,
}

impl HookEvent {
    pub const fn plain(name: &'static str) -> Self {
        Self { name, matcher: None }
    }
    pub const fn matched(name: &'static str, matcher: &'static str) -> Self {
        Self { name, matcher: Some(matcher) }
    }
}

/// 某变体的 hooks 接线规格。加/改 agent 只写这张声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookSpec {
    /// 配置文件相对 `data_dir` 的路径（`settings.json` / `hooks.json` / `config.toml`）。
    pub config_rel: &'static str,
    pub format: ConfigFormat,
    pub missing: MissingConfig,
    pub events: &'static [HookEvent],
    pub command: CommandSpec,
}

impl HookSpec {
    /// 幂等接入 meowo-reporter hooks。用户自有的 hook 条目一概不动。
    pub fn ensure_hooks(&self, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        match self.format {
            ConfigFormat::KimiToml => kimi_toml::ensure_hooks(self, cur_text, reporter, agent_id),
        }
    }

    /// 新会话是否真能入库：**只看 SessionStart**，且必须是当前 meowo-reporter。仅在别的事件
    /// （如 Stop）挂了 reporter，不能保证新会话被记录，不应误判成已接入。
    pub fn has_reporter(&self, cur_text: &str, agent_id: &str) -> bool {
        self.claimed_at(cur_text, agent_id, Some(SESSION_START)).is_some_and(|p| is_current_reporter(&p))
    }

    /// 从既有配置里取出已认领的**当前** meowo-reporter 绝对路径，用于复用其位置（不限事件）。
    /// 历史 cc-reporter 不算：它已废弃，当成目标写回去 hooks 依旧失效。
    pub fn claimed_reporter(&self, cur_text: &str, agent_id: &str) -> Option<String> {
        self.claimed_at(cur_text, agent_id, None).filter(|p| is_current_reporter(p))
    }

    /// 认领到的 reporter 路径（可能是历史 cc-reporter）。`event=None` 表示不限事件。
    fn claimed_at(&self, cur_text: &str, agent_id: &str, event: Option<&str>) -> Option<String> {
        match self.format {
            ConfigFormat::KimiToml => kimi_toml::claimed_at(self, cur_text, agent_id, event),
        }
    }
}

/// 决定「新会话能否入库」的事件名。三家同名。
const SESSION_START: &str = "SessionStart";

// ═══ 命令行解析（各格式共用） ═══

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

fn file_name_lower(path: &str) -> Option<String> {
    Some(std::path::Path::new(path).file_name()?.to_str()?.to_ascii_lowercase())
}

/// 是 meowo-reporter 或历史遗留的 cc-reporter。
fn is_any_reporter(path: &str) -> bool {
    file_name_lower(path).is_some_and(|n| {
        matches!(n.as_str(), "meowo-reporter" | "meowo-reporter.exe" | "cc-reporter" | "cc-reporter.exe")
    })
}

/// 是**当前** meowo-reporter（排除历史 cc-reporter）。
fn is_current_reporter(path: &str) -> bool {
    file_name_lower(path).is_some_and(|n| matches!(n.as_str(), "meowo-reporter" | "meowo-reporter.exe"))
}

// ═══ KimiToml ═══

mod kimi_toml {
    use super::*;

    pub fn ensure_hooks(spec: &HookSpec, cur_text: &str, reporter: &str, agent_id: &str) -> EnsureOutcome {
        // 解析失败绝不写（kimi 自身对坏文件同样拒写）。
        let Ok(mut doc) = cur_text.parse::<DocumentMut>() else {
            return EnsureOutcome::Abandon(RepairReason::ConfigUnreadable);
        };
        match merge(spec, &mut doc, reporter, agent_id) {
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

    /// 幂等合并 `[[hooks]]`：逐事件找认领条目（event 相符 + command 可认领），路径不符则更新
    /// command；缺则追加 `{event, command, timeout=5}`。用户条目一概不动。
    ///
    /// 纪律（源码调研 kimi 0.20）：kimi 自身会全量重写此文件（注释全丢）——幂等判定只按
    /// (event, command) 内容匹配，绝不依赖注释标记。
    fn merge(spec: &HookSpec, doc: &mut DocumentMut, reporter: &str, agent_id: &str) -> Merge {
        let desired_cmd = spec.command.render(reporter, agent_id);
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
        // 仍用 let-else 而非 expect/panic。
        let Some(arr) = doc["hooks"].as_array_of_tables_mut() else {
            return Merge::Abandon;
        };

        for ev in spec.events {
            let mut found = false;
            for t in arr.iter_mut() {
                if t.get("event").and_then(|v| v.as_str()) != Some(ev.name) {
                    continue;
                }
                let Some(path) = t
                    .get("command")
                    .and_then(|v| v.as_str())
                    .and_then(|c| spec.command.claim(c, agent_id))
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
                t.insert("event", value(ev.name));
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

    /// 认领到的 reporter 路径。解析失败 → None（调用方对「读不出来」另有 Unknown 通道）。
    pub fn claimed_at(spec: &HookSpec, cur_text: &str, agent_id: &str, event: Option<&str>) -> Option<String> {
        // 迭代器借用 doc；必须先算出 owned String 再让 doc 出作用域。
        let doc = cur_text.parse::<DocumentMut>().ok()?;
        let found = doc
            .get("hooks")
            .and_then(|it| it.as_array_of_tables())
            .into_iter()
            .flat_map(|a| a.iter())
            .filter(|t| match event {
                None => true,
                Some(ev) => t.get("event").and_then(|v| v.as_str()) == Some(ev),
            })
            .find_map(|t| t.get("command").and_then(|v| v.as_str()).and_then(|c| spec.command.claim(c, agent_id)));
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// kimi 的接线规格（与 plugins/kimi.rs 同源，此处独立声明以免测试依赖插件表的演化）。
    const KIMI_CMD: CommandSpec = CommandSpec { quote_exe: false, with_provider: true };
    static KIMI_EVENTS: [HookEvent; 6] = [
        HookEvent::plain("SessionStart"),
        HookEvent::plain("UserPromptSubmit"),
        HookEvent::plain("PostToolUse"),
        HookEvent::plain("Stop"),
        HookEvent::plain("SessionEnd"),
        HookEvent::plain("PermissionRequest"),
    ];
    static KT: HookSpec = HookSpec {
        config_rel: "config.toml",
        format: ConfigFormat::KimiToml,
        missing: MissingConfig::Fail(RepairReason::NeedLogin),
        events: &KIMI_EVENTS,
        command: KIMI_CMD,
    };

    fn changed(text: &str, reporter: &str) -> String {
        match KT.ensure_hooks(text, reporter, "kimi") {
            EnsureOutcome::Changed(s) => s,
            other => panic!("期望 Changed，实得 {other:?}"),
        }
    }

    // ── CommandSpec ──

    #[test]
    fn render_covers_three_agent_shapes() {
        let claude = CommandSpec { quote_exe: true, with_provider: false };
        let codex = CommandSpec { quote_exe: true, with_provider: true };
        assert_eq!(claude.render("C:/x/meowo-reporter.exe", "claude"), "\"C:/x/meowo-reporter.exe\"");
        assert_eq!(codex.render("C:/x/meowo-reporter.exe", "codex"), "\"C:/x/meowo-reporter.exe\" --provider codex");
        assert_eq!(KIMI_CMD.render("C:/x/meowo-reporter.exe", "kimi"), "C:/x/meowo-reporter.exe --provider kimi");
    }

    #[test]
    fn claim_with_provider_is_strict() {
        // 认领：带引号/裸路径两种形态。
        assert_eq!(KIMI_CMD.claim("\"C:/x/meowo-reporter.exe\" --provider kimi", "kimi").as_deref(), Some("C:/x/meowo-reporter.exe"));
        assert_eq!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider kimi", "kimi").as_deref(), Some("C:/x/meowo-reporter.exe"));
        // 历史遗留 cc-reporter 也认领，便于升级时替换旧路径。
        assert_eq!(KIMI_CMD.claim("C:/x/cc-reporter.exe --provider kimi", "kimi").as_deref(), Some("C:/x/cc-reporter.exe"));
        // 拒绝：agent 不符 / 无参数 / 多余参数 / 别的可执行 / 子串陷阱。
        assert!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider codex", "kimi").is_none());
        assert!(KIMI_CMD.claim("\"C:/x/meowo-reporter.exe\"", "kimi").is_none());
        assert!(KIMI_CMD.claim("C:/x/meowo-reporter.exe --provider kimi --v", "kimi").is_none());
        assert!(KIMI_CMD.claim("node meowo-reporter-notify.js --provider kimi", "kimi").is_none());
        assert!(KIMI_CMD.claim("C:/x/cc-reporter-not-us.exe --provider kimi", "kimi").is_none());
    }

    #[test]
    fn claim_bare_quoted_rejects_any_argument() {
        // claude 形态：单个（可带引号的）可执行路径，禁带参数。
        let bare = CommandSpec { quote_exe: true, with_provider: false };
        assert_eq!(bare.claim("\"C:/x/meowo-reporter.exe\"", "claude").as_deref(), Some("C:/x/meowo-reporter.exe"));
        assert_eq!(bare.claim("/usr/local/bin/meowo-reporter", "claude").as_deref(), Some("/usr/local/bin/meowo-reporter"));
        // 带参数 = 不是我们写的那条；用户自有 hook 一概不认领。
        assert!(bare.claim("\"C:/x/meowo-reporter.exe\" --flag", "claude").is_none());
        assert!(bare.claim("node tools/meowo-reporter-notify.js", "claude").is_none());
        assert!(bare.claim("/opt/meowo-reporter/run.sh", "claude").is_none());
        assert!(bare.claim("meowo-reporter-wrapper", "claude").is_none());
        assert!(bare.claim("", "claude").is_none());
    }

    // ── KimiToml::ensure_hooks ──

    #[test]
    fn adds_all_when_absent_and_preserves_content() {
        let src = "default_model = \"kimi-code/kimi-for-coding\"\n# 用户注释\n[loop_control]\nmax_steps_per_turn = 100\n";
        let out = changed(src, "C:/x/meowo-reporter.exe");
        assert!(out.contains("# 用户注释")); // 结构保持：注释仍在
        assert!(out.contains("max_steps_per_turn = 100"));
        for ev in KIMI_EVENTS {
            assert!(out.contains(&format!("event = \"{}\"", ev.name)), "{} 未写入", ev.name);
        }
        assert!(out.contains(r#"command = "C:/x/meowo-reporter.exe --provider kimi""#));
        assert_eq!(KT.ensure_hooks(&out, "C:/x/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    #[test]
    fn adopts_manual_and_updates_stale_path() {
        let dev = "C:/Users/larry/Desktop/workspace/meowo/target/release/meowo-reporter.exe";
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!("[[hooks]]\nevent = \"{}\"\ncommand = \"{dev} --provider kimi\"\ntimeout = 5\n\n", ev.name));
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
        let mut src = String::from("theme = \"light\"\n");
        for ev in KIMI_EVENTS {
            src.push_str(&format!(
                "[[hooks]]\nevent = \"{}\"\ncommand = \"C:/x/cc-reporter.exe --provider kimi\"\ntimeout = 5\n\n",
                ev.name
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
        let reparsed: DocumentMut = out.parse().expect("产物应为合法 TOML");
        let arr = reparsed["hooks"].as_array_of_tables().expect("hooks 应为 array-of-tables");
        assert_eq!(arr.len(), 6);
        assert_eq!(out.matches(r#"command = "C:/app/meowo-reporter.exe --provider kimi""#).count(), 6);
        // 其余键与 [section] 表原样保留（含 api_key，绝不能丢）。
        assert_eq!(reparsed["default_model"].as_str(), Some("kimi-code/kimi-for-coding"));
        assert_eq!(reparsed["merge_all_available_skills"].as_bool(), Some(true));
        assert_eq!(reparsed["providers"]["managed:kimi-code"]["api_key"].as_str(), Some("secret-should-survive"));
        assert_eq!(reparsed["loop_control"]["max_steps_per_turn"].as_integer(), Some(100));
        assert_eq!(KT.ensure_hooks(&out, "C:/app/meowo-reporter.exe", "kimi"), EnsureOutcome::Unchanged);
    }

    // ── has_reporter / claimed_reporter ──

    #[test]
    fn has_reporter_only_counts_session_start() {
        let session_start = "[[hooks]]\nevent = \"SessionStart\"\ncommand = \"/home/u/.local/meowo-reporter --provider kimi\"\ntimeout = 5\n";
        assert!(KT.has_reporter(session_start, "kimi"));
        assert!(!KT.has_reporter(session_start, "codex")); // agent 不符

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
