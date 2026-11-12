//! codex 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：Codex 0.166.0 本机程序文件 `~/.codex/packages/standalone/releases/0.166.0-x86_64-pc-windows-msvc/bin/codex.exe`
//! 的字符串（按 UTF-8 小写搜），以及第三轮调研 research-cli-drift.md 的 X 系列结论，2026-11-12。
//! codex 的 TUI 底栏是「按键 + ` to …`」分段拼出来的，整句在程序文件里通常没有原样，
//! 这类规则给能搜到的那一段，搜不到的写明理由。

use crate::cli_text::CliTextDep as D;

pub(super) static DEPS: &[D] = &[
    // ── 正则规则的样例 ──
    D::new(
        "clipboard_image",
        "[image #",
        "[Image #1]",
        "0.166.0 codex.exe 含 `- [Image #…]` 模板（附件占位，与输入框里的贴图占位同一写法）",
    ),
    D::new(
        "screen:live_strong_blocker",
        " to confirm",
        "  enter to confirm   esc to cancel",
        "0.166.0 codex.exe：` to confirm` 14 处、` to cancel` 7 处，审批框底栏是按键 + 这两段拼出（research-cli-drift X7）",
    ),
    D::new(
        "screen:screen_working_fallback",
        "",
        "\u{2022} Working (3m \u{00B7} esc to interrupt)",
        "运行时拼接：0.166.0 codex.exe 里 `working (` 与 `esc to interrupt` 都是 0 处，只有 `to interrupt`；状态行没有 0.166 实拍，规则沿用 0.16x 写法，属于「有风险但没证据」",
    ),
    // ── 自动抽取的字面量：旧版兼容 ──
    D::new(
        "literal:do you trust the files in this folder",
        "do you trust the files in this folder",
        "Do you trust the files in this folder?",
        "0.16x 之前的信任页标题；0.166.0 改为 `Trust this folder?`（codex.exe 里 `do you trust` 0 处）",
    )
    .legacy(),
    D::new(
        "literal:do you trust the contents of this directory",
        "do you trust the contents of this directory",
        "Do you trust the contents of this directory?",
        "更早版本的信任页标题；0.166.0 已无",
    )
    .legacy(),
    D::new("literal:↑/↓ to scroll", "↑/↓ to scroll", "↑/↓ to scroll", "0.16x 之前记录查看页底栏（herdr codex.toml）；0.166.0 改为按键 + ` to scroll`").legacy(),
    D::new("literal:pgup/pgdn to", "pgup/pgdn to", "pgup/pgdn to page", "0.16x 之前记录查看页底栏；0.166.0 已无").legacy(),
    D::new("literal:home/end to jump", "home/end to jump", "home/end to jump", "0.16x 之前记录查看页底栏；0.166.0 改为按键 + ` to jump`").legacy(),
    D::new("literal:q to quit", "q to quit", "q to quit", "0.16x 之前记录查看页底栏；0.166.0 里 0 处").legacy(),
    D::new("literal:esc to edit prev", "esc to edit prev", "esc to edit prev", "0.16x 之前记录查看页底栏；0.166.0 里 0 处").legacy(),
    D::new("literal:esc/← to edit prev", "esc/← to edit prev", "esc/← to edit prev", "0.16x 之前记录查看页底栏；0.166.0 里 0 处").legacy(),
    D::new(
        "literal:press enter to confirm or esc to cancel",
        "press enter to confirm or esc to cancel",
        "press enter to confirm or esc to cancel",
        "0.16x 之前审批框整句（herdr codex.toml）；0.166.0 改为按键分段拼出",
    )
    .legacy(),
    D::new("literal:allow command?", "allow command?", "Allow command?", "0.16x 之前审批框标题；0.166.0 里 0 处").legacy(),
    D::new("literal:yes (y)", "yes (y)", "Yes (y)", "早期审批选项写法（herdr 弱规则）；0.166.0 里 0 处").legacy(),
    // ── 自动抽取的字面量：运行时拼接，给能搜到的那一段 ──
    D::new(
        "literal: to scroll",
        "to scroll",
        "↑/↓ to scroll",
        "0.166.0 记录查看页底栏是按键 + ` to scroll`；程序文件里 `to scroll` 1 处（紧挨 tui\\src\\pager_overlay.rs），前导空格在拼接处",
    ),
    D::new(
        "literal: to jump",
        "to jump",
        "home/end to jump",
        "0.166.0 记录查看页底栏是按键 + ` to jump`；程序文件里 `to jump` 1 处",
    ),
    D::new(
        "literal:enter to submit answer",
        "submit answer",
        "enter to submit answer",
        "0.166.0 提问表单底栏是按键 + `to submit answer`；程序文件里 `submit answer` 1 处",
    ),
    D::new(
        "literal:enter to submit all",
        "submit all",
        "enter to submit all",
        "0.166.0 提问表单底栏是按键 + `to submit all`；程序文件里 `submit all` 1 处",
    ),
];
