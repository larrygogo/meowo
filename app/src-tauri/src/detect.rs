//! 托管 PTY 的屏幕状态检测**引擎**：跨平台纯逻辑，不依赖任何 VT 实现，也**不认识任何
//! 具体 agent**——规则由各 agent 插件声明（`meowo_agent::screen`），这里只负责求值。
//!
//! 输入是**终端仿真后的干净末屏文本**（vt100 grid 重建，无 ANSI 转义），不是原始字节流——
//! 对字节流做字符串匹配会被全屏重绘/光标定位序列打穿，这是本模块存在的前提（接线见
//! `pty.rs` 的 ScreenProbe）。规则语义移植自 herdr 的清单（Apache-2.0，证据驱动维护），
//! 保持同一套区域锚定与从严纪律：
//!
//! - **blocked 从严**：只有命中已知的可见审批 UI 才判 blocked；什么都没命中回退
//!   idle（`fallback_idle`），失准的表现是「少报 blocked」而不是误报——安全侧失败。
//! - **防抖**（[`ScreenDebounce`]）：working → 无可见证据的 idle 要连续确认或超时才发布，
//!   吸收 spinner 帧闪烁与清屏瞬间；带可见证据（提示框 `❯`）的 idle 直接放行。
//! - **hold 规则**：transcript 查看器 / 模型选单这类覆盖层不代表状态变化，命中时保持
//!   已发布状态不动。

use meowo_agent::screen::{Matcher, Region, ScreenRule};
use std::time::{Duration, Instant};

pub(crate) use meowo_agent::screen::ScreenState;

/// 终端仿真后的末屏快照。`lines` 自上而下、已去尾部空白与尾部空行；`title` 来自 OSC 0/2。
pub(crate) struct ScreenSnapshot {
    pub(crate) lines: Vec<String>,
    pub(crate) title: Option<String>,
    /// OSC 9 的 payload（进度序列 `4;<state>;<pct>`）。与标题并列的第二个非屏幕信号源。
    pub(crate) progress: Option<String>,
}

impl ScreenSnapshot {
    pub(crate) fn new(mut lines: Vec<String>, title: Option<String>) -> Self {
        for line in &mut lines {
            let trimmed = line.trim_end();
            if trimmed.len() != line.len() {
                line.truncate(trimmed.len());
            }
        }
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        Self {
            lines,
            title,
            progress: None,
        }
    }

    pub(crate) fn with_progress(mut self, progress: Option<String>) -> Self {
        self.progress = progress;
        self
    }
}

/// 一次求值的结论。
pub(crate) enum Evaluation {
    /// 常规判定（含 fallback_idle）。
    Publish(Detection),
    /// 命中覆盖层类规则：屏幕内容不代表状态变化，保持已发布状态。
    Hold { rule_id: &'static str },
}

pub(crate) struct Detection {
    pub(crate) state: ScreenState,
    /// 有可见证据（审批 UI / 提示框 / spinner 标题）：防抖直接放行，不需要确认期。
    pub(crate) visible: bool,
    pub(crate) rule_id: &'static str,
}

impl Evaluation {
    pub(crate) fn rule_id(&self) -> &'static str {
        match self {
            Evaluation::Publish(det) => det.rule_id,
            Evaluation::Hold { rule_id } => rule_id,
        }
    }
}

/// 无规则命中时的回退标签（对齐 herdr 的 default_known_agent_idle_fallback 语义）。
pub(crate) const FALLBACK_RULE_ID: &str = "fallback_idle";

/// 取该 provider 的规则集。经 `meowo_agent::resolve` 解析身份——别名（"claude-code"
/// 等）与未知 provider 的处理都归它，本模块不维护第二份身份表。
fn rules_for(provider: &str) -> &'static [ScreenRule] {
    meowo_agent::resolve(Some(provider)).map_or(&[], |plugin| plugin.screen_rules())
}

/// 该 provider 是否有规则集。ScreenProbe 用它决定要不要建终端仿真器——无规则的会话
/// 连解析都不做（门必须设在生产端，否则 reader 每 chunk 白跑一遍 VT 状态机）。
pub(crate) fn provider_supported(provider: &str) -> bool {
    !rules_for(provider).is_empty()
}

/// 按 provider 求值。None = 该 provider 没有规则集（不做屏幕检测）。
///
/// 规则按 priority 降序求值，首个命中者胜；平局时先声明者胜（与 herdr 一致）。
/// 一条都没命中回退 idle——blocked 从严，见模块文档。
pub(crate) fn evaluate(provider: &str, snap: &ScreenSnapshot) -> Option<Evaluation> {
    let rules = rules_for(provider);
    if rules.is_empty() {
        return None;
    }
    // 匹配统一在小写文本上做（声明侧写小写字面量）；标题另走原文，它判的是字符集。
    let lower: Vec<String> = snap.lines.iter().map(|line| line.to_lowercase()).collect();
    let title = snap.title.as_deref().unwrap_or("");
    let progress = snap.progress.as_deref().unwrap_or("");
    // Title/Progress 的小写化与正文的 `lower` 一样提到规则循环外：标题类规则有多条时，
    // 此前同一标题每条规则都重算一遍 to_lowercase（检测每 300ms 一轮）。
    let lower_title = [title.to_lowercase()];
    let lower_progress = [progress.to_lowercase()];

    let mut best: Option<&ScreenRule> = None;
    for rule in rules {
        // 平局先声明者胜：只有严格更高的优先级才顶掉已命中的。
        if best.is_some_and(|winner| winner.priority >= rule.priority) {
            continue;
        }
        if matches_rule(rule, &lower, title, &lower_title, &lower_progress) {
            best = Some(rule);
        }
    }
    Some(match best {
        Some(rule) if rule.hold => Evaluation::Hold { rule_id: rule.id },
        Some(rule) => Evaluation::Publish(Detection {
            state: rule.state,
            visible: rule.visible,
            rule_id: rule.id,
        }),
        None => Evaluation::Publish(Detection {
            state: ScreenState::Idle,
            visible: false,
            rule_id: FALLBACK_RULE_ID,
        }),
    })
}

fn matches_rule(
    rule: &ScreenRule,
    lower: &[String],
    title: &str,
    lower_title: &[String],
    lower_progress: &[String],
) -> bool {
    match rule.region {
        // 进度序列同样当单行区域喂进去（小写：`4;0` 这类 payload 本无大小写，转了无害）。
        Region::Progress => eval_matcher(&rule.matcher, lower_progress, title),
        // 标题当作单行区域喂进去，同样**小写**——文本类谓词（Contains 等）全按小写约定
        // 比较（codex 的标题是 "Action Required"，不转就匹配不上）。字符集类谓词
        // （StartsWithCharIn / WordCharIn）另走原文 title 参数，它们判的是 spinner 帧
        // 这类无大小写的符号，不受影响。
        Region::Title => eval_matcher(&rule.matcher, lower_title, title),
        _ => {
            let region = region_of(rule.region, lower);
            eval_matcher(&rule.matcher, region, title)
        }
    }
}

fn eval_matcher(matcher: &Matcher, region: &[String], title: &str) -> bool {
    match matcher {
        Matcher::Contains(needles) => needles.iter().all(|n| region_contains(region, n)),
        Matcher::AnyContains(needles) => needles.iter().any(|n| region_contains(region, n)),
        Matcher::LineStartsWith(prefixes) => region.iter().any(|line| {
            let rest = line.trim_start();
            prefixes.iter().any(|p| rest.starts_with(p))
        }),
        Matcher::LineStartsWithContaining(prefix, needles) => region.iter().any(|line| {
            let rest = line.trim_start();
            rest.starts_with(prefix) && needles.iter().any(|n| rest.contains(n))
        }),
        Matcher::LineRegex(pattern) => {
            compiled_regex(pattern).is_some_and(|re| region.iter().any(|line| re.is_match(line)))
        }
        Matcher::CharRunAtLeast(set, min) => region.iter().any(|line| {
            let mut run = 0usize;
            line.chars().any(|ch| {
                run = if set.contains(ch) { run + 1 } else { 0 };
                run >= *min
            })
        }),
        Matcher::NumberedOption(number, word) => region
            .iter()
            .any(|line| numbered_option(line, *number, word)),
        Matcher::BareYes => region.iter().any(|line| bare_yes_option(line)),
        Matcher::SoleCharLineIn(set) => region.iter().any(|line| {
            let trimmed = line.trim();
            let mut chars = trimmed.chars();
            matches!((chars.next(), chars.next()), (Some(c), None) if set.contains(c))
        }),
        Matcher::BraillePrefixed(keywords) => region.iter().any(|line| {
            let rest = line.trim_start();
            let stripped = rest.trim_start_matches(|ch: char| ('\u{2800}'..='\u{28FF}').contains(&ch));
            stripped.len() != rest.len()
                && keywords
                    .iter()
                    .any(|keyword| stripped.trim_start().starts_with(keyword))
        }),
        Matcher::StartsWithCharIn(set) => {
            let mut chars = title.chars();
            matches!((chars.next(), chars.next()), (Some(c), Some(' ')) if set.contains(c))
        }
        Matcher::StartsWithCharInRange(lo, hi) => {
            let mut chars = title.chars();
            matches!((chars.next(), chars.next()), (Some(c), Some(' ')) if (*lo..=*hi).contains(&c))
        }
        Matcher::WordCharIn(set) => word_bounded_char(title, set),
        Matcher::NonEmpty => region.iter().any(|line| !line.trim().is_empty()),
        Matcher::All(inner) => inner.iter().all(|m| eval_matcher(m, region, title)),
        Matcher::Any(inner) => inner.iter().any(|m| eval_matcher(m, region, title)),
        Matcher::Not(inner) => !inner.iter().any(|m| eval_matcher(m, region, title)),
    }
}

