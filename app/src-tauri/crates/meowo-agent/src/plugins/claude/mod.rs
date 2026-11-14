//! claude（Anthropic Claude Code）插件。数据目录只有一种（`~/.claude`），特点有三：
//!
//! 1. hooks 条目**带 `matcher`**——同一事件下按 matcher 与用户自有 hook 共存
//!    （如 `PreToolUse:Bash` 预检 vs 我方的 `PreToolUse:AskUserQuestion`）。
//! 2. hooks 与 `statusLine` 同住 `settings.json`，故 statusLine 的包装脚本走写前改写——见 `setup.rs`。
//! 3. 凭据在 macOS 走登录 Keychain，其它平台走 `~/.claude/.credentials.json`——见 `account.rs`。
//!    哪一条由注入的 `KeychainPort` 在运行时判定，本层没有平台 `cfg`。
//!
//! 可执行的落法（顺序即优先级）：
//!
//! | 优先级 | 安装方式 | 落点 |
//! |---|---|---|
//! | 1 | 官方 native installer（`claude.ai/install.ps1\|sh`） | `~/.local/bin/claude[.exe]` |
//! | 2 | npm 全局（Windows） | `%APPDATA%/npm/node_modules/@anthropic-ai/claude-code/bin/claude.exe` |
//! | 3 | PATH 兜底 | 裸名 |
//!
//! 前两条**必须**直查绝对路径：安装脚本只改持久 PATH，运行中的 meowo-app 进程 PATH 是启动时的
//! 旧快照、看不到新目录——装完却「打不开 / 提示找不到文件」正是这么来的（codex 的 standalone
//! 候选同理）。npm 那条只对 Windows 有意义：npm 在 unix 生成的 shim 是无扩展名的 `claude`，
//! `OnPath` 就能命中；Windows 上生成的是 `claude.cmd`，`exe_on_path("claude.exe")` 看不见它，
//! 故直查包内的 `bin/claude.exe`（该 npm 包分发的是原生二进制，不是 JS 入口）。

pub mod account;
mod catalog;
mod cli_text;
pub mod fleet;
pub mod install;
mod screen;
pub mod setup;
mod subagent_prompt;
pub mod telemetry;
pub mod transcript;

use crate::{
    auth::{AuthScheme, CredentialSource, OAuthRefresh},
    caps::TelemetryCap,
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 接线事件集。`PreToolUse` 用 matcher 限定只在两种工具触发，与用户自有 `PreToolUse:Bash` 共存。
/// `PreCompact`/`PostCompact` 是压缩进行期间的「正在压缩」指示通道（transcript 在压缩期间
/// 零新增字节，只能靠 hook）；不加自定义 timeout——压缩 hook 是纯落库，5s 足够。
///
/// **此表须与 `scripts/install-hooks.mjs` 的 `SPECS` 保持一致**——由 meowo-app 的绊线测试守卫。
static EVENTS: [HookEvent; 11] = [
    HookEvent::matched("SessionStart", "*"),
    HookEvent::matched("UserPromptSubmit", "*"),
    HookEvent::matched("PostToolUse", "*"),
    HookEvent::matched("Stop", "*"),
    HookEvent::matched("StopFailure", "*"),
    HookEvent::matched("SessionEnd", "*"),
    HookEvent::matched("PermissionRequest", "*").with_timeout(310),
    // AskUserQuestion 的代答桥挂在这条上（reporter 阻塞等 GUI 作答）。提问不限时（用户要求
    // 2026-11-09），层层递减：hook > reporter 读（等待上限 + 5s）> broker 等待上限。
    // 实测 claude 2.1.319 接受 660s 的阻塞 PreToolUse；更长若被 claude 自己掐断，reporter
    // 随之退出、零输出等同放行，表单照常回到终端，不会更坏。
    HookEvent::matched("PreToolUse", "AskUserQuestion")
        .with_timeout(meowo_protocol::broker::QUESTION_HOOK_TIMEOUT_SECS),
    HookEvent::matched("PreToolUse", "ExitPlanMode"),
    HookEvent::matched("PreCompact", "*"),
    HookEvent::matched("PostCompact", "*"),
];

/// `settings.json` 不存在时从空对象建：刚装 Claude Code、没改过设置的用户就没有这个文件。
/// 但**不凭空造 `~/.claude` 目录**——数据目录不存在＝没装，由 `is_configured()` 在上游拦下。
///
/// command 形态：`"<exe>"`（带引号、无参数）。claude 靠 settings 里的位置区分 provider，不带
/// `--provider`；认领规则据此要求余参为空。
static HOOKS: HookSpec = HookSpec {
    config_rel: "settings.json",
    format: ConfigFormat::ClaudeJson,
    missing: MissingConfig::CreateFrom("{}"),
    events: &EVENTS,
    command: CommandSpec {
        quote_exe: true,
        with_provider: false,
        ps_call_operator: false,
    },
};

/// Claude Code 公开 OAuth client id 与刷新端点。macOS 把凭据存进登录 Keychain 的通用密码
/// （service = `Claude Code-credentials`），其它平台落 `<data>/.credentials.json`。
/// `account` 是写回 Keychain 时的条目名兜底值——读得到实际 account 时以实际值为准。
///
/// 用量端点（`api.anthropic.com/api/oauth/usage`）不在此处：`AuthScheme` 只管「凭据在哪 +
/// 怎么刷新」，用量是 account 侧的事。
/// 多账号：`CLAUDE_CONFIG_DIR` 一个变量就把整个数据目录搬走（凭据、settings.json、历史全在里面）。
///
/// macOS 上 claude 的凭据默认在 Keychain 而非文件——但**设了 `CLAUDE_CONFIG_DIR` 之后不影响隔离**：
/// 各 profile 的 hooks / settings / 历史都各自独立，而 Keychain 那份凭据是全局的，等于所有 profile
/// 共享同一个登录身份。**这一条尚未解决**（见 `creds_rel` 指向的文件回退路径），macOS 上要真正切换
/// 账号还需要额外轮换 Keychain 条目。Windows / Linux 无此问题。
static PROFILE: crate::profile::ProfileSpec = crate::profile::ProfileSpec {
    envs: &[("CLAUDE_CONFIG_DIR", "")],
    data_rel: "",
    creds_rel: ".credentials.json",
};

/// Claude 的会话可跨账号继续：`claude --resume <id>` 只认 `CLAUDE_CONFIG_DIR` 下的
/// 会话数据，把这几处 session 级数据复制到目标账号目录后即可在新账号下接着跑。
///
/// 只列 session 级数据——credentials / settings / plugins 一概不搬（那是账号本身，
/// 搬过去等于把两个账号合并）。
static CROSS_ACCOUNT: crate::profile::CrossAccountSession = crate::profile::CrossAccountSession {
    // `<root>/projects/<项目>/<session-id>.jsonl`
    transcript_depth: 3,
    session_dir_up: 0,
    session_buckets: &["file-history", "session-env", "tasks"],
    subagents_beside_transcript: true,
};

static AUTH: AuthScheme = AuthScheme {
    credentials: CredentialSource::KeychainOrFile {
        service: "Claude Code-credentials",
        account: "root",
        file: ".credentials.json",
    },
    refresh: Some(OAuthRefresh {
        token_url: "https://platform.claude.com/v1/oauth/token",
        client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    }),
    default_base_url: "",
    // 实测（claude --help / claude auth --help）：登录在 `auth` 子命令下，**没有** `claude login`。
    // 另有 `claude setup-token`（长期 token），不是交互式 OAuth 登录，不用它。
    login: Some(&["auth", "login"]),
    logout_args: &["auth", "logout"],
};

static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "claude",
    candidates: &[
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".local/bin",
        },
        // npm 全局前缀：Windows 上是 %APPDATA%\npm，某些环境 APPDATA 缺失则由 USERPROFILE 推。
        LaunchCandidate::Exe {
            root: Root::Env("APPDATA"),
            sub: "npm/node_modules/@anthropic-ai/claude-code/bin",
        },
        LaunchCandidate::Exe {
            root: Root::Env("USERPROFILE"),
            sub: "AppData/Roaming/npm/node_modules/@anthropic-ai/claude-code/bin",
        },
        LaunchCandidate::OnPath,
    ],
};

