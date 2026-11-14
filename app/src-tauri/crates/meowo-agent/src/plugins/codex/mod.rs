//! codex（OpenAI Codex CLI）插件。数据目录只有一种（`~/.codex`），复杂度全在**可执行的三种落法**：
//!
//! | 优先级 | 安装方式 | argv |
//! |---|---|---|
//! | 1 | bun 全局（用户多用 bun 装/更新） | `[~/.bun/bin/codex]` |
//! | 2 | npm 全局 | `["node", "<npm>/node_modules/@openai/codex/bin/codex.js"]` |
//! | 3 | 官方独立安装 | `[<data>/packages/standalone/current/bin/codex]` |
//!
//! npm 那条**必须**走 node 包装：直接拉原生 codex.exe 不会真正恢复会话（无 rollout、无 hook）。
//! npm 副本常是过期版（resume 会拉到旧版、每次提示更新），故排在 bun 之后。
//! 独立安装那条也是修「装完仍显示未安装」的关键：安装脚本只改持久 PATH，运行中的 meowo-app
//! 进程 PATH 是启动时的旧快照、看不到新目录，故直查这个固定路径。
//!
//! 接线还有一步**副作用**（往 `config.toml` 写 `[hooks.state]` 的 trusted_hash 预信任）——见 `setup.rs`。

pub mod account;
mod cli_text;
mod exec_script;
mod lead;
mod lineage;
mod screen;
mod subagents;
pub mod setup;
pub mod telemetry;

use crate::{
    auth::{AuthScheme, CredentialSource},
    caps::TelemetryCap,
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 接线事件集：dispatch 消化面 ∩ codex 支持面。不配 PreToolUse（其 matcher 目标是 claude 专属工具）。
/// PreCompact/PostCompact 的支持面已在实装二进制上取证（0.152.0 的 HookEventsToml 枚举
/// 字符串里两个事件名俱在），供 /compact 期间显示「正在压缩」。
static EVENTS: [HookEvent; 9] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    // Codex 当前 hooks 文档支持正常关闭和中断收尾；两者预算上限 3s，不能沿用普通事件的 5s。
    HookEvent::plain("SessionEnd").with_timeout(3),
    HookEvent::plain("Interrupt").with_timeout(3),
    HookEvent::plain("PreCompact"),
    HookEvent::plain("PostCompact"),
    HookEvent::plain("PermissionRequest").with_timeout(310),
];

/// hooks.json 不存在时从空态建——与 kimi「config.toml 缺失即未登录」不同是有意的：
/// 此处不存在不代表用户手改过畸形内容，codex 本就允许该文件缺席。
static HOOKS: HookSpec = HookSpec {
    config_rel: "hooks.json",
    format: ConfigFormat::CodexJson,
    missing: MissingConfig::CreateFrom("{\"hooks\":{}}"),
    events: &EVENTS,
    command: CommandSpec {
        quote_exe: true,
        with_provider: true,
        ps_call_operator: false,
    },
};

/// codex 的 `auth.json` 由 CLI 自己维护（含 OIDC id_token），Meowo 只读不刷新 → `refresh: None`。
/// 用量走 rollout 文件与 auth.json 内的字段，无独立 base_url。
/// 多账号：`CODEX_HOME` 一个变量搬走整个数据目录（凭据 auth.json、hooks.json、rollout 全在里面）。
static PROFILE: crate::profile::ProfileSpec = crate::profile::ProfileSpec {
    envs: &[("CODEX_HOME", "")],
    data_rel: "",
    creds_rel: "auth.json",
};

/// codex 的会话可跨账号继续：resume 只按 `$CODEX_HOME/sessions/**` 找 rollout 文件，
/// 把那一个文件按原相对路径复制进目标账号目录即可在新账号下接着跑。
///
/// 实测（codex 0.152.0，本机 2026-09-08）：
/// - 空 `CODEX_HOME` 下 `codex exec resume <id>` → `Error: thread/resume failed: no rollout found
///   for thread id <id>`；
/// - 同一个空目录里**只**放进 `sessions/2026/08/20/rollout-<时刻>-<id>.jsonl` 一个文件 →
///   直接接上会话（打印出同一个 session id 的会话头）。
///
/// 于是 `thread_history_1.sqlite`（新版的分页 thread history 投影）与 `history.jsonl` 都**不搬**：
/// 实测证明 resume 不需要它们，而它们是账号级的聚合文件，搬过去等于把两个账号的历史混在一起。
static CROSS_ACCOUNT: crate::profile::CrossAccountSession = crate::profile::CrossAccountSession {
    // `<root>/sessions/<年>/<月>/<日>/rollout-<时刻>-<session-id>.jsonl`
    transcript_depth: 5,
    session_dir_up: 0,
    session_buckets: &[],
    subagents_beside_transcript: false,
};

static AUTH: AuthScheme = AuthScheme {
    credentials: CredentialSource::File("auth.json"),
    refresh: None,
    default_base_url: "",
    // `codex login`（另有 `codex login status`，但 kimi 无 status 子命令，登录态检测统一走读凭据）。
    login: Some(&["login"]),
    logout_args: &["logout"],
};

static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "codex",
    candidates: &[
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".bun/bin",
        },
        // npm 全局前缀：Windows 上是 %APPDATA%\npm，某些环境 APPDATA 缺失则由 USERPROFILE 推。
        LaunchCandidate::NodeScript {
            root: Root::Env("APPDATA"),
            rel: "npm/node_modules/@openai/codex/bin/codex.js",
        },
        LaunchCandidate::NodeScript {
            root: Root::Env("USERPROFILE"),
            rel: "AppData/Roaming/npm/node_modules/@openai/codex/bin/codex.js",
        },
        // `current` 是指向当前 release 的 junction/symlink，跨平台稳定。
        LaunchCandidate::Exe {
            root: Root::DataDir,
            sub: "packages/standalone/current/bin",
        },
        LaunchCandidate::Exe {
            root: Root::Env("LOCALAPPDATA"),
            sub: "Programs/OpenAI/Codex/bin",
        },
        LaunchCandidate::OnPath,
    ],
};

static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec {
        env: Some("CODEX_HOME"),
        env_suffix: "",
        candidates: &[".codex"],
    },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
    trust: None,
}];

pub struct Codex;

/// codex **无法**从配置文件配代理：让主进程走代理的 issue（openai/codex#6060）仍开着，PR 被拒。
/// `config.toml` 里那两个看着像的键都不是：`features.network_proxy` 是**沙箱子进程**的代理，
/// `shell_environment_policy.set` 只注入给它派生的 shell——都不影响 codex 自己的 API 请求。
/// 只剩进程环境变量一条路（reqwest 会自动读）。
///
/// SOCKS 不支持：codex-rs 的 reqwest 没编译 `socks` feature（另见 issue #20844：Windows 下
/// SOCKS5 不稳，改用 HTTP 代理即恢复）。
static PROXY: crate::proxy::ProxySpec = crate::proxy::ProxySpec {
    socks: false,
    config_env: false,
    http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
    socks_keys: &[],
};

struct CodexRelay;
static RELAY: CodexRelay = CodexRelay;
static RELAY_AUTH: [crate::RelayOption; 1] = [crate::RelayOption {
    value: "bearer",
    label: "Bearer Token",
}];
static RELAY_SUGGESTIONS: [crate::RelaySuggestionGroup; 1] = [crate::RelaySuggestionGroup {
    protocol: "",
    models: &[
        "gpt-5.6-sol",
        "gpt-5.6-terra",
        "gpt-5.6-luna",
        "gpt-5.4",
        "gpt-5.3-codex",
    ],
}];

impl crate::RelayCap for CodexRelay {
    fn ui(&self) -> crate::RelayUi {
        crate::RelayUi {
            protocols: &[],
            auth_modes: &RELAY_AUTH,
            default_protocol: "",
            default_auth: "bearer",
            suggestions: &RELAY_SUGGESTIONS,
            env_options: &[],
        }
    }
    fn launch_env(&self, _config: crate::RelayConfig<'_>, key: &str) -> Vec<(String, String)> {
        vec![("MEOWO_CODEX_RELAY_KEY".into(), key.into())]
    }
    fn augment_argv(
        &self,
        config: crate::RelayConfig<'_>,
        has_secret: bool,
        mut argv: Vec<String>,
    ) -> Vec<String> {
        if !has_secret {
            return argv;
        }
        let quoted = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
        for value in [
            "model_provider=\"meowo-relay\"".to_string(),
            "model_providers.meowo-relay.name=\"Meowo Relay\"".to_string(),
            format!(
                "model_providers.meowo-relay.base_url={}",
                quoted(config.base_url.trim().trim_end_matches('/'))
            ),
            "model_providers.meowo-relay.env_key=\"MEOWO_CODEX_RELAY_KEY\"".to_string(),
            "model_providers.meowo-relay.wire_api=\"responses\"".to_string(),
            format!("model={}", quoted(config.model.trim())),
        ] {
            argv.extend(["-c".into(), value]);
        }
        argv
    }
    fn model_request(&self, _config: crate::RelayConfig<'_>) -> crate::RelayModelRequest {
        crate::RelayModelRequest {
            auth: crate::RelayModelAuth::Bearer,
            anthropic_version: false,
        }
    }
}

