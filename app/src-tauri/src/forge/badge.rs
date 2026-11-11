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
//!
//! 进程数（bench_sidebar_badges_20_dirs，Windows 上一个 git 进程约 40ms）：以前每轮对每个仓库都现算一遍
//! repo_info（含默认分支，4～6 个进程），哪怕 PR 走缓存；改动数走完整摘要（rev-parse、rev-list 也算）。
//! 20 个目录冷态 125 个进程、串行 5.4 秒，15 秒内复查也要 95 个。现在：PR 去向按目录缓存、HEAD / 配置
//! 变了才重算；改动数只跑 status（有跟踪改动才加 numstat）；非仓库目录不起进程；目录之间限并发并行。
//! 同一基准（release）改后：冷态 438ms / 38 个进程，热态 1ms / 0 个，30 秒一轮的稳态 297ms / 24 个
//!（改前 4100ms / 125 个）；大仓库单目录冷态 372→155ms。

use super::{lock, pr_key, repo_head, view_pr, Ctx, GhState, PR_TTL};
use meowo_protocol::ipc::{PullRequestDto, SidebarGitBadgeDto, SidebarPrBadgeDto};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// 改动数的缓存时长。标题栏的分支胶囊按 5 秒轮询，侧栏只是扫一眼的摘要，慢一些无妨。
pub(crate) const DIFF_TTL: Duration = Duration::from_secs(15);

/// (增, 删, 文件数)。
pub(crate) type DiffCounts = (u32, u32, u32);

/// 一次请求最多算这么多个目录：侧栏一页 60 行，去重后通常十几个；防手机端塞一大串。
const MAX_CWDS: usize = 64;

/// 同时算几个目录。每个目录都是起 git 进程、等它读盘，串行时 20 个目录冷态要两秒多；并发再高
/// 收益变小，还会和用户正在跑的构建抢 CPU / 磁盘（bench_sidebar_badges_20_dirs：4 路冷态 438ms，
/// 8 路 405ms——瓶颈成了大仓库自己那串 status → ls-files → numstat）。
const DIR_WORKERS: usize = 4;

/// 本地 git 的按目录缓存：改动数，与 PR 去向（缓存键 + 查 PR 要用的仓库与 head）。
#[derive(Default)]
pub(crate) struct DiffCache {
    entries: Mutex<HashMap<String, (Instant, Option<DiffCounts>)>>,
    targets: Mutex<HashMap<String, TargetEntry>>,
}

/// 一个目录的 PR 去向：(缓存键, 仓库 owner/name, gh 认的 head)。没有 GitHub 远端、detached 时为 None。
type PrTarget = Option<(String, String, String)>;

struct TargetEntry {
    at: Instant,
    /// 判定依赖的文件（HEAD、仓库配置、worktree 配置、forge.json）与它们当时的修改时间：
    /// 切分支、改远端、在面板里换远端都会改其中之一，改了就重算，不等 TTL。
    watch: Vec<PathBuf>,
    stamps: Vec<Option<SystemTime>>,
    target: PrTarget,
}

#[cfg(test)]
impl DiffCache {
    /// 基准用：只让改动数过期（模拟前端 30 秒一轮时 15 秒的改动缓存已失效），PR 去向留着。
    pub(crate) fn expire_counts(&self) {
        lock(&self.entries).clear();
    }
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

fn stamps(paths: &[PathBuf]) -> Vec<Option<SystemTime>> {
    paths.iter().map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok()).collect()
}

/// 目录的 PR 去向，按目录缓存。以前每轮都现算 repo_info（连默认分支一起，每个仓库 4～6 个 git 进程），
/// PR 走缓存时也照算，只为拼出缓存键；键里其实没有默认分支。现在依赖文件没变就复用，最多 [`PR_TTL`]
/// 重算一次（全局 git 配置、远端默认分支这类不在 watch 里的改动，最晚 60 秒跟上，与 PR 缓存同一节奏）。
fn pr_target(cache: &DiffCache, ctx: &Ctx, dir: &str) -> PrTarget {
    if let Some(entry) = lock(&cache.targets).get(dir) {
        if entry.at.elapsed() < PR_TTL && stamps(&entry.watch) == entry.stamps {
            return entry.target.clone();
        }
    }
    let (info, common_dir, git_dir) = repo_head(dir, ctx.env, &ctx.prefs, true)?;
    let target = pr_key(&info).and_then(|key| Some((key, info.gh.slug()?.to_string(), info.head_ref()?)));
    let watch = vec![git_dir.join("HEAD"), common_dir.join("config"), git_dir.join("config.worktree"), ctx.prefs.clone()];
    let entry = TargetEntry { at: Instant::now(), stamps: stamps(&watch), watch, target: target.clone() };
    lock(&cache.targets).insert(dir.to_string(), entry);
    target
}

