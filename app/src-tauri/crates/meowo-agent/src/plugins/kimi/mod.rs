//! kimi 插件。两个变体——这正是变体层存在的理由：
//!
//! | tag | 发行 | 数据目录 | hooks 默认形态 | 可执行 |
//! |---|---|---|---|---|
//! | `modern` | Node 版 **Kimi Code** | `~/.kimi-code` | 无顶层 `hooks` 键 | `<data>/bin/kimi` |
//! | `legacy` | 旧 Python 版 **kimi-cli** | `~/.kimi` | `hooks = []` 空内联数组 | `~/.local/bin/kimi` 等 |
//!
//! 两者共用 TOML 格式与 stdin 载荷；事件集按实现区分，不能把新版事件写入旧 Python 配置。

pub mod account;
mod cli_text;
mod lead;
mod screen;
pub mod telemetry;

use crate::{
    auth::{AuthScheme, CredentialSource, OAuthRefresh},
    caps::TelemetryCap,
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig, RepairReason},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 接线事件集。PermissionRequest = kimi 交互式等待用户审批前触发（官方源码确认，observation-only），
/// 用于卡片「待交互」显示。PreCompact/PostCompact 是压缩进行期间的「正在压缩」指示通道
/// （transcript 在压缩期间零新增字节，只能靠 hook；白名单 EVENT_WHITELIST 已含这两个名字，
/// kimi 0.29 二进制确认支持）。
static EVENTS: [HookEvent; 11] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    HookEvent::plain("StopFailure"),
    HookEvent::plain("Interrupt"),
    HookEvent::plain("SessionEnd"),
    HookEvent::plain("PermissionRequest"),
    HookEvent::plain("PermissionResult"),
    HookEvent::plain("PreCompact"),
    HookEvent::plain("PostCompact"),
];

/// kimi 0.20 支持的全部 hook 事件（HOOK_EVENT_TYPES）。一条非法 event 会让 kimi **静默禁用全部**
/// hooks（源码 salvageConfigData），故 EVENTS 有针对本表的绊线测试。
pub const EVENT_WHITELIST: [&str; 16] = [
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionResult",
    "UserPromptSubmit",
    "Stop",
    "StopFailure",
    "Interrupt",
    "SessionStart",
    "SessionEnd",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "Notification",
];

/// config.toml 由 `kimi login` 生成——不存在即「需先登录」，不凭空创建。
/// command 不加引号：与 kimi 现存配置的书写形态一致，避免无谓改写用户在用的条目
///（reporter 路径含空白时仍强制加引号，否则执行与认领同按空白截断，见 `CommandSpec::quote_exe`）。
static HOOKS: HookSpec = HookSpec {
    config_rel: "config.toml",
    format: ConfigFormat::KimiToml,
    missing: MissingConfig::Fail(RepairReason::NeedLogin),
    events: &EVENTS,
    command: CommandSpec {
        quote_exe: false,
        with_provider: true,
        ps_call_operator: false,
    },
};

// Python kimi-cli 官方 hooks 表只有 13 类事件，不能把 Node 版的审批/中断事件写过去导致配置失败。
static LEGACY_EVENTS: [HookEvent; 8] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    HookEvent::plain("StopFailure"),
    HookEvent::plain("SessionEnd"),
    HookEvent::plain("PreCompact"),
    HookEvent::plain("PostCompact"),
];
static LEGACY_HOOKS: HookSpec = HookSpec {
    events: &LEGACY_EVENTS,
    ..HOOKS
};

/// 来源：kimi-code 开源包 `packages/oauth/src/constants.ts`。
/// 多账号：`KIMI_CODE_HOME` 一个变量搬走整个数据目录（凭据、config.toml、会话 wire.jsonl 全在里面）。
///
/// **绝不能是 `KIMI_SHARE_DIR`**（此前就是它，等于多账号完全没隔离）：在 kimi-code 0.40.1 的
/// 二进制里，`KIMI_SHARE_DIR` 的含义是「旧版 kimi-cli 的**迁移来源**目录」，不是数据目录——
/// `resolveLegacySourceHome(env, home, cwd)` 拿它当 `~/.kimi` 的替代来源，而数据根另有其人：
/// `resolveKimiHome(homeDir) { return homeDir ?? process.env["KIMI_CODE_HOME"] ?? join(homedir(), ".kimi-code") }`。
///
/// 实测（0.40.1，本机）：`KIMI_SHARE_DIR=<空目录> kimi session list` 照样列出真实 `~/.kimi-code`
/// 里的会话、日志也仍写在那儿；换成 `KIMI_CODE_HOME=<空目录>` 才只列出该目录里的会话。
/// 症状因此是最坏的那种「看起来隔离了、其实没有」：所有 kimi 账号共用 `~/.kimi-code`，凭据互相
/// 覆盖、会话历史混在一起，连预写工作区信任与 reporter 的 transcript 查找也一并落空。
///
/// profile 恒取 `variants().first()`（＝modern），故这里只声明 modern 的变量；legacy 的
/// Python kimi-cli 走不到 profile 这条路。
static PROFILE: crate::profile::ProfileSpec = crate::profile::ProfileSpec {
    envs: &[("KIMI_CODE_HOME", "")],
    data_rel: "",
    creds_rel: "credentials/kimi-code.json",
};

/// kimi 的会话可跨账号继续：会话目录整棵搬进目标账号的数据根即可，**不必**改
/// `session_index.jsonl`——kimi 自己会扫 `sessions/**`。
///
/// 实测（kimi-code 0.40.1，本机 2026-09-08）：新建一个空 `KIMI_CODE_HOME`，只把
/// `sessions/<wd_工作区>/<session-id>/` 整个目录拷进去（没有 session_index.jsonl、没有凭据）——
/// - `kimi session list` 列出了它；
/// - `kimi -r <该 id> -p x` 报的是 `no provider configured`（＝会话已找到，卡在没登录），
///   而同一个目录里换一个不存在的 id 报的是 `Session "…" not found`。两条错误分得开，
///   证明会话查找纯靠拷进去的那棵目录。
///
/// 搬的是整棵会话目录而不是单个 `wire.jsonl`：正文旁边还有 blobs（用户贴的图）与各 agent 的
/// 侧车，单搬正文会造出一个 kimi 自己读不全的半份副本。
static CROSS_ACCOUNT: crate::profile::CrossAccountSession = crate::profile::CrossAccountSession {
    // `<root>/sessions/<wd_工作区>/<session-id>/agents/main/wire.jsonl`
    transcript_depth: 6,
    // 正文往上第 3 级就是 `<session-id>/`（wire.jsonl → main → agents → 会话目录）。
    session_dir_up: 3,
    session_buckets: &[],
    subagents_beside_transcript: false,
};

const AUTH_MODERN: AuthScheme = AuthScheme {
    credentials: CredentialSource::File("credentials/kimi-code.json"),
    refresh: Some(OAuthRefresh {
        token_url: "https://auth.kimi.com/api/oauth/token",
        client_id: "17e5f671-d194-4dfb-9706-5516cb48c098",
    }),
    default_base_url: "https://api.kimi.com/coding/v1",
    // `kimi login`。config.toml 正是由它生成——故 MissingConfig::Fail(NeedLogin) 的提示可直接
    // 引导用户点登录（两者指向同一个动作）。
    login: Some(&["login"]),
    // 当前 kimi-code CLI 没有 logout 子命令；宿主只删除下方声明的凭据文件。
    logout_args: &[],
};