// ---------------------------------------------------------------------------
// 区域锚定（语义对齐 herdr manifest.rs 的同名函数）
// ---------------------------------------------------------------------------

fn region_of(region: Region, lines: &[String]) -> &[String] {
    match region {
        // Title / Progress 不走这里（matches_rule 已单独处理），列出只为穷尽匹配。
        Region::WholeScreen | Region::Title | Region::Progress => lines,
        Region::BottomNonEmpty(n) => bottom_non_empty_slice(lines, n),
        Region::AfterLastRule => after_last_horizontal_rule(lines),
        Region::PromptBoxBody => prompt_box_body(lines).unwrap_or(&[]),
        Region::AfterLastPromptMarker => after_last_prompt_marker(lines),
    }
}

/// 该行是否是水平分隔线：trim 后以 `─` 开头，且「─ 连跑之后为空」或「连跑 ≥3」
/// （后者容忍 `─── 标题` 式分节线）。
fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    let run = trimmed.chars().take_while(|&ch| ch == '\u{2500}').count();
    if run == 0 {
        return false;
    }
    let suffix_start = trimmed
        .char_indices()
        .nth(run)
        .map_or(trimmed.len(), |(index, _)| index);
    trimmed[suffix_start..].trim_start().is_empty() || run >= 3
}

/// 最后一条水平分隔线之后的区域；没有分隔线时是整个屏幕。
fn after_last_horizontal_rule(lines: &[String]) -> &[String] {
    let start = lines
        .iter()
        .rposition(|line| is_horizontal_rule(line))
        .map_or(0, |index| index + 1);
    &lines[start..]
}

/// codex 的提示符标记行（`›` 独占或 `› ` 开头）。
fn codex_prompt_line(line: &str) -> bool {
    line == "\u{203A}" || line.starts_with("\u{203A} ")
}

/// 最后一个提示符标记行之后的区域；没有标记时是整个屏幕。
fn after_last_prompt_marker(lines: &[String]) -> &[String] {
    let start = lines
        .iter()
        .rposition(|line| codex_prompt_line(line))
        .map_or(0, |index| index + 1);
    &lines[start..]
}

/// 提示框体：自底向上第 2 条水平分隔线（框顶）之后、到下一条分隔线之前的行。
/// 找不到两条分隔线 = 屏幕上没有提示框。
fn prompt_box_body(lines: &[String]) -> Option<&[String]> {
    let mut rules = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| is_horizontal_rule(line))
        .map(|(index, _)| index);
    let _bottom = rules.next()?;
    let top = rules.next()?;
    let end = lines[top + 1..]
        .iter()
        .position(|line| is_horizontal_rule(line))
        .map_or(lines.len(), |relative| top + 1 + relative);
    Some(&lines[top + 1..end])
}

/// 底部 N 个非空行。返回**切片**而非拷贝：从最后一个非空行往回数 N 个非空行，
/// 取其间的连续区间（中间夹的空行无妨，谓词都跳空白）。
fn bottom_non_empty_slice(lines: &[String], n: usize) -> &[String] {
    let mut seen = 0;
    let mut start = lines.len();
    for (index, line) in lines.iter().enumerate().rev() {
        if !line.trim().is_empty() {
            seen += 1;
            start = index;
            if seen == n {
                break;
            }
        }
    }
    &lines[start..]
}

fn region_contains(region: &[String], needle: &str) -> bool {
    region.iter().any(|line| line.contains(needle))
}

/// 取（并缓存）一条规则正则的编译结果。
///
/// 规则来自插件的 `&'static` 声明，条数固定且很少，故编译一次长期复用——检测每 300ms
/// 跑一轮，每轮重编译几十条正则是纯浪费。缓存键就是模式串本身（同一 `&'static str`）。
///
/// 编译失败返回 None（该条规则不命中），**绝不 panic**：规则是数据，写坏一条不该让
/// 整个状态检测停摆；下面有测试保证内置规则全部可编译。
fn compiled_regex(pattern: &'static str) -> Option<regex::Regex> {
    use std::sync::{Mutex, OnceLock};
    // 键收 `&'static str`（规则声明本就是静态的）：此前键是 String，缓存**命中**也要为
    // entry() 分配一次——这条路径每 300ms × 每条正则规则跑一遍，还与主线程的
    // screen_detect_explain 争同一把锁，持锁时间应压到最短。
    static CACHE: OnceLock<Mutex<std::collections::HashMap<&'static str, Option<regex::Regex>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut cache = cache.lock().ok()?;
    cache
        .entry(pattern)
        .or_insert_with(|| {
            // 统一大小写不敏感：规则声明侧写小写字面量，与其它谓词的约定一致。
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .ok()
        })
        .clone()
}

/// `^\s*❯?\s*yes\b`（行已小写）。
fn bare_yes_option(line: &str) -> bool {
    let rest = line.trim_start();
    let rest = rest.strip_prefix('\u{276F}').map_or(rest, str::trim_start);
    starts_with_word(rest, "yes")
}

/// `^\s*❯?\s*N\.\s*<word>\b`（行已小写）。
fn numbered_option(line: &str, number: char, word: &str) -> bool {
    let rest = line.trim_start();
    let rest = rest.strip_prefix('\u{276F}').map_or(rest, str::trim_start);
    let Some(rest) = rest.strip_prefix(number) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('.') else {
        return false;
    };
    starts_with_word(rest.trim_start(), word)
}

fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word)
        .is_some_and(|tail| !tail.starts_with(char::is_alphanumeric))
}

/// 文本里是否有 `set` 中的某个字符**独立成词**出现（两侧是空格或文本边界）。
fn word_bounded_char(text: &str, set: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars.iter().enumerate().any(|(index, ch)| {
        set.contains(*ch)
            && index.checked_sub(1).is_none_or(|prev| chars[prev] == ' ')
            && chars.get(index + 1).is_none_or(|next| *next == ' ')
    })
}

// ---------------------------------------------------------------------------
// 防抖
// ---------------------------------------------------------------------------

/// working → 「无可见证据的 idle」的确认次数与时间上限（对齐 herdr：3 次或 700ms）。
/// spinner 帧切换/清屏瞬间会闪出一帧什么都匹配不上的屏幕，直接发布会造成状态抖动。
const PENDING_IDLE_CONFIRMS: u8 = 3;
const PENDING_IDLE_CAP: Duration = Duration::from_millis(700);

/// 每会话一份的发布门。[`Self::observe`] 返回 Some = 对外发布的状态发生变化。
#[derive(Default)]
pub(crate) struct ScreenDebounce {
    published: Option<ScreenState>,
    /// (首次观察到待确认 idle 的时刻, 已确认次数)。
    pending_idle: Option<(Instant, u8)>,
    /// 当前发布值是否来自无规则命中的回退（[`FALLBACK_RULE_ID`]）。回退 idle 是
    /// 「什么都没认出来」而非「确认空闲」，前端据此把角标降级为中性点（T-15）。
    fallback: bool,
}

impl ScreenDebounce {
    pub(crate) fn published(&self) -> Option<ScreenState> {
        self.published
    }

    /// 当前发布值是否来自回退规则（仅 published() 为 Some 时有意义）。
    pub(crate) fn published_is_fallback(&self) -> bool {
        self.fallback
    }

