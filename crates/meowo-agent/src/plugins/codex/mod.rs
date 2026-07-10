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
pub mod setup;
pub mod telemetry;

use crate::{
    caps::TelemetryCap,
    auth::{AuthScheme, CredentialSource},
    config::{CommandSpec, ConfigFormat, HookEvent, HookSpec, MissingConfig},
    id::{self, AgentId},
    launch::{LaunchCandidate, LaunchSpec, Root},
    registry::AgentPlugin,
    variant::{DataDirSpec, Variant},
};

/// 接线事件集：dispatch 消化面 ∩ codex 0.142 支持面。无 SessionEnd（codex 不支持，会话收尾靠
/// Stop + liveness）；不配 PreToolUse（其 matcher 目标是 claude 专属工具）。
static EVENTS: [HookEvent; 5] = [
    HookEvent::plain("SessionStart"),
    HookEvent::plain("UserPromptSubmit"),
    HookEvent::plain("PostToolUse"),
    HookEvent::plain("Stop"),
    HookEvent::plain("PermissionRequest"),
];

/// hooks.json 不存在时从空态建——与 kimi「config.toml 缺失即未登录」不同是有意的：
/// 此处不存在不代表用户手改过畸形内容，codex 本就允许该文件缺席。
static HOOKS: HookSpec = HookSpec {
    config_rel: "hooks.json",
    format: ConfigFormat::CodexJson,
    missing: MissingConfig::CreateFrom("{\"hooks\":{}}"),
    events: &EVENTS,
    command: CommandSpec { quote_exe: true, with_provider: true },
};

/// codex 的 `auth.json` 由 CLI 自己维护（含 OIDC id_token），Meowo 只读不刷新 → `refresh: None`。
/// 用量走 rollout 文件与 auth.json 内的字段，无独立 base_url。
static AUTH: AuthScheme = AuthScheme {
    credentials: CredentialSource::File("auth.json"),
    refresh: None,
    default_base_url: "",
    // `codex login`（另有 `codex login status`，但 kimi 无 status 子命令，登录态检测统一走读凭据）。
    login_args: &["login"],
};

static LAUNCH: LaunchSpec = LaunchSpec {
    stem: "codex",
    candidates: &[
        LaunchCandidate::Exe { root: Root::Home, sub: ".bun/bin" },
        // npm 全局前缀：Windows 上是 %APPDATA%\npm，某些环境 APPDATA 缺失则由 USERPROFILE 推。
        LaunchCandidate::NodeScript { root: Root::Env("APPDATA"), rel: "npm/node_modules/@openai/codex/bin/codex.js" },
        LaunchCandidate::NodeScript {
            root: Root::Env("USERPROFILE"),
            rel: "AppData/Roaming/npm/node_modules/@openai/codex/bin/codex.js",
        },
        // `current` 是指向当前 release 的 junction/symlink，跨平台稳定。
        LaunchCandidate::Exe { root: Root::DataDir, sub: "packages/standalone/current/bin" },
    ],
};

static VARIANTS: [Variant; 1] = [Variant {
    tag: "stable",
    data_dir: DataDirSpec { env: Some("CODEX_HOME"), candidates: &[".codex"] },
    hooks: &HOOKS,
    auth: Some(&AUTH),
    launch: &LAUNCH,
}];

pub struct Codex;

impl AgentPlugin for Codex {
    fn id(&self) -> AgentId {
        id::CODEX
    }
    fn display_name(&self) -> &'static str {
        "Codex"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
    fn process_names(&self) -> &'static [&'static str] {
        // 会话本体是原生 codex 二进制；npm 包装时它由 node 启动但 hook 由 codex 自身触发，上溯命中
        // codex(.exe) 即可。不收 node.exe（过宽，会把任意 node 进程误判为 agent）。
        &["codex", "codex.exe"]
    }
    fn resume_args(&self) -> &'static [&'static str] {
        &["resume"]
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
        Some(crate::install::InstallScript {
            url: if windows {
                "https://github.com/openai/codex/releases/latest/download/install.ps1"
            } else {
                "https://github.com/openai/codex/releases/latest/download/install.sh"
            },
            unix_shell: "sh", // 官方命令写的就是 `| sh`
        })
    }
    // sets_terminal_tab_title / writes_tab_token 均取默认 false：
    // codex 不写「任务标题」式标签名（meowo-app 无法按任务名匹配），且它持续用 SetWindowTitle 管理
    // 标签标题(spinner+project，如 "⠹ larry")，会盖掉我们写的任何 token，无 session_id 组件、无禁用
    // 开关可绕过(实测 0.142.3=当前最新发布版)。其源码里「tui.terminal_title=[] 关闭标题管理」只在未
    // 发布主干，已发布版 [] 反而 clear 成终端默认(路径)。故 codex 的精确切标签暂不可达，meowo-app 走
    // 窗口级兜底。待 codex 发布 [] 禁用后，覆写 writes_tab_token 返回 true 即与 kimi 同。
    fn telemetry(&self) -> Option<&'static dyn TelemetryCap> {
        Some(&telemetry::TELEMETRY)
    }
    fn account(&self) -> Option<&'static dyn crate::account::AccountCap> {
        Some(&account::ACCOUNT)
    }
    fn wiring(&self) -> Option<&'static dyn crate::wiring::WiringCap> {
        Some(&setup::WIRING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_and_credentials_sit_under_data_dir() {
        let home = std::env::temp_dir().join(format!("meowo-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".codex")).unwrap();

        let v = &VARIANTS[0];
        let dir = v.data_dir.candidates.iter().map(|c| home.join(c)).find(|p| p.is_dir()).unwrap();
        let inst = v.installation_at(id::CODEX, dir, Some(&home));
        assert_eq!(inst.config_path(), home.join(".codex").join("hooks.json"));
        assert_eq!(inst.credentials_path(), Some(home.join(".codex").join("auth.json")));
        assert!(inst.is_configured());
        // 三处候选都没有 → 回退裸名。
        assert_eq!(inst.launch_argv(), vec!["codex".to_string()]);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn standalone_install_under_data_dir_is_found() {
        // 「装完仍显示未安装」的回归：PATH 是旧快照，但独立安装的固定路径能直查到。
        let home = std::env::temp_dir().join(format!("meowo-codex-standalone-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let bin = home.join(".codex").join("packages").join("standalone").join("current").join("bin");
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
        assert_eq!(inst.launch_argv(), vec![home.join(".bun").join("bin").join(&name).to_string_lossy().into_owned()]);

        let _ = std::fs::remove_dir_all(&home);
    }
}
