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
mod lineage;
mod screen;
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

impl AgentPlugin for Codex {
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
        let home = env
            .iter()
            .find(|(key, value)| key == "CODEX_HOME" && !value.is_empty())
            .map(|(_, value)| std::path::PathBuf::from(value))
            .or_else(|| crate::installation(crate::id::CODEX).map(|inst| inst.data_dir));
        home.and_then(|home| std::fs::read_to_string(home.join("models_cache.json")).ok())
            .map(|content| parse_models_cache(&content))
            .unwrap_or_default()
    }
    /// `/model` 是交互式菜单（不接受内联参数，故无 model_presets）。声明它，GUI 就能
    /// 发出这条命令再把弹出的菜单渲染成按钮——模型清单由 CLI 现给。
    fn model_menu_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    /// Esc 中断当前回合:codex 状态行自述 "esc to interrupt"(真机 capture 在档,
    /// 见 docs/research/tui-menu-captures-2026-07.md)。排队键 Tab 在空闲时同样直接提交
    /// (probe_tui_script,0.162.1,2026-10-16),运行态误判也不会卡住消息。
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
    /// Ctrl+J（裸 LF）在 composer 里换行、不提交（tests/probe_tui_script.rs，0.162.1，
    /// 2026-10-16）。Alt+Enter 同样可用，但取不带 ESC 前缀的那个——crossterm 拆包时 ESC 可能
    /// 被单独认成 Esc，运行中就是一次中断。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\n")
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
    /// 启动选项（实测 `codex --help`）：审批/沙箱形态。模型**不声明**——codex 的模型名随
    /// 版本频繁更迭，声明一份很快就过期的清单等于给出会启动失败的选项。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        use crate::{LaunchChoice, LaunchOption};
        static OPTIONS: [LaunchOption; 1] = [LaunchOption {
            id: "approval",
            default: "default",
            choices: &[
                LaunchChoice {
                    id: "default",
                    label: "Default",
                    args: &[],
                    risk: false,
                },
                LaunchChoice {
                    id: "readOnly",
                    label: "Read Only",
                    args: &["--sandbox", "read-only"],
                    risk: false,
                },
                LaunchChoice {
                    id: "fullAuto",
                    label: "Full Auto",
                    // `--full-auto` 在 codex 0.16x 已移除（实测 0.162.1 `--help` 无此项，
                    // 旧会话按残留选项启动即报 unexpected argument 直接退出）——它就是
                    // `-a never -s workspace-write` 的速记，拆开写等价且新旧通吃。
                    args: &[
                        "--ask-for-approval",
                        "never",
                        "--sandbox",
                        "workspace-write",
                    ],
                    risk: false,
                },
                LaunchChoice {
                    id: "yolo",
                    label: "YOLO",
                    args: &["--dangerously-bypass-approvals-and-sandbox"],
                    risk: true,
                },
            ],
        }];
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
            Some(crate::ModelCatalogEntry {
                id,
                label,
                group: None,
                efforts,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
