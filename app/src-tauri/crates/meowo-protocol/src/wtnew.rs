//! 「在新 worktree 里开会话」的 DTO（包 5，定案见关卡 B-20261110-p5-design）。
//!
//! 单独成文件而不是塞进 ipc.rs：push / PR 那条车道同时在改 ipc.rs，分开放合并时不打架。

use serde::{Deserialize, Serialize};

/// 新建 worktree 之前的预览：建议的用途名、分支名、目录和基点，以及有没有撞名。
/// 新建会话页勾上「在新 worktree 里开」后实时显示，让用户在点开始之前就知道会建在哪。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreePreviewDto {
    /// 选中目录所在仓库的根目录（选的是子目录时也是根）。
    pub repo_root: String,
    /// 主检出根目录：worktree 会话在看板和侧栏归到它下面。
    pub origin_root: String,
    pub repo_name: String,
    pub slug: String,
    pub branch: String,
    pub path: String,
    /// 基点的展示名：`origin/main`，没有远端主分支时是 `HEAD`。
    pub base: String,
    pub dir_exists: bool,
    pub branch_exists: bool,
    /// 分支已经在别的 worktree 里检出（git 不允许同一分支检出两次）。
    pub branch_checked_out: bool,
    /// 仓库里配置了 setup（meowo / cursor / conductor 任一）。
    pub has_setup: bool,
}

/// setup 脚本的进度。`state`：needs_trust | running | ok | failed | timeout | stopped | skipped。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSetupDto {
    pub task_id: String,
    pub path: String,
    pub state: String,
    pub exit_code: Option<i32>,
    /// 日志最后几十行。完整日志在 `~/.meowo/worktree-setup/<任务号>.log`，不写进 worktree。
    pub log_tail: String,
    pub log_path: Option<String>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub started_at: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub finished_at: Option<i64>,
    /// 没跑或跑不了的原因码：`none`（没配 setup）、`user`（用户跳过）、
    /// `worktree/config_version`、`worktree/config_invalid`、`worktree/setup_shell_unsupported`。
    pub reason: Option<String>,
    /// 要执行的脚本原文（信任确认框原样展示）。
    pub script: Option<String>,
    /// 配置文本的 hash，确认信任时原样回传：用户看到的和要跑的必须是同一份。
    pub config_hash: Option<String>,
    /// 配置来源：meowo | cursor | conductor。
    pub config_source: Option<String>,
    /// 用哪个 shell 跑：pwsh | powershell | bash | sh。
    pub runner: Option<String>,
}

/// 一次新建的结果。prune / add / 复制已经同步做完，setup 在后台跑（见 `setup`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateDto {
    pub task_id: String,
    pub path: String,
    pub branch: String,
    pub base: String,
    pub origin_root: String,
    /// 从原目录复制过来的被忽略文件数（`.env*` 等）。
    pub copied: u32,
    /// 超过总量上限没复制的文件数。
    pub copy_skipped: u32,
    pub setup: WorktreeSetupDto,
}
