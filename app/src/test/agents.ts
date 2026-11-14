import type { AgentDescriptor, ChatUi, LaunchChoice, LaunchOption, SlashCommand } from "../api";

/** 与 plugins/claude 的 status_line_pattern 同一串（改一边必须同步另一边）。 */
export const CLAUDE_STATUS_LINE_PATTERN = String.raw`^\s*[✻✽✶✳✢·*✺✷✸✹]\s+([A-Z][A-Za-z'-]*(?: [A-Za-z'-]+){0,2}…)\s*(?:\((\d[^()]{0,160})\))?\s*$`;

/** 与 plugins/claude 的 retry_line_pattern 同一串（改一边必须同步另一边）。 */
export const CLAUDE_RETRY_LINE_PATTERN = String.raw`· (?:Retrying in|retrying, waiting up to|next try in) [^·]{1,60}· attempt (\d+)/(\d+)(?: · [^·]{0,40})?\s*$`;

/** 测试里已知的五家产品名。真实值由后端 `list_agents()` 下发（须与注册表的 display_name 同值）。 */
const NAMES: Record<string, string> = {
  claude: "Claude Code",
  codex: "Codex",
  kimi: "Kimi Code",
  gemini: "Gemini CLI",
  opencode: "OpenCode",
};
const RELAYS: Record<string, AgentDescriptor["relay"]> = {
  claude: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }, { value: "api_key", label: "API Key (x-api-key)" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["claude-sonnet-5"] }], env_options: [] },
  codex: { protocols: [], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "", default_auth: "bearer", suggestions: [{ protocol: "", models: ["gpt-5.4"] }], env_options: [] },
  kimi: { protocols: [{ value: "kimi", label: "Kimi" }, { value: "anthropic", label: "Anthropic Messages" }, { value: "openai", label: "OpenAI Chat Completions" }], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "kimi", default_auth: "bearer", suggestions: [{ protocol: "kimi", models: ["kimi-for-coding"] }], env_options: [] },
  // gemini：只有一种协议（讲 Gemini 自己的 generateContent，故 protocols 空）+ API Key 认证。
  gemini: { protocols: [], auth_modes: [{ value: "api_key", label: "API Key" }], default_protocol: "", default_auth: "api_key", suggestions: [{ protocol: "", models: ["gemini-2.5-pro"] }], env_options: [] },
  // opencode：中转＝往它配置里注入自定义 provider（OpenAI 兼容 / Anthropic）。
  opencode: { protocols: [{ value: "openai", label: "OpenAI 兼容" }, { value: "anthropic", label: "Anthropic Messages" }], auth_modes: [{ value: "bearer", label: "Bearer Token" }], default_protocol: "openai", default_auth: "bearer", suggestions: [{ protocol: "openai", models: ["gpt-5.4"] }], env_options: [] },
};

/**
 * 能被套上代理的那些（＝插件声明了 ProxySpec）。当前**五家都有**（gemini/opencode 也认标准代理
 * 环境变量，实测其 bundle/二进制）。
 * 与后端 `AgentPlugin::proxy()` 的能力矩阵同源——那边加了代理能力，这里也要跟上。
 */
const SUPPORTS_PROXY = new Set(["claude", "codex", "kimi", "gemini", "opencode"]);

/**
 * 代理能写进自己配置文件（＝不管由谁启动都生效）的那些。与后端 `ProxySpec.config_env`
 * 同源——**仅 claude**：codex/kimi/gemini 只认进程环境变量，故只覆盖 Meowo 拉起的会话。
 */
const PROXY_COVERS_ALL_LAUNCHES = new Set(["claude"]);

/**
 * 支持 SOCKS 代理的那些。与后端 `ProxySpec.socks` 同源，逐个核对过插件声明：
 * claude 官方明确不支持、codex 未编译 reqwest 的 socks feature、opencode 亦为 false，
 * 只有 kimi 与 gemini 认 SOCKS。
 */
const PROXY_ACCEPTS_SOCKS = new Set(["kimi", "gemini"]);

/**
 * 有账号概念的那些（＝插件声明了 account 能力槽）。当前**五家都有**，故这里全收。
 *
 * 它仍然是个可选能力槽：为 false 的 agent 不显示登录态、也不给登录入口（它的 `login_argv()`
 * 会是 None，按钮点下去只会报「拉起登录失败」）。要测那条路径，在用例里手写一个
 * `supports_account: false` 的 descriptor——别改这里，这里必须与后端的真实矩阵同源。
 */