static BROKER_APPROVAL: crate::caps::BrokerApprovalCap = crate::caps::BrokerApprovalCap;

impl AgentPlugin for Codex {
    fn broker_approval(&self) -> Option<&'static crate::caps::BrokerApprovalCap> {
        Some(&BROKER_APPROVAL)
    }

    fn screen_rules(&self) -> &'static [crate::screen::ScreenRule] {
        screen::RULES
    }

    fn id(&self) -> AgentId {
        id::CODEX
    }
    fn display_name(&self) -> &'static str {
        "Codex"
    }
    /// PermissionRequest hook 声明了 310s 阻塞（EVENTS 里的 with_timeout），决策输出会被采纳。
    fn permission_hook_decides(&self) -> bool {
        true
    }
    fn lead(&self) -> Option<&'static dyn crate::lead_install::LeadCap> {
        Some(&lead::CAP)
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn cross_account_session(&self) -> Option<&'static crate::profile::CrossAccountSession> {
        Some(&CROSS_ACCOUNT)
    }
    fn proxy(&self) -> Option<&'static crate::proxy::ProxySpec> {
        Some(&PROXY)
    }
    fn relay(&self) -> Option<&'static dyn crate::RelayCap> {
        Some(&RELAY)
    }
    fn process_names(&self) -> &'static [&'static str] {
        // 会话本体是原生 codex 二进制；npm 包装时它由 node 启动但 hook 由 codex 自身触发，上溯命中
        // codex(.exe) 即可。不收 node.exe（过宽，会把任意 node 进程误判为 agent）。
        &["codex", "codex.exe"]
    }
    /// 0.167.2 的 app-server 持有 ThreadManager 多线程表；嵌入式服务也不等于独占。
    /// 来源：0.167.2 隔离双 thread 的 SessionStart/UserPromptSubmit 父链全部是同一 daemon PID
    /// （target/lead-provider-probes/codex-shared/result.json），原生 TUI 退出后也仍可续跑。
    fn process_scope(&self, argv: &[String]) -> crate::registry::ProcessScope {
        use crate::registry::ProcessScope;
        if !argv
            .first()
            .is_some_and(|executable| self.owns_process(executable))
        {
            ProcessScope::Unknown
        } else if argv.iter().skip(1).any(|arg| {
            matches!(arg.as_str(), "app-server" | "exec-server" | "--remote")
                || arg.starts_with("--remote=")
        }) {
            ProcessScope::Shared
        } else {
            ProcessScope::Exclusive
        }
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["resume"]
    }
    /// `codex fork <SESSION_ID>`：起交互界面并分叉出新线程（`codex fork --help`，0.166）。
    /// 面板浏览器的 MCP 服务器怎么交给它（见 [`crate::McpAttach`]）。
    fn mcp_attach(&self) -> Option<crate::McpAttach> {
        Some(crate::McpAttach::TomlOverride { flag: "-c" })
    }

    fn fork_mode(&self) -> Option<crate::ForkMode> {
        Some(crate::ForkMode::Args {
            before: &["fork"],
            after: &[],
        })
    }
    /// 模型目录读 `$CODEX_HOME/models_cache.json`：codex 自己从服务端拉回、给 `/model` 菜单
    /// 用的那份缓存（0.163 实测：`models[].slug/display_name/visibility/supported_reasoning_levels`）。
    /// 只列 `visibility = "list"` 的——`hide` 的（内部审查模型等）codex 菜单里也不出现。
    fn model_catalog(&self, env: &[(String, String)]) -> Vec<crate::ModelCatalogEntry> {
        codex_home(env)
            .and_then(|home| std::fs::read_to_string(home.join("models_cache.json")).ok())
            .map(|content| parse_models_cache(&content))
            .unwrap_or_default()
    }
    /// 没登录过（没有 models_cache.json）时用 CLI 内置目录：`--bundled` 跳过联网刷新（`--help`），
    /// 实测 0.06s（research-codex-opencode §1）。
    fn model_catalog_command(&self) -> Option<&'static [&'static str]> {
        Some(&["debug", "models", "--bundled"])
    }
    fn parse_model_catalog_output(&self, output: &str) -> Vec<crate::ModelCatalogEntry> {
        parse_debug_models(output)
    }
    /// 裸 `/model` 弹菜单，只作兜底（会话没有 app-server 时）。主路线是会话自带服务的
    /// `thread/settings/update {model, effort}`：TUI 底栏立刻变、下一回合生效、草稿和待批审批都不动、
    /// 不写 config.toml（research-codex-opencode §2 exp2/exp3）。恢复时不带 `-m`（会弹新模型推荐框，
    /// 选 1 改写默认模型，§3）。
    fn model_menu_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    fn model_switch(&self) -> Option<crate::ModelSwitch> {
        Some(crate::ModelSwitch::ServerSettings)
    }
    fn model_launch_flag(&self) -> Option<&'static str> {
        Some("-m")
    }
    fn launch_defaults(
        &self,
        env: &[(String, String)],
        _cwd: Option<&std::path::Path>,
    ) -> crate::LaunchDefaults {
        codex_home(env)
            .and_then(|home| std::fs::read_to_string(home.join("config.toml")).ok())
            .map(|content| parse_config_defaults(&content))
            .unwrap_or_default()
    }
    /// 「自动审查」首轮只读的根因（真机第四轮 N7，evidence/round4-n7）：Windows 上没配 Windows 沙箱时，
    /// Codex 读配置会把 workspace-write 降成 read-only（上游 codex-rs/config/src/config_toml.rs
    /// `effective_sandbox_mode`：Windows + WindowsSandboxLevel::Disabled + WorkspaceWrite → ReadOnly；
    /// 本机 0.167.2 `codex debug prompt-input` 实测：不配时首轮提示词写 `sandbox_mode` is `read-only`，
    /// `-c windows.sandbox=unelevated` 或 `elevated` 时是 `workspace-write`）。认领后经 app-server 补设虽能
    /// 改成 workspace-write，但沙箱级别仍是 Disabled，上游 `effective_windows_sandbox_type` 此时给
    /// SandboxType::None——命令实际不进沙箱。
    ///
    /// 所以凡是映射到 workspace-write 的档（「必要时询问」ask、「自动审查」approveForMe，按声明表的启动参数认），
    /// 只要 CLI 配置里没配 Windows 沙箱（`[windows] sandbox`，或旧的
    /// `features.experimental_windows_sandbox` / `elevated_windows_sandbox` / `enable_experimental_windows_sandbox`），
    /// 启动参数就补 `-c windows.sandbox=unelevated`：Codex 自带、不要管理员权限的受限令牌沙箱，首轮起就是
    /// workspace-write 且真进沙箱（leader 2026-11-14 定范围）。用户自己配过的（包括 elevated、mxc）一律不覆盖。
    /// 新建与接回都加：接回时权限参数被丢掉（远程 resume 不收，见 codex_app_server 模块头），但 `-c` 照常交给
    /// TUI 与 app-server，thread 存下的 workspace-write 才不会在接回后又被降成只读。只在 Windows。
    /// 这一步只让档位名副其实，不连 app-server（认领后补设的范围另见 session_settings::claim_server_update）。
    fn extra_launch_args(
        &self,
        selections: &std::collections::HashMap<String, String>,
        env: &[(String, String)],
        cwd: Option<&std::path::Path>,
    ) -> Vec<String> {
        let workspace_write = selections.get("approval").is_some_and(|picked| {
            self.launch_options()
                .iter()
                .find(|option| option.id == "approval")
                .and_then(|option| {
                    option
                        .choices
                        .iter()
                        .find(|choice| choice.id == picked.as_str())
                })
                .is_some_and(|choice| {
                    choice
                        .args
                        .windows(2)
                        .any(|pair| pair == ["--sandbox", "workspace-write"])
                })
        });
        if !cfg!(windows) || !workspace_write {
            return Vec::new();
        }
        if windows_sandbox_configured_in_layers(codex_home(env).as_deref(), cwd) {
            Vec::new()
        } else {
            vec!["-c".into(), "windows.sandbox=unelevated".into()]
        }
    }
    /// Esc 中断当前回合:codex 状态行自述 "esc to interrupt"(真机 capture 在档,
    /// 见 docs/research/tui-menu-captures-2026-07.md)。排队键 Tab 在空闲时同样直接提交
    /// (probe_tui_script,0.162.1,2026-10-16),运行态误判也不会卡住消息。
    /// 输入框规格（这一家唯一的一份，见 chat_ui::ComposerSpec）。
    /// - 框体：2026-11-12 隔离无模型实拍（target/lead-provider-probes/composer）。
    /// - 占位 `Ask Codex to do anything` 是暗色、光标停在列 2；真打出同一句是正常色、光标在文末
    ///   （0.167.2，2026-11-14 probe_tui_script `cells`）。读屏带光标就能分开。
    /// - 清空：Ctrl-C 清掉整段草稿，不留提示；回合进行中按，草稿清掉、回合照常继续（同日实拍：
    ///   `Count from 1 to 300` 途中按下，数字接着往下出）。空输入框上的 Ctrl-C 是退出提示，
    ///   所以只在认出有字时按。
    fn composer(&self, version: Option<&str>) -> Option<crate::chat_ui::ComposerSpec> {
        if !crate::lead_install::version_at_least(version, (0, 167, 2)) {
            return None;
        }
        Some(crate::chat_ui::ComposerSpec {
            prompt_footer: true,
            input_start_pattern: r"^(› )",
            top_pattern: r"^› ",
            bottom_pattern: r"^  [A-Za-z0-9][^\r\n]* · .+$",
            row_pattern: r"^(?:› |  )(.*)$",
            prompt_prefix: "",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r"^(?:Ask Codex to do anything)?$",
            send_requires_recognition: true,
            stash: None,
            clear_inputs: &["\x03"],
            clear_rounds: 1,
            clear_while_running: true,
            alt_modes: &[],
        })
    }
    fn queue_input(&self) -> Option<&'static str> {
        Some("\t")
    }

    /// Tab 是「排到本回合结束后」;插进当前回合的是 Enter(官方键盘文档,见 queue_input)。
    fn queue_delivery(&self) -> Option<crate::chat_ui::QueueDelivery> {
        Some(crate::chat_ui::QueueDelivery::TurnEnd)
    }

    fn interrupt_input(&self) -> Option<&'static str> {
        Some("\x1b")
    }
    /// Alt+Enter（ESC+CR）在 composer 里换行、不提交。
    ///
    /// 此前声明的是 Ctrl+J（裸 LF，0.162.1 取证 2026-10-16），理由是它不带 ESC 前缀。0.167.2 上它
    /// 已经不换行：真机 `line one` + LF + `line two` 在输入框里连成一行 `line oneline two`
    /// （tests/probe_tui_script.rs，2026-11-14），对话页终端里的 Ctrl/Shift+Enter 因此失效。
    /// 同一轮取证：Alt+Enter 与 Shift+Enter 的 CSI-u 序列（`ESC[13;2u`）都换行；Ctrl+J 的 CSI-u
    /// 序列（`ESC[106;5u`）被当成字面文字。取 Alt+Enter：0.162.1 上它同样可用（当时的注释），
    /// 两个版本都成立。
    ///
    /// 当初担心的「ESC 被单独认成 Esc，运行中就是一次中断」：0.167.2 回合进行中各发一次 Alt+Enter 与
    /// CSI-u，输入框里都是两行，回合照常数完，屏幕上没有 interrupted（同日实拍）。序列是一次写入，
    /// 不拆开发。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\x1b\r")
    }
    /// `@绝对路径 正文` 提交后模型自行读取该文件（实测 0.162.1：Read secret.txt → 答对）。
    /// 与 kimi 同为文本级惯例。版本探测不到时兜底 false。
    fn attachment_mention(&self, version: Option<&str>) -> bool {
        version.is_some()
    }
    /// Ctrl-V 读系统剪贴板原生附加图片，composer 落出 `[Image #N]`；提交后模型答对了测试图
    /// 的颜色（probe_tui_script 实测 0.162.1）。粘贴键用默认的 Ctrl-V。
    /// 空格可省，理由同 claude：增量重绘会用光标定位跳过与上一帧相同的空白格（codex 侧
    /// 未实测，ratatui 同为按格 diff 输出，按同一形态防）。
    fn clipboard_image_paste(&self, version: Option<&str>) -> Option<&'static str> {
        version.map(|_| r"\[Image ?#\d")
    }
    /// CLI 漂移检查的手工声明部分（正则规则的样例、旧版兼容与运行时拼接的字面量），见 cli_text.rs。
    fn cli_text_deps(&self) -> &'static [crate::cli_text::CliTextDep] {
        cli_text::DEPS
    }
    /// codex 的 `/model` 是交互式菜单（不接内联参数），故只列命令、不声明模型预设。
    /// Codex：自己的 `skills/`（跳过自带的 `.system`）与共享的 `~/.agents/skills`、
    /// `config.toml` 的 MCP、`prompts/` 自定义命令、`hooks.json` 钩子。
    fn extensions(
        &self,
        inst: &crate::variant::Installation,
    ) -> Vec<crate::extensions::ExtensionGroupDto> {
        use crate::extensions::{self as ext, ExtensionKind};
        let dir = &inst.data_dir;
        let hooks_path = dir.join("hooks.json");
        let hooks = ext::read_json(&hooks_path);
        [
            ext::skills_group(&dir.join("skills"), false),
            ext::shared_agent_skills(),
            ext::mcp_group_from_codex_toml(&dir.join("config.toml")),
            ext::markdown_group(&dir.join("prompts"), ExtensionKind::Command),
            ext::hooks_group_from_json(&hooks_path, hooks.as_ref().and_then(|h| h.get("hooks"))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn slash_commands(&self) -> &'static [&'static str] {
        &[
            "/clear", "/compact", "/diff", "/help", "/model", "/new", "/review", "/status",
        ]
    }
    /// 目录信任屏。0.166.0 的标题改成 `Trust this folder? Codex can read, edit, and run files
    /// here, ...`（codex.exe 内嵌字符串，research-cli-drift X1，2026-11-12 复核：二进制里
    /// `do you trust` 0 处）。只认通用表的旧句，新目录首条消息会被信任屏吞掉、会话像卡住。
    /// 带问号取前半句：整句会随终端宽度折行，问号让它不至于撞上普通对话里的「trust this folder」。
    /// 通用表的两句保留，0.16x 之前的老版本用户还在。
    fn startup_attention_markers(&self) -> &'static [&'static str] {
        &[
            "do you trust the files in this folder",
            "do you trust the contents of this directory",
            "trust this folder?",
        ]
    }
    /// 协作模式：会话里经 app-server `thread/settings/update {collaborationMode}` 直接设（exp4 实测底栏
    /// 出现 `Plan mode`），没有服务时退回 Shift+Tab 循环。回显随 rollout 的 turn_context。
    fn mode_controls(&self) -> &'static [crate::ModeControl] {
        static MODES: [crate::ModeControl; 1] = [crate::ModeControl {
            menu_command: None,
            dimension: "collaboration",
            cycle_input: Some("\x1b[Z"),
            options: &[],
            // codex 的 TUI 指示文案未经验证，宁缺毋滥；显示仍随 transcript 状态走。
            screen_markers: &[],
        }];
        &MODES
    }
    /// 启动选项按 0.167.2 TUI `/permissions` 的四档（Read Only / Ask for approval / Approve for me /
    /// Full Access，exp6 `screen-permissions.txt` 原文，research-codex-opencode §9）。旧版的
    /// 「Full Auto」（`-a never -s workspace-write`）不再是任何一档，存了它的老会话按默认档恢复。
    /// 栏 id 仍叫 approval、Full Access 仍用 id `yolo`：老会话存的选择照旧对得上。
    /// 0.167.2 Windows 实测：经 `--remote` 起的新会话，TUI 侧的 `--sandbox workspace-write` 首轮仍是
    /// read-only，app-server 侧 `-c sandbox_mode=…` 也一样（真机验证 2026-11-14）；只有会话内
    /// `thread/settings/update` 设得上。所以会话认领后宿主按存的这一档再用服务补设一次
    /// （session_settings::apply_launch_server_settings），对话页按 rollout 的实际值显示。
    /// 会话里经 app-server 改：`server` 片段是宿主 CodexSettingsUpdate 的 JSON（取值用 CLI 参数写法，
    /// 由 lead_native::codex_app_server 换成 `thread/settings/update` 参数，字段见 ThreadSettingsUpdateParams）。
    /// 协作模式（Default / Plan）没有启动参数，只在会话里切。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        use crate::{Ask, LaunchChoice, LaunchOption, ModeSwitch, ModeTraits};
        static OPTIONS: [LaunchOption; 2] = [
            LaunchOption {
                id: "approval",
                default: "default",
                choices: &[
                    LaunchChoice {
                        id: "default",
                        label: "Default",
                        args: &[],
                        risk: false,
                        traits: None,
                        desc: Some("codex.approval.default"),
                        server: None,
                    },
                    LaunchChoice {
                        id: "readOnly",
                        label: "Read Only",
                        args: &["--sandbox", "read-only"],
                        risk: false,
                        // 「Read workspace files, with approval required for edits or internet access」
                        traits: Some(ModeTraits::new(Ask::Always, Ask::Risky, false)),
                        desc: Some("codex.approval.readOnly"),
                        server: Some(
                            r#"{"approval":"on-request","approvalsReviewer":"user","sandbox":"read-only"}"#,
                        ),
                    },
                    LaunchChoice {
                        id: "ask",
                        label: "Ask for approval",
                        args: &[
                            "--sandbox",
                            "workspace-write",
                            "--ask-for-approval",
                            "on-request",
                        ],
                        risk: false,
                        // 「Read and edit workspace files and run commands, with approval required for
                        // internet access or edits outside the workspace」
                        traits: Some(ModeTraits::new(Ask::Risky, Ask::Risky, false)),
                        desc: Some("codex.approval.ask"),
                        server: Some(
                            r#"{"approval":"on-request","approvalsReviewer":"user","sandbox":"workspace-write"}"#,
                        ),
                    },
                    LaunchChoice {
                        id: "approveForMe",
                        label: "Approve for me",
                        // 不用 `--approve-for-me`：0.167.2 Windows 实测单独用它启动沙箱落成 read-only（与 --help
                        // 写的 workspace-write 不符），会话里显示成「只读」；它又与 --sandbox 互斥（真机验证
                        // codex/README 问题 1）。改为与会话内切到这一档同一组值：workspace-write + on-request +
                        // 自动审查（config 键 approvals_reviewer 与 app-server 的 approvalsReviewer 同义）。
                        args: &[
                            "--sandbox",
                            "workspace-write",
                            "--ask-for-approval",
                            "on-request",
                            "-c",
                            "approvals_reviewer=auto_review",
                        ],
                        risk: false,
                        // 「Only ask for actions detected as potentially unsafe」；越界先交自动审查。
                        traits: Some(ModeTraits::new(Ask::Risky, Ask::Risky, false)),
                        desc: Some("codex.approval.approveForMe"),
                        server: Some(
                            r#"{"approval":"on-request","approvalsReviewer":"auto_review","sandbox":"workspace-write"}"#,
                        ),
                    },
                    LaunchChoice {
                        id: "yolo",
                        label: "Full Access",
                        args: &[
                            "--sandbox",
                            "danger-full-access",
                            "--ask-for-approval",
                            "never",
                        ],
                        risk: true,
                        // 「Use with caution: Codex can edit files outside this workspace and access the
                        // internet without approval」
                        traits: Some(ModeTraits::new(Ask::Never, Ask::Never, false)),
                        desc: Some("codex.approval.yolo"),
                        server: Some(
                            r#"{"approval":"never","approvalsReviewer":"user","sandbox":"danger-full-access"}"#,
                        ),
                    },
                ],
                switch: ModeSwitch::Server,
                resume_upgrade_only: false,
                in_session_only: false,
            },
            LaunchOption {
                id: "collaboration",
                default: "default",
                choices: &[
                    LaunchChoice {
                        id: "default",
                        label: "Default",
                        args: &[],
                        risk: false,
                        // 问不问由权限档决定。
                        traits: Some(ModeTraits::new(Ask::Inherit, Ask::Inherit, false)),
                        desc: Some("codex.collaboration.default"),
                        server: Some(r#"{"collaboration":"default"}"#),
                    },
                    LaunchChoice {
                        id: "plan",
                        label: "Plan",
                        args: &[],
                        risk: false,
                        // 「You must not perform mutating actions」：提示词约束，不是沙箱约束。
                        traits: Some(ModeTraits::new(Ask::Blocked, Ask::Inherit, true)),
                        desc: Some("codex.collaboration.plan"),
                        server: Some(r#"{"collaboration":"plan"}"#),
                    },
                ],
                switch: ModeSwitch::Server,
                resume_upgrade_only: false,
                in_session_only: true,
            },
        ];
        &OPTIONS
    }
    /// 自定义 prompt：`~/.codex/prompts/*.md` → `/<文件名>`。只有用户级（codex 无项目级目录），
    /// 平铺无命名空间。
    fn custom_commands(&self) -> Option<&'static crate::CustomCommandSpec> {
        static SPEC: crate::CustomCommandSpec = crate::CustomCommandSpec {
            user_dir: Some("prompts"),
            project_dir: None,
            ext: "md",
            namespace_sep: None,
        };
        Some(&SPEC)
    }
    /// 直取 GitHub Releases，**不走 `chatgpt.com`**。
    ///
    /// 官方命令是 `irm https://chatgpt.com/codex/install.ps1 | iex`，而 `chatgpt.com` 在
    /// Cloudflare 后面（实测 `server: cloudflare` + `cf-ray`），会间歇触发人机校验——其页面以
    /// HTTP 200 返回，裸管道会把那坨 HTML 喂给解释器。
    ///
    /// 但那个地址**只是一个 302**，终点就是下面这个 URL：
    ///
    /// ```text
    /// chatgpt.com/codex/install.ps1
    ///   → 302 → github.com/openai/codex/releases/latest/download/install.ps1
    ///           server: github.com → Windows-Azure-Blob，无 Cloudflare
    /// ```
    ///
    /// 两者内容逐字节相同（实测 sha256 一致）。直接取终点即可绕开 CF，不必去啃 GitHub API 的
    /// 未认证限流（60/h），也不必解压 unix 侧那些 `.tar.gz`——脚本自己会处理。
    ///
    /// `latest/download/` 是 GitHub 的稳定跳转，始终指向最新 release 的同名资产。
    fn install_script(&self, windows: bool) -> Option<crate::install::InstallScript> {
        Some(crate::install::InstallScript::Fetch {
            url: if windows {
                "https://github.com/openai/codex/releases/latest/download/install.ps1"
            } else {
                "https://github.com/openai/codex/releases/latest/download/install.sh"
            },
            unix_shell: "sh", // 官方命令写的就是 `| sh`
        })
    }
    // Codex 在首条 prompt 前尚未写 spinner/project 标题，此时 cwd 匹配没有信号；SessionStart hook
    // 先写 session token，便可精确定位空白新会话。首条 prompt 后 Codex 会覆盖 token 为自己的标题，
    // app 随即回退到 cwd 匹配，因此消息前后都可定位。它不写「任务标题」式标签，故
    // sets_terminal_tab_title 仍取默认 false。
    fn writes_tab_token(&self) -> bool {
        true
    }
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
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

