//! claude（Anthropic Claude Code）插件。数据目录只有一种（`~/.claude`），特点有三：
//!
//! 1. hooks 条目**带 `matcher`**——同一事件下按 matcher 与用户自有 hook 共存
//!    （如 `PreToolUse:Bash` 预检 vs 我方的 `PreToolUse:AskUserQuestion`）。
//! 2. hooks 与 `statusLine` 同住 `settings.json`。statusLine 的包装脚本要落盘、要 db_path，
//!    不在本层——见 meowo-app 的 `setup::Amend`。
//! 3. 凭据在 macOS 走登录 Keychain，其它平台走 `~/.claude/.credentials.json`。
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
///
/// **此表须与 `scripts/install-hooks.mjs` 的 `SPECS` 保持一致**——由 meowo-app 的绊线测试守卫。
static EVENTS: [HookEvent; 8] = [
    HookEvent::matched("SessionStart", "*"),
    HookEvent::matched("UserPromptSubmit", "*"),
    HookEvent::matched("PostToolUse", "*"),
    HookEvent::matched("Stop", "*"),
    HookEvent::matched("SessionEnd", "*"),
    HookEvent::matched("PermissionRequest", "*"),
    HookEvent::matched("PreToolUse", "AskUserQuestion"),
    HookEvent::matched("PreToolUse", "ExitPlanMode"),
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
    },
};

/// Claude Code 公开 OAuth client id 与刷新端点。macOS 把凭据存进登录 Keychain 的通用密码
/// （service = `Claude Code-credentials`），其它平台落 `<data>/.credentials.json`。
/// `account` 是写回 Keychain 时的条目名兜底值——读得到实际 account 时以实际值为准。
///
/// 用量端点（`api.anthropic.com/api/oauth/usage`）不在此处：`AuthScheme` 只管「凭据在哪 +
/// 怎么刷新」，用量是 account 侧的事。
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
    login_args: &["auth", "login"],
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
        candidates: &[".claude"],
    },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
}];

pub struct Claude;

impl AgentPlugin for Claude {
    fn id(&self) -> AgentId {
        id::CLAUDE
    }
    fn display_name(&self) -> &'static str {
        "Claude Code"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["claude", "claude.exe"]
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["--resume"]
    }
    fn install_script(&self, windows: bool) -> Option<String> {
        Some(if windows {
            "irm https://claude.ai/install.ps1 | iex".into()
        } else {
            "curl -fsSL https://claude.ai/install.sh | bash".into()
        })
    }
    /// claude 把任务标题写进标签页 → meowo-app 可按标题精确切标签，无需我们补 token。
    fn sets_terminal_tab_title(&self) -> bool {
        true
    }
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(argv, vec![exe.to_string_lossy().into_owned(), "auth".into(), "login".into()]);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn events_cover_the_eight_specs_with_matchers() {
        assert_eq!(EVENTS.len(), 8);
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
    }
}