/// 旧 Python 版的凭据布局与新版相同（实测 `~/.kimi/credentials/kimi-code.json` 字段一致）。
/// **client_id 未经证实**：若刷新 token 返回 `invalid_client`，就把这里换成旧版的值——
/// 变体层的意义正在于此，届时只改这一个 const，account 侧无需再动。
const AUTH_LEGACY: AuthScheme = AUTH_MODERN;

static LAUNCH_MODERN: LaunchSpec = LaunchSpec {
    stem: "kimi",
    candidates: &[
        LaunchCandidate::Exe {
            root: Root::DataDir,
            sub: "bin",
        },
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".kimi-code/bin",
        },
    ],
};

/// 旧版常经 uv/pipx 装到 `~/.local/bin`，不在数据目录下。
static LAUNCH_LEGACY: LaunchSpec = LaunchSpec {
    stem: "kimi",
    candidates: &[
        LaunchCandidate::Exe {
            root: Root::DataDir,
            sub: "bin",
        },
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".kimi/bin",
        },
        LaunchCandidate::Exe {
            root: Root::Home,
            sub: ".local/bin",
        },
    ],
};

/// 0.40.x 的工作区信任名册：`<数据目录>/workspace-trust/<wd_key>`，读端只看文件是否存在。取证与
/// key 算法见 `docs/research/kimi-workspace-trust-2026-09.md`。旧 Python 版无此机制。
static TRUST: crate::trust::WorkspaceTrustSpec = crate::trust::WorkspaceTrustSpec {
    dir_rel: "workspace-trust",
};

static VARIANTS: [Variant; 2] = [
    Variant {
        tag: "modern",
        data_dir: DataDirSpec {
            // 与 PROFILE 注入的变量必须是同一个（见那儿的实测注释）：reporter 作为 kimi 的
            // hook 子进程继承这套环境，靠它解析出**同一个**数据根，否则 profile 会话的
            // transcript/上下文/改名全部静默落空。legacy 变体沿用 KIMI_SHARE_DIR 不动——
            // 那是旧 Python 版的世界，本机无从取证，且 profile 走不到它。
            env: Some("KIMI_CODE_HOME"),
            env_suffix: "",
            candidates: &[".kimi-code"],
        },
        hooks: &HOOKS,
        auth: Some(&AUTH_MODERN),
        launch: &LAUNCH_MODERN,
        trust: Some(&TRUST),
    },
    Variant {
        tag: "legacy",
        data_dir: DataDirSpec {
            env: Some("KIMI_SHARE_DIR"),
            env_suffix: "",
            candidates: &[".kimi"],
        },
        hooks: &LEGACY_HOOKS,
        auth: Some(&AUTH_LEGACY),
        launch: &LAUNCH_LEGACY,
        // 旧 Python 版没有工作区信任这道确认屏。
        trust: None,
    },
];

pub struct Kimi;

/// kimi 的 config.toml 没有 proxy 键、也没有环境变量注入机制（`[providers.*.env]` 只作 api_key /
/// base_url 的 fallback），故只能靠进程环境变量。
///
/// **三家里唯一支持 SOCKS 的**：新版 kimi-code 自 v0.12.0 起认全套
/// `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY`，含 socks5。按官方文档，SOCKS 通常经
/// `ALL_PROXY` 配置——写进 `HTTPS_PROXY` 未必被识别，故 socks 单列一组键。
///
/// 注意旧的 Python 版 kimi-cli 连环境变量都不认（aiohttp `trust_env=False`，修复 PR 未合）——
/// 那个版本无论怎么配都走不了代理，这里无从区分（变体表按目录形态分，不按 CLI 实现语言分）。
static PROXY: crate::proxy::ProxySpec = crate::proxy::ProxySpec {
    socks: true,
    config_env: false,
    http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
    socks_keys: &["ALL_PROXY"],
};

struct KimiRelay;
static RELAY: KimiRelay = KimiRelay;
static RELAY_AUTH: [crate::RelayOption; 1] = [crate::RelayOption {
    value: "bearer",
    label: "Bearer Token",
}];
static RELAY_PROTOCOLS: [crate::RelayOption; 3] = [
    crate::RelayOption {
        value: "kimi",
        label: "Kimi",
    },
    crate::RelayOption {
        value: "anthropic",
        label: "Anthropic Messages",
    },
    crate::RelayOption {
        value: "openai",
        label: "OpenAI Chat Completions",
    },
];
static RELAY_SUGGESTIONS: [crate::RelaySuggestionGroup; 3] = [
    crate::RelaySuggestionGroup {
        protocol: "kimi",
        models: &["kimi-for-coding", "kimi-for-coding-highspeed"],
    },
    crate::RelaySuggestionGroup {
        protocol: "anthropic",
        models: &["claude-fable-5", "claude-opus-4-8", "claude-sonnet-5"],
    },
    crate::RelaySuggestionGroup {
        protocol: "openai",
        models: &["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.4", "gpt-5.3-codex"],
    },
];

impl crate::RelayCap for KimiRelay {
    fn ui(&self) -> crate::RelayUi {
        crate::RelayUi {
            protocols: &RELAY_PROTOCOLS,
            auth_modes: &RELAY_AUTH,
            default_protocol: "kimi",
            default_auth: "bearer",
            suggestions: &RELAY_SUGGESTIONS,
            env_options: &[],
        }
    }
    fn supports_variant(&self, variant_tag: &str) -> bool {
        variant_tag != "legacy"
    }
    fn launch_env(&self, config: crate::RelayConfig<'_>, key: &str) -> Vec<(String, String)> {
        vec![
            ("KIMI_MODEL_NAME".into(), config.model.trim().into()),
            ("KIMI_MODEL_API_KEY".into(), key.into()),
            (
                "KIMI_MODEL_BASE_URL".into(),
                config.base_url.trim().trim_end_matches('/').into(),
            ),
            ("KIMI_MODEL_PROVIDER_TYPE".into(), config.protocol.into()),
        ]
    }
    fn augment_argv(
        &self,
        _config: crate::RelayConfig<'_>,
        _has_secret: bool,
        argv: Vec<String>,
    ) -> Vec<String> {
        argv
    }
    fn model_request(&self, config: crate::RelayConfig<'_>) -> crate::RelayModelRequest {
        crate::RelayModelRequest {
            auth: if config.protocol == "anthropic" {
                crate::RelayModelAuth::ApiKey
            } else {
                crate::RelayModelAuth::Bearer
            },
            anthropic_version: config.protocol == "anthropic",
        }
    }
}