static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec {
        env: Some("CLAUDE_CONFIG_DIR"),
        env_suffix: "",
        candidates: &[".claude"],
    },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
    // claude 的目录信任是 `~/.claude.json` 的 `hasTrustDialogAccepted`，机制不同且未观察到卡点，
    // 暂不预写（见 docs/research/kimi-workspace-trust-2026-09.md 第 7 节）。
    trust: None,
}];

pub struct Claude;

/// claude 是**唯一**能把代理写进自己配置文件的 agent：`settings.json` 的 `env` 块，官方定义为
/// 「作用于每个会话及其派生子进程」——于是用户自己在终端敲 `claude` 也会走代理。
///
/// SOCKS 明确不支持（官方原文：Claude Code does not support SOCKS proxies），故 `socks: false`，
/// `socks_keys` 留空——填了也没用，只会让用户以为配上了。
static PROXY: crate::proxy::ProxySpec = crate::proxy::ProxySpec {
    socks: false,
    config_env: true,
    http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
    socks_keys: &[],
};

struct ClaudeRelay;
static RELAY: ClaudeRelay = ClaudeRelay;
static RELAY_AUTH: [crate::RelayOption; 2] = [
    crate::RelayOption {
        value: "bearer",
        label: "Bearer Token",
    },
    crate::RelayOption {
        value: "api_key",
        label: "API Key (x-api-key)",
    },
];
/// 中转文档常附带的两个 Claude Code 环境变量，做成可勾选项随中转注入：
/// 关掉遥测/自动更新等外联（中转场景下它们只会增加噪音），以及去掉归因标识头。
static RELAY_ENV_OPTIONS: [crate::RelayEnvOption; 2] = [
    crate::RelayEnvOption {
        id: "disable_nonessential_traffic",
        label: "Disable non-essential traffic",
        env: ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    },
    crate::RelayEnvOption {
        id: "no_attribution_header",
        label: "Omit attribution header",
        env: ("CLAUDE_CODE_ATTRIBUTION_HEADER", "0"),
    },
];
static RELAY_SUGGESTIONS: [crate::RelaySuggestionGroup; 1] = [crate::RelaySuggestionGroup {
    protocol: "",
    models: &[
        "claude-fable-5",
        "claude-opus-4-8",
        "claude-sonnet-5",
        "claude-haiku-4-5-20251001",
    ],
}];

impl crate::RelayCap for ClaudeRelay {
    fn ui(&self) -> crate::RelayUi {
        crate::RelayUi {
            protocols: &[],
            auth_modes: &RELAY_AUTH,
            default_protocol: "",
            default_auth: "bearer",
            suggestions: &RELAY_SUGGESTIONS,
            env_options: &RELAY_ENV_OPTIONS,
        }
    }
    fn launch_env(&self, config: crate::RelayConfig<'_>, key: &str) -> Vec<(String, String)> {
        let mut env = vec![
            (
                "ANTHROPIC_BASE_URL".into(),
                config.base_url.trim().trim_end_matches('/').into(),
            ),
            (
                (if config.auth == "api_key" {
                    "ANTHROPIC_API_KEY"
                } else {
                    "ANTHROPIC_AUTH_TOKEN"
                })
                .into(),
                key.into(),
            ),
        ];
        // 用户勾选的附加环境变量（只取本插件声明过的 id，未声明的一律忽略）。
        for option in &RELAY_ENV_OPTIONS {
            if config.env_options.iter().any(|id| id == option.id) {
                env.push((option.env.0.into(), option.env.1.into()));
            }
        }
        env
    }
    fn augment_argv(
        &self,
        config: crate::RelayConfig<'_>,
        _has_secret: bool,
        mut argv: Vec<String>,
    ) -> Vec<String> {
        argv.extend(["--model".into(), config.model.trim().into()]);
        argv
    }
    fn model_request(&self, config: crate::RelayConfig<'_>) -> crate::RelayModelRequest {
        crate::RelayModelRequest {
            auth: if config.auth == "api_key" {
                crate::RelayModelAuth::ApiKey
            } else {
                crate::RelayModelAuth::Bearer
            },
            anthropic_version: true,
        }
    }
}

static BROKER_QUESTION: crate::caps::BrokerQuestionCap = crate::caps::BrokerQuestionCap;
static BROKER_APPROVAL: crate::caps::BrokerApprovalCap = crate::caps::BrokerApprovalCap;

