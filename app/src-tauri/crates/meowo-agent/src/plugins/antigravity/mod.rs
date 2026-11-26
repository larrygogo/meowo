//! antigravity（Google Antigravity CLI，命令 `agy`）插件。2026-11 起替换 gemini CLI。
//!
//! 与 gemini CLI 是两套程序，没有一处能沿用：
//!
//! - **原生可执行**（Go），Windows 装在 `%LOCALAPPDATA%\agy\bin\agy.exe`，unix 在 `~/.local/bin/agy`
//!   （官方安装脚本说明，getting-started）。不再收 `node` 进程。
//! - **数据目录** `~/.gemini/antigravity-cli/`：每个对话一个 SQLite（`conversations/<id>.db`），
//!   正文是 protobuf（proto.rs / store.rs）。
//! - **hook 在全局 `~/.gemini/config/hooks.json`**，与 Antigravity App / IDE 共用，结构是
//!   「命名 hook → 事件 → handler」（[`crate::ConfigFormat::AgyJson`]）。`settings.json` 里的 hooks
//!   不加载。负载是 camelCase、部分字段是 JSON 字符串，由 [`hook::normalize`] 改写成规范形状。
//! - 会自己后台升级（实测 1.5.17 → 1.5.18 发生在两次启动之间），CLI 文案漂移要靠 cli-drift 盯。
//!
//! 事件对照（真机探针 agents/probe-hooks-1.md）：
//!
//! | agy | 规范名 | 说明 |
//! |---|---|---|
//! | `SessionStart` | `SessionStart` | 第一条消息提交时才触发（不在启动时） |
//! | `PreInvocation`（invocationNum == 0） | `UserPromptSubmit` | 每回合第一次模型调用；正文在 lastUserInput |
//! | `PostToolUse` | `PostToolUse` | 工具跑完 |
//! | `Stop` | `Stop` | 回合结束；负载不带回复正文，从库里读 |
//!
//! 没有 SessionEnd / Notification：会话结束靠进程退出判，等审批靠屏幕识别。

mod cli_text;
pub mod hook;
mod lead;
mod models;
mod proto;
mod screen;
mod store;
pub mod telemetry;

use crate::{
    caps::TelemetryCap,
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 数据目录（相对 home）。agy 没有覆盖它的环境变量（1.5.17 二进制里没有 GEMINI_CLI_HOME 一类的串）。
pub(crate) const DATA_DIR_REL: &str = ".gemini/antigravity-cli";

/// hook timeout 单位是**秒**（默认 30，agy 内置 hooks 文档 agy-strings 120084 行）。reporter 快进快出，
/// 10 秒足够，又不至于在 reporter 卡死时拖住 agy 太久（hook 同步阻塞 agent）。
const HOOK_TIMEOUT_SECS: u64 = 10;
static EVENTS: [HookEvent; 4] = [
    HookEvent::plain("SessionStart").with_timeout(HOOK_TIMEOUT_SECS),
    HookEvent::plain("PreInvocation").with_timeout(HOOK_TIMEOUT_SECS),
    HookEvent::matched("PostToolUse", "*").with_timeout(HOOK_TIMEOUT_SECS),
    HookEvent::plain("Stop").with_timeout(HOOK_TIMEOUT_SECS),
];

/// agy 文档化的全部 hook 事件（agy-strings 120034-120044；proto HookArgs 也只有这 6 种）。
pub const EVENT_WHITELIST: [&str; 6] = [
    "SessionStart",
    "PreToolUse",
    "PostToolUse",
    "PreInvocation",
    "PostInvocation",
    "Stop",
];

/// `~/.gemini/config/hooks.json`，相对数据目录 `~/.gemini/antigravity-cli` 往上一层。
///
/// Windows 上 agy 用 `cmd /c` 执行命令（不是 PowerShell，gemini 的 `& ` 前缀在这里是语法错误）。
/// 一对引号包住可执行、后跟参数的形态真机可执行（probe-hooks-1.md 第 2 条）。
static HOOKS: HookSpec = HookSpec {
    config_rel: "../config/hooks.json",
    format: ConfigFormat::AgyJson,
    missing: MissingConfig::CreateFrom("{}"),
    events: &EVENTS,
    command: CommandSpec {
        quote_exe: true,
        with_provider: true,
        ps_call_operator: false,
    },
};

static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "agy",
    candidates: &[
        // Windows 官方安装脚本的默认位置（`irm https://antigravity.google/cli/install.ps1 | iex`）。
        LaunchCandidate::Exe {
            root: Root::Env("LOCALAPPDATA"),
            sub: "agy/bin",
        },
        // unix 官方安装脚本的默认位置（`curl -fsSL https://antigravity.google/cli/install.sh | bash`）。
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".local/bin",
        },
        LaunchCandidate::OnPath,
    ],
};

