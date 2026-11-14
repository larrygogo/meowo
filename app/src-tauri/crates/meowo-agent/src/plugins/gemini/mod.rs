//! gemini（Google Gemini CLI）插件。数据目录与 hooks 文件都平平无奇——`~/.gemini/settings.json`
//! 的 hooks 块与 claude 的 `settings.json` **结构完全同构**，故直接复用 [`ConfigFormat::ClaudeJson`]，
//! 一行合并逻辑都不必新写。
//!
//! 真正的两处不同：
//!
//! **一、事件名自成一派。** Gemini 没有 `UserPromptSubmit` 与 `Stop`，对应的叫 `BeforeAgent` /
//! `AfterAgent`，`PostToolUse` 叫 `AfterTool`。写进配置的必须是 Gemini 认识的名字，而 reporter 的
//! dispatch 只认规范名，故本插件覆写 [`canonical_event`](AgentPlugin::canonical_event) 把它们译回去。
//! 这也是「加 agent 只动 `plugins/`」的一次兑现：dispatch 里没有一个 `if provider == "gemini"`。
//!
//! **二、它是纯 JS，没有自己的可执行。** npm 全局装出来的 `gemini.cmd` 只是个 shim，真正跑起来的
//! 进程是 `node`（实测 0.50.0：`bin` 指向 `bundle/gemini.js`）。这逼得
//! [`process_names`](crate::registry::AgentPlugin::process_names) 必须收
//! `node`——代价见那里的注释。
//!
//! 事件表（配置里写左边，dispatch 看到的是右边）：
//!
//! | Gemini | 规范名 | 用途 |
//! |---|---|---|
//! | `SessionStart` | `SessionStart` | 建会话 |
//! | `BeforeAgent` | `UserPromptSubmit` | 用户提交（负载带 `prompt`） |
//! | `AfterTool` | `PostToolUse` | 工具跑完 |
//! | `AfterAgent` | `Stop` | 回合结束（负载带 `prompt_response`＝AI 正文） |
//! | `SessionEnd` | `SessionEnd` | 会话收尾 |
//! `PreCompress` 没有完成配对，暂不写入「正在压缩」，避免手动压缩后永久挂起。

pub mod account;
mod cli_text;
mod lead;
mod models;
/// 单测用：gemini 是否打开了 modelSteering（见 lead::model_steering）。
#[cfg(test)]
pub(crate) fn lead_model_steering(data_dir: &std::path::Path) -> bool {
    lead::model_steering(data_dir)
}
mod screen;
pub mod telemetry;
mod transcript;
#[cfg(test)]
pub(crate) use transcript::materialize_for_test;

use crate::{
    auth::{AuthScheme, CredentialSource},
    caps::TelemetryCap,
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// Gemini 的 OAuth 凭据。文件名取自 0.50 bundle 里的常量，与账号信息同处 `~/.gemini`（＝ data_dir）。
///
/// **登录入口是 `Some(&[])`——裸启动**。gemini 没有登录子命令（实测 `gemini --help`：只有
/// mcp / extensions / skills / hooks / gemma），跑 `gemini` 本身首次就会引导你选认证方式
/// （Google 账号 / API key）。这与「没有登录入口」（`login: None`）是两回事。
///
/// 不声明 OAuth 刷新：token 由 gemini 自己维护，Meowo 只读不刷。
static AUTH: AuthScheme = AuthScheme {
    credentials: CredentialSource::File("oauth_creds.json"),
    refresh: None,
    default_base_url: "",
    login: Some(&[]),
    // gemini 没有登出子命令（`gemini --help` 里只有 mcp / extensions / skills / hooks / gemma）
    // → 宿主直接删凭据文件。
    logout_args: &[],
};

/// 接线事件集，写的是 **Gemini 的**事件名（配置里必须如此），译回规范名见 [`Gemini::canonical_event`]。
///
/// 不带 matcher（`plain`）是有意的：Gemini 的工具事件按正则匹配工具名，但生命周期事件的 matcher 是
/// **精确字符串**比对——claude 那套 `matcher: "*"` 搬过来会一条都匹配不上，而缺省即「不限制」。
///
/// timeout 单位是**毫秒**，与 claude 的秒语义相反（实测 0.51 hookRunner：
/// `setTimeout(..., hookConfig.timeout ?? 60000)`）。沿用 `plain` 的默认 5 就是 5ms——reporter
/// 刚 spawn 即被 `taskkill /f /t` 整树击杀，每个 hook 都「超时失败」，会话根本落不了库。
const GEMINI_HOOK_TIMEOUT_MS: u64 = 5_000;
static EVENTS: [HookEvent; 6] = [
    HookEvent::plain("SessionStart").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
    HookEvent::plain("BeforeAgent").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
    HookEvent::plain("AfterTool").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
    HookEvent::plain("AfterAgent").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
    HookEvent::plain("SessionEnd").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
    HookEvent::plain("Notification").with_timeout(GEMINI_HOOK_TIMEOUT_MS),
];

/// Gemini 0.50 支持的全部 hook 事件。写进一条它不认识的 event 会怎样尚未实测，但 kimi 的前车之鉴
/// （一条非法 event 令**全部** hooks 静默失效）足以让这张白名单值得存在——EVENTS 有针对它的绊线测试。
pub const EVENT_WHITELIST: [&str; 11] = [
    "BeforeTool",
    "AfterTool",
    "BeforeAgent",
    "AfterAgent",
    "BeforeModel",
    "AfterModel",
    "BeforeToolSelection",
    "SessionStart",
    "SessionEnd",
    "Notification",
    "PreCompress",
];

/// `settings.json` 与 claude 的同名文件同构，且同样承载 hooks 之外的用户配置（主题、模型…），
/// 故一律原样保留。文件不存在时从 `{}` 建——gemini 本就允许它缺席。
///
/// `ps_call_operator`：gemini 在 Windows 恒经 PowerShell 执行 hook（0.51 `getShellConfiguration`
/// 没有 cmd 分支），引号开头的 `"exe" --provider gemini` 是字符串表达式、解析即以 code 1 失败，
/// 必须写成 `& "exe" --provider gemini`。
static HOOKS: HookSpec = HookSpec {
    config_rel: "settings.json",
    format: ConfigFormat::ClaudeJson,
    missing: MissingConfig::CreateFrom("{}"),
    events: &EVENTS,
    command: CommandSpec {
        quote_exe: true,
        with_provider: true,
        ps_call_operator: true,
    },
};

/// 纯 JS，没有原生二进制：**必须**走 node 包装。
///
/// 实测 npm 全局（Windows）：`%APPDATA%\npm\node_modules\@google\gemini-cli\bundle\gemini.js`，
/// 而 `%APPDATA%\npm\gemini.exe` **并不存在**——npm 只生成 `gemini.cmd` / `gemini.ps1` / `gemini`(sh)。
/// 所以 Windows 上绝不能指望 `OnPath`（它查的是 `gemini.exe`），只能直取那个 js。
///
/// unix 侧 npm 全局前缀五花八门（`/usr/local`、`~/.npm-global`、nvm 的版本目录…），逐一枚举是徒劳；
/// 好在那里的 shim 是无扩展名的 `gemini`，`OnPath` 正好能兜住，故末位留它。
static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "gemini",
    candidates: &[
        LaunchCandidate::NodeScript {
            root: Root::Env("APPDATA"),
            rel: "npm/node_modules/@google/gemini-cli/bundle/gemini.js",
        },
        // APPDATA 缺失的环境里由 USERPROFILE 推。
        LaunchCandidate::NodeScript {
            root: Root::Env("USERPROFILE"),
            rel: "AppData/Roaming/npm/node_modules/@google/gemini-cli/bundle/gemini.js",
        },
        LaunchCandidate::OnPath,
    ],
};