const SUPPORTS_ACCOUNT = new Set(["claude", "codex", "kimi", "gemini", "opencode"]);

/**
 * 能用 API Key 登录的那些（＝插件声明了 ApiKeyLoginCap）。**只有 gemini**：它的 OAuth 被官方
 * 停用（个人版 Code Assist 关闸），key 是唯一活路，且 CLI 没有输入 key 的登录子命令。
 * 与后端 `AgentPlugin::api_key_login()` 同源。
 */
const SUPPORTS_API_KEY_LOGIN = new Set(["gemini"]);

/**
 * 能有多个账号的那些（＝插件声明了 ProfileSpec）。**gemini 不行**：它的数据目录无法被环境变量
 * 覆盖（`GEMINI_DIR` 实测无效），谎称支持会让两个「账号」静默共用同一份凭据。
 */
const SUPPORTS_PROFILES = new Set(["claude", "codex", "kimi", "opencode"]);

/**
 * meowo 能显示上下文占用的那些。**gemini/opencode 不行**：gemini 的 hook 不带 token，opencode
 * 没声明 telemetry（token 在它自己库里）。与后端 `AgentPlugin::provides_context()` 同源。
 */
const SUPPORTS_CONTEXT = new Set(["claude", "codex", "kimi"]);

/**
 * 会话历史可导出交接（跨 provider 切换的**来源**门槛）的那些。与后端
 * `telemetry().transcript().supports_chat()` 同源——gemini/opencode 无结构化
 * transcript，只能当切换目标，不能当来源。
 */
const SUPPORTS_CHAT_EXPORT = new Set(["claude", "codex", "kimi"]);

/**
 * 切账号时会话能跟着搬到新账号的那些。与后端 `cross_account_session()` 同源——
 * opencode 的会话存储没取证过，gemini 压根不支持多账号。
 */
const MOVES_SESSIONS_ACROSS_ACCOUNTS = new Set(["claude", "codex", "kimi"]);

/**
 * 对话页内置 `/` 补全候选。与后端 `AgentPlugin::slash_commands()` 同源——各家命令表是插件
 * 声明的事实（gemini 是 `/stats` 不是 `/status`，opencode 是 `/models` 不是 `/model`）。
 */
const SLASH_COMMANDS: Record<string, string[]> = {
  claude: ["/clear", "/compact", "/config", "/cost", "/help", "/init", "/mcp", "/memory", "/model", "/resume", "/review", "/status"],
  codex: ["/clear", "/compact", "/diff", "/help", "/model", "/new", "/review", "/status"],
  kimi: ["/clear", "/compact", "/help", "/model", "/status"],
  gemini: ["/chat", "/clear", "/compress", "/help", "/mcp", "/memory", "/stats", "/tools"],
  opencode: ["/compact", "/exit", "/help", "/init", "/models", "/new", "/undo"],
};

/**
 * 快速切模型预设。**只有 claude**：它的 `/model` 接受内联参数；其余四家是交互式菜单，
 * 声明了预设等于给出一个点了没反应的菜单。与后端 `AgentPlugin::model_presets()` 同源。
 */
const MODEL_PRESETS: Record<string, ChatUi["model_presets"]> = {
  claude: [
    { id: "fable", label: "Fable" },
    { id: "opus", label: "Opus" },
    { id: "sonnet", label: "Sonnet" },
    { id: "haiku", label: "Haiku" },
    { id: "opusplan", label: "Opus Plan" },
  ],
};

/**
 * 造一份 `agent_chat_ui()` 的返回：该 agent 的内置命令表 + 模型预设（无自定义命令）。
 * 要测自定义命令，在用例里 `custom` 传发现出来的条目。未知 provider 回 null——与后端同语义。
 */
/**
 * 整句可操作提示的识别规格。与后端各插件的 `attention_patterns()` 声明同源——
 * 那边改了正则，这里也要跟上，否则前端测试测的是一套过期规则。
 * 未取证的 provider 为空（不做整句识别）。
 */