/// Go 程序：字符串里有 HTTPS_PROXY / HTTP_PROXY / NO_PROXY 与 socks5（agy-strings），走标准库的
/// ProxyFromEnvironment，socks5 串同样从 HTTPS_PROXY 读。只认进程环境，只覆盖从 Meowo 打开的会话。
static PROXY: crate::proxy::ProxySpec = crate::proxy::ProxySpec {
    socks: true,
    config_env: false,
    http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
    socks_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
};

static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec {
        env: None,
        env_suffix: "",
        candidates: &[DATA_DIR_REL],
    },
    hooks: &HOOKS,
    // 凭据在系统钥匙串（Windows 凭据管理器），没有可读的凭据文件（install.md「secure keyring」）；
    // 也没有 login / logout 子命令（`agy --help`），登出是 TUI 里的 /logout。不声明，不给走不通的路。
    auth: None,
    launch: &LAUNCH,
    // 信任记录写在 settings.json 的 trustedWorkspaces，agy 启动时会按自己的格式重写整个文件
    // （settings.md）；不去预写，首次进入的信任提示由屏幕规则认出、对话页给按钮。
    trust: None,
}];

pub struct Antigravity;

impl AgentPlugin for Antigravity {
    fn id(&self) -> AgentId {
        id::ANTIGRAVITY
    }
    fn display_name(&self) -> &'static str {
        "Antigravity CLI"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn lead(&self) -> Option<&'static dyn crate::lead_install::LeadCap> {
        Some(&lead::CAP)
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["agy", "agy.exe"]
    }

    /// `agy acp` 是 ACP stdio 服务（Meowo 不用，可能被编辑器拉起），不独占会话。
    fn process_scope(&self, argv: &[String]) -> crate::registry::ProcessScope {
        use crate::registry::ProcessScope;
        let own = argv.first().is_some_and(|arg| {
            let path = arg.replace('\\', "/").to_ascii_lowercase();
            matches!(path.rsplit('/').next(), Some("agy" | "agy.exe"))
        });
        if !own {
            return ProcessScope::Unknown;
        }
        if argv.get(1).is_some_and(|arg| arg == "acp") {
            ProcessScope::Shared
        } else {
            ProcessScope::Exclusive
        }
    }

    fn normalize_hook_payload(&self, raw: &str) -> crate::HookPayload {
        hook::normalize(raw)
    }

    fn screen_rules(&self) -> &'static [crate::screen::ScreenRule] {
        screen::RULES
    }

    /// CLI 漂移检查的手工声明部分（正则规则的样例、运行时拼接的字面量），见 cli_text.rs。
    fn cli_text_deps(&self) -> &'static [crate::cli_text::CliTextDep] {
        cli_text::DEPS
    }

    /// `agy --conversation <id>`（-r / --resume 是 1.5.16 起的别名，changelog）。
    fn resume_args(&self) -> &'static [&'static str] {
        &["--conversation"]
    }

    /// Meowo 的 MCP 服务器登记在全局 `~/.gemini/config/mcp_config.json` 的 `mcpServers`
    /// （mcp.md）。agy 没有临时挂 MCP 的启动参数。MCP 子进程继承 agy 的环境（hook 子进程实测连
    /// MEOWO_PTY_TOKEN 这类带 TOKEN 的变量都在，probe-hooks-1.md 第 3 条），不用在配置里展开。
    fn mcp_attach(&self) -> Option<crate::McpAttach> {
        Some(crate::McpAttach::DataDirJson {
            rel: "../config/mcp_config.json",
            expand_env: false,
            approval: None,
        })
    }

    fn model_launch_flag(&self) -> Option<&'static str> {
        Some("--model")
    }
    /// `/model` 在 TUI 里弹选单；对话页切模型走带参重启接回（`--model <id> --conversation <id>`），
    /// `--model` 认 `agy models` 的 id（实测 `-p … --model gemini-3.8-flash-low` 正常答复）。
    fn model_menu_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    fn model_switch(&self) -> Option<crate::ModelSwitch> {
        Some(crate::ModelSwitch::RestartResume {
            flag: "--model",
            unsent_resume_args: None,
        })
    }
    /// `agy models`：每行 `<id>\t<显示名>`，首行是「Fetching available models...」。要联网取，
    /// 宿主有 15 秒上限。
    fn model_catalog_command(&self) -> Option<&'static [&'static str]> {
        Some(&["models"])
    }
    fn parse_model_catalog_output(&self, output: &str) -> Vec<crate::ModelCatalogEntry> {
        models::parse_catalog(output, self.resolve().map(|inst| inst.data_dir).as_deref())
    }

    /// 启动模式（`agy --help`：`--mode` 只收 accept-edits / auto / plan）。不传时 agy 用 auto
    /// （cli_reference.md：可用时默认 auto）；「每步都问」的 default 档没有启动参数，只能启动后
    /// Shift+Tab 切过去，所以不列（列了做不到）。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        use crate::{Ask, LaunchChoice, LaunchOption, ModeSwitch, ModeTraits};
        static OPTIONS: [LaunchOption; 1] = [LaunchOption {
            id: "approval",
            default: "auto",
            choices: &[
                LaunchChoice {
                    id: "auto",
                    label: "auto",
                    args: &["--mode", "auto"],
                    risk: false,
                    // 占位文字原文「Auto mode: edits & safe commands auto-approved」。
                    traits: Some(ModeTraits::new(Ask::Never, Ask::Risky, false)),
                    desc: Some("antigravity.approval.auto"),
                    server: None,
                },
                LaunchChoice {
                    id: "accept-edits",
                    label: "accept-edits",
                    args: &["--mode", "accept-edits"],
                    risk: false,
                    // 「Accept-edits mode: file edits auto-approved」：命令仍问。
                    traits: Some(ModeTraits::new(Ask::Never, Ask::Always, false)),
                    desc: Some("antigravity.approval.acceptEdits"),
                    server: None,
                },
                LaunchChoice {
                    id: "plan",
                    label: "plan",
                    args: &["--mode", "plan"],
                    risk: false,
                    // 「Plan mode: research & plan only」。
                    traits: Some(ModeTraits::new(Ask::Blocked, Ask::Blocked, true)),
                    desc: Some("antigravity.approval.plan"),
                    server: None,
                },
            ],
            switch: ModeSwitch::Restart,
            resume_upgrade_only: false,
            in_session_only: false,
        }];
        &OPTIONS
    }

    /// 状态栏右侧 `<模式> · <模型> · <effort>`（如 `auto · Gemini 4 Neon · high`）；default 档不写
    /// 模式名（run1/04_mode_*.txt）。只用于读出当前模式，读不到时不显示、不重启。
    fn mode_controls(&self) -> &'static [crate::ModeControl] {
        static MARKERS: [crate::ModeScreenMarker; 3] = [
            crate::ModeScreenMarker {
                marker: "auto · ",
                value: "auto",
            },
            crate::ModeScreenMarker {
                marker: "accept-edits · ",
                value: "accept-edits",
            },
            crate::ModeScreenMarker {
                marker: "plan · ",
                value: "plan",
            },
        ];
        static MODES: [crate::ModeControl; 1] = [crate::ModeControl {
            dimension: "approval",
            menu_command: None,
            cycle_input: None,
            options: &[],
            screen_markers: &MARKERS,
        }];
        &MODES
    }

    /// 运行中输入框仍可用，回车是排队（research-screen.md §2：「Enter 是排队」）。
    fn queue_input(&self) -> Option<&'static str> {
        Some("\r")
    }
    fn queue_delivery(&self) -> Option<crate::chat_ui::QueueDelivery> {
        Some(crate::chat_ui::QueueDelivery::TurnEnd)
    }
    /// Esc 中断当前回合（审批面板上按 Esc 也是中断整个回合，research-screen.md §3）。
    fn interrupt_input(&self) -> Option<&'static str> {
        Some("\x1b")
    }
    /// Ctrl+J（裸 LF）插入换行（research-screen.md §7 实测有效）。Alt+Enter 也行，但两字节的
    /// ESC CR 一旦被拆开写入，就成了「Esc 中断 + 回车提交」。`\` + 回车的兜底同批写入会被当成
    /// 提交，不用。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\n")
    }

    /// 输入框：两条满宽 `─` 夹着 `> `，续行缩进两格；占位文字是当前模式的说明，default 档没有
    /// 占位（research-screen.md §1、run2/09_pasted.txt）。历史里每条用户消息也画成同样的框，但框线
    /// 只有半宽，从屏幕底往上找到的第一对才是输入框。
    ///
    /// 清空草稿的按键没有取证（Ctrl+C 不清空、只提示再按一次退出），不声明：有草稿时对话页不替用户
    /// 清，给出由人选择的出路。
    fn composer(&self, _version: Option<&str>) -> Option<crate::chat_ui::ComposerSpec> {
        Some(crate::chat_ui::ComposerSpec {
            prompt_footer: false,
            // 空输入框那一行只有一个 `>`（行尾不带空格，run2/27_idle_after_intro.txt），有字时是 `> 正文`。
            input_start_pattern: r"^(>(?: |$))",
            top_pattern: r"^─+\s*$",
            bottom_pattern: r"^─+\s*$",
            // 捕获组含提示符：读框时拿首行的 `>` 对 prompt_prefix 认输入起点。
            row_pattern: r"^(>(?: .*)?|  .*)$",
            prompt_prefix: ">",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r"^(?:(?:Auto|Plan|Accept-edits) mode: .*\(shift\+tab to cycle\))?$",
            suggestion_placeholder: false,
            send_requires_recognition: true,
            stash: None,
            clear_inputs: &[],
            clear_rounds: 1,
            clear_while_running: false,
            alt_modes: &[],
            fidelity: None,
        })
    }

    /// 审批面板与提问表单（research-screen.md §3、§4）。选项行 `> 1. Yes, run command` 的 `>` 是焦点；
    /// 数字键也能即选即确认（按 `1` 批准了 b.txt 的创建），但卡片走 ProceedBox 文法，作答是从焦点相对移动
    /// 再回车（dev 实例走查 2026-11-26 点允许、拒绝都生效）。
    fn attention_patterns(&self) -> &'static [crate::chat_ui::AttentionPattern] {
        use crate::chat_ui::{
            AttentionDetails, AttentionKind, AttentionOptionRule, AttentionPattern, OptionRole,
            PersistScope, RoleKey,
        };
        &[
            AttentionPattern {
                id: "antigravity:command-approval",
                patterns: &[r"^\s*run this command\?\s*$", r"^\s*allow [^?\n]{1,80}\?\s*$"],
                last: true,
                details: AttentionDetails::ProceedBox,
                kind: AttentionKind::Approval,
                confirms: None,
                // 命令审批四项（run2/05_approval_cmd.txt）：`Yes, run command`、`Yes, and always allow in
                // this conversation for commands that start with '…'`、`Yes, and always allow for commands
                // that start with '…' (Persist to settings.json)`、`No, cancel`。文件审批两项：
                // `Yes, allow creation`、`No, deny creation`。别的审批（访问、工具调用、权限）的选项是同一套
                // 模板（agy.exe 1.5.18 里的格式串）：`Yes, allow access` / `Yes, allow tool call`；
                // `Yes, and always allow access to '%s' in this conversation`、`Yes, and always grant
                // permission for '%s' in this conversation`；`Yes, and always allow tool '%s' (Persist to %s)`；
                // `No, deny access` / `No, deny tool call` / `No, deny permission`。
                options: &[
                    AttentionOptionRule {
                        pattern: r"^yes, (?:run command|allow [a-z ]+)$",
                        role: OptionRole::AllowOnce,
                        scope: None,
                        typed: None,
                    },
                    AttentionOptionRule {
                        pattern: r"^yes, and always (?:allow|grant)\b.*\bin this conversation\b",
                        role: OptionRole::AllowPersistent,
                        scope: Some(PersistScope::Session),
                        typed: None,
                    },
                    AttentionOptionRule {
                        pattern: r"^yes, and always allow\b.*\(persist to [^)]+\)$",
                        role: OptionRole::AllowPersistent,
                        scope: Some(PersistScope::Remember),
                        typed: None,
                    },
                    AttentionOptionRule {
                        pattern: r"^no, (?:cancel|deny(?: [a-z ]+)?)$",
                        role: OptionRole::Deny,
                        scope: None,
                        typed: None,
                    },
                ],
                // 选项认不出时只留拒绝：Esc 中断整个回合，与焦点无关。
                unparsed_keys: &[RoleKey {
                    role: OptionRole::Deny,
                    input: "\x1b",
                }],
                notice: None,
                anchors: &[],
                tail_lines: None,
            },
            AttentionPattern {
                id: "antigravity:question",
                patterns: &[r"^\s*question \d+/\d+: "],
                last: true,
                details: AttentionDetails::None,
                kind: AttentionKind::Question,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: None,
                anchors: &[],
                tail_lines: None,
            },
            AttentionPattern {
                id: "antigravity:trust",
                patterns: &[r"^\s*do you trust the contents of this project\?\s*$"],
                last: true,
                details: AttentionDetails::None,
                kind: AttentionKind::Choice,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
                notice: None,
                anchors: &[],
                tail_lines: None,
            },
        ]
    }

    /// 提问表单（`Question 1/1:` + `> 1. 选项`）的最后一项是要现场打字的写入项（run2/16_ask_form：
    /// `3. Write-in...`）。它是这种编号列表的锚点：有它的编号组才是表单，正文里的编号列表不会变成按钮；
    /// 它自己不出「点了就选」的按钮。
    fn selector_anchors(&self) -> &'static [crate::chat_ui::SelectorAnchor] {
        &[crate::chat_ui::SelectorAnchor {
            marker: "write-in",
            kind: crate::chat_ui::SelectorAnchorKind::Input,
        }]
    }

    /// 帮助面板里的命令（run1/03_help_commands*.txt）。只列对话页常用、行为已知的几条。
    fn slash_commands(&self) -> &'static [&'static str] {
        &[
            "/clear",
            "/compact",
            "/context",
            "/effort",
            "/help",
            "/model",
            "/resume",
            "/rewind",
            "/usage",
        ]
    }

    /// 技能目录：CLI 额外读 `~/.gemini/antigravity-cli/skills/`（skills.md「CLI skill locations」）；
    /// MCP 在全局 mcp_config.json。
    fn extensions(
        &self,
        inst: &crate::variant::Installation,
    ) -> Vec<crate::extensions::ExtensionGroupDto> {
        use crate::extensions as ext;
        let dir = &inst.data_dir;
        let mcp_path = crate::join_rel(dir, "../config/mcp_config.json");
        let mcp = ext::read_json(&mcp_path);
        [
            ext::skills_group(&dir.join("skills"), false),
            ext::shared_agent_skills(),
            ext::mcp_group_from_json(&mcp_path, mcp.as_ref().and_then(|m| m.get("mcpServers"))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn install_script(&self, windows: bool) -> Option<crate::install::InstallScript> {
        Some(crate::install::InstallScript::Fetch {
            url: if windows {
                "https://antigravity.google/cli/install.ps1"
            } else {
                "https://antigravity.google/cli/install.sh"
            },
            unix_shell: "bash",
        })
    }

    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
    fn proxy(&self) -> Option<&'static crate::proxy::ProxySpec> {
        Some(&PROXY)
    }
    /// 每次模型调用都记了已用量与窗口大小（gen_metadata 的 context_window_metadata）。
    fn provides_context(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