/// 一个目录当前分支的 PR。online=false 时只看缓存（不论新旧），绝不调 gh。
fn pr_badge(ctx: &Ctx, target: PrTarget, dir: &str, online: bool) -> Option<SidebarPrBadgeDto> {
    let (key, slug, head) = target?;
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
    match view_pr(ctx.gh, ctx.caches, dir, Some(&slug), Some(&head)) {
        Ok(pr) => {
            lock(&ctx.caches.prs).insert(key, (Instant::now(), pr.clone()));
            pr.as_ref().map(to_badge)
        }
        // 断网、限流：沿用上一次的，状态点不会一闪而没。
        Err(_) => cached.and_then(|(_, pr)| pr).as_ref().map(to_badge),
    }
}

fn diff_counts(cache: &DiffCache, dir: &str, compute: &Compute<'_>) -> Option<DiffCounts> {
    if let Some((at, counts)) = lock(&cache.entries).get(dir) {
        if at.elapsed() < DIFF_TTL {
            return *counts;
        }
    }
    let counts = compute(dir);
    lock(&cache.entries).insert(dir.to_string(), (Instant::now(), counts));
    counts
}

/// 改动数的算法（测试换成假的）。要跨线程共用，见 [`DIR_WORKERS`]。
pub(crate) type Compute<'a> = dyn Fn(&str) -> Option<DiffCounts> + Sync + 'a;

/// 真实的改动数：与标题栏分支胶囊同一口径（同一条 status、同一套折叠与行数；未跟踪文件计入，
/// 折叠的未跟踪目录按里面的文件数算），只是不算分支与上游。
pub(crate) fn real_diff_counts(dir: &str) -> Option<DiffCounts> {
    crate::git::sidebar_diff_counts_blocking(dir)
}

/// 按原顺序对每项求值，最多 workers 个线程同时跑。
fn par_map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if items.len() <= 1 || workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.min(items.len()) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(item) = items.get(index) else { break };
                *lock(&slots[index]) = Some(f(item));
            });
        }
    });
    // scope 结束时所有线程都已跑完（有 panic 会在上面传出来），每格都有值。
    slots.into_iter().filter_map(|slot| slot.into_inner().unwrap_or_else(|e| e.into_inner())).collect()
}

/// 批量算侧栏摘要。只回「有 PR 或有改动」的目录：干净且没有 PR 的会话行什么都不显示，回包也省了。
pub(crate) fn badges_with(
    ctx: &Ctx,
    diffs: &DiffCache,
    cwds: &[String],
    online: bool,
    compute: &Compute<'_>,
) -> Vec<SidebarGitBadgeDto> {
    let mut seen = HashSet::new();
    let list: Vec<&String> = cwds.iter().filter(|cwd| !cwd.trim().is_empty() && seen.insert(cwd.as_str())).take(MAX_CWDS).collect();
    // 本地 git（改动数、PR 去向）各目录互不相干，并行算；gh 留在下面串行：登录状态只问一次，
    // 联网查 PR 本来就每仓库 60 秒才一次。
    let local = par_map(&list, DIR_WORKERS, |cwd| {
        let dir = cwd.trim();
        if !std::path::Path::new(dir).is_dir() {
            return None;
        }
        let counts = diff_counts(diffs, dir, compute)?;
        Some((counts, pr_target(diffs, ctx, dir)))
    });
    list.into_iter()
        .zip(local)
        .filter_map(|(cwd, local)| {
            let ((additions, deletions, files), target) = local?;
            let pr = pr_badge(ctx, target, cwd.trim(), online);
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

#[cfg(test)]
mod par_tests {
    use super::par_map;

    #[test]
    fn par_map_保持顺序且每项只算一次() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let items: Vec<usize> = (0..37).collect();
        let out = par_map(&items, 4, |n| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            n * 2
        });
        assert_eq!(out, items.iter().map(|n| n * 2).collect::<Vec<_>>());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 37);
        assert_eq!(par_map(&[5usize], 4, |n| n + 1), vec![6]);
        assert!(par_map(&[] as &[usize], 4, |n| n + 1).is_empty());
    }
}
