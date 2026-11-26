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

/// 屏幕上正挂着的「自动重试」行（插件的 `retry_line_pattern`，与对话页运行条同一串正则）。
/// 侧栏据此把「其实卡在网络/接口上」和「正常在干活」分开说。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ScreenRetry {
    /// 请求出错，按次数重试：第 attempt 次，共 max 次。
    Attempt { attempt: u32, max: u32 },
    /// 请求发出后迟迟没有回应，倒计时到了再重发（没有次数上限）。
    Stalled,
}

/// 只看屏幕底部这么多行：重试行画在输入框上方的 spinner 区，正文里引用的同款文字在更上面
/// （与前端 cliStatus.ts 的 RETRY_SCAN_LINES 同值）。
const RETRY_SCAN_LINES: usize = 12;

/// 从末屏认出正在进行的自动重试。provider 没声明重试行正则、或屏幕上没有，返回 None。
pub(crate) fn retry_note(provider: &str, snap: &ScreenSnapshot) -> Option<ScreenRetry> {
    let pattern = meowo_agent::resolve(Some(provider))?.retry_line_pattern()?;
    let re = compiled_regex(pattern)?;
    let start = snap.lines.len().saturating_sub(RETRY_SCAN_LINES);
    snap.lines[start..].iter().rev().find_map(|line| {
        let caps = re.captures(line)?;
        if caps.get(3).is_some() {
            return Some(ScreenRetry::Stalled);
        }
        let attempt = caps.get(1)?.as_str().parse().ok()?;
        let max = caps.get(2)?.as_str().parse().ok()?;
        (max > 0).then_some(ScreenRetry::Attempt { attempt, max })
    })
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
        Region::PromptBoxWithTurnTail => prompt_box_with_turn_tail(lines).unwrap_or(&[]),
        Region::ThroughPromptBox => prompt_box_bounds(lines).map_or(&[], |(_, end)| &lines[..end]),
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
    let (top, end) = prompt_box_bounds(lines)?;
    Some(&lines[top + 1..end])
}

/// 提示框的框顶分隔线下标与框体结束下标（不含）。
fn prompt_box_bounds(lines: &[String]) -> Option<(usize, usize)> {
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
    Some((top, end))
}