/// 解析 codex 的 `models_cache.json`（见 `model_catalog`）。格式不认得即空表，不猜。
fn parse_models_cache(content: &str) -> Vec<crate::ModelCatalogEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(models) = value.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    models
        .iter()
        .filter(|model| model.get("visibility").and_then(|v| v.as_str()) != Some("hide"))
        .filter_map(|model| {
            let id = model.get("slug").and_then(|v| v.as_str())?.to_string();
            let label = model
                .get("display_name")
                .and_then(|v| v.as_str())
                .filter(|name| !name.is_empty())
                .map_or_else(|| id.clone(), str::to_string);
            let efforts = model
                .get("supported_reasoning_levels")
                .and_then(|v| v.as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|level| level.get("effort").and_then(|e| e.as_str()))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let mut entry = crate::ModelCatalogEntry::new(id, label);
            entry.efforts = efforts;
            entry.default_effort = model
                .get("default_reasoning_level")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            entry.is_default = model.get("is_default").and_then(|v| v.as_bool()) == Some(true);
            Some(entry)
        })
        .collect()
}

/// `codex debug models --bundled` 的输出：与 models_cache.json 同形，前面可能夹一行 WARNING。
fn parse_debug_models(output: &str) -> Vec<crate::ModelCatalogEntry> {
    output
        .find('{')
        .map(|start| parse_models_cache(&output[start..]))
        .unwrap_or_default()
}