fn modern_launch_options() -> &'static [crate::LaunchOption] {
    use crate::{Ask, LaunchChoice, LaunchOption, ModeSwitch, ModeTraits};
    // 统一说法依据 Kimi 2.6 权限策略链（kimi.exe 内嵌源码 K:327623 起）与 /permission 菜单原文，
    // 见 research-kimi-gemini §5。`--yolo` 是「常规自动、危险仍问」，`--auto` 才是「都不问」。
    static OPTIONS: [LaunchOption; 2] = [
        LaunchOption {
            id: "permission",
            // 「manual」与 wire 里 permission.set_mode 的值同名，会话里的当前档直接对得上。
            // 旧会话存的 "default" 是未知 id，按声明表落回这一档（同样不带参数），行为不变。
            default: "manual",
            choices: &[
                LaunchChoice {
                    id: "manual",
                    label: "Always Ask",
                    args: &[],
                    risk: false,
                    // 「Auto-read only; everything else needs your approval first.」
                    traits: Some(ModeTraits::new(Ask::Always, Ask::Always, false)),
                    desc: Some("kimi.permission.manual"),
                    server: None,
                },
                LaunchChoice {
                    id: "yolo",
                    label: "Ask When Needed",
                    args: &["--yolo"],
                    risk: false,
                    // 「Routine edits and commands run automatically; risky actions, questions,
                    // and plans still ask.」
                    traits: Some(ModeTraits::new(Ask::Risky, Ask::Risky, false)),
                    desc: Some("kimi.permission.yolo"),
                    server: None,
                },
                LaunchChoice {
                    id: "auto",
                    label: "Never Ask",
                    args: &["--auto"],
                    // 「Everything runs and is decided automatically; only plans in Plan mode
                    // wait for your review.」
                    risk: true,
                    traits: Some(ModeTraits::new(Ask::Never, Ask::Never, false)),
                    desc: Some("kimi.permission.auto"),
                    server: None,
                },
            ],
            // 会话里改权限档：空闲时 `-r <id> --yolo|--auto` 重启接回。
            switch: ModeSwitch::Restart,
            // 2.6 接回时会话记住的档优先，参数只能放宽（K:558700），收紧到 Always Ask 做不到。
            resume_upgrade_only: true,
            in_session_only: false,
        },
        LaunchOption {
            id: "work",
            default: "default",
            choices: &[
                LaunchChoice {
                    id: "default",
                    label: "Default",
                    args: &[],
                    risk: false,
                    traits: None,
                    desc: Some("kimi.work.default"),
                    server: None,
                },
                LaunchChoice {
                    id: "plan",
                    label: "Plan",
                    args: &["--plan"],
                    risk: false,
                    // plan 只硬禁写文件（计划文件除外，K:268495 guardToolExecution），命令按权限档。
                    traits: Some(ModeTraits::new(Ask::Blocked, Ask::Inherit, true)),
                    desc: Some("kimi.work.plan"),
                    server: None,
                },
            ],
            // 会话里用 `/plan on|off`（带参即生效，mode_controls 的 work 维度）。
            switch: ModeSwitch::Control,
            resume_upgrade_only: true,
            in_session_only: false,
        },
    ];
    &OPTIONS
}

static BROKER_QUESTION: crate::caps::BrokerQuestionCap = crate::caps::BrokerQuestionCap;

/// Kimi `config.toml` 顶层（第一个表头之前）的一个字符串 / 布尔键。
fn top_level_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            let v = v.trim();
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

/// 新建页默认值（K:558520 新建默认值：`default_permission_mode` 缺省 manual，旧字段 `yolo = true`
/// 迁移成 yolo；`default_plan_mode = true` 开 plan，写法按 schema 推断，未实测）。
fn parse_launch_defaults(content: &str) -> crate::LaunchDefaults {
    let mut out = crate::LaunchDefaults {
        model: top_level_value(content, "default_model").filter(|m| !m.is_empty()),
        ..Default::default()
    };
    let permission = top_level_value(content, "default_permission_mode").or_else(|| {
        (top_level_value(content, "yolo").as_deref() == Some("true")).then(|| "yolo".to_string())
    });
    if let Some(mode) = permission.filter(|m| matches!(m.as_str(), "manual" | "yolo" | "auto")) {
        out.choices.insert("permission".into(), mode);
    }
    if top_level_value(content, "default_plan_mode").as_deref() == Some("true") {
        out.choices.insert("work".into(), "plan".into());
    }
    out
}