const ATTENTION_PATTERNS: Record<string, NonNullable<ChatUi["attention_patterns"]>> = {
  claude: [
    {
      id: "claude:long-session-resume",
      patterns: [
        "this session is[^\\n]{0,120}\\bold and[^\\n]{0,80}\\btokens\\b",
        "resuming the full session will consume a substantial portion of your usage limits",
      ],
      last: false,
      details: "none",
      kind: "choice",
      confirms: null,
    },
    {
      id: "claude:command-approval",
      patterns: ["this command requires approval", "do you want to proceed\\?"],
      last: true,
      details: "proceed_box",
      kind: "approval",
      confirms: null,
    },
    {
      id: "claude:plan-approval",
      patterns: ["written\\s+up\\s+a\\s+plan\\s+and\\s+is\\s+ready\\s+to\\s+execute"],
      last: true,
      details: "none",
      kind: "plan",
      confirms: null,
    },
  ],
  kimi: [
    {
      id: "kimi:question",
      patterns: ["↑↓ select\\s+(?:1-\\d+ / ↵ (?:choose|toggle)|1/2 choose\\s+↵ confirm)\\s+←/→/tab switch"],
      last: true,
      details: "bracket_form",
      kind: "question",
      confirms: null,
    },
    {
      id: "kimi:command-approval",
      patterns: [
        "↑/↓ select · [\\d/]+ choose · ↵ confirm",
        "^\\s*▶\\s*(?:run this command|write this file|apply these edits|stop this task|ready to build with this plan|approve [^\\n?]+)\\?",
      ],
      last: true,
      details: "arrow_panel",
      kind: "approval",
      confirms: null,
    },
  ],
  gemini: [
    {
      id: "gemini:command-approval",
      patterns: [
        "^\\s*│\\s*allow execution of [^\\n]*\\?",
        "^\\s*│\\s*apply this change\\?",
        "^\\s*│\\s*do you want to proceed\\?",
        "^\\s*│\\s*to run \\[[^\\]\\n]+\\], allow access to the following\\?",
      ],
      last: true,
      details: "framed_box",
      kind: "approval",
      confirms: null,
    },
    {
      id: "gemini:usage-limit",
      patterns: ["^\\s*│\\s*usage limit reached\\b", "^\\s*│\\s*we are currently experiencing high demand\\b"],
      last: true,
      details: "none",
      kind: "choice",
      confirms: null,
    },
    { id: "gemini:question", patterns: ["^\\s*│\\s*answer questions\\b"], last: true, details: "framed_form", kind: "question", confirms: null },
  ],
  opencode: [
    { id: "opencode:command-approval", patterns: ["△ permission required"], last: true, details: "option_row", kind: "approval", confirms: null },
    { id: "opencode:always-allow-confirm", patterns: ["△ always allow"], last: true, details: "option_row", kind: "choice", confirms: "opencode:command-approval" },
  ],
};

/**
 * 识别文法固件：带上该 provider 由插件声明的整句规则。
 *
 * 改造前测试可以只写 `{ provider: "kimi", selectorAnchors: [] }`，因为识别层按
 * `provider ===` 自带规则；现在规则由后端下发，**不传就等于该 agent 没声明规则**。
 * 用这个构造器而不是手写字面量，测试才与生产路径（chatUi 下发）走同一份数据。
 */
/** claude 插件 subagent_view() 的同值镜像(见 plugins/claude/mod.rs)。 */
export const SUBAGENT_VIEW: NonNullable<ChatUi["subagent_view"]> = {
  placeholder_prefix: "Message @",
  panel_main_label: "main",
  panel_pointer: "❯",
  panel_viewed_marker: "●",
  down_input: "\u001b[B",
  up_input: "\u001b[A",
  open_input: "\r",
  leave_panel_input: "\u001b",
};