/// gemini（node/undici）的代理支持，实测 0.50 bundle：
/// - 读 `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY`（大小写都认），用 `setGlobalDispatcher(new ProxyAgent)`
///   挂到全局 fetch。
/// - **SOCKS 支持**：bundle 里 `socks-proxy-agent` / `SocksProxyAgent` / `socks://` 明确接线，
///   socks 串同样从 `HTTPS_PROXY` 读（不读 `ALL_PROXY`），故 socks_keys 与 http_keys 同。
/// - 只认进程环境变量、不写进自己的配置（`config_env=false`）：与 codex/kimi 一样，只覆盖从 Meowo
///   打开的会话。
static PROXY: crate::proxy::ProxySpec = crate::proxy::ProxySpec {
    socks: true,
    config_env: false,
    http_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
    socks_keys: &["HTTPS_PROXY", "HTTP_PROXY"],
};

static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec {
        // 0.65 Storage.getGlobalGeminiDir 在覆盖的 home 下拼 .gemini，不能直接把变量当数据根。
        env: Some("GEMINI_CLI_HOME"),
        env_suffix: ".gemini",
        candidates: &[".gemini"],
    },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
    trust: None,
}];

pub struct Gemini;

impl AgentPlugin for Gemini {
    fn process_scope(&self, argv: &[String]) -> crate::registry::ProcessScope {
        use crate::registry::ProcessScope;
        let is_gemini = |arg: &String| {
            let path = arg.replace('\\', "/").to_ascii_lowercase();
            matches!(
                path.rsplit('/').next(),
                Some("gemini" | "gemini.exe" | "gemini.cmd")
            ) || path.ends_with("/@google/gemini-cli/bundle/gemini.js")
        };
        // 入口是 argv[0]；node 启动时是 node 之后第一个非选项参数。0.68 会以
        // `node --max-old-space-size=N … gemini.js` 重启自身（bundle/chunk-G62RLJX6.js:1927），
        // gemini.js 不在前两项（做空复现）。
        let script = argv.iter().skip(1).find(|arg| !arg.starts_with('-'));
        let own = argv.first().is_some_and(is_gemini) || script.is_some_and(is_gemini);
        if !own {
            return ProcessScope::Unknown;
        }
        if argv.iter().any(|arg| {
            matches!(arg.as_str(), "--acp" | "--experimental-acp")
                || arg.starts_with("--acp=")
                || arg.starts_with("--experimental-acp=")
        }) {
            ProcessScope::Shared
        } else {
            ProcessScope::Exclusive
        }
    }
    fn screen_rules(&self) -> &'static [crate::screen::ScreenRule] {
        screen::RULES
    }

    /// CLI 漂移检查的手工声明部分（正则规则的样例、旧版兼容与运行时拼接的字面量），见 cli_text.rs。
    fn cli_text_deps(&self) -> &'static [crate::cli_text::CliTextDep] {
        cli_text::DEPS
    }
    fn id(&self) -> AgentId {
        id::GEMINI
    }
    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }
    fn lead(&self) -> Option<&'static dyn crate::lead_install::LeadCap> {
        Some(&lead::CAP)
    }

    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }

    /// **不得不收 `node`。** gemini 没有自己的可执行——会话本体就是一个跑着 `bundle/gemini.js` 的
    /// node 进程，不收它，owner_pid 上溯就找不到会话宿主（PID 抓不到 → 判活拿不到依据），
    /// 整条会话生命周期都会瘸。
    ///
    /// 代价是 `is_agent_process("node.exe")` 从此为真，判活因而变宽：某个 gemini 会话的 PID 若被
    /// 系统回收、又恰好落给另一个 node 进程，那个已死的会话会被误判为仍然活着。这是**已知且被接受**
    /// 的取舍——反过来（不收 node）的代价是每个 gemini 会话从一开始就没有 PID，那是必然的坏，
    /// 而这个是偶然的坏。
    ///
    /// 上溯本身不受这层宽泛影响：reporter 是 gemini 派生的 hook 子进程，父链上第一个 node 必是它。
    fn process_names(&self) -> &'static [&'static str] {
        &["gemini", "gemini.exe", "node", "node.exe"]
    }

    /// Gemini 的事件名 → meowo 规范名。这是本插件存在的主要理由，见模块文档的对照表。
    fn canonical_event<'a>(&self, raw: &'a str) -> &'a str {
        match raw {
            "BeforeAgent" => "UserPromptSubmit",
            "AfterAgent" => "Stop",
            "AfterTool" => "PostToolUse",
            // 0.61 / 官方 hooks reference 仍无完成事件。兼容旧接线留下的 PreCompress，不能再置永久 running。
            "PreCompress" => "",
            other => other,
        }
    }

    fn resume_args(&self) -> &'static [&'static str] {
        &["--resume"]
    }
    /// 裸 `/model` 弹对话框，只作清单为空时的兜底。0.68 其实有 `/model set <id>`，但它不校验模型名、
    /// 运行中被拒的文字残留在输入框、有待批审批时打字会替用户批准（research-kimi-gemini §3.2/§3.4），
    /// 所以主路线是空闲时 `--resume <uuid> -m <id>` 重启接回（§3.3 E3-A 实测 id 不变、历史完整、
    /// 下一轮用新模型）。还没发过消息时会话文件退出即删，resume 报 Invalid session identifier，
    /// 改用 `--session-id <同一 uuid>`（E3-D）。
    fn model_menu_command(&self) -> Option<&'static str> {
        Some("/model")
    }
    /// 没发过消息的会话：Gemini 正常退出时会删掉只有元数据的会话文件（research-kimi-gemini §3.3 e3-05），
    /// Meowo 硬杀进程（broker.stop）时来不及删，留着它 `--resume` 报「No previous sessions found」、
    /// `--session-id` 报「already exists」（0.68 bundle `sessionExists` 按 chats/ 下同 id 文件判，
    /// 真机验证 G1，2026-11-14）。这里替 CLI 做完它本来会做的清理，但只挪进备份目录、不删；
    /// 判据只认确定形态（做空 N1：旧版 .json 曾被误判为空），见 transcript::session_content。
    fn prepare_unsent_resume(&self, session_id: &str) -> Option<bool> {
        transcript::prepare_unsent_resume(session_id)
    }
    fn model_switch(&self) -> Option<crate::ModelSwitch> {
        Some(crate::ModelSwitch::RestartResume {
            flag: "-m",
            unsent_resume_args: Some(&["--session-id"]),
        })
    }
    fn model_launch_flag(&self) -> Option<&'static str> {
        Some("-m")
    }
    fn model_catalog(&self, _env: &[(String, String)]) -> Vec<crate::ModelCatalogEntry> {
        self.resolve()
            .map(|inst| models::catalog(&inst.data_dir, None))
            .unwrap_or_default()
    }
    fn launch_defaults(
        &self,
        _env: &[(String, String)],
        cwd: Option<&std::path::Path>,
    ) -> crate::LaunchDefaults {
        self.resolve()
            .map(|inst| models::launch_defaults(&inst.data_dir, cwd))
            .unwrap_or_default()
    }

    /// 运行中排队用回车（probe_tui_script，0.64，2026-10-16）：回车与 Tab 运行中都会进
    /// `Queued (press ↑ to edit)`，但空闲时 Tab 不提交、正文停在输入框里。对话页的运行态
    /// 是推断（DB 启动即 running、hook 执行期屏幕无 esc to cancel），推断错了按 Tab 就丢
    /// 消息，按回车两种状态都对。
    /// 输入框规格（这一家唯一的一份，见 chat_ui::ComposerSpec）。
    /// - 框体：2026-11-12 隔离无模型实拍（target/lead-provider-probes/composer）。
    /// - 占位是灰色、比真实草稿多缩进两格，光标停在列 3；真打出同一句光标在文末（0.68.0，
    ///   2026-11-14 probe_tui_script `cells`）。开了 vim 模式（`general.vimMode`）占位换成
    ///   `Press 'Esc' for NORMAL mode.` / `Press 'i' for INSERT mode.`，旧规则不认，开 vim 的人
    ///   每次发送都被当成「已有草稿」。
    /// - 清空：Ctrl-C 能清，但回合进行中它会取消当前请求（实拍 `Request cancelled.`），空闲时还会
    ///   挂上「Press Ctrl+C again to exit.」。改用行编辑键 Ctrl-K、Ctrl-U、退格逐行清，每轮后复核：
    ///   回合进行中两行草稿两轮清完，回合不受影响（同日实拍）。
    /// - shell 模式：提示符 `!`（占位 `Type your shell command`，状态行 `shell mode enabled (esc to
    ///   disable)`），按一次 Esc 回到普通输入框。vim NORMAL：状态行左侧 `[NORMAL]`，按 `i` 回到
    ///   `[INSERT]`。这两种模式下写进去的字会被当成 shell 命令 / vim 命令，先退出再写（同日实拍）。
    fn composer(&self, version: Option<&str>) -> Option<crate::chat_ui::ComposerSpec> {
        if !crate::lead_install::version_at_least(version, (0, 65, 0)) {
            return None;
        }
        Some(crate::chat_ui::ComposerSpec {
            prompt_footer: false,
            // 提示符按状态取值（0.68 interactiveCli-ZJ2ZELJM.js 约 22698 行）：平常 `>`，YOLO 档 `*`（做空 R2，
            // 实拍 g7-2）；shell 模式 `!` 走 alt_modes；反向搜索 `(r:)` 有意不认，保守按认不出。
            input_start_pattern: r"^(\s*[>*] )",
            // 0.65 是整条细线 `─`；0.68 改画半块字符，上沿 `▄`、下沿 `▀`（真机验证 g2/g7/g8 四帧，
            // UTF-8 E2 96 84 / E2 96 80，做空 N2）。历史里每条用户消息也画同样的框，从屏幕底往上找到的
            // 第一个下沿才是输入框；等审批时输入框被审批面板取代，认到的是历史框（有字），只会更保守。
            top_pattern: r"^(?:─+|▄+)\s*$",
            bottom_pattern: r"^(?:─+|▀+)\s*$",
            row_pattern: r"^ (.*)$",
            // 多个提示符用 `|` 分隔（composer_read / composerCandidate 逐个试）。
            prompt_prefix: ">|*",
            tail_rows: 0,
            tail_pattern: None,
            empty_pattern: r"^(?:Type your message or @path/to/file|Press 'Esc' for NORMAL mode\.|Press 'i' for INSERT mode\.)?$",
            send_requires_recognition: true,
            stash: None,
            clear_inputs: &["\x0b", "\x15", "\x7f"],
            clear_rounds: 12,
            clear_while_running: true,
            alt_modes: &[
                crate::chat_ui::ComposerAltMode {
                    id: "shell",
                    prompt_pattern: Some(r"^!(?:\s|$)"),
                    below_pattern: None,
                    exit_input: "\x1b",
                },
                crate::chat_ui::ComposerAltMode {
                    id: "vim_normal",
                    prompt_pattern: None,
                    below_pattern: Some(r"^\s*\[NORMAL\]"),
                    exit_input: "i",
                },
            ],
        })
    }
    fn queue_input(&self) -> Option<&'static str> {
        Some("\r")
    }

    /// 运行中回车进 `Queued (press ↑ to edit)`，当前回答结束后才发出（probe_tui_script，0.64）。
    /// 打开 `experimental.modelSteering` 后（Lead 安装会打开，leader 2026-11-13 同意）语义变了：
    /// 运行中回车是「插话」，在下一步模型请求前插进当前回合；回合先结束就在空闲时自动提交
    /// （0.68 interactiveCli handleFinalSubmit → injectionService，等审批时不提交）。按用户此刻的
    /// 设置如实说，Lead 装与不装两种情况都对。
    fn queue_delivery(&self) -> Option<crate::chat_ui::QueueDelivery> {
        let steering = self
            .resolve()
            .is_some_and(|inst| lead::model_steering(&inst.data_dir));
        Some(if steering {
            crate::chat_ui::QueueDelivery::NextStep
        } else {
            crate::chat_ui::QueueDelivery::TurnEnd
        })
    }

    // 审批模式会话里改走带参重启（launch_options 的 switch: Restart）：Shift+Tab / Ctrl+Y 在有待批
    // 审批时会**自动放行**挂起的调用（G:interactiveCli 29445-29478，research-kimi-gemini §3.4 e5-06
    // 实测 gate-p2 被写入），不再作切换手段。保留维度只为屏幕回显：transcript 与 hook 都不带审批
    // 模式，底栏原文是唯一来源（§3.5：「auto-accept edits」「YOLO Ctrl+Y」「plan Shift+Tab to manual」）。
    fn mode_controls(&self) -> &'static [crate::ModeControl] {
        // default 档底栏只有「Shift+Tab to accept edits」（research §4.2），也认，才读得到「当前是每步都问」
        // ——带参重启要按当前值拼参数，读不到就不重启（做空 H1）。
        static MARKERS: [crate::ModeScreenMarker; 4] = [
            crate::ModeScreenMarker {
                marker: "shift+tab to accept edits",
                value: "default",
            },
            crate::ModeScreenMarker {
                marker: "auto-accept edits",
                value: "autoEdit",
            },
            crate::ModeScreenMarker {
                marker: "yolo ctrl+y",
                value: "yolo",
            },
            crate::ModeScreenMarker {
                marker: "shift+tab to manual",
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

    /// Esc 中断当前回合：运行中状态行自述 `(esc to cancel, Ns)`（probe_tui_script，0.64）。
    fn interrupt_input(&self) -> Option<&'static str> {
        Some("\x1b")
    }
    /// Ctrl+J（裸 LF）在输入框里换行、不提交（tests/probe_tui_script.rs，0.61，2026-10-16）。
    /// 剪贴板贴图（Ctrl-V）虽然可用，但 gemini 会把图片落盘到工作目录的 `.gemini-clipboard/`
    /// 再以 @路径 引用——等于往用户仓库里写文件，故不声明，图片照旧走指令文本。
    fn newline_input(&self) -> Option<&'static str> {
        Some("\n")
    }
    /// 一次写进输入框的一串字里的半角问号会被吞掉（0.68.0 真机，tests/probe_tui_script.rs，
    /// 2026-11-14，只打字不提交）：
    /// - `What is 17+25? Reply now` 一次写入，输入框里是 `What is 17+25 Reply now`；
    ///   `a?b` 成了 `ab`；`ab??cd? ef` 成了 `abcd ef`；
    /// - 问号单独一次写入（`x`、`?`、`y` 分三次写）是 `x?y`，不丢；
    /// - 同一句包成 bracketed paste，原样不丢。
    ///
    /// 对话页的单行消息是一次写入，带半角问号的问句发到 Gemini 就少了问号（全角问号不受影响）。
    /// 带半角问号的单行正文改走粘贴。只对问号声明：别的字符没有取证到丢失，不扩大。
    fn single_line_paste_pattern(&self) -> Option<&'static str> {
        Some(r"\?")
    }
    /// 工具确认框（源码 ToolConfirmationMessage，0.64 未变）。gemini 没有能代为决定的权限 hook，
    /// 屏幕识别是 GUI 给出可操作审批的唯一通道。问句只认**行首是外框 │** 的那一行——模型
    /// 正文里引用同一句不带框线，不该凭空弹卡锁输入框。真机屏（0.61，`--approval-mode default`）：
    /// `│ Allow execution of [Shell]? │` 之下是 `│ ● 1. Allow once │ / 2. Allow for this session /
    /// 3. No, suggest changes (esc)`。
    ///
    /// 额度耗尽框（0.64 真机，2026-10-16）：`│ Usage limit reached for gemini-3.8-flash. │` 下接
    /// `● 1. Keep trying / 2. Stop`，取代输入框。不识别时对话页毫无提示，发出的正文落进这个
    /// 框被丢弃、紧随的回车选中 Keep trying 再撞一次额度——用户只看到「发不了消息」。
    /// 走通用卡（无结构化文法）：确认 = Enter = Keep trying，取消 = Esc = 「Request cancelled」
    /// 并还回输入框（两键均经 probe_tui_script 实测）。
    ///
    /// 以下三条是 0.65.0 的新形态（npm bundle 源码取证，2026-11-12，未实拍）：
    /// - 高负载框：同一个 ProQuotaDialog，首行换成 `We are currently experiencing high demand
    ///   for <模型>.`（useQuotaAndFallback），选项仍以 Keep trying 开头。只认额度耗尽时，
    ///   高负载下发出的正文同样落进这个框被丢弃。
    /// - 沙箱扩权审批：`To run [Shell], allow access to the following?` 下列 `• Network:` /
    ///   `• Write:` / `• Read:`，选项同 Allow once 一套（ToolConfirmationMessage 的
    ///   sandbox_expansion 分支）。只在开了沙箱时出现。
    /// - 提问表单（ask_user，AskUserTool 默认注册）：绿框头行 `Answer Questions`
    ///   （getConfirmationHeader），不识别时对话页不出卡，用户只能去终端回答。
    fn attention_patterns(&self) -> &'static [crate::chat_ui::AttentionPattern] {
        &[
            crate::chat_ui::AttentionPattern {
                id: "gemini:command-approval",
                patterns: &[
                    r"^\s*│\s*allow execution of [^\n]*\?",
                    r"^\s*│\s*apply this change\?",
                    r"^\s*│\s*do you want to proceed\?",
                    r"^\s*│\s*to run \[[^\]\n]+\], allow access to the following\?",
                ],
                // 重绘不清屏，上一个确认框会残留在缓冲上方：取最后一次。
                last: true,
                details: crate::chat_ui::AttentionDetails::FramedBox,
                kind: crate::chat_ui::AttentionKind::Approval,
                confirms: None,
                // 选项角色（0.68.0 npm bundle 的 ToolConfirmationMessage，2026-11-14 逐条核对；屏幕样本仍是
                // 0.61 真机的）：`Allow once`、几种只管本会话的（`Allow for this session`、
                // `Allow tool for this session`、`Allow all server tools for this session`）、几种写进设置的
                // （`Allow for all future sessions`、`Allow this command for all future sessions`、
                // `Allow for this file in all future sessions`、`Allow tool for all future sessions`）、
                // `No, suggest changes (esc)`。
                // 最后一项的值是 `cancel`，与按 Esc 相同：它只是取消这次调用，「suggest changes」指的是
                // 取消之后用户自己再发一条消息，没有单独的说明输入框。所以它是 deny，不是 deny_with_feedback。
                // `Modify with external editor` 会在本机打开外部编辑器，不是放行也不是拒绝，不声明，归 other。
                //
                // unparsed_keys：选项认不出（焦点标记 ● 没抓到）时只留拒绝，Esc 与焦点无关（拒绝项原文就写着
                // `(esc)`）。不给「允许一次」：回车选的是当前焦点项，焦点在哪认不出来就不能替用户按。
                options: &[
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^allow once$",
                        role: crate::chat_ui::OptionRole::AllowOnce,
                        scope: None,
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^allow (?:tool |all server tools )?for this session$",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Session),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^allow (?:tool |this command )?for (?:this file in )?all future sessions$",
                        role: crate::chat_ui::OptionRole::AllowPersistent,
                        scope: Some(crate::chat_ui::PersistScope::Remember),
                        typed: None,
                    },
                    crate::chat_ui::AttentionOptionRule {
                        pattern: r"^no, suggest changes(?: \(esc\))?$",
                        role: crate::chat_ui::OptionRole::Deny,
                        scope: None,
                        typed: None,
                    },
                ],
                unparsed_keys: &[
                    crate::chat_ui::RoleKey {
                        role: crate::chat_ui::OptionRole::Deny,
                        input: "\x1b",
                    },
                ],
            },
            crate::chat_ui::AttentionPattern {
                id: "gemini:usage-limit",
                patterns: &[
                    r"^\s*│\s*usage limit reached\b",
                    r"^\s*│\s*we are currently experiencing high demand\b",
                ],
                last: true,
                details: crate::chat_ui::AttentionDetails::None,
                // 回合中途的额度或高负载提示：CLI 要用户选「继续重试 / 停下」，不是审批也不是启动步骤。
                kind: crate::chat_ui::AttentionKind::Choice,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
            },
            crate::chat_ui::AttentionPattern {
                id: "gemini:question",
                patterns: &[r"^\s*│\s*answer questions\b"],
                // 多问题表单切页重绘，上一页残留在上方：取最后一次。
                last: true,
                details: crate::chat_ui::AttentionDetails::FramedForm,
                kind: crate::chat_ui::AttentionKind::Question,
                confirms: None,
                options: &[],
                unparsed_keys: &[],
            },
        ]
    }
    /// `@路径` 是 gemini 的一等注入语法(官方文档;headless 实测提交 `@绝对路径` 能答出
    /// 文件内容)。版本探测不到时兜底 false。
    fn attachment_mention(&self, version: Option<&str>) -> bool {
        version.is_some()
    }
    /// gemini 没有 `/status`/`/compact`——对应物叫 `/stats` 与 `/compress`。此前前端的通用
    /// fallback 给它补全 `/status`，发进去只会报 unknown command。
    /// Gemini：`skills/` 与共享的 `~/.agents/skills`、`settings.json` 的 MCP 与钩子、
    /// `commands/*.toml`、`extensions/`（位置取自其 bundle 的 `Storage.getUser*Dir`）。
    fn extensions(
        &self,
        inst: &crate::variant::Installation,
    ) -> Vec<crate::extensions::ExtensionGroupDto> {
        use crate::extensions as ext;
        let dir = &inst.data_dir;
        let settings_path = dir.join("settings.json");
        let settings = ext::read_json(&settings_path);
        [
            ext::skills_group(&dir.join("skills"), false),
            ext::shared_agent_skills(),
            ext::mcp_group_from_json(
                &settings_path,
                settings.as_ref().and_then(|s| s.get("mcpServers")),
            ),
            ext::gemini_commands_group(&dir.join("commands")),
            ext::gemini_extensions_group(&dir.join("extensions")),
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
            "/chat",
            "/clear",
            "/compress",
            "/help",
            "/mcp",
            "/memory",
            "/stats",
            "/tools",
        ]
    }
    /// 启动选项（`gemini --help` 0.68）：审批模式四档。内部枚举 default / autoEdit / yolo / plan
    /// （G:273739），对外写法 default / auto_edit / yolo / plan（G:gemini-Q2FNZWQG.js 8876-8935）。
    /// 统一说法按 bundle/policies/*.toml 的策略优先级（research-kimi-gemini §5）。
    /// 四档都显式带参数：「每步都问」就是 `--approval-mode default`，不随 settings 的 defaultApprovalMode
    /// 漂移；新建页按 launch_defaults 预选配置里的那一档并显式传它。
    fn launch_options(&self) -> &'static [crate::LaunchOption] {
        use crate::{Ask, LaunchChoice, LaunchOption, ModeSwitch, ModeTraits};
        static OPTIONS: [LaunchOption; 1] = [LaunchOption {
            id: "approval",
            default: "default",
            choices: &[
                LaunchChoice {
                    id: "default",
                    label: "default",
                    // 显式传：不传时 CLI 用 settings 的 defaultApprovalMode（可能是 auto_edit / plan），
                    // 界面说「每步都问」实际却不是（做空审查 M1）。
                    args: &["--approval-mode", "default"],
                    risk: false,
                    // 「default (prompt for approval)」；工作区内已知安全的命令（isKnownSafeCommand）
                    // 不问，其余都问。
                    traits: Some(ModeTraits::new(Ask::Always, Ask::Always, false)),
                    desc: Some("gemini.approval.default"),
                    server: None,
                },
                LaunchChoice {
                    id: "autoEdit",
                    label: "auto_edit",
                    args: &["--approval-mode", "auto_edit"],
                    risk: false,
                    // 「auto_edit (auto-approve edit tools)」；工作区外的路径仍问，命令照 default。
                    traits: Some(ModeTraits::new(Ask::Never, Ask::Always, false)),
                    desc: Some("gemini.approval.autoEdit"),
                    server: None,
                },
                LaunchChoice {
                    id: "plan",
                    label: "plan",
                    args: &["--approval-mode", "plan"],
                    risk: false,
                    // 「plan (read-only mode)」；只能写 plans 目录下的 .md，shell 工具不可用
                    // （实测 `Tool "run_shell_command" not found`）。
                    traits: Some(ModeTraits::new(Ask::Blocked, Ask::Blocked, true)),
                    desc: Some("gemini.approval.plan"),
                    server: None,
                },
                LaunchChoice {
                    id: "yolo",
                    label: "yolo",
                    args: &["--yolo"],
                    risk: true,
                    // 「yolo (auto-approve all tools)」；危险命令也放行（yolo.toml 优先级 998）。
                    traits: Some(ModeTraits::new(Ask::Never, Ask::Never, false)),
                    desc: Some("gemini.approval.yolo"),
                    server: None,
                },
            ],
            switch: ModeSwitch::Restart,
            resume_upgrade_only: false,
            in_session_only: false,
        }];
        &OPTIONS
    }
    /// 自定义命令是 TOML：`~/.gemini/commands/**.toml` + 项目级 `.gemini/commands/`；
    /// 子目录按 `:` 命名空间（`commands/git/commit.toml` → `/git:commit`），描述取顶层
    /// `description` 键。
    fn custom_commands(&self) -> Option<&'static crate::CustomCommandSpec> {
        static SPEC: crate::CustomCommandSpec = crate::CustomCommandSpec {
            user_dir: Some("commands"),
            project_dir: Some(".gemini/commands"),
            ext: "toml",
            namespace_sep: Some(":"),
        };
        Some(&SPEC)
    }

    /// 只有 npm 一条安装路，官方没有 `curl|sh` 引导脚本——用 [`InstallScript::Command`] 直接跑
    /// `npm i -g @google/gemini-cli`（两平台命令一致）。前提是本机有 node/npm；没有则安装子进程
    /// 会以「npm 找不到」失败，用户看到重试按钮——这合理，没有 node 本就装不了 gemini-cli。
    fn install_script(&self, _windows: bool) -> Option<crate::install::InstallScript> {
        Some(crate::install::InstallScript::Command {
            body: "npm install -g @google/gemini-cli",
            unix_shell: "bash",
        })
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
    /// OAuth 已死（个人版 Code Assist 被停），API Key 是个人用户唯一的活路——而 gemini 没有
    /// 「输入 key」的登录子命令，只能由宿主替用户把 key 落进 `~/.gemini/.env`。见 account.rs。
    fn api_key_login(&self) -> Option<&'static dyn crate::account::ApiKeyLoginCap> {
        Some(&account::API_KEY_LOGIN)
    }
    fn proxy(&self) -> Option<&'static crate::proxy::ProxySpec> {
        Some(&PROXY)
    }
    fn relay(&self) -> Option<&'static dyn crate::RelayCap> {
        Some(&RELAY)
    }
    /// 0.65 日志有 token 计数，但没有可靠的模型窗口容量；不能把计数冒充占用百分比。
    fn provides_context(&self) -> bool {
        false
    }
}

