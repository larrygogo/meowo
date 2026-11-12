//! opencode 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：opencode 1.19.3 本机程序文件 `%APPDATA%/npm/node_modules/opencode-ai/bin/opencode.exe`
//! 内嵌 JS 的字符串（按 UTF-8 小写搜），以及第三轮调研 research-cli-drift.md 的 O 系列结论，2026-11-12。
//! opencode 的 TUI 把按键和说明分成两个文本节点渲染（如 `K("esc ")` 后接 `"interrupt"`），
//! 屏幕上连在一起的「esc dismiss」「enter confirm」在程序文件里没有原样；这类写明理由、不搜。

use crate::cli_text::CliTextDep as D;

const KEY_LABEL_SPLIT: &str =
    "运行时拼接：opencode 1.19.3 把按键与说明分成两个节点渲染（程序文件里 `\"esc \"` 8 处单独出现），整串没有原样";

pub(super) static DEPS: &[D] = &[
    // ── 正则规则的样例 ──
    D::new(
        "clipboard_image",
        "[image ",
        "[Image 1]",
        "1.19.3 opencode.exe 内嵌 JS：`[image ${n+1}]`（与 `[pdf ${n+1}]` 并列的附件占位）",
    ),
    D::new(
        "attention:opencode:command-approval",
        "permission required",
        "\u{25B3} Permission required",
        "1.19.3 opencode.exe：`c(x,k(\"\\u25b3\")),c(j,k(\"permission required\"))`，三角符号与文字分开渲染",
    ),
    // ── 自动抽取的字面量 ──
    D::new(
        "literal:do you trust the files in this folder",
        "",
        "Do you trust the files in this folder?",
        "通用信任表（chat_ui.rs）里的句子；opencode 没有信任页，程序文件里本来就没有，不搜",
    ),
    D::new(
        "literal:do you trust the contents of this directory",
        "",
        "Do you trust the contents of this directory?",
        "通用信任表（chat_ui.rs）里的句子；opencode 没有信任页，程序文件里本来就没有，不搜",
    ),
    D::new(
        "literal:build · ",
        "",
        "build · claude-opus-5-5",
        "运行时拼接：模式名与模型名在底栏分别渲染，中间点由模板插入；`build`、`plan` 在程序文件里是通用词，搜它们证明不了什么",
    ),
    D::new(
        "literal:plan · ",
        "",
        "plan · claude-opus-5-5",
        "运行时拼接：同 build · ",
    ),
    D::new(
        "literal:\u{25B3} permission required",
        "permission required",
        "\u{25B3} Permission required",
        "1.19.3 opencode.exe：三角符号 `\\u25b3` 与 `permission required` 分开渲染，后者 4 处",
    ),
    D::new("literal:esc dismiss", "", "esc dismiss", KEY_LABEL_SPLIT),
    D::new("literal:enter confirm", "", "enter confirm", KEY_LABEL_SPLIT),
    D::new("literal:enter submit", "", "enter submit", KEY_LABEL_SPLIT),
    D::new("literal:enter toggle", "", "enter toggle", KEY_LABEL_SPLIT),
    D::new("literal:↑↓ select", "", "↑↓ select", KEY_LABEL_SPLIT),
    D::new("literal:⇆ tab", "", "⇆ tab", KEY_LABEL_SPLIT),
    D::new(
        "literal:esc to interrupt",
        "esc to interrupt",
        "esc to interrupt",
        "1.19.2 之前的运行中底栏；1.19.3 里 0 处（改成 `esc ` + `interrupt`）",
    )
    .legacy(),
    D::new("literal:ctrl+c to interrupt", "ctrl+c to interrupt", "ctrl+c to interrupt", "早期运行中底栏写法；1.19.3 里 0 处").legacy(),
    D::new("literal:press esc to interrupt", "press esc to interrupt", "press esc to interrupt", "早期运行中底栏写法；1.19.3 里 0 处").legacy(),
    D::new(
        "literal:esc again to interrupt",
        "again to interrupt",
        "esc again to interrupt",
        "1.19.3：按过一次 Esc 后底栏是 `esc ` + `again to interrupt`，后者在程序文件里 2 处",
    ),
    D::new(
        "literal:esc interrupt",
        "",
        "esc interrupt",
        "运行时拼接：1.19.3 底栏是 `K(\"esc \")` 后接 `\"interrupt\"`（research-cli-drift O1）；`interrupt` 是通用词，搜它证明不了什么",
    ),
];
