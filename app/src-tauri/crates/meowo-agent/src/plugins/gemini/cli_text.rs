//! gemini 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：Gemini CLI 0.65.0 本机 npm 包 `@google/gemini-cli/bundle/*.js`（按 UTF-8 小写计数），
//! 以及第三轮 kimi/gemini 组的取证（scratchpad ux2/impl-cli-kimigem/lit.py），2026-11-12。
//! 对话框的框线 `│` 是终端界面库运行时画的，不在字符串里，带框线的规则一律去掉框线再搜；
//! bundle 里的非 ASCII 字符多写成 `\uXXXX` 转义，带箭头、中间点的整串搜不到原样。

use crate::cli_text::CliTextDep as D;

const FRAMED: &str = "0.65.0 bundle 里有这句；框线 `│` 是运行时画的边框，去掉框线再搜";

pub(super) static DEPS: &[D] = &[
    // ── 输入框规格（composer）──
    D::new(
        "composer.empty",
        "Type your message or @path/to/file",
        "Type your message or @path/to/file",
        "0.68.0 真机 2026-11-14（probe_tui_script cells）：空输入框的灰色占位",
    ),
    D::new(
        "composer.empty",
        "for NORMAL mode.",
        "Press 'Esc' for NORMAL mode.",
        "0.68.0 真机 2026-11-14：general.vimMode 打开、INSERT 模式下空输入框的占位",
    ),
    D::new(
        "composer.empty",
        "for INSERT mode.",
        "Press 'i' for INSERT mode.",
        "0.68.0 真机 2026-11-14：general.vimMode 打开、NORMAL 模式下空输入框的占位",
    ),
    D::new(
        "composer.paste",
        "[Pasted Text: ",
        " > [Pasted Text: 60 lines]",
        "0.68.0 真机 2026-11-14（tests/probe_draft_fidelity.rs）：粘贴 60 行后输入框里只剩这个占位，5120 字单行是 `[Pasted Text: 5120 chars]`；bundle/chunk-ASDLQWJU.js 48836 行的模板",
    ),
    D::new(
        "composer.alt:shell",
        "Type your shell command",
        "!   Type your shell command",
        "0.68.0 真机 2026-11-14：按 ! 进 shell 模式后提示符变成 `!`，占位是这句；规则认的是提示符，占位用来在程序文件里定位这个模式还在",
    ),
    D::new(
        "composer.alt:vim_normal",
        "",
        " [NORMAL]       ~\\repo        no sandbox         Auto",
        "0.68.0 真机 2026-11-14：vim NORMAL 模式下状态行最左侧的标记（INSERT 时是 [INSERT]）。方括号是运行时拼的，bundle 里没有原样的 `[NORMAL]`；vim 模式是否还在由 composer.empty 的两句占位（for NORMAL mode. 与 for INSERT mode.）盯着",
    ),
    // ── 正则规则的样例 ──
    D::new(
        "attention:gemini:command-approval",
        "allow execution of",
        "\u{2502} Allow execution of npm test?",
        "0.65.0 bundle：`Allow execution of` 在工具确认框里；同规则还认 Apply this change? / Do you want to proceed? / 沙箱扩权 To run [Shell] …",
    ),
    D::new(
        "attention:gemini:usage-limit",
        "usage limit reached",
        "\u{2502} Usage limit reached for gemini-2.5-pro.",
        "0.65.0 bundle：`Usage limit reached`；高负载框 `We are currently experiencing high demand for ${model}.` 同规则",
    ),
    D::new(
        "attention:gemini:question",
        "answer questions",
        "\u{2502} Answer Questions",
        "0.65.0 bundle：ask_user 提问表单表头 `Answer Questions`",
    ),
    D::new(
        "screen:spinner_line_working",
        "(esc to cancel",
        "\u{280B} Thinking about the plan (esc to cancel, 3s)",
        "0.65.0 bundle：运行中 spinner 行尾 `(esc to cancel`",
    ),
    D::new(
        "screen:apply_or_allow_change",
        "allow once",
        "\u{2502} \u{25CF} 1. Yes, allow once",
        "0.65.0 bundle：确认框首个选项 `Yes, allow once`；编号与圆点是运行时画的",
    ),
    D::new(
        "screen:ask_user_dialog",
        "answer questions",
        "\u{2502} Answer Questions",
        "0.65.0 bundle：表头 `Answer Questions`，底部提示 `Enter to select` 各段以 ` \\xB7 ` 连接，整串搜不到原样",
    ),
    // ── 自动抽取的字面量 ──
    D::new(
        "literal:do you trust the contents of this directory",
        "",
        "Do you trust the contents of this directory?",
        "通用信任表（chat_ui.rs）里的句子，给别的 CLI 用；gemini 0.65.0 的信任页是 `Do you trust the files in this folder`（能搜到），这句 0 处",
    ),
    D::new(
        "literal:│ apply this change",
        "apply this change",
        "\u{2502} Apply this change?",
        FRAMED,
    ),
    D::new(
        "literal:│ allow execution",
        "allow execution",
        "\u{2502} Allow execution of ls?",
        FRAMED,
    ),
    D::new(
        "literal:│ do you want to proceed",
        "do you want to proceed",
        "\u{2502} Do you want to proceed?",
        FRAMED,
    ),
    D::record(
        "record:Request cancelled.",
        "Request cancelled.",
        r#"{"id":"0f6f6a2e","timestamp":"2026-07-20T06:10:40.000Z","type":"info","content":"Request cancelled."}"#,
        "0.6x 会话日志实样（用户按 Esc 后的 info 记录，对话页据此画中断分隔条）；0.68.0 bundle 里 18 处",
    ),
];