// ═══ API 中转 ═══
//
// gemini-cli 认这四个环境变量（实测 0.50 bundle）：`GEMINI_API_KEY`（密钥）、`GOOGLE_GEMINI_BASE_URL`
// （自定义端点，代码里 `baseUrl || process.env["GOOGLE_GEMINI_BASE_URL"]`）、`GEMINI_MODEL`（模型）、
// `GEMINI_DEFAULT_AUTH_TYPE`（强制走 API-key 认证，取值 `gemini-api-key`——否则 TUI 仍可能去跑 OAuth）。
//
// **只有一种协议**：中转端点必须讲 Gemini 自己的 generateContent 格式（不是 OpenAI/Anthropic），
// 故 protocols 留空（validate 对空协议表不校验），auth 只有 API Key 一种。

static RELAY: GeminiRelay = GeminiRelay;
static RELAY_AUTH: [crate::RelayOption; 1] = [crate::RelayOption {
    value: "api_key",
    label: "API Key",
}];
static RELAY_SUGGESTIONS: [crate::RelaySuggestionGroup; 1] = [crate::RelaySuggestionGroup {
    protocol: "",
    models: &["gemini-2.5-pro", "gemini-2.5-flash"],
}];

pub struct GeminiRelay;

impl crate::RelayCap for GeminiRelay {
    fn ui(&self) -> crate::RelayUi {
        crate::RelayUi {
            protocols: &[],
            auth_modes: &RELAY_AUTH,
            default_protocol: "",
            default_auth: "api_key",
            suggestions: &RELAY_SUGGESTIONS,
            env_options: &[],
        }
    }
    fn launch_env(&self, config: crate::RelayConfig<'_>, key: &str) -> Vec<(String, String)> {
        vec![
            ("GEMINI_API_KEY".into(), key.into()),
            (
                "GOOGLE_GEMINI_BASE_URL".into(),
                config.base_url.trim().trim_end_matches('/').into(),
            ),
            ("GEMINI_MODEL".into(), config.model.trim().into()),
            // 强制 API-key 认证，别让 TUI 回到 OAuth。取值取自 bundle 的 AuthType 常量。
            ("GEMINI_DEFAULT_AUTH_TYPE".into(), "gemini-api-key".into()),
        ]
    }
    fn augment_argv(
        &self,
        _config: crate::RelayConfig<'_>,
        _has_secret: bool,
        argv: Vec<String>,
    ) -> Vec<String> {
        argv // 模型经 GEMINI_MODEL 注入，不改 argv
    }
    fn model_request(&self, _config: crate::RelayConfig<'_>) -> crate::RelayModelRequest {
        // 中转端点讲 Gemini 协议，`/models` 用 x-api-key 取（取不到也无妨，靠 suggestions 兜底）。
        crate::RelayModelRequest {
            auth: crate::RelayModelAuth::ApiKey,
            anthropic_version: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_process_scope_rejects_unknown_and_acp() {
        use crate::registry::ProcessScope;
        assert_eq!(Gemini.process_scope(&[]), ProcessScope::Unknown);
        assert_eq!(
            Gemini.process_scope(&["node".into(), "unrelated.js".into()]),
            ProcessScope::Unknown
        );
        let argv = vec![
            "node".into(),
            "C:/npm/node_modules/@google/gemini-cli/bundle/gemini.js".into(),
        ];
        assert_eq!(Gemini.process_scope(&argv), ProcessScope::Exclusive);
        let mut acp = argv;
        acp.push("--acp=true".into());
        assert_eq!(Gemini.process_scope(&acp), ProcessScope::Shared);
        // 0.68 重启自身的真实形态：node 选项在前，gemini.js 在第三项之后（做空复现，本机 63GB 内存）。
        let relaunched: Vec<String> = vec![
            "C:/Program Files/nodejs/node.exe".into(),
            "--no-warnings=DEP0040".into(),
            "--max-old-space-size=31553".into(),
            "C:/Users/x/AppData/Roaming/npm/node_modules/@google/gemini-cli/bundle/gemini.js"
                .into(),
            "--resume".into(),
        ];
        assert_eq!(Gemini.process_scope(&relaunched), ProcessScope::Exclusive);
        // node 选项后跟的不是 gemini.js，仍不认领。
        let other: Vec<String> = vec![
            "node".into(),
            "--max-old-space-size=1".into(),
            "server.js".into(),
            "gemini.js".into(),
        ];
        assert_eq!(Gemini.process_scope(&other), ProcessScope::Unknown);
    }
    use crate::RelayCap;

    /// 代理：HTTP 走 HTTPS_PROXY/HTTP_PROXY；gemini **支持 SOCKS**（socks-proxy-agent），
    /// socks 串同样从 HTTPS_PROXY 读。
    #[test]
    fn proxy_accepts_http_and_socks() {
        let http = PROXY.env_for("http://127.0.0.1:7890");
        assert!(http.iter().any(|(k, _)| *k == "HTTPS_PROXY"));
        // socks 不被拒（socks=true），且写进 HTTPS_PROXY 而非 ALL_PROXY。
        let socks = PROXY.env_for("socks5://127.0.0.1:1080");
        assert!(!socks.is_empty(), "gemini 支持 SOCKS，不该返回空");
        assert!(socks
            .iter()
            .any(|(k, v)| *k == "HTTPS_PROXY" && v == "socks5://127.0.0.1:1080"));
        assert!(PROXY.accepts("socks5://h:1").is_ok());
    }

    /// 中转：把 gemini-cli 指向自定义端点的四个环境变量（变量名实测自 0.50 bundle）。
    #[test]
    fn relay_points_gemini_at_custom_endpoint() {
        let cfg = crate::RelayConfig {
            base_url: "https://relay.example/v1/", // 尾斜杠应被去掉
            model: " gemini-2.5-pro ",             // 前后空白应被 trim
            protocol: "",
            auth: "api_key",
            env_options: &[],
        };
        let env: std::collections::HashMap<_, _> =
            RELAY.launch_env(cfg, "sk-key").into_iter().collect();
        assert_eq!(
            env.get("GEMINI_API_KEY").map(String::as_str),
            Some("sk-key")
        );
        assert_eq!(
            env.get("GOOGLE_GEMINI_BASE_URL").map(String::as_str),
            Some("https://relay.example/v1")
        );
        assert_eq!(
            env.get("GEMINI_MODEL").map(String::as_str),
            Some("gemini-2.5-pro")
        );
        // 强制 API-key 认证，否则 TUI 可能回到 OAuth。
        assert_eq!(
            env.get("GEMINI_DEFAULT_AUTH_TYPE").map(String::as_str),
            Some("gemini-api-key")
        );
    }

    /// gemini 的 hook timeout 单位是毫秒（hookRunner `setTimeout`，默认 60000）。写成 claude
    /// 的秒语义（5）就是 5ms——reporter 一 spawn 即被整树击杀，每个 hook 都超时失败，
    /// 会话永远落不了库。绊线：任何事件的 timeout 掉回秒量级即失败。
    #[test]
    fn hook_timeouts_are_milliseconds_not_seconds() {
        for ev in EVENTS {
            assert!(
                ev.timeout >= 1_000,
                "{} 的 timeout={} 是秒语义残留，gemini 会按毫秒解释",
                ev.name,
                ev.timeout
            );
        }
    }

    /// Windows 上 gemini 恒经 PowerShell 执行 hook：引号开头的命令是字符串表达式、以 code 1
    /// 失败，必须带调用运算符 `&`。非 Windows（bash -c）不得带——`&` 在 bash 是后台运算符。
    /// 两种形态都必须被认领（升级迁移 + 幂等）。
    #[test]
    fn hook_command_is_powershell_invocable_on_windows() {
        let cmd = HOOKS.command.render("C:/x/meowo-reporter.exe", "gemini");
        if cfg!(windows) {
            assert_eq!(cmd, "& \"C:/x/meowo-reporter.exe\" --provider gemini");
        } else {
            assert_eq!(cmd, "\"C:/x/meowo-reporter.exe\" --provider gemini");
        }
        // 新旧两种形态都认领（旧＝无 `&` 的历史接线，升级时要能原地更新而非重复追加）。
        for form in [
            "& \"C:/x/meowo-reporter.exe\" --provider gemini",
            "\"C:/x/meowo-reporter.exe\" --provider gemini",
        ] {
            assert_eq!(
                HOOKS.command.claim(form, "gemini").as_deref(),
                Some("C:/x/meowo-reporter.exe"),
                "未认领形态：{form}"
            );
        }
    }

    /// 防连坐绊线：照 kimi 的教训，一条非法 event 有可能让**全部** hooks 静默失效。
    #[test]
    fn events_all_in_upstream_whitelist() {
        for ev in EVENTS {
            assert!(
                EVENT_WHITELIST.contains(&ev.name),
                "{} 不在 Gemini 事件白名单",
                ev.name
            );
        }
    }

    /// 声明的每个事件都必须译得出规范名，且译出的必须是 dispatch 真正消化的那几个——
    /// 少译一个，该事件就会静静地什么都不做。
    #[test]
    fn every_declared_event_maps_to_a_canonical_name_dispatch_handles() {
        // dispatch 的消化面（见 meowo_reporter::dispatch）。
        const CANONICAL: [&str; 8] = [
            "SessionStart",
            "UserPromptSubmit",
            "PostToolUse",
            "Stop",
            "SessionEnd",
            "PreCompact",
            "PostCompact",
            "Notification",
        ];
        for ev in EVENTS {
            let mapped = Gemini.canonical_event(ev.name);
            assert!(
                CANONICAL.contains(&mapped),
                "{} 译成了 dispatch 不认的 {mapped}",
                ev.name
            );
        }
        // 逐条钉死映射本身——写反一条（如 AfterAgent → PostToolUse）不会有任何报错，
        // 只会让「回合结束」永远不发生，卡片永远停在运行中。
        assert_eq!(Gemini.canonical_event("BeforeAgent"), "UserPromptSubmit");
        assert_eq!(Gemini.canonical_event("AfterAgent"), "Stop");
        assert_eq!(Gemini.canonical_event("AfterTool"), "PostToolUse");
        assert_eq!(Gemini.canonical_event("PreCompress"), "");
        // 同名的原样透传。
        assert_eq!(Gemini.canonical_event("SessionStart"), "SessionStart");
        assert_eq!(Gemini.canonical_event("SessionEnd"), "SessionEnd");
    }

    #[test]
    fn hooks_live_in_settings_json_under_data_dir() {
        let home = std::env::temp_dir().join(format!("meowo-gemini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".gemini")).unwrap();

        let inst = VARIANTS[0]
            .probe(id::GEMINI, &home)
            .expect("数据目录在 → 应命中");
        assert_eq!(
            inst.config_path(),
            home.join(".gemini").join("settings.json")
        );
        assert!(inst.is_configured());
        // 凭据与配置同处 data_dir（不像 opencode 那样把配置与数据分了家）。
        assert_eq!(
            inst.credentials_path(),
            Some(home.join(".gemini").join("oauth_creds.json"))
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 登录入口 = **裸启动**。gemini 没有登录子命令（`gemini --help` 里只有 mcp / extensions /
    /// skills / hooks / gemma），跑它自己首次就会引导你选认证方式。
    ///
    /// 这条曾经是反的：gemini 被声明成「无鉴权」，`login_argv()` 于是返回 None，后端回一句
    /// 「该 agent 未声明登录入口」——而前端仍旧亮着登录按钮，点下去只得到「拉起登录失败」。
    #[test]
    fn login_entry_is_a_bare_launch_not_a_subcommand() {
        let _env = crate::env_guard();
        let inst = Gemini.resolve().expect("总能推出默认落点");
        let login = inst.login_argv().expect("gemini 有登录入口（裸启动）");
        assert_eq!(
            login,
            inst.launch_argv(),
            "登录 argv 就该是启动 argv 本身，不多任何子命令"
        );
    }

    /// npm 全局的 js 入口必须走 `node` 包装：直接拿裸名 `gemini` 在 Windows 上解析不出可执行
    /// （npm 只放了 `.cmd`/`.ps1`，没有 `.exe`）。
    #[test]
    fn npm_global_yields_node_prefixed_argv() {
        let home = std::env::temp_dir().join(format!("meowo-gemini-npm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let js = crate::join_rel(
            &home,
            "AppData/Roaming/npm/node_modules/@google/gemini-cli/bundle/gemini.js",
        );
        std::fs::create_dir_all(js.parent().unwrap()).unwrap();
        std::fs::write(&js, b"").unwrap();

        // 用 USERPROFILE 那条候选（APPDATA 指向真实机器，测试里不该依赖它）。
        let _env = crate::env_guard();
        std::env::remove_var("APPDATA");
        std::env::set_var("USERPROFILE", &home);

        let argv = LAUNCH.probe(None, Some(&home)).expect("js 在 → 应命中");
        assert_eq!(argv[0], "node", "gemini 没有原生可执行，必须由 node 拉起");
        assert!(argv[1].ends_with("gemini.js"));

        let _ = std::fs::remove_dir_all(&home);
    }
}