/// 本回合的尾巴连同提示框体（见 [`Region::PromptBoxWithTurnTail`]）：起点是框顶之上
/// 最后一条 `❯` 开头的回显行之后；没有回显（滚出屏幕）就从屏幕顶开始。
fn prompt_box_with_turn_tail(lines: &[String]) -> Option<&[String]> {
    let (top, end) = prompt_box_bounds(lines)?;
    let start = lines[..top]
        .iter()
        .rposition(|line| line.trim_start().starts_with('\u{276F}'))
        .map_or(0, |index| index + 1);
    Some(&lines[start..end])
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
/// 测试用：正则实际编译了几次（缓存命中不计）。性能测试据此断言缓存生效，不靠墙钟时间。
#[cfg(test)]
static REGEX_COMPILES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
            #[cfg(test)]
            REGEX_COMPILES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

    /// 用户实拍（2026-11-18）：回合中请求卡住，spinner 行换成红字的等待重试行，输入框照画 ❯。
    /// 没有标题证据时也要判 working（回合还在），且认出是「API 无响应、等待重试」。
    #[test]
    fn api_stalled_retry_line_is_working_and_noted() {
        let s = snap(&[
            "  \u{23BF}  Running\u{2026}",
            "",
            "\u{273B} Waiting for API response \u{00B7} will retry in 1m 9s \u{00B7} check your network",
            "────────────────────────",
            " \u{276F} ",
            "────────────────────────",
            "  Opus 5.5 · 41% ctx",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "api_retry_working"))
        );
        assert_eq!(super::retry_note("claude", &s), Some(super::ScreenRetry::Stalled));

        let s = snap(&[
            "\u{2717} API Error (529 Overloaded) \u{00B7} Retrying in 5s \u{00B7} attempt 2/10",
            "────────────────────────",
            " \u{276F} ",
            "────────────────────────",
        ]);
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "api_retry_working"))
        );
        assert_eq!(
            super::retry_note("claude", &s),
            Some(super::ScreenRetry::Attempt { attempt: 2, max: 10 })
        );

        // 正文里引用这句（不在行尾）不算；没有重试行时为 None。
        let quoted = snap(&[
            "日志里出现过 Waiting for API response · will retry in 5s · check your network，后来恢复了",
            "────────────────────────",
            " \u{276F} ",
            "────────────────────────",
        ]);
        assert_eq!(super::retry_note("claude", &quoted), None);
        assert_eq!(
            state_of(evaluate("claude", &quoted)),
            Some((ScreenState::Idle, "live_prompt_box"))
        );
        assert_eq!(super::retry_note("codex", &s), None);
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

    /// 实拍（2026-11-19，CC 2.1.327，主回合已结束、2 个后台子代理在跑）的末屏，去掉了正文。
    /// 输入框上方紧挨着的是回合结束行「✻ Waiting for N background agents to finish」，
    /// 不是运行中的 spinner 行。
    const BACKGROUND_WAIT_FRAME: &[&str] = &[
        "  修好的标准：",
        "  - 主会话空闲时，插话立刻发出去。",
        "",
        "\u{273B} Waiting for 2 background agents to finish",
        "",
        "────────────────────────────────────────────────────",
        "\u{276F}",
        "────────────────────────────────────────────────────",
        "  Opus 5.5 · 50% ctx",
        "  \u{23F5}\u{23F5} bypass permissions on · 2 shells · \u{2193} to manage",
        "",
        "  \u{25CF} main",
        "  \u{25EF} general-purpose  正… 10m 13s · \u{2193} 219.9k tokens",
        "  \u{25EF} general-purpose  读…   1m 30s · \u{2193} 68.9k tokens",
    ];

    /// 主回合结束、只剩后台子代理时，标题 spinner 还在转（DB 已是 waiting，宿主却一直发布
    /// working，对话页于是把插话扣在排队里——实拍 2026-11-19 会话 1823）。输入框上方紧挨着
    /// 「Waiting for N background agents to finish」就是主回合空闲的可见证据，要压过标题。
    #[test]
    fn background_agents_wait_line_beats_spinner_title() {
        for title in [
            "\u{25D0} Claude Code",
            "\u{25D1} 修插话排队",
            "\u{280B} Claude Code",
            "\u{2733} Claude Code",
        ] {
            let s = snap_titled(BACKGROUND_WAIT_FRAME, title);
            assert_eq!(
                state_of(evaluate("claude", &s)),
                Some((ScreenState::Idle, "background_wait_idle")),
                "title={title}"
            );
        }
        // 没有标题也一样。
        assert_eq!(
            state_of(evaluate("claude", &snap(BACKGROUND_WAIT_FRAME))),
            Some((ScreenState::Idle, "background_wait_idle"))
        );
        // 单个后台子代理（单数文案）、框里有草稿也算。
        let mut one: Vec<&str> = BACKGROUND_WAIT_FRAME.to_vec();
        one[3] = "\u{273B} Waiting for 1 background agent to finish";
        one[6] = "\u{276F} 怎么样了";
        let one = snap_titled(&one, "\u{25D0} Claude Code");
        assert_eq!(
            state_of(evaluate("claude", &one)),
            Some((ScreenState::Idle, "background_wait_idle"))
        );
        // 结束行与输入框之间夹着系统通知（实拍 2026-11-19 00:18 起那几帧）。
        let noticed = snap_titled(
            &[
                "\u{276F} 上一句用户输入",
                "",
                "\u{25CF} 收到，这条进了主会话。",
                "",
                "\u{273B} Waiting for 2 background agents to finish",
                "",
                "\u{25CF} Language server",
                "  \"plugin:rust-analyzer-lsp:rust-analyzer\"",
                "  couldn't start. The Errors tab in /plugin",
                "  may have the reason.",
                "",
                "────────────────────────────────────────────────────",
                "\u{276F} 怎么样了",
                "────────────────────────────────────────────────────",
                "  Opus 5.5 · 50% ctx",
                "  \u{23F5}\u{23F5} bypass permissions on (shift+tab to cycle) ·",
                "",
                "  \u{25CF} main",
                "  \u{25EF} general-purpose  Re… 18m 24s · \u{2193} 305.5k tokens",
                "  \u{25EF} general-purpose  正…  9m 41s · \u{2193} 155.9k tokens",
            ],
            "\u{25D0} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &noticed)),
            Some((ScreenState::Idle, "background_wait_idle"))
        );
    }

    /// 不误伤真正在跑的主回合：上一回合留下的「Waiting for …」行在滚动区里，新回合的
    /// spinner 行紧挨着输入框（实拍 2026-11-19 00:15:13 那一帧的形状）。
    #[test]
    fn stale_background_wait_line_does_not_mask_running_turn() {
        let frame = [
            "\u{273B} Waiting for 2 background agents to finish",
            "",
            "\u{276F} 请读取并结合以下本地附件完成任务",
            "",
            "\u{25CF} Reading 1 file\u{2026}",
            "",
            "\u{273B} Levitating\u{2026} (7s \u{00B7} \u{2193} 90 tokens)",
            "────────────────────────────────────────────────────",
            "\u{276F}",
            "────────────────────────────────────────────────────",
            "  \u{23F5}\u{23F5} bypass permissions on · 2 shells · esc to interrupt",
            "  \u{25CF} main",
            "  \u{25EF} general-purpose  正… 11m 32s · \u{2193} 234.9k tokens",
            "  \u{25EF} general-purpose  正…  2m 49s · \u{2193} 102.8k tokens",
        ];
        let s = snap_titled(&frame, "\u{25D0} Claude Code");
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "osc_title_moon_working"))
        );
        // 主回合跑起来、还没输出时 spinner 行直接接在 Waiting 行下面，同样不算空闲。
        let s = snap_titled(
            &[
                "\u{273B} Waiting for 2 background agents to finish",
                "\u{2722} Transmuting\u{2026}",
                "────────────────────────",
                "\u{276F}",
                "────────────────────────",
            ],
            "\u{25D0} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Working, "osc_title_moon_working"))
        );
        // 正文里引用这句（不是帧字符开头）不算强证据，只落到弱证据那条。
        let s = snap_titled(
            &[
                "  日志里有一行 Waiting for 2 background agents to finish",
                "────────────────────────",
                "\u{276F}",
                "────────────────────────",
            ],
            "\u{25D0} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &s)),
            Some((ScreenState::Idle, "spinner_title_idle_prompt"))
        );
    }

    /// 实拍（2026-11-19 00:31:12，DB waiting、2 个后台子代理在跑）：主回合以正文收尾，输入框上方
    /// 没有「Waiting for …」那一行，标题照转。只能靠「本回合尾巴里没有 spinner 行」认出主回合
    /// 停着——弱证据，不带 visible，working → idle 要经防抖确认期。
    #[test]
    fn spinner_title_without_spinner_line_is_weak_idle() {
        let frame = [
            "  已知限制：Lead 面板只拿到记录文件最后 6000",
            "  字，所以要判断开头是不是被截掉了。",
            "",
            "  插话排队的问题，代理还在抓现场数据和修复。",
            "",
            "────────────────────────────────────────────────────",
            "\u{276F} 怎么样了",
            "────────────────────────────────────────────────────",
            "  Opus 5.5 · 53% ctx",
            "  \u{23F5}\u{23F5} bypass permissions on · 1 shell · \u{2193} to manage",
            "",
            "  \u{25CF} main",
            "  \u{25EF} general-purpose  运… 18m 49s · \u{2193} 192.7k tokens",
            "  \u{25EF} lead-redteam     正…  5m 53s · \u{2193} 111.4k tokens",
        ];
        let eval = evaluate("claude", &snap_titled(&frame, "\u{25D0} Claude Code"));
        match &eval {
            Some(Evaluation::Publish(det)) => assert!(!det.visible, "弱证据不能直通防抖"),
            _ => panic!("应发布 idle"),
        }
        assert_eq!(
            state_of(eval),
            Some((ScreenState::Idle, "spinner_title_idle_prompt"))
        );

        // 防抖：从 working 降下来要连续确认，单帧不发布。
        let t0 = Instant::now();
        let mut debounce = ScreenDebounce::default();
        let working =
            evaluate("claude", &snap_titled(&frame[..5], "\u{25D0} Claude Code")).unwrap();
        assert_eq!(debounce.observe(&working, t0), Some(ScreenState::Working));
        let idle = evaluate("claude", &snap_titled(&frame, "\u{25D0} Claude Code")).unwrap();
        assert_eq!(
            debounce.observe(&idle, t0 + Duration::from_millis(10)),
            None
        );
        assert_eq!(
            debounce.observe(&idle, t0 + Duration::from_millis(800)),
            Some(ScreenState::Idle)
        );
    }

    /// 不误伤：主回合真在跑时（实拍 2026-11-19 00:31:27–37，后台子代理结束通知触发的新一轮），
    /// 输入框上方是 spinner 行，标题照判 working；请求卡住等重试时也一样。
    #[test]
    fn spinner_line_or_retry_keeps_title_working() {
        for spinner in [
            "\u{2722} Lollygagging\u{2026}",
            "\u{273B} Lollygagging\u{2026} (8s \u{00B7} thought for 1s)",
            "\u{00B7} Lollygagging\u{2026} (13s \u{00B7} \u{2193} 536 tokens)",
            "\u{273B} Waiting for API response \u{00B7} will retry in 1m 9s \u{00B7} check your network",
            "\u{2717} API Error (529 Overloaded) \u{00B7} Retrying in 5s \u{00B7} attempt 2/10",
        ] {
            let frame = [
                "  插话排队的问题，代理还在抓现场数据和修复。",
                "",
                "\u{25CF} Agent \"Review Lead panel polish\" finished \u{00B7} 6m 5s",
                "",
                spinner,
                "",
                "────────────────────────────────────────────────────",
                "\u{276F}",
                "────────────────────────────────────────────────────",
                "  Opus 5.5 · 53% ctx",
                "  \u{23F5}\u{23F5} bypass permissions on · 1 shell · /tasks to \u{2026}",
                "",
                "  \u{25CF} main",
                "  \u{25EF} general-purpose  正…  19m 4s · \u{2193} 192.7k tokens",
            ];
            let s = snap_titled(&frame, "\u{25D0} Claude Code");
            assert_eq!(
                state_of(evaluate("claude", &s)),
                Some((ScreenState::Working, "osc_title_moon_working")),
                "spinner={spinner}"
            );
        }
        // 运行中排队的插话若画在 spinner 行与输入框之间（`❯ …`），spinner 行照样作数。
        let queued = snap_titled(
            &[
                "\u{273B} Waiting for 2 background agents to finish",
                "",
                "\u{2722} Lollygagging\u{2026} (21s \u{00B7} \u{2193} 1.2k tokens)",
                "",
                "\u{276F} 顺手改个名",
                "────────────────────────",
                "\u{276F}",
                "────────────────────────",
            ],
            "\u{25D0} Claude Code",
        );
        assert_eq!(
            state_of(evaluate("claude", &queued)),
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

    /// 2.6.1 跑长工具时 spinner 可以滚出末屏，但输入框仍常驻底部；此时不能再凭输入框
    /// 发布确凿 idle，否则它会覆盖 hook 的 running，让排队消息按 echo 记账并报未确认送达。
    #[test]
    fn kimi_composer_prompt_falls_back_instead_of_claiming_idle() {
        let s = snap(&[
            "  cargo test",
            "     Running unittests src/main.rs",
            "\u{256D}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256E}",
            "\u{2502} >      \u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{256F}",
            "kimi-for-coding  ~/repo",
            "        context: 12% (24k/200k)",
        ]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
        // bash 模式同样只是输入框形态，不能单独证明回合已结束。
        let bash = snap(&["\u{2502} !      \u{2502}", "context: 1%"]);
        assert_eq!(
            state_of(evaluate("kimi", &bash)),
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    /// 工作状态行与常驻输入框同屏时，仍以 spinner 的可见 working 证据为准。
    #[test]
    fn kimi_working_status_with_composer_prompt_is_working() {
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

    /// 2.6.1 的 live 回合标签是 `Working for <时长>`（workingLabel）；同屏输入框不得抢成等待。
    #[test]
    fn kimi_working_for_duration_outranks_composer_prompt() {
        let s = snap(&[" \u{280B} Working for 12s", "\u{2502} >      \u{2502}"]);
        assert_eq!(
            state_of(evaluate("kimi", &s)),
            Some((ScreenState::Working, "braille_spinner_working"))
        );
    }

    /// 转录里的普通输出与 bullet 用户消息都没有可见工作证据，只能落 fallback。
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
    /// 2026-11-12 取证）。旧规则只认 `working...`，会漏掉这种工作屏。
    /// 旧版三个点的写法（上面 `kimi_spinners_are_working`）仍认。
    #[test]
    fn kimi_251_ellipsis_spinner_with_composer_prompt_is_working() {
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
            Some((ScreenState::Idle, FALLBACK_RULE_ID))
        );
    }

    // -- antigravity --
    //
    // 以下画面都是 agy 1.5.18 真机屏幕（120x40 ConPTY + pyte 重建，2026-11-26，接入 antigravity 的
    // Lead 任务 research-screen-work/run2），只截了末尾十来行，路径与账号换成了占位。

    const AGY_STATUS_BUSY: &str = "esc to cancel                                                                                       Gemini 4 Neon \u{00B7} high";

    /// 命令审批面板取代输入框（run2/21_approval_cmd_2）。
    #[test]
    fn antigravity_command_approval_is_blocked() {
        let s = snap(&[
            "\u{25CF} Ran (Get-ChildItem -Force) (ctrl+o to expand)",
            "Command",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            "Requesting permission for:",
            "   Get-ChildItem -Force",
            "Run this command?",
            "> 1. Yes, run command",
            "  2. Yes, and always allow in this conversation for commands that start with 'Get-ChildItem -Force'",
            "  3. Yes, and always allow for commands that start with 'Get-ChildItem -Force' (Persist to settings.json)",
            "  4. No, cancel",
            "  \u{2191}/\u{2193} Navigate \u{00B7} tab Amend \u{00B7} ctrl+g edit/expand command \u{00B7} ctrl+r Review",
            AGY_STATUS_BUSY,
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &s)),
            Some((ScreenState::Blocked, "approval_panel"))
        );
    }

    /// 文件创建审批（run2/19_approval_file_2）：历史里还留着上一题的 `? 问句` / `> 答案` 回显。
    #[test]
    fn antigravity_file_approval_is_blocked() {
        let s = snap(&[
            "? \u{8BF7}\u{9009}\u{62E9}\u{8981}\u{521B}\u{5EFA}\u{7684}\u{6587}\u{4EF6}\u{540D}\u{FF1A}",
            "> b.txt",
            "Create file",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            "C:/Users/user/work/sandbox/b.txt",
            "  1  +  hi",
            "Allow creation of this file?",
            "> 1. Yes, allow creation",
            "  2. No, deny creation",
            "  \u{2191}/\u{2193} Navigate \u{00B7} f full diff \u{00B7} ctrl+r Review",
            AGY_STATUS_BUSY,
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &s)),
            Some((ScreenState::Blocked, "approval_panel"))
        );
    }

    /// 提问表单（run2/16_ask_form）。
    #[test]
    fn antigravity_question_form_is_blocked() {
        let s = snap(&[
            "Question",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            "Question 1/1: \u{8BF7}\u{9009}\u{62E9}\u{8981}\u{521B}\u{5EFA}\u{7684}\u{6587}\u{4EF6}\u{540D}\u{FF1A}",
            "> 1. (Recommended) a.txt",
            "  2. b.txt",
            "  3. Write-in...",
            "  \u{2191}/\u{2193} Navigate \u{00B7} enter Select \u{00B7} esc Skip \u{00B7} ctrl+r Review",
            "esc to cancel",
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &s)),
            Some((ScreenState::Blocked, "question_form"))
        );
    }

    /// 运行中：spinner 行 + 状态栏 `esc to cancel` 同屏（run2/26_running_intro_8）。答完的提问在历史里
    /// 留下 `? 问句` / `> 答案` 两行（run2/18_after_answer_1），不能被当成还在等回答。
    #[test]
    fn antigravity_spinner_is_working_and_answered_question_echo_is_not_blocked() {
        let running = snap(&[
            "> \u{7528}\u{4E00}\u{53E5}\u{8BDD}\u{4ECB}\u{7ECD}\u{4F60}\u{81EA}\u{5DF1}",
            "\u{287F}  Generating...",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            ">",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            AGY_STATUS_BUSY,
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &running)),
            Some((ScreenState::Working, "spinner_working"))
        );
        let answered = snap(&[
            "? \u{8BF7}\u{9009}\u{62E9}\u{8981}\u{521B}\u{5EFA}\u{7684}\u{6587}\u{4EF6}\u{540D}\u{FF1A}",
            "> b.txt",
            "\u{28FD}  Working...",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            ">",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            AGY_STATUS_BUSY,
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &answered)),
            Some((ScreenState::Working, "spinner_working"))
        );
    }

    /// 空闲时打开斜杠弹窗，状态栏左侧也是 `esc to cancel`（run2/24_slash_popup）：不能判成运行中。
    #[test]
    fn antigravity_slash_popup_with_esc_to_cancel_is_not_working() {
        let s = snap(&[
            "> /ti",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            "> /title                          Toggle custom terminal window title",
            "  /thinking                       Toggle streaming the model's full thinking in the transcript",
            "   \u{2193} 4 more",
            "  \u{2191}/\u{2193} Navigate \u{00B7} enter Select \u{00B7} tab Complete",
            AGY_STATUS_BUSY,
        ]);
        let state = state_of(evaluate("antigravity", &s)).map(|(state, _)| state);
        assert_ne!(state, Some(ScreenState::Working));
        assert_ne!(state, Some(ScreenState::Blocked));
    }

    /// 回合结束回到空闲（run2/27_idle_after_intro）：状态栏左侧 `? for shortcuts`。
    #[test]
    fn antigravity_shortcuts_hint_is_idle() {
        let s = snap(&[
            "  \u{6211}\u{662F} Antigravity\u{3002}",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            ">",
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
            "? for shortcuts                                                                                     Gemini 4 Neon \u{00B7} high",
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &s)),
            Some((ScreenState::Idle, "shortcuts_hint_idle"))
        );
    }

    /// 首次进入未信任目录（run1/00_start 的备用屏）。
    #[test]
    fn antigravity_trust_prompt_is_blocked() {
        let s = snap(&[
            "Accessing workspace:",
            "C:\\Users\\user\\work\\sandbox",
            "Do you trust the contents of this project?",
            "Antigravity CLI requires permission to read, edit, and execute files here.",
            "> Yes, I trust this folder",
            "  No, exit",
            "  \u{2191}/\u{2193} Navigate \u{00B7} enter Confirm",
        ]);
        assert_eq!(
            state_of(evaluate("antigravity", &s)),
            Some((ScreenState::Blocked, "trust_prompt"))
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

        // 先把三家各跑一次让正则进缓存（首次编译不计入——线上也只付一次）。
        for agent in ["claude", "codex", "kimi"] {
            let _ = evaluate(agent, &snap);
        }
        let compiles_before = REGEX_COMPILES.load(std::sync::atomic::Ordering::Relaxed);

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
        // 缓存是否生效直接数编译次数：预热后再求值不得重编译任何正则。此前只用 1ms 墙钟阈值兜，
        // Windows CI 共享机实测 1.07～1.13ms 连续误报（2026-11-18，三个 PR 上与改动无关地失败）。
        // 别的测试并行时也可能编译它们自己的正则，所以只认「本测试的规则」：同一批规则重复求值，
        // 计数若涨了，必是缓存失效（并行测试编的是别的模式串，预热后不会在本批循环里反复涨）。
        let compiles_after = REGEX_COMPILES.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            compiles_after - compiles_before < (batches * rounds) as usize,
            "预热后重复求值又编译了 {} 次正则——缓存失效",
            compiles_after - compiles_before
        );
        // 墙钟只兜数量级的退化（例如每轮扫描变成平方级），阈值放到 10ms，不再拿它判断缓存。
        assert!(
            per_eval < std::time::Duration::from_millis(10),
            "单次求值 {per_eval:?} 过慢"
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
