//! Meowo 依赖的 CLI 输出文案清单，供「CLI 漂移检查」脚本（`app/scripts/cli-drift.mjs`）用。
//!
//! 为什么要有：CLI 升级后改了一句提示，Meowo 的识别规则就悄悄失效（2026-11-12 调研
//! research-cli-drift.md：五个 CLI 共 16 条已失效，全是升级后静默漂移，没有一条是先发现的）。
//! 脚本在本机已装 CLI 的程序文件里逐条搜这些文案，缺了就报出来，赶在用户撞上之前。
//!
//! 清单不手抄，从规则本身来，分两部分：
//! - **自动抽取**的字面量：插件声明里的信任页标记、模式回显、选择器锚点、侧问/撤回/草稿
//!   标记、子代理占位，以及屏幕规则里的子串谓词。规则改了，清单跟着变。
//! - **插件手工声明**的 [`CliTextDep`]（`AgentPlugin::cli_text_deps`）：正则规则抽不出字面量，
//!   由插件给一条真实输出样例和要搜的静态片段；运行时拼出来、程序文件里没有原样的字面量，
//!   也在这里给替代片段或说明。测试保证每条声明都对得上现行规则（规则改了声明没改就失败），
//!   并保证声明完整的插件每条正则规则都有样例。
//!
//! `cargo test -p meowo-agent --lib` 把清单写到 `app/src/generated/cli-text-deps.json`，
//! CI 校验无 diff（与 ts-rs 契约同一套机制）。

use serde::Serialize;

/// 插件手工声明的一条 CLI 文案依赖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliTextDep {
    /// 指向插件里真正在用的那条规则：
    /// - `effort.confirm` / `effort.reject` / `model.confirm` / `model.reject` / `undo.reject` /
    ///   `status_line` / `retry_line` / `clipboard_image`：对应能力槽的正则；
    /// - `attention:<id>`：该提示规则的任一正则；
    /// - `screen:<id>`：该屏幕规则里的任一 `LineRegex`；
    /// - `literal:<文案>`：自动抽取的某个字面量（给替代搜索片段，或标成旧版兼容）。
    pub rule: &'static str,
    /// 在 CLI 程序文件里搜的片段（模板的静态部分，不区分大小写）。空串 = 程序文件里没有可搜的
    /// 原样片段（运行时拼接），只做样例测试，`evidence` 写明为什么。
    pub needle: &'static str,
    /// 这条文案的真实输出原文（屏幕或 transcript 实样；只有模板时按模板填占位符）。
    /// 测试用 `rule` 指向的规则去匹配它，并要求它含 `needle`。
    pub sample: &'static str,
    /// 取证出处：CLI 版本、来源、日期。
    pub evidence: &'static str,
    /// 旧版兼容规则：当前版本程序文件里没有是正常的（老版本用户还在，规则不删）。
    pub legacy: bool,
}

impl CliTextDep {
    pub const fn new(
        rule: &'static str,
        needle: &'static str,
        sample: &'static str,
        evidence: &'static str,
    ) -> Self {
        Self {
            rule,
            needle,
            sample,
            evidence,
            legacy: false,
        }
    }

    /// 标成旧版兼容。
    pub const fn legacy(mut self) -> Self {
        self.legacy = true;
        self
    }
}

/// 清单里的一条：脚本据此搜程序文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestEntry {
    pub rule: String,
    /// 要搜的片段；空 = 不搜（见 `note`）。
    pub needle: String,
    pub legacy: bool,
    /// 取证出处或不搜的理由。
    pub note: String,
}