/// 新建页默认值：`$CODEX_HOME/config.toml` 顶层的 `model`、`approval_policy`、`sandbox_mode`、
/// `approvals_reviewer`（research-codex-opencode §7；`-p` 叠加的 profile 不读）。
fn parse_config_defaults(content: &str) -> crate::LaunchDefaults {
    let mut top = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim();
            let value = value.split(" #").next().unwrap_or(value).trim();
            top.insert(key.trim().to_string(), value.trim_matches('"').to_string());
        }
    }
    let mut out = crate::LaunchDefaults {
        model: top.get("model").filter(|m| !m.is_empty()).cloned(),
        ..Default::default()
    };
    let tier = telemetry::permission_tier(
        top.get("approval_policy")
            .map(String::as_str)
            .or(Some("on-request")),
        top.get("sandbox_mode").map(String::as_str),
        top.get("approvals_reviewer").map(String::as_str),
    );
    if let Some(tier) = tier {
        out.choices.insert("approval".into(), tier.into());
    }
    out
}

/// CLI 配置里有没有选 Windows 沙箱实现（与上游 `resolve_windows_sandbox_mode` 同口径：`[windows] sandbox`，
/// 或旧的 features 开关）。解析不了的配置当作「配过」，不去改它（Codex 自己也会报错）。
fn windows_sandbox_configured(content: &str) -> bool {
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return true;
    };
    if doc
        .get("windows")
        .and_then(|windows| windows.get("sandbox"))
        .is_some_and(|sandbox| sandbox.as_str().is_some())
    {
        return true;
    }
    let features = doc.get("features");
    [
        "experimental_windows_sandbox",
        "elevated_windows_sandbox",
        "enable_experimental_windows_sandbox",
    ]
    .iter()
    .any(|key| {
        features
            .and_then(|features| features.get(key))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    })
}

