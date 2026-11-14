//! opencode 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：opencode 1.19.3 本机程序文件 `%APPDATA%/npm/node_modules/opencode-ai/bin/opencode.exe`
//! 内嵌 JS 的字符串（按 UTF-8 小写搜），以及第三轮调研 research-cli-drift.md 的 O 系列结论，2026-11-12。
//! opencode 的 TUI 把按键和说明分成两个文本节点渲染（如 `K("esc ")` 后接 `"interrupt"`），
//! 屏幕上连在一起的「esc dismiss」「enter confirm」在程序文件里没有原样；这类按「分段都在且相距
//! 很近」查（[`D::near`]，做空第一轮 #7，2026-11-12），间距取 1.19.3 实测值的余量。

use crate::cli_text::CliTextDep as D;

pub(super) static DEPS: &[D] = &[
    // ── 输入框规格（composer）──
    D::new(
        "composer.empty",
        "Ask anything",
        "Ask anything… \"Fix a TODO in the codebase\"",
        "1.19.5 真机 2026-11-14（probe_tui_script cells）：空输入框的灰色占位，引号里的示例会轮换",
    ),
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
    D::new(
        "attention:opencode:always-allow-confirm",
        "always allow",
        "\u{25B3} Always allow",
        "1.19.5 opencode.exe：权限框第二段 `title:\"Always allow\"`，三角符号与文字分开渲染；真机屏幕见 probe_tui_script 2026-11-14",
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
    // 按键与说明分两个节点渲染：`r("esc "),…,z(n,r("dismiss"))`。间距是 1.19.3 opencode.exe 实测最近距离的余量。
    D::near("literal:esc dismiss", &["\"esc \"", "\"dismiss\""], 200, "esc dismiss", "1.19.3：`r(\"esc \")` 与 `r(\"dismiss\")` 相距 47 字节"),
    D::near("literal:enter confirm", &["\"enter \"", "\"confirm\""], 400, "enter confirm", "1.19.3：`r(\"enter \")` 与 `r(\"confirm\")` 相距 159 字节"),
    // 提问表单的按键说明先算好（`if(H())return"submit";if(b()?.multiple)return"toggle";…`）再交给
    // 底栏渲染，离 `"enter "` 约 4KB。
    D::near("literal:enter submit", &["\"enter \"", "\"submit\""], 6000, "enter submit", "1.19.3：说明在 `C=R(()=>{…return\"submit\"…})` 里算好，与 `r(\"enter \")` 相距 3939 字节"),
    D::near("literal:enter toggle", &["\"enter \"", "\"toggle\""], 6000, "enter toggle", "1.19.3：同 enter submit，`return\"toggle\"` 相距 3971 字节"),
    D::near("literal:↑↓ select", &["\"↑↓ \"", "\"select\""], 200, "↑↓ select", "1.19.3：`r(\"\\u2191\\u2193 \")` 与 `r(\"select\")` 相距 56 字节"),
    D::near("literal:⇆ tab", &["\"⇆ \"", "\"tab\""], 200, "⇆ tab", "1.19.3：`r(\"\\u21C6 \")` 与 `r(\"tab\")` 相距 49 字节"),
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
    // 运行中底栏：`DU=K("esc ")` 与 `kU.interrupt>0?"again to interrupt":"interrupt"` 之间隔着重试提示的
    // 渲染代码。research-cli-drift O1 的漂移正是这一条（1.19.2 的 `esc to interrupt` 改成两段）。
    D::near(
        "literal:esc interrupt",
        &["\"esc \"", "\"interrupt\""],
        2500,
        "esc interrupt",
        "1.19.3：`K(\"esc \")` 与 `\"interrupt\"` 相距 1677 字节（research-cli-drift O1）",
    ),
    // ── 对话页中断分隔与工具行依赖的库内记录（2026-11-14，本机 1.19.5 的 opencode.db 与 opencode.exe）──
    D::record(
        "record:MessageAbortedError",
        "MessageAbortedError",
        r#"{"error":{"data":{"message":"Aborted"},"name":"MessageAbortedError"},"key":"~error","kind":"error","msg":"msg_142cd36290011e33OiUHQ20U5k","time":1792122113434}"#,
        "1.19.x 库内实样经导出（用户中断，画中断分隔条而不是错误卡；本机 3 次）；opencode.exe 15 处",
    ),
    D::record(
        "record:Tool execution aborted",
        "Tool execution aborted",
        r#"{"kind":"part","part":{"callID":"toolu_vrtx_01EovD7P","state":{"error":"Tool execution aborted","input":{"command":"ping -n 30 127.0.0.1"},"status":"error"},"tool":"bash","type":"tool"}}"#,
        "1.19.x 库内实样经导出（用户中断时正在跑的工具以这句收场，工具行显示已中断而不是失败；本机 3 次）；opencode.exe 2 处",
    ),
];