/// 一条规则的可检查形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleText {
    /// 子串规则（不区分大小写）。
    Literal(&'static str),
    /// 正则规则（任一命中即可；按 `(?im)` 编译，与前端 `new RegExp(…, "im")`、屏幕引擎 `(?i)` 同口径）。
    Regex(Vec<&'static str>),
}

/// 字面量太短或不含字母（`❯`、`●`）时搜它没有意义：任何程序文件里都有。
fn worth_scanning(literal: &str) -> bool {
    literal.chars().count() >= 4 && literal.chars().any(|c| c.is_ascii_alphabetic())
}

fn collect_matcher(m: &crate::screen::Matcher, out: &mut Vec<&'static str>) {
    use crate::screen::Matcher as M;
    match m {
        M::Contains(list)
        | M::AnyContains(list)
        | M::LineStartsWith(list)
        | M::BraillePrefixed(list) => out.extend(list.iter().copied()),
        M::LineStartsWithContaining(prefix, list) => {
            out.push(prefix);
            out.extend(list.iter().copied());
        }
        M::All(list) | M::Any(list) | M::Not(list) => {
            for sub in list.iter() {
                collect_matcher(sub, out);
            }
        }
        _ => {}
    }
}

fn collect_regex(m: &crate::screen::Matcher, out: &mut Vec<&'static str>) {
    use crate::screen::Matcher as M;
    match m {
        M::LineRegex(p) => out.push(p),
        // 否决门里的正则是「不能出现」，不是依赖。
        M::All(list) | M::Any(list) => {
            for sub in list.iter() {
                collect_regex(sub, out);
            }
        }
        _ => {}
    }
}

/// 自动抽取插件声明里的字面量（去重，保持声明顺序）。
pub fn auto_literals(p: &dyn crate::AgentPlugin) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    out.extend(p.startup_attention_markers().iter().copied());
    for control in p.mode_controls() {
        out.extend(control.screen_markers.iter().map(|m| m.marker));
    }
    out.extend(p.selector_anchors().iter().map(|a| a.marker));
    if let Some(side) = p.side_question() {
        out.push(side.footer_marker);
        out.push(side.ready_marker);
    }
    if let Some(menu) = p.undo_control().and_then(|u| u.menu) {
        out.push(menu.ready_marker);
        out.push(menu.option_label);
    }
    if let Some(stash) = p.draft_stash() {
        out.push(stash.stashed_marker);
    }
    if let Some(view) = p.subagent_view() {
        out.push(view.placeholder_prefix);
    }
    for rule in p.screen_rules() {
        collect_matcher(&rule.matcher, &mut out);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|l| worth_scanning(l) && seen.insert(l.to_lowercase()));
    out
}

/// 插件里所有正则规则的键（声明完整的插件，每个键都要有样例）。
pub fn regex_rule_keys(p: &dyn crate::AgentPlugin) -> Vec<String> {
    let mut keys = Vec::new();
    if p.effort_control().is_some() {
        keys.push("effort.confirm".to_string());
        keys.push("effort.reject".to_string());
    }
    if p.model_select_reply().is_some() {
        keys.push("model.confirm".to_string());
        keys.push("model.reject".to_string());
    }
    if p.undo_control().and_then(|u| u.reject_pattern).is_some() {
        keys.push("undo.reject".to_string());
    }
    if p.status_line_pattern().is_some() {
        keys.push("status_line".to_string());
    }
    if p.retry_line_pattern().is_some() {
        keys.push("retry_line".to_string());
    }
    if p.clipboard_image_paste(Some("0")).is_some() {
        keys.push("clipboard_image".to_string());
    }
    for a in p.attention_patterns() {
        keys.push(format!("attention:{}", a.id));
    }
    for rule in p.screen_rules() {
        let mut regex = Vec::new();
        collect_regex(&rule.matcher, &mut regex);
        if !regex.is_empty() {
            keys.push(format!("screen:{}", rule.id));
        }
    }
    keys
}

/// 把规则键解析成插件里现行的规则文本；None = 规则已不存在（清单过期）。
pub fn resolve(p: &dyn crate::AgentPlugin, rule: &str) -> Option<RuleText> {
    if let Some(literal) = rule.strip_prefix("literal:") {
        return auto_literals(p)
            .into_iter()
            .find(|l| l.eq_ignore_ascii_case(literal))
            .map(RuleText::Literal);
    }
    if let Some(id) = rule.strip_prefix("attention:") {
        return p
            .attention_patterns()
            .iter()
            .find(|a| a.id == id)
            .map(|a| RuleText::Regex(a.patterns.to_vec()));
    }
    if let Some(id) = rule.strip_prefix("screen:") {
        let found = p.screen_rules().iter().find(|r| r.id == id)?;
        let mut regex = Vec::new();
        collect_regex(&found.matcher, &mut regex);
        return (!regex.is_empty()).then_some(RuleText::Regex(regex));
    }
    let one = |p: Option<&'static str>| p.map(|p| RuleText::Regex(vec![p]));
    match rule {
        "effort.confirm" => one(p.effort_control().map(|e| e.confirm_pattern)),
        "effort.reject" => one(p.effort_control().map(|e| e.reject_pattern)),
        "model.confirm" => one(p.model_select_reply().map(|r| r.confirm_pattern)),
        "model.reject" => one(p.model_select_reply().map(|r| r.reject_pattern)),
        "undo.reject" => one(p.undo_control().and_then(|u| u.reject_pattern)),
        "status_line" => one(p.status_line_pattern()),
        "retry_line" => one(p.retry_line_pattern()),
        "clipboard_image" => one(p.clipboard_image_paste(Some("0"))),
        _ => None,
    }
}