/// 按 Codex 的配置分层看有没有配 Windows 沙箱（做空第五轮 P4）：用户层 `$CODEX_HOME/config.toml`，再加
/// 项目层——从 cwd 往上到项目根（最近的含 `.git` 的目录，没有就是 cwd 本身）每一级的 `.codex/config.toml`，
/// 只认已信任的（用户层 `[projects."<目录>"] trust_level = "trusted"`，目录本身或项目根信任即可；Windows 上
/// 路径不分大小写）。与上游 codex-rs/config/src/loader/mod.rs `discover_project_layers` /
/// `decision_for_dir` 同口径；未信任的项目层上游加载但禁用，这里不算。`$CODEX_HOME/.codex` 跳过（上游同）。
/// 任何一层配了就算「用户配过」，不补参数。
fn windows_sandbox_configured_in_layers(
    home: Option<&std::path::Path>,
    cwd: Option<&std::path::Path>,
) -> bool {
    let user = home.and_then(|home| std::fs::read_to_string(home.join("config.toml")).ok());
    if user.as_deref().is_some_and(windows_sandbox_configured) {
        return true;
    }
    let Some(cwd) = cwd else {
        return false;
    };
    let fold = |path: &std::path::Path| trust_key(path);
    // 已信任的目录：每个信任键记规范路径与原样两种拼写（见 trust_keys），比较时任一相同即可。
    let trusted: std::collections::HashSet<String> = user
        .as_deref()
        .and_then(|content| content.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|doc| {
            doc.get("projects")
                .and_then(|projects| projects.as_table_like())
                .map(|projects| {
                    projects
                        .iter()
                        .filter(|(_, entry)| {
                            entry.get("trust_level").and_then(|level| level.as_str())
                                == Some("trusted")
                        })
                        .flat_map(|(key, _)| trust_keys(std::path::Path::new(key)))
                        .collect()
                })
        })
        .unwrap_or_default();
    let is_trusted =
        |path: &std::path::Path| trust_keys(path).iter().any(|key| trusted.contains(key));
    let root = cwd
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(cwd);
    // git worktree（.git 是文件）按主仓库的信任算（上游 git-utils/src/trust.rs resolve_root_git_project_for_trust，
    // 第六轮复核 L1）：主仓库被信任，worktree 和它的子目录都加载项目层。项目层仍只在 cwd 到 worktree 根之间找。
    let main_root = main_checkout_for_trust(root);
    let root_trusted = is_trusted(root) || main_root.as_deref().is_some_and(is_trusted);
    let home_dot = home.map(fold);
    for dir in cwd.ancestors() {
        let dot = dir.join(".codex");
        let trusted_here = root_trusted || is_trusted(dir);
        if trusted_here && home_dot.as_deref() != Some(fold(&dot).as_str()) {
            if let Ok(content) = std::fs::read_to_string(dot.join("config.toml")) {
                if windows_sandbox_configured(&content) {
                    return true;
                }
            }
        }
        if dir == root {
            break;
        }
    }
    false
}

/// 信任键的比较口径：去掉末尾分隔符；Windows 上分隔符统一成 `\`、再小写（第七轮复核 R2：git 写进 worktree
/// `.git` 文件的 gitdir 是正斜杠 `C:/Users/...`，Codex 记信任键是反斜杠 `c:\users\...`，不统一就永远对不上）。
fn trust_key(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    } else {
        text.trim_end_matches('/').to_string()
    }
}

/// 一个目录用来查信任的各种拼写：规范路径（解开符号链接 / 目录联接、Windows 8.3 短名展开成长名、去掉
/// `\\?\` 前缀）与原样，都按 [`trust_key`] 折叠。照上游 codex-rs/config/src/loader/mod.rs
/// `normalized_project_trust_keys`（dunce::canonicalize 后的与原样的，规范的优先）；规范化失败（目录不存在等）
/// 就只用原样。CI 的临时目录就是这种情况：macOS 的 /var 是 /private/var 的链接，Windows runner 的 TEMP 是
/// RUNNER~1 短名，git 写进 gitdir 的却是解析后的长路径（PR #172 CI）。
fn trust_keys(path: &std::path::Path) -> Vec<String> {
    let original = trust_key(path);
    match std::fs::canonicalize(path) {
        Ok(canonical) => {
            let canonical = trust_key(&strip_verbatim(canonical));
            if canonical == original {
                vec![original]
            } else {
                vec![canonical, original]
            }
        }
        Err(_) => vec![original],
    }
}

/// Windows 的 canonicalize 返回 `\\?\C:\...` 或 `\\?\UNC\server\share\...`：去掉前缀变回常见写法（与
/// dunce::simplified 同取向）。其他平台原样返回。
fn strip_verbatim(path: std::path::PathBuf) -> std::path::PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return std::path::PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return std::path::PathBuf::from(rest);
    }
    path
}

/// worktree 的主仓库根（与上游 `resolve_root_git_project_for_trust` 同口径，只按文件系统核对、不调 git）：
/// `<root>/.git` 是文件、写着 `gitdir: <主仓库>/.git/worktrees/<名字>`；那个目录要在 `worktrees/` 下，它的
/// `gitdir` 文件要指回 `<root>/.git`，`commondir` 要指回主仓库的 `.git`，主仓库根的 `.git` 也要就是这个公共目录。
/// 任何一步对不上都返回 None（不借主仓库的信任）。`.git` 是目录时就是普通仓库，返回它自己。
fn main_checkout_for_trust(root: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    const MAX: u64 = 64 * 1024;
    let read_small = |path: &Path| -> Option<String> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        (meta.is_file() && meta.len() <= MAX)
            .then(|| std::fs::read_to_string(path).ok())
            .flatten()
    };
    let dot_git = root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if meta.is_dir() {
        return Some(root.to_path_buf());
    }
    let gitdir_of = |file: &Path| -> Option<PathBuf> {
        let text = read_small(file)?;
        let target = text.trim().strip_prefix("gitdir:")?.trim().to_string();
        (!target.is_empty()).then(|| file.parent().map(|dir| dir.join(&target)))?
    };
    let git_dir = gitdir_of(&dot_git)?;
    let git_dir_meta = std::fs::symlink_metadata(&git_dir).ok()?;
    if !git_dir_meta.is_dir() {
        return None;
    }
    let canonical = |path: &Path| std::fs::canonicalize(path).ok();
    let canonical_git_dir = canonical(&git_dir)?;
    let worktrees = canonical_git_dir.parent()?;
    if worktrees.file_name()? != "worktrees" {
        return None;
    }
    let common = worktrees.parent()?.to_path_buf();
    let registered = read_small(&canonical_git_dir.join("gitdir"))?;
    let registered = canonical_git_dir.join(registered.trim());
    if registered.file_name()? != ".git" || canonical(registered.parent()?)? != canonical(root)? {
        return None;
    }
    let commondir = read_small(&canonical_git_dir.join("commondir"))?;
    if canonical(&canonical_git_dir.join(commondir.trim()))? != common {
        return None;
    }
    // 信任键沿用 gitdir 里写的主仓库路径（不规范化，与上游「保留原信任键」同取向），再核对它确实拥有公共目录。
    let main_root = git_dir.parent()?.parent()?.parent()?.to_path_buf();
    let main_dot_git = main_root.join(".git");
    let main_git_dir = if std::fs::symlink_metadata(&main_dot_git).ok()?.is_dir() {
        main_dot_git
    } else {
        gitdir_of(&main_dot_git)?
    };
    (canonical(&main_git_dir)? == common).then_some(main_root)
}