impl AgentPlugin for Kimi {
    fn process_scope(&self, argv: &[String]) -> crate::registry::ProcessScope {
        use crate::registry::ProcessScope;
        let own = argv.first().is_some_and(|arg| {
            matches!(
                arg.replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "kimi" | "kimi.exe" | "kimi.cmd"
            )
        });
        if !own {
            return ProcessScope::Unknown;
        }
        // 服务可承载多个会话；即使参数位置不确定，也不能拿它当单会话进程结束。
        if argv
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "web" | "server" | "rc" | "remote" | "acp"))
        {
            ProcessScope::Shared
        } else {
            ProcessScope::Exclusive
        }
    }

    fn broker_question(&self) -> Option<&'static crate::caps::BrokerQuestionCap> {
        Some(&BROKER_QUESTION)
    }

    fn screen_rules(&self) -> &'static [crate::screen::ScreenRule] {
        screen::RULES
    }

    /// 审批面板（ApprovalPanel；形态与按键语义为官方源码取证，见
    /// `apps/kimi-code/src/tui/components/dialogs/approval-panel.ts @ 0.29）。
    ///
    /// kimi 的 `PermissionRequest` hook 是 observation-only（`permission_hook_decides`
    /// 为 false），broker 审批桥对它关闭——屏幕识别是 GUI 给出**可操作**审批的唯一通道。
    /// 按键提示行是面板的稳定签名；提示行被裁掉时退回标题行。
    fn attention_patterns(&self) -> &'static [crate::chat_ui::AttentionPattern] {
        &[
            // 提问表单（AskUserQuestion，kimi 2.4.1 真机取证 2026-11-05）。底部按键提示行是签名，
            // 三种页各一种：单选题 `1-3 / ↵ choose`、多选题 `1-4 / ↵ toggle`、确认页
            // `1/2 choose  ↵ confirm`，后面都跟 `←/→/tab switch`（多页签表单独有）。注意和审批面板
            // 的提示行不同：审批是 `↑/↓ select · … · ↵ confirm`（带斜杠与圆点），不会串。
            // 此前 kimi 只声明了审批，提问在对话页不出卡片（用户实拍）。
            crate::chat_ui::AttentionPattern {
                id: "kimi:question",
                patterns: &[
                    r"↑↓ select\s+(?:1-\d+ / ↵ (?:choose|toggle)|1/2 choose\s+↵ confirm)\s+←/→/tab switch",
                ],
                // kimi 重绘不清屏，上一页的表单残留在上方：取最后一次。
                last: true,
                details: crate::chat_ui::AttentionDetails::BracketForm,
                kind: crate::chat_ui::AttentionKind::Question,
                confirms: None,
                // 自定义答案项 `Other`（表单恒在末尾追加，2.6.1 程序文件 `DEFAULT_OTHER_LABEL`）：按它的数字键
                // 进入编辑，这一行变成 `→ [3] Other:`，题干下方出现 `Type your answer, then press Enter to save.`，
                // 页脚换成 `type answer  ↵ save  tab switch  esc cancel`；打字后回车存下答案，表单跳到确认页
                // （`[1] Submit` / `[2] Cancel`，由提问卡照常接手）。单选题真机走通：模型收到的正是打进去的
                // 答案（tests/probe_kimi_other.rs，2.6.1，2026-11-15）。多选题里的 Other 没有走过，
                // 前端只在单选页给入口。
                options: &[
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^other$",
                        role: crate::chat_ui::OptionRole::CustomAnswer,
                        scope: None,
                        typed: Some(crate::chat_ui::TypedInput {
                            amend_input: None,
                            ready_pattern: r"^\s*type your answer, then press enter to save\.?\s*$",
                            submit_input: "\r",
                        }),
                    },
                ],
                unparsed_keys: &[],
                notice: None,
            },
            crate::chat_ui::AttentionPattern {
                id: "kimi:command-approval",
                patterns: &[
                    r"↑/↓ select · [\d/]+ choose · ↵ confirm",
                    r"^\s*▶\s*(?:run this command|write this file|apply these edits|stop this task|ready to build with this plan|approve [^\n?]+)\?",
                ],
                // 必须取最后一次：kimi 重绘也不清屏，上一条命令的面板残留在缓冲上方。
                // 取首个匹配会把标题/命令锁在残影上，而数字键打给的是屏幕最下方的活动
                // 面板——用户看着 A 批准了 B。
                last: true,
                details: crate::chat_ui::AttentionDetails::ArrowPanel,
                kind: crate::chat_ui::AttentionKind::Approval,
                confirms: None,
                // 选项角色（2.6.1 程序文件 `DEFAULT_APPROVAL_CHOICES` / `PLAN_REJECT_CHOICES` 与真机屏幕）：
                //   ▶ 1. Approve once / 2. Approve for this session / 3. Reject / 4. Reject with feedback
                // 计划审阅是 `Approve`（或计划自带的几个选项）加 `Reject`、`Revise`；计划自带的选项文案
                // 不固定，认不出的归 other，卡片照原文列出来。
                //
                // `Reject with feedback`：按它的数字键进入输入状态（`selectAndSubmit` 里
                // `requires_feedback` 分支），页脚从 `↑/↓ select · …` 变成 `Type feedback · ↵ submit.`，
                // 打字后回车以 rejected 加 feedback 作答。真机走通：命令没有执行，模型收到
                // `… rejected the approval request. Reason: <说明>`（probe_deny_feedback，2.6.1，2026-11-14）。
                // `Revise` 是同一个组件、同一个 `requires_feedback` 分支，没有单独走真机。
                options: &[
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^approve(?: once)?$",
                        role: crate::chat_ui::OptionRole::AllowOnce,
                        scope: None,
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^approve for this session$",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Session),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^reject$",
                        role: crate::chat_ui::OptionRole::Deny,
                        scope: None,
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^(?:reject with feedback|revise)$",
                        role: crate::chat_ui::OptionRole::DenyWithFeedback,
                        scope: None,
                        typed: Some(crate::chat_ui::TypedInput {
                            amend_input: None,
                            ready_pattern: r"^\s*type feedback · ↵ submit\.?\s*$",
                            submit_input: "\r",
                        }),
                    },
                ],
                unparsed_keys: &[],
                notice: None,
            },
        ]
    }

    /// CLI 漂移检查的手工声明部分（正则规则的样例、旧版兼容与运行时拼接的字面量），见 cli_text.rs。
    fn cli_text_deps(&self) -> &'static [crate::cli_text::CliTextDep] {
        cli_text::DEPS
    }
    fn id(&self) -> AgentId {
        id::KIMI
    }
    fn display_name(&self) -> &'static str {
        "Kimi Code"
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
    fn process_names(&self) -> &'static [&'static str] {
        &["kimi", "kimi.exe"]
    }
    fn proxy(&self) -> Option<&'static crate::proxy::ProxySpec> {
        Some(&PROXY)
    }
    fn relay(&self) -> Option<&'static dyn crate::RelayCap> {
        Some(&RELAY)
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["-r"]
    }
    /// `kimi fork -y --cwd <目录> <id>` 是非交互的：复制出新会话就退出，打印
    /// `Forked to session_<新id> ("Fork: <标题>") in 499ms`（2.4.0 实测）；会话按工作目录查找，
    /// 不带 --cwd 在别的目录下跑会报 not found。之后按 `-r <新id>` 打开。
    /// 面板浏览器的 MCP 服务器怎么交给它（见 [`crate::McpAttach`]）。
    fn mcp_attach(&self) -> Option<crate::McpAttach> {
        // Kimi Code 2.x 没有临时挂 MCP 的启动参数，只读 `<数据目录>/mcp.json` 等文件
        // （2026-11-06 查 kimi.exe 内嵌源码：resolveKimiHome(...) + "mcp.json"）。
        Some(crate::McpAttach::DataDirJson { rel: "mcp.json" })
    }

    fn fork_mode(&self) -> Option<crate::ForkMode> {
        Some(crate::ForkMode::Command {
            args: &["fork", "-y"],
            cwd_flag: Some("--cwd"),
            new_id_pattern: r"Forked to (session_[0-9a-fA-F-]+)",
        })
    }
    /// 启动 flag 取自 `kimi --help`（kimi-code 0.2x 实测）：`--auto` / `-y, --yolo` 是权限档，
    /// `--plan` 是计划模式。维度 id 与 [`Self::mode_controls`] 对齐，前端两处共用同一套文案。
    ///
    /// **不声明 model**：`-m, --model <alias>` 确实存在，但别名来自用户的 `config.toml`
    /// （`kimi provider` 可增删），不是产品固定值。本表只放插件能担保的字面量——
    /// 硬编码一份别名清单，用户改了 provider 后就会静默传一个不存在的模型。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        if crate::installation(crate::id::KIMI)
            .is_some_and(|installation| installation.variant_tag == "modern")
        {
            modern_launch_options()
        } else {
            &[]
        }
    }
    /// kimi 的待办工具叫 `TodoList`，参数是整份快照 `{todos:[{title,status}]}`。
    fn todo_snapshot_tools(&self) -> &'static [&'static str] {
        &["TodoList"]
    }
    /// `/effort <档位>`（kimi.exe 2.4.0 包内 handleEffortCommand：带参即切，不弹选择器；
    /// 成功回 `Thinking set to <档位>.`，模型不支持时回 `Unsupported thinking effort "<x>"
    /// for <别名>. Available: …`，回合进行中回 `Cannot switch models while streaming…`）。
    /// 档位随模型变（`support_efforts`），由模型目录给，这里不列。当前档位读会话日志。
    fn effort_control(&self) -> Option<crate::chat_ui::EffortControl> {
        Some(crate::chat_ui::EffortControl {
            command: "/effort",
            levels: &[],
            confirm_pattern: r"(?:thinking set to|with thinking) (\w+)",
            reject_pattern: r"unsupported thinking effort|cannot switch models while streaming|failed to switch model",
        })
    }
    /// `/undo N` 撤最近 N 轮（kimi.exe 包内 `src/tui/commands/undo.ts`：带参直接撤，不弹菜单；
    /// 只在空闲时可用）。撤回在 wire 里追加 `context.undo {count}`，解析层据此收回消息
    /// （见 telemetry 的 chat_line_mark）。拒绝文案同取自该文件。
    ///
    /// kimi 2.5.1 超出可撤范围改报 `Cannot undo 3 prompts; only 1 can be undone in the active
    /// context.`（包内 formatUndoLimitMessage，压缩后再加 ` after the last compaction`），
    /// 另有 `Usage: /undo [count]…`、`No active session…` 两种拒绝（2026-11-12 取证）。
    /// 匹配到的整行原样转给用户，所以每种拒绝都要认，不然只报「未确认」。旧文案保留。
    fn undo_control(&self) -> Option<crate::chat_ui::UndoControl> {
        Some(crate::chat_ui::UndoControl {
            command: "/undo",
            count_arg: true,
            restores_files: false,
            reject_pattern: Some(UNDO_REJECT_PATTERN),
            returns_input: false,
            menu: None,
        })
    }
    /// 模型目录直接读 `config.toml` 的 `[models."<别名>"]`（kimi CLI 自己就这么读：
    /// `displayName: e.display_name, supportEfforts: e.support_efforts`，kimi.exe 0.4x 包内）。
    /// 多账号会话的 `KIMI_CODE_HOME` 指向 profile 目录，配置也在那里。
    fn model_catalog(&self, env: &[(String, String)]) -> Vec<crate::ModelCatalogEntry> {
        config_path(env)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|content| parse_model_catalog(&content))
            .unwrap_or_default()
    }
    /// 会话内的 `/effort <档>` 走 persistModelSelection，会改写 config.toml 的 default_model 与
    /// [thinking]（取证见下方 model_switch 的注释）。
    fn effort_saves_default(&self) -> bool {
        true
    }
    /// `/model <alias>` 仍弹菜单、Enter 会改写 config.toml 的 default_model（research-kimi-gemini
    /// §2.2），所以菜单只作兜底；主路线是空闲时 `-r <id> -m <alias>` 重启接回（§2.3 e3 实测会话 id
    /// 不变、历史回放、下一轮用新模型、不改配置）。还没建会话时直接带 `-m` 重开（p7）。
    fn model_menu_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    /// 思考档不跟着新模型调整（research §2.3 p2：从不支持思考的 A 换到 B 后仍是 thinking disabled），
    /// 这里不补：Kimi 2.6 没有指定思考档的启动参数（`kimi --help` 只有 -m），会话内的 `/effort <档>`
    /// 走 confirmThenPerformEffortSwitch(…, "model") → persistModelSelection，会改写 config.toml 的
    /// default_model 与 [thinking]（kimi.exe K:530950、K:529381），违反「不改用户 CLI 默认配置」。
    /// 用户在模型菜单的思考档分段里自己选（同样经 /effort），是用户主动的改动。
    fn model_switch(&self) -> Option<crate::ModelSwitch> {
        Some(crate::ModelSwitch::RestartResume {
            flag: "-m",
            unsent_resume_args: None,
        })
    }
    fn model_launch_flag(&self) -> Option<&'static str> {
        Some("-m")
    }
    fn launch_defaults(
        &self,
        env: &[(String, String)],
        _cwd: Option<&std::path::Path>,
    ) -> crate::LaunchDefaults {
        config_path(env)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|content| parse_launch_defaults(&content))
            .unwrap_or_default()
    }
    /// 2.6 新开 TUI 在第一条消息前不建会话、不触发 SessionStart，屏幕原文
    /// `No session yet — one will be created on your first message.`（research-kimi-gemini §2.6，
    /// e4 Q1 实拍）。认出它 = CLI 已就绪，只是在等第一条消息。
    fn awaiting_first_message_markers(&self) -> &'static [&'static str] {
        &["No session yet — one will be created on your first message."]
    }
    /// Esc 中断当前回合。**用户在真实 app 里确认可用**(2026-07:运行中的 kimi 会话按
    /// 「打断并发送」→ Esc 确实停下当前回合)。自动探针抓不到证据(kimi.exe 的 launcher
    /// shim 在 ConPTY 下即退),故走真机人工确认——与 claude(官方文档)/codex(状态行)
    /// 同为确证级别。
    /// 输入框规格（这一家唯一的一份，见 chat_ui::ComposerSpec）。
    /// - 框体：2026-11-12 隔离无模型 PTY 实拍（target/lead-provider-probes/composer）。没有占位文字，
    ///   硬件光标常年隐藏（自绘光标）。
    /// - 清空：Ctrl-C 清掉整段草稿，随后状态行挂「Press Ctrl+C again to exit」；回合进行中按，草稿
    ///   清掉、回合照常继续（2.6.1，2026-11-14 probe_tui_script：`Count from 1 to 300` 途中按下，
    ///   数字接着往下出）。空输入框上的 Ctrl-C 是退出的第一下，所以只在认出有字时按。
    fn composer(&self, version: Option<&str>) -> Option<crate::chat_ui::ComposerSpec> {
        if !crate::lead_install::version_at_least(version, (2, 5, 3))
            || !self
                .resolve()
                .is_some_and(|inst| inst.variant_tag == "modern")
        {
            return None;
        }
        Some(crate::chat_ui::ComposerSpec {
            prompt_footer: false,
            input_start_pattern: r"^(\s*│ > )",
            // 草稿超出可视高度时上边框中间嵌一段 `↑ 33 more`（2.6.1 真机，45 行草稿只画最后 12 行）。
            // 不认它的话整框变成「认不出」，错误条说的是认不出输入框，而不是草稿没显示全。
            // 底边的 `↓ N more` 是按同一画法推的，没有实拍。
            top_pattern: r"^\s*╭─+(?: [↑↓] \d+ more ─+)?╮\s*$",
            bottom_pattern: r"^\s*╰─+(?: [↑↓] \d+ more ─+)?╯\s*$",
            row_pattern: r"^\s*│(.*)│\s*$",
            prompt_prefix: ">",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r"^$",
            send_requires_recognition: true,
            stash: None,
            clear_inputs: &["\x03"],
            clear_rounds: 1,
            clear_while_running: true,
            alt_modes: &[],
            // 草稿保真（2.6.1 真机，tests/probe_draft_fidelity.rs，2026-11-14）：
            // - 粘贴占位 `[paste #1 5120 chars]`（5120 字单行）、`[paste #1 +60 lines]`（60 行）；3 行小粘贴原样显示。
            //   正则照它自己认占位的那条写（kimi.exe 内嵌 `\[paste #(\d+)( (\+\d+ lines|\d+ chars))?\]`，尾部可缺）。
            //   图片占位 `[image #N (W×H)]` 见 clipboard_image_paste，按宽的认。
            // - 超出可视高度：见 top_pattern 的 `↑ N more`。
            // - 框宽 119 列时一行正文最多 110 格（正文从第 5 列起，离底边右端 4 格），按词折行；续行缩进 3 格。
            fidelity: Some(crate::chat_ui::ComposerFidelity {
                paste_pattern: r"\[paste #\d+(?: (?:\+\d+ lines|\d+ chars))?\]|\[[Ii]mage[^\]\r\n]*\]",
                overflow_pattern: Some(r"[↑↓] \d+ more"),
                overflow_rows: None,
                wrap_edge: crate::chat_ui::ComposerWrapEdge::Border,
                wrap_inset: 4,
            }),
        })
    }
    fn queue_input(&self) -> Option<&'static str> {
        Some("\r")
    }

    fn interrupt_input(&self) -> Option<&'static str> {
        Some("\x1b")
    }
    /// Ctrl+J（裸 LF）在输入框里换行、不提交（tests/probe_tui_script.rs，2.2.0，2026-10-16）。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\n")
    }
    /// `@` 是 kimi 自己的一等文件引用语法(官方文档;TUI 输 `@` 有补全)。它是**文本级**
    /// 惯例——wire.jsonl 实测 turn.prompt 原样保留 @路径文本、由模型自行读取,TUI 补全
    /// 同样只是插入路径文本。故注入 `@路径` 与用户手敲等价,即 kimi 的原生形态。
    /// 版本探测不到时兜底 false。
    fn attachment_mention(&self, version: Option<&str>) -> bool {
        version.is_some()
    }
    /// kimi TUI 的原生图片附加：触发键读系统剪贴板并把图片缓存为 blob 原生附加（composer
    /// 显示 `[image #N (W×H)]` 占位；旧版是 `[image:…]`，marker 正则两种形态都覆盖；
    /// wire.jsonl 实测入 prompt 为 image_url blobref）。触发键见 `clipboard_paste_input`。
    /// 版本探测不到时**仍声明**:能力已在真机实测,探测失败只是拿不到版本号,不代表
    /// 能力缺失(同 attachment_mention 的取值理由,但方向相反——那里探测不到时宁可
    /// 不用 `@`,因为文本通道恒可用;这里探测不到就**没有**声明的话,前端会静默退回
    /// 文本通道,把注入指令原文整段打进 TUI,实拍踩过)。剪贴板通道走不通时前端原有
    /// 的 sendWithClipboardImages → sendText 回退仍在,风险有底。
    fn clipboard_image_paste(&self, _version: Option<&str>) -> Option<&'static str> {
        Some(r"\[image[:# ]")
    }
    /// kimi 在 Windows 上的贴图键是 **Alt+V**，不是 Ctrl+V：0.29 与上一版（.bak）二进制
    /// 里都是 `matchesKey(..., win32 ? "alt+v" : ctrl("v"))`；真机探针
    /// （app/src-tauri/tests/probe_clipboard_image.rs）实测 `\x16` 注入 composer 零反应，
    /// `\x1bv`（传统 Meta 编码）~40ms 落出 `[image #1 (120×48)]`。kitty CSI-u 编码的
    /// Alt+V（`ESC[118;3u`）反而不被认——kimi 虽推 kitty 协议，这个键的匹配走的是
    /// 传统 ESC 前缀形态。版本门不设：两版二进制行为一致，探测不到版本时按现状声明。
    /// 非 Windows 按同一行 `ctrl("v")` 退回 Ctrl-V（读码推断，mac 未实测）。
    fn clipboard_paste_input(&self, _version: Option<&str>) -> Option<&'static str> {
        Some(if cfg!(windows) { "\x1bv" } else { "\x16" })
    }
    /// kimi 的 `/model` 同样是交互式菜单（二进制里的命令描述就是 `/model: switch model`，
    /// 且失败提示是「Run /model to select one first」），不接受内联参数，故不声明模型预设。
    /// Kimi Code：`skills/` 与共享的 `~/.agents/skills`、`mcp.json`、`config.toml` 的钩子。
    /// 旧版 Python kimi-cli（`~/.kimi`）的布局不同，不列。
    fn extensions(
        &self,
        inst: &crate::variant::Installation,
    ) -> Vec<crate::extensions::ExtensionGroupDto> {
        use crate::extensions as ext;
        if inst.variant_tag != "modern" {
            return Vec::new();
        }
        let dir = &inst.data_dir;
        let mcp_path = dir.join("mcp.json");
        let mcp = ext::read_json(&mcp_path);
        [
            ext::skills_group(&dir.join("skills"), false),
            ext::shared_agent_skills(),
            ext::mcp_group_from_json(&mcp_path, mcp.as_ref().and_then(|m| m.get("mcpServers"))),
            ext::hooks_group_from_kimi_toml(&dir.join("config.toml")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn slash_commands(&self) -> &'static [&'static str] {
        &["/clear", "/compact", "/help", "/model", "/status"]
    }
    fn mode_controls(&self) -> &'static [crate::ModeControl] {
        use crate::{ModeControl, ModeInput, ModeOption};
        static PLAN_ON: [ModeInput; 1] = [ModeInput {
            data: "/plan on",
            submit: true,
        }];
        static PLAN_OFF: [ModeInput; 1] = [ModeInput {
            data: "/plan off",
            submit: true,
        }];
        static WORK: [ModeOption; 2] = [
            ModeOption {
                value: "default",
                inputs: &PLAN_OFF,
            },
            ModeOption {
                value: "plan",
                inputs: &PLAN_ON,
            },
        ];
        static MODES: [ModeControl; 2] = [
            ModeControl {
                dimension: "work",
                menu_command: None,
                cycle_input: Some("\x1b[Z"),
                options: &WORK,
                // kimi 的模式切换有 wire 事件即时落 transcript（telemetry 的 set_mode），
                // 不依赖屏幕回显。
                screen_markers: &[],
            },
            ModeControl {
                dimension: "permission",
                menu_command: Some("/permission"),
                cycle_input: None,
                options: &[],
                screen_markers: &[],
            },
        ];
        // https://moonshotai.github.io/kimi-code/en/reference/slash-commands.html：权限命令打开菜单，不能盲发 on/off。
        // 旧 Python 版 kimi-cli 的命令和 wire 事件不同；只给已验证的 modern Kimi Code 声明。
        if crate::installation(crate::id::KIMI)
            .is_some_and(|installation| installation.variant_tag == "modern")
        {
            &MODES
        } else {
            &[]
        }
    }
    /// 装当前 Node 版 Kimi Code（装到 `~/.kimi-code/bin/kimi.exe`，与 modern 变体的候选一致）。
    /// 注意路径里的 `/kimi-code/`——不带它的 `code.kimi.com/install.ps1` 装的是旧 Python `kimi-cli`
    /// （落到 `~/.local/bin/kimi-cli.exe`，检测不到）。
    ///
    /// **不直下**，也**不换入口**，理由都已实测：
    ///
    /// - `code.kimi.com` 是 nginx 直服（`server: nginx`，无 `cf-ray`），压根不在 Cloudflare 后面，
    ///   不会被人机校验拦。判定仍照做——中间设备也可能塞一张 HTML。
    /// - 它的引导脚本有 417 行（claude 的只有 110 行）。除了「取 latest → 读 manifest 的 checksum →
    ///   下载 → 校验」这段与 claude 同构之外，它还要迁移旧 `kimi-cli` 安装（重命名成
    ///   `kimi-legacy.exe`）、备份**正在运行**中被占用的 `kimi.exe`、写用户 PATH。把这些重新实现
    ///   一遍就是在复刻 kimi 的安装语义，上游一改我们就悄悄装坏。
    ///
    /// 对比 claude：它的脚本是段三步胶水，真正的安装由 `claude.exe install` 自己完成，
    /// 所以那边直下是干净的（见 `plugins/claude/install.rs`）。
    fn install_script(&self, windows: bool) -> Option<crate::install::InstallScript> {
        Some(crate::install::InstallScript::Fetch {
            url: if windows {
                "https://code.kimi.com/kimi-code/install.ps1"
            } else {
                "https://code.kimi.com/kimi-code/install.sh"
            },
            unix_shell: "bash",
        })
    }
    /// Kimi Code 0.23 会把当前任务标题写进终端标签。仍保留 reporter token：较早版本不写标题，
    /// 新版也可能在 hook 后才覆盖 token；app 会始终优先匹配 token，再匹配任务标题。
    fn sets_terminal_tab_title(&self) -> bool {
        true
    }
    fn writes_tab_token(&self) -> bool {
        true
    }
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
    fn account(&self) -> Option<&'static dyn crate::account::AccountCap> {
        Some(&account::ACCOUNT)
    }
    fn profile(&self) -> Option<&'static crate::profile::ProfileSpec> {
        Some(&PROFILE)
    }
}

