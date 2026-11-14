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
    // ── 审批面板与提问表单选项的角色规则 ──
    D::new(
        "option:kimi:question#0",
        "",
        "Other",
        "2.6.1 kimi.exe：提问表单末尾恒有的自定义答案项（`DEFAULT_OTHER_LABEL = \"Other\"`）。选项只有一个通用词，搜程序文件证明不了什么",
    ),
    D::new(
        "option:kimi:command-approval#0",
        "Approve once",
        "Approve once",
        "2.6.1 kimi.exe `DEFAULT_APPROVAL_CHOICES` 与真机屏幕 2026-11-14；计划审阅的 `Approve` 同规则",
    ),
    D::new(
        "option:kimi:command-approval#1",
        "Approve for this session",
        "Approve for this session",
        "2.6.1 kimi.exe `DEFAULT_APPROVAL_CHOICES` 与真机屏幕 2026-11-14",
    ),
    D::new(
        "option:kimi:command-approval#2",
        "",
        "Reject",
        "2.6.1 真机屏幕 2026-11-14。选项只有一个通用词，搜程序文件证明不了什么；这一项在不在由同屏的问句规则盯着",
    ),
    D::new(
        "option:kimi:command-approval#3",
        "Reject with feedback",
        "Reject with feedback",
        "2.6.1 kimi.exe `DEFAULT_APPROVAL_CHOICES`（requires_feedback）与真机屏幕 2026-11-14；计划审阅的 `Revise` 同规则",
    ),
    D::new(
        "option-ready:kimi:command-approval#3",
        "Type feedback · ↵ submit.",
        "  Type feedback · ↵ submit.",
        "2.6.1 真机屏幕 2026-11-14（probe_deny_feedback：按 4 之后的页脚）；程序文件里是原句",
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
    D::new(
        "literal:do you trust the files in this folder",
        "",
        "Do you trust the files in this folder?",
        TRUST_TABLE,
    ),
    D::new(
        "literal:do you trust the contents of this directory",
        "",
        "Do you trust the contents of this directory?",
        TRUST_TABLE,
    ),
    // 提问提示行：`${numberHint} / ↵ ${question.multi_select ? "toggle" : "choose"}`，整串 0 处，按两段近邻查
    //（做空第一轮 #7，2026-11-12）。
    D::near(
        "literal:↵ choose",
        &[" / ↵ ", "\"choose\""],
        120,
        "1-3 / ↵ choose",
        "2.5.1 kimi.EXE：模板段 ` / ↵ ` 与 `\"choose\"` 相距 44 字节",
    ),
    D::near(
        "literal:↵ toggle",
        &[" / ↵ ", "\"toggle\""],
        120,
        "1-3 / ↵ toggle",
        "2.5.1 kimi.EXE：同 ↵ choose，`\"toggle\"` 相距 33 字节（多选题）",
    ),
    // 审批提示行 `↑/↓ select · ${buildNumericHint(n)} choose · ↵ confirm`：数字段运行时生成，后半段是模板原样。
    D::new(
        "literal:1/2/3/4 choose",
        " choose · ↵ confirm",
        "↑/↓ select · 1/2/3/4 choose · ↵ confirm",
        "2.5.1 kimi.EXE：模板静态段 ` choose · ↵ confirm` 1 处（数字段由 buildNumericHint 生成）",
    ),
    // ── 记录格式：wire.jsonl 事件名与字段（解析器认的字符串，做空第一轮 #7，2026-11-12）──
    D::record(
        "record:plan_mode.cancel",
        "plan_mode.cancel",
        r#"{"type":"plan_mode.cancel","time":1784863258011}"#,
        "2.5.1 wire.jsonl 实样（/plan off 与 Shift+Tab 关计划模式都写它，research-cli-drift K1）；kimi.EXE 6 处",
    ),
    D::record(
        "record:plan_mode.enter",
        "plan_mode.enter",
        r#"{"type":"plan_mode.enter","time":1784863250000}"#,
        "2.5.1 kimi.EXE 6 处（包内回放器 case）",
    ),
    D::record(
        "record:token_counting.rebased",
        "token_counting.rebased",
        r#"{"type":"token_counting.rebased","agentId":"main","length":12,"tokens":34584,"measured":false,"time":1792043997330}"#,
        "2.5.1 wire.jsonl 实样（压缩后 Context% 读它，research-cli-drift K4）；kimi.EXE 3 处",
    ),
    D::record(
        "record:usageScope",
        "usageScope",
        r#"{"type":"usage.record","agentId":"main","usage":{"inputOther":263123},"usageScope":"session","time":1792043997320}"#,
        "2.5.1 wire.jsonl 实样（压缩请求记 session 档，不能当上下文，research-cli-drift K4）；kimi.EXE 5 处",
    ),
    D::record(
        "record:full_compaction.complete",
        "full_compaction.complete",
        r#"{"type":"full_compaction.complete","agentId":"main","time":1792043997331}"#,
        "2.5.1 wire.jsonl 实样（已压缩分隔条）；kimi.EXE 2 处",
    ),
    D::record(
        "record:permission.set_mode",
        "permission.set_mode",
        r#"{"type":"permission.set_mode","mode":"yolo"}"#,
        "2.5.1 wire.jsonl 实样（权限档显示）；kimi.EXE 5 处",
    ),
    D::record(
        "record:kimi-file://",
        "kimi-file://",
        r#"{"type":"image_url","imageUrl":{"url":"kimi-file://f_0a1b"}}"#,
        "2.5.1 wire.jsonl 实样（粘贴图片的地址形态，research-cli-drift K3）；kimi.EXE 20 处",
    ),
    D::new(
        "literal:thinking...",
        "thinking...",
        "Thinking...",
        "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Thinking…`",
    )
    .legacy(),
    D::new(
        "literal:working...",
        "working...",
        "Working...",
        "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Working…`",
    )
    .legacy(),
    D::record(
        "record:turn.ended",
        "turn.ended",
        r#"{"type":"turn.ended","agentId":"main","turnId":1,"reason":"cancelled","durationMs":1433879,"time":1793173662714}"#,
        "2.6.1 wire.jsonl 实样（reason=cancelled 画中断分隔条、failed 出错误卡；本机 164 份主链里 cancelled 17 次）；kimi.exe 73 处",
    ),
    D::new(
        "literal:compacting...",
        "compacting...",
        "Compacting...",
        "2.5.1 之前的三个点写法；2.5.1 改用省略号字符 `Compacting…`",
    )
    .legacy(),
];
