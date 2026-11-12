//! kimi 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：Kimi Code 2.5.1 本机程序文件 `~/.kimi-code/bin/kimi.EXE` 的字符串（按 UTF-8 小写计数），
//! 以及第三轮 kimi/gemini 组的取证（scratchpad ux2/impl-cli-kimigem/lit.py），2026-11-12。
//! kimi 的底栏提示是「按键 + 说明」逐段拼出来的（如 `1-${n} / ↵ choose`），整串在程序文件里没有原样；
//! 这类规则给能搜到的那一段，搜不到的写明理由。

use crate::cli_text::CliTextDep as D;

const TRUST_TABLE: &str =
    "通用信任表（chat_ui.rs）里的句子，给有信任页的 CLI 用；kimi 2.5.1 程序文件里 0 处，不搜";

pub(super) static DEPS: &[D] = &[
    // ── 正则规则的样例 ──
    D::new(
        "effort.confirm",
        "thinking set to",
        "Thinking set to high.",
        "2.5.1 kimi.EXE：`Thinking set to ` 2 处（/effort 成功后的回话）",
    ),
    D::new(
        "effort.reject",
        "unsupported thinking effort",
        "Unsupported thinking effort: max",
        "2.5.1 kimi.EXE：`unsupported thinking effort` 1 处；同一规则里的 `cannot switch models while streaming` 0 处，留作旧版兼容",
    ),
    D::new(
        "undo.reject",
        "cannot undo ",
        "Cannot undo 3 prompts; only 1 in this session.",
        "2.5.1 kimi.EXE formatUndoLimitMessage：`Cannot undo ` 4 处，后面的数字与原因是运行时拼的",
    ),
    D::new(
        "clipboard_image",
        "[image #",
        "[Image #1]",
        "2.5.1 kimi.EXE：`[image #` 2 处（输入框里的贴图占位）；`[image:` 1 处是旧形态",
    ),
    D::new(
        "attention:kimi:question",
        "←/→/tab switch",
        "↑↓ select  1/2 choose  ↵ confirm  ←/→/tab switch  esc cancel",
        "2.5.1 kimi.EXE renderSubmitTab 的提示行；`←/→/tab switch` 2 处，`1-${n} / ↵ choose` 是运行时拼的",
    ),
    D::new(
        "attention:kimi:command-approval",
        "run this command?",
        " \u{25B6}  Run this command?",
        "2.5.1 kimi.EXE：`Run this command?` 在审批问句表里（与 Write this file? 等并列）",
    ),
    D::new(
        "screen:current_approval_panel",
        "approve this command?",
        " \u{25B6}  Approve this command?",
        "0.29 实拍的审批框标题（旧证据）；2.5.1 改用 Run this command? 等分类问句",
    )
    .legacy(),
    D::new(
        "screen:question_panel",
        "",
        " ? Which database should we use?",
        "题干由 agent 生成、表头是通用词 question，搜程序文件证明不了什么；同屏的 ←/→/tab switch 已由 attention:kimi:question 盯着",
    ),
    D::new(
        "screen:question_submit_panel",
        "ready to submit your answers?",
        "  Ready to submit your answers?",
        "2.5.1 kimi.EXE renderSubmitTab：`Ready to submit your answers?` 2 处（提问表单最后的确认页）",
    ),
    D::new(
        "screen:background_agent_status_working",
        "agents running]",
        "kimi-k2 thinking  [2 agents running]",
        "2.5.1 之前状态行里的后台子代理计数；2.5.1 里 0 处，运行中改靠主 spinner 判断",
    )
    .legacy(),
    D::new(
        "screen:agent_running_status_working",
        "running (",
        "\u{280B} explore running (3s)",
        "2.5.1 之前的子代理运行行；2.5.1 里 0 处",
    )
    .legacy(),
    D::new(
        "screen:moon_tip_status_working",
        "· tip:",
        "\u{1F315} · Tip: press esc to interrupt",
        "2.5.1 kimi.EXE：月相 spinner 后接 ` · Tip:` 2 处",
    ),
    D::new(
        "screen:moon_word_status_working",
        "working…",
        "\u{1F317} Working…",
        "2.5.1 kimi.EXE：`ensureActivitySpinner(\"braille\",\"Working…\")`，省略号是单个字符；重试时为 `Retrying (n/m)`",
    ),
    // ── 自动抽取的字面量 ──
    D::new("literal:do you trust the files in this folder", "", "Do you trust the files in this folder?", TRUST_TABLE),
    D::new("literal:do you trust the contents of this directory", "", "Do you trust the contents of this directory?", TRUST_TABLE),
    D::new(
        "literal:↵ choose",
        "",
        "1-3 / ↵ choose",
        "运行时拼接：2.5.1 提问提示行是 `1-${n} / ` 加 `↵ choose`，按键与说明分开存，整串 0 处",
    ),
    D::new(
        "literal:↵ toggle",
        "",
        "1-3 / ↵ toggle",
        "运行时拼接：同 ↵ choose（多选题的写法）",
    ),
    D::new(
        "literal:1/2/3/4 choose",
        "",
        "↑/↓ select · 1/2/3/4 choose · ↵ confirm",
        "运行时拼接：审批提示行的数字段按选项个数生成，整串 0 处；`↵ confirm` 能搜到",
    ),
    D::new("literal:thinking...", "thinking...", "Thinking...", "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Thinking…`").legacy(),
    D::new("literal:working...", "working...", "Working...", "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Working…`").legacy(),
    D::new("literal:compacting...", "compacting...", "Compacting...", "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Compacting…`").legacy(),
];
