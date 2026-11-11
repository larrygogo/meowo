//! 侧栏会话行的 PR / CI 状态点与 `+N −M`（UI 优化 #54，对照 Conductor / Warp 的侧栏 GitHub 状态）。
//!
//! 节流规矩与二期「更改」面板同一份（规格「gh 调用每仓库 ≥60s 一次，窗口隐藏 0 次」）：
//! - PR 读写的是 forge 的 [`Caches::prs`]，键同 [`pr_key`]（公共 .git + 仓库 + head 分支）。面板刚查过的，
//!   侧栏直接用；侧栏查过的，面板 60 秒内也不再查。同一仓库的多个会话只算一次。
//! - `online = false`（窗口隐藏 / 最小化）：只读缓存，**一次 gh 都不调**，连 `gh auth status` 也不碰。
//! - 没装 gh / 没登录：PR 一律为 None，只给改动数（产品事实：用户不一定装了 gh）。
//!
//! 改动数走本地 git（不联网），按目录缓存 [`DIFF_TTL`]：侧栏跟着 board-changed 每几百毫秒就可能刷新一次，
//! 几十个工作目录每轮各跑一遍 `git status` + `numstat` 撑不住。

use super::{lock, pr_key, repo_info, view_pr, Ctx, GhState, PR_TTL};
use meowo_protocol::ipc::{PullRequestDto, SidebarGitBadgeDto, SidebarPrBadgeDto};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 改动数的缓存时长。标题栏的分支胶囊按 5 秒轮询，侧栏只是扫一眼的摘要，慢一些无妨。
pub(crate) const DIFF_TTL: Duration = Duration::from_secs(15);

/// (增, 删, 文件数)。
pub(crate) type DiffCounts = (u32, u32, u32);

/// 一次请求最多算这么多个目录：侧栏一页 60 行，去重后通常十几个；防手机端塞一大串。
const MAX_CWDS: usize = 64;

#[derive(Default)]
pub(crate) struct DiffCache {
    entries: Mutex<HashMap<String, (Instant, Option<DiffCounts>)>>,
}

static DIFFS: std::sync::LazyLock<DiffCache> = std::sync::LazyLock::new(DiffCache::default);

fn to_badge(pr: &PullRequestDto) -> SidebarPrBadgeDto {
    SidebarPrBadgeDto {
        number: pr.number,
        state: pr.state.clone(),
        checks: pr.checks_summary.clone(),
        url: pr.url.clone(),
        title: pr.title.clone(),
    }
}

/// 一个目录当前分支的 PR。online=false 时只看缓存（不论新旧），绝不调 gh。
fn pr_badge(ctx: &Ctx, dir: &str, online: bool) -> Option<SidebarPrBadgeDto> {
    let info = repo_info(dir, ctx.env, &ctx.prefs)?;
    let key = pr_key(&info)?;
    let cached = lock(&ctx.caches.prs).get(&key).cloned();
    if let Some((at, pr)) = &cached {
        if !online || at.elapsed() < PR_TTL {
            return pr.as_ref().map(to_badge);
        }
    }
    if !online {
        return None;
    }
    // 登录状态自带 5 分钟缓存；没装、没登录就不查 PR（缓存里有旧的也不显示：登录失效时别挂着过期的 CI 结果）。
    if ctx.caches.gh_state(ctx.gh, false) != GhState::Ok {
        return None;
    }
    match view_pr(ctx.gh, ctx.caches, dir, info.gh.slug(), info.head_ref().as_deref()) {
        Ok(pr) => {
            lock(&ctx.caches.prs).insert(key, (Instant::now(), pr.clone()));
            pr.as_ref().map(to_badge)
        }
        // 断网、限流：沿用上一次的，状态点不会一闪而没。
        Err(_) => cached.and_then(|(_, pr)| pr).as_ref().map(to_badge),
    }
}

fn diff_counts(cache: &DiffCache, dir: &str, compute: &dyn Fn(&str) -> Option<DiffCounts>) -> Option<DiffCounts> {
    if let Some((at, counts)) = lock(&cache.entries).get(dir) {
        if at.elapsed() < DIFF_TTL {
            return *counts;
        }
    }
    let counts = compute(dir);
    lock(&cache.entries).insert(dir.to_string(), (Instant::now(), counts));
    counts
}

/// 真实的改动数：与标题栏分支胶囊同一口径（git.rs 的摘要；折叠的未跟踪目录按里面的文件数算）。
fn real_diff_counts(dir: &str) -> Option<DiffCounts> {
    let summary = crate::git::git_diff_summary_blocking(dir).ok()?;
    if !summary.is_repo {
        return None;
    }
    Some(summary.files.iter().fold((0, 0, 0), |(add, del, files), file| {
        (
            add + file.additions.unwrap_or(0),
            del + file.deletions.unwrap_or(0),
            files + file.untracked_files.unwrap_or(1),
        )
    }))
}

/// 批量算侧栏摘要。只回「有 PR 或有改动」的目录：干净且没有 PR 的会话行什么都不显示，回包也省了。
pub(crate) fn badges_with(
    ctx: &Ctx,
    diffs: &DiffCache,
    cwds: &[String],
    online: bool,
    compute: &dyn Fn(&str) -> Option<DiffCounts>,
) -> Vec<SidebarGitBadgeDto> {
    let mut seen = HashSet::new();
    cwds.iter()
        .filter(|cwd| !cwd.trim().is_empty() && seen.insert(cwd.as_str()))
        .take(MAX_CWDS)
        .filter_map(|cwd| {
            let dir = cwd.trim();
            if !std::path::Path::new(dir).is_dir() {
                return None;
            }
            let (additions, deletions, files) = diff_counts(diffs, dir, compute)?;
            let pr = pr_badge(ctx, dir, online);
            (files > 0 || pr.is_some()).then(|| SidebarGitBadgeDto { cwd: cwd.clone(), additions, deletions, files, pr })
        })
        .collect()
}

/// 调用方窗口看不见（隐藏到托盘、最小化）就不联网。读不到状态按看得见处理：宁可多查一次，不让状态点永远不来。
fn window_online(window: &tauri::WebviewWindow) -> bool {
    let visible = window.is_visible().unwrap_or(true);
    let minimized = window.is_minimized().unwrap_or(false);
    visible && !minimized
}

#[tauri::command]
pub(crate) async fn sidebar_git_badges(window: tauri::WebviewWindow, cwds: Vec<String>) -> Result<Vec<SidebarGitBadgeDto>, String> {
    let online = window_online(&window);
    sidebar_git_badges_for(cwds, online).await
}

/// 手机桥与桌面命令共用。手机页自己只在页面可见时轮询，桥这边按在线处理。
pub(crate) async fn sidebar_git_badges_for(cwds: Vec<String>, online: bool) -> Result<Vec<SidebarGitBadgeDto>, String> {
    tauri::async_runtime::spawn_blocking(move || badges_with(&Ctx::real(), &DIFFS, &cwds, online, &real_diff_counts))
        .await
        .map_err(|e| e.to_string())
}
