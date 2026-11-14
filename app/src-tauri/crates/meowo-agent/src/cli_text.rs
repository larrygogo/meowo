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
//! - **分段近邻**（[`CliTextDep::near`]）：运行时拼接、整句没有原样的文案，给几个静态片段和最大
//!   间距，脚本要求它们在程序文件里相距不超过这个字节数。只搜一段泛词（`interrupt`、` to page`）
//!   哪个程序文件都能命中，永远报「找到」；整句又搜不到，只好标「不搜」（做空第一轮 #7，
//!   2026-11-12：26 条不搜里有 opencode `esc interrupt` 这类本轮正好漂移过的）。
//! - **记录格式**（[`CliTextDep::record`]）：transcript / wire / rollout 的事件名与字段。
//!   它们不是屏幕规则，却同样随 CLI 升级漂移（kimi `plan_mode.cancel` 是用户先撞上的，
//!   research-cli-drift K1）。
//! - **版本行**（[`CliTextDep::version_line`]）：`--version` 首行的格式（codex 的 `codex-cli`
//!   前缀是用户先撞上的，research-cli-drift X2）。不搜程序文件，脚本拿它比对 `<cli> --version`
//!   的真实首行——版本行是命令名与版本号运行时拼的，命令名在程序文件里的位置随链接布局变
//!   （codex 0.167.2 实测），按位置搜靠不住。
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
    /// - `option:<提示 id>#<序号>`：该提示规则里第几条选项角色规则；`option-ready:…`：那一项
    ///   现场打字流程的就绪标志（`TypedInput::ready_pattern`）；
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
    /// 分段近邻：这些片段都在、且相距不超过 `max_gap` 字节才算找到（`needle` 此时为空）。
    /// 片段两端可带 `"`，要求它是 JS 字符串字面量（`"esc "` 不会命中 `"escape"`）；去掉引号后
    /// 每段都要出现在样例里。
    pub parts: &'static [&'static str],
    pub max_gap: usize,
    /// 版本行依赖：`needle` 是 `<cli> --version` 首行该有的前缀，脚本拿它比对真实输出，不搜程序文件。
    pub version_line: bool,
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
            parts: &[],
            max_gap: 0,
            version_line: false,
        }
    }

    /// 运行时拼接的文案：各段都在且相距不超过 `max_gap` 字节。间距取本机程序文件实测值的
    /// 一倍余量（evidence 写明实测值），太宽就退化成「各自在」，失去意义。
    pub const fn near(
        rule: &'static str,
        parts: &'static [&'static str],
        max_gap: usize,
        sample: &'static str,
        evidence: &'static str,
    ) -> Self {
        Self {
            rule,
            needle: "",
            sample,
            evidence,
            legacy: false,
            parts,
            max_gap,
            version_line: false,
        }
    }

    /// 记录格式依赖：`rule` 写成 `record:<名字>`，`needle` 是 Meowo 解析时认的事件名/字段/前缀，
    /// `sample` 是一行真实记录。测试要求 `needle` 确实出现在插件源码里（解析器还在用它）。
    pub const fn record(
        rule: &'static str,
        needle: &'static str,
        sample: &'static str,
        evidence: &'static str,
    ) -> Self {
        Self::new(rule, needle, sample, evidence)
    }

    /// `--version` 首行的格式：`rule` 写成 `record:<名字>`，`prefix` 是首行该有的前缀（测试要求它在
    /// 插件源码里、且样例以它开头），`sample` 是一行真实的首行原文。
    ///
    /// 脚本拿 `prefix` 比对 `<cli> --version` 的真实首行，不搜程序文件。此前按「命令名与 usage 模板
    /// 相距 ≤N 字节」在程序文件里搜：命令名 `codex-cli` 是个泛词（它是 `codex-client\src\…` 包名路径
    /// 的前缀，单搜永远「找到」，做空第二轮 N2），只能靠位置锚；而 codex 0.167.2 的链接布局一变
    /// （命令名从 usage 模板后 10022 字节挪到了前 160400 字节，2026-11-14 实测），版本行明明没变
    /// 却报缺失。版本行是运行时拼的，真实输出才是它唯一可靠的出处——脚本本来就要跑 `--version`。
    pub const fn version_line(
        rule: &'static str,
        prefix: &'static str,
        sample: &'static str,
        evidence: &'static str,
    ) -> Self {
        let mut dep = Self::new(rule, prefix, sample, evidence);
        dep.version_line = true;
        dep
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
    /// 分段近邻的片段与最大间距（字节）；空 = 按 `needle` 整串搜。
    pub parts: Vec<String>,
    pub max_gap: usize,
    /// 非空 = 版本行依赖：不搜程序文件，`<cli> --version` 的首行要以它开头（此时 `needle` 为空）。
    pub version_prefix: String,
}