fn codex_home(env: &[(String, String)]) -> Option<std::path::PathBuf> {
    env.iter()
        .find(|(key, value)| key == "CODEX_HOME" && !value.is_empty())
        .map(|(_, value)| std::path::PathBuf::from(value))
        .or_else(|| crate::installation(crate::id::CODEX).map(|inst| inst.data_dir))
}

#[cfg(test)]
mod tests {
    /// 真机第四轮 N7：Windows 沙箱配没配，按上游同口径认；没配才补，配过的（任何实现）不覆盖。
    #[test]
    fn windows_sandbox_configured_follows_upstream_keys() {
        use super::windows_sandbox_configured as configured;
        assert!(!configured(""));
        assert!(!configured(
            "model = \"gpt\"\n[projects.'C:/x']\ntrust_level = \"trusted\"\n"
        ));
        assert!(configured("[windows]\nsandbox = \"elevated\"\n"));
        assert!(configured("[windows]\nsandbox = \"unelevated\"\n"));
        assert!(configured("[windows]\nsandbox = \"mxc\"\n"));
        assert!(configured(
            "[features]\nexperimental_windows_sandbox = true\n"
        ));
        assert!(configured("[features]\nelevated_windows_sandbox = true\n"));
        assert!(configured(
            "[features]\nenable_experimental_windows_sandbox = true\n"
        ));
        assert!(!configured(
            "[features]\nexperimental_windows_sandbox = false\n"
        ));
        assert!(configured("this is = = not toml"));
    }