/** claude 插件 effort_control().reject_pattern 的同值镜像（2.1.322 取证，见 plugins/claude/mod.rs）。 */
export const CLAUDE_EFFORT_REJECT_PATTERN = String.raw`effort '[^']+' exceeds the cap(?:[^;]*; set to '(\w+)' instead)?|effort '[^']+' isn't|invalid argument: |failed to set effort level: |not applied: claude_code_effort_level=|claude_code_effort_level=\S+ overrides this session|is session-scoped and won't reach the remote process`;
/** claude 插件 model_select_reply().reject_pattern 的同值镜像。 */
export const CLAUDE_MODEL_REJECT_PATTERN = String.raw`^\s*(?:⎿\s*)?(?:kept model as |model switch cancelled|model switch (?:to .+ was )?blocked by a premodelswitch hook|model switch failed: |model '[^']+' is (?:restricted|not available|not currently available)|failed to validate model: |couldn't confirm model '|1m context is turned off|\S.* doesn['’]t have a 1m context window|'[^']+' isn['’]t available: the |(?:opus|sonnet) with 1m context is not available|\S.* isn['’]t available for your account yet)`;

export function grammar(
  provider: string,
  selectorAnchors: { marker: string; kind: "input" | "chat" }[] = [],
): { provider: string; selectorAnchors: typeof selectorAnchors; attentionPatterns: typeof ATTENTION_PATTERNS[string] } {
  return { provider, selectorAnchors, attentionPatterns: ATTENTION_PATTERNS[provider] ?? [] };
}

// 与四家已实拍版本的 ComposerGuard 同值；未声明不表示终端可以直接写入。
const COMPOSER_VERSIONS: Record<string, string> = {
  "codex": "0.167.2",
  "kimi": "2.5.3",
  "gemini": "0.65.0",
  "opencode": "1.19.3"
};
const COMPOSER_GUARDS: Record<string, NonNullable<ChatUi["composer_guard"]>> = {
  "codex": {
    "prompt_footer": true,
    "input_start_pattern": "^(› )",
    "top_pattern": "^› ",
    "bottom_pattern": "^  [A-Za-z0-9][^\\r\\n]* · .+$",
    "row_pattern": "^(?:› |  )(.*)$",
    "prompt_prefix": "",
    "tail_rows": 0,
    "tail_pattern": null,
    "empty_pattern": "^(?:Ask Codex to do anything)?$"
  },
  "kimi": {
    "prompt_footer": false,
    "input_start_pattern": "^(\\s*│ > )",
    "top_pattern": "^\\s*╭─+╮\\s*$",
    "bottom_pattern": "^\\s*╰─+╯\\s*$",
    "row_pattern": "^\\s*│(.*)│\\s*$",
    "prompt_prefix": ">",
    "tail_rows": 0,
    "tail_pattern": null,
    "empty_pattern": "^$"
  },
  "gemini": {
    "prompt_footer": false,
    "input_start_pattern": "^(\\s*[>*] )",
    "top_pattern": "^(?:─+|▄+)\\s*$",
    "bottom_pattern": "^(?:─+|▀+)\\s*$",
    "row_pattern": "^ (.*)$",
    "prompt_prefix": ">|*",
    "tail_rows": 0,
    "tail_pattern": null,
    "empty_pattern": "^(?:Type your message or @path/to/file)?$"
  },
  "opencode": {
    "prompt_footer": false,
    "input_start_pattern": "^(\\s*┃  |\\s*┃$)",
    "top_pattern": "^\\s*┃\\s*$",
    "bottom_pattern": "^\\s*╹▀+\\s*$",
    "row_pattern": "^\\s*┃(.*)$",
    "prompt_prefix": "",
    "tail_rows": 2,
    "tail_pattern": "^\\s*┃\\s+\\S.* · .+$",
    "empty_pattern": "^(?:Ask anything…(?: \"[^\"\\r\\n]*\")?)?$"
  }
};

