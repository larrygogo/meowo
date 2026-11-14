//! claude 插件依赖的 CLI 文案中需要手工声明的部分（机制见 [`crate::cli_text`]）。
//!
//! 取证：Claude Code 2.1.322 本机程序文件 `~/.local/bin/claude.exe` 内嵌 JS 的模板字符串，
//! 以及 `~/.claude/projects` 下 2.1.322 transcript 的 `<local-command-stdout>` 实样，2026-11-12。
//! 只有模板没有实拍的，样例按模板填占位符（填的值写在出处里）。

use crate::cli_text::CliTextDep as D;

pub(super) static DEPS: &[D] = &[
    // ── /effort ──
    D::new(
        "effort.confirm",
        "Set effort level to ",
        "Set effort level to high (this session only): Comprehensive implementation with extensive testing",
        "2.1.322 模板 `Set effort level to ${vJ(o)}${A}: ${L}`，vJ 即 String(e)，小写档位名；冒号后的说明按模板填",
    ),
    D::new(
        "effort.reject",
        "Invalid argument: ",
        "Invalid argument: maxx. Valid options are: low, medium, high, xhigh, max, auto",
        "2.1.322 模板 `Invalid argument: ${e}. Valid options are: ${g(at())}`",
    ),
    D::new(
        "effort.reject",
        "Failed to set effort level: ",
        "Failed to set effort level: EPERM: operation not permitted, open 'settings.json'",
        "2.1.322 模板 `Failed to set effort level: ${S.message}`，message 按写设置失败填",
    ),
    D::new(
        "effort.reject",
        "Not applied: CLAUDE_CODE_EFFORT_LEVEL=",
        "Not applied: CLAUDE_CODE_EFFORT_LEVEL=high overrides effort this session, and max is session-only (nothing saved)",
        "2.1.322 模板 `Not applied: CLAUDE_CODE_EFFORT_LEVEL=${U} overrides effort this session, and ${vJ(o)} is session-only (nothing saved)`",
    ),
    D::new(
        "effort.reject",
        " overrides this session",
        "CLAUDE_CODE_EFFORT_LEVEL=high overrides this session \u{2014} clear it and max takes over",
        "2.1.322 模板 `CLAUDE_CODE_EFFORT_LEVEL=${U} overrides this session \\u2014 clear it and ${vJ(o)} takes over`（包内以 UTF-16 常量存放）",
    ),
    D::new(
        "effort.reject",
        " is session-scoped and won't reach the remote process",
        "max is session-scoped and won't reach the remote process. Use low, medium, high, or xhigh instead.",
        "2.1.322 模板 `${o} is session-scoped and won't reach the remote process. …`（远程会话）",
    ),
    D::new(
        "effort.reject",
        " exceeds the cap for ",
        "Effort 'max' exceeds the cap for claude-opus-5-5 set by your settings or organization; set to 'high' instead (this session only): Comprehensive implementation",
        "2.1.322 模板 `Effort '${e}' exceeds the cap for ${u} set by your settings or organization; set to '${o}' instead${A}: ${L}`；CLI 已改成 ${o}",
    ),
    D::new(
        "effort.reject",
        "",
        "Effort 'max' isn't supported by this model",
        "2.1.314 的旧拒绝句 `Effort '<x>' isn't …`（插件旧注释），2.1.322 全包已无；只存了前半句，不搜",
    )
    .legacy(),
    // ── /model ──
    D::new(
        "model.confirm",
        "Set model to ",
        "Set model to `Opus 5.5` and saved as your default for new sessions",
        "2.1.322 transcript <local-command-stdout> 实样 2026-11-12；模板 `var BTe=\"Set model to \"`",
    ),
    D::new(
        "model.reject",
        "Kept model as ",
        "  \u{23BF}  Kept model as Opus 5.5",
        "2.1.322 模板 `var ift=\"Kept model as \"` + `${ift}${Ew(uT(i))}`：额度/计费确认框被取消，模型没变",
    ),
    D::new(
        "model.reject",
        "Model switch cancelled",
        "Model switch cancelled",
        "2.1.322 `o(U(\"Model switch cancelled\"))`：PreModelSwitch hook 运行中按了 Esc",
    ),
    D::new(
        "model.reject",
        " was blocked by a PreModelSwitch hook",
        "Model switch to claude-sonnet-5 was blocked by a PreModelSwitch hook: not allowed in this repo",
        "2.1.322 模板 `Model switch to ${uT(e)} was blocked by a PreModelSwitch hook${o?`: ${jp(o)}`:\"\"}`",
    ),
    D::new(
        "model.reject",
        "Model switch blocked by a PreModelSwitch hook",
        "Model switch blocked by a PreModelSwitch hook: confirmation required, and this session cannot ask",
        "2.1.322 模板 `Model switch blocked by a PreModelSwitch hook${…}`",
    ),
    D::new(
        "model.reject",
        "Model switch failed: ",
        "Model switch failed: request timed out",
        "2.1.322 模板 `Model switch failed: ${jp(d(bo))}`",
    ),
    D::new(
        "model.reject",
        "' is restricted by your organization's settings.",
        "Model 'opus' is restricted by your organization's settings. Switch to a different model.",
        "2.1.322 别名校验 kje 模板 `Model '${Mf(n)}' is restricted by your organization's settings. …`",
    ),
    D::new(
        "model.reject",
        "' is not available. ",
        "Model 'opus' is not available. Your organization restricts model selection.",
        "2.1.322 kje 模板 `Model '${Mf(n)}' is not available. Your organization restricts model selection.`",
    ),
    D::new(
        "model.reject",
        "' is not currently available for your account. ",
        "Model 'fable' is not currently available for your account. Contact your admin.",
        "2.1.322 kje 模板 `Model '${Mf(n)}' is not currently available for your account. ${vvt(r.description)}`",
    ),
    D::new(
        "model.reject",
        "Failed to validate model: ",
        "Failed to validate model: fetch failed",
        "2.1.322 kje 模板 `Failed to validate model: ${vvt(d(r))}`",
    ),
    D::new(
        "model.reject",
        "Couldn't confirm model '",
        "Couldn't confirm model 'opus' is available (the check timed out) \u{b7} try again",
        "2.1.322 模板 `Couldn't confirm model '${Mf(a)}' is available (the check timed out) \\xB7 try again`",
    ),
    D::new(
        "model.reject",
        "1M context is turned off",
        "1M context is turned off (CLAUDE_CODE_DISABLE_1M_CONTEXT is set), so 'opus[1m]' isn\u{2019}t available. Switch to opus instead.",
        "2.1.322 kje 模板 `1M context is turned off (CLAUDE_CODE_DISABLE_1M_CONTEXT is set), so '${f}' isn\\u2019t available. …`",
    ),
    D::new(
        "model.reject",
        " have a 1M context window, so '",
        "Haiku 4.5 doesn\u{2019}t have a 1M context window, so 'haiku[1m]' isn\u{2019}t available. Switch to haiku to use its standard context window.",
        "2.1.322 kje 模板 `${jk(bs(r.carrier))} doesn\\u2019t have a 1M context window, so '${f}' isn\\u2019t available. …`",
    ),
    D::new(
        "model.reject",
        "' isn't available: the ",
        "'opusplan[1m]' isn't available: the opusplan setting switches models in plan mode and has no 1M form. Run /model opusplan, or /model sonnet[1m] for a 1M context window.",
        "2.1.322 kje 模板 `'${f}' isn't available: the ${u} setting switches models in plan mode and has no 1M form. …`",
    ),
    D::new(
        "model.reject",
        " with 1M context is not available for your account",
        "Opus with 1M context is not available for your account. Learn more: https://code.claude.com/docs/en/model-config#extended-context-with-1m",
        "2.1.322 kje 字面量",
    ),
    D::new(
        "model.reject",
        " isn't available for your account yet",
        "Fable 5.2 isn't available for your account yet. Switch to a different model.",
        "2.1.322 kje 模板 `${r.displayName} isn't available for your account yet. …`",
    ),
    // ── 屏幕行与整句提示 ──
    D::new(
        "status_line",
        "",
        "\u{273B} Composing\u{2026} (41s \u{b7} \u{2193} 427 tokens \u{b7} thought for 4s)",
        "2.1.303 实拍（插件注释）；动词随机、计时与 token 数运行时拼，程序文件里没有可搜的原样片段",
    ),
    D::new(
        "retry_line",
        "retrying, waiting up to",
        "No response from the API after 30s \u{b7} retrying, waiting up to 1m \u{b7} attempt 2/10",
        "2.1.312 包内渲染源码（插件注释）；2.1.322 包内 ` \\xB7 retrying, waiting up to` 仍在",
    ),
    D::new(
        "retry_line",
        "Retrying in ",
        "API Error: 529 Overloaded \u{b7} Retrying in 5s \u{b7} attempt 2/10",
        "2.1.312 包内渲染源码；2.1.322 包内 ` \\xB7 Retrying in` 仍在",
    ),
    D::new(
        "retry_line",
        "next try in ",
        "Auto mode check unavailable \u{b7} next try in 5s \u{b7} attempt 2/10 \u{b7} esc to interrupt",
        "2.1.312 包内渲染源码；2.1.322 包内 ` \\xB7 next try in` 仍在",
    ),
    D::new(
        "clipboard_image",
        "[Image #",
        "[Image #1]",
        "2.1.303 pywinpty 实测占位符（插件注释）；2.1.322 包内 `[Image #` 仍在",
    ),
    D::new(
        "attention:claude:long-session-resume",
        "This session is ",
        "This session is 3 days old and 412.5k tokens.",
        "2.1.322 模板 `This session is ${Ue} old and ${We} tokens.`",
    ),
    D::new(
        "attention:claude:long-session-resume",
        "Resuming the full session will consume a substantial portion of your usage limits",
        "Resuming the full session will consume a substantial portion of your usage limits.",
        "2.1.322 包内字面量",
    ),
    D::new(
        "attention:claude:command-approval",
        "This command requires approval",
        "This command requires approval",
        "2.1.322 包内字面量",
    ),
    D::new(
        "attention:claude:command-approval",
        "Do you want to proceed?",
        "Do you want to proceed?",
        "2.1.322 包内字面量",
    ),
    D::new(
        "attention:claude:plan-approval",
        "written up a plan and is ready to execute",
        "Claude has written up a plan and is ready to execute. Would you like to proceed?",
        "2.1.227 实拍（插件注释）；2.1.322 包内原句仍在",
    ),
    D::new(
        "screen:btw_overlay_working",
        "",
        "/btw 这个函数为什么要加锁",
        "行首是用户自己打的 /btw 回显；CLI 文案部分 esc to close 由侧问 footer_marker 自动检查",
    ),
    // 按键提示由 `e(y,{chord:"c",action:"copy"})` 逐个拼：查三个动作名在同一段渲染代码里（做空第一轮 #7）。
    D::near(
        "screen:btw_overlay_answered",
        &["\"copy\"", "\"fork\"", "\"close\""],
        400,
        "  c to copy \u{b7} f to fork \u{b7} Esc to close",
        "2.1.314 实拍（cc-btw-side-question 取证）；2.1.322 claude.exe 里 `action:\"copy\"/\"fork\"/\"close\"` 最近相距 142 字节",
    ),
    D::new(
        "screen:status_line_working",
        "",
        "\u{2722} Brewing\u{2026} (7s \u{b7} \u{2193} 129 tokens)",
        "2.1.233 实拍（screen.rs 注释）；动词与计时运行时拼，不搜",
    ),
    D::new(
        "screen:osc_progress_idle",
        "",
        "4;0",
        "OSC 9;4 进度序列，不是文案，不搜",
    ),
    // ── 自动抽取的字面量里，程序文件中没有原样的 ──
    D::new(
        "literal:do you trust the files in this folder",
        "do you trust the files in this folder",
        "Do you trust the files in this folder?",
        "旧版信任页标题；2.1.322 改为 `Quick safety check: Is this a project you trust?`，靠 trust this folder 命中",
    )
    .legacy(),
    D::new(
        "literal:do you trust the contents of this directory",
        "do you trust the contents of this directory",
        "Do you trust the contents of this directory?",
        "更早版本的信任页措辞，2.1.322 全包 0 处",
    )
    .legacy(),
    D::new(
        "literal:bypass permissions on",
        "bypass permissions",
        "\u{23F5}\u{23F5} bypass permissions on (shift+tab to cycle)",
        "2.1.322 模式表 `bypassPermissions:{indicator:\"bypass permissions\"}`，组件 CM 运行时拼 `indicator + \" on\"`",
    ),
    D::new(
        "literal:don't ask on",
        "don't ask",
        "\u{23F5}\u{23F5} don't ask on (shift+tab to cycle)",
        "2.1.322 模式表 `dontAsk:{indicator:\"don't ask\"}`，运行时拼 \" on\"",
    ),
    D::new(
        "literal:manual mode on",
        "manual mode",
        "manual mode on (shift+tab to cycle)",
        "2.1.322 模式表 `default:{indicator:\"manual mode\"}`，运行时拼 \" on\"",
    ),
    D::new(
        "literal:\u{276F} (current)",
        "(current)",
        "\u{276F} (current)",
        "2.1.322 `/rewind` 菜单 `yje=\"(current)\"`，指针 ❯ 另行渲染",
    ),
    D::new(
        "literal:\u{203A} stashed",
        " stashed",
        "\u{203A} stashed",
        "2.1.322 `[oe.pointerSmall,\" stashed\"]`，› 是 pointerSmall 符号",
    ),
    D::new(
        "literal:\u{2191}\u{2193} scroll",
        "scroll \u{b7} v to",
        "\u{2191}\u{2193} scroll \u{b7} v to open in editor \u{b7} ? for shortcuts",
        "2.1.322 transcript 查看器 `${oe.arrowUp}${oe.arrowDown} scroll \\xB7 v to ${xt} \\xB7 ? for shortcuts`",
    ),
    D::new(
        "literal:arrows to navigate",
        "arrows to navigate",
        "Enter to select \u{b7} arrows to navigate \u{b7} Esc to cancel",
        "旧版审批表单导航提示，2.1.322 用 Tab/Arrow keys to navigate",
    )
    .legacy(),
    D::new(
        "literal:\u{2191}\u{2193} to navigate",
        "\u{2191}\u{2193} to navigate",
        "Enter to select \u{b7} \u{2191}\u{2193} to navigate \u{b7} Esc to cancel",
        "旧版审批表单导航提示，2.1.322 用 Tab/Arrow keys to navigate",
    )
    .legacy(),
    D::new(
        "literal:enter to set as default",
        "set as default",
        "Enter to set as default \u{b7} s to use this session only \u{b7} Esc to cancel",
        "2.1.322 模型选择器 `Y({chord:\"enter\",action:N?\"set as default\":\"confirm\"})` 运行时拼，未实拍",
    ),
    D::near(
        "literal:tab to amend",
        &["\"tab\"", "\"amend\""],
        40,
        "Esc to cancel \u{b7} Tab to amend",
        "2.1.322 `e(y,{chord:\"tab\",action:\"amend\"})` 运行时拼；两段相距 13 字节（做空第一轮 #7）",
    ),
    D::new(
        "literal:ctrl+e to explain",
        "ctrl+e to explain",
        "Esc to cancel \u{b7} ctrl+e to explain",
        "2.1.322 包内没有 explain 的按键提示（无 chord:\"ctrl+e\"），疑似已移除；bash 审批另有 do you want to proceed? 兜底",
    )
    .legacy(),
    D::new(
        "literal:skip interview and plan immediately",
        "skip interview and plan immediately",
        "Skip interview and plan immediately",
        "2.1.322 全包 0 处，疑似已改名；所在规则 legacy_no_prompt_blocker 只是弱兜底",
    )
    .legacy(),
];