impl ManifestEntry {
    fn from_dep(d: &CliTextDep) -> Self {
        Self {
            rule: d.rule.to_string(),
            needle: if d.version_line { String::new() } else { d.needle.to_string() },
            legacy: d.legacy,
            note: d.evidence.to_string(),
            parts: d.parts.iter().map(|p| p.to_string()).collect(),
            max_gap: d.max_gap,
            version_prefix: if d.version_line { d.needle.to_string() } else { String::new() },
        }
    }
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
    if let Some(stash) = composer_for_scan(p).and_then(|c| c.stash) {
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

/// 取插件的输入框规格用来抽文案。规格按 CLI 版本声明（旧版本返回 None），清单要的是「现行
/// 版本的规则」：先按不带版本取，再按各主版本的一个很高的小版本取。
pub fn composer_for_scan(p: &dyn crate::AgentPlugin) -> Option<crate::chat_ui::ComposerSpec> {
    p.composer(None).or_else(|| {
        (0..10).find_map(|major| p.composer(Some(&format!("{major}.9999.0"))))
    })
}

/// 输入框规格里依赖 CLI 文案的正则：占位文字（`composer.empty`，没有占位的不算）、特殊输入
/// 模式的标记（`composer.alt:<id>`）、粘贴占位（`composer.paste`）与超出可视高度的滚动标记
/// （`composer.overflow`，不画标记的家没有）。
fn composer_rules(p: &dyn crate::AgentPlugin) -> Vec<(String, Vec<&'static str>)> {
    let Some(composer) = composer_for_scan(p) else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    if composer.empty_pattern != "^$" {
        rules.push(("composer.empty".to_string(), vec![composer.empty_pattern]));
    }
    for mode in composer.alt_modes {
        let patterns: Vec<&'static str> = [mode.prompt_pattern, mode.below_pattern]
            .into_iter()
            .flatten()
            .collect();
        rules.push((format!("composer.alt:{}", mode.id), patterns));
    }
    // 草稿保真（ComposerFidelity）：粘贴占位的格式变了，占位会被当成普通草稿清掉、原文丢失；
    // 滚动标记变了，没显示全的草稿会被当成全文搬走。两条都要盯。
    if let Some(fidelity) = composer.fidelity {
        rules.push(("composer.paste".to_string(), vec![fidelity.paste_pattern]));
        if let Some(overflow) = fidelity.overflow_pattern {
            rules.push(("composer.overflow".to_string(), vec![overflow]));
        }
    }
    rules
}

/// 插件里所有正则规则的键（声明完整的插件，每个键都要有样例）。
pub fn regex_rule_keys(p: &dyn crate::AgentPlugin) -> Vec<String> {
    let mut keys = Vec::new();
    keys.extend(composer_rules(p).into_iter().map(|(key, _)| key));
    if p.effort_control().is_some() {
        keys.push("effort.confirm".to_string());
        keys.push("effort.reject".to_string());
    }
    if p.model_select_reply().is_some() {
        keys.push("model.confirm".to_string());
        keys.push("model.reject".to_string());
    }
    if p.switch_confirm().is_some() {
        keys.push("switch_confirm.title".to_string());
        keys.push("switch_confirm.accept".to_string());
        keys.push("switch_confirm.auto".to_string());
    }
    if p.switch_confirm().and_then(|c| c.manual_pattern).is_some() {
        keys.push("switch_confirm.manual".to_string());
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
        // 选项角色规则同样是在认 CLI 的文案：选项改了说法，按钮就从「允许一次」掉成普通选项。
        // 一条规则一个键（`option:<提示 id>#<声明序号>`）；带现场打字流程的另有就绪标志一条。
        for (index, option) in a.options.iter().enumerate() {
            keys.push(format!("option:{}#{index}", a.id));
            if option.typed.is_some() {
                keys.push(format!("option-ready:{}#{index}", a.id));
            }
        }
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
    // `option:<提示 id>#<序号>` 与 `option-ready:<提示 id>#<序号>`：提示 id 里自带冒号，按最后一个 `#` 切。
    for (prefix, ready) in [("option-ready:", true), ("option:", false)] {
        if let Some(rest) = rule.strip_prefix(prefix) {
            let (id, index) = rest.rsplit_once('#')?;
            let option = p
                .attention_patterns()
                .iter()
                .find(|a| a.id == id)?
                .options
                .get(index.parse::<usize>().ok()?)?;
            return if ready {
                option.typed.map(|typed| RuleText::Regex(vec![typed.ready_pattern]))
            } else {
                Some(RuleText::Regex(vec![option.pattern]))
            };
        }
    }
    if let Some(id) = rule.strip_prefix("screen:") {
        let found = p.screen_rules().iter().find(|r| r.id == id)?;
        let mut regex = Vec::new();
        collect_regex(&found.matcher, &mut regex);
        return (!regex.is_empty()).then_some(RuleText::Regex(regex));
    }
    if rule.starts_with("composer.") {
        return composer_rules(p)
            .into_iter()
            .find(|(key, _)| key == rule)
            .map(|(_, patterns)| RuleText::Regex(patterns));
    }
    let one = |p: Option<&'static str>| p.map(|p| RuleText::Regex(vec![p]));
    match rule {
        "effort.confirm" => one(p.effort_control().map(|e| e.confirm_pattern)),
        "effort.reject" => one(p.effort_control().map(|e| e.reject_pattern)),
        "model.confirm" => one(p.model_select_reply().map(|r| r.confirm_pattern)),
        "model.reject" => one(p.model_select_reply().map(|r| r.reject_pattern)),
        "switch_confirm.title" => one(p.switch_confirm().map(|c| c.title_pattern)),
        "switch_confirm.accept" => one(p.switch_confirm().map(|c| c.accept_pattern)),
        "switch_confirm.auto" => one(p.switch_confirm().map(|c| c.auto_pattern)),
        "switch_confirm.manual" => one(p.switch_confirm().and_then(|c| c.manual_pattern)),
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
                parts: Vec::new(),
                max_gap: 0,
                version_prefix: String::new(),
            });
        }
        out.extend(overrides.into_iter().map(ManifestEntry::from_dep));
    }
    out.extend(
        deps.iter()
            .filter(|d| !d.rule.starts_with("literal:"))
            .map(ManifestEntry::from_dep),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// 声明已补全的插件：每条正则规则都必须有真实样例。新接入的插件补齐 cli_text_deps 后加进来
    ///（2026-11-12 第三轮五家全部补齐）。
    const COMPLETE: &[&str] = &["claude", "codex", "gemini", "kimi", "opencode"];

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

    /// 插件目录下的源码（不含声明文件本身），小写：记录格式依赖要在这里找到解析器在用的字符串。
    fn plugin_source_lower(plugin: &str) -> String {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/plugins")
            .join(plugin);
        let mut body = String::new();
        for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if f.file_name() != "cli_text.rs" {
                body.push_str(&std::fs::read_to_string(f.path()).unwrap_or_default());
            }
        }
        body.to_lowercase()
    }