impl AgentPlugin for Claude {
    fn broker_question(&self) -> Option<&'static crate::caps::BrokerQuestionCap> {
        Some(&BROKER_QUESTION)
    }

    fn broker_approval(&self) -> Option<&'static crate::caps::BrokerApprovalCap> {
        Some(&BROKER_APPROVAL)
    }

    fn process_scope(&self, argv: &[String]) -> crate::registry::ProcessScope {
        use crate::registry::ProcessScope;
        let is_claude = |arg: &String| {
            let path = arg.replace('\\', "/").to_ascii_lowercase();
            matches!(
                path.rsplit('/').next(),
                Some("claude" | "claude.exe" | "claude.cmd")
            ) || path.ends_with("/@anthropic-ai/claude-code/cli.js")
        };
        // node 启动时脚本是 node 之后第一个非选项参数（`node --max-old-space-size=N cli.js`），
        // 只看前两项会漏判（同 gemini 0.68 重启自身的做空复现）。
        let script = argv.iter().skip(1).find(|arg| !arg.starts_with('-'));
        let own = argv.first().is_some_and(is_claude) || script.is_some_and(is_claude);
        if !own {
            return ProcessScope::Unknown;
        }
        // 后台服务/远程控制不等于一条交互会话，不能被「结束此会话」连带关闭。
        if argv.iter().skip(1).any(|arg| {
            matches!(
                arg.as_str(),
                "daemon" | "server" | "remote-control" | "--sdk-url"
            ) || arg.starts_with("--sdk-url=")
        }) {
            ProcessScope::Shared
        } else {
            ProcessScope::Exclusive
        }
    }
    fn screen_rules(&self) -> &'static [crate::screen::ScreenRule] {
        screen::RULES
    }
    fn cross_account_session(&self) -> Option<&'static crate::profile::CrossAccountSession> {
        Some(&CROSS_ACCOUNT)
    }

    fn id(&self) -> AgentId {
        id::CLAUDE
    }
    fn display_name(&self) -> &'static str {
        "Claude Code"
    }
    /// PermissionRequest hook 声明了 310s 阻塞（EVENTS 里的 with_timeout），决策输出会被采纳。
    fn permission_hook_decides(&self) -> bool {
        true
    }
    /// TUI 权限框与 PermissionRequest hook 并行显示、两边竞速（官方 hooks 文档明载 +
    /// 实测确认）：用户可直接在终端作答，hook 的决策随之被丢弃。终端视图因此不挂引导横幅。
    fn permission_prompt_races_hook(&self) -> bool {
        true
    }
    /// 旧版 Claude Code 的 `TodoWrite` 带整份快照。当前版本已换成增量的
    /// `TaskCreate`/`TaskUpdate`（见下方 delta 槽）——留着 `TodoWrite` 只为兼容旧版用户。
    fn todo_snapshot_tools(&self) -> &'static [&'static str] {
        &["TodoWrite"]
    }
    /// 现版本的增量任务工具：`TaskCreate` 单条新建（编号在结果文本
    /// `Task #N created successfully: …` 里），`TaskUpdate` 按 `taskId` 改状态/标题。
    /// 形状为真实 transcript 取证（2026-08，autopilot 仓的会话记录）。
    fn todo_delta_tools(&self) -> &'static [&'static str] {
        &["TaskCreate", "TaskUpdate"]
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["claude", "claude.exe"]
    }
    fn proxy(&self) -> Option<&'static crate::proxy::ProxySpec> {
        Some(&PROXY)
    }
    fn relay(&self) -> Option<&'static dyn crate::RelayCap> {
        Some(&RELAY)
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["--resume"]
    }
    /// `--resume <id> --fork-session`：恢复时换新 session id，原会话不动（`claude --help`，2.1.314）。
    /// 面板浏览器的 MCP 服务器怎么交给它（见 [`crate::McpAttach`]）。
    fn mcp_attach(&self) -> Option<crate::McpAttach> {
        Some(crate::McpAttach::ConfigFileArg {
            file_flag: "--mcp-config",
            allow_flag: Some("--allowedTools"),
            allow_prefix: "mcp__",
        })
    }

    fn fork_mode(&self) -> Option<crate::ForkMode> {
        Some(crate::ForkMode::Args {
            before: &["--resume"],
            after: &["--fork-session"],
        })
    }
    /// Claude：技能、`~/.claude.json` 的用户级 MCP、自定义命令与子代理、钩子、插件。
    /// `.claude.json` 在设了 `CLAUDE_CONFIG_DIR` 时跟数据目录走，否则在 home 下（Kimi 迁移文档）。
    fn extensions(
        &self,
        inst: &crate::variant::Installation,
    ) -> Vec<crate::extensions::ExtensionGroupDto> {
        use crate::extensions::{self as ext, ExtensionKind};
        let dir = &inst.data_dir;
        let claude_json = if inst.data_dir_from_env {
            dir.join(".claude.json")
        } else {
            crate::home_dir().unwrap_or_default().join(".claude.json")
        };
        let settings_path = dir.join("settings.json");
        let settings = ext::read_json(&settings_path);
        let user_config = ext::read_json(&claude_json);
        [
            ext::skills_group(&dir.join("skills"), false),
            ext::mcp_group_from_json(
                &claude_json,
                user_config.as_ref().and_then(|c| c.get("mcpServers")),
            ),
            ext::markdown_group(&dir.join("commands"), ExtensionKind::Command),
            ext::markdown_group(&dir.join("agents"), ExtensionKind::Agent),
            ext::claude_plugins_group(dir, settings.as_ref()),
            ext::hooks_group_from_json(
                &settings_path,
                settings.as_ref().and_then(|s| s.get("hooks")),
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn slash_commands(&self) -> &'static [&'static str] {
        &[
            "/add-dir", "/clear", "/compact", "/config", "/cost", "/help", "/init", "/mcp",
            "/memory", "/model", "/resume", "/review", "/status",
        ]
    }
    /// 启动选项（实测 `claude --help`）：`--model <alias>` 与 `--permission-mode
    /// <default|plan|acceptEdits|bypassPermissions>`。default 项一律不传 flag——CLI 的默认
    /// 行为由 CLI 自己决定，不替它猜。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        use crate::{Ask, LaunchChoice, LaunchOption, ModeTraits};
        static OPTIONS: [LaunchOption; 2] = [
            LaunchOption {
                id: "model",
                default: "default",
                choices: &[
                    LaunchChoice {
                        id: "default",
                        label: "Default",
                        args: &[],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    // 别名是 CLI 文档化的稳定契约（不带版本，由 CLI 解析到当期型号）；
                    // 这里的 label 只是**学到真实菜单之前的兜底**——GUI 会从 CLI 自己弹出的
                    // /model 菜单学真实标签并按 CLI 版本缓存（见前端 chat/modelLabels.ts），
                    // 所以这份文案过时也不会显示给用户，不必追着 CLI 改版同步。
                    LaunchChoice {
                        id: "fable",
                        label: "Fable 5",
                        args: &["--model", "fable"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "opus",
                        label: "Opus 5",
                        args: &["--model", "opus"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "sonnet",
                        label: "Sonnet 5",
                        args: &["--model", "sonnet"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "haiku",
                        label: "Haiku 4.5",
                        args: &["--model", "haiku"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "opusplan",
                        label: "Opus Plan",
                        args: &["--model", "opusplan"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    // `[m1]` 是字面方括号（CLI 文档化的 1M 上下文别名形式）。订阅计划下
                    // Fable/Opus 分 200K 与 1M 两档，裸别名落 200K 档——不提供这几个变体，
                    // 用户在会话里切到 1M 后每次恢复都被回放的 --model 拽回 200K（实拍反馈）。
                    LaunchChoice {
                        id: "fable[1m]",
                        label: "Fable 5 (1M)",
                        args: &["--model", "fable[1m]"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "opus[1m]",
                        label: "Opus 5 (1M)",
                        args: &["--model", "opus[1m]"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "sonnet[1m]",
                        label: "Sonnet 5 (1M)",
                        args: &["--model", "sonnet[1m]"],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                ],
                ..LaunchOption::PLAIN
            },
            LaunchOption {
                id: "permission",
                default: "default",
                choices: &[
                    LaunchChoice {
                        id: "default",
                        label: "Default",
                        args: &[],
                        risk: false,
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "plan",
                        label: "Plan",
                        args: &["--permission-mode", "plan"],
                        risk: false,
                        // 官方 permission-modes 文档：plan 只读、只出方案。
                        traits: Some(ModeTraits::new(Ask::Blocked, Ask::Blocked, true)),
                        desc: Some("claude.permission.plan"),
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "acceptEdits",
                        label: "Accept Edits",
                        args: &["--permission-mode", "acceptEdits"],
                        risk: false,
                        traits: Some(ModeTraits::new(Ask::Never, Ask::Always, false)),
                        desc: Some("claude.permission.acceptEdits"),
                        ..LaunchChoice::PLAIN
                    },
                    LaunchChoice {
                        id: "bypassPermissions",
                        label: "Bypass Permissions",
                        args: &["--permission-mode", "bypassPermissions"],
                        risk: true,
                        traits: Some(ModeTraits::new(Ask::Never, Ask::Never, false)),
                        desc: Some("claude.permission.bypassPermissions"),
                        ..LaunchChoice::PLAIN
                    },
                ],
                switch: crate::ModeSwitch::Control,
                ..LaunchOption::PLAIN
            },
        ];
        &OPTIONS
    }
    /// 用户级 `<数据目录>/commands/*.md`（多账号时随 profile 走）+ 项目级 `.claude/commands/`；
    /// 子目录按 `:` 命名空间（`commands/git/commit.md` → `/git:commit`）。
    fn custom_commands(&self) -> Option<&'static crate::CustomCommandSpec> {
        static SPEC: crate::CustomCommandSpec = crate::CustomCommandSpec {
            user_dir: Some("commands"),
            project_dir: Some(".claude/commands"),
            ext: "md",
            namespace_sep: Some(":"),
        };
        Some(&SPEC)
    }
    /// 内置命令中裸发送会弹出交互界面的那些（官方 CLI 行为）：`/config` 设置面板、
    /// `/mcp` 服务器面板、`/memory` 记忆文件选择器、`/model` 无参时的模型选择器、
    /// `/resume` 会话选择器。只声明内置表里确有、且形态经确认的——`/status`/`/cost`
    /// 之类只读面板不在此列（没有可选项，识别通道无用武之地）。
    fn menu_slash_commands(&self) -> &'static [&'static str] {
        &["/config", "/mcp", "/memory", "/model", "/resume"]
    }
    /// 提交文本里的 `@绝对路径` 会被原生附加(2.1.217 headless 实测:禁用全部读文件工具
    /// 仍能答出文件内容)。图片例外——@提及不产生图像块,前端已按此退回指令文本。
    /// 版本探测不到时兜底 false。
    fn attachment_mention(&self, version: Option<&str>) -> bool {
        version.is_some()
    }
    /// claude TUI 的 Ctrl-V 读系统剪贴板并把图片原生附加,composer 显示 `[Image #N]`。
    /// (@提及对图片无效——不产生图像块,故图片的原生化只有这条剪贴板通道。)
    /// 空格可省：运行中贴图时 claude 增量重绘，那格空白与上一帧相同就用光标定位跳过，
    /// 原始流是 `[Image\x1b[10G#1]`，剥掉控制码只剩 `[Image#1]`（pywinpty 实测 2.1.303，
    /// 屏幕上 0.55s 已落出占位符，旧规则一直等不到 → 运行中发图必报「图片附加未完成」）。
    fn clipboard_image_paste(&self, version: Option<&str>) -> Option<&'static str> {
        version.map(|_| r"\[Image ?#\d")
    }
    /// claude 在 Windows（含 WSL）上的贴图键是 **Alt+V**：2.1.301~303 二进制里都是
    /// `Le=windows||wsl?"alt+v":"ctrl+v"`（Ctrl+V 会被 Windows Terminal 自己的粘贴吃掉）。
    /// pywinpty 真机实测 2.1.303：`\x16` 8s 零反应；`\x1bv` 67ms 出 `Pasting…`、554ms
    /// 落出 `[Image #1]`。此前发 Ctrl-V 一直靠指令文本静默兜底，0.6.11 改为部分粘贴即
    /// 阻断后,Windows 上对话窗发图 100% 报「图片附加未完成」（用户实拍）。
    fn clipboard_paste_input(&self, _version: Option<&str>) -> Option<&'static str> {
        Some(if cfg!(windows) { "\x1bv" } else { "\x16" })
    }
    /// 输入框上方的运行中状态行：`✻ Composing… (41s · ↓ 427 tokens · thought for 4s)`
    /// （2.1.303 实拍）。行首是闪烁 spinner 字形之一，动词 1~3 个词以「…」收尾；括号统计
    /// 刚开跑时没有，有则以耗时数字开头——挡掉正文里恰好以 spinner 字形起头的普通行。
    fn status_line_pattern(&self) -> Option<&'static str> {
        Some(
            r"^\s*[✻✽✶✳✢·*✺✷✸✹]\s+([A-Z][A-Za-z'-]*(?: [A-Za-z'-]+){0,2}…)\s*(?:\((\d[^()]{0,160})\))?\s*$",
        )
    }
    /// 请求失败自动重试时 spinner 区画的那一行（2.1.312 包内渲染源码，重试不落 transcript）：
    /// `<错误> · Retrying in 5s · attempt 2/10`、`No response from the API after 30s ·
    /// retrying, waiting up to 1m · attempt 2/10`、`Auto mode check unavailable · next try in
    /// 5s · attempt 2/10 · esc to interrupt`。只认带「上限」的三种；容量排队的 `next try in …
    /// · attempt 3`（无上限）是在等空位，不是失败，不算。锚在行尾，正文里引用的半句不会命中。
    fn retry_line_pattern(&self) -> Option<&'static str> {
        Some(
            r"· (?:Retrying in|retrying, waiting up to|next try in) [^·]{1,60}· attempt (\d+)/(\d+)(?: · [^·]{0,40})?\s*$",
        )
    }
    /// `/effort <档位>`（immediate 命令，成功回 `Set effort level to <level> (…): …`）。
    /// 档位全集 low/medium/high/xhigh/max。当前档位由 statusline 的 `effort.level` 上报。
    ///
    /// 拒绝行（2.1.322 包内 `/effort` 实现逐条取证，2026-11-12）：2.1.314 的
    /// `Effort '<x>' isn't …` 已整句消失（全包 `' isn't` 模板 0 处），旧规则成了死规则，
    /// 被拒时前端只能报「未确认」。现在的失败出口是：
    /// - `Invalid argument: ${e}. Valid options are: …`
    /// - `Failed to set effort level: ${S.message}`
    /// - `Not applied: CLAUDE_CODE_EFFORT_LEVEL=${U} overrides effort this session, and …`
    /// - `CLAUDE_CODE_EFFORT_LEVEL=${U} overrides this session — clear it and ${o} takes over`
    ///   （设置写了，但本会话仍被环境变量钉住，等于没切）
    /// - `${o} is session-scoped and won't reach the remote process. …`
    /// - 封顶：`Effort '${e}' exceeds the cap for ${u} set by your settings or organization;
    ///   set to '${o}' instead…`。这条 CLI 其实已改成 `${o}`，捕获组 1 让前端落到实际档位。
    ///
    /// 模型不支持 xhigh/max 时 2.1.322 不再报错（只按组织/设置上限截断），照常回
    /// `Set effort level to max`。旧版那句仍保留：老版本用户还在。
    fn effort_control(&self) -> Option<crate::chat_ui::EffortControl> {
        Some(crate::chat_ui::EffortControl {
            command: "/effort",
            levels: &["low", "medium", "high", "xhigh", "max"],
            confirm_pattern: r"set effort level to (\w+)",
            reject_pattern: r"effort '[^']+' exceeds the cap(?:[^;]*; set to '(\w+)' instead)?|effort '[^']+' isn't|invalid argument: |failed to set effort level: |not applied: claude_code_effort_level=|claude_code_effort_level=\S+ overrides this session|is session-scoped and won't reach the remote process",
        })
    }
    /// 家族别名 → 当前版本的显示名（`opus` → `Opus 5.5`），读装着的 claude 可执行里内嵌的
    /// 模型目录（见 catalog.rs）。别名本身是稳定契约，过时的只是插件内置的标签。
    fn model_catalog(&self, _env: &[(String, String)]) -> Vec<crate::ModelCatalogEntry> {
        crate::installation(crate::id::CLAUDE)
            .and_then(|inst| inst.launch)
            .and_then(|argv| argv.into_iter().next())
            .map(|exe| catalog::family_catalog(std::path::Path::new(&exe)))
            .unwrap_or_default()
    }
    /// `/model <别名>` 的确认行原文是「Set model to … and saved as your default for new sessions」
    /// （见 model_select_reply 的取证）：对话页选模型会连新会话的默认模型一起改。
    fn model_select_saves_default(&self) -> bool {
        true
    }
    /// claude 的 `/model` 接受内联参数（`/model sonnet`），可以在对话页静默切换；
    /// 别名与官方 CLI 一致（`opusplan`：规划用 Opus、执行用 Sonnet）。
    fn model_select_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    /// `/model <别名>` 之后的回话（2.1.322 包内模板，2026-11-12 取证）。
    ///
    /// 确认：`var BTe="Set model to "` 拼 `${BTe}${显示名}${…}`，transcript 实样
    /// ``Set model to `Opus 5.5` and saved as your default for new sessions``。
    ///
    /// 拒绝（以前只报「未确认」，用户看不到 CLI 说了什么）：
    /// - `var ift="Kept model as "`：确认框（额度/1M 计费同意）被取消，模型没变；
    /// - `Model switch cancelled`：PreModelSwitch hook 跑的时候按了 Esc；
    /// - `Model switch to ${uT(e)} was blocked by a PreModelSwitch hook: …`、
    ///   `Model switch blocked by a PreModelSwitch hook…`、`Model switch failed: …`；
    /// - 别名校验失败（kje）：`Model '${n}' is restricted by your organization's settings.`、
    ///   `Model '${n}' is not available. …`、`Model '${n}' is not currently available for your account.`、
    ///   `Failed to validate model: …`、`Couldn't confirm model '${a}' is available (the check timed out)`、
    ///   `1M context is turned off …`、`… doesn't have a 1M context window, so '${f}' isn't available.`、
    ///   `Opus with 1M context is not available for your account.`。
    ///
    /// 锚在行首（允许命令输出前的 `⎿` 引线）：屏幕重绘可能把正文里引用的半句带进来，
    /// 只有 CLI 自己的输出行以这些词开头。
    fn model_select_reply(&self) -> Option<crate::chat_ui::CommandReply> {
        Some(crate::chat_ui::CommandReply {
            confirm_pattern: r"set model to ",
            reject_pattern: r"^\s*(?:⎿\s*)?(?:kept model as |model switch cancelled|model switch (?:to .+ was )?blocked by a premodelswitch hook|model switch failed: |model '[^']+' is (?:restricted|not available|not currently available)|failed to validate model: |couldn't confirm model '|1m context is turned off|\S.* doesn['’]t have a 1m context window|'[^']+' isn['’]t available: the |(?:opus|sonnet) with 1m context is not available|\S.* isn['’]t available for your account yet)",
        })
    }

    /// `/model <别名>`、`/effort <档>` 在对话中途的二次确认框（形态见 chat_ui::SwitchConfirm）。
    ///
    /// - `/model`：2.1.325 真机取证（tests/probe_claude_model.rs，2026-11-14）。空会话里切不弹，
    ///   有过回合之后切就弹；确认后照常回 `Set model to …`，选「No, go back」回 `Kept model as …`。
    /// - `/effort`：同一个组件，标题是 `Change effort level?`（2.1.325 程序文件里两句标题相邻，
    ///   确认项同为模板 `Yes, switch to ${…}`）。**没有真机走过**：切思考档会不会改写用户默认配置
    ///   没有取证，走查不碰它。所以这一半是按程序文件写的，核对条件与 `/model` 相同。
    /// - 确认项的编号可能不显示、焦点也可能默认落在取消项上（同一段代码里 `hideIndexes`、
    ///   `focus: Ve ? "cancel" : "confirm"`）。所以正则不要求编号，但要求焦点标记 `❯` 就在确认项上；
    ///   焦点在取消项上时不按键。
    fn switch_confirm(&self) -> Option<crate::chat_ui::SwitchConfirm> {
        Some(crate::chat_ui::SwitchConfirm {
            title_pattern: r"^\s*(?:switch model|change effort level)\?\s*$",
            accept_pattern: r"^\s*❯\s*(?:\d+\.\s*)?yes, switch to (.+?)\s*$",
            accept_input: "\r",
            // 真机两帧（/model haiku、/model fable）都有这一行；/effort 的同款框按程序文件也是它。
            auto_pattern: r"^\s*your next response will be slower and use more tokens\s*$",
            // 2.1.325 程序文件：`A PreModelSwitch hook asked you to confirm`（没有真机画面：要触发它得给
            // 用户配一个 hook）。只认关键词，hook 给的理由拼在哪里不影响判断。
            manual_pattern: Some(r"premodelswitch hook"),
        })
    }

    fn model_presets(&self) -> &'static [crate::ModelPreset] {
        // label 是学到真实菜单前的兜底，维护约定见 launch_options 里的注释。
        &[
            crate::ModelPreset {
                id: "fable",
                label: "Fable 5",
            },
            crate::ModelPreset {
                id: "opus",
                label: "Opus 5",
            },
            crate::ModelPreset {
                id: "sonnet",
                label: "Sonnet 5",
            },
            crate::ModelPreset {
                id: "haiku",
                label: "Haiku 4.5",
            },
            crate::ModelPreset {
                id: "opusplan",
                label: "Opus Plan",
            },
            // 1M 档同 launch_options 的注释：`/model fable[1m]` 内联切换有效。
            crate::ModelPreset {
                id: "fable[1m]",
                label: "Fable 5 (1M)",
            },
            crate::ModelPreset {
                id: "opus[1m]",
                label: "Opus 5 (1M)",
            },
            crate::ModelPreset {
                id: "sonnet[1m]",
                label: "Sonnet 5 (1M)",
            },
        ]
    }
    /// Claude Code 官方的 `chat:cycleMode` 键位（Shift+Tab）。模式集合会随账号与启动参数
    /// 变化，因此这里只声明“循环下一项”，不向 GUI 虚构一张固定、可直接跳转的列表。
    fn mode_controls(&self) -> &'static [crate::ModeControl] {
        // 屏幕回显标记 = 官方文档承诺的状态栏指示文本（permission-modes 文档）。cycle 是盲切，
        // 且 claude 只在活跃回合往 transcript 写模式记录——空闲切换若无屏幕回显，GUI 标签
        // 纹丝不动，用户只能认为按钮坏了。value 与 transcript 的 `permissionMode` 值一致。
        static MARKERS: [crate::ModeScreenMarker; 6] = [
            crate::ModeScreenMarker {
                marker: "bypass permissions on",
                value: "bypassPermissions",
            },
            crate::ModeScreenMarker {
                marker: "accept edits on",
                value: "acceptEdits",
            },
            crate::ModeScreenMarker {
                marker: "plan mode on",
                value: "plan",
            },
            crate::ModeScreenMarker {
                marker: "auto mode on",
                value: "auto",
            },
            crate::ModeScreenMarker {
                marker: "don't ask on",
                value: "dontAsk",
            },
            crate::ModeScreenMarker {
                marker: "manual mode on",
                value: "default",
            },
        ];
        static MODES: [crate::ModeControl; 1] = [crate::ModeControl {
            menu_command: None,
            dimension: "permission",
            cycle_input: Some("\x1b[Z"),
            options: &[],
            screen_markers: &MARKERS,
        }];
        &MODES
    }
    /// Claude 在首次进入某个 cwd 时会先显示 workspace trust 选择器。它不是可接收聊天文本的
    /// composer；这些片段覆盖现有版本的标题、错误兜底与非交互提示措辞。
    fn startup_attention_markers(&self) -> &'static [&'static str] {
        &[
            "do you trust the files in this folder",
            "do you trust the contents of this directory",
            "trust this folder",
            "workspace not trusted",
            "workspace trust dialog",
        ]
    }
    /// Esc 中断当前回合(官方文档行为):中断后已完成的工作保留,排队消息随即被处理。
    fn queue_input(&self) -> Option<&'static str> {
        Some("\r")
    }

    /// 排队消息在下一个工具边界插进当前回合，不等回合结束（2.1.304 实拍：注入后以
    /// queued_command 落盘，入队到落盘约 30s）。
    fn queue_delivery(&self) -> Option<crate::chat_ui::QueueDelivery> {
        Some(crate::chat_ui::QueueDelivery::NextStep)
    }

    fn interrupt_input(&self) -> Option<&'static str> {
        Some("\x1b")
    }

    /// 输入框规格（这一家唯一的一份，见 chat_ui::ComposerSpec）。
    /// - 框体：composer 夹在两条 `─` 横线之间，首行 `❯` + 不换行空格 + 正文，续行缩进两格；子代理
    ///   视图里上边框右侧挂描述（`──── probe-sleeper ─`）。空时渲染为独行 `❯`，或暗色占位
    ///   （`Try "fix typecheck errors"`、预测的下一句，内容不固定）。2.1.280 / 2.1.325 真机取证
    ///   （tests/probe_draft_residual.rs；2026-11-14 probe_tui_script `cells`：占位是 SGR dim，
    ///   光标停在列 2；真打出来的字不暗、光标在文末）。bash 模式（`!`）等别的提示符不认。
    /// - 暂存：Ctrl-S 暂存草稿(多行整段),下一次提交后 CLI 自动还原;有暂存时 composer 上方常驻
    ///   `› stashed`,暂存后输入框回到占位。Ctrl-U/Ctrl-Y 不可替代:多行只剪一行,且空 composer
    ///   时会粘出更早删掉的文本。
    /// - 清空：双 Esc（多行整段；Ctrl-U 只清一行）；空 composer 的双 Esc 会打开回退菜单
    ///   （tests/probe_claude_rewind.rs）。运行中 Esc 是中断，所以运行中不清，只用暂存。
    /// - 认不出输入框时照发：这一家另有提示识别、子代理视图、启动标记兜着，认不出多半是画面
    ///   被别的东西盖住了一角，拦发送只会让对话页发不出去。
    fn composer(&self, _version: Option<&str>) -> Option<crate::chat_ui::ComposerSpec> {
        Some(crate::chat_ui::ComposerSpec {
            prompt_footer: false,
            input_start_pattern: r"^(❯[ \xa0]?)",
            top_pattern: r"^\s*─{3,}(?:\s+\S.*?\s+─{1,3})?\s*$",
            bottom_pattern: r"^\s*─{3,}\s*$",
            row_pattern: r"^(?:❯[ \xa0]?|  |$)(.*)$",
            prompt_prefix: "",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r#"^(?:Try "[^"\r\n]*")?$"#,
            send_requires_recognition: false,
            stash: Some(crate::chat_ui::ComposerStash {
                input: "\x13",
                stashed_marker: "› stashed",
            }),
            clear_inputs: &["\x1b", "\x1b"],
            clear_rounds: 1,
            clear_while_running: false,
            alt_modes: &[],
            // 草稿保真（2.1.325 真机，tests/probe_draft_fidelity.rs，2026-11-14）：
            // - 粘贴占位 `[Pasted text #1]`（5120 字单行）、`[Pasted text #2 +59 lines]`（60 行）；3 行小粘贴原样显示。
            //   图片占位 `[Image #N]` 没有实拍，按宽的认。
            // - 输入框没有高度上限：45 行草稿把上边框顶出屏幕，这时整框认不出（不清）。所以不用溢出规则。
            // - 120 列时一行正文最多 116 格（提示符 2 格，右边留 2 格），按词折行；续行缩进 2 格。
            // - 发送前走的是暂存（Ctrl-S，CLI 自己保管原文），用不到这份判定；撤回前、切模型前清草稿才用。
            fidelity: Some(crate::chat_ui::ComposerFidelity {
                paste_pattern: r"\[Pasted text #\d+(?: \+\d+ lines)?\]|\[[Ii]mage[^\]\r\n]*\]",
                overflow_pattern: None,
                overflow_rows: None,
                wrap_edge: crate::chat_ui::ComposerWrapEdge::Terminal,
                wrap_inset: 2,
            }),
        })
    }

    /// 子代理视图（2.1.322 真机取证 tests/probe_claude_subagent_view.rs，形态与键见
    /// chat_ui::SubagentView）：占位 `Message @<类型>…` 出自源码 placeholder 的 viewingAgentName
    /// 分支；面板首行 `main`，Footer 键位表 ↑/↓ 移动、回车打开、Esc 取消选中，main 行回车
    /// 调用退出视图（footer:openSelected 在 coordinatorTaskIndex 为 0 时）。
    fn subagent_view(&self) -> Option<crate::chat_ui::SubagentView> {
        Some(crate::chat_ui::SubagentView {
            placeholder_prefix: "Message @",
            label_pattern: r"^\s*─{3,}\s+(\S.*?)\s+─{1,3}\s*$",
            panel_main_label: "main",
            panel_pointer: "❯",
            panel_viewed_marker: "●",
            down_input: "\x1b[B",
            up_input: "\x1b[A",
            open_input: "\r",
            leave_panel_input: "\x1b",
        })
    }

    /// `/rewind` 打开回退菜单（2.1.314 真机取证，tests/probe_claude_rewind.rs）：只取「Restore
    /// conversation」——与 kimi 撤回同一语义，文件不动；有代码改动时它是第 2 项、没有时是
    /// 第 1 项，按文案找。回退零写入 transcript，下一条消息从回退点长出新分支（读取层按
    /// parentUuid 收回被放弃的分支，见 ClaudeTranscript::chat_line_mark）。原文回到 composer，
    /// 双 Esc 清空（多行整段；Ctrl-U 只清一行）；空 composer 的双 Esc 会打开回退菜单，前端
    /// 先认出框里是原文才按。
    fn undo_control(&self) -> Option<crate::chat_ui::UndoControl> {
        Some(crate::chat_ui::UndoControl {
            command: "/rewind",
            count_arg: false,
            restores_files: false,
            reject_pattern: None,
            returns_input: true,
            menu: Some(crate::chat_ui::UndoMenu {
                title: "Rewind",
                ready_marker: "❯ (current)",
                step_input: "\x1b[A",
                selected_prefix: "❯",
                enter_input: "\r",
                quote_prefix: "│",
                option_label: "Restore conversation",
                cancel_input: "\x1b",
            }),
        })
    }

    /// `/btw <问题>`：旁路作答、零写入 transcript（2.1.314 探针实测，见 chat_ui::SideQuestion）。
    /// 答案区每按一次 ↓ 下移 3 行。
    fn side_question(&self) -> Option<crate::chat_ui::SideQuestion> {
        Some(crate::chat_ui::SideQuestion {
            command: "/btw",
            footer_marker: "esc to close",
            ready_marker: "c to copy",
            scroll_input: "\x1b[B",
            close_input: "\x1b",
        })
    }

    /// ESC+CR(meta+return)= composer 插入换行,ink 官方识别;WT 的 /terminal-setup
    /// 给 Shift+Enter 配置的正是这条序列,xterm 默认的 Alt+Enter 有效亦同理(实拍确认)。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\x1b\r")
    }
    /// AskUserQuestion 选择器的固有锚点项(真机截屏取证):单选/多问题标签页形态的
    /// 纯编号菜单靠它们与正文编号列表区分。识别层的选择器文法由此声明,不再硬编码。
    /// 整句可操作提示（真机截屏取证）。此前硬编码在前端并按 `provider === "claude"`
    /// 门控——门控的理由仍成立且已转移到这里：别家 agent 的输出里**引用**同一句
    /// （讨论审批流程、cat 一个含该句的脚本）不该误弹 Claude 的审批卡。
    fn attention_patterns(&self) -> &'static [crate::chat_ui::AttentionPattern] {
        &[
            crate::chat_ui::AttentionPattern {
                id: "claude:long-session-resume",
                patterns: &[
                    r"this session is[^\n]{0,120}\bold and[^\n]{0,80}\btokens\b",
                    "resuming the full session will consume a substantial portion of your usage limits",
                ],
                // 该提示每次恢复只出现一次，取首个匹配即可。
                last: false,
                details: crate::chat_ui::AttentionDetails::None,
                kind: crate::chat_ui::AttentionKind::Choice,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: None,
            },
            crate::chat_ui::AttentionPattern {
                id: "claude:command-approval",
                // `?` 是正则元字符，必须转义——否则 "proceed?" 会变成「d 可选」，
                // 匹配到 "do you want to procee" 这种根本不存在的前缀语义。
                patterns: &["this command requires approval", r"do you want to proceed\?"],
                last: true,
                details: crate::chat_ui::AttentionDetails::ProceedBox,
                kind: crate::chat_ui::AttentionKind::Approval,
                confirms: None,
                // 选项角色（2.1.325 真机屏幕，tests/probe_approval.rs 与 probe_deny_feedback.rs，2026-11-14）：
                //   ❯ 1. Yes
                //     2. Yes, and always allow access to <目录> from this project
                //     3. Yes, and switch to auto mode · auto mode handles these prompts for you
                //     4. No
                //   Esc to cancel · Tab to amend
                // 第二项随请求变：命令类是 `Yes, and don’t ask again for: curl *`（同版本真机），放行目录时是
                // 上面这句；长了会折到下一行，屏幕上这一行只剩 `… from this`，所以句尾不强求。
                // 旧版另有只管本会话的写法（`Yes, allow all edits during this session`）。
                //
                // 「说明并拒绝」不是单独的一项：焦点移到 `No` 上按 Tab，这一行变成
                // `❯ 4. No, and tell Claude what to do differently`，页脚的 `Tab to amend` 消失，
                // 再打的字替换掉那半句，回车提交。真机走通：命令没有执行（`Not run · rejected`），
                // 模型按说明作答（probe_deny_feedback，2.1.325）。
                // 最后一条规则：旧版把那句直接列成拒绝项（带 `(esc)`）；2.1.325 上它是 `No` 按过 Tab 之后的
                // 样子。两种情况下回车都是拒绝（没打字就是不带说明的拒绝）。旧版上怎么打说明没有取证，不给 typed。
                options: &[
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes$",
                        role: crate::chat_ui::OptionRole::AllowOnce,
                        scope: None,
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes, and (?:switch to|use) auto mode\b",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::AutoMode),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes,.*\bthis session\b",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Session),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes, and don['’]t ask again for:?\s*(.+)$",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Remember),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes, and always allow access to (.+?)(?:\s+from this(?: project)?)?$",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Remember),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^yes, and (?:don['’]t ask again|always allow)\b",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Remember),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^no$",
                        role: crate::chat_ui::OptionRole::Deny,
                        scope: None,
                        typed: Some(crate::chat_ui::TypedInput {
                            amend_input: Some("\t"),
                            ready_pattern: r"^\s*❯\s*(?:\d+\.\s*)?no, and tell claude what to do differently\s*$",
                            submit_input: "\r",
                        }),
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^no, and tell claude what to do differently",
                        role: crate::chat_ui::OptionRole::Deny,
                        scope: None,
                        typed: None,
                    },
                ],
                unparsed_keys: &[],
                notice: None,
            },
            // 以「都不问」档（`--dangerously-skip-permissions`）启动、而这个账号目录还没接受过时的确认页。
            // 2.1.325 真机（隔离配置目录加假密钥抓的，probe_tui_script，2026-11-15；与用户切账号后的实拍逐字一致）：
            //   WARNING: Claude Code running in Bypass Permissions mode
            //   In Bypass Permissions mode, Claude Code will not ask for your approval before running potentially dangerous commands.
            //   …
            //   ❯ No, exit
            //     Yes, I accept
            //   Enter to confirm · Esc to cancel
            // 接受记在账号目录的 settings.json（`skipDangerousModePermissionPrompt`），所以切到一个新账号、
            // 会话在新账号下以这一档重启时会再问一次。此前对话页不认识它，会话卡住而页面上没有任何提示
            // （用户实拍 2026-11-15）。**不代按**：这是上游的安全确认，由用户自己点。
            // 选项没有编号，焦点默认在 `No, exit`：下移一格再回车是接受（进了主界面），在 `No, exit` 上回车
            // CLI 直接退出（同一次真机都走过）。
            crate::chat_ui::AttentionPattern {
                id: "claude:bypass-permissions-confirm",
                patterns: &[r"^\s*WARNING: Claude Code running in Bypass Permissions mode\s*$"],
                last: true,
                details: crate::chat_ui::AttentionDetails::None,
                kind: crate::chat_ui::AttentionKind::Choice,
                confirms: None,
                options: &[crate::chat_ui::AttentionOptionRule {
                    pattern: r"^no, exit$",
                    role: crate::chat_ui::OptionRole::Exit,
                    scope: None,
                    typed: None,
                }],
                unparsed_keys: &[],
                notice: Some(crate::chat_ui::AttentionNotice::BypassPermissions),
            },
            // 环境里带着 ANTHROPIC_API_KEY、而这个账号目录没有对它表过态时的确认页（2.1.325 真机，同上）：
            //   Detected a custom API key in your environment
            //   ANTHROPIC_API_KEY: sk-ant-...
            //   Do you want to use this API key?
            //     Yes
            //   ❯ No (recommended)
            // 两项都不退出、也不是放行或拒绝，照原文列出。
            crate::chat_ui::AttentionPattern {
                id: "claude:custom-api-key-confirm",
                patterns: &[r"^\s*Detected a custom API key in your environment\s*$"],
                last: true,
                details: crate::chat_ui::AttentionDetails::None,
                kind: crate::chat_ui::AttentionKind::Choice,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: Some(crate::chat_ui::AttentionNotice::CustomApiKey),
            },
            // `/model`、`/effort` 的二次确认框（形态见 chat_ui::SwitchConfirm）。Meowo 自己发起的切换、且是
            // 「切到刚选的那一个」时由对话页代按，不出卡；其余情况（hook 要求确认、焦点在取消项上、
            // 用户自己在终端里打的命令）这个框就是一张选择卡，选项照原文列出。标题是整行才认：
            // 正文里引用这句不弹卡。
            crate::chat_ui::AttentionPattern {
                id: "claude:switch-confirm",
                patterns: &[r"^\s*(?:switch model|change effort level)\?\s*$"],
                last: true,
                details: crate::chat_ui::AttentionDetails::None,
                kind: crate::chat_ui::AttentionKind::Choice,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: None,
            },
            crate::chat_ui::AttentionPattern {
                id: "claude:plan-approval",
                // 计划模式的批准提示（claude 2.1.227 实拍取证，plan-file 流程）：
                //   Claude has written up a plan and is ready to execute. Would you like to proceed?
                //   ❯ 1. Yes, and bypass permissions / 2. Yes, manually approve edits / 3. Tell Claude what to change
                // 该提示**不触发** PreToolUse:ExitPlanMode 与 PermissionRequest hook（上游
                // 回归，官方文档明载两者都应触发；同版本普通工具的 PermissionRequest 正常）,
                // hook 系的 pendingReview/broker 卡整条链落空，屏幕识别是唯一的出口——
                // 因此这条 pattern 不能挂靠 interactivePrompt 门控（那个门控以 pendingReview
                // 为前置，正是缺失的一环）。
                // 词间用 \s+ 连接：窄终端把整句折行时中间是换行符，字面空格会匹配不上。
                // 不单独匹配 "would you like to proceed?"——正文引用该句（讨论审批流程）
                // 会误弹卡片锁住输入框，首句的「代笔完计划」措辞才是计划审批的独有指纹。
                patterns: &[r"written\s+up\s+a\s+plan\s+and\s+is\s+ready\s+to\s+execute"],
                last: true,
                details: crate::chat_ui::AttentionDetails::None,
                kind: crate::chat_ui::AttentionKind::Plan,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: None,
            },
        ]
    }

    /// CLI 漂移检查的手工声明部分（正则规则的样例、运行时拼接的字面量），见 cli_text.rs。
    fn cli_text_deps(&self) -> &'static [crate::cli_text::CliTextDep] {
        cli_text::DEPS
    }

    fn selector_anchors(&self) -> &'static [crate::chat_ui::SelectorAnchor] {
        &[
            crate::chat_ui::SelectorAnchor {
                marker: "type something",
                kind: crate::chat_ui::SelectorAnchorKind::Input,
            },
            crate::chat_ui::SelectorAnchor {
                marker: "chat about this",
                kind: crate::chat_ui::SelectorAnchorKind::Chat,
            },
        ]
    }
    /// 直下：从 `downloads.claude.ai`（GCS，**无 Cloudflare**）取二进制并校验 SHA-256。
    /// 见 `install.rs`——引导脚本做的正是这三步。
    fn direct_install(&self) -> Option<&'static dyn crate::install::InstallCap> {
        Some(&install::DIRECT_INSTALL)
    }

    /// 回退路径。`claude.ai` 在 Cloudflare 后面，会间歇触发人机校验（其页面以 HTTP 200 返回），
    /// 故优先走 `direct_install`；只有它失败（如发布物 schema 变了）才落到这里。
    fn install_script(&self, windows: bool) -> Option<crate::install::InstallScript> {
        Some(crate::install::InstallScript::Fetch {
            url: if windows {
                "https://claude.ai/install.ps1"
            } else {
                "https://claude.ai/install.sh"
            },
            unix_shell: "bash", // 脚本用了 `[[ ]]`，dash 跑不了
        })
    }
    /// claude 会把**自动生成的会话标题**写进标签页——但只有自然语言首条消息才触发生成；
    /// 首条消息是斜杠命令（如 `/code-review`）的会话标签永远停留在默认 "Claude Code"，
    /// 按任务标题匹配必然落空。故仍保留标题匹配，同时补 reporter token 兜底（见下）。
    fn sets_terminal_tab_title(&self) -> bool {
        true
    }
    /// 标签停在默认标题（斜杠命令会话）或标题漂移时，token 是唯一能精确命中的线索。
    /// 与 kimi 相同的已知折扣：claude 运行中 spinner 持续刷新标题会覆盖 token；Stop 时
    /// 写入的 token 在空闲/等输入期间存活——「点卡片定位过去」主要发生在这个阶段。
    fn writes_tab_token(&self) -> bool {
        true
    }
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
    /// FleetView 的后台会话（`sessions/<pid>.json` 的 `kind: "bg"`）。
    fn runtime(&self) -> Option<&'static dyn crate::caps::RuntimeCap> {
        Some(&fleet::CLAUDE_RUNTIME)
    }
    fn account(&self) -> Option<&'static dyn crate::account::AccountCap> {
        Some(&account::ACCOUNT)
    }
    fn wiring(&self) -> Option<&'static dyn crate::wiring::WiringCap> {
        Some(&setup::WIRING)
    }
    fn profile(&self) -> Option<&'static crate::profile::ProfileSpec> {
        Some(&PROFILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_process_scope_allows_only_identified_session_processes() {
        use crate::registry::ProcessScope;
        assert_eq!(Claude.process_scope(&[]), ProcessScope::Unknown);
        assert_eq!(
            Claude.process_scope(&["node".into(), "other.js".into()]),
            ProcessScope::Unknown
        );
        assert_eq!(
            Claude.process_scope(&["C:/bin/claude.exe".into(), "--resume".into(), "sid".into()]),
            ProcessScope::Exclusive
        );
        for service in [
            "daemon",
            "server",
            "remote-control",
            "--sdk-url=ws://127.0.0.1",
        ] {
            assert_eq!(
                Claude.process_scope(&["claude.exe".into(), service.into()]),
                ProcessScope::Shared
            );
        }
    }

    /// 斜杠命令会话的标签停在默认 "Claude Code"，标题匹配落空 → 必须同时声明 token 兜底。
    #[test]
    fn terminal_title_capabilities_cover_slash_command_sessions() {
        assert!(Claude.sets_terminal_tab_title());
        assert!(Claude.writes_tab_token());
    }

    /// 对话页运行条照搬状态行的前提:claude 声明了整行锚定的状态行正则。匹配行为
    /// 由前端 cliStatus.test.ts 用同一串正则测(本 crate 不依赖 regex)。
    #[test]
    fn declares_retry_line_pattern_anchored_at_line_end() {
        let pattern = Claude
            .retry_line_pattern()
            .expect("claude 应声明重试行正则");
        assert!(pattern.ends_with('$'));
        assert!(pattern.contains(r"attempt (\d+)/(\d+)"));
    }

    #[test]
    fn declares_anchored_status_line_pattern() {
        let pattern = Claude
            .status_line_pattern()
            .expect("claude 应声明状态行正则");
        assert!(pattern.starts_with('^') && pattern.ends_with('$'));
        assert!(pattern.contains('…'));
    }

    /// 回归钉:Windows 上发 Ctrl-V claude 零反应,对话窗发图必报「图片附加未完成」。
    #[test]
    fn clipboard_paste_input_is_alt_v_on_windows() {
        let expected = if cfg!(windows) { "\x1bv" } else { "\x16" };
        assert_eq!(Claude.clipboard_paste_input(None), Some(expected));
        assert_eq!(
            Claude.clipboard_paste_input(Some("2.1.303")),
            Some(expected)
        );
    }

    fn probe_in(home: &std::path::Path) -> Option<crate::Installation> {
        VARIANTS[0].probe(id::CLAUDE, home)
    }

    /// 每个测试一个独立 home，避免并发串扰。
    fn temp_home(tag: &str) -> std::path::PathBuf {
        let home = std::env::temp_dir().join(format!("meowo-claude-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        home
    }

    fn touch_exe(dir: &std::path::Path, stem: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(crate::exe_file_name(stem));
        std::fs::write(&p, b"").unwrap();
        p
    }

    #[test]
    fn config_and_credentials_sit_under_data_dir() {
        let home = temp_home("layout");
        let inst = probe_in(&home).expect("~/.claude 存在应命中");
        assert_eq!(inst.variant_tag, "stable");
        assert_eq!(
            inst.config_path(),
            home.join(".claude").join("settings.json")
        );
        // Keychain 变体在非 macOS 回退到文件路径；macOS 上调用方改读 Keychain，此路径仅作回退。
        assert_eq!(
            inst.credentials_path(),
            Some(home.join(".claude").join(".credentials.json"))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn native_installer_local_bin_is_found() {
        // 官方 installer 的落点。PATH 里没有它也必须能启动——这正是「装完打不开」的修复点。
        // 逐段 join：`join(".local/bin")` 在 Windows 上会原样留下 `/`，与 `join_rel` 拼出的
        // `\` 不是同一个字符串（虽指向同一文件），断言会假失败。
        let home = temp_home("localbin");
        let exe = touch_exe(&home.join(".local").join("bin"), "claude");
        let inst = probe_in(&home).unwrap();
        assert!(inst.is_launchable());
        assert_eq!(inst.launch_argv(), vec![exe.to_string_lossy().into_owned()]);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 候选顺序是声明表的一部分，改动会改变「装了多份 claude 时启动哪个」，故绊线守住。
    /// 不实测 npm/OnPath 两条：前者的根是真实环境变量（`APPDATA`/`USERPROFILE`），后者依赖
    /// 进程 PATH——在跑测试的机器上都不可控，实测只会得到一个随环境漂移的假测试。
    #[test]
    fn candidate_order_is_native_then_npm_then_path() {
        let names: Vec<&str> = LAUNCH
            .candidates
            .iter()
            .map(|c| match c {
                LaunchCandidate::Exe {
                    root: Root::Home,
                    sub,
                } => *sub,
                LaunchCandidate::Exe {
                    root: Root::Env(v), ..
                } => *v,
                LaunchCandidate::Exe {
                    root: Root::DataDir,
                    ..
                } => "data-dir",
                LaunchCandidate::NodeScript { .. } => "node-script",
                LaunchCandidate::OnPath => "on-path",
            })
            .collect();
        assert_eq!(
            names,
            vec![".local/bin", "APPDATA", "USERPROFILE", "on-path"]
        );
    }

    /// 登录 argv 接在启动 argv 之后。claude 是 `auth login` 两段——实测没有 `claude login`，
    /// 写错会让「登录」按钮拉起一个报 unknown command 的终端。
    #[test]
    fn login_argv_is_auth_login_appended_to_launch() {
        let home = temp_home("login");
        let exe = touch_exe(&home.join(".local").join("bin"), "claude");
        let inst = probe_in(&home).unwrap();
        let argv = inst.login_argv().expect("claude 应声明登录入口");
        assert_eq!(
            argv,
            vec![
                exe.to_string_lossy().into_owned(),
                "auth".into(),
                "login".into()
            ]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn events_cover_lifecycle_and_interaction_with_matchers() {
        assert_eq!(EVENTS.len(), 11);
        // PreToolUse 恰两条，matcher 分别是两种工具；其余六条 matcher 均为 "*"。
        let pre: Vec<_> = EVENTS
            .iter()
            .filter(|e| e.name == "PreToolUse")
            .map(|e| e.matcher.unwrap())
            .collect();
        assert_eq!(pre, vec!["AskUserQuestion", "ExitPlanMode"]);
        assert!(EVENTS
            .iter()
            .filter(|e| e.name != "PreToolUse")
            .all(|e| e.matcher == Some("*")));
        // 长超时恰两条：都在等人（PermissionRequest 等审批 310s、PreToolUse:AskUserQuestion
        // 等代答且不限时），其余 hook 是纯落库、5s 足够。
        for event in EVENTS.iter() {
            let question = event.name == "PreToolUse" && event.matcher == Some("AskUserQuestion");
            assert_eq!(
                event.timeout,
                if question {
                    meowo_protocol::broker::QUESTION_HOOK_TIMEOUT_SECS
                } else if event.name == "PermissionRequest" {
                    310
                } else {
                    5
                },
                "{} {:?}",
                event.name,
                event.matcher
            );
        }
    }
}