/// 该会话的 config.toml：多账号会话的 `KIMI_CODE_HOME` 指向 profile 目录，配置也在那里。
fn config_path(env: &[(String, String)]) -> Option<std::path::PathBuf> {
    let home = env
        .iter()
        .find(|(key, value)| key == "KIMI_CODE_HOME" && !value.is_empty())
        .map(|(_, value)| std::path::PathBuf::from(value));
    match home {
        Some(home) => Some(home.join("config.toml")),
        None => telemetry::kimi_install().map(|inst| inst.config_path()),
    }
}

/// 逐行解析 `config.toml` 的 `[models."<别名>"]` 段（不引 toml 依赖，同 telemetry 的
/// `context_window` 既有范式）。只取菜单要的三项：显示名、所属 provider、支持的思考档位。
/// `[models."x".sub]` 这类子表不算模型段；多行数组不解析（kimi 自己写出的是单行）。
fn parse_model_catalog(content: &str) -> Vec<crate::ModelCatalogEntry> {
    fn toml_str(value: &str) -> Option<String> {
        let inner = value.trim().strip_prefix('"')?.strip_suffix('"')?;
        Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
    }
    let default_model = top_level_value(content, "default_model");
    let mut out = Vec::new();
    let mut current: Option<crate::ModelCatalogEntry> = None;
    // `always_thinking` 的模型没有 off 档（kimi 的 segmentsFor），段尾统一补。
    let mut always_on = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            out.extend(current.take().map(|entry| with_off(entry, always_on)));
            always_on = false;
            if let Some(alias) = line
                .strip_prefix("[models.\"")
                .and_then(|rest| rest.strip_suffix("\"]"))
            {
                let mut entry = crate::ModelCatalogEntry::new(alias, alias);
                entry.is_default = default_model.as_deref() == Some(alias);
                current = Some(entry);
            }
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "display_name" => {
                if let Some(label) = toml_str(value).filter(|label| !label.is_empty()) {
                    entry.label = label;
                }
            }
            "provider" => entry.group = toml_str(value).map(|provider| provider_label(&provider)),
            "default_effort" => entry.default_effort = toml_str(value),
            "capabilities" => always_on = value.contains("\"always_thinking\""),
            "support_efforts" => {
                entry.efforts = value
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .filter_map(toml_str)
                    .collect();
            }
            _ => {}
        }
    }
    out.extend(current.map(|entry| with_off(entry, always_on)));
    out
}

