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
    // 版本行 = clap 顶层命令名 + 空格 + 版本号，运行时拼，`codex-cli ` 带尾空格 0 处。单搜 `codex-cli` 的 19 处里
    // 只有 1 处是命令名，其余是 `codex-client\src\retry.rs` 这类包名路径的前缀，codex 改了命令名也照样「找到」
    //（做空第二轮 N2，2026-11-12）。改按近邻查：命令名紧跟在顶层命令 MultitoolCli 的 clap 常量后面，那串
    // 常量是「about + 版本号 0.166.0 + usage 模板 `codex [OPTIONS] [PROMPT]`」连着存的，usage 全包仅 1 处。
    // 样例第二行是同一个 clap 命令 `--help` 的 usage 行（模板原样，未实拍），只为让锚点出现在样例里。
    D::record_near(
        "record:codex-cli",
        &["codex-cli", "codex [options] [prompt]"],
        20000,
        "codex-cli 0.166.0\nUsage: codex [OPTIONS] [PROMPT]",
        "0.166.0 `codex --version` 首行原样（research-cli-drift X2）；codex.exe 里命令名 `codex-cli` 在 usage 模板后 10022 字节，其余 18 处离 usage 模板最近也有 29 万字节",
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
    // 记录查看页底栏：三段说明在 pager_overlay.rs 的常量里紧挨着存（`to scrollto pageto jump`），按三段
    // 近邻查。` to page` 单搜是泛词（`Failed to page read-only thread history`），永远「找到」（做空第一轮
    // #7）。规则本身要求每段连着按键字形（#9），样例按 codex.exe 的按键显示表拼出，非实拍。
    D::near(
        "screen:transcript_viewer",
        &["to scroll", "to page", "to jump"],
        48,
        "\u{2191}/\u{2193} to scroll   pgup/pgdn to page   home/end to jump",
        "0.166.0 codex.exe：三段紧挨 `tui\\src\\pager_overlay.rs`，相距 16 字节；按键名 pgup/pgdn 是内联常量",
    ),
    // 规则同时要求每段连着按键字形（做空第一轮 #9），字形也会漂移，三段说明之外再查按键名（做空第二轮 N6，
    // 2026-11-12）。字形来自 codex 的按键显示函数（tui key_hint）：pgup/pgdn 是它特判的内联常量（机器码里与
    // `ente`(r)、`spac`(e) 同一个函数，pgup→pgdn 相距 77 字节）；home/end 走 crossterm 的 KeyCode Display
    //（`…UpDownHomeEndPage UpPage Down…` 原样表）再转小写。方向键 ↑/↓ 是拆开的立即数（公共前缀 `e2 86`
    // + 末字节 91/93 分开写），程序文件里没有可搜的整串，不查。
    // 程序文件另有 `pgup / pgdn`（带空格，机器码里 `pgup / p` + `pgdn` 两条立即数拼出，11 字节），看不出属于哪个
    // 界面；记录查看页三段说明后紧挨的是 `/` 常量，推断键间不带空格。规则正则 `\s*/\s*` 两种都认，测试两种都覆盖。
    D::near(
        "screen:transcript_viewer",
        &["pgup", "pgdn"],
        160,
        "\u{2191}/\u{2193} to scroll   pgup/pgdn to page   home/end to jump",
        "0.166.0 codex.exe：按键显示函数里 pgup→pgdn 相距 77 字节；另 3 处（按键名解析 12 字节、`pgup / pgdn` 14 字节、文本框帮助 `pgup/pgdn/⌃b` 5 字节）也在间距内",
    ),
    D::near(
        "screen:transcript_viewer",
        &["home", "end"],
        8,
        "\u{2191}/\u{2193} to scroll   pgup/pgdn to page   home/end to jump",
        "0.166.0 codex.exe：crossterm KeyCode 名表 `HomeEnd` 3 处（Display 与 Debug），home→end 相距 4 字节；文本框帮助 `home/end` 3 处相距 5 字节",
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
    // ── 对话页工具行与中断分隔依赖的 rollout 记录（2026-11-14，本机 0.167.2 的 68 份 rollout 与 codex.exe）──
    D::record(
        "record:turn_aborted",
        "turn_aborted",
        r#"{"timestamp":"2026-11-12T15:16:16.944Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"01a1d032","reason":"interrupted","duration_ms":1521570}}"#,
        "0.167.2 rollout 实样（用户中断处的分隔条，68 份里 64 次）；codex.exe 17 处",
    ),
    D::record(
        "record:Script running",
        "Script running",
        "Script running with cell ID g9:1\nWall time 0.0 seconds\nOutput:\n",
        "0.167.2 rollout 实样（exec 脚本让到后台时的回执头，结果之后由 wait 取）；codex.exe `Script running with cell ID` 2 处",
    ),
    D::record(
        "record:aborted by user",
        "aborted by user",
        "aborted by user after 20.9s",
        "0.167.2 rollout 实样（脚本被用户打断时的回执，工具行据此显示已中断）；codex.exe 4 处",
    ),
];