/// 一个插件的完整清单：自动抽取的字面量（被声明覆盖的按声明）+ 声明的正则样例。
pub fn manifest(p: &dyn crate::AgentPlugin) -> Vec<ManifestEntry> {
    let deps = p.cli_text_deps();
    let mut out = Vec::new();
    for literal in auto_literals(p) {
        let overrides: Vec<&CliTextDep> = deps
            .iter()
            .filter(|d| {
                d.rule
                    .strip_prefix("literal:")
                    .is_some_and(|l| l.eq_ignore_ascii_case(literal))
            })
            .collect();
        if overrides.is_empty() {
            out.push(ManifestEntry {
                rule: format!("literal:{literal}"),
                needle: literal.to_string(),
                legacy: false,
                note: String::new(),
            });
        }
        for d in overrides {
            out.push(ManifestEntry {
                rule: d.rule.to_string(),
                needle: d.needle.to_string(),
                legacy: d.legacy,
                note: d.evidence.to_string(),
            });
        }
    }
    for d in deps.iter().filter(|d| !d.rule.starts_with("literal:")) {
        out.push(ManifestEntry {
            rule: d.rule.to_string(),
            needle: d.needle.to_string(),
            legacy: d.legacy,
            note: d.evidence.to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// 声明已补全的插件：每条正则规则都必须有真实样例。其余插件的识别规则正由别组改动
    /// （2026-11-12 第三轮），合并时补齐声明后加进来。
    const COMPLETE: &[&str] = &["claude", "codex", "opencode"];

    fn compile(pattern: &str) -> regex::Regex {
        regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .multi_line(true)
            .build()
            .unwrap_or_else(|e| panic!("规则编译失败 {pattern}: {e}"))
    }

    fn matches(rule: &RuleText, sample: &str) -> bool {
        match rule {
            RuleText::Literal(l) => sample.to_lowercase().contains(&l.to_lowercase()),
            RuleText::Regex(list) => list.iter().any(|p| compile(p).is_match(sample)),
        }
    }

    #[test]
    fn declared_deps_match_live_rules() {
        for p in crate::registry::all() {
            for d in p.cli_text_deps() {
                let rule = resolve(*p, d.rule).unwrap_or_else(|| {
                    panic!(
                        "{} 的清单条目 `{}` 指向的规则已不存在：规则改了，清单也要改",
                        p.id(),
                        d.rule
                    )
                });
                assert!(
                    matches(&rule, d.sample),
                    "{} 的 `{}`：样例 {:?} 不再被现行规则命中（规则改了要同步样例与取证）",
                    p.id(),
                    d.rule,
                    d.sample
                );
                assert!(
                    d.sample.to_lowercase().contains(&d.needle.to_lowercase()),
                    "{} 的 `{}`：搜索片段 {:?} 不在样例里",
                    p.id(),
                    d.rule,
                    d.needle
                );
                assert!(!d.evidence.is_empty(), "{} 的 `{}` 缺出处", p.id(), d.rule);
                if d.needle.is_empty() {
                    assert!(
                        d.evidence.len() > 8,
                        "{} 的 `{}` 不搜程序文件，要写明理由",
                        p.id(),
                        d.rule
                    );
                }
            }
        }
    }

    #[test]
    fn complete_plugins_have_a_sample_for_every_regex_rule() {
        for p in crate::registry::all() {
            if !COMPLETE.contains(&p.id().as_str()) {
                continue;
            }
            for key in regex_rule_keys(*p) {
                assert!(
                    p.cli_text_deps().iter().any(|d| d.rule == key),
                    "{} 新增或改名了正则规则 `{key}`，要在 cli_text_deps 里补一条真实样例",
                    p.id()
                );
            }
        }
    }

    /// 规则在源码里的位置：缺失时脚本把人直接指过去。按字面在插件目录、再在 crate 根下找。
    fn locate(src: &Path, plugin: &str, text: &str) -> String {
        let mut files: Vec<PathBuf> = Vec::new();
        if let Ok(dir) = std::fs::read_dir(src.join("plugins").join(plugin)) {
            // 跳过声明文件本身：要指向规则所在处，不是指向清单。
            let mut list: Vec<PathBuf> = dir
                .flatten()
                .map(|e| e.path())
                .filter(|f| f.file_name().is_some_and(|n| n != "cli_text.rs"))
                .collect();
            list.sort();
            files.extend(list);
        }
        for f in ["chat_ui.rs", "registry.rs", "screen.rs"] {
            files.push(src.join(f));
        }
        let rel = |f: &Path| {
            let tail = f
                .strip_prefix(src)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/");
            format!("app/src-tauri/crates/meowo-agent/src/{tail}")
        };
        for f in &files {
            let Ok(body) = std::fs::read_to_string(f) else {
                continue;
            };
            for (i, line) in body.lines().enumerate() {
                if line.contains(text) && !line.trim_start().starts_with("//") {
                    return format!("{}:{}", rel(f), i + 1);
                }
            }
        }
        rel(&src.join("plugins").join(plugin))
    }

    fn rule_text_for_location(p: &dyn crate::AgentPlugin, rule: &str) -> Option<String> {
        if let Some(l) = rule.strip_prefix("literal:") {
            return Some(format!("\"{l}\""));
        }
        if let Some(id) = rule
            .strip_prefix("attention:")
            .or_else(|| rule.strip_prefix("screen:"))
        {
            return Some(format!("\"{id}\""));
        }
        match resolve(p, rule)? {
            RuleText::Regex(list) => list.first().map(|s| s.to_string()),
            RuleText::Literal(l) => Some(l.to_string()),
        }
    }

    /// 把清单写进 `app/src/generated/cli-text-deps.json`（CI 校验无 diff）。
    #[test]
    fn manifest_snapshot() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut agents = Vec::new();
        for p in crate::registry::all() {
            let id = p.id().as_str();
            let entries: Vec<serde_json::Value> = manifest(*p)
                .into_iter()
                .map(|e| {
                    let at = rule_text_for_location(*p, &e.rule)
                        .map(|t| locate(&src, id, &t))
                        .unwrap_or_else(|| locate(&src, id, "\u{0}"));
                    serde_json::json!({
                        "rule": e.rule, "needle": e.needle, "legacy": e.legacy,
                        "note": e.note, "source": at,
                    })
                })
                .collect();
            agents.push(serde_json::json!({
                "id": id,
                "name": p.display_name(),
                "command": p.launch_stem(),
                "declared_complete": COMPLETE.contains(&id),
                "entries": entries,
            }));
        }
        let doc = serde_json::json!({
            "about": "由 meowo-agent 的 cli_text 测试生成，勿手改。用法：node app/scripts/cli-drift.mjs",
            "agents": agents,
        });
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/generated/cli-text-deps.json");
        let body = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
            std::fs::write(&path, body).unwrap();
        }
    }

    #[test]
    fn auto_literals_skip_glyphs_and_dedupe() {
        assert!(!worth_scanning("❯"));
        assert!(!worth_scanning("●"));
        assert!(worth_scanning("trust this folder"));
        let claude = crate::registry::all()
            .iter()
            .find(|p| p.id().as_str() == "claude")
            .unwrap();
        let lits = auto_literals(*claude);
        assert!(lits.contains(&"trust this folder"));
        assert!(lits.contains(&"manual mode on"));
        let mut lower: Vec<String> = lits.iter().map(|l| l.to_lowercase()).collect();
        lower.sort();
        lower.dedup();
        assert_eq!(lower.len(), lits.len());
    }
}
