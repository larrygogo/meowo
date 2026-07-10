//! kimi 插件。两个变体——这正是变体层存在的理由：
//!
//! | tag | 发行 | 数据目录 | hooks 默认形态 | 可执行 |
//! |---|---|---|---|---|
//! | `modern` | Node 版 **Kimi Code** | `~/.kimi-code` | 无顶层 `hooks` 键 | `<data>/bin/kimi` |
//! | `legacy` | 旧 Python 版 **kimi-cli** | `~/.kimi` | `hooks = []` 空内联数组 | `~/.local/bin/kimi` 等 |
//!
//! 两者的 hook 配置格式与 hook stdin 载荷（session_id/cwd/hook_event_name）实测一致，故共用
//! 同一份 [`HookSpec`]——差的只是目录与「空内联数组」这一形态，都已在声明里表达。

pub mod account;
pub mod telemetry;

use crate::{
    caps::TelemetryCap,
    auth::{AuthScheme, CredentialSource, OAuthRefresh},
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig, RepairReason},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 接线事件集。PermissionRequest = kimi 交互式等待用户审批前触发（官方源码确认，observation-only），
/// 用于卡片「待交互」显示。
static EVENTS: [HookEvent; 6] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    HookEvent::plain("SessionEnd"),
    HookEvent::plain("PermissionRequest"),
];

/// kimi 0.20 支持的全部 hook 事件（HOOK_EVENT_TYPES）。一条非法 event 会让 kimi **静默禁用全部**
/// hooks（源码 salvageConfigData），故 EVENTS 有针对本表的绊线测试。
pub const EVENT_WHITELIST: [&str; 16] = [
    "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "PermissionResult",
    "UserPromptSubmit", "Stop", "StopFailure", "Interrupt", "SessionStart", "SessionEnd",
    "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "Notification",
];

/// config.toml 由 `kimi login` 生成——不存在即「需先登录」，不凭空创建。
/// command 不加引号：与 kimi 现存配置的书写形态一致，避免无谓改写用户在用的条目。
static HOOKS: HookSpec = HookSpec {
    config_rel: "config.toml",
    format: ConfigFormat::KimiToml,
    missing: MissingConfig::Fail(RepairReason::NeedLogin),
    events: &EVENTS,
    command: CommandSpec { quote_exe: false, with_provider: true },
};

/// 来源：kimi-code 开源包 `packages/oauth/src/constants.ts`。
const AUTH_MODERN: AuthScheme = AuthScheme {
    credentials: CredentialSource::File("credentials/kimi-code.json"),
    refresh: Some(OAuthRefresh {
        token_url: "https://auth.kimi.com/api/oauth/token",
        client_id: "17e5f671-d194-4dfb-9706-5516cb48c098",
    }),
    default_base_url: "https://api.kimi.com/coding/v1",
    // `kimi login`。config.toml 正是由它生成——故 MissingConfig::Fail(NeedLogin) 的提示可直接
    // 引导用户点登录（两者指向同一个动作）。
    login_args: &["login"],
};

/// 旧 Python 版的凭据布局与新版相同（实测 `~/.kimi/credentials/kimi-code.json` 字段一致）。
/// **client_id 未经证实**：若刷新 token 返回 `invalid_client`，就把这里换成旧版的值——
/// 变体层的意义正在于此，届时只改这一个 const，account 侧无需再动。
const AUTH_LEGACY: AuthScheme = AUTH_MODERN;

static LAUNCH_MODERN: LaunchSpec = LaunchSpec {
    stem: "kimi",
    candidates: &[
        LaunchCandidate::Exe { root: Root::DataDir, sub: "bin" },
        LaunchCandidate::Exe { root: Root::Home, sub: ".kimi-code/bin" },
    ],
};

/// 旧版常经 uv/pipx 装到 `~/.local/bin`，不在数据目录下。
static LAUNCH_LEGACY: LaunchSpec = LaunchSpec {
    stem: "kimi",
    candidates: &[
        LaunchCandidate::Exe { root: Root::DataDir, sub: "bin" },
        LaunchCandidate::Exe { root: Root::Home, sub: ".kimi/bin" },
        LaunchCandidate::Exe { root: Root::Home, sub: ".local/bin" },
    ],
};

static VARIANTS: [Variant; 2] = [
    Variant {
        tag: "modern",
        data_dir: DataDirSpec { env: Some("KIMI_SHARE_DIR"), candidates: &[".kimi-code"] },
        hooks: &HOOKS,
        auth: Some(&AUTH_MODERN),
        launch: &LAUNCH_MODERN,
    },
    Variant {
        tag: "legacy",
        data_dir: DataDirSpec { env: Some("KIMI_SHARE_DIR"), candidates: &[".kimi"] },
        hooks: &HOOKS,
        auth: Some(&AUTH_LEGACY),
        launch: &LAUNCH_LEGACY,
    },
];

pub struct Kimi;

impl AgentPlugin for Kimi {
    fn id(&self) -> AgentId {
        id::KIMI
    }
    fn display_name(&self) -> &'static str {
        "Kimi Code"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["kimi", "kimi.exe"]
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["-r"]
    }
    fn install_script(&self, windows: bool) -> Option<String> {
        // 装当前 Node 版 Kimi Code（装到 ~/.kimi-code/bin/kimi.exe，与 modern 变体的候选一致）。
        // 注意路径里的 `/kimi-code/`——不带它的 code.kimi.com/install.ps1 装的是旧 Python `kimi-cli`
        // （落到 ~/.local/bin/kimi-cli.exe，检测不到）。
        Some(if windows {
            "irm https://code.kimi.com/kimi-code/install.ps1 | iex".into()
        } else {
            "curl -LsSf https://code.kimi.com/kimi-code/install.sh | bash".into()
        })
    }
    /// kimi 不写标签标题、也不抢 → 由 meowo-reporter 在 hook 时补 session_id token，
    /// meowo-app 据此精确切到该标签（已验证）。
    fn writes_tab_token(&self) -> bool {
        true
    }
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
    fn account(&self) -> Option<&'static dyn crate::account::AccountCap> {
        Some(&account::ACCOUNT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Installation;
    use std::path::Path;

    /// 直接对变体表 probe（不碰真实 home，也不读 env）。
    fn probe_at(home: &Path) -> Option<Installation> {
        VARIANTS.iter().find_map(|v| {
            let dir = v.data_dir.candidates.iter().map(|c| home.join(c)).find(|p| p.is_dir())?;
            Some(v.installation_at(id::KIMI, dir, Some(home)))
        })
    }

    #[test]
    fn events_all_in_upstream_whitelist() {
        // 防连坐绊线：一条非法 event 会让 kimi 静默禁用全部 hooks。
        for ev in EVENTS {
            assert!(EVENT_WHITELIST.contains(&ev.name), "{} 不在 kimi 0.20 事件白名单", ev.name);
        }
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
            Some(home.join(".kimi").join("credentials").join("kimi-code.json"))
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