    /// 是否有待确认的降级——ticker 据此在内容序号未变时也继续扫描（确认的正是「屏幕
    /// 稳定不变」这件事，跳扫会让确认永远等不齐）。
    pub(crate) fn pending(&self) -> bool {
        self.pending_idle.is_some()
    }

    pub(crate) fn observe(&mut self, eval: &Evaluation, now: Instant) -> Option<ScreenState> {
        let det = match eval {
            Evaluation::Hold { .. } => return None,
            Evaluation::Publish(det) => det,
        };
        if self.published == Some(det.state) {
            self.pending_idle = None;
            // 同值再发布（如回退 idle → 带可见证据的 idle）不改变状态，但证据强度
            // 变了——角标的自信度跟着最新一次判定走。
            self.fallback = det.rule_id == FALLBACK_RULE_ID;
            return None;
        }
        if det.state == ScreenState::Idle
            && !det.visible
            && self.published == Some(ScreenState::Working)
        {
            let (since, confirms) = self.pending_idle.get_or_insert((now, 0));
            *confirms += 1;
            if *confirms < PENDING_IDLE_CONFIRMS && now.duration_since(*since) < PENDING_IDLE_CAP {
                return None;
            }
        }
        self.pending_idle = None;
        self.fallback = det.rule_id == FALLBACK_RULE_ID;
        self.published = Some(det.state);
        Some(det.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(lines: &[&str]) -> ScreenSnapshot {
        ScreenSnapshot::new(lines.iter().map(|s| s.to_string()).collect(), None)
    }

    fn snap_titled(lines: &[&str], title: &str) -> ScreenSnapshot {
        ScreenSnapshot::new(
            lines.iter().map(|s| s.to_string()).collect(),
            Some(title.to_string()),
        )
    }

    fn state_of(eval: Option<Evaluation>) -> Option<(ScreenState, &'static str)> {
        match eval? {
            Evaluation::Publish(det) => Some((det.state, det.rule_id)),
            Evaluation::Hold { rule_id } => Some((ScreenState::Idle, rule_id)),
        }
    }

    /// 「插件声明了规则，引擎立刻认得」——加 agent 只动 `plugins/` 的正向验证。
    ///
    /// 反向约束（引擎源码里不得出现 agent 身份）归 lib.rs 的
    /// `host_code_does_not_branch_on_agent_identity` 统一管：它的清单已含 detect.rs，
    /// 且按测试模块**区间**跳过而非首个标记截断。这里曾经自己写过一份 `split("mod tests")`
    /// 的截断版——正是那种写法在 terminal.rs 上漏掉了 69% 的文件，不再保留第二份。
    #[test]
    fn declared_rules_take_effect_without_touching_the_engine() {
        let declared: Vec<&str> = meowo_agent::all()
            .iter()
            .filter(|plugin| !plugin.screen_rules().is_empty())
            .map(|plugin| plugin.id().as_str())
            .collect();
        assert!(
            declared.len() >= 3,
            "至少 claude/codex/kimi 三家声明了规则，实得 {declared:?}"
        );
        for id in declared {
            assert!(provider_supported(id), "{id} 声明了规则却未被引擎认出");
        }
    }

    #[test]
    fn unknown_provider_has_no_rules() {
        assert!(evaluate("wezterm", &snap(&["anything"])).is_none());
    }

    /// auto 模式环境学习引导（2.1.314 实拍逐行复刻）：标题是空闲的 ✳，开关行以 ❯ 开头，
    /// 必须判 blocked——判成 idle 时对话页发出的消息会被这个对话框吞掉。
    #[test]
    fn auto_mode_setup_dialog_is_blocked() {
        let s = snap_titled(
            &[
                "\u{273B} Saut\u{e9}ed for 2s \u{00B7} done 22:37",
                "\u{2594}\u{2594}\u{2594}\u{2594}\u{2594}\u{2594}\u{2594}\u{2594}",
                "   Teach auto mode about your environment?",
                "   Claude Code reads this project, your recent Claude sessions, and optionally your shell history and other",
                "     How you use Claude here     Mixed",
                "   \u{276F} Also scan shell history     true",
                "     Also scan your other repos  false",
                "     Continue",
                "   \u{2190}/\u{2192} to change \u{00B7} Enter to continue \u{00B7} Esc to cancel",
            ],
            "\u{2733} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "auto_mode_setup_prompt"))
        );
    }

    /// /btw 覆盖层两种形态（2.1.314 实拍逐行复刻）：作答中 = working；答完 = blocked——
    /// 覆盖层吞按键（`c` 复制、`f` 分叉），旧规则把它一直判成运行中，对话页照常发消息就被吃掉。
    #[test]
    fn btw_overlay_answering_works_and_answered_blocks() {
        let rule = "\u{2594}".repeat(40);
        let answering = snap_titled(
            &[
                "\u{273B} Churned for 2s \u{00B7} done 22:47",
                rule.as_str(),
                "    /btw \u{5217}\u{51FA} 1 \u{5230} 60",
                "      \u{00B7} Answering\u{2026}",
                "    Esc to close",
            ],
            "\u{2733} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &answering)),
            Some((ScreenState::Working, "btw_overlay_working"))
        );
        let mut answered: Vec<String> = vec![rule.clone(), "    /btw \u{5217}\u{51FA} 1 \u{5230} 60".into()];
        answered.extend((1..=26).map(|n| format!("      {n}")));
        answered.push("    \u{2191}/\u{2193} to scroll \u{00B7} c to copy \u{00B7} f to fork \u{00B7} Esc to close".into());
        let answered = ScreenSnapshot::new(answered, Some("\u{2733} Claude Code".into()));
        assert_eq!(
            state_of(evaluate("claude", &answered)),
            Some((ScreenState::Blocked, "btw_overlay_answered"))
        );
    }

    /// 标题盲文 spinner 帧 → working（最高优先级，压过屏幕上的一切）。
    #[test]
    fn braille_title_wins_as_working() {
        let s = snap_titled(&["Do you want to proceed?", "\u{276F} 1. Yes"], "\u{280B} Herding bits…");
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "osc_title_working"))
        );
    }

    #[test]
    fn esc_title_means_idle() {
        let s = snap_titled(&["some scrollback"], "\u{2733} claude");
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Idle, "osc_title_idle"))
        );
    }

    /// CC 2.1.233 起标题 spinner 换成月相帧（◐◑，实拍取证）：运行中标题必须判
    /// working——否则 ❯ 提示框会把正在跑的会话判成可见 idle，黄环「等你」错挂整个
    /// 长任务（「状态不及时」实拍反馈的根因）。
    #[test]
    fn moon_title_wins_as_working() {
        let s = snap_titled(&["  some output", " \u{276F}"], "\u{25D0} Claude Code");
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "osc_title_moon_working"))
        );
    }

    /// 标题不可用时（用户自设终端标题、标题事件丢失），底部状态行
    /// `✢ Brewing… (7s · ↓ 129 tokens)` 是第二运行证据，且必须压过 ❯ 提示框——
    /// 运行中输入框照画。屏幕结构照 2.1.233 实拍逐行复刻。
    #[test]
    fn running_status_line_beats_prompt_box() {
        let s = snap(&[
            "  poem line above",
            "\u{2722} Brewing\u{2026} (7s \u{00B7} \u{2193} 129 tokens)",
            "────────────────────────",
            " \u{276F}",
            "────────────────────────",
            "  Fable 5 · 25% ctx",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "status_line_working"))
        );
    }

    /// 回合结束行 `✻ Baked for 11s` 没有括号计时，不是运行证据——❯ 提示框正常
    /// 判回可见 idle，状态跟着回合结束立即翻绿转黄，不再等下一个 hook 事件。
    #[test]
    fn finished_status_line_returns_to_idle() {
        let s = snap(&[
            "\u{273B} Baked for 11s",
            "────────────────────────",
            " \u{276F} ",
            "────────────────────────",
            "  Fable 5 · 25% ctx",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Idle, "live_prompt_box"))
        );
    }

    /// 分隔线后的选择型审批表单 → blocked（可见证据）。
    #[test]
    fn selection_form_after_rule_is_blocked() {
        let s = snap(&[
            "  tool output above",
            "────────────────────────",
            " Do you want to make this edit?",
            " \u{276F} 1. Yes",
            "   2. No",
            " Enter to select · Esc to cancel · ↑/↓ to navigate",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "live_blocked_form"))
        );
    }

    #[test]
    fn bash_permission_prompt_is_blocked() {
        let s = snap(&[
            " Bash command",
            "   rm -rf ./build",
            " Do you want to proceed?",
            " \u{276F} 1. Yes",
            "   2. No, and tell Claude what to do differently",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "bash_permission_prompt"))
        );
    }

    #[test]
    fn powershell_permission_prompt_is_blocked() {
        let s = snap(&[
            " PowerShell command",
            "   Get-Process -Id 1",
            " Do you want to proceed?",
            " \u{276F} 1. Yes",
            "   2. No",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "bash_permission_prompt"))
        );
    }

    #[test]
    fn generic_permission_prompt_is_blocked() {
        let s = snap(&[
            "────────────────────────",
            " Do you want to proceed?",
            " 1. Yes",
            " 2. Yes, allow all edits during this session",
            " 3. No",
            " Esc to cancel",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "generic_permission_prompt"))
        );
    }

    /// 计划模式批准提示(2.1.227 plan-file 流程,实拍取证)。它不触发任何 hook,
    /// 没有 pending_review 兜底,这条可见 blocked 规则是「待交互」的唯一来源。
    #[test]
    fn plan_approval_prompt_is_blocked() {
        let s = snap(&[
            " Claude has written up a plan and is ready to execute. Would you like to proceed?",
            " \u{276F} 1. Yes, and bypass permissions",
            "   2. Yes, manually approve edits",
            "   3. Tell Claude what to change",
            " ctrl+g to edit in Notepad · ~\\.claude\\plans\\compiled-wiggling-papert.md",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Blocked, "plan_approval_prompt"))
        );
    }

    /// 提示框（倒数第二条分隔线之后）里 ❯ 开头且无审批文案 → 可见 idle。
    #[test]
    fn prompt_box_is_visible_idle() {
        let s = snap(&[
            "  previous output",
            "────────────────────────",
            " \u{276F} try \"fix the failing test\"",
            "────────────────────────",
            "  ? for shortcuts",
        ]);
        let Some(Evaluation::Publish(det)) = evaluate("claude", &s) else {
            panic!("应有判定");
        };
        assert_eq!((det.state, det.rule_id), (ScreenState::Idle, "live_prompt_box"));
        assert!(det.visible, "提示框是可见 idle 证据");
    }

    /// 提示框里出现审批导航文案时不能判**可见** idle（选择表单的 ❯ 是选项指针不是提示符）：
    /// not 门否决 live_prompt_box，只能落到无可见证据的 fallback——防抖会扣住它，不会
    /// 在审批表单还挂着时把卡片翻成空闲。
    #[test]
    fn prompt_box_with_approval_text_is_not_visible_idle() {
        let s = snap(&[
            "────────────────────────",
            " \u{276F} 1. Yes",
            "   2. No",
            " Enter to select · Esc to cancel · arrow keys to navigate",
            "────────────────────────",
            "  hint line",
        ]);
        let Some(Evaluation::Publish(det)) = evaluate("claude", &s) else {
            panic!("应有判定");
        };
        assert_ne!(det.rule_id, "live_prompt_box");
        assert!(!det.visible, "审批表单在屏时绝不能给出可见 idle 证据");
    }

    /// transcript 覆盖层 → Hold，不改状态。
    #[test]
    fn transcript_viewer_holds_state() {
        let s = snap(&[
            "  lots of transcript lines",
            " Showing detailed transcript",
            "  ctrl+o to toggle",
        ]);
        assert!(matches!(
            evaluate("claude", &s),
            Some(Evaluation::Hold { rule_id: "transcript_viewer" })
        ));
    }

    /// 什么都没命中 → 回退 idle（blocked 从严：宁可少报）。
    #[test]
    fn no_match_falls_back_to_idle() {
        let s = snap(&["  compiling foo v0.1.0", "  warning: unused variable"]);
        let Some(Evaluation::Publish(det)) = evaluate("claude", &s) else {
            panic!("应有判定");
        };
        assert_eq!((det.state, det.rule_id), (ScreenState::Idle, FALLBACK_RULE_ID));
        assert!(!det.visible);
    }

    /// 空提示符行否决弱 blocked（legacy 规则的 not 门）。
    #[test]
    fn bare_prompt_line_vetoes_legacy_blocker() {
        let s = snap(&["  do you want to refactor? yes we discussed it", " \u{276F}"]);
        let got = state_of(evaluate("claude", &s));
        assert_ne!(got.map(|(state, _)| state), Some(ScreenState::Blocked));
    }

    // -- codex --

    #[test]
    fn codex_action_required_title_is_blocked() {
        let s = snap_titled(&["some output"], "Action Required · codex");
        assert_eq!(
            state_of(evaluate("codex", &s)),
            Some((ScreenState::Blocked, "osc_title_blocked"))
        );
    }

    #[test]
    fn codex_spinner_title_is_working() {
        let s = snap_titled(&["output"], "codex \u{280B} building");
        assert_eq!(
            state_of(evaluate("codex", &s)),
            Some((ScreenState::Working, "osc_title_working"))
        );
    }

    /// 强 blocker 只认提示符标记之后的区域：标记之前的历史文本不算数。
    #[test]
    fn codex_strong_blocker_after_prompt_marker() {
        let s = snap(&[
            "  old scroll: allow command? was answered long ago",
            "\u{203A} previous prompt",
            " Allow command?",
            " press enter to confirm or esc to cancel",
        ]);
        assert_eq!(
            state_of(evaluate("codex", &s)),
            Some((ScreenState::Blocked, "live_strong_blocker"))
        );
    }

    /// codex 0.166.0 审批框：整句 `press enter to confirm or esc to cancel` / `allow command?`
    /// 在二进制里已经没有，底栏由按键 + ` to confirm` / ` to cancel` 拼出，选项固定带
    /// `No, and tell Codex what to do differently`（codex.exe 字符串，research-cli-drift X7，
    /// 2026-11-12；行布局按 ratatui 选择列表推断，未实拍）。光标停在哪一项都要认。
    #[test]
    fn codex_0166_approval_overlay_is_blocked() {
        let cursor_first = snap(&[
            "\u{203A} run the tests",
            "  Would you like to run the following command?",
            "  $ cargo test",
            "\u{203A} 1. Yes, just this once",
            "  2. Yes, and don't ask again for commands that start with `cargo test`",
            "  3. No, and tell Codex what to do differently",
            "  Press enter to confirm or esc to cancel",
        ]);
        assert_eq!(
            state_of(evaluate("codex", &cursor_first)),
            Some((ScreenState::Blocked, "live_strong_blocker"))
        );
        // 光标在最后一项：选项行本身是提示符标记行、不在区域内，靠底栏两段认
        let cursor_last = snap(&[
            "  Would you like to make the following edits?",
            "  1. Yes, proceed",
            "\u{203A} 2. No, and tell Codex what to do differently",
            "  enter to confirm \u{00B7} esc to cancel",
        ]);
        assert_eq!(
            state_of(evaluate("codex", &cursor_last)),
            Some((ScreenState::Blocked, "live_strong_blocker"))
        );
        // 已答过的审批留在历史里（最后一个 `›` 之前）不算
        let answered = snap(&[
            "  3. No, and tell Codex what to do differently",
            "  enter to confirm \u{00B7} esc to cancel",
            "\u{203A} next prompt",
        ]);
        assert_ne!(
            state_of(evaluate("codex", &answered)).map(|(state, _)| state),
            Some(ScreenState::Blocked)
        );
    }

    /// codex 0.166.0 的 transcript 覆盖层（Ctrl+T）：`q to quit` / `esc to edit prev` 已不在
    /// 二进制里，pager_overlay 只剩 ` to scroll` / ` to page` / ` to jump`（research-cli-drift X6）。
    /// 老版本的整套提示仍认。
    #[test]
    fn codex_transcript_pager_holds_state_old_and_new() {
        let new = snap(&[
            "  user: run the tests",
            "  \u{2191}/\u{2193} to scroll   pgup/pgdn to page   home/end to jump",
        ]);
        assert!(matches!(
            evaluate("codex", &new),
            Some(Evaluation::Hold { rule_id: "transcript_viewer" })
        ));
        let old = snap(&[
            "  user: run the tests",
            "  \u{2191}/\u{2193} to scroll   pgup/pgdn to page   home/end to jump   q to quit   esc to edit prev",
        ]);
        assert!(matches!(
            evaluate("codex", &old),
            Some(Evaluation::Hold { rule_id: "transcript_viewer" })
        ));
        // 做空第二轮 N6（2026-11-12）：0.166.0 codex.exe 里另有带空格的 `pgup / pgdn` 常量（机器码立即数
        // `pgup / p` + `pgdn` 拼出，看不出属于哪个界面）。键间换成 ` / ` 照样认，三组按键都覆盖。
        let spaced = snap(&[
            "  user: run the tests",
            "  \u{2191} / \u{2193} to scroll   pgup / pgdn to page   home / end to jump",
        ]);
        assert!(matches!(
            evaluate("codex", &spaced),
            Some(Evaluation::Hold { rule_id: "transcript_viewer" })
        ));
    }

    /// 做空第一轮 #9（2026-11-12）：正文里同时出现 to scroll / to page / to jump（agent 在讲分页器、
    /// 用户贴的 README），状态行还在 Working。修前三段子串同屏就判记录查看页并 hold，运行中被冻住。
    /// 底栏样本按 0.166.0 codex.exe 的按键显示表与 pager_overlay 三段说明拼出（非实拍）。
    #[test]
    fn codex_prose_mentioning_pager_hints_is_not_transcript_viewer() {
        let prose = snap(&[
            "  The pager supports arrow keys to scroll, PgUp to page and Home to jump.",
            "  Scroll down to page two; swipe to jump; drag to scroll.",
            " \u{2022} Working (3m 12s \u{00B7} esc to interrupt)",
        ]);
        assert_eq!(
            state_of(evaluate("codex", &prose)),
            Some((ScreenState::Working, "screen_working_fallback"))
        );
        // 只有说明、没有按键字形的残片也不算。
        let fragments = snap(&["  scroll  to scroll   to page   to jump"]);
        assert!(!matches!(
            evaluate("codex", &fragments),
            Some(Evaluation::Hold { .. })
        ));
        // 窄窗把底栏折成三行，各段仍带按键：照样认。
        let wrapped = snap(&[
            "  user: run the tests",
            "  \u{2191}/\u{2193} to scroll",
            "  pgup/pgdn to page",
            "  home/end to jump",
        ]);
        assert!(matches!(
            evaluate("codex", &wrapped),
            Some(Evaluation::Hold { rule_id: "transcript_viewer" })
        ));
    }

    #[test]
    fn codex_working_status_line_is_working() {
        let s = snap(&[
            "  compiling...",
            " \u{2022} Working (3m 12s · esc to interrupt) · 42 tokens",
        ]);
        assert_eq!(
            state_of(evaluate("codex", &s)),
            Some((ScreenState::Working, "screen_working_fallback"))
        );
    }

    /// 有标题但既不转也不告警 → idle；无标题无命中 → fallback。
    #[test]
    fn codex_plain_title_is_idle() {
        let s = snap_titled(&["  quiet"], "codex — ~/repo");
        assert_eq!(
            state_of(evaluate("codex", &s)),
            Some((ScreenState::Idle, "osc_title_idle"))
        );
        let bare = snap(&["  quiet"]);
        assert_eq!(
            state_of(evaluate("codex", &bare)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    /// codex 的状态行在不同版本/终端宽度下，`•` 与 `Working` 之间可能不止一个空格
    /// （herdr 原始规则用 `\s+`）。写死单空格会让这些屏幕**漏判成 idle**——而漏判
    /// working 的后果是卡片显示空闲、通知也不发，用户以为它停了。
    #[test]
    fn codex_working_line_tolerates_extra_spaces() {
        for line in [
            " \u{2022} Working (3m 12s \u{00B7} esc to interrupt)",
            " \u{2022}  Working (3m 12s \u{00B7} esc to interrupt)",
            " \u{2022}   Working (12s \u{00B7} esc to interrupt) \u{00B7} 42 tokens",
        ] {
            let s = snap(&["  compiling", line]);
            assert_eq!(
                state_of(evaluate("codex", &s)),
                Some((ScreenState::Working, "screen_working_fallback")),
                "应识别为运行中: {line:?}"
            );
        }
    }

    // -- kimi --

    #[test]
    fn kimi_approval_panel_is_blocked() {
        let s = snap(&[
            " Requesting approval",
            " Run this command?",
            "   cargo test --all",
            " \u{25B6} Approve once   Reject   Revise",
            " 1/2/3 choose · \u{21B5} confirm · esc cancel",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Blocked, "current_approval_panel"))
        );
    }

    #[test]
    fn kimi_question_panel_is_blocked() {
        let s = snap(&[
            " Question",
            " ? 晚饭吃什么",
            "   火锅",
            "   寿司",
            " ↑↓ select · \u{21B5} choose · esc cancel",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Blocked, "question_panel"))
        );
    }

    #[test]
    fn kimi_spinners_are_working() {
        let moon = snap(&["  output", " 🌕 "]);
        assert_eq!(
            state_of(evaluate("kimi", &moon)),
            Some((ScreenState::Working, "moon_spinner_working"))
        );
        let braille = snap(&[" \u{280B} Thinking... 3s"]);
        assert_eq!(
            state_of(evaluate("kimi", &braille)),
            Some((ScreenState::Working, "braille_spinner_working"))
        );
    }

    #[test]
    fn kimi_current_tui_status_lines_are_working() {
        // 当前版 TUI 的两种工作指示（2026-08 实拍回归：整块活跃工作屏被判 idle →
        // 看板误报「待交互」）：盲文 spinner + `<Agent> Running (…)`，月相提示行。
        let agent = snap(&[
            "  output",
            " \u{2834} Explore Agent Running (\u{2026}) \u{00B7} 72 tools \u{00B7} 30m 38s",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &agent)),
            Some((ScreenState::Working, "agent_running_status_working"))
        );
        let tip = snap(&["  output", " \u{1F318} \u{00B7} Tip: ! to run a shell command"]);
        assert_eq!(
            state_of(evaluate("kimi", &tip)),
            Some((ScreenState::Working, "moon_tip_status_working"))
        );
    }

    /// 月相**紧贴**状态词（实拍 `🌗Working...`，swarm 模式常驻）：两条旧月相规则一条
    /// 要求月相独占整行、一条要求「月相 空格 · 空格」，这种形态两头不沾，整块正在干活
    /// 的屏因此落进 fallback idle——看板上顶着「认不出」的中性灰点，tab 还被归进
    /// 「待交互」（2026-08-31 实拍回归）。
    #[test]
    fn kimi_moon_glued_status_line_is_working() {
        let s = snap(&["  output", "\u{1F317}Working...  \u{2500}\u{2500}\u{2500}\u{2500}"]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Working, "moon_word_status_working"))
        );
    }

    /// 反向钉子：新规则靠**状态词白名单**从严。月相是普通 emoji，正文里出现一个不能
    /// 把「等你输入」谎报成在跑——放宽成「月相开头的任意行」就会踩这个坑。
    #[test]
    fn kimi_moon_in_prose_is_not_working() {
        let s = snap(&["  \u{1F317} 月相只是正文里提了一句", "  nothing pending"]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    #[test]
    fn kimi_falls_back_to_idle() {
        let s = snap(&["  plain output", "  nothing pending"]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    /// 空闲输入框是**正向** idle 证据（0.29 源码取证：custom-editor.ts 的
    /// injectPromptSymbol + wrapWithSideBorders，行呈 `│ > `，bash 模式 `│ ! `）。
    /// 此前 kimi 没有 idle 规则，空闲屏恒落 fallback——卡片挂「认不出来」灰点。
    #[test]
    fn kimi_idle_composer_prompt_is_confident_idle() {
        let s = snap(&[
            "  earlier output",
            "\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256E}",
            "\u{2502} >      \u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256F}",
            "kimi-for-coding  ~/repo",
            "        context: 12% (24k/200k)",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Idle, "composer_prompt_idle"))
        );
        // bash 模式的 `!` 提示符同样认。
        let bash = snap(&["\u{2502} !      \u{2502}", "context: 1%"]);
        assert_eq!(
            state_of(evaluate("kimi", &bash)),
            Some((ScreenState::Idle, "composer_prompt_idle"))
        );
    }

    /// 优先级钉子：工作状态行与输入框同屏（工作中输入框照画）时，working 规则
    /// 必须压过 composer_prompt_idle——否则转着的会话会被谎报成等你输入。
    #[test]
    fn kimi_working_status_outranks_composer_prompt() {
        let s = snap(&[
            "  output",
            " \u{1F318} \u{00B7} Tip: ! to run a shell command",
            "\u{2502} >      \u{2502}",
            "context: 12%",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Working, "moon_tip_status_working"))
        );
    }

    /// 反向钉子：转录里用户消息是 bullet 开头的 `> ...`（user-message.ts），没有
    /// `│` 边框——不能认成空闲输入框，只能落 fallback。
    #[test]
    fn kimi_bare_gt_transcript_line_is_not_composer() {
        let s = snap(&["> 刚才发的那条消息", "  nothing pending"]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    /// kimi 2.5.1 的盲文 spinner 状态词改成 Unicode 省略号（包内 MoonLoader.updateDisplay
    /// 拼 `${frame} ${label}` + ` · Tip: …`，label 是 `"Working…"` / `"Compacting…"`，
    /// 2026-11-12 取证）。旧规则只认 `working...`，这行与输入框同屏时整屏判成空闲。
    /// 旧版三个点的写法（上面 `kimi_spinners_are_working`）仍认。
    #[test]
    fn kimi_251_ellipsis_spinner_outranks_composer_prompt() {
        for line in [
            " \u{280B} Working\u{2026} \u{00B7} Tip: ! to run a shell command",
            " \u{2834} Working\u{2026}",
            " \u{2819} Compacting\u{2026}",
        ] {
            let s = snap(&["  output", line, "\u{2502} >      \u{2502}", "context: 12%"]);
            assert_eq!(
                state_of(evaluate("kimi", &s)),
                Some((ScreenState::Working, "braille_spinner_working")),
                "{line}"
            );
        }
        let old = snap(&[" \u{280B} Working... 3s", "\u{2502} >      \u{2502}"]);
        assert_eq!(
            state_of(evaluate("kimi", &old)),
            Some((ScreenState::Working, "braille_spinner_working"))
        );
    }

    /// kimi 2.5.1 提问表单的确认页（包内 question-dialog.ts renderSubmitTab 原文拼出，2026-11-12）：
    /// 没有 `? 问题` 行，修复前判成空闲。
    #[test]
    fn kimi_251_question_submit_page_is_blocked() {
        let s = snap(&[
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            " question",
            "",
            " (\u{2713}) Database  (\u{2713}) Features   Submit ",
            "",
            " Review your answer before submit",
            "",
            "  Q  Which database should we use?",
            "  \u{2192}  SQLite",
            "",
            " Ready to submit your answers?",
            "",
            "  \u{2192} [1] Submit",
            "    [2] Cancel",
            "",
            "  \u{2191}\u{2193} select  1/2 choose  \u{21B5} confirm  \u{2190}/\u{2192}/tab switch  esc cancel",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Blocked, "question_submit_panel"))
        );
    }

    /// kimi 2.5.1 重试时月相 spinner 带 `Retrying (n/m) · <错误名>` 标签（包内
    /// formatStepRetryLabel，2026-11-12 取证）。没有 `(n/m)` 计数的 retrying 是正文，不认。
    #[test]
    fn kimi_251_retry_spinner_is_working() {
        let s = snap(&[
            "  output",
            " \u{1F318} Retrying (2/5) \u{00B7} RateLimitError \u{00B7} in 4s",
            "\u{2502} >      \u{2502}",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Working, "moon_word_status_working"))
        );
        let prose = snap(&["\u{1F318} retrying later is fine", "\u{2502} >      \u{2502}"]);
        assert_eq!(
            state_of(evaluate("kimi", &prose)),
            Some((ScreenState::Idle, "composer_prompt_idle"))
        );
    }

    // -- gemini --

    #[test]
    fn gemini_approval_box_is_blocked() {
        let s = snap(&[
            "  │ Apply this change to src/main.rs?",
            "  │ \u{276F} Yes, apply",
            "  │   No",
        ]);
        assert_eq!(
            state_of(evaluate("gemini", &s)),
            Some((ScreenState::Blocked, "apply_or_allow_change"))
        );
    }

    /// 选项指针与 yes 必须在**同一行**：拆成两个谓词会把「某处有 ❯、别处有 yes」的
    /// 普通输出误判成审批（LineStartsWithContaining 存在的理由）。
    #[test]
    fn gemini_pointer_and_yes_must_share_a_line() {
        let same_line = snap(&["  \u{276F} Yes, allow it"]);
        assert_eq!(
            state_of(evaluate("gemini", &same_line)),
            Some((ScreenState::Blocked, "apply_or_allow_change"))
        );
        // 指针行与 yes 分处两行、且无任何审批文案 → 不该判阻塞。
        let split = snap(&["  \u{276F} run the tests", "  the answer is yes"]);
        assert_ne!(
            state_of(evaluate("gemini", &split)).map(|(state, _)| state),
            Some(ScreenState::Blocked)
        );
    }

    #[test]
    fn gemini_esc_to_cancel_is_working() {
        let s = snap(&["  generating response...", "  (esc to cancel)"]);
        assert_eq!(
            state_of(evaluate("gemini", &s)),
            Some((ScreenState::Working, "esc_cancel_working"))
        );
    }

    /// gemini 0.65 提问表单（ask_user）。画面按 npm bundle 源码拼出（2026-11-12，未实拍）：
    /// ToolConfirmationQueue 的绿框 StickyHeader 头行 `Answer Questions`，AskUserDialog 的
    /// BaseSelectionList（`● 1.  选项`）与 DialogFooter（`Enter to select · ↑/↓ to navigate ·
    /// Esc to cancel`）。修复前提示行的 `Esc to cancel` 让兜底规则判成运行中。
    #[test]
    fn gemini_065_ask_user_dialog_is_blocked() {
        let s = snap(&[
            "\u{280F} Waiting for user confirmation...",
            "\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256E}",
            "\u{2502} Answer Questions                                  \u{2502}",
            "\u{2502}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2502}",
            "\u{2502} Which database should we use?                     \u{2502}",
            "\u{2502}                                                   \u{2502}",
            "\u{2502} \u{25CF} 1.  PostgreSQL                                  \u{2502}",
            "\u{2502}   2.  SQLite                                      \u{2502}",
            "\u{2502}   3.  Enter a custom value                        \u{2502}",
            "\u{2502}                                                   \u{2502}",
            "\u{2502} Enter to select \u{00B7} \u{2191}/\u{2193} to navigate \u{00B7} Esc to cancel \u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256F}",
        ]);
        assert_eq!(
            state_of(evaluate("gemini", &s)),
            Some((ScreenState::Blocked, "ask_user_dialog"))
        );
        // 只剩头行（表单已答完、头行滚成残影）不算在等回答。
        let stale = snap(&["\u{2502} Answer Questions   \u{2502}", "  plain output"]);
        assert_ne!(
            state_of(evaluate("gemini", &stale)).map(|(state, _)| state),
            Some(ScreenState::Blocked)
        );
    }

    /// 做空第一轮 #8（2026-11-12）：提示行按框宽折行时也要认出。样本按 0.65.0 bundle
    /// （interactiveCli-*.js 的 AskUserDialog keyboardHints + DialogFooter 用 ` · ` 连接）拼出，非实拍：
    /// 多题选择 `Enter to select · ←/→ to switch questions · Esc to cancel`，多题文本
    /// `Enter to submit · Tab/Shift+Tab to switch questions · Esc to cancel`，确认页
    /// `Enter to submit · Tab/Shift+Tab to edit answers · Esc to cancel`。修前要求整串单行，窄窗
    /// 下 `Esc to cancel` 折到下一行就落回兜底判运行中。
    #[test]
    fn gemini_065_ask_user_dialog_wrapped_and_multi_question() {
        let frame = |body: &[&str]| {
            let mut lines = vec![
                "\u{280F} Waiting for user confirmation...",
                "\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{256E}",
                "\u{2502} Answer Questions              \u{2502}",
                "\u{2502}\u{2500}\u{2500}\u{2500}\u{2500}\u{2502}",
            ];
            lines.extend_from_slice(body);
            lines.push("\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{256F}");
            snap(&lines)
        };
        let cases = [
            // 多题选择，40 列框：导航段与取消段折到第二、三行。
            frame(&[
                "\u{2502} \u{2190} Q1 \u{2502} Q2 \u{2192}                   \u{2502}",
                "\u{2502} Which database should we use?  \u{2502}",
                "\u{2502} \u{25CF} 1.  PostgreSQL                \u{2502}",
                "\u{2502}   2.  SQLite                    \u{2502}",
                "\u{2502} Enter to select \u{00B7} \u{2190}/\u{2192} to switch  \u{2502}",
                "\u{2502} questions \u{00B7} Esc to cancel       \u{2502}",
            ]),
            // 多题文本题，提示行折成两行。
            frame(&[
                "\u{2502} What should the table be named? \u{2502}",
                "\u{2502} > users                         \u{2502}",
                "\u{2502} Enter to submit \u{00B7} Tab/Shift+Tab to \u{2502}",
                "\u{2502} switch questions \u{00B7} Esc to cancel \u{2502}",
            ]),
            // 确认页（多题答完），不折行。
            frame(&[
                "\u{2502} Review your answers:            \u{2502}",
                "\u{2502} Q1 PostgreSQL                   \u{2502}",
                "\u{2502} Enter to submit \u{00B7} Tab/Shift+Tab to edit answers \u{00B7} Esc to cancel \u{2502}",
            ]),
        ];
        for s in &cases {
            assert_eq!(
                state_of(evaluate("gemini", s)),
                Some((ScreenState::Blocked, "ask_user_dialog"))
            );
        }
        // 正文里出现 Enter to select（没在框里、行首也不是它）不算。
        let prose = snap(&[
            "\u{2502} Answer Questions   \u{2502}",
            "  Press Enter to select a file (esc to cancel, 3s)",
        ]);
        assert_ne!(
            state_of(evaluate("gemini", &prose)).map(|(state, _)| state),
            Some(ScreenState::Blocked)
        );
    }

    /// gemini 0.65 沙箱扩权审批（ToolConfirmationMessage 的 sandbox_expansion 分支，源码取证
    /// 2026-11-12）：问句 `To run [Shell], allow access to the following?` 在框内。
    #[test]
    fn gemini_065_sandbox_expansion_is_blocked() {
        let s = snap(&[
            "\u{2502} ? Shell  npm install                         \u{2502}",
            "\u{2502} To run [Shell], allow access to the following? \u{2502}",
            "\u{2502} \u{2022} Network: All Urls                        \u{2502}",
            "\u{2502} \u{25CF} 1. Allow once                             \u{2502}",
        ]);
        assert_eq!(
            state_of(evaluate("gemini", &s)),
            Some((ScreenState::Blocked, "apply_or_allow_change"))
        );
        // 正文引用同一句（不在框里）不算。
        let prose = snap(&["To run [Shell], allow access to the following?"]);
        assert_eq!(
            state_of(evaluate("gemini", &prose)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    // -- opencode --

    #[test]
    fn opencode_permission_panel_is_blocked() {
        let titled = snap(&["  \u{25B3} Permission required", "  write src/lib.rs"]);
        assert_eq!(
            state_of(evaluate("opencode", &titled)),
            Some((ScreenState::Blocked, "permission_required"))
        );
        // 无标题时要三类快捷键同时在场。
        let by_hints = snap(&["  some panel", "  ↑↓ select · enter confirm · esc dismiss"]);
        assert_eq!(
            state_of(evaluate("opencode", &by_hints)),
            Some((ScreenState::Blocked, "permission_required"))
        );
        // 只有 dismiss 提示的普通浮层不算审批。
        let plain = snap(&["  some overlay", "  esc dismiss"]);
        assert_ne!(
            state_of(evaluate("opencode", &plain)).map(|(state, _)| state),
            Some(ScreenState::Blocked)
        );
    }

    #[test]
    fn opencode_progress_and_interrupt_are_working() {
        let bar = snap(&["  building", "  \u{25A0}\u{25A0}\u{25A0}\u{25A0}\u{25A0} 62%"]);
        assert_eq!(
            state_of(evaluate("opencode", &bar)),
            Some((ScreenState::Working, "progress_bar_working"))
        );
        // 不足 4 连的方块不算进度条（避免命中普通符号）。
        let short = snap(&["  \u{25A0}\u{25A0} bullet"]);
        assert_eq!(
            state_of(evaluate("opencode", &short)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
        let hint = snap(&["  thinking (esc to interrupt)"]);
        assert_eq!(
            state_of(evaluate("opencode", &hint)),
            Some((ScreenState::Working, "interrupt_hint_working"))
        );
        // 1.19.3 底栏：`esc ` + `interrupt`，按过一次 Esc 后 `esc again to interrupt`；关了动画时
        // spinner 换成 `[⋯]`（opencode.exe 内嵌 JS，research-cli-drift O1，2026-11-12）。
        for line in [
            " [\u{22EF}] esc interrupt",
            " [\u{22EF}] esc again to interrupt",
        ] {
            assert_eq!(
                state_of(evaluate("opencode", &snap(&["  reading files", line]))),
                Some((ScreenState::Working, "interrupt_hint_working")),
                "应识别为运行中: {line:?}"
            );
        }
    }

    /// 所有内置规则的正则必须可编译。求值层遇到坏正则是「整条规则跳过」（不 panic），
    /// 那对线上是对的容错，但会让写错的规则**静默失效**——只能靠这条测试当场发现。
    #[test]
    fn every_declared_regex_compiles() {
        fn walk(matcher: &meowo_agent::screen::Matcher, out: &mut Vec<&'static str>) {
            use meowo_agent::screen::Matcher::*;
            match matcher {
                LineRegex(pattern) => out.push(pattern),
                All(inner) | Any(inner) | Not(inner) => {
                    inner.iter().for_each(|m| walk(m, out));
                }
                _ => {}
            }
        }
        let mut checked = 0;
        for plugin in meowo_agent::all() {
            for rule in plugin.screen_rules() {
                let mut patterns = Vec::new();
                walk(&rule.matcher, &mut patterns);
                for pattern in patterns {
                    assert!(
                        compiled_regex(pattern).is_some(),
                        "{}／{} 的正则编译失败：{pattern}",
                        plugin.id().as_str(),
                        rule.id
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 0, "应至少有一条规则用了正则");
    }

    /// kimi `/undo N` 的拒绝行：前端按插件声明的 reject_pattern（`i` 标志）逐行找，命中的整行
    /// 原样转给用户，认不出就只报「未确认」。这里用产品里的那份正则（不是前端测试夹具）
    /// 对 kimi 2.5.1 包内原文逐条核对（undo.ts 的 handleUndoCommand / formatUndoLimitMessage，
    /// 2026-11-12 取证），旧版文案照认。放在这里是因为只有本 crate 依赖 regex。
    #[test]
    fn kimi_undo_reject_pattern_matches_251_wording() {
        let pattern = meowo_agent::resolve(Some("kimi"))
            .and_then(|plugin| plugin.undo_control())
            .and_then(|control| control.reject_pattern)
            .expect("kimi 声明了 /undo 拒绝规则");
        let re = regex::RegexBuilder::new(pattern)
            .case_insensitive(true)
            .build()
            .expect("拒绝规则可编译");
        for line in [
            // 2.5.1 新文案
            "Cannot undo 3 prompts; only 1 can be undone in the active context.",
            "Cannot undo 2 prompts; only 1 prompt can be undone in the active context after the last compaction.",
            "Cannot undo 1 prompt; only 0 prompts can be undone in the active context.",
            "Usage: /undo [count], where count is a positive integer.",
            "No active session. Send a message to start one.",
            "Cannot undo while streaming \u{2014} press Esc or Ctrl-C first.",
            "Nothing to undo after the last compaction.",
            "Loaded updates from another client. Review and run /undo again.",
            "Failed to undo: session busy",
            // 旧版文案
            "Nothing to undo.",
            "Cannot undo while streaming",
        ] {
            assert!(re.is_match(line), "应认出拒绝行：{line}");
        }
        // 普通正文里提到 undo 不能当成拒绝。
        for line in ["Undid 1 prompt.", "我们可以 undo 一下吗", "cannot undo this, sorry"] {
            assert!(!re.is_match(line), "不应当成拒绝：{line}");
        }
    }

    /// 一次求值的开销必须远小于检测节拍（300ms），否则会话一多就会拖住 ticker 线程。
    ///
    /// 正则是本模块唯一可能变慢的部分：编译一次就够，但**每轮都重编译**的话，几十条
    /// 规则 × 每 300ms × 每个会话，很快就会变成可观的持续开销。这条测试跑满屏文本的
    /// 求值，把单次耗时钉在毫秒级——它验证的与其说是速度，不如说是「缓存确实生效」。
    #[test]
    fn evaluating_a_full_screen_stays_cheap() {
        // 24×80 的满屏文本，且刻意不命中任何高优先级规则（走完全部规则最费）。
        let lines: Vec<&str> = (0..24)
            .map(|_| "  compiling some crate with a fairly long line of build output here")
            .collect();
        let snap = snap_titled(&lines, "codex - building");

        // 先跑一次让正则进缓存（首次编译不计入——线上也只付一次）。
        let _ = evaluate("codex", &snap);

        // 分批计时取最快一批：整体平均会被 CI 共享机器的一次抢占拉高（macOS CI 实测
        // 1.09ms 误报）。缓存失效时每一批都慢，最快那批照样超标，拦截能力不变。
        let (batches, rounds) = (10u32, 20u32);
        let per_eval = (0..batches)
            .map(|_| {
                let started = std::time::Instant::now();
                for _ in 0..rounds {
                    let _ = evaluate("claude", &snap);
                    let _ = evaluate("codex", &snap);
                    let _ = evaluate("kimi", &snap);
                }
                started.elapsed() / (rounds * 3)
            })
            .min()
            .unwrap();
        // 阈值取 1ms：实测在开发机上是微秒级，留足 CI 慢机器的余量，只拦「缓存失效」
        // 这种量级的退化（重编译正则会让单次跳到几十微秒以上并随规则数线性增长）。
        assert!(
            per_eval < std::time::Duration::from_millis(1),
            "单次求值 {per_eval:?} 过慢——正则缓存可能失效了"
        );
    }

    /// 规则的「同行耦合」与「整行等值」不能退化成 region 级的分散包含——
    /// 屏幕上别处出现同样的词很常见，退化后会把普通输出误判成需要关注的状态。
    /// 三条均由代码审查发现，此处用它给出的失败场景钉住。
    #[test]
    fn rules_require_same_line_evidence_not_scattered_words() {
        // claude /btw：`/btw` 后须是空白或行尾，提示须在行尾。
        let fake_btw = snap(&[
            "  /btwhatever is this",
            "  press esc to close this dialog and continue",
        ]);
        assert_ne!(
            state_of(evaluate("claude", &fake_btw)).map(|(state, _)| state),
            Some(ScreenState::Working),
            "/btwhatever 与句中的 esc to close 不构成 /btw 覆盖层"
        );

        // kimi 提问面板：标题行必须整行等于 question。
        let fake_question = snap(&[
            " questions about the design",
            " ? which one",
            " ↑↓ select \u{00B7} \u{21B5} choose \u{00B7} esc cancel",
        ]);
        assert_ne!(
            state_of(evaluate("kimi", &fake_question)).map(|(state, _)| state),
            Some(ScreenState::Blocked),
            "「questions about…」不是提问面板的标题行"
        );

        // codex 状态行：Working 与 esc to interrupt 必须同行。
        let split_lines = snap(&[
            " \u{2022} Working (3m 12s)",
            "  earlier output mentioning esc to interrupt",
        ]);
        assert_ne!(
            state_of(evaluate("codex", &split_lines)).map(|(state, _)| state),
            Some(ScreenState::Working),
            "中断提示在别行时不算当前状态行"
        );
        // 同行的真状态行仍然识别。
        let real = snap(&[" \u{2022}  Working (3m 12s \u{00B7} esc to interrupt)"]);
        assert_eq!(
            state_of(evaluate("codex", &real)),
            Some((ScreenState::Working, "screen_working_fallback"))
        );
    }

    /// kimi 的审批标题行必须**以问号结尾**（原始规则 `^\s*▶?\s*approve .*\?$`）。
    /// 漏掉这个约束会把「approve this plan」这类普通输出误报成等待审批——而 blocked
    /// 误报是最坏方向：弹通知说 agent 在等你，实际它跑得好好的。
    #[test]
    fn kimi_approve_title_requires_a_question_mark() {
        let panel = |title: &str| {
            snap(&[
                title,
                " \u{25B6} Approve once   Reject",
                " 1/2/3 choose \u{00B7} \u{21B5} confirm",
            ])
        };
        // 真实审批标题：问号结尾 → blocked。
        assert_eq!(
            state_of(evaluate("kimi", &panel(" \u{25B6}  Approve this command?"))),
            Some((ScreenState::Blocked, "current_approval_panel")),
            "▶ 与 approve 之间多个空格也应识别"
        );
        // 普通输出：无问号 → 不得判 blocked。
        let plain = snap(&[
            " I will approve this plan and continue",
            " \u{25B6} Approve once   Reject",
            " 1/2/3 choose \u{00B7} \u{21B5} confirm",
        ]);
        assert_ne!(
            state_of(evaluate("kimi", &plain)).map(|(state, _)| state),
            Some(ScreenState::Blocked),
            "没有问号的 approve 句子不是审批标题"
        );
    }

    // -- 防抖 --

    fn working_det() -> Evaluation {
        Evaluation::Publish(Detection {
            state: ScreenState::Working,
            visible: true,
            rule_id: "t",
        })
    }

    fn plain_idle() -> Evaluation {
        Evaluation::Publish(Detection {
            state: ScreenState::Idle,
            visible: false,
            rule_id: "fallback_idle",
        })
    }

    fn visible_idle() -> Evaluation {
        Evaluation::Publish(Detection {
            state: ScreenState::Idle,
            visible: true,
            rule_id: "live_prompt_box",
        })
    }

    /// working → 光秃 idle 需要 3 次确认；中途回到 working 则清零。
    #[test]
    fn plain_idle_needs_confirmations_after_working() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        assert_eq!(debounce.observe(&working_det(), t0), Some(ScreenState::Working));
        assert_eq!(debounce.observe(&plain_idle(), t0), None);
        assert!(debounce.pending());
        assert_eq!(debounce.observe(&plain_idle(), t0), None);
        // 第三次确认发布。
        assert_eq!(debounce.observe(&plain_idle(), t0), Some(ScreenState::Idle));
        assert!(!debounce.pending());
    }

    #[test]
    fn pending_idle_publishes_after_time_cap() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        debounce.observe(&working_det(), t0);
        assert_eq!(debounce.observe(&plain_idle(), t0), None);
        // 超过 700ms 后第二次观察即发布，不必凑满 3 次。
        assert_eq!(
            debounce.observe(&plain_idle(), t0 + Duration::from_millis(701)),
            Some(ScreenState::Idle)
        );
    }

    #[test]
    fn visible_idle_bypasses_hold() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        debounce.observe(&working_det(), t0);
        assert_eq!(debounce.observe(&visible_idle(), t0), Some(ScreenState::Idle));
    }

    #[test]
    fn working_resumption_clears_pending() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        debounce.observe(&working_det(), t0);
        assert_eq!(debounce.observe(&plain_idle(), t0), None);
        // spinner 下一帧回来了：清掉待确认，不发布任何变化。
        assert_eq!(debounce.observe(&working_det(), t0), None);
        assert!(!debounce.pending());
    }

    #[test]
    fn hold_keeps_published_state() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        debounce.observe(&working_det(), t0);
        assert_eq!(
            debounce.observe(&Evaluation::Hold { rule_id: "transcript_viewer" }, t0),
            None
        );
        assert_eq!(debounce.published(), Some(ScreenState::Working));
    }

    /// blocked 立即发布（审批不能等确认期）。
    #[test]
    fn blocked_publishes_immediately() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        debounce.observe(&working_det(), t0);
        let blocked = Evaluation::Publish(Detection {
            state: ScreenState::Blocked,
            visible: true,
            rule_id: "live_blocked_form",
        });
        assert_eq!(debounce.observe(&blocked, t0), Some(ScreenState::Blocked));
    }

    /// T-15：发布值的 fallback 来源随状态一起被记住——回退 idle（什么都没认出来）
    /// 与带可见证据的 idle（确认空闲）对角标是两种自信度。
    #[test]
    fn fallback_origin_is_tracked_with_the_published_state() {
        let mut debounce = ScreenDebounce::default();
        let t0 = Instant::now();
        assert!(!debounce.published_is_fallback()); // 未发布时不算
        debounce.observe(&working_det(), t0);
        assert!(!debounce.published_is_fallback());
        // 回退 idle 发布（确认凑齐后）→ 标 fallback。
        assert_eq!(debounce.observe(&plain_idle(), t0), None);
        assert_eq!(
            debounce.observe(&plain_idle(), t0 + Duration::from_millis(701)),
            Some(ScreenState::Idle)
        );
        assert!(debounce.published_is_fallback());
        // 同为 idle 但换成带可见证据的判定：状态不变（返回 None），自信度跟着升级。
        assert_eq!(debounce.observe(&visible_idle(), t0), None);
        assert!(!debounce.published_is_fallback());
        // 回到 working 当然也不是 fallback。
        assert_eq!(debounce.observe(&working_det(), t0), Some(ScreenState::Working));
        assert!(!debounce.published_is_fallback());
    }
}
