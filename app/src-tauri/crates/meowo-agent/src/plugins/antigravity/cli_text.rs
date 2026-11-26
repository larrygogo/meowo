//! antigravity 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：agy 1.5.18 本机程序文件 `%LOCALAPPDATA%/agy/bin/agy.exe`（Go 程序，字符串按 UTF-8 小写搜；
//! 2026-11-26 导出的字符串表里逐条核过出现次数），以及同日的真机屏幕（接入 antigravity 的 Lead 任务
//! research-screen-work）。选项文案在程序里是格式串（`Yes, and always allow tool '%s' (Persist to %s)`），
//! 搜的是格式串里固定的那一段。

use crate::cli_text::CliTextDep as D;

pub(super) static DEPS: &[D] = &[
    // ── 输入框规格（composer）──
    D::new(
        "composer.empty",
        "auto mode: edits",
        "Auto mode: edits & safe commands auto-approved (shift+tab to cycle)",
        "1.5.18 真机 run2/27：auto 档空输入框的占位；plan / accept-edits 档各有一句，default 档没有占位",
    ),
    // ── 屏幕规则的样例 ──
    D::new(
        "screen:approval_panel",
        "run this command?",
        "Run this command?",
        "1.5.18 真机 run2/05、21；agy.exe 里 2 处。文件审批的问句 `Allow creation of this file?` 1 处",
    ),
    D::new(
        "screen:question_form",
        "",
        "Question 1/1: 请选择要创建的文件名：",
        "1.5.18 真机 run2/16。agy.exe 里是格式串 `Question %d/%d`（1 处），题号运行时填，样例里没有原样，不搜",
    ),
    D::new(
        "screen:trust_prompt",
        "yes, i trust this folder",
        "> Yes, I trust this folder",
        "1.5.18 真机 run1/00_start 的备用屏；agy.exe 1 处",
    ),
    D::new(
        "screen:spinner_working",
        "esc to cancel",
        "esc to cancel                                         Gemini 4 Neon · high",
        "1.5.18 真机 run2/26：运行中状态栏左侧；agy.exe 8 处。盲文 spinner 帧是运行时画的，不搜",
    ),
    D::new(
        "screen:shortcuts_hint_idle",
        "for shortcuts",
        "? for shortcuts                                       auto · Gemini 4 Neon · high",
        "1.5.18 真机 run2/27：空闲状态栏左侧；agy.exe 2 处",
    ),
    // ── 提示识别与选项角色 ──
    D::new(
        "attention:antigravity:command-approval",
        "run this command?",
        "Run this command?",
        "同 screen:approval_panel",
    ),
    D::new(
        "option:antigravity:command-approval#0",
        "yes, run command",
        "Yes, run command",
        "1.5.18 真机 run2/05；agy.exe 1 处。文件审批 `Yes, allow creation`、访问 `Yes, allow access`、工具 `Yes, allow tool call` 各 1 处",
    ),
    D::new(
        "option:antigravity:command-approval#1",
        "in this conversation for commands that start with",
        "Yes, and always allow in this conversation for commands that start with 'Get-ChildItem -Force'",
        "1.5.18 真机 run2/05；agy.exe 格式串 `Yes, and always allow in this conversation for commands that start with '%s'`",
    ),
    D::new(
        "option:antigravity:command-approval#2",
        "(persist to %s)",
        "Yes, and always allow for commands that start with 'Get-ChildItem -Force' (Persist to %s)",
        "1.5.18 真机 run2/05；agy.exe 里 4 个格式串都以 `(Persist to %s)` 结尾",
    ),
    D::new(
        "option:antigravity:command-approval#3",
        "no, cancel",
        "No, cancel",
        "1.5.18 真机 run2/05；agy.exe 1 处。另有 `No, deny creation` / `No, deny access` / `No, deny tool call`",
    ),
    D::new(
        "attention:antigravity:question",
        "",
        "Question 1/1: 请选择要创建的文件名：",
        "同 screen:question_form：格式串 `Question %d/%d`，不搜",
    ),
    D::new(
        "attention:antigravity:trust",
        "do you trust the contents of this project?",
        "Do you trust the contents of this project?",
        "1.5.18 真机 run1/00_start；agy.exe 1 处",
    ),
    // ── 自动抽取的字面量 ──
    D::new(
        "literal:do you trust the files in this folder",
        "",
        "Do you trust the files in this folder?",
        "通用信任表（chat_ui.rs）里的句子；agy 的信任页说的是 contents of this project，程序文件里本来就没有，不搜",
    ),
    D::new(
        "literal:do you trust the contents of this directory",
        "",
        "Do you trust the contents of this directory?",
        "通用信任表（chat_ui.rs）里的句子；agy 用的是 this project，不搜",
    ),
    D::new(
        "literal:auto · ",
        "",
        "auto · Gemini 4 Neon · high",
        "运行时拼接：状态栏右侧的模式名、模型名、effort 分别渲染，中间点由模板插入；`auto` 在程序文件里是通用词，不搜",
    ),
    D::new(
        "literal:accept-edits · ",
        "",
        "accept-edits · Gemini 4 Neon · high",
        "运行时拼接：同 auto · ",
    ),
    D::new(
        "literal:plan · ",
        "",
        "plan · Gemini 4 Neon · high",
        "运行时拼接：同 auto · ",
    ),
];
