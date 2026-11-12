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
    // 状态行 `(… • esc to interrupt)`：按键运行时拼，结尾 ` to interrupt)` 是模板原样，全包仅 1 处
    //（做空第一轮 #7，2026-11-12；此前标「不搜」）。
    D::new(
        "screen:screen_working_fallback",
        "to interrupt)",
        "\u{2022} Working (3m \u{00B7} esc to interrupt)",
        "0.166.0 codex.exe：` to interrupt)` 1 处（紧挨 `fast_mode` 与 ` • ` 分隔符）；`working (` 0 处，运行时拼。状态行没有 0.166 实拍",
    ),
    // ── 记录格式：--version 首行与 rollout 字段（做空第一轮 #7，2026-11-12）──
    D::record(
        "record:codex-cli",
        "codex-cli",
        "codex-cli 0.166.0",
        "0.166.0 `codex --version` 首行原样（带 `codex-cli ` 前缀，research-cli-drift X2）；codex.exe 19 处",
    ),
    D::record(
        "record:ContextCompaction",
        "ContextCompaction",
        r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"ContextCompaction","id":"01a13e21"}}}"#,
        "0.162.1 rollout 实样（已压缩分隔条，research-cli-drift X3）；0.166.0 codex.exe 33 处",
    ),
    D::record(
        "record:approval_policy",
        "approval_policy",
        r#"{"type":"turn_context","payload":{"approval_policy":"on-request","sandbox_policy":{"type":"workspace-write"}}}"#,
        "0.163 rollout 实样（运行中权限档，research-cli-drift X4）；0.166.0 codex.exe 55 处",
    ),
    D::record(
        "record:history_base",
        "history_base",
        r#"{"type":"session_meta","payload":{"history_mode":"paginated","history_base":"019a"}}"#,
        "0.163+ 回溯续写段的 session_meta（lineage.rs 拼接）；0.166.0 codex.exe 6 处",
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
    // 记录查看页底栏三段在 pager_overlay.rs 的常量里紧挨着存（`to scrollto pageto jump`）。` to page` 单搜
    // 是泛词（`Failed to page read-only thread history`），永远「找到」；改为三段近邻（做空第一轮 #7）。
    D::near(
        "literal: to scroll",
        &["to scroll", "to page", "to jump"],
        48,
        "↑/↓ to scroll   pgup/pgdn to page   home/end to jump",
        "0.166.0 codex.exe：三段紧挨 `tui\\src\\pager_overlay.rs`，相距 16 字节",
    ),
    D::near(
        "literal: to page",
        &["to scroll", "to page", "to jump"],
        48,
        "↑/↓ to scroll   pgup/pgdn to page   home/end to jump",
        "同 to scroll；` to page` 单搜在 codex.exe 有 3 处，其中 2 处与记录查看页无关",
    ),
    D::near(
        "literal: to jump",
        &["to scroll", "to page", "to jump"],
        48,
        "↑/↓ to scroll   pgup/pgdn to page   home/end to jump",
        "同 to scroll",
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