/// 分组名用人话：`managed:kimi-code` 这类是 Kimi 自家托管的 provider（config.toml 里
/// `[providers."managed:kimi-code"] type = "kimi"`，本机配置实拍），菜单写「Kimi」；用户自己加的
/// provider 名本就是用户起的（「反向推理」「xbot」），原样用。
fn provider_label(provider: &str) -> String {
    if provider.starts_with("managed:") {
        "Kimi".to_string()
    } else {
        provider.to_string()
    }
}

/// kimi 的档位列表 = `support_efforts`，非 always_thinking 的模型前面多一个 `off`
/// （kimi.exe `segmentsFor`）。没声明 support_efforts 的是开关型模型，不给档位。
fn with_off(mut entry: crate::ModelCatalogEntry, always_on: bool) -> crate::ModelCatalogEntry {
    if !entry.efforts.is_empty() && !always_on {
        entry.efforts.insert(0, "off".to_string());
    }
    entry
}

/// `/undo` 的拒绝行（见 `undo_control` 注释的出处）。宿主按 `(?i)` 编译、逐行匹配。
pub(crate) const UNDO_REJECT_PATTERN: &str = r"cannot undo while streaming|cannot undo \d+ prompts?;|nothing to undo|failed to undo|run /undo again|usage: /undo|no active session";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_process_scope_preserves_multisession_services() {
        use crate::registry::ProcessScope;
        assert_eq!(Kimi.process_scope(&[]), ProcessScope::Unknown);
        assert_eq!(
            Kimi.process_scope(&["node".into(), "other.js".into()]),
            ProcessScope::Unknown
        );
        assert_eq!(
            Kimi.process_scope(&["C:/bin/kimi.exe".into(), "--session".into(), "sid".into()]),
            ProcessScope::Exclusive
        );
        for service in ["web", "server", "acp", "rc", "remote"] {
            assert_eq!(
                Kimi.process_scope(&["kimi.exe".into(), service.into()]),
                ProcessScope::Shared
            );
        }
    }
    use crate::variant::Installation;
    use std::path::Path;

    /// 新建页默认值读 config.toml 顶层键；子表里的同名键不算。
    #[test]
    fn launch_defaults_read_top_level_model_permission_and_plan() {
        let content = r#"default_model = "kimi-code/k3"
default_permission_mode = "yolo"
default_plan_mode = true

[models."kimi-code/k3"]
default_permission_mode = "auto"
"#;
        let defaults = parse_launch_defaults(content);
        assert_eq!(defaults.model.as_deref(), Some("kimi-code/k3"));
        assert_eq!(
            defaults.choices.get("permission").map(String::as_str),
            Some("yolo")
        );
        assert_eq!(
            defaults.choices.get("work").map(String::as_str),
            Some("plan")
        );
        // 旧字段 yolo = true 迁移成 yolo（K:346047）。
        let legacy = parse_launch_defaults(
            "yolo = true
",
        );
        assert_eq!(
            legacy.choices.get("permission").map(String::as_str),
            Some("yolo")
        );
        assert!(parse_launch_defaults(
            "default_permission_mode = \"weird\"
"
        )
        .choices
        .is_empty());
    }

    /// 等第一条消息的屏幕原文（research-kimi-gemini §2.6 e4 Q1）。
    #[test]
    fn awaiting_first_message_marker_is_the_exact_kimi_text() {
        let screen = "  Session:
  No session yet — one will be created on your first message.
";
        assert!(Kimi
            .awaiting_first_message_markers()
            .iter()
            .any(|marker| screen.contains(marker)));
    }

    /// 样本形制取自本机 kimi-code 生成的 config.toml（2026-10，敏感字段已去）。
    #[test]
    fn model_catalog_reads_display_name_provider_and_efforts() {
        let content = r#"default_model = "kimi-code/kimi-for-coding"

[thinking]
effort = "high"

[providers."managed:kimi-code"]
base_url = "https://example.invalid"

[models."kimi-code/k3.1-pro"]
provider = "managed:kimi-code"
model = "k3.1-pro"
display_name = "K3.1 Pro"
support_efforts = [ "low", "high", "max" ]

[models."kimi-code/k3"]
provider = "managed:kimi-code"
capabilities = [ "thinking", "always_thinking" ]
display_name = "K3"
support_efforts = [ "low", "high" ]

[models."反向推理/DeepSeek-V4.1-Flash-1017"]
provider = "反向推理"
model = "DeepSeek-V4.1-Flash-1017"

[models."kimi-code/k3.1-pro".extra]
display_name = "不是模型段"
"#;
        let catalog = parse_model_catalog(content);
        assert_eq!(catalog.len(), 3);
        // always_thinking 的模型没有 off 档。
        assert_eq!(catalog[1].efforts, vec!["low", "high"]);
        assert_eq!(catalog[0].id, "kimi-code/k3.1-pro");
        assert_eq!(catalog[0].label, "K3.1 Pro");
        assert_eq!(catalog[0].group.as_deref(), Some("Kimi"));
        assert_eq!(catalog[0].efforts, vec!["off", "low", "high", "max"]);
        // 没写 display_name 的回退别名，菜单上至少认得出是哪个。
        assert_eq!(catalog[2].label, "反向推理/DeepSeek-V4.1-Flash-1017");
        assert!(catalog[2].efforts.is_empty());
    }

    /// 直接对变体表 probe（不碰真实 home，也不读 env）。
    fn probe_at(home: &Path) -> Option<Installation> {
        VARIANTS.iter().find_map(|v| {
            let dir = v
                .data_dir
                .candidates
                .iter()
                .map(|c| home.join(c))
                .find(|p| p.is_dir())?;
            Some(v.installation_at(id::KIMI, dir, Some(home)))
        })
    }

    #[test]
    fn events_all_in_upstream_whitelist() {
        // 防连坐绊线：一条非法 event 会让 kimi 静默禁用全部 hooks。
        for ev in EVENTS {
            assert!(
                EVENT_WHITELIST.contains(&ev.name),
                "{} 不在 kimi 0.20 事件白名单",
                ev.name
            );
        }
    }

    #[test]
    fn permission_tiers_follow_kimi_2_2_help_and_legacy_hooks_stay_separate() {
        let permission = modern_launch_options()
            .iter()
            .find(|option| option.id == "permission")
            .unwrap();
        let auto = permission
            .choices
            .iter()
            .find(|choice| choice.id == "auto")
            .unwrap();
        let yolo = permission
            .choices
            .iter()
            .find(|choice| choice.id == "yolo")
            .unwrap();
        assert_eq!(
            (auto.label, auto.args, auto.risk),
            ("Never Ask", ["--auto"].as_slice(), true)
        );
        assert_eq!(
            (yolo.label, yolo.args, yolo.risk),
            ("Ask When Needed", ["--yolo"].as_slice(), false)
        );
        assert!(!LEGACY_EVENTS.iter().any(|event| matches!(
            event.name,
            "PermissionRequest" | "PermissionResult" | "Interrupt"
        )));
        assert!(EVENTS.iter().any(|event| event.name == "Interrupt"));
    }

    #[test]
    fn terminal_title_capabilities_cover_old_and_new_kimi() {
        let plugin = Kimi;
        assert!(plugin.sets_terminal_tab_title());
        assert!(plugin.writes_tab_token());
    }

    #[test]
    fn legacy_migration_removes_only_our_unsupported_events() {
        let old = "[[hooks]]\nevent = \"PermissionRequest\"\ncommand = \"C:/old/meowo-reporter.exe --provider kimi\"\n\n[[hooks]]\nevent = \"Notification\"\ncommand = \"echo user-notification\"\n";
        let crate::EnsureOutcome::Changed(text) =
            LEGACY_HOOKS.ensure_hooks(old, "C:/new/meowo-reporter.exe", "kimi")
        else {
            panic!("旧接线应迁移");
        };
        let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
        let hooks = doc["hooks"].as_array_of_tables().unwrap();
        assert!(!hooks
            .iter()
            .any(|hook| hook["event"].as_str() == Some("PermissionRequest")));
        assert!(hooks
            .iter()
            .any(|hook| hook["command"].as_str() == Some("echo user-notification")));
    }

    #[test]
    fn clipboard_image_paste_declared_even_without_version() {
        // 版本探测失败(拿不到版本号)不等于能力缺失:剪贴板发图已真机实测,探测不到
        // 就不声明会让前端静默退回文本通道、把注入指令原文打进 TUI(实拍)。无论版本
        // 是否探到,marker 声明必须一致。
        let plugin = Kimi;
        assert_eq!(plugin.clipboard_image_paste(None), Some(r"\[image[:# ]"));
        assert_eq!(
            plugin.clipboard_image_paste(Some("1.0")),
            Some(r"\[image[:# ]")
        );
    }

    #[test]
    fn clipboard_paste_input_is_alt_v_on_windows() {
        // 回归钉:kimi(win32)的贴图键是 Alt+V(`\x1bv`),不是通用的 Ctrl-V。发错键
        // composer 零反应,占位符超时后前端静默回退指令文本(用户实拍故障)。
        let plugin = Kimi;
        let expected = if cfg!(windows) { "\x1bv" } else { "\x16" };
        assert_eq!(plugin.clipboard_paste_input(None), Some(expected));
        assert_eq!(plugin.clipboard_paste_input(Some("0.29.0")), Some(expected));
    }

    #[test]
    fn prefers_modern_then_legacy() {
        let home = std::env::temp_dir().join(format!("meowo-kimi-variants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);

        // 都不存在 → 未配置过。
        std::fs::create_dir_all(&home).unwrap();
        assert!(probe_at(&home).is_none());

        // 只有旧版 → legacy 命中，配置路径落在 ~/.kimi/config.toml。
        std::fs::create_dir_all(home.join(".kimi")).unwrap();
        let legacy = probe_at(&home).expect("legacy 应命中");
        assert_eq!(legacy.variant_tag, "legacy");
        assert_eq!(legacy.config_path(), home.join(".kimi").join("config.toml"));
        assert_eq!(
            legacy.credentials_path(),
            Some(
                home.join(".kimi")
                    .join("credentials")
                    .join("kimi-code.json")
            )
        );

        // 新版出现 → 抢先。
        std::fs::create_dir_all(home.join(".kimi-code")).unwrap();
        let modern = probe_at(&home).expect("modern 应命中");
        assert_eq!(modern.variant_tag, "modern");
        assert_eq!(modern.data_dir, home.join(".kimi-code"));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn launch_falls_back_to_bare_name_when_not_found() {
        let home = std::env::temp_dir().join(format!("meowo-kimi-exe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".kimi")).unwrap();

        let inst = probe_at(&home).unwrap();
        assert!(!inst.is_launchable());
        assert_eq!(inst.launch_argv(), vec!["kimi".to_string()]); // 回退裸名走 PATH

        // 旧版真实落点：~/.local/bin/kimi[.exe]
        let local_bin = home.join(".local").join("bin");
        std::fs::create_dir_all(&local_bin).unwrap();
        let exe = local_bin.join(crate::exe_file_name("kimi"));
        std::fs::write(&exe, b"").unwrap();
        let inst = probe_at(&home).unwrap();
        assert!(inst.is_launchable());
        assert_eq!(inst.launch_argv(), vec![exe.to_string_lossy().into_owned()]);

        let _ = std::fs::remove_dir_all(&home);
    }
}
