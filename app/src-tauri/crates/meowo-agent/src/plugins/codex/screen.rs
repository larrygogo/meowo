//! Codex 的屏幕状态规则（语义移植自 herdr codex.toml 2026.07.18.1，Apache-2.0）。
//!
//! codex 的特点是把状态写进**终端标题**：`Action Required` 直接判阻塞（唯一这么干的
//! 宿主），spinner 帧判运行中；屏幕侧则用 `›` 提示符标记锚定交互区。

use crate::screen::{Matcher, Region, ScreenRule, ScreenState};

/// codex 的标题 spinner（盲文十连帧，独立成词出现）。
const SPINNER: &str = "\u{280B}\u{2819}\u{2839}\u{2838}\u{283C}\u{2834}\u{2826}\u{2827}\u{2807}\u{280F}";

pub(super) static RULES: &[ScreenRule] = &[
    ScreenRule::new(
        "osc_title_blocked",
        ScreenState::Blocked,
        1100,
        Region::Title,
        Matcher::Contains(&["action required"]),
    )
    .visible(),
    ScreenRule::new(
        "osc_title_working",
        ScreenState::Working,
        1050,
        Region::Title,
        Matcher::WordCharIn(SPINNER),
    )
    .visible(),
    ScreenRule::new(
        "transcript_viewer",
        ScreenState::Idle,
        1000,
        Region::AfterLastPromptMarker,
        Matcher::Any(&[
            // 0.16x 之前（herdr codex.toml 原文）
            Matcher::All(&[
                Matcher::Contains(&[
                    "↑/↓ to scroll",
                    "pgup/pgdn to",
                    "home/end to jump",
                    "q to quit",
                ]),
                Matcher::AnyContains(&["esc to edit prev", "esc/← to edit prev"]),
            ]),
            // 0.166.0：pager_overlay.rs 的提示只剩按键 + ` to scroll` / ` to page` / ` to jump`
            // 三段（codex.exe 字符串紧挨 `tui\src\pager_overlay.rs`），`q to quit` 与
            // `esc to edit prev` 在二进制里 0 处（research-cli-drift X6，2026-11-12）。
            // 每段都要连着按键字形：只认三段说明时，正文里同时出现 to scroll / to page / to jump
            // 就被当成覆盖层（优先级 1000 带 hold，盖过运行中状态行），做空第一轮 #9，2026-11-12。
            // 字形取自 codex.exe 的按键显示表（`ente`/`pgup`/`pgdn`/`spac` 内联常量，方向键是箭头，
            // home/end 按小写名），键之间用 `/` 连（紧挨三段说明的 `/` 常量）。各段单独成行也认：
            // 窄窗会把底栏折行。
            Matcher::All(&[
                Matcher::LineRegex(r"\x{2191}\s*/\s*\x{2193} to scroll\b"),
                Matcher::LineRegex(r"\bpgup\s*/\s*pgdn to page\b"),
                Matcher::LineRegex(r"\bhome\s*/\s*end to jump\b"),
            ]),
        ]),
    )
    .hold(),
    ScreenRule::new(
        "live_strong_blocker",
        ScreenState::Blocked,
        900,
        Region::AfterLastPromptMarker,
        Matcher::Any(&[
            // 0.16x 之前的整句（herdr codex.toml）；提问表单的两句 0.166 仍在
            Matcher::AnyContains(&[
                "press enter to confirm or esc to cancel",
                "enter to submit answer",
                "enter to submit all",
                "allow command?",
            ]),
            // 0.166.0 的审批框：整句 `press enter to confirm…` 与 `allow command?` 在 codex.exe
            // 里 0 处，底栏改成按键 + ` to confirm` / ` to cancel` 两段拼出（二进制里紧挨审批
            // 选项文案）；审批选项固定带一条 `No, and tell Codex what to do differently`
            // （research-cli-drift X7，2026-11-12）。两者都只在审批框里出现。
            Matcher::LineRegex(r"enter to confirm.*esc to cancel"),
            Matcher::AnyContains(&["no, and tell codex what to do differently"]),
        ]),
    )
    .visible(),
    // 弱证据，不带 visible。区域锚定在最后一个 `›` 提示符之后（与 live_strong_blocker
    // 同区）：codex 正在跑的命令回显（apt/npm 的 `[y/n]`、diff 里的 yes）都落在标记
    // **之前**的输出区，不算数。优先级压在 screen_working_fallback(500) 之下：状态行
    // 还写着 Working 时，屏上散落的疑似审批词绝不能把运行中翻成待交互——规则失准
    // 只许少报 blocked，不许误报（标题无 spinner 时曾整段误报，实拍回归）。
    ScreenRule::new(
        "weak_blocker",
        ScreenState::Blocked,
        400,
        Region::AfterLastPromptMarker,
        Matcher::Any(&[
            Matcher::Contains(&["[y/n]"]),
            Matcher::Contains(&["yes (y)"]),
            Matcher::All(&[
                Matcher::Contains(&["do you want to"]),
                Matcher::AnyContains(&["yes", "\u{276F}"]),
            ]),
            Matcher::All(&[
                Matcher::Contains(&["would you like to"]),
                Matcher::AnyContains(&["yes", "\u{276F}"]),
            ]),
        ]),
    ),
    // 底部的 `• Working (…esc to interrupt)` 状态行；被中断则不算。
    ScreenRule::new(
        "screen_working_fallback",
        ScreenState::Working,
        500,
        Region::BottomNonEmpty(3),
        Matcher::All(&[
            // 逐字对齐 herdr 原始规则的 `^[•◦]\s+Working \(`：项目符号与 Working 之间
            // 是 `\s+` 而非单个空格——状态行的对齐空格随终端宽度变化，写死单空格会让
            // 那些屏幕漏判成空闲（已由 codex_working_line_tolerates_extra_spaces 钉住）。
            // 状态行是一整行：`• Working (3m · esc to interrupt)`。中断提示必须与
            // Working 在**同一行**——拆成 region 级 Contains 会把「上一行是 Working、
            // 另一行提到 esc to interrupt」的残影屏误判成运行中。
            Matcher::LineRegex(r"^\s*[\u{2022}\u{25E6}]\s+working \(.*esc to interrupt"),
            Matcher::Not(&[Matcher::Contains(&["■ conversation interrupted"])]),
        ]),
    )
    .visible(),
    // 有标题但既不转也不告警 → 空闲。
    ScreenRule::new(
        "osc_title_idle",
        ScreenState::Idle,
        100,
        Region::Title,
        Matcher::All(&[
            Matcher::NonEmpty,
            Matcher::Not(&[
                Matcher::WordCharIn(SPINNER),
                Matcher::Contains(&["action required"]),
            ]),
        ]),
    )
    .visible(),
];
