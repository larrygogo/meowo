//! Antigravity CLI 的屏幕状态规则。
//!
//! 全部来自真机屏幕（agy 1.5.17/1.5.18，120x40 ConPTY + pyte 重建，2026-11-26，任务目录
//! agents/research-screen.md，样本在 research-screen-work/run2）。要点：
//!
//! - agy 默认不发终端标题（`/title` 未配置时报错），运行态只能看屏幕：输入框上方一行
//!   「8 点盲文 spinner + 两个空格 + 文案」（`⣾  Generating...`、`⣾  Running command... 1s`，
//!   或灰色的思考摘要原文）。
//! - `esc to cancel` 在运行中、审批、提问、**空闲时打开斜杠弹窗**都会出现在状态栏左侧
//!   （run2/24_slash_popup.txt），不能单独当运行中的证据。
//! - 审批与提问面板会取代输入框，底部都有一行 `↑/↓ Navigate · …` 按键提示；面板答完就从屏幕上
//!   消失，不留在滚动区，所以拿它作证不会被历史里的旧面板骗到。历史里会留下 `? 问句` 和
//!   `> 答案` 两行回显，这两行**不能**作证（run2/18_after_answer_*.txt）。

use crate::screen::{Matcher, Region, ScreenRule, ScreenState};

pub(super) static RULES: &[ScreenRule] = &[
    // 工具审批面板。三样同时在场：独占一行、以问号结尾的问句；编号的 Yes 选项；底部按键提示。
    // 命令审批 `Run this command?` + `> 1. Yes, run command`（run2/05_approval_cmd.txt、
    // 21_approval_cmd_2.txt）；文件审批 `Allow creation of this file?` + `> 1. Yes, allow creation`
    // + `↑/↓ Navigate · f full diff`（run2/12_execute.txt、19_approval_file_2.txt）。光标移到拒绝项后
    // 提示条多一节 `tab Amend`（13_approval_file_cursor_on_no.txt），提示行只锚定开头。编辑已有文件
    // 的问句没有实拍，问句正则放宽到「Allow … ?」。
    ScreenRule::new(
        "approval_panel",
        ScreenState::Blocked,
        400,
        Region::WholeScreen,
        Matcher::All(&[
            Matcher::LineRegex(r"^(?:run this command|allow [^?]{1,80})\?$"),
            // 焦点移到别的项后，第一项是 `  1. Yes…`（缩进两格，run2/13_approval_file_cursor_on_no）。
            Matcher::LineRegex(r"^\s*(?:>\s*)?1\.\s+yes\b"),
            // 按键提示行在真机屏幕上缩进两格；正则按原样的行匹配，行首要容许空白。
            Matcher::LineRegex(r"^\s*↑/↓ navigate · "),
        ]),
    )
    .visible(),
    // 提问表单（模型调 ask_question）：`Question 1/1: <问句>` + 编号选项 + `↑/↓ Navigate · enter Select
    // · esc Skip`（run2/16_ask_form.txt）。
    ScreenRule::new(
        "question_form",
        ScreenState::Blocked,
        390,
        Region::WholeScreen,
        Matcher::All(&[
            Matcher::LineRegex(r"^question \d+/\d+: "),
            Matcher::LineRegex(r"^\s*↑/↓ navigate · enter select · esc skip\b"),
        ]),
    )
    .visible(),
    // 首次进入未信任目录的信任提示（画在备用屏里，run1/01_trust_prompt.txt）：会话刚起就停在这里，
    // 回车即信任。
    ScreenRule::new(
        "trust_prompt",
        ScreenState::Blocked,
        380,
        Region::WholeScreen,
        Matcher::All(&[
            Matcher::Contains(&["do you trust the contents of this project?"]),
            Matcher::LineRegex(r"^(?:>\s*)?yes, i trust this folder$"),
        ]),
    )
    .visible(),
    // 运行中：spinner 行独占一行（文案不限关键词，思考摘要是任意文本），并且状态栏左侧是
    // `esc to cancel`、右侧是「模型 · effort」。两者同屏才算：单独的盲文开头行可能是历史里的输出。
    // 出处 run2/26_running_intro_*.txt、10_running_create_*.txt。
    ScreenRule::new(
        "spinner_working",
        ScreenState::Working,
        200,
        Region::WholeScreen,
        Matcher::All(&[
            Matcher::LineRegex(r"^[\x{2800}-\x{28FF}]\s{2}\S"),
            Matcher::LineRegex(r"^esc to cancel\s{2,}.*·"),
        ]),
    )
    .visible(),
    // 空闲的正向证据：状态栏左侧 `? for shortcuts`（输入框为空时，run2/27_idle_after_intro.txt）。
    ScreenRule::new(
        "shortcuts_hint_idle",
        ScreenState::Idle,
        100,
        Region::BottomNonEmpty(1),
        Matcher::LineRegex(r"^\? for shortcuts\s{2,}"),
    ),
];