export function chatUi(provider: string, custom: SlashCommand[] = []): ChatUi | null {
  const builtins = SLASH_COMMANDS[provider];
  if (!builtins) return null;
  const commands: SlashCommand[] = [
    ...builtins
      .filter((name) => !custom.some((c) => c.name === name))
      .map((name) => ({ name, description: null, source: "builtin" as const })),
    ...custom,
  ].sort((a, b) => a.name.localeCompare(b.name));
  return {
    slash_commands: commands,
    attention_patterns: ATTENTION_PATTERNS[provider] ?? [],
    model_presets: MODEL_PRESETS[provider] ?? [],
    model_select_command: provider === "claude" ? "/model" : null,
    // 与后端同源:claude 插件 model_select_reply(2.1.322 包内模板取证)。
    model_select_reply: provider === "claude" ? { confirm_pattern: "set model to ", reject_pattern: CLAUDE_MODEL_REJECT_PATTERN } : null,
    queue_input: provider === "codex" ? "\t" : "\r",
    // 与后端插件声明同源：claude 边干边读，kimi 未取证，其余排到回合结束。
    queue_delivery: provider === "claude" ? "next_step" : provider === "kimi" ? null : "turn_end",
    // 只有 claude 的 `/model` 接受内联参数；其余几家是交互式菜单，靠这条命令打开。
    // opencode 的命令是 /models(复数)——与后端插件声明同源。
    model_menu_command: (MODEL_PRESETS[provider] ?? []).length > 0 ? null : provider === "opencode" ? "/models" : "/model",
    // 切模型机制与后端插件声明同源（任务 20261114-tui-model-switch）。
    model_switch: provider === "codex" ? "server_settings" : provider === "kimi" || provider === "gemini" ? "restart_resume" : provider === "opencode" ? "tui_dialog" : null,
    awaiting_first_message_markers: provider === "kimi" ? ["No session yet — one will be created on your first message."] : [],
    mode_controls: provider === "claude"
      ? [{
        dimension: "permission", menu_command: null, cycle_input: "\u001b[Z", options: [],
        // 与后端 claude 插件同源：官方文档承诺的状态栏指示文本 → transcript 的 permissionMode 值。
        screen_markers: [
          { marker: "bypass permissions on", value: "bypassPermissions" },
          { marker: "accept edits on", value: "acceptEdits" },
          { marker: "plan mode on", value: "plan" },
          { marker: "auto mode on", value: "auto" },
          { marker: "don't ask on", value: "dontAsk" },
          { marker: "manual mode on", value: "default" },
        ],
      }]
      : provider === "codex"
        ? [{ dimension: "collaboration", menu_command: null, cycle_input: "\u001b[Z", options: [], screen_markers: [] }]
        : provider === "opencode"
          ? [{
            dimension: "agent", menu_command: null, cycle_input: "\t", options: [],
            screen_markers: [{ marker: "build · ", value: "build" }, { marker: "plan · ", value: "plan" }],
          }]
          // 与后端 kimi 插件（modern 变体）同源：plan 用 /plan on|off 直达，权限档开 /permission 菜单。
          : provider === "kimi"
            ? [
              {
                dimension: "work", menu_command: null, cycle_input: "\u001b[Z", screen_markers: [],
                options: [
                  { value: "default", inputs: [{ data: "/plan off", submit: true }] },
                  { value: "plan", inputs: [{ data: "/plan on", submit: true }] },
                ],
              },
              { dimension: "permission", menu_command: "/permission", cycle_input: null, options: [], screen_markers: [] },
            ]
            : [],
    startup_attention_markers: provider === "claude"
      ? ["do you trust the files in this folder", "do you trust the contents of this directory", "trust this folder", "workspace not trusted", "workspace trust dialog"]
      : provider === "codex"
        // 与后端 codex 插件同源：0.166.0 标题改成 "Trust this folder? ..."（research-cli-drift X1）。
        ? ["do you trust the files in this folder", "do you trust the contents of this directory", "trust this folder?"]
        : ["do you trust the files in this folder", "do you trust the contents of this directory"],
    // 与后端总装同源：claude 声明的五个 + 交互式 /model 型 CLI 并入的 model_menu_command。
    menu_slash_commands: provider === "claude"
      ? ["/config", "/mcp", "/memory", "/model", "/resume"]
      : (MODEL_PRESETS[provider] ?? []).length > 0 ? [] : [provider === "opencode" ? "/models" : "/model"],
    // 与后端同源:选择器锚点是插件声明的识别文法,当前只有 claude 有真机取证。
    selector_anchors: provider === "claude"
      ? [{ marker: "type something", kind: "input" as const }, { marker: "chat about this", kind: "chat" as const }]
      : [],
    // 与后端同源:五家都用 Esc 中断(claude/codex 确证,其余约定推断,见插件注释)。
    interrupt_input: "\x1b",
    // 与后端同源:opencode 首按 Esc 只提示 "esc again to interrupt",须分两次写。
    interrupt_presses: provider === "opencode" ? 2 : 1,
    // 与后端同源:换行注入序列 claude 是 ESC+CR,其余四家是 ctrl+j(裸 LF),见各插件的取证注释。
    newline_input: provider === "claude" ? "\u001b\r" : "\n",
    runtime_commands_pending: false,
    // 与后端同源:claude/gemini(提交时原生附加)与 kimi/codex/opencode(@ 是文本级惯例,
    // 模型自行读取)声明 true。测试桩模拟「已探测到版本」的常态,故不做 version 门控。
    attachment_mention: true,
    // 与后端同源:claude/kimi/codex/opencode 的 TUI 实测 Ctrl-V(kimi 是 Alt+V)读剪贴板原生附加
    // 图片,占位符各家不同;gemini 会落盘到用户工作目录,刻意不声明。
    clipboard_image_paste: provider === "claude" || provider === "codex" ? "\\[Image ?#\\d"
      : provider === "kimi" ? "\\[image[:# ]"
      : provider === "opencode" ? "\\[Image \\d" : null,
    // 与后端同源:粘贴键 kimi 在 Windows 上是 Alt+V(二进制里 win32 ? "alt+v" : ctrl("v")),
    // 其余声明该能力的是 Ctrl-V。
    clipboard_paste_input: provider === "kimi" ? "\u001bv" : provider === "gemini" ? null : "\u0016",
    // 与后端同源:运行中状态行正则只有 claude 声明(plugins/claude status_line_pattern)。
    status_line_pattern: provider === "claude" ? CLAUDE_STATUS_LINE_PATTERN : null,
    retry_line_pattern: provider === "claude" ? CLAUDE_RETRY_LINE_PATTERN : null,
    // 与后端同源:思考程度切换 claude(2.1.314)与 kimi(2.4.0)都是 /effort <档位>。
    effort: provider === "claude"
      ? { command: "/effort", levels: ["low", "medium", "high", "xhigh", "max"], confirm_pattern: String.raw`set effort level to (\w+)`, reject_pattern: CLAUDE_EFFORT_REJECT_PATTERN }
      // kimi 档位随模型（模型目录的 support_efforts），静态表留空。
      : provider === "kimi"
        ? { command: "/effort", levels: [], confirm_pattern: String.raw`(?:thinking set to|with thinking) (\w+)`, reject_pattern: String.raw`unsupported thinking effort|cannot switch models while streaming|failed to switch model` }
        : null,
    // 与后端同源:原生分叉 claude/codex/opencode(启动参数)、kimi(fork 子命令后 -r 打开)。
    fork: ["claude", "codex", "opencode", "kimi"].includes(provider),
    // 与后端同源:claude `/btw` 侧问(2.1.314 探针实测)。
    side_question: provider === "claude"
      ? { command: "/btw", footer_marker: "esc to close", ready_marker: "c to copy", scroll_input: "\x1b[B", close_input: "\x1b" }
      : null,
    // 与后端同源:裸可执行名 + 各家 resume_args。
    resume_command: ({ claude: ["claude", "--resume"], codex: ["codex", "resume"], kimi: ["kimi", "-r"], opencode: ["opencode", "--session"], gemini: ["gemini", "--resume"] } as Record<string, string[]>)[provider] ?? [],
    // 与后端同源:kimi `/undo N`(2.4.0 包内取证);opencode `/undo` 一次一轮、还原文件、
    // 原文放回它的输入框(1.19.3 probe_opencode_undo);claude `/rewind` 菜单(2.1.314 probe_claude_rewind)。
    undo: provider === "kimi"
      ? { command: "/undo", count_arg: true, restores_files: false, reject_pattern: String.raw`cannot undo while streaming|cannot undo \d+ prompts?;|nothing to undo|failed to undo|run /undo again|usage: /undo|no active session`, input_clear: null, menu: null }
      : provider === "opencode"
        ? { command: "/undo", count_arg: false, restores_files: true, reject_pattern: null, input_clear: { inputs: ["\u0003"], composer: { kind: "block", row_prefix: "┃", bottom_prefix: "╹" } }, menu: null }
        : provider === "claude"
          ? { command: "/rewind", count_arg: false, restores_files: false, reject_pattern: null, input_clear: { inputs: ["\u001b", "\u001b"], composer: { kind: "prompt", prompt: "❯", border: "─" } }, menu: { title: "Rewind", ready_marker: "❯ (current)", step_input: "\u001b[A", selected_prefix: "❯", enter_input: "\r", quote_prefix: "│", option_label: "Restore conversation", cancel_input: "\u001b" } }
          : null,
    // 与后端同源:草稿暂存键只有 claude 经真机取证(issue #72)。
    draft_stash: provider === "claude" ? { input: "\u0013", composer_prompt: "❯", composer_border: "─", stashed_marker: "› stashed" } : null,
    // 与后端同源:子代理视图只有 claude 有(2.1.322 真机取证)。
    subagent_view: provider === "claude" ? SUBAGENT_VIEW : null,
    // 与后端同源:仅 claude 取证过「TUI 权限框与 PermissionRequest hook 并行竞速」。
    permission_prompt_races_hook: provider === "claude",
    composer_guard: COMPOSER_GUARDS[provider],
    version: COMPOSER_VERSIONS[provider] ?? null,
  };
}

