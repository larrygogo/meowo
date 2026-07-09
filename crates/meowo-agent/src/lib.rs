//! Agent 插件层：一个 agent = 一个自包含模块，声明自己的**变体表**（同一 agent 的多个版本形态），
//! 由注册表统一驱动。承载「身份 + 变体探测 + 路径解析 + hooks 配置格式 + 鉴权参数」这层纯逻辑
//! ——它跨 `meowo-reporter`（hook 子进程）与 `meowo-app`（GUI）两个二进制共用，此前散在两边，
//! kimi 新旧版差异只能靠各处临时 `if`/fallback 硬凑。
//!
//! 本 crate 只依赖 std/serde/toml_edit：不碰文件写入、不联网、不依赖 Tauri。落盘与联网各由
//! meowo-app 完成，但都以本层探测出的 [`Installation`] 为输入，不再各自重推路径。
//!
//! 迁移状态：kimi 已走本层（试点）；claude/codex 仍走 meowo-app 内的旧路径，待逐个迁入 `plugins/`。

pub mod auth;
pub mod config;
pub mod id;
pub mod launch;
pub mod plugins;
pub mod registry;
pub mod variant;

pub use auth::{AuthScheme, CredentialSource, OAuthRefresh};
pub use config::{CommandSpec, ConfigFormat, EnsureOutcome, HookEvent, HookSpec, MissingConfig, RepairReason};
pub use id::AgentId;
pub use launch::{LaunchCandidate, LaunchSpec, Root};
pub use registry::{all, by_id, AgentPlugin};
pub use variant::{DataDirSpec, Installation, Variant};

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