    /// 片段去掉两端的 `"`（那只是要求它是 JS 字符串字面量），剩下的是样例里该有的文字。
    fn part_text(part: &str) -> &str {
        part.trim_matches('"')
    }

    #[test]
    fn declared_deps_match_live_rules() {
        for p in crate::registry::all() {
            for d in p.cli_text_deps() {
                if !d.parts.is_empty() {
                    assert!(
                        d.needle.is_empty() && d.max_gap > 0,
                        "{} 的 `{}`：分段近邻不再给整串 needle，且要给最大间距",
                        p.id(),
                        d.rule
                    );
                    for part in d.parts {
                        assert!(
                            d.sample
                                .to_lowercase()
                                .contains(&part_text(part).to_lowercase()),
                            "{} 的 `{}`：片段 {part:?} 不在样例里",
                            p.id(),
                            d.rule
                        );
                    }
                }
                if d.rule.starts_with("record:") {
                    // 分段近邻的记录格式：第一段才是解析器认的片段，其余是程序文件里的位置锚点。
                    let needle = d.parts.first().map_or(d.needle, |p| part_text(p)).to_lowercase();
                    assert!(
                        !needle.is_empty(),
                        "{} 的 `{}` 要给搜索片段",
                        p.id(),
                        d.rule
                    );
                    assert!(
                        d.sample.to_lowercase().contains(&needle),
                        "{} 的 `{}`：片段不在样例里",
                        p.id(),
                        d.rule
                    );
                    if d.version_line {
                        assert!(
                            d.parts.is_empty() && d.sample.to_lowercase().starts_with(&needle),
                            "{} 的 `{}`：版本行样例要以声明的前缀开头，且不再给分段近邻",
                            p.id(),
                            d.rule
                        );
                    }
                    assert!(
                        plugin_source_lower(p.id().as_str()).contains(&needle),
                        "{} 的 `{}`：插件源码里已不用 {:?}，解析器改了清单也要改",
                        p.id(),
                        d.rule,
                        d.needle
                    );
                    continue;
                }
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
                if d.needle.is_empty() && d.parts.is_empty() {
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
        if rule.starts_with("record:") {
            // 指向解析器里认这个片段的那一行。
            let dep = p.cli_text_deps().iter().find(|d| d.rule == rule)?;
            return Some(dep.parts.first().map_or(dep.needle, |p| part_text(p)).to_string());
        }
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
                    let mut row = serde_json::json!({
                        "rule": e.rule, "needle": e.needle, "legacy": e.legacy,
                        "note": e.note, "source": at,
                    });
                    if !e.parts.is_empty() {
                        row["parts"] = serde_json::json!(e.parts);
                        row["max_gap"] = serde_json::json!(e.max_gap);
                    }
                    if !e.version_prefix.is_empty() {
                        row["version_prefix"] = serde_json::json!(e.version_prefix);
                    }
                    row
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