/**
 * 启动选项。与后端 `AgentPlugin::launch_options()` 同源：claude 有模型+权限两栏，
 * codex/gemini 各一栏审批，kimi/opencode 未调研到稳定 flag、如实不声明。
 */
type Traits = NonNullable<LaunchChoice["traits"]>;
const tr = (edits: Traits["edits"], commands: Traits["commands"], plan_only = false): Traits => ({ edits, commands, plan_only });
/** 夹具档位：不写的新字段（统一说法、说明键、服务端片段）按「无」补齐。 */
const ch = (id: string, label: string, args: string[], risk = false, extra: Partial<LaunchChoice> = {}): LaunchChoice =>
  ({ id, label, args, risk, traits: null, desc: null, server: null, ...extra });
const opt = (id: string, def: string, choices: LaunchChoice[], extra: Partial<LaunchOption> = {}): LaunchOption =>
  ({ id, default: def, choices, switch: "launch_only", resume_upgrade_only: false, in_session_only: false, ...extra });
const LAUNCH_OPTIONS: Record<string, LaunchOption[]> = {
  claude: [
    opt("model", "default", [
      ch("default", "Default", []),
      ch("fable", "Fable", ["--model", "fable"]),
      ch("opus", "Opus", ["--model", "opus"]),
      ch("sonnet", "Sonnet", ["--model", "sonnet"]),
      ch("haiku", "Haiku", ["--model", "haiku"]),
      ch("opusplan", "Opus Plan", ["--model", "opusplan"]),
    ]),
    opt("permission", "default", [
      ch("default", "Default", []),
      ch("plan", "Plan", ["--permission-mode", "plan"], false, { traits: tr("blocked", "blocked", true), desc: "claude.permission.plan" }),
      ch("acceptEdits", "Accept Edits", ["--permission-mode", "acceptEdits"], false, { traits: tr("never", "always"), desc: "claude.permission.acceptEdits" }),
      ch("bypassPermissions", "Bypass Permissions", ["--permission-mode", "bypassPermissions"], true, { traits: tr("never", "never"), desc: "claude.permission.bypassPermissions" }),
    ], { switch: "control" }),
  ],
  codex: [
    opt("approval", "default", [
      ch("default", "Default", [], false, { desc: "codex.approval.default" }),
      ch("readOnly", "Read Only", ["--sandbox", "read-only"], false, { traits: tr("always", "risky"), desc: "codex.approval.readOnly", server: '{"approval":"on-request","approvalsReviewer":"user","sandbox":"read-only"}' }),
      ch("ask", "Ask for approval", ["--sandbox", "workspace-write", "--ask-for-approval", "on-request"], false, { traits: tr("risky", "risky"), desc: "codex.approval.ask", server: '{"approval":"on-request","approvalsReviewer":"user","sandbox":"workspace-write"}' }),
      ch("approveForMe", "Approve for me", ["--sandbox", "workspace-write", "--ask-for-approval", "on-request", "-c", "approvals_reviewer=auto_review"], false, { traits: tr("risky", "risky"), desc: "codex.approval.approveForMe", server: '{"approval":"on-request","approvalsReviewer":"auto_review","sandbox":"workspace-write"}' }),
      ch("yolo", "Full Access", ["--sandbox", "danger-full-access", "--ask-for-approval", "never"], true, { traits: tr("never", "never"), desc: "codex.approval.yolo", server: '{"approval":"never","approvalsReviewer":"user","sandbox":"danger-full-access"}' }),
    ], { switch: "server" }),
    opt("collaboration", "default", [
      ch("default", "Default", [], false, { traits: tr("inherit", "inherit"), desc: "codex.collaboration.default", server: '{"collaboration":"default"}' }),
      ch("plan", "Plan", [], false, { traits: tr("blocked", "inherit", true), desc: "codex.collaboration.plan", server: '{"collaboration":"plan"}' }),
    ], { switch: "server", in_session_only: true }),
  ],
  gemini: [
    opt("approval", "default", [
      ch("default", "default", ["--approval-mode", "default"], false, { traits: tr("always", "always"), desc: "gemini.approval.default" }),
      ch("autoEdit", "auto_edit", ["--approval-mode", "auto_edit"], false, { traits: tr("never", "always"), desc: "gemini.approval.autoEdit" }),
      ch("plan", "plan", ["--approval-mode", "plan"], false, { traits: tr("blocked", "blocked", true), desc: "gemini.approval.plan" }),
      ch("yolo", "yolo", ["--yolo"], true, { traits: tr("never", "never"), desc: "gemini.approval.yolo" }),
    ], { switch: "restart" }),
  ],
  kimi: [
    opt("permission", "manual", [
      ch("manual", "Always Ask", [], false, { traits: tr("always", "always"), desc: "kimi.permission.manual" }),
      ch("yolo", "Ask When Needed", ["--yolo"], false, { traits: tr("risky", "risky"), desc: "kimi.permission.yolo" }),
      ch("auto", "Never Ask", ["--auto"], true, { traits: tr("never", "never"), desc: "kimi.permission.auto" }),
    ], { switch: "restart", resume_upgrade_only: true }),
    opt("work", "default", [
      ch("default", "Default", [], false, { desc: "kimi.work.default" }),
      ch("plan", "Plan", ["--plan"], false, { traits: tr("blocked", "inherit", true), desc: "kimi.work.plan" }),
    ], { switch: "control", resume_upgrade_only: true }),
  ],
  opencode: [
    opt("agent", "build", [
      ch("build", "build", [], false, { traits: tr("risky", "risky"), desc: "opencode.agent.build" }),
      ch("plan", "plan", ["--agent", "plan"], false, { traits: tr("blocked", "risky", true), desc: "opencode.agent.plan" }),
    ], { switch: "dialog" }),
  ],
};

