//! Agent 插件层：一个 agent = 一个自包含模块，声明自己的**变体表**（同一 agent 的多个版本形态），
//! 由注册表统一驱动。承载「身份 + 变体探测 + 路径解析 + hooks 配置格式 + 鉴权参数」这层纯逻辑
//! ——它跨 `meowo-reporter`（hook 子进程）与 `meowo-app`（GUI）两个二进制共用，此前散在两边，
//! kimi 新旧版差异只能靠各处临时 `if`/fallback 硬凑。
//!
//! 本 crate 只依赖 std/serde/toml_edit：不碰文件写入、不联网、不依赖 Tauri。落盘与联网各由
//! meowo-app 完成，但都以本层探测出的 [`Installation`] 为输入，不再各自重推路径。
//!
//! 迁移状态（详见 `docs/architecture/agent-plugin.md`）：
//!
//! - Phase 1 ✅ 身份收敛：`AgentId` 是全项目唯一的 agent 身份类型，`meowo_store::ProviderKey`
//!   枚举已删除，DB 的 provider 列退化为原样字符串。解析走 [`resolve`]，未知 id 不再冒名默认 agent。
//! - Phase 2 ✅ 注册表合一：`meowo-reporter` 的 `Agent` trait 与那张并行注册表已折进本 crate 的
//!   [`AgentPlugin`]——进程名、resume/启动 argv、安装脚本、标签页行为是声明式方法，会话遥测走
//!   [`caps::TelemetryCap`] 能力槽。transcript 解析亦随之迁入（[`transcript`] + `plugins/claude/`）。
//! - Phase 3 ✅ 端口注入：[`ports::HttpPort`] / [`ports::KeychainPort`] 由宿主注入，于是账号
//!   （[`account::AccountCap`]）与接线副作用（[`wiring::WiringCap`]）也住进了 `plugins/<id>/`。
//!   本 crate 不依赖 HTTP 栈，插件层没有一行 `#[cfg(target_os)]`。
//! - Phase 4 ✅ 前端描述符：`list_agents()` 下发 id/展示名/安装态，前端不再自带 agent 名单。
//!   图标与品牌色留在前端资产表——位图 logo 与主题相关的颜色是资产，不是能塞进字段的数据。
//!
//! 终局验收：加一个 agent 只需新增 `plugins/<new>/` 与 `registry.rs` 一行（后端零其它改动）；
//! 前端只需在资产表补一个图标，不补也只是显示中性徽标 + id，不会崩、不会冒名成 claude。

pub mod account;
pub mod auth;
pub mod caps;
pub mod codec;
pub mod fsutil;
pub mod config;
pub mod id;
pub mod launch;
pub mod plugins;
pub mod ports;
pub mod registry;
pub mod transcript;
pub mod variant;
pub mod wiring;

pub use account::{Account, AccountCap, ProviderUsage, UsageKind, UsageLane, USAGE_UNSUPPORTED};
pub use auth::{AuthScheme, CredentialSource, OAuthRefresh};
pub use caps::{ContextUsage, HookContext, StopOutputs, TelemetryCap};
pub use ports::{Body, HttpError, HttpPort, HttpRequest, KeychainPort, NoKeychain, Ports};
pub use config::{CommandSpec, ConfigFormat, EnsureOutcome, HookEvent, HookSpec, MissingConfig, RepairReason};
pub use id::AgentId;
pub use launch::{exe_on_path, LaunchCandidate, LaunchSpec, Root};
pub use registry::{all, by_id, installation, is_agent_process, resolve, AgentPlugin, DEFAULT_ID};
pub use transcript::{
    default_resolve_cwd, TranscriptCache, TranscriptInfo, TranscriptParser, TranscriptSpec, TurnError,
};
pub use variant::{DataDirSpec, Installation, Variant};
pub use wiring::{backup_once, wire_hooks, WiringCap, WiringContext};

use std::path::{Path, PathBuf};

/// 用户 home（Windows `USERPROFILE` 优先，回退 `HOME`）。所有目录解析的根。
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

/// 按 `/` 分段拼接相对路径。直接 `join("a/b")` 在 Windows 上会拼出混合分隔符（`dir\a/b`），
/// 虽多数 API 容忍，但比较/展示都会走样，故统一分段。
pub(crate) fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|s| !s.is_empty()).fold(base.to_path_buf(), |p, s| p.join(s))
}

/// 可执行文件名：Windows 补 `.exe`。
pub(crate) fn exe_file_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_rel_splits_segments() {
        let p = join_rel(Path::new("/base"), "credentials/kimi-code.json");
        assert_eq!(p, Path::new("/base").join("credentials").join("kimi-code.json"));
    }
}