    /// 做空第五轮 P4：项目层 `.codex/config.toml` 配了 Windows 沙箱、且项目已信任时不补；未信任的项目层不算；
    /// 从 cwd 往上到项目根（含 .git 的目录）每一级都看。
    #[test]
    fn project_layer_windows_sandbox_counts_only_when_trusted() {
        use super::windows_sandbox_configured_in_layers as configured;
        let root = std::env::temp_dir().join(format!("meowo-codex-p4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let repo = root.join("repo");
        let sub = repo.join("pkg").join("app");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join(".codex")).unwrap();
        std::fs::write(
            repo.join(".codex").join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        let trust = |dir: &std::path::Path| {
            let key = dir.to_string_lossy().replace('\\', "\\\\");
            std::fs::write(
                home.join("config.toml"),
                format!("[projects.\"{key}\"]\ntrust_level = \"trusted\"\n"),
            )
            .unwrap();
        };
        // 没信任：项目层不算。
        std::fs::write(home.join("config.toml"), "").unwrap();
        assert!(!configured(Some(&home), Some(&sub)));
        // 信任项目根：从子目录启动也算（上游 decision_for_dir 退回项目根的信任）。
        trust(&repo);
        assert!(configured(Some(&home), Some(&sub)));
        assert!(configured(Some(&home), Some(&repo)));
        // 只信任子目录、项目层在根上：子目录本身被信任时根的 .codex 也在扫描路径上，但根不受信任，不算。
        trust(&sub);
        assert!(!configured(Some(&home), Some(&sub)));
        // 子目录自己的 .codex 配了、子目录受信任：算。
        std::fs::create_dir_all(sub.join(".codex")).unwrap();
        std::fs::write(
            sub.join(".codex").join("config.toml"),
            "[windows]\nsandbox = \"unelevated\"\n",
        )
        .unwrap();
        assert!(configured(Some(&home), Some(&sub)));
        // 用户层配了：不看项目层也算；没给 cwd 只看用户层。
        std::fs::write(
            home.join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        assert!(configured(Some(&home), None));
        std::fs::write(home.join("config.toml"), "").unwrap();
        assert!(!configured(Some(&home), None));
        // extra_launch_args 走同一判定。
        let codex = crate::by_id("codex").unwrap();
        trust(&repo);
        let env = vec![(
            "CODEX_HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )];
        let pick = std::collections::HashMap::from([("approval".to_string(), "ask".to_string())]);
        assert!(codex.extra_launch_args(&pick, &env, Some(&repo)).is_empty());
        let other = root.join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(
            codex
                .extra_launch_args(&pick, &env, Some(&other))
                .is_empty(),
            !cfg!(windows)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 第六轮复核 L1：git worktree 按主仓库的信任算。主仓库被信任时，worktree 和它的子目录都认 worktree 里的
    /// `.codex/config.toml`；主仓库不被信任、或 .git 指向的不是登记过的 worktree 时不借主仓库的信任。
    #[test]
    fn worktree_trust_follows_main_checkout() {
        use super::windows_sandbox_configured_in_layers as configured;
        let root = std::env::temp_dir().join(format!("meowo-codex-l1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let main = root.join("main");
        let wt = root.join("wt");
        let sub = wt.join("pkg");
        let admin = main.join(".git").join("worktrees").join("wt");
        for dir in [&home, &admin, &sub] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(main.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        std::fs::write(admin.join("commondir"), "../..\n").unwrap();
        std::fs::create_dir_all(wt.join(".codex")).unwrap();
        std::fs::write(
            wt.join(".codex").join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        let trust = |dir: &std::path::Path| {
            let key = dir.to_string_lossy().replace('\\', "\\\\");
            std::fs::write(
                home.join("config.toml"),
                format!("[projects.\"{key}\"]\ntrust_level = \"trusted\"\n"),
            )
            .unwrap();
        };
        assert_eq!(super::main_checkout_for_trust(&wt), Some(main.clone()));
        assert_eq!(super::main_checkout_for_trust(&main), Some(main.clone()));
        // 主仓库没被信任：不加载。
        std::fs::write(home.join("config.toml"), "").unwrap();
        assert!(!configured(Some(&home), Some(&wt)));
        // 主仓库被信任：worktree 根和子目录都认 worktree 里配的 elevated。
        trust(&main);
        assert!(configured(Some(&home), Some(&wt)));
        assert!(configured(Some(&home), Some(&sub)));
        let codex = crate::by_id("codex").unwrap();
        let env = vec![(
            "CODEX_HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )];
        let pick =
            std::collections::HashMap::from([("approval".to_string(), "approveForMe".to_string())]);
        assert!(codex.extra_launch_args(&pick, &env, Some(&sub)).is_empty());
        // 登记对不上（worktrees/<名字>/gitdir 不指回这个 worktree）：不借主仓库的信任。
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", root.join("other").join(".git").display()),
        )
        .unwrap();
        assert_eq!(super::main_checkout_for_trust(&wt), None);
        assert!(!configured(Some(&home), Some(&wt)));
        // .git 文件指向的不在 worktrees/ 下：同样不借。
        std::fs::write(
            admin.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        let stray = root.join("stray");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", stray.display())).unwrap();
        assert_eq!(super::main_checkout_for_trust(&wt), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 第七轮复核 R2：用 `git worktree add` 建出真实目录（.git 文件里的 gitdir 是 git 写的正斜杠），信任键按
    /// Codex 的写法（Windows 上反斜杠、小写）只给主仓库；worktree 与子目录都要认到项目层的 elevated。
    #[test]
    fn real_git_worktree_with_codex_style_trust_key() {
        use super::windows_sandbox_configured_in_layers as configured;
        let git = |dir: &std::path::Path, args: &[&str]| {
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=probe",
                    "-c",
                    "user.email=probe@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir)
                .output()
                .is_ok_and(|out| out.status.success())
        };
        let root = std::env::temp_dir().join(format!("meowo-codex-r2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let main = root.join("Main");
        let wt = root.join("WT");
        let home = root.join("home");
        std::fs::create_dir_all(&main).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        if !git(&main, &["init", "-q"]) {
            eprintln!("没有 git，跳过");
            return;
        }
        std::fs::write(main.join("a.txt"), "a").unwrap();
        assert!(git(&main, &["add", "a.txt"]));
        assert!(git(&main, &["commit", "-q", "-m", "init"]));
        assert!(git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]));
        let dot_git = std::fs::read_to_string(wt.join(".git")).unwrap();
        if cfg!(windows) {
            assert!(
                dot_git.contains('/') && !dot_git.contains('\\'),
                "git 写的应是正斜杠：{dot_git}"
            );
        }
        let sub = wt.join("pkg");
        std::fs::create_dir_all(sub.join(".codex")).unwrap();
        std::fs::create_dir_all(wt.join(".codex")).unwrap();
        std::fs::write(
            wt.join(".codex").join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        // 比较规范路径：CI 的临时目录可能经链接 / 短名到达，git 写的是解析后的长路径（PR #172 CI）。
        assert!(super::main_checkout_for_trust(&wt)
            .is_some_and(|m| super::trust_keys(&m)[0] == super::trust_keys(&main)[0]));
        // Codex 写信任键的样子：Windows 上反斜杠、小写（照本机 ~/.codex/config.toml）；TOML 单引号字面量。
        let key = if cfg!(windows) {
            main.to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase()
        } else {
            main.to_string_lossy().into_owned()
        };
        std::fs::write(home.join("config.toml"), "").unwrap();
        assert!(!configured(Some(&home), Some(&wt)));
        std::fs::write(
            home.join("config.toml"),
            format!("[projects.'{key}']\ntrust_level = \"trusted\"\n"),
        )
        .unwrap();
        assert!(configured(Some(&home), Some(&wt)));
        assert!(configured(Some(&home), Some(&sub)));
        let codex = crate::by_id("codex").unwrap();
        let env = vec![(
            "CODEX_HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )];
        let pick = std::collections::HashMap::from([("approval".to_string(), "ask".to_string())]);
        assert!(codex.extra_launch_args(&pick, &env, Some(&wt)).is_empty());
        let _ = git(
            &main,
            &["worktree", "remove", "--force", wt.to_str().unwrap()],
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// PR #172 CI：临时目录经别名路径到达（macOS /var → /private/var 的符号链接、Windows runner 的 8.3 短名）。
    /// 这里用目录联接（Windows）/ 符号链接（其他平台）造一个别名根，git 经别名建仓库与 worktree，gitdir 里写的
    /// 是解析后的真实路径；信任键分别用别名拼写和真实拼写，worktree 用别名和真实路径来问，都要认到主仓库的信任。
    #[test]
    fn trust_matches_through_path_aliases() {
        use super::windows_sandbox_configured_in_layers as configured;
        let git = |dir: &std::path::Path, args: &[&str]| {
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=probe",
                    "-c",
                    "user.email=probe@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(dir)
                .output()
                .is_ok_and(|out| out.status.success())
        };
        let base = std::env::temp_dir().join(format!("meowo-codex-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let real = base.join("real");
        let alias = base.join("alias");
        std::fs::create_dir_all(&real).unwrap();
        #[cfg(windows)]
        let linked = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                alias.to_str().unwrap(),
                real.to_str().unwrap(),
            ])
            .output()
            .is_ok_and(|out| out.status.success());
        #[cfg(not(windows))]
        let linked = std::os::unix::fs::symlink(&real, &alias).is_ok();
        if !linked {
            eprintln!("建不了目录别名，跳过");
            let _ = std::fs::remove_dir_all(&base);
            return;
        }
        let main = alias.join("Main");
        let wt = alias.join("WT");
        let home = real.join("home");
        std::fs::create_dir_all(&main).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let cleanup = || {
            #[cfg(windows)]
            let _ = std::fs::remove_dir(&alias); // 只拆联接，不进去删
            #[cfg(not(windows))]
            let _ = std::fs::remove_file(&alias);
            let _ = std::fs::remove_dir_all(&base);
        };
        if !git(&main, &["init", "-q"]) {
            eprintln!("没有 git，跳过");
            cleanup();
            return;
        }
        std::fs::write(main.join("a.txt"), "a").unwrap();
        assert!(git(&main, &["add", "a.txt"]));
        assert!(git(&main, &["commit", "-q", "-m", "init"]));
        assert!(git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]));
        std::fs::create_dir_all(wt.join(".codex")).unwrap();
        std::fs::write(
            wt.join(".codex").join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        // Codex 写信任键的样子：Windows 上反斜杠、小写。
        let codex_key = |path: &std::path::Path| {
            if cfg!(windows) {
                path.to_string_lossy()
                    .replace('/', "\\")
                    .to_ascii_lowercase()
            } else {
                path.to_string_lossy().into_owned()
            }
        };
        for key in [codex_key(&main), codex_key(&real.join("Main"))] {
            std::fs::write(
                home.join("config.toml"),
                format!("[projects.'{key}']\ntrust_level = \"trusted\"\n"),
            )
            .unwrap();
            assert!(
                configured(Some(&home), Some(&wt)),
                "别名路径的 worktree，信任键 {key}"
            );
            assert!(
                configured(Some(&home), Some(&real.join("WT"))),
                "真实路径的 worktree，信任键 {key}"
            );
        }
        std::fs::write(home.join("config.toml"), "").unwrap();
        assert!(!configured(Some(&home), Some(&wt)), "没信任就不读项目层");
        let _ = git(
            &main,
            &["worktree", "remove", "--force", wt.to_str().unwrap()],
        );
        cleanup();
    }

    /// 第七轮复核 R2 真机：本机真实 worktree（默认 meowo-tui-codex，`MEOWO_R2_WORKTREE` 可改）。隔离的 CODEX_HOME 里
    /// 只放主仓库的信任键（照本机写法：反斜杠、小写），worktree 项目层临时放一份配了 [windows] sandbox 的
    /// .codex/config.toml，跑完删掉。`MEOWO_R2_OUT` 给出时把结果写成 JSON。不碰用户的 ~/.codex。
    #[test]
    #[ignore = "第七轮复核 R2：本机真实 worktree；手动跑"]
    fn r2_real_worktree_on_this_machine() {
        let wt = std::path::PathBuf::from(
            std::env::var("MEOWO_R2_WORKTREE")
                .unwrap_or_else(|_| r"C:\Users\35122\Desktop\workspace\meowo-tui-codex".into()),
        );
        let dot_git =
            std::fs::read_to_string(wt.join(".git")).expect("应是 worktree（.git 是文件）");
        let main = super::main_checkout_for_trust(&wt).expect("认得出主仓库");
        let key = super::trust_key(&main);
        let home = std::env::temp_dir().join(format!("meowo-codex-r2-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!("[projects.'{key}']\ntrust_level = \"trusted\"\n"),
        )
        .unwrap();
        let dot_codex = wt.join(".codex");
        let created_dir = !dot_codex.exists();
        let config = dot_codex.join("config.toml");
        assert!(
            !config.exists(),
            "worktree 里已有 .codex/config.toml，不覆盖：{}",
            config.display()
        );
        std::fs::create_dir_all(&dot_codex).unwrap();
        std::fs::write(&config, "[windows]\nsandbox = \"elevated\"\n").unwrap();
        let codex = crate::by_id("codex").unwrap();
        let env = vec![(
            "CODEX_HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )];
        let pick = std::collections::HashMap::from([("approval".to_string(), "ask".to_string())]);
        let with_layer = codex.extra_launch_args(&pick, &env, Some(&wt));
        let sub = wt.join("app");
        let with_layer_sub = codex.extra_launch_args(&pick, &env, Some(&sub));
        // 对照：项目层拿掉后应补参数（说明是项目层让它不补，而不是别的原因）。
        let _ = std::fs::remove_file(&config);
        if created_dir {
            let _ = std::fs::remove_dir(&dot_codex);
        }
        let without_layer = codex.extra_launch_args(&pick, &env, Some(&wt));
        let _ = std::fs::remove_dir_all(&home);
        let out = serde_json::json!({
            "worktree": wt, "dot_git": dot_git.trim(), "main_checkout": main, "trust_key_in_isolated_home": key,
            "extra_args_with_project_layer": with_layer, "extra_args_with_project_layer_from_subdir": with_layer_sub,
            "extra_args_after_removing_layer": without_layer,
            "cleanup": {"config_removed": !config.exists(), "dot_codex_removed_if_created": !created_dir || !dot_codex.exists(), "home_removed": !home.exists()},
        });
        if let Ok(path) = std::env::var("MEOWO_R2_OUT") {
            std::fs::write(path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
        }
        assert!(with_layer.is_empty() && with_layer_sub.is_empty(), "{out}");
        assert_eq!(without_layer.is_empty(), !cfg!(windows), "{out}");
        assert!(!config.exists());
    }

    /// workspace-write 的档（必要时询问、自动审查）在没配 Windows 沙箱时补 `-c windows.sandbox=unelevated`；
    /// 其他档、配过的都不动。
    #[test]
    fn workspace_write_tiers_add_unelevated_sandbox_only_when_unconfigured() {
        let codex = crate::by_id("codex").unwrap();
        let home = std::env::temp_dir().join(format!("meowo-codex-n7-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let env = vec![(
            "CODEX_HOME".to_string(),
            home.to_string_lossy().into_owned(),
        )];
        let pick = |tier: &str| {
            std::collections::HashMap::from([("approval".to_string(), tier.to_string())])
        };
        let expect: Vec<String> = if cfg!(windows) {
            vec!["-c".into(), "windows.sandbox=unelevated".into()]
        } else {
            Vec::new()
        };
        // 没有 config.toml：没配。
        assert_eq!(
            codex.extra_launch_args(&pick("approveForMe"), &env, None),
            expect
        );
        std::fs::write(
            home.join("config.toml"),
            "check_for_update_on_startup = false\n",
        )
        .unwrap();
        assert_eq!(
            codex.extra_launch_args(&pick("approveForMe"), &env, None),
            expect
        );
        assert_eq!(codex.extra_launch_args(&pick("ask"), &env, None), expect);
        for other in ["default", "readOnly", "yolo", "nonsense"] {
            assert!(
                codex.extra_launch_args(&pick(other), &env, None).is_empty(),
                "{other}"
            );
        }
        assert!(codex
            .extra_launch_args(&std::collections::HashMap::new(), &env, None)
            .is_empty());
        std::fs::write(
            home.join("config.toml"),
            "[windows]\nsandbox = \"elevated\"\n",
        )
        .unwrap();
        assert!(codex
            .extra_launch_args(&pick("approveForMe"), &env, None)
            .is_empty());
        assert!(codex.extra_launch_args(&pick("ask"), &env, None).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// 样本形制取自本机 codex 0.163 的 models_cache.json（描述截短）。
    #[test]
    fn models_cache_lists_visible_models_with_reasoning_levels() {
        let content = r#"{"fetched_at":"x","models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1-Sol","visibility":"list",
             "supported_reasoning_levels":[{"effort":"low","description":"a"},{"effort":"max","description":"b"}]},
            {"slug":"codex-auto-review","display_name":"Codex Auto Review","visibility":"hide"},
            {"slug":"gpt-5.6-luna","visibility":"list"}
        ]}"#;
        let catalog = parse_models_cache(content);
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog[0].label, "GPT-6.1-Sol");
        assert_eq!(catalog[0].efforts, vec!["low", "max"]);
        assert_eq!(catalog[1].label, "gpt-5.6-luna");
        assert!(parse_models_cache("not json").is_empty());
    }

    /// `codex debug models --bundled` 前面会夹一行 WARNING（隔离 CODEX_HOME 实测 0.167.2）。
    #[test]
    fn debug_models_output_skips_leading_warning_and_reads_default_effort() {
        let output = "WARNING: proceeding, even though we could not create PATH aliases\n{\"models\":[{\"slug\":\"gpt-6-astra\",\"display_name\":\"GPT-6-Astra\",\"default_reasoning_level\":\"low\",\"supported_reasoning_levels\":[{\"effort\":\"low\"}],\"visibility\":\"list\"}]}";
        let catalog = super::parse_debug_models(output);
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].default_effort.as_deref(), Some("low"));
    }

    #[test]
    fn config_defaults_map_to_permission_tiers() {
        let defaults = super::parse_config_defaults(
            "model = \"gpt-5.5\"\napproval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\n[profiles.x]\nmodel = \"other\"\n",
        );
        assert_eq!(defaults.model.as_deref(), Some("gpt-5.5"));
        assert_eq!(
            defaults.choices.get("approval").map(String::as_str),
            Some("yolo")
        );
        let ro = super::parse_config_defaults("sandbox_mode = \"read-only\"\n");
        assert_eq!(
            ro.choices.get("approval").map(String::as_str),
            Some("readOnly")
        );
        assert!(super::parse_config_defaults("").choices.is_empty());
    }

    use super::*;

    /// codex 0.166.0 信任屏原文（codex.exe 内嵌字符串，research-cli-drift X1）。
    #[test]
    fn startup_markers_cover_trust_folder_screen_old_and_new() {
        let screen = "Trust this folder? Codex can read, edit, and run files here, subject to \
                      your permission settings."
            .to_lowercase();
        let markers = Codex.startup_attention_markers();
        assert!(markers.iter().any(|m| screen.contains(m)));
        // 旧版（< 0.16x）标题仍认
        let old = "Do you trust the contents of this directory?".to_lowercase();
        assert!(markers.iter().any(|m| old.contains(m)));
        // 普通对话里提到这几个词不算
        let chat = "you can trust this folder layout".to_lowercase();
        assert!(!markers.iter().any(|m| chat.contains(m)));
    }

    #[test]
    fn writes_session_token_before_first_prompt() {
        assert!(Codex.writes_tab_token());
        assert!(!Codex.sets_terminal_tab_title());
    }

    #[test]
    fn app_server_is_shared_even_when_embedded_but_tui_is_private() {
        use crate::registry::ProcessScope;
        let scope = |args: &[&str]| {
            Codex.process_scope(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(scope(&[]), ProcessScope::Unknown);
        assert_eq!(scope(&["unrelated.exe"]), ProcessScope::Unknown);
        assert_eq!(
            scope(&["codex.exe", "--remote", "unix://test"]),
            ProcessScope::Shared
        );
        assert_eq!(
            scope(&["codex.exe", "app-server", "--stdio"]),
            ProcessScope::Shared
        );
        assert_eq!(
            scope(&["codex.exe", "app-server", "--listen", "unix://test"]),
            ProcessScope::Shared
        );
        assert_eq!(
            scope(&["codex.exe", "--no-daemon", "resume", "thread"]),
            ProcessScope::Exclusive
        );
    }

    #[test]
    fn permission_hook_waits_for_gui_decision() {
        assert_eq!(
            EVENTS
                .iter()
                .find(|e| e.name == "PermissionRequest")
                .unwrap()
                .timeout,
            310
        );
        assert!(EVENTS
            .iter()
            .filter(|e| !matches!(e.name, "PermissionRequest" | "SessionEnd" | "Interrupt"))
            .all(|e| e.timeout == 5));
    }

    #[test]
    fn config_and_credentials_sit_under_data_dir() {
        let home = std::env::temp_dir().join(format!("meowo-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".codex")).unwrap();

        let v = &VARIANTS[0];
        let dir = v
            .data_dir
            .candidates
            .iter()
            .map(|c| home.join(c))
            .find(|p| p.is_dir())
            .unwrap();
        let inst = v.installation_at(id::CODEX, dir, Some(&home));
        assert_eq!(inst.config_path(), home.join(".codex").join("hooks.json"));
        assert_eq!(
            inst.credentials_path(),
            Some(home.join(".codex").join("auth.json"))
        );
        assert!(inst.is_configured());
        // 三处候选都没有 → 回退裸名。
        assert_eq!(
            inst.launch_argv(),
            LAUNCH
                .probe(Some(&inst.data_dir), Some(&home))
                .unwrap_or_else(|| vec!["codex".into()])
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn standalone_install_under_data_dir_is_found() {
        // 「装完仍显示未安装」的回归：PATH 是旧快照，但独立安装的固定路径能直查到。
        let home =
            std::env::temp_dir().join(format!("meowo-codex-standalone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let bin = home
            .join(".codex")
            .join("packages")
            .join("standalone")
            .join("current")
            .join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(crate::exe_file_name("codex"));
        std::fs::write(&exe, b"").unwrap();

        let v = &VARIANTS[0];
        let inst = v.installation_at(id::CODEX, home.join(".codex"), Some(&home));
        assert!(inst.is_launchable());
        assert_eq!(inst.launch_argv(), vec![exe.to_string_lossy().into_owned()]);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bun_global_beats_standalone() {
        let home = std::env::temp_dir().join(format!("meowo-codex-bun-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let name = crate::exe_file_name("codex");
        for sub in [".codex/packages/standalone/current/bin", ".bun/bin"] {
            let d = crate::join_rel(&home, sub);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(&name), b"").unwrap();
        }
        let inst = VARIANTS[0].installation_at(id::CODEX, home.join(".codex"), Some(&home));
        assert_eq!(
            inst.launch_argv(),
            vec![home
                .join(".bun")
                .join("bin")
                .join(&name)
                .to_string_lossy()
                .into_owned()]
        );

        let _ = std::fs::remove_dir_all(&home);
    }
}