/**
 * 造一份 `list_agents()` 的返回：列出全部已知 agent，其中 `installed` 里的标记为已装。
 *
 * 各测试此前各自 mock `availableAgents()` 的 id 数组；descriptor 多了展示名与安装态两个字段，
 * 抽到一处免得三个测试文件各写一遍。
 */
export function descriptors(installed: string[]): AgentDescriptor[] {
  return Object.keys(NAMES).map((id) => ({
    id,
    display_name: NAMES[id],
    installed: installed.includes(id),
    supports_proxy: SUPPORTS_PROXY.has(id),
    proxy_covers_all_launches: PROXY_COVERS_ALL_LAUNCHES.has(id),
    proxy_accepts_socks: PROXY_ACCEPTS_SOCKS.has(id),
    supports_account: SUPPORTS_ACCOUNT.has(id),
    supports_api_key_login: SUPPORTS_API_KEY_LOGIN.has(id),
    supports_profiles: SUPPORTS_PROFILES.has(id),
    supports_context: SUPPORTS_CONTEXT.has(id),
    supports_chat_export: SUPPORTS_CHAT_EXPORT.has(id),
    // 旧用例明确覆盖原生CLI路径；协议托管用例需单独开启能力并给真实结构化历史。
    // 与后端事实对齐:claude/codex/kimi 声明了 CrossAccountSession(会话能搬去另一个账号);
    // opencode 的会话存储没取证过,gemini 压根不支持多账号。
    moves_sessions_across_accounts: MOVES_SESSIONS_ACROSS_ACCOUNTS.has(id),
    // 与后端事实对齐:目前只有 claude 声明了附加目录 flag(--add-dir)。
    launch_options: LAUNCH_OPTIONS[id] ?? [],
    relay: RELAYS[id],
  }));
}
