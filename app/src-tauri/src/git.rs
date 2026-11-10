//! 「查看改动」弹层的 git 数据命令：工作树相对 HEAD 的变更清单与单文件 diff。
//! 不经 shell 直接 spawn git（路径作为独立 argv，无注入面）；子进程可能被杀软拖慢，
//! 统一走 spawn_blocking，不进主线程（同类教训见 terminal.rs open_project_dir 的注释）。

use meowo_protocol::ipc::{GitChangedFileDto, GitDiffSummaryDto, GitFileDiffDto, PullRequestDto, RepoChangesDto};
use std::path::Path;
use std::process::Command;

/// diff 文本与未跟踪文件读取的统一上限（超出即截断并置 truncated）。
const MAX_DIFF_BYTES: usize = 200 * 1024;
/// 未跟踪文件的二进制嗅探窗口：前 8KB 出现 NUL 即按二进制处理。
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

/// 工作树改动摘要：git 可用性、是否仓库、分支名与变更文件列表。
/// 按钮可见性由前端按 `is_repo` 决定，故此命令对「不是仓库」返回正常 DTO 而非报错。
#[tauri::command]
pub(crate) async fn git_diff_summary(cwd: String) -> Result<GitDiffSummaryDto, String> {
    tauri::async_runtime::spawn_blocking(move || git_diff_summary_blocking(&cwd))
        .await
        .map_err(|e| e.to_string())?
}

pub(crate) fn git_diff_summary_blocking(cwd: &str) -> Result<GitDiffSummaryDto, String> {
    summary_blocking(cwd, true)
}

/// counts = 补增删行数（`diff --numstat`，未跟踪文件还要逐个读盘数行）。仓库清单里没选中的仓库
/// 不数，只要分支、↑↓、文件清单（见 RepoChangesDto::detailed）。
fn summary_blocking(cwd: &str, counts: bool) -> Result<GitDiffSummaryDto, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    let unavailable = || GitDiffSummaryDto {
        git_available: false,
        is_repo: false,
        branch: None,
        ahead: None,
        behind: None,
        files: vec![],
        upstream: None,
        unpushed: None,
        push_target: None,
    };
    // spawn 失败（git 未安装/被拦）不算错误——前端只是不显示按钮。
    // 顺带要公共 .git 目录与本 worktree 的 .git 目录：推送去向靠直接读它们的 config 判定
    // （git_target::resolve），不再多起 `git remote` / `git config` 进程。
    let inside = match git_output(dir, &["rev-parse", "--is-inside-work-tree", "--git-common-dir", "--git-dir"]) {
        Ok(output) => output,
        Err(_) => return Ok(unavailable()),
    };
    let inside_text = stdout_text(&inside.stdout);
    let mut inside_lines = inside_text.lines();
    if !inside.status.success() || inside_lines.next().map(str::trim) != Some("true") {
        return Ok(GitDiffSummaryDto {
            is_repo: false,
            ..unavailable()
        });
    }
    // --no-renames：-z 格式下重命名条目会带两个路径，禁掉后每个条目恒为「XY <path>\0」。
    // -unormal：整个目录都未跟踪时只报一条「?? dir/」，git 不往里扫。以前用 -uall 把它展开到
    // 每个文件，没进 .gitignore 的构建目录有一万五千多个文件时整份发给前端，面板卡死（用户实测）。
    // 目录条目由 fold_untracked_dirs 处理：文件少的照旧展开成逐个文件（目录条目直接当文件渲染会
    // 文件名为空、点开按文件读目录报错，-uall 当初就是为这个加的），多的折成一条带文件数的。
    // --branch：第一条是「## 分支...上游 [ahead 1, behind 2]」，分支名、上游、领先落后一次读出。
    // 以前另跑 `rev-parse --abbrev-ref HEAD` 和 `rev-list --left-right --count`，「更改」面板要显示
    // 推送按钮和未推数，又不能让冷耗时变慢（规格预算 ≤197ms），合进这一条反而少起两个进程。
    // porcelain 格式下这一行不随语言设置翻译（wt-status.c 的 wt_porcelain_print 置 no_gettext）。
    // 二期实测（release，bench_summary_with_15000_untracked_files，同轮对照）：改前 -uall 182ms，折叠后冷态
    // 从 199ms 降到 146ms（Windows 上每个 git 进程固定约 30ms）。
    let status = git_output(dir, &["status", "--porcelain", "-z", "--no-renames", "-unormal", "--branch"])
        .map_err(|e| e.to_string())?;
    if !status.status.success() {
        return Err("git status 执行失败".into());
    }
    let (header, body) = split_branch_header(&status.stdout);
    let mut files = fold_untracked_dirs(dir, parse_status_entries(body))?;
    if counts {
        fill_line_counts(dir, &mut files);
    }
    let mut head = header.map(|h| parse_branch_header(&h)).unwrap_or_default();
    // 推送去向与 forge::push_with 同一个判定（git_target::resolve，做空第二轮）：远端名原样从
    // branch.<b>.remote 取，带斜杠（team/fork）也不拆；pushRemote / pushDefault 优先；上游是远端默认分支
    // 而本地不是它时按没上游处理（第一轮做空：照 origin/main 推，提交进了远端 main）。
    let path_line = |line: Option<&str>| line.map(str::trim).filter(|l| !l.is_empty()).map(|l| Path::new(dir).join(l));
    let common = path_line(inside_lines.next());
    let git_dir = path_line(inside_lines.next());
    let target = match (&common, &head.branch) {
        (Some(common), Some(branch)) => {
            let config = crate::git_target::GitConfig::load(common, git_dir.as_deref(), &crate::git_target::process_env);
            crate::git_target::resolve(&config, common, branch)
        }
        _ => None,
    };
    // 上游以判定认的为准；status 说上游已删（gone）时照旧按没上游。
    let upstream = target
        .as_ref()
        .and_then(|t| t.upstream.as_ref())
        .filter(|_| head.upstream.is_some())
        .map(|u| u.label());
    if upstream.is_none() {
        (head.ahead, head.behind) = (None, None);
    }
    // status.aheadBehind=false 时只写「[different]」不给数：退回单独数一次，和以前一样。
    if upstream.is_some() && head.ahead.is_none() {
        if let Some((ahead, behind)) = upstream_divergence(dir) {
            (head.ahead, head.behind) = (Some(ahead), Some(behind));
        }
    }
    let unpushed = match &target {
        // 推的就是上游：↑ 就是未推数。
        Some(t) if t.is_upstream() && upstream.is_some() => head.ahead,
        // 推去别处（没上游、上游已删、pushDefault 指向 fork）：数推送远端上还没有的提交，与 push_with 报的
        // 「推了几个」同一口径。只给选中的仓库算（多一次进程）。
        Some(_) if !counts => None,
        Some(t) => unpushed_to_remote(dir, &t.remote),
        None => None,
    };
    let push_target = target.as_ref().map(|t| t.label());
    Ok(GitDiffSummaryDto {
        git_available: true,
        is_repo: true,
        branch: head.branch,
        ahead: head.ahead,
        behind: head.behind,
        files,
        upstream,
        unpushed,
        push_target,
    })
}

/// `git status --branch` 第一条的内容。
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct BranchHeader {
    /// detached（`HEAD (no branch)`）和还没有提交的分支为 None（同以前 `rev-parse --abbrev-ref HEAD` 失败）。
    pub(crate) branch: Option<String>,
    pub(crate) upstream: Option<String>,
    pub(crate) ahead: Option<u32>,
    pub(crate) behind: Option<u32>,
}

/// 把 `-z` 输出里开头的 `## ...` 条目切出来，其余照旧交给 parse_status_entries。
fn split_branch_header(bytes: &[u8]) -> (Option<String>, &[u8]) {
    if !bytes.starts_with(b"## ") {
        return (None, bytes);
    }
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let header = String::from_utf8_lossy(&bytes[3..end]).into_owned();
    (Some(header), bytes.get(end + 1..).unwrap_or_default())
}

/// 解析「main...origin/main [ahead 1, behind 2]」这类分支行（wt-status.c wt_shortstatus_print_tracking
/// 的几种写法：无上游、同步、ahead/behind、gone、detached、还没有提交）。上游已删（gone）当作没上游：
/// 和以前 `rev-list ...@{upstream}` 失败一样不显示 ↑↓。
pub(crate) fn parse_branch_header(line: &str) -> BranchHeader {
    let line = line.trim_end();
    if line.starts_with("HEAD (no branch)") {
        return BranchHeader::default();
    }
    let initial = line.starts_with("No commits yet on ") || line.starts_with("Initial commit on ");
    let (names, tracking) = match line.find(" [") {
        Some(at) => (&line[..at], Some(line[at + 2..].trim_end_matches(']'))),
        None => (line, None),
    };
    let (branch, upstream) = match names.split_once("...") {
        Some((branch, upstream)) => (branch, Some(upstream.to_string())),
        None => (names, None),
    };
    if initial {
        return BranchHeader::default();
    }
    let mut head = BranchHeader { branch: Some(branch.to_string()).filter(|b| !b.is_empty()), ..Default::default() };
    if tracking == Some("gone") {
        return head;
    }
    head.upstream = upstream;
    if head.upstream.is_some() {
        let (mut ahead, mut behind) = (Some(0), Some(0));
        for part in tracking.unwrap_or_default().split(", ") {
            if let Some(n) = part.strip_prefix("ahead ") {
                ahead = n.trim().parse().ok();
            } else if let Some(n) = part.strip_prefix("behind ") {
                behind = n.trim().parse().ok();
            } else if !part.is_empty() {
                // 「different」：aheadBehind 关掉了，数字未知。
                (ahead, behind) = (None, None);
            }
        }
        (head.ahead, head.behind) = (ahead, behind);
    }
    head
}

/// 有几个提交还不在这个远端上（`rev-list --count HEAD --not --remotes=<远端>`）。
fn unpushed_to_remote(dir: &str, remote: &str) -> Option<u32> {
    let output = git_output(dir, &["rev-list", "--count", "HEAD", "--not", &format!("--remotes={remote}")])
        .ok()
        .filter(|o| o.status.success())?;
    stdout_text(&output.stdout).trim().parse().ok()
}

/// 相对上游的 (领先, 落后) 提交数：`rev-list --left-right --count HEAD...@{upstream}` 输出
/// 「左侧独有 右侧独有」。没设上游、detached HEAD 都会失败，返回 None（前端不显示）。
fn upstream_divergence(dir: &str) -> Option<(u32, u32)> {
    let output = git_output(dir, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .ok()
        .filter(|output| output.status.success())?;
    let text = stdout_text(&output.stdout);
    let mut counts = text.split_whitespace().map(str::parse::<u32>);
    Some((counts.next()?.ok()?, counts.next()?.ok()?))
}

// ═══ 未跟踪目录的折叠 ═══

/// 未跟踪目录里的文件多于这个数就折成一条（DTO 的 untracked_files）。少的照旧展开：新建一个放了
/// 两三个文件的目录，用户要的是直接看到那几个文件，不是再点一下。
const UNTRACKED_FOLD_MIN: usize = 50;
/// 折叠目录文件数的缓存时长。数一次要让 git 把整个目录扫一遍（15000 个文件和 -uall 一样慢），
/// 5 秒轮询每轮都数就又回到了老样子；折叠行上的数字晚半分钟更新无妨，点开时的清单是现拉的。
/// 只缓存折叠的（多于 UNTRACKED_FOLD_MIN）：展开的要逐个文件准确，而且小目录现数也便宜。
const FOLD_COUNT_TTL: std::time::Duration = std::time::Duration::from_secs(30);
static FOLD_CACHE: TtlCache<usize> = std::sync::Mutex::new(None);

/// `git status --porcelain -z -unormal` 的一项：普通条目，或整个未跟踪的目录（`?? dir/`，带尾斜杠）。
enum StatusEntry {
    File(GitChangedFileDto),
    UntrackedDir(String),
}

fn untracked_dto(path: String, untracked_files: Option<u32>) -> GitChangedFileDto {
    GitChangedFileDto {
        path,
        status: "U".into(),
        additions: None,
        deletions: None,
        index_status: None,
        worktree_status: Some("U".into()),
        untracked_files,
    }
}

fn fold_cache_key(dir: &str, path: &str) -> String {
    format!("{}\u{0}{path}", norm_slashes(dir).to_lowercase())
}

/// 把目录条目换成「逐个文件」或「一条折叠目录」，其余条目原样、顺序不变（porcelain 按路径排好了）。
/// 只含嵌套仓库、一个文件都没有的目录不出现：-uall 时代嵌套仓库（`.claude/worktrees/x/`）就不列，
/// 列出来「暂存」会把它当 gitlink 加进去（commit_all_skips_untracked_nested_repo）。
fn fold_untracked_dirs(dir: &str, entries: Vec<StatusEntry>) -> Result<Vec<GitChangedFileDto>, String> {
    let dirs: Vec<&str> = entries
        .iter()
        .filter_map(|entry| match entry {
            StatusEntry::UntrackedDir(path) => Some(path.as_str()),
            StatusEntry::File(_) => None,
        })
        .collect();
    let mut cached: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    if let Ok(cache) = FOLD_CACHE.lock() {
        if let Some(map) = cache.as_ref() {
            for path in &dirs {
                if let Some((at, count)) = map.get(&fold_cache_key(dir, path)) {
                    if at.elapsed() < FOLD_COUNT_TTL && *count > UNTRACKED_FOLD_MIN {
                        cached.insert((*path).to_string(), *count);
                    }
                }
            }
        }
    }
    let to_list: Vec<String> = dirs.iter().filter(|path| !cached.contains_key(**path)).map(|path| path.to_string()).collect();
    let mut buckets = bucket_by_dir(&to_list, &untracked_listing(dir, &to_list)?);
    let mut out = Vec::with_capacity(entries.len());
    let mut fresh: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        match entry {
            StatusEntry::File(file) => out.push(file),
            StatusEntry::UntrackedDir(path) => {
                if let Some(&count) = cached.get(&path) {
                    out.push(untracked_dto(path, Some(u32::try_from(count).unwrap_or(u32::MAX))));
                    continue;
                }
                let files = buckets.remove(&path).map(|bucket| bucket.files).unwrap_or_default();
                if files.len() > UNTRACKED_FOLD_MIN {
                    fresh.push((path.clone(), files.len()));
                    out.push(untracked_dto(path, Some(u32::try_from(files.len()).unwrap_or(u32::MAX))));
                } else {
                    out.extend(files.into_iter().map(|file| untracked_dto(file, None)));
                }
            }
        }
    }
    if !fresh.is_empty() {
        if let Ok(mut cache) = FOLD_CACHE.lock() {
            let map = cache.get_or_insert_with(Default::default);
            let now = std::time::Instant::now();
            for (path, count) in fresh {
                map.insert(fold_cache_key(dir, &path), (now, count));
            }
        }
    }
    Ok(out)
}

/// 一个未跟踪目录里 ls-files 列出的东西：文件，和有没有嵌套仓库（ls-files 把它报成带尾斜杠的一项）。
#[derive(Default)]
struct DirBucket {
    files: Vec<String>,
    nested_repo: bool,
}

/// ls-files 的结果按所属目录条目分桶。status 给出的目录条目互不嵌套，按路径前缀找第一个命中的即可。
fn bucket_by_dir(dirs: &[String], listing: &[String]) -> std::collections::HashMap<String, DirBucket> {
    let mut buckets: std::collections::HashMap<String, DirBucket> =
        dirs.iter().map(|path| (path.clone(), DirBucket::default())).collect();
    for item in listing {
        let owner = item.match_indices('/').map(|(at, _)| &item[..=at]).find(|prefix| buckets.contains_key(*prefix));
        let Some(owner) = owner.map(str::to_string) else { continue };
        let bucket = buckets.get_mut(&owner).expect("owner 来自 buckets 的键");
        if item.ends_with('/') {
            bucket.nested_repo = true;
        } else {
            bucket.files.push(item.clone());
        }
    }
    buckets
}

/// 这些路径（目录或文件，相对仓库根）下未跟踪、未被忽略的东西：`git ls-files --others --exclude-standard`。
/// 嵌套仓库 git 不往里走，报成带尾斜杠的一项。`--full-name`：从子目录跑时也给相对仓库根的路径，
/// 和 porcelain 同一口径。路径多时分批（同 run_pathspec_batches）。
fn untracked_listing(dir: &str, paths: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let specs: Vec<String> = paths.iter().map(|p| format!(":(top,literal){p}")).collect();
    for batch in pathspec_batches(&specs) {
        let mut argv: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--full-name", "-z", "--"];
        argv.extend(batch.iter().map(String::as_str));
        let output = git_output(dir, &argv).map_err(|e| format!("git/failed: {e}"))?;
        if !output.status.success() {
            return Err(classify_git_failure(&output));
        }
        out.extend(
            output
                .stdout
                .split(|b| *b == 0)
                .filter(|item| !item.is_empty())
                .map(|item| String::from_utf8_lossy(item).into_owned()),
        );
    }
    Ok(out)
}

/// 「更改」面板点开一个折叠目录：列出其中的未跟踪文件（用户点开时才跑一次，不进 5 秒轮询）。
/// path 必须逐字是此刻 status 里的一个未跟踪目录条目（手机端传什么都进得来，同 listed_changes）。
#[tauri::command]
pub(crate) async fn git_untracked_dir_files(cwd: String, path: String) -> Result<Vec<GitChangedFileDto>, String> {
    tauri::async_runtime::spawn_blocking(move || untracked_dir_files_blocking(&cwd, &path))
        .await
        .map_err(|e| e.to_string())?
}

fn untracked_dir_files_blocking(cwd: &str, path: &str) -> Result<Vec<GitChangedFileDto>, String> {
    let dir = existing_dir(cwd)?;
    require_untracked_dir(dir, path)?;
    let listing = untracked_listing(dir, &[path.to_string()])?;
    Ok(listing
        .into_iter()
        .filter(|item| !item.ends_with('/') && item.starts_with(path))
        .map(|item| untracked_dto(item, None))
        .collect())
}

/// path 是此刻 status 里的一个未跟踪目录条目（带尾斜杠），否则 git/not_in_changes。
fn require_untracked_dir(dir: &str, path: &str) -> Result<(), String> {
    if unsafe_rel_path(path) || !path.ends_with('/') {
        return Err("git/not_in_changes".into());
    }
    let listed = status_entries(dir)?
        .iter()
        .any(|entry| matches!(entry, StatusEntry::UntrackedDir(d) if d == path));
    if listed { Ok(()) } else { Err("git/not_in_changes".into()) }
}

fn status_entries(dir: &str) -> Result<Vec<StatusEntry>, String> {
    let status = git_output(dir, &["status", "--porcelain", "-z", "--no-renames", "-unormal"])
        .map_err(|e| format!("git/failed: {e}"))?;
    if !status.status.success() {
        return Err(classify_git_failure(&status));
    }
    Ok(parse_status_entries(&status.stdout))
}

/// 折叠目录的「加入 .gitignore」：把 `/<目录>/` 追加到仓库根的 .gitignore（没有就新建）。
/// 已有同一行（或不带尾斜杠的 `/<目录>`）不重复加。手机端不放行（remote.rs 拒绝名单）：
/// 改的是仓库里的配置文件，不是「更改」本身。
#[tauri::command]
pub(crate) async fn git_ignore_dir(cwd: String, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || ignore_dir_blocking(&cwd, &path))
        .await
        .map_err(|e| e.to_string())?
}

fn ignore_dir_blocking(cwd: &str, path: &str) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    require_untracked_dir(dir, path)?;
    let root = repo_root(dir).ok_or_else(|| "git/failed: no repo root".to_string())?;
    let line = format!("/{}", escape_gitignore(path));
    let file = Path::new(&root).join(".gitignore");
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        // 读不出（不是 UTF-8 之类）就不碰：重写一份会把用户原来的内容弄坏。
        Err(e) => return Err(format!("git/failed: {e}")),
    };
    let bare = line.trim_end_matches('/');
    if text.lines().map(str::trim_end).any(|existing| existing == line || existing == bare) {
        return Ok(());
    }
    // 换行跟着原文件走：原来是 CRLF 就 CRLF；原来末尾没有换行，追加后末尾也不加。
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let body = if text.is_empty() {
        format!("{line}{eol}")
    } else if text.ends_with('\n') {
        format!("{text}{line}{eol}")
    } else {
        format!("{text}{eol}{line}")
    };
    let result = meowo_agent::fsutil::write_atomic(&file, &body).map_err(|e| format!("git/failed: {e}"));
    invalidate_caches();
    result
}

/// .gitignore 里有特殊含义的字符转义成字面量：`*?[` 是通配符，`\` 是转义符本身。行首的 `#` `!`
/// 不用管（行首恒是 `/`），行尾空格也不用管（行尾恒是 `/`）。
fn escape_gitignore(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        if matches!(ch, '\\' | '*' | '?' | '[') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 未跟踪文件最多数这么多个：-uall 下没进 .gitignore 的产物目录能有上千个文件，
/// 每个都读一遍盘会把摘要拖慢（摘要在面板每次可见时重拉）。超出的不显示行数。
const MAX_UNTRACKED_COUNTED: usize = 200;

/// 给变更清单补上增删行数（文件列表上的 `+12 −3`，对照 Codex/Claude 桌面版的改动面板）。
/// 数不出来就留 None：行数只是辅助信息，任何一步失败都不能让摘要报错。
fn fill_line_counts(dir: &str, files: &mut [GitChangedFileDto]) {
    // 仓库还没有任何提交时 HEAD 不存在，这条会失败，跟踪文件就都不显示行数。
    if let Ok(output) = git_output(dir, &["diff", "HEAD", "--numstat", "-z", "--no-renames"]) {
        if output.status.success() {
            let counts = parse_numstat_z(&output.stdout);
            for file in files.iter_mut() {
                if let Some(&(added, deleted)) = counts.get(file.path.as_str()) {
                    file.additions = Some(added);
                    file.deletions = Some(deleted);
                }
            }
        }
    }
    // 折叠的目录条目不是文件，不数（也不占 MAX_UNTRACKED_COUNTED 的名额）。
    for file in files.iter_mut().filter(|file| file.status == "U" && file.untracked_files.is_none()).take(MAX_UNTRACKED_COUNTED) {
        if let Some(lines) = untracked_line_count(dir, &file.path) {
            file.additions = Some(lines);
            file.deletions = Some(0);
        }
    }
}

/// `git diff --numstat -z --no-renames`：每条是 `<增>\t<删>\t<路径>\0`，二进制文件的增删是 `-`，跳过。
fn parse_numstat_z(bytes: &[u8]) -> std::collections::HashMap<String, (u32, u32)> {
    let mut counts = std::collections::HashMap::new();
    for entry in bytes.split(|b| *b == 0) {
        let text = String::from_utf8_lossy(entry);
        let mut parts = text.splitn(3, '\t');
        let (Some(added), Some(deleted), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if let (Ok(added), Ok(deleted)) = (added.parse(), deleted.parse()) {
            counts.insert(path.to_string(), (added, deleted));
        }
    }
    counts
}

/// 未跟踪文件的行数：超过 diff 上限或像二进制就不数（那种文件 diff 里也看不全）。
fn untracked_line_count(dir: &str, path: &str) -> Option<u32> {
    use std::io::Read;
    let full_path = crate::fsutil::resolve_inside(dir, path).ok()?;
    let mut bytes = Vec::new();
    std::fs::File::open(full_path)
        .ok()?
        .take((MAX_DIFF_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_DIFF_BYTES || bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return None;
    }
    Some(count_lines(&bytes))
}

/// 与 `str::lines` 同一算法：末尾没有换行的最后一行也算一行。
fn count_lines(bytes: &[u8]) -> u32 {
    let newlines = bytes.iter().filter(|b| **b == b'\n').count();
    let tail = usize::from(bytes.last().is_some_and(|b| *b != b'\n'));
    u32::try_from(newlines + tail).unwrap_or(u32::MAX)
}

// ═══ 多仓库「更改」═══

/// 工作目录下一层最多看这么多个子目录（找有改动的仓库，见 changes_overview_blocking）。
const MAX_CHILD_SCAN: usize = 200;
/// 按目录缓存、带写入时刻的小缓存（过期由读的一方按各自的 TTL 判断）。
type TtlCache<T> = std::sync::Mutex<Option<std::collections::HashMap<String, (std::time::Instant, T)>>>;
/// touched 里最多为这么多个不同目录查所属仓库（见 changes_overview_blocking）。
const MAX_ROOT_LOOKUPS: usize = 24;
/// 「下一层有改动的仓库」粗筛结果的缓存时长。
const CHILD_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);
/// 默认分支解析结果的缓存时长（远程分支很少变，不必每 5 秒重认一遍）。
const BASE_REF_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// 「更改」面板的仓库清单。`touched` 是会话编辑 / 写入过的文件（绝对路径），按各自所在仓库
/// 归组；工作目录所在仓库排第一。会话还没动过文件、工作目录又不是仓库时，列工作目录下一层
/// 当前有改动的仓库（用户要求，2026-11-09：只列会话动过的，不混入别的会话或手动的改动）。
/// `commands` 是会话最近跑过的命令文本：agent 常用命令 cd 进 worktree 干活（编辑工具没碰过那边的
/// 文件，touched 认不出），命令里出现过路径的相关仓库也上榜（用户要求 2026-11-09，见 command_mentioned_roots）。
///
/// `detail`：只给哪个仓库算完整内容（增删行数、「本分支已提交」）。空串 = 默认那个（工作目录所在仓库，
/// 没有就第一个）；不带 = 全部都算（旧前端）。仓库多时每 5 秒一轮全算太重（用户要求 2026-11-09：
/// 先显示目录，具体内容点击后异步）。
#[tauri::command]
pub(crate) async fn git_changes_overview(
    cwd: String,
    touched: Vec<String>,
    commands: Option<Vec<String>>,
    detail: Option<String>,
) -> Result<Vec<RepoChangesDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        changes_overview_detail(&cwd, &touched, commands.as_deref().unwrap_or_default(), detail.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 全部都算完整内容（测试用；命令与桥都走 changes_overview_detail）。
#[cfg(test)]
pub(crate) fn changes_overview_blocking(
    cwd: &str,
    touched: &[String],
    commands: &[String],
) -> Result<Vec<RepoChangesDto>, String> {
    changes_overview_detail(cwd, touched, commands, None)
}

/// 见 git_changes_overview 的 `detail`。
pub(crate) fn changes_overview_detail(
    cwd: &str,
    touched: &[String],
    commands: &[String],
    detail: Option<&str>,
) -> Result<Vec<RepoChangesDto>, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    let cwd_root = repo_root(dir);
    let mut roots: Vec<String> = cwd_root.iter().cloned().collect();
    // 5 秒轮询里每个文件都跑一次 rev-parse 会很重（做空实测 100 个文件 3 秒）：同一目录只查一次，
    // 总共最多查 MAX_ROOT_LOOKUPS 个目录。不能按「落在已认出的仓库里」跳过：Claude Code 默认把
    // worktree 建在仓库里面（.claude/worktrees/<名字>），子模块、嵌套仓库同理，跳过就认不出（做空复现）。
    let mut looked: Vec<String> = Vec::new();
    for path in touched {
        if looked.len() >= MAX_ROOT_LOOKUPS {
            break;
        }
        let Some(start) = existing_dir_of(path) else { continue };
        if looked.iter().any(|seen| same_path(seen, &start)) {
            continue;
        }
        looked.push(start.clone());
        if let Some(root) = repo_root(&start) {
            if !roots.iter().any(|known| same_path(known, &root)) {
                roots.push(root);
            }
        }
    }
    // 候选只取相关仓库（本仓库的 worktree + 下一层仓库）：命令里提到的任意路径都上榜的话，
    // `cat C:/别的项目/x` 也会拉进一个无关仓库；别的会话建的 worktree 没在本会话命令里出现，不上榜。
    if !commands.is_empty() {
        let candidates: Vec<String> = related_roots(dir)
            .into_iter()
            .filter(|root| !roots.iter().any(|known| same_path(known, root)))
            .collect();
        // 仓库个数不设上限（用户 2026-11-09：主目录下有改动的仓库常常多于 8 个，截掉的就看不到）。
        roots.extend(command_mentioned_roots(dir, &candidates, commands));
    }
    let mut list: Vec<(String, bool)> = roots
        .iter()
        .map(|root| (root.clone(), cwd_root.as_deref().is_some_and(|c| same_path(c, root))))
        .collect();
    let fallback = list.is_empty() && cwd_root.is_none();
    if fallback {
        list = changed_children(dir).into_iter().map(|root| (root, false)).collect();
    }
    // 要算完整内容的那一个：点名的在清单里就用它，否则工作目录所在仓库，再否则第一个（与前端 pickRepo 一致）。
    let full_at = detail.map(|want| {
        list.iter()
            .position(|(root, _)| !want.is_empty() && same_path(root, want))
            .or_else(|| list.iter().position(|(_, is_cwd)| *is_cwd))
            .unwrap_or(0)
    });
    let out: Vec<RepoChangesDto> = list
        .iter()
        .enumerate()
        .filter_map(|(at, (root, is_cwd))| {
            if full_at.is_none_or(|full| full == at) {
                repo_changes(root, *is_cwd)
            } else {
                repo_changes_light(root, *is_cwd)
            }
        })
        // 兜底列的是「此刻有改动的」：粗筛缓存 30 秒，期间改干净了的不列。
        .filter(|repo| !fallback || !repo.summary.files.is_empty())
        .collect();
    Ok(out)
}

/// 工作目录下一层里此刻有未提交改动的仓库。每个子仓库只跑一条 `git status --porcelain` 粗筛，
/// 结果按目录缓存 CHILD_CACHE_TTL：codex / gemini 会话认不出改过的文件（touched 恒空），会一直
/// 走这条路，workspace 下二十几个仓库挨个跑完整的 repo_changes 要好几秒（做空实测 6.7 秒，超过
/// 5 秒的轮询间隔）。
/// changed_children 的缓存。放在外面是为了让写操作（暂存 / 提交 / 放弃）之后能清掉，见 invalidate_caches。
static CHILD_CACHE: TtlCache<Vec<String>> = std::sync::Mutex::new(None);

fn changed_children(dir: &str) -> Vec<String> {
    let key = norm_slashes(dir).to_lowercase();
    if let Ok(cache) = CHILD_CACHE.lock() {
        if let Some((at, hit)) = cache.as_ref().and_then(|map| map.get(&key)) {
            if at.elapsed() < CHILD_CACHE_TTL {
                return hit.clone();
            }
        }
    }
    let changed: Vec<String> = child_repos(dir)
        .into_iter()
        .filter(|root| {
            // 只看有没有输出：-unormal 下整个未跟踪目录一条，不用把构建目录展开到每个文件（这里在轮询里跑）。
            git_output(root, &["status", "--porcelain", "-unormal"])
                .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
        })
        .collect();
    if let Ok(mut cache) = CHILD_CACHE.lock() {
        cache.get_or_insert_with(Default::default).insert(key, (std::time::Instant::now(), changed.clone()));
    }
    changed
}

/// 和会话目录相关的仓库：它所在仓库、这个仓库的各个 worktree、它下面两层的仓库及这些仓库各自的
/// worktree。手机远程页只放行这些（远程端传什么路径都进得来，不能借面板去看任意仓库）。
pub(crate) fn related_roots(cwd: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = repo_root(cwd) {
        if let Ok(output) = git_output(&root, &["worktree", "list", "--porcelain"]) {
            if output.status.success() {
                for line in stdout_text(&output.stdout).lines() {
                    if let Some(path) = line.strip_prefix("worktree ") {
                        out.push(norm_slashes(path));
                    }
                }
            }
        }
        if !out.iter().any(|known| same_path(known, &root)) {
            out.push(root);
        }
    }
    for child in child_repos(cwd) {
        // 子仓库的 worktree（前后端分仓时 agent 给 frontend 开的 worktree）：会话开在主目录、只用命令在
        // worktree 里干活时，没有它就认不出（用户 2026-11-09）。
        for wt in linked_worktrees(&child) {
            if !out.iter().any(|known| same_path(known, &wt)) {
                out.push(wt);
            }
        }
        if !out.iter().any(|known| same_path(known, &child)) {
            out.push(child);
        }
    }
    out
}

/// 命令文本里提到过的候选仓库，按首次出现的顺序。
///
/// 做法是把命令切成「像路径的词」，再看每个词本身或它在某个 `/` 处截断的前缀是不是某个候选的
/// 写法——而不是拿每个候选去全文 find：会话开在 workspace 时候选可达上百个，300 条 × 4KB 的命令
/// 逐个全文扫一遍会拖慢 5 秒一轮的轮询。只在 `/` 处截断，`meowo-x` 就不会命中 `meowo-x2`。
/// 候选的写法：绝对路径（正反斜杠、Windows 不分大小写）、Git Bash 的 `/c/...`、相对 cwd 的
/// `../x`、`./x`、`x/y`（agent 在 Windows 上多半跑在 Git Bash 里，三种都见过）。
fn command_mentioned_roots(cwd: &str, candidates: &[String], commands: &[String]) -> Vec<String> {
    let fold = |text: &str| {
        // 反斜杠归一成正斜杠，连写的 `//`（JSON 里转义过的 `\\`）收成一个。
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            let ch = if ch == '\\' { '/' } else { ch };
            if ch == '/' && out.ends_with('/') {
                continue;
            }
            out.push(ch);
        }
        if cfg!(target_os = "windows") {
            out.to_lowercase()
        } else {
            out
        }
    };
    // 会话目录与候选各取两种写法（原样、真实路径）：命令里多半是会话目录的写法，候选却来自 git /
    // worktree 记录（解开了软链、8.3 短名），只用一种算相对写法会对不上（CI 复现：macOS /var vs /private/var）。
    let cwd_forms: Vec<String> = {
        let raw = fold(&norm_slashes(cwd));
        let real = fold(&real_path(cwd));
        if raw == real { vec![raw] } else { vec![raw, real] }
    };
    // 值：(候选下标, 是不是裸目录名)。裸目录名（`backend`，不带 ./、../、也不含 /）在命令里常常只是
    // 个普通词（`git commit -m "fix backend"`、`npm run backend`），单独出现不算，要有路径的样子：
    // 前一个词是 cd / -C 这类，或者后面跟着 `/`（用户 2026-11-09：只列本会话用过的，误认等于混进别的）。
    let mut needles: std::collections::HashMap<String, (usize, bool)> = std::collections::HashMap::new();
    for (index, root) in candidates.iter().enumerate() {
        let mut roots = vec![fold(&norm_slashes(root))];
        let real = fold(&real_path(root));
        if !roots.contains(&real) {
            roots.push(real);
        }
        // 候选在会话目录下面时，补一个「会话目录原样写法 + 剩余部分」的绝对路径。
        for cwd_real in &cwd_forms[1..] {
            for r in roots.clone() {
                if let Some(rest) = r.strip_prefix(&format!("{cwd_real}/")) {
                    roots.push(format!("{}/{rest}", cwd_forms[0]));
                }
            }
        }
        for abs in &roots {
            for cwd_form in &cwd_forms {
                for form in path_spellings(abs, cwd_form) {
                    let bare = !form.contains('/');
                    needles.entry(form).or_insert((index, bare));
                }
            }
        }
    }
    let path_verb = |word: &str| {
        matches!(word, "cd" | "pushd" | "-c" | "-C" | "--prefix" | "--cwd" | "--dir" | "--work-tree" | "--git-dir" | "--manifest-path")
    };
    let mut hits: Vec<usize> = Vec::new();
    // 词的左边界：空白、引号、shell 运算符、`=`（`--dir=x`）、括号。右边界同理，外加 `/` 处截断。
    let is_delim = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '&' | '|' | '(' | ')' | '<' | '>' | '=' | ',');
    for command in commands {
        let text = fold(command);
        let words: Vec<&str> = text.split(is_delim).filter(|w| !w.is_empty()).collect();
        for (at, raw) in words.iter().enumerate() {
            let word = raw.trim_end_matches('/');
            let slash_after = word.len() < raw.len();
            let after_verb = at > 0 && path_verb(words[at - 1]);
            // 从长到短试：候选之间可能嵌套（仓库里的 `.claude/worktrees/x`），取最具体的那个。
            let mut prefixes = std::iter::once(word).chain(word.rmatch_indices('/').map(|(cut, _)| &word[..cut]));
            let hit = prefixes.find_map(|prefix| {
                let (index, bare) = *needles.get(prefix)?;
                // 裸目录名：整词就是它时要前面有 cd 这类、或后面带 `/`；作为前缀命中（`backend/src/a.rs`）本身就是路径。
                let path_like = !bare || prefix.len() < word.len() || slash_after || after_verb;
                path_like.then_some(index)
            });
            if let Some(index) = hit {
                if !hits.contains(&index) {
                    hits.push(index);
                }
            }
        }
    }
    hits.into_iter().map(|index| candidates[index].clone()).collect()
}

/// 一个仓库根（已 fold 过的绝对路径）在命令里可能的写法。
fn path_spellings(abs: &str, cwd: &str) -> Vec<String> {
    let mut out = vec![abs.to_string()];
    // `C:/x` ↔ Git Bash 的 `/c/x`。
    let bytes = abs.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        out.push(format!("/{}{}", (bytes[0] as char).to_ascii_lowercase(), &abs[2..]));
    }
    // 相对 cwd：数公共前缀段，剩下的 cwd 段各算一个 `..`。不在同一个盘上就没有相对写法。
    let root_segs: Vec<&str> = abs.split('/').collect();
    let cwd_segs: Vec<&str> = cwd.split('/').collect();
    let common = root_segs.iter().zip(&cwd_segs).take_while(|(a, b)| a == b).count();
    // Unix 路径首段是空串（根 `/`），只共享根不算「附近」，不给相对写法。
    let min_common = if root_segs[0].is_empty() { 2 } else { 1 };
    if common >= min_common {
        let ups = cwd_segs.len() - common;
        let rest = root_segs[common..].join("/");
        // rest 为空 = 仓库是 cwd 的上级，相对写法只剩 `..`，太常见，不认。
        if !rest.is_empty() {
            let mut parts = vec![".."; ups];
            parts.push(rest.as_str());
            let rel = parts.join("/");
            if ups == 0 {
                out.push(format!("./{rel}"));
            }
            out.push(rel);
        }
    }
    out
}

/// 远程端带来的目录是不是 session_dir 的相关仓库之一。
pub(crate) fn is_related_root(session_dir: &str, root: &str) -> bool {
    // 按真实路径比：git / worktree 记录给的是长名、解开软链的路径，会话目录可能是 8.3 短名
    //（CI 的 C:/Users/RUNNER~1）或软链（macOS 的 /var → /private/var），逐字比对不上（CI 复现）。
    let root = real_path(root);
    related_roots(session_dir).iter().any(|known| same_path(known, root.as_str()) || same_path(&real_path(known), &root))
}

/// 文件系统里的真实路径（解开软链、8.3 短名），正斜杠、去掉 Windows 的 `\\?\` 前缀。拿不到（不存在）
/// 就原样归一。
fn real_path(path: &str) -> String {
    match std::fs::canonicalize(path) {
        Ok(real) => {
            let text = real.to_string_lossy();
            let text = text.strip_prefix(r"\\?\UNC\").map(|rest| format!(r"\\{rest}")).unwrap_or_else(|| text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned());
            norm_slashes(&text)
        }
        Err(_) => norm_slashes(path),
    }
}

/// 一个仓库的两段改动。不是仓库（或 git 跑不起来）返回 None。
pub(crate) fn repo_changes(root: &str, is_cwd_repo: bool) -> Option<RepoChangesDto> {
    let summary = git_diff_summary_blocking(root).ok()?;
    if !summary.is_repo {
        return None;
    }
    let (base, base_label, committed) = match default_base(root, summary.branch.as_deref()) {
        Some((sha, label)) => {
            let committed = committed_files(root, &sha);
            (Some(sha), Some(label), committed)
        }
        None => (None, None, Vec::new()),
    };
    let name = Path::new(root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string());
    Some(RepoChangesDto {
        root: root.to_string(),
        name,
        is_cwd_repo,
        summary,
        base,
        base_label,
        committed,
        detailed: true,
    })
}

/// 仓库清单里没选中的仓库：只跑 status（分支、↑↓、文件清单），不数行数、不算「本分支已提交」。
fn repo_changes_light(root: &str, is_cwd_repo: bool) -> Option<RepoChangesDto> {
    let summary = summary_blocking(root, false).ok()?;
    if !summary.is_repo {
        return None;
    }
    let name = Path::new(root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string());
    Some(RepoChangesDto {
        root: root.to_string(),
        name,
        is_cwd_repo,
        summary,
        base: None,
        base_label: None,
        committed: Vec::new(),
        detailed: false,
    })
}

/// 「已提交未合并」的基准：merge-base(HEAD, 默认分支)。默认分支先认 origin/HEAD 指向的，
/// 再退到 origin/main、origin/master、本地 main、master。当前就在默认分支上、HEAD 本身就是
/// 基准（没有自己的提交）时返回 None。
fn default_base(root: &str, branch: Option<&str>) -> Option<(String, String)> {
    let base_ref = default_base_ref(root)?;
    let label = base_ref.strip_prefix("origin/").unwrap_or(&base_ref).to_string();
    if branch == Some(label.as_str()) {
        return None;
    }
    let output = git_output(root, &["merge-base", "HEAD", &base_ref]).ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = stdout_text(&output.stdout).trim().to_string();
    let head = git_output(root, &["rev-parse", "HEAD"]).ok()?;
    if stdout_text(&head.stdout).trim() == sha || !is_commit_sha(&sha) {
        return None;
    }
    Some((sha, label))
}

/// 默认分支的引用名（如 `origin/main`），按仓库缓存 BASE_REF_TTL。
fn default_base_ref(root: &str) -> Option<String> {
    static CACHE: TtlCache<Option<String>> = std::sync::Mutex::new(None);
    let key = norm_slashes(root).to_lowercase();
    if let Ok(cache) = CACHE.lock() {
        if let Some((at, hit)) = cache.as_ref().and_then(|map| map.get(&key)) {
            if at.elapsed() < BASE_REF_TTL {
                return hit.clone();
            }
        }
    }
    let found = resolve_default_base_ref(root);
    if let Ok(mut cache) = CACHE.lock() {
        cache.get_or_insert_with(Default::default).insert(key, (std::time::Instant::now(), found.clone()));
    }
    found
}

fn resolve_default_base_ref(root: &str) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(output) = git_output(root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]) {
        let name = stdout_text(&output.stdout).trim().to_string();
        if output.status.success() && name.starts_with("origin/") && name != "origin/HEAD" {
            candidates.push(name);
        }
    }
    for name in ["origin/main", "origin/master", "main", "master"] {
        if !candidates.iter().any(|c| c == name) {
            candidates.push(name.to_string());
        }
    }
    candidates.into_iter().find(|name| {
        git_output(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])
            .is_ok_and(|o| o.status.success())
    })
}

/// `git diff --name-status base HEAD` 加行数。
fn committed_files(root: &str, base: &str) -> Vec<GitChangedFileDto> {
    let Ok(output) = git_output(root, &["diff", "--name-status", "-z", "--no-renames", base, "HEAD"]) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let mut files = parse_name_status_z(&output.stdout);
    if let Ok(numstat) = git_output(root, &["diff", "--numstat", "-z", "--no-renames", base, "HEAD"]) {
        if numstat.status.success() {
            let counts = parse_numstat_z(&numstat.stdout);
            for file in files.iter_mut() {
                if let Some(&(added, deleted)) = counts.get(file.path.as_str()) {
                    file.additions = Some(added);
                    file.deletions = Some(deleted);
                }
            }
        }
    }
    files
}

/// `--name-status -z`：状态和路径各占一个 NUL 段（`M\0path\0A\0path\0…`）。
fn parse_name_status_z(bytes: &[u8]) -> Vec<GitChangedFileDto> {
    let parts: Vec<&[u8]> = bytes.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
    parts
        .chunks(2)
        .filter_map(|pair| {
            let [status, path] = pair else { return None };
            let letter = *status.first()?;
            Some(GitChangedFileDto {
                path: String::from_utf8_lossy(path).into_owned(),
                status: (letter as char).to_string(),
                additions: None,
                deletions: None,
                // 已提交的改动不分暂存区 / 工作区（只读段，没有暂存、放弃操作）。
                index_status: None,
                worktree_status: None,
                untracked_files: None,
            })
        })
        .collect()
}

/// 目录所在仓库（或 worktree）的根，正斜杠。
fn repo_root(dir: &str) -> Option<String> {
    let output = git_output(dir, &["rev-parse", "--show-toplevel"]).ok()?;
    if !output.status.success() {
        return None;
    }
    let root = stdout_text(&output.stdout).trim().to_string();
    (!root.is_empty()).then(|| norm_slashes(&root))
}

/// 文件路径往上找到第一个还在的目录（文件可能已被删掉）。
fn existing_dir_of(path: &str) -> Option<String> {
    let mut current = Path::new(path.trim());
    if current.is_dir() {
        return Some(current.to_string_lossy().into_owned());
    }
    while let Some(parent) = current.parent() {
        if parent.as_os_str().is_empty() {
            return None;
        }
        if parent.is_dir() {
            return Some(parent.to_string_lossy().into_owned());
        }
        current = parent;
    }
    None
}

/// 下一层里的仓库（有 `.git` 的子目录，`.git` 是目录或 worktree 的指针文件都算）。
fn child_repos(dir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for path in scan_dirs(dir) {
        if path.join(".git").exists() {
            out.push(norm_slashes(&path.to_string_lossy()));
            continue;
        }
        // 第二层：大项目常把仓库放在 apps/、services/、packages/ 下，主目录本身不是仓库
        //（用户 2026-11-09：前后端分仓的大项目）。只看不是仓库的目录，仓库里面不再往下扫。
        let skip = path.file_name().and_then(|n| n.to_str()).is_none_or(skip_scan);
        if skip {
            continue;
        }
        out.extend(
            scan_dirs(&path.to_string_lossy())
                .into_iter()
                .filter(|sub| sub.join(".git").exists())
                .map(|sub| norm_slashes(&sub.to_string_lossy())),
        );
    }
    out.sort();
    out
}

/// 目录下的子目录（最多 MAX_CHILD_SCAN 个）。
fn scan_dirs(dir: &str) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().take(MAX_CHILD_SCAN).map(|entry| entry.path()).filter(|path| path.is_dir()).collect()
}

/// 找仓库时不往里钻的目录：隐藏目录、依赖、构建产物。里面要么没有用户的仓库，要么是装进来的第三方
/// 仓库（node_modules 里带 .git 的包），列出来只会混淆。
fn skip_scan(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build" | "vendor" | "out")
}

/// 仓库自己的 linked worktree 根目录：读 `.git/worktrees/<名字>/gitdir`（里面是 worktree 的 `.git`
/// 文件路径），不起 git 进程——会话开在大项目主目录时子仓库几十个，5 秒一轮每个都跑 git 太贵。
fn linked_worktrees(repo: &str) -> Vec<String> {
    let admin = Path::new(repo).join(".git").join("worktrees");
    let Ok(entries) = std::fs::read_dir(admin) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("gitdir")).ok())
        .filter_map(|gitdir| Path::new(gitdir.trim()).parent().map(Path::to_path_buf))
        .filter(|root| root.is_dir())
        .map(|root| norm_slashes(&root.to_string_lossy()))
        .collect()
}

fn norm_slashes(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
}

/// path 在 root 目录里（或就是它）。按斜杠归一、Windows 不分大小写比较。
pub(crate) fn path_under(path: &str, root: &str) -> bool {
    let (p, r) = (norm_slashes(path), norm_slashes(root));
    let (p, r) = if cfg!(target_os = "windows") { (p.to_lowercase(), r.to_lowercase()) } else { (p, r) };
    p == r || p.starts_with(&format!("{r}/"))
}

/// 远程桥用：两个仓库根是不是同一个目录。
pub(crate) fn same_root(a: &str, b: &str) -> bool {
    same_path(a, b)
}

fn same_path(a: &str, b: &str) -> bool {
    let (a, b) = (norm_slashes(a), norm_slashes(b));
    if cfg!(target_os = "windows") {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

/// 单个文件的 diff。tracked 走 `git diff HEAD -- <path>`（staged+unstaged 合一）；
/// untracked 直接读盘合成伪 diff（`--- /dev/null` / `+++ b/<path>` 头 + 每行 `+` 前缀，
/// 见 GitFileDiffDto 注释）。
/// `base` 给了就是「已提交未合并」段的文件：看 `base..HEAD` 的 diff（base 是 overview 给出的
/// merge-base SHA）。
#[tauri::command]
pub(crate) async fn git_file_diff(
    cwd: String,
    path: String,
    untracked: bool,
    base: Option<String>,
) -> Result<GitFileDiffDto, String> {
    tauri::async_runtime::spawn_blocking(move || git_file_diff_blocking(&cwd, &path, untracked, base.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// 基准只认完整或缩写的提交 SHA：它会拼进 git 的参数里，不能让 `--output=…` 这类东西混进来。
fn is_commit_sha(value: &str) -> bool {
    (7..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn git_file_diff_blocking(cwd: &str, path: &str, untracked: bool, base: Option<&str>) -> Result<GitFileDiffDto, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    if let Some(base) = base {
        if !is_commit_sha(base) {
            return Err("git/bad_base".into());
        }
    } else if untracked {
        return untracked_file_diff(dir, path);
    }
    // 改动清单里的路径都相对仓库根（porcelain / name-status 的口径），而 `-- path` 默认按 cwd
    // 解析：cwd 是仓库子目录时 diff 是空的（做空复现）。`:(top)` 让它从仓库根算。
    // literal：`app/[id]/page.tsx` 这类文件名里的 `[ ] * ?` 不当通配符（Next.js 动态路由，做空复现
    // 一次拉回三个文件的 diff）。
    let spec = format!(":(top,literal){path}");
    let output = match base {
        Some(base) => git_output(dir, &["diff", base, "HEAD", "--", &spec]),
        None => git_output(dir, &["diff", "HEAD", "--", &spec]),
    }
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("git diff 执行失败".into());
    }
    let text = stdout_text(&output.stdout);
    // 跟踪文件里的二进制(图片、含 NUL 的源码等):git 只回「Binary files a/… and b/…
    // differ」这一行,没有 hunk。原样透出会渲染成几行灰底 meta(看着像坏掉),归一成
    // 与 untracked 分支同一个「(binary file)」占位,前端据此显示「二进制文件,无法显示
    // diff」,而不是误报「文件与最新提交一致」。
    if diff_is_binary(&text) {
        return Ok(GitFileDiffDto {
            path: path.to_string(),
            status: "M".into(),
            diff: "(binary file)\n".into(),
            truncated: false,
        });
    }
    let (diff, truncated) = truncate_utf8(text, MAX_DIFF_BYTES);
    Ok(GitFileDiffDto {
        path: path.to_string(),
        status: "M".into(),
        diff,
        truncated,
    })
}

/// `git diff` 对二进制文件只吐一行 `Binary files a/… and b/… differ`(新增/删除侧
/// 是 `/dev/null`),没有 `@@` hunk。命中即视为二进制,归一到 `(binary file)` 占位。
fn diff_is_binary(diff: &str) -> bool {
    diff.lines()
        .any(|line| line.starts_with("Binary files ") && line.ends_with(" differ"))
}

/// 未跟踪文件的伪 diff：读盘（封顶 MAX_DIFF_BYTES），二进制嗅探命中则只回占位行。
fn untracked_file_diff(dir: &str, path: &str) -> Result<GitFileDiffDto, String> {
    use std::io::Read;
    // path 正常来自 git status，但前端传什么都进得来：裸 join 时绝对路径会整个顶掉
    // cwd，`..` 能一路爬出工作目录。统一走 resolve_inside（canonicalize + 前缀校验），
    // 越界返回与文件页签同一句「路径超出工作目录」。tracked 分支不需要——那边 path
    // 是 git 的 pathspec，git 自己限定在仓库内。
    let full_path = crate::fsutil::resolve_inside(dir, path)?;
    let file = std::fs::File::open(&full_path).map_err(|e| format!("无法读取文件：{e}"))?;
    let mut bytes = Vec::new();
    file.take((MAX_DIFF_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("无法读取文件：{e}"))?;
    let truncated = bytes.len() > MAX_DIFF_BYTES;
    if bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0) {
        return Ok(GitFileDiffDto {
            path: path.to_string(),
            status: "U".into(),
            diff: "(binary file)\n".into(),
            truncated: false,
        });
    }
    if truncated {
        bytes.truncate(MAX_DIFF_BYTES);
    }
    // 与 stdout_text 同一解码口径：untracked 的 GBK 文件同样不能整屏替换符。
    let text = crate::fsutil::decode_text_bytes(&bytes);
    let mut diff = format!("--- /dev/null\n+++ b/{path}\n");
    for line in text.lines() {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }
    Ok(GitFileDiffDto {
        path: path.to_string(),
        status: "U".into(),
        diff,
        truncated,
    })
}

/// 当前分支的 Pull Request（对话页标题栏，对照 Kimi 的「打开 Pull Request」）。走 `gh pr view`：
/// 没装 gh、没登录、不是 GitHub 仓库、分支没有 PR 都是 None（不报错，前端不显示）。
/// gh 要联网，按目录缓存 60s：标题栏每次重渲染不该打一次 GitHub API。
#[tauri::command]
pub(crate) async fn git_pull_request(cwd: String) -> Result<Option<PullRequestDto>, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(pull_request_cached(&cwd)))
        .await
        .map_err(|e| e.to_string())?
}

const PR_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

type PrCache = std::collections::HashMap<String, (std::time::Instant, Option<PullRequestDto>)>;

fn pull_request_cached(cwd: &str) -> Option<PullRequestDto> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<PrCache>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = cwd.trim().to_string();
    if let Some((at, value)) = cache.lock().ok().and_then(|map| map.get(&key).cloned()) {
        if at.elapsed() < PR_CACHE_TTL {
            return value;
        }
    }
    let value = pull_request_blocking(&key);
    if let Ok(mut map) = cache.lock() {
        map.insert(key, (std::time::Instant::now(), value.clone()));
    }
    value
}

fn pull_request_blocking(dir: &str) -> Option<PullRequestDto> {
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return None;
    }
    let mut command = Command::new("gh");
    command
        .current_dir(dir)
        .args(["pr", "view", "--json", "number,state,isDraft,url,title"]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW，同 git_output
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_pull_request(&String::from_utf8_lossy(&output.stdout))
}

/// `gh pr view --json number,state,isDraft,url,title` 的输出 → DTO。草稿是 OPEN + isDraft。
pub(crate) fn parse_pull_request(json: &str) -> Option<PullRequestDto> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let number = u32::try_from(value.get("number")?.as_u64()?).ok()?;
    let draft = value
        .get("isDraft")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let state = match value.get("state")?.as_str()? {
        "OPEN" if draft => "draft",
        "OPEN" => "open",
        "MERGED" => "merged",
        "CLOSED" => "closed",
        _ => return None,
    };
    Some(PullRequestDto {
        number,
        state: state.to_string(),
        url: value.get("url")?.as_str()?.to_string(),
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        ..Default::default()
    })
}

// ═══ 写操作：暂存 / 取消暂存 / 放弃 / 提交（用户要求 2026-11-09：面板照 VS Code 源代码管理能直接操作）═══

/// 一次 git 调用里路径参数最多拼这么多字节：Windows 命令行上限 32K 字符，「全部暂存」几百个
/// 文件一次塞进去会 spawn 失败，分批跑。
const MAX_PATHSPEC_BYTES: usize = 16 * 1024;

/// 环境变量覆盖：Some 设值、None 删掉。正式命令恒为空，测试用它隔离用户的全局 git 配置。
type GitEnv<'a> = &'a [(&'a str, Option<&'a str>)];

/// 暂存：`git add`。paths 相对仓库根，必须是当前有工作区改动的文件。
#[tauri::command]
pub(crate) async fn git_stage(cwd: String, paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || stage_blocking(&cwd, &paths))
        .await
        .map_err(|e| e.to_string())?
}

/// 取消暂存：`git restore --staged`；仓库还没有提交（没有 HEAD 可恢复）时用 `git rm --cached`。
#[tauri::command]
pub(crate) async fn git_unstage(cwd: String, paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || unstage_blocking(&cwd, &paths))
        .await
        .map_err(|e| e.to_string())?
}

/// 放弃工作区改动：已跟踪的恢复成暂存区内容，未跟踪的移进系统回收站。确认由前端负责。
#[tauri::command]
pub(crate) async fn git_discard(cwd: String, paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || discard_blocking(&cwd, &paths))
        .await
        .map_err(|e| e.to_string())?
}

/// 提交暂存的改动，返回新提交的短 SHA。`all` = 先 `git add -A`（一个都没暂存时的「提交全部」，
/// 照 VS Code 的默认做法，含未跟踪文件）。
#[tauri::command]
///
/// `paths`：「提交全部」时界面上显示的那些文件。带了就只暂存它们（还得都在此刻的改动清单里），
/// 列表是几秒前轮询的，agent 正在写的、还没显示出来的文件不跟着进提交（做空第 2 轮）。不带 = 此刻全部。
pub(crate) async fn git_commit(cwd: String, message: String, all: bool, paths: Option<Vec<String>>) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || commit_blocking(&cwd, &message, all, paths.as_deref(), &[]))
        .await
        .map_err(|e| e.to_string())?
}

/// 冲突文件不许暂存 / 放弃：要先在终端或编辑器里解决（见 split_porcelain_xy 的 "!"）。
fn refuse_conflicts(files: &[GitChangedFileDto]) -> Result<(), String> {
    if files.iter().any(|f| f.worktree_status.as_deref() == Some("!")) {
        return Err("git/conflict".into());
    }
    Ok(())
}

fn stage_blocking(cwd: &str, paths: &[String]) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    let files = listed_changes(dir, paths, |f| f.worktree_status.is_some())?;
    refuse_conflicts(&files)?;
    let result = stage_paths(dir, &files).and_then(|specs| run_pathspec_batches(dir, &["add"], &specs));
    invalidate_caches();
    result
}

fn unstage_blocking(cwd: &str, paths: &[String]) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    listed_changes(dir, paths, |f| f.index_status.is_some())?;
    let has_head = git_output(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok_and(|o| o.status.success());
    // 没有 HEAD 时 restore --staged 无处可恢复会报错。rm --cached 只动暂存区、不碰工作区文件；
    // -f 是因为「已暂存又改过」（AM）的文件暂存区和工作区、HEAD 都不一样，不加会被拒。
    let args: &[&str] = if has_head { &["restore", "--staged"] } else { &["rm", "--cached", "-q", "-f"] };
    let result = run_pathspec_batches(dir, args, paths);
    invalidate_caches();
    result
}

/// 把一个未跟踪文件挪走的实现。抽成函数指针是为了测试能注入「回收失败」，验证不退回永久删除。
type TrashFn = fn(&[std::path::PathBuf]) -> Result<(), String>;

/// 移进系统回收站（Windows 回收站 / macOS 废纸篓 / Linux freedesktop Trash）。
/// 未跟踪文件 git 里没有任何副本，以前直接 remove_file，手滑点了「放弃」就永久没了。
/// 失败一律报 git/trash_failed、文件原样保留，**不退回永久删除**：没回收站的盘（网络盘、
/// 部分 U 盘）上宁可让用户自己去删，也不替用户做不可恢复的决定。
fn trash_to_recycle_bin(paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    // 必须在一次性的专用线程里调：trash 5.2.9 在 Windows 上把所在线程 CoInitializeEx 成 STA 且
    // 永不 CoUninitialize（windows.rs:317-348）。若直接跑在 spawn_blocking 线程池里，这个线程以后
    // 被 focus_session 复用时 UIAutomation 要 MTA 会失败（terminal.rs:144 要求干净 apartment），
    // 反过来先跑过 UIA 的线程上 trash 也会失败（做空审查 comprobe 探针实测两向都复现）。
    // 用完即弃的线程让 COM 状态随线程结束。
    // 整批一次 delete_all：一个线程、一次 COM 初始化、一次 IFileOperation。逐个调的话，新生成的
    // 目录有上千个未跟踪文件（-uall 逐个列出）时要串行上千轮，回收站里也散成上千条（做空审查）。
    let owned = paths.to_vec();
    let joined = std::thread::Builder::new()
        .name("meowo-trash".into())
        .spawn(move || {
            // trash 的 Windows 实现对 CoCreateInstance 直接 unwrap（windows.rs:95），COM 起不来会
            // panic；接住它按回收失败报错。
            std::panic::catch_unwind(|| trash_context().delete_all(&owned))
        })
        .map_err(|e| format!("git/trash_failed: {e}"))?
        .join();
    match joined {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => return Err(format!("git/trash_failed: {e}")),
        Ok(Err(_)) | Err(_) => return Err("git/trash_failed: recycle bin unavailable".into()),
    }
    // 防御：IFileOperation 只靠 GetAnyOperationsAborted 报失败，trash 自己也注明拿不到具体
    // 原因（windows.rs:111）。落地逐个再核一次，免得界面说放弃成功、文件其实还在。
    if paths.iter().any(|path| std::fs::symlink_metadata(path).is_ok()) {
        return Err("git/trash_failed: file still exists".into());
    }
    Ok(())
}

fn trash_context() -> trash::TrashContext {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    // macOS 默认走 osascript 让 Finder 删：首次会弹「Meowo 想控制 Finder」的自动化授权框，
    // 每删一个都放废纸篓音效（trash 5.2.9 macos/mod.rs DeleteMethod 文档表）。NSFileManager
    // 不要额外权限、无声；代价是部分系统版本右键没有「放回原处」，文件仍在废纸篓可拖出。
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx
}

fn discard_blocking(cwd: &str, paths: &[String]) -> Result<(), String> {
    discard_with(cwd, paths, trash_to_recycle_bin)
}

fn discard_with(cwd: &str, paths: &[String], trash: TrashFn) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    let files = listed_changes(dir, paths, |f| f.worktree_status.is_some())?;
    refuse_conflicts(&files)?;
    let (untracked, tracked): (Vec<_>, Vec<_>) =
        files.iter().partition(|f| f.worktree_status.as_deref() == Some("U"));
    let tracked: Vec<String> = tracked.iter().map(|f| f.path.clone()).collect();
    let result = (|| {
        // --worktree 只恢复工作区（到暂存区的内容）：已暂存的那部分改动是用户挑好的，不跟着丢。
        run_pathspec_batches(dir, &["restore", "--worktree"], &tracked)?;
        // porcelain 的路径相对仓库根；cwd 按约定就是仓库根，保险起见还是以 git 认的根为准。
        let root = repo_root(dir).unwrap_or_else(|| dir.to_string());
        let mut to_trash = Vec::new();
        for file in untracked {
            if let Some(rel) = file.path.strip_suffix('/') {
                // 折叠的未跟踪目录：整个目录一次进回收站（一条，能整个还原），先确认里面没有链接和嵌套仓库。
                to_trash.push(untracked_dir_to_trash(&root, rel)?);
                continue;
            }
            // 不 canonicalize：未跟踪的符号链接要删的是链接本身，跟过去会删到链接指向的文件。
            // 路径已在 listed_changes 里挡掉了绝对路径和 `..`，git 也不会穿过符号链接目录列文件。
            let full = Path::new(&root).join(&file.path);
            let meta = std::fs::symlink_metadata(&full).map_err(|e| format!("git/failed: {e}"))?;
            // 文件条目却是目录（清单之后被换掉了）：不递归删。目录只走上面那条带检查的路。
            if meta.is_dir() {
                return Err(format!("git/failed: {} 是目录", file.path));
            }
            if meta.file_type().is_symlink() {
                // 符号链接不进回收站，照旧只删链接本身。链接不承载数据（内容在指向处），
                // 删了零损失。trash 5.2.9 本身只规范化父目录、保留链接名（其 lib.rs
                // canonicalize_paths），但最终交给 Windows Shell / Finder 处理链接：早期 Explorer
                // 删 junction 会连带删目标内容，Finder 的行为本机也验证不了，不值得冒这个险。
                std::fs::remove_file(&full).map_err(|e| format!("git/failed: {e}"))?;
            } else {
                to_trash.push(full);
            }
        }
        // 只放弃了符号链接（已在上面直接删掉）时没有要进回收站的：不调回收站。CI 的 Linux/macOS
        // 能建链接，discard_untracked_symlink_removes_link_only 注入必败回收站钉住这条（本机 Windows
        // 无开发者模式建不了链接，该测试跳过，第一次整批化时漏在这里）。
        if to_trash.is_empty() {
            Ok(())
        } else {
            trash(&to_trash)
        }
    })();
    invalidate_caches();
    result
}

/// 整目录放弃前的检查，通过了返回要进回收站的绝对路径。
///
/// 目录本身、它在仓库里的任何一级上级、或里面任何一处是符号链接 / junction 就拒（git/dir_has_links），
/// 一个文件都不动：回收站对链接的处理靠 Windows Shell / Finder，早期 Explorer 删 junction 会连带删掉
/// 指向处的内容（同 discard_with 里单个链接不进回收站的理由），整目录放弃时链接可能藏在几层之下，
/// 用户在确认框里看不到。Windows 上 `FileType::is_symlink` 对 junction 也为 true（std 按 name-surrogate
/// 重解析点判断，discard_dir_refuses_junction 实测钉住）。
/// 里面有嵌套仓库（任何一层出现 `.git`）也拒（git/dir_has_repo）：那是另一个仓库，有自己的提交历史，
/// 不是这个仓库的「更改」。
fn untracked_dir_to_trash(root: &str, rel: &str) -> Result<std::path::PathBuf, String> {
    let mut current = Path::new(root).to_path_buf();
    for segment in rel.split('/') {
        current.push(segment);
        let meta = std::fs::symlink_metadata(&current).map_err(|e| format!("git/failed: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err("git/dir_has_links".into());
        }
    }
    if !std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.is_dir()) {
        return Err("git/not_in_changes".into());
    }
    let mut stack = vec![current.clone()];
    while let Some(next) = stack.pop() {
        let entries = std::fs::read_dir(&next).map_err(|e| format!("git/failed: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("git/failed: {e}"))?;
            let kind = entry.file_type().map_err(|e| format!("git/failed: {e}"))?;
            if kind.is_symlink() {
                return Err("git/dir_has_links".into());
            }
            if entry.file_name() == ".git" {
                return Err("git/dir_has_repo".into());
            }
            if kind.is_dir() {
                stack.push(entry.path());
            }
        }
    }
    Ok(current)
}

fn commit_blocking(cwd: &str, message: &str, all: bool, shown: Option<&[String]>, env: GitEnv) -> Result<String, String> {
    let dir = existing_dir(cwd)?;
    let message = message.trim();
    if message.is_empty() {
        return Err("git/empty_message".into());
    }
    let run = |args: &[&str], stdin: Option<&[u8]>| git_run(dir, args, env, stdin).map_err(|e| format!("git/failed: {e}"));
    // 「提交全部」替用户暂存的文件：提交没成（没配身份、钩子失败……）就原样退回，不留下用户没做过的暂存。
    let mut staged_by_us: Vec<String> = Vec::new();
    let result = (|| {
        // 有冲突（unmerged）不提交：冲突文件在列表里只显示成 M，「提交全部」会把带冲突标记的内容
        // 当成已解决提交进去（做空复现：agent 在 worktree 里 merge 到一半，手机上顺手一点）。
        let unmerged = run(&["diff", "--name-only", "--diff-filter=U"], None)?;
        if !unmerged.status.success() {
            return Err(classify_git_failure(&unmerged));
        }
        if !stdout_text(&unmerged.stdout).trim().is_empty() {
            return Err("git/conflict".into());
        }
        if all {
            // 只暂存列表里显示的那些文件，不用 `add -A`：未跟踪的嵌套仓库（.claude/worktrees/x）
            // 在列表里不显示，`add -A` 却会把它当 gitlink 提交进去（做空复现）。范围与界面一致。
            let files = match shown {
                Some(shown) => listed_changes(dir, shown, |f| f.worktree_status.is_some())?,
                None => {
                    let status = run(&["status", "--porcelain", "-z", "--no-renames", "-uall"], None)?;
                    if !status.status.success() {
                        return Err(classify_git_failure(&status));
                    }
                    parse_porcelain_z(&status.stdout)
                }
            };
            refuse_conflicts(&files)?;
            let files: Vec<GitChangedFileDto> = files.into_iter().filter(|f| f.worktree_status.is_some()).collect();
            // 折叠的未跟踪目录同「暂存」：整目录加，有嵌套仓库就换成其中的文件（stage_paths）。
            let paths = stage_paths(dir, &files)?;
            staged_by_us = files
                .iter()
                .filter(|f| f.index_status.is_none())
                .map(|f| f.path.clone())
                .collect();
            run_pathspec_batches(dir, &["add"], &paths)?;
        }
        // 先用退出码判「没东西可提交」「没配身份」：git 的提示文字会随用户的语言设置本地化，
        // 光靠匹配英文输出认不全。diff --cached --quiet 有差异时退出码 1。
        // 合并进行中（MERGE_HEAD）没有差异也能提交合并结果，不拦。
        let staged = run(&["diff", "--cached", "--quiet"], None)?;
        match staged.status.code() {
            Some(1) => {}
            Some(0) => {
                let merging = run(&["rev-parse", "-q", "--verify", "MERGE_HEAD"], None)?.status.success();
                if !merging {
                    return Err("git/nothing_to_commit".into());
                }
            }
            _ => return Err(classify_git_failure(&staged)),
        }
        for ident in ["GIT_AUTHOR_IDENT", "GIT_COMMITTER_IDENT"] {
            if !run(&["var", ident], None)?.status.success() {
                return Err("git/identity".into());
            }
        }
        // 信息走 stdin（-F -）：多行、引号、开头是 `-` 都不用操心转义。不加 --no-verify，
        // 用户的提交钩子照跑，钩子失败按 git/failed 带出它的输出。
        let committed = run(&["commit", "-F", "-"], Some(message.as_bytes()))?;
        if !committed.status.success() {
            return Err(classify_git_failure(&committed));
        }
        let head = run(&["rev-parse", "--short", "HEAD"], None)?;
        Ok(stdout_text(&head.stdout).trim().to_string())
    })();
    if result.is_err() && !staged_by_us.is_empty() {
        // 退回失败不覆盖原来的错误：用户要看的是提交为什么没成。
        let has_head = git_output(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok_and(|o| o.status.success());
        // -r：退回的可能是整个目录（折叠的未跟踪目录），rm 不带 -r 拒绝目录。
        let args: &[&str] = if has_head { &["restore", "--staged"] } else { &["rm", "--cached", "-q", "-f", "-r"] };
        let _ = run_pathspec_batches(dir, args, &staged_by_us);
    }
    invalidate_caches();
    result
}

fn existing_dir(cwd: &str) -> Result<&str, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    Ok(dir)
}

/// 写操作的路径收口：每个 path 都得逐字是此刻改动清单里满足 `want` 的一项（前端只会给清单里的，
/// 手机远程端传什么都进得来——不收口就能借「放弃」删任意文件）。绝对路径、含 `..` 的直接拒。
///
/// 清单按 -unormal 取（同摘要）：整个未跟踪的目录只有一条 `dir/`。所以认三种：普通条目、未跟踪目录
/// 条目本身（折叠行的整目录暂存 / 放弃）、未跟踪目录里的文件（折叠行点开后逐个操作，或文件少、摘要里
/// 已展开的）。第三种再问一次 ls-files，确认它真是未跟踪且没被忽略的文件，不能凭「在某个未跟踪
/// 目录下面」就放行（目录里被忽略的文件、嵌套仓库都不在清单里）。
fn listed_changes(
    dir: &str,
    paths: &[String],
    want: fn(&GitChangedFileDto) -> bool,
) -> Result<Vec<GitChangedFileDto>, String> {
    if paths.iter().any(|p| unsafe_rel_path(p)) {
        return Err("git/not_in_changes".into());
    }
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    for entry in status_entries(dir)? {
        match entry {
            StatusEntry::File(file) => files.push(file),
            StatusEntry::UntrackedDir(path) => dirs.push(path),
        }
    }
    let mut out: Vec<Option<GitChangedFileDto>> = Vec::with_capacity(paths.len());
    let mut inside: Vec<String> = Vec::new();
    for path in paths {
        if let Some(file) = files.iter().find(|f| f.path == *path) {
            out.push(Some(file.clone()));
        } else if dirs.iter().any(|d| d == path) {
            out.push(Some(untracked_dto(path.clone(), None)));
        } else if !path.ends_with('/') && dirs.iter().any(|d| path.starts_with(d.as_str())) {
            inside.push(path.clone());
            out.push(None);
        } else {
            return Err("git/not_in_changes".into());
        }
    }
    let confirmed: std::collections::HashSet<String> = if inside.is_empty() {
        Default::default()
    } else {
        untracked_listing(dir, &inside)?.into_iter().collect()
    };
    paths
        .iter()
        .zip(out)
        .map(|(path, found)| {
            found
                .or_else(|| confirmed.contains(path).then(|| untracked_dto(path.clone(), None)))
                .filter(want)
                .ok_or_else(|| "git/not_in_changes".to_string())
        })
        .collect()
}

/// 手机桥看单文件 diff 的收口：path 是此刻清单里一个未跟踪的文件（含折叠目录里的），不是目录。
pub(crate) fn untracked_file_listed(cwd: &str, path: &str) -> bool {
    !path.ends_with('/')
        && listed_changes(cwd.trim(), &[path.to_string()], |f| f.worktree_status.as_deref() == Some("U")).is_ok()
}

/// 暂存用的路径：未跟踪目录条目原样交给 `git add -- dir/`（一万五千个文件一次加完），但目录里有嵌套
/// 仓库时换成其中的文件逐个加：`git add dir/` 会把嵌套仓库当 gitlink 加进去（同
/// commit_all_skips_untracked_nested_repo 那条做空结论）。
fn stage_paths(dir: &str, files: &[GitChangedFileDto]) -> Result<Vec<String>, String> {
    let dirs: Vec<String> = files.iter().filter(|f| f.path.ends_with('/')).map(|f| f.path.clone()).collect();
    if dirs.is_empty() {
        return Ok(files.iter().map(|f| f.path.clone()).collect());
    }
    let mut buckets = bucket_by_dir(&dirs, &untracked_listing(dir, &dirs)?);
    let mut out = Vec::new();
    for file in files {
        match buckets.remove(&file.path) {
            Some(bucket) if bucket.nested_repo => out.extend(bucket.files),
            Some(_) => out.push(file.path.clone()),
            None if file.path.ends_with('/') => {}
            None => out.push(file.path.clone()),
        }
    }
    Ok(out)
}

fn unsafe_rel_path(path: &str) -> bool {
    path.is_empty()
        || path.starts_with(['/', '\\'])
        || Path::new(path).is_absolute()
        // `C:foo` 这种盘符相对路径 is_absolute 认不出。
        || path.as_bytes().get(1) == Some(&b':')
        || path.split(['/', '\\']).any(|seg| seg == "..")
}

/// `git <args> -- <pathspec…>`，路径多时分批。`:(top,literal)` 同 git_file_diff：从仓库根算、
/// 方括号星号不当通配符（不然暂存 `app/[id]/page.tsx` 会把 `app/i/page.tsx` 一起暂存了）。
fn run_pathspec_batches(dir: &str, args: &[&str], paths: &[String]) -> Result<(), String> {
    let specs: Vec<String> = paths.iter().map(|p| format!(":(top,literal){p}")).collect();
    for batch in pathspec_batches(&specs) {
        let mut argv: Vec<&str> = args.to_vec();
        argv.push("--");
        argv.extend(batch.iter().map(String::as_str));
        let output = git_output(dir, &argv).map_err(|e| format!("git/failed: {e}"))?;
        if !output.status.success() {
            return Err(classify_git_failure(&output));
        }
    }
    Ok(())
}

/// 把 pathspec 切成每批不超过 MAX_PATHSPEC_BYTES 的几段。空输入不出任何一批：调用方据此不跑 git，
/// 不带 pathspec 的 ls-files / add 意味着「整个仓库」。
fn pathspec_batches(specs: &[String]) -> Vec<&[String]> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < specs.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < specs.len() && (end == start || bytes + specs[end].len() < MAX_PATHSPEC_BYTES) {
            bytes += specs[end].len() + 1;
            end += 1;
        }
        out.push(&specs[start..end]);
        start = end;
    }
    out
}

/// git 失败归类成「git/代码」（前端 i18n/errors.ts 映射成中英文提示）。index.lock 出现在路径里，
/// 不受语言设置影响；其余几条英文匹配是兜底，主判据在 commit_blocking 里按退出码先判了。
fn classify_git_failure(output: &std::process::Output) -> String {
    let stderr = stdout_text(&output.stderr);
    let stdout = stdout_text(&output.stdout);
    let both = format!("{stderr}\n{stdout}");
    if both.contains("index.lock") {
        return "git/locked".into();
    }
    if both.contains("Please tell me who you are") || both.contains("user.email") || both.contains("unable to auto-detect email") {
        return "git/identity".into();
    }
    if both.contains("nothing to commit") || both.contains("no changes added to commit") {
        return "git/nothing_to_commit".into();
    }
    // 钩子失败的输出可能只在 stdout（lint 工具多半打 stdout）。只带最后几行，错误提示别撑爆界面。
    let source = if stderr.trim().is_empty() { &stdout } else { &stderr };
    let lines: Vec<&str> = source.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(6)..].join("\n");
    format!("git/failed: {tail}")
}

/// 写操作之后清掉会让列表滞后的缓存：「下一层有改动的仓库」缓存 30 秒，提交 / 放弃之后某个仓库
/// 可能已经干净了（或刚暂存的状态要立刻反映）。默认分支名的缓存不受写操作影响，不清。
fn invalidate_caches() {
    if let Ok(mut cache) = CHILD_CACHE.lock() {
        *cache = None;
    }
    // 暂存 / 放弃了折叠目录里的文件，数字要马上对上。
    if let Ok(mut cache) = FOLD_CACHE.lock() {
        *cache = None;
    }
}

// 测试用：本线程起过几个 git 进程（摘要的进程数预算，见 summary_spawns_at_most_four_git_processes）。
#[cfg(test)]
thread_local! {
    static GIT_SPAWNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn git_output(dir: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    git_run(dir, args, &[], None)
}

/// spawn git 的唯一出口。stdin 给了就喂进去（提交信息），用单独线程写：提交钩子先跑、git 后读
/// stdin，信息又长时，同步写满管道会和钩子写满 stdout 互相等死。
fn git_run(dir: &str, args: &[&str], env: GitEnv, stdin: Option<&[u8]>) -> std::io::Result<std::process::Output> {
    use std::process::Stdio;
    #[cfg(test)]
    GIT_SPAWNS.with(|n| n.set(n.get() + 1));
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    for (key, value) in env {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    // Windows：GUI 进程 spawn 控制台子进程会闪现 conhost 黑窗，压掉。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let Some(input) = stdin else {
        return command.output();
    };
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut pipe = child.stdin.take();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        if let Some(pipe) = pipe.as_mut() {
            let _ = pipe.write_all(&input);
        }
        // 线程结束时 pipe 被 drop，git 读到 EOF。
    });
    let output = child.wait_with_output();
    let _ = writer.join();
    output
}

/// git 输出解码走 fsutil 的统一口径（严格 UTF-8 → GBK → lossy）：GBK 源码文件
/// （中文 Windows 常见）的 diff 此前被无脑 lossy 解成整屏替换符。
fn stdout_text(bytes: &[u8]) -> String {
    crate::fsutil::decode_text_bytes(bytes)
}

/// 截断到 cap 字节（落在 UTF-8 字符边界上），返回是否发生了截断。
fn truncate_utf8(mut text: String, cap: usize) -> (String, bool) {
    if text.len() <= cap {
        return (text, false);
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

/// porcelain 的 X / Y 拆成「暂存区」「工作区」两个状态（「更改」面板的两组）。
/// 冲突中的条目（UU/AA/DD/AU…）的 Y 位 `U` 表示「未合并」，与未跟踪的 "U" 撞字母：放弃更改按
/// "U" 走删文件，冲突文件绝不能被当成未跟踪删掉，所以冲突一律只记工作区 "M"、不记暂存区——
/// 能暂存（= 标记冲突已解决），放弃时 git restore 会自己拒绝未合并的路径。
fn split_porcelain_xy(x: u8, y: u8) -> (Option<String>, Option<String>) {
    if x == b'?' && y == b'?' {
        return (None, Some("U".into()));
    }
    let unmerged = x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D');
    if unmerged {
        // 冲突单独标 "!"（VS Code 同样单列）：标成 M 时用户分不出来，点「暂存」= git 当它已解决，
        // 带冲突标记的内容随后被提交（做空第 2 轮复现）。不能用 porcelain 的 U，会和未跟踪撞字母、
        // 放弃时被当未跟踪文件删掉。暂存 / 放弃对它一律拒绝（git/conflict）。
        return (None, Some("!".into()));
    }
    let letter = |b: u8| (b != b' ' && b != b'?').then(|| (b as char).to_string());
    (letter(x), letter(y))
}

/// 解析 `git status --porcelain -z --no-renames` 输出：每个条目是 `XY <path>\0`。
/// 状态归一化：X/Y 任一为 M/A/D/T 取该字母（X 优先）；`??` 记为 "U"（未跟踪）。
/// 目录条目不在结果里（要目录条目的走 parse_status_entries）。
fn parse_porcelain_z(bytes: &[u8]) -> Vec<GitChangedFileDto> {
    parse_status_entries(bytes)
        .into_iter()
        .filter_map(|entry| match entry {
            StatusEntry::File(file) => Some(file),
            StatusEntry::UntrackedDir(_) => None,
        })
        .collect()
}

/// 同 parse_porcelain_z，另把 `?? dir/`（-unormal 下整个未跟踪的目录）作为目录条目留下。
fn parse_status_entries(bytes: &[u8]) -> Vec<StatusEntry> {
    let mut files = Vec::new();
    for entry in bytes.split(|b| *b == 0) {
        if entry.len() < 4 {
            continue;
        }
        let x = entry[0];
        let y = entry[1];
        // entry[2] 是空格分隔符；路径原样保留（porcelain 用正斜杠，前端按它展示）。
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        // 目录条目(尾斜杠,「?? dir/」)不是文件:当文件渲染是一行空名,点开按文件读目录必报错。
        // 未跟踪的单独留作目录条目交给 fold_untracked_dirs;其余(嵌套仓库之类)跳过。
        if path.is_empty() {
            continue;
        }
        if path.ends_with('/') {
            if x == b'?' && y == b'?' {
                files.push(StatusEntry::UntrackedDir(path));
            }
            continue;
        }
        let status: String = if x == b'?' && y == b'?' {
            "U".into()
        } else if matches!(x, b'M' | b'A' | b'D' | b'T') {
            // X 位优先：已暂存的变更比工作区的更能代表「相对 HEAD」的状态。
            (x as char).to_string()
        } else if matches!(y, b'M' | b'A' | b'D' | b'T') {
            (y as char).to_string()
        } else {
            // 不认识的状态字母（R/C 等，--no-renames 下本不该出现）：原样透出非空格位。
            let other = if x == b' ' { y } else { x };
            if other == b' ' || other == b'?' {
                continue;
            }
            (other as char).to_string()
        };
        let (index_status, worktree_status) = split_porcelain_xy(x, y);
        files.push(StatusEntry::File(GitChangedFileDto {
            path,
            status,
            additions: None,
            deletions: None,
            index_status,
            worktree_status,
            untracked_files: None,
        }));
    }
    files
}

#[cfg(test)]
mod tests {

    #[test]
    fn 解析_gh_pr_view_输出_草稿与合并都认得() {
        let pr = parse_pull_request(r#"{"number":42,"state":"OPEN","isDraft":true,"url":"https://github.com/o/r/pull/42","title":"t"}"#).unwrap();
        assert_eq!(
            (pr.number, pr.state.as_str(), pr.url.as_str()),
            (42, "draft", "https://github.com/o/r/pull/42")
        );
        let state_of = |json: &str| parse_pull_request(json).unwrap().state;
        assert_eq!(
            state_of(r#"{"number":7,"state":"MERGED","isDraft":false,"url":"u","title":""}"#),
            "merged"
        );
        assert_eq!(
            state_of(r#"{"number":7,"state":"OPEN","isDraft":false,"url":"u","title":""}"#),
            "open"
        );
        assert!(parse_pull_request("no pull requests found").is_none());
    }

    use super::*;

    #[test]
    fn numstat_z_parses_counts_and_skips_binary() {
        let raw = b"12\t3\tsrc/a.rs\0-\t-\tlogo.png\x000\t7\tdocs/\xe8\xaf\xb4\xe6\x98\x8e.md\0";
        let counts = parse_numstat_z(raw);
        assert_eq!(counts.get("src/a.rs"), Some(&(12, 3)));
        assert_eq!(counts.get("docs/说明.md"), Some(&(0, 7)));
        assert!(!counts.contains_key("logo.png"));
    }

    #[test]
    fn count_lines_matches_str_lines() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"a"), 1);
        assert_eq!(count_lines(b"a\nb\n"), 2);
        assert_eq!(count_lines(b"a\nb"), 2);
    }

    #[test]
    fn porcelain_z_maps_status_letters() {
        let raw = b" M src/a.rs\0M  src/b.rs\0A  src/c.rs\0D  src/d.rs\0?? src/new.rs\0";
        let files = parse_porcelain_z(raw);
        let got: Vec<(&str, &str)> = files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("src/a.rs", "M"),
                ("src/b.rs", "M"),
                ("src/c.rs", "A"),
                ("src/d.rs", "D"),
                ("src/new.rs", "U"),
            ]
        );
    }

    #[test]
    fn porcelain_z_skips_untracked_directory_entries() {
        // -uall 缺席或旧输出里的目录条目(尾斜杠):不是文件,不能进「更改」清单。
        let raw = b"?? target-check/\0?? src/new.rs\0";
        let files = parse_porcelain_z(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/new.rs");
    }

    #[test]
    fn diff_is_binary_matches_git_binary_marker() {
        // 修改中的二进制:git 的三行块(头 + index + Binary files … differ)。
        let modified = "diff --git a/x.png b/x.png\nindex abc..def 100644\nBinary files a/x.png and b/x.png differ\n";
        assert!(diff_is_binary(modified));
        // 新增二进制:比对一侧是 /dev/null。
        let added = "diff --git a/x.bin b/x.bin\nBinary files /dev/null and b/x.bin differ\n";
        assert!(diff_is_binary(added));
        // 正常文本 diff 不误判(即便正文里恰好有以 Binary files 开头的一行,也不以
        //  differ 收尾)。
        let text = "diff --git a/a.rs b/a.rs\n@@ -1 +1 @@\n-old\n+Binary files list differ here, ok\n";
        assert!(!diff_is_binary(text));
        assert!(!diff_is_binary(""));
    }

    #[test]
    fn porcelain_z_skips_short_entries_and_keeps_utf8_paths() {
        let raw = b"\0M  \0?? \xe6\x96\xb0\xe5\xbb\xba.txt\0";
        let files = parse_porcelain_z(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "新建.txt");
        assert_eq!(files[0].status, "U");
    }

    /// untracked 的 path 由前端透传，必须挡住 `..` 与绝对路径逃逸——两种目标都真实
    /// 存在，确保拒绝来自包含校验（「路径超出工作目录」）而不是「路径不存在」。
    #[test]
    fn untracked_file_diff_rejects_paths_outside_cwd() {
        let root = std::env::temp_dir().join(format!("meowo-git-escape-{}", std::process::id()));
        let inside = root.join("repo");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(root.join("outside.txt"), "secret").unwrap();
        std::fs::write(inside.join("ok.txt"), "hello").unwrap();
        let cwd = inside.to_str().unwrap();
        assert_eq!(
            untracked_file_diff(cwd, "../outside.txt").unwrap_err(),
            "路径超出工作目录"
        );
        // 绝对路径 join 会整个顶掉 cwd，是逃逸里最顺手的一种。
        let abs = root.join("outside.txt");
        assert_eq!(
            untracked_file_diff(cwd, abs.to_str().unwrap()).unwrap_err(),
            "路径超出工作目录"
        );
        // 仓库内的正常文件不受影响。
        let ok = untracked_file_diff(cwd, "ok.txt").unwrap();
        assert!(ok.diff.contains("+hello"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// GBK 源码文件（中文 Windows 常见）的 diff 输出必须解出中文，而不是整屏 U+FFFD。
    #[test]
    fn stdout_text_decodes_gbk_bytes() {
        let mut raw = b"+++ b/a.txt\n+".to_vec();
        raw.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]); // GBK 的「中文」
        raw.push(b'\n');
        let text = stdout_text(&raw);
        assert!(text.contains("中文"), "GBK diff 被解成了：{text}");
        // 纯 ASCII / UTF-8 输出照旧原样。
        assert_eq!(stdout_text("diff --git 中".as_bytes()), "diff --git 中");
    }

    /// 领先/落后按本地记录的上游分支算：本地多提交一个 = 领先 1；没上游 = None。
    #[test]
    fn upstream_divergence_counts_commits_against_tracking_branch() {
        let root = std::env::temp_dir().join(format!("meowo-git-ahead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let origin = root.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let run = |dir: &Path, args: &[&str]| {
            let output = git_output(dir.to_str().unwrap(), args).unwrap();
            assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        let commit = |dir: &Path, message: &str| {
            run(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-q", "-m", message]);
        };
        run(&origin, &["init", "-q"]);
        commit(&origin, "base");
        assert_eq!(upstream_divergence(origin.to_str().unwrap()), None);
        let clone = root.join("clone");
        run(&root, &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        assert_eq!(upstream_divergence(clone.to_str().unwrap()), Some((0, 0)));
        commit(&clone, "local");
        commit(&origin, "remote");
        // 没 fetch：远程新提交还看不到，只数出本地领先的那个。
        assert_eq!(upstream_divergence(clone.to_str().unwrap()), Some((1, 0)));
        run(&clone, &["fetch", "-q"]);
        assert_eq!(upstream_divergence(clone.to_str().unwrap()), Some((1, 1)));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 多仓库测试的小工具：在临时目录里跑 git、提交。
    struct Sandbox(std::path::PathBuf);
    impl Sandbox {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("meowo-multi-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Sandbox(root)
        }
        fn git(&self, dir: &Path, args: &[&str]) {
            let output = git_output(dir.to_str().unwrap(), args).unwrap();
            assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        }
        fn repo(&self, name: &str) -> std::path::PathBuf {
            let dir = self.0.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            self.git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("README.md"), "hi\n").unwrap();
            self.commit(&dir, "init");
            dir
        }
        fn commit(&self, dir: &Path, message: &str) {
            self.git(dir, &["add", "-A"]);
            self.git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", message]);
        }
    }
    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn s(path: &Path) -> String {
        norm_slashes(&path.to_string_lossy())
    }

    /// 会话开在父目录（不是仓库），下面两个项目：没动过文件时列有改动的那个；动过 a 就只列 a。
    #[test]
    fn overview_from_parent_dir_lists_changed_or_touched_repos() {
        let sb = Sandbox::new("parent");
        let a = sb.repo("a");
        let b = sb.repo("b");
        std::fs::write(b.join("x.txt"), "changed\n").unwrap();
        let parent = s(&sb.0);
        let repos = changes_overview_blocking(&parent, &[], &[]).unwrap();
        assert_eq!(repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(!repos[0].is_cwd_repo);
        let touched = [s(&a.join("README.md"))];
        let repos = changes_overview_blocking(&parent, &touched, &[]).unwrap();
        assert_eq!(repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["a"]);
    }

    /// 会话开在主仓库，agent 新开 worktree 去改：主仓库排第一，worktree 一组显示新分支。
    #[test]
    fn overview_includes_worktree_the_session_edited() {
        let sb = Sandbox::new("wt");
        let main = sb.repo("meowo");
        let wt = sb.0.join("meowo-feat");
        sb.git(&main, &["worktree", "add", "-q", "-b", "feat/x", wt.to_str().unwrap()]);
        std::fs::write(wt.join("new.rs"), "fn main() {}\n").unwrap();
        let repos = changes_overview_blocking(&s(&main), &[s(&wt.join("new.rs"))], &[]).unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos[0].is_cwd_repo);
        assert_eq!(repos[0].name, "meowo");
        assert_eq!(repos[1].name, "meowo-feat");
        assert_eq!(repos[1].summary.branch.as_deref(), Some("feat/x"));
        assert_eq!(repos[1].summary.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["new.rs"]);
        // 相关仓库（手机远程页的放行清单）同样认得这个 worktree。
        assert!(is_related_root(&s(&main), &s(&wt)));
        assert!(!is_related_root(&s(&main), &s(&sb.0)));
    }

    /// Claude Code 默认把 worktree 建在仓库里面（.claude/worktrees/<名字>）：也要单独认成一组。
    #[test]
    fn overview_recognizes_worktree_nested_inside_the_repo() {
        let sb = Sandbox::new("nested");
        let main = sb.repo("main");
        std::fs::write(main.join(".gitignore"), ".claude/worktrees/\n").unwrap();
        sb.commit(&main, "ignore");
        let wt = main.join(".claude").join("worktrees").join("feat");
        sb.git(&main, &["worktree", "add", "-q", "-b", "feat/x", wt.to_str().unwrap()]);
        std::fs::write(wt.join("a.ts"), "x\n").unwrap();
        let repos = changes_overview_blocking(&s(&main), &[s(&wt.join("a.ts"))], &[]).unwrap();
        assert_eq!(repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["main", "feat"]);
        assert_eq!(repos[1].summary.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["a.ts"]);
    }

    /// 文件名里的方括号不当通配符。
    #[test]
    fn file_diff_treats_brackets_literally() {
        let sb = Sandbox::new("brackets");
        let repo = sb.repo("r");
        for dir in ["[id]", "i", "d"] {
            std::fs::create_dir_all(repo.join("app").join(dir)).unwrap();
            std::fs::write(repo.join("app").join(dir).join("page.tsx"), "a\n").unwrap();
        }
        sb.commit(&repo, "pages");
        for dir in ["[id]", "i", "d"] {
            std::fs::write(repo.join("app").join(dir).join("page.tsx"), "b\n").unwrap();
        }
        let diff = git_file_diff_blocking(&s(&repo), "app/[id]/page.tsx", false, None).unwrap().diff;
        assert_eq!(diff.matches("diff --git").count(), 1, "{diff}");
    }

    /// 已提交的进「已提交未合并」、没提交的进「未提交」；在默认分支上没有已提交段。
    #[test]
    fn committed_and_uncommitted_are_separate_sections() {
        let sb = Sandbox::new("sections");
        let repo = sb.repo("r");
        let on_main = repo_changes(&s(&repo), true).unwrap();
        assert_eq!(on_main.base, None);
        assert!(on_main.committed.is_empty());
        sb.git(&repo, &["checkout", "-q", "-b", "feat/y"]);
        std::fs::write(repo.join("done.txt"), "a\nb\n").unwrap();
        sb.commit(&repo, "feat");
        std::fs::write(repo.join("wip.txt"), "wip\n").unwrap();
        let changes = repo_changes(&s(&repo), true).unwrap();
        assert_eq!(changes.base_label.as_deref(), Some("main"));
        assert!(changes.base.as_deref().is_some_and(is_commit_sha));
        assert_eq!(changes.committed.iter().map(|f| (f.path.as_str(), f.status.as_str(), f.additions)).collect::<Vec<_>>(), [("done.txt", "A", Some(2))]);
        assert_eq!(changes.summary.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["wip.txt"]);
    }

    /// 已提交段的单文件 diff 看 base..HEAD；base 不是 SHA 一律拒绝（会拼进 git 参数）。
    #[test]
    fn file_diff_with_base_shows_committed_change() {
        let sb = Sandbox::new("base");
        let repo = sb.repo("r");
        sb.git(&repo, &["checkout", "-q", "-b", "feat/z"]);
        std::fs::write(repo.join("README.md"), "hi\nmore\n").unwrap();
        sb.commit(&repo, "edit");
        let base = repo_changes(&s(&repo), true).unwrap().base.unwrap();
        let diff = git_file_diff_blocking(&s(&repo), "README.md", false, Some(&base)).unwrap();
        assert!(diff.diff.contains("+more"), "{}", diff.diff);
        // 没 base 时这份改动已经提交，相对 HEAD 是空的。
        assert!(git_file_diff_blocking(&s(&repo), "README.md", false, None).unwrap().diff.is_empty());
        assert_eq!(git_file_diff_blocking(&s(&repo), "README.md", false, Some("--output=x")).unwrap_err(), "git/bad_base");
    }

    // ─── 写操作 ───

    /// 测试里的 git 写操作和提交都隔离掉用户的全局 / 系统配置（钩子、签名、身份），不读也不动真实配置。
    fn isolated_env(sb: &Sandbox, identity: bool) -> Vec<(&'static str, Option<String>)> {
        let empty = sb.0.join("empty.gitconfig");
        std::fs::write(&empty, "").unwrap();
        let mut env: Vec<(&'static str, Option<String>)> = vec![
            ("GIT_CONFIG_GLOBAL", Some(s(&empty))),
            ("GIT_CONFIG_NOSYSTEM", Some("1".into())),
            ("HOME", Some(s(&sb.0))),
            ("USERPROFILE", Some(s(&sb.0))),
            ("EMAIL", None),
        ];
        for key in ["GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL"] {
            env.push((key, identity.then(|| if key.ends_with("EMAIL") { "t@t".into() } else { "t".into() })));
        }
        env
    }

    fn commit_isolated(sb: &Sandbox, dir: &Path, message: &str, all: bool, identity: bool) -> Result<String, String> {
        let env = isolated_env(sb, identity);
        let env: Vec<(&str, Option<&str>)> = env.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        commit_blocking(&s(dir), message, all, None, &env)
    }

    fn statuses(dir: &Path) -> Vec<(String, Option<String>, Option<String>)> {
        let output = git_output(&s(dir), &["status", "--porcelain", "-z", "--no-renames", "-uall"]).unwrap();
        parse_porcelain_z(&output.stdout)
            .into_iter()
            .map(|f| (f.path, f.index_status, f.worktree_status))
            .collect()
    }

    fn st(path: &str, index: Option<&str>, worktree: Option<&str>) -> (String, Option<String>, Option<String>) {
        (path.into(), index.map(Into::into), worktree.map(Into::into))
    }

    #[test]
    fn porcelain_z_splits_index_and_worktree_status() {
        let raw = b"MM both.rs\0M  staged.rs\0 M wip.rs\0?? new.rs\0A  added.rs\0 D gone.rs\0UU conflict.rs\0";
        let got: Vec<_> = parse_porcelain_z(raw)
            .into_iter()
            .map(|f| (f.path, f.status, f.index_status, f.worktree_status))
            .collect();
        let row = |p: &str, s: &str, i: Option<&str>, w: Option<&str>| (p.to_string(), s.to_string(), i.map(Into::into), w.map(Into::into));
        assert_eq!(
            got,
            vec![
                row("both.rs", "M", Some("M"), Some("M")),
                row("staged.rs", "M", Some("M"), None),
                row("wip.rs", "M", None, Some("M")),
                row("new.rs", "U", None, Some("U")),
                row("added.rs", "A", Some("A"), None),
                row("gone.rs", "D", None, Some("D")),
                // 冲突文件不能记成未跟踪的 "U"（放弃会按未跟踪删文件）。
                row("conflict.rs", "U", None, Some("!")),
            ]
        );
    }

    /// 真仓库里部分暂存：暂存一版、再改一版，两个字段都有值。
    #[test]
    fn partially_staged_file_has_both_statuses() {
        let sb = Sandbox::new("partial");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "one\n").unwrap();
        stage_blocking(&s(&repo), &["README.md".into()]).unwrap();
        std::fs::write(repo.join("README.md"), "two\n").unwrap();
        let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
        let file = &summary.files[0];
        assert_eq!((file.index_status.as_deref(), file.worktree_status.as_deref()), (Some("M"), Some("M")));
    }

    #[test]
    fn stage_unstage_and_discard_round_trip() {
        let sb = Sandbox::new("stage");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        std::fs::create_dir_all(repo.join("dir")).unwrap();
        std::fs::write(repo.join("dir").join("new.txt"), "n\n").unwrap();
        let dir = s(&repo);
        stage_blocking(&dir, &["README.md".into(), "dir/new.txt".into()]).unwrap();
        assert_eq!(statuses(&repo), vec![st("README.md", Some("M"), None), st("dir/new.txt", Some("A"), None)]);
        // 已暂存、没有工作区改动的文件不能再「暂存」，也不能「放弃」（清单里它只在暂存组）。
        assert_eq!(stage_blocking(&dir, &["README.md".into()]).unwrap_err(), "git/not_in_changes");
        assert_eq!(discard_blocking(&dir, &["README.md".into()]).unwrap_err(), "git/not_in_changes");
        unstage_blocking(&dir, &["README.md".into(), "dir/new.txt".into()]).unwrap();
        assert_eq!(statuses(&repo), vec![st("README.md", None, Some("M")), st("dir/new.txt", None, Some("U"))]);
        // 放弃：已跟踪的恢复，未跟踪的挪走（目录留着）。用假回收站，单测不往本机回收站塞东西。
        discard_with(&dir, &["README.md".into(), "dir/new.txt".into()], fake_trash_ok).unwrap();
        assert!(statuses(&repo).is_empty());
        assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap().trim(), "hi");
        assert!(!repo.join("dir").join("new.txt").exists());
        assert!(repo.join("dir").is_dir());
    }

    /// 放弃只丢工作区那一层：部分暂存的文件放弃后回到暂存区的内容。
    #[test]
    fn discard_keeps_staged_content() {
        let sb = Sandbox::new("discard-staged");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "staged\n").unwrap();
        stage_blocking(&s(&repo), &["README.md".into()]).unwrap();
        std::fs::write(repo.join("README.md"), "later\n").unwrap();
        discard_blocking(&s(&repo), &["README.md".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap().replace("\r\n", "\n"), "staged\n");
        assert_eq!(statuses(&repo), vec![st("README.md", Some("M"), None)]);
    }

    /// 假回收站：直接删，模拟「回收成功、原路径消失」。
    fn fake_trash_ok(paths: &[std::path::PathBuf]) -> Result<(), String> {
        for path in paths {
            // 整目录放弃交来的是目录本身。
            let removed = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
            removed.map_err(|e| format!("git/trash_failed: {e}"))?;
        }
        Ok(())
    }
    /// 假回收站：回收失败（没回收站的盘、COM 起不来……）。
    fn fake_trash_fail(_: &[std::path::PathBuf]) -> Result<(), String> {
        Err("git/trash_failed: injected".into())
    }

    /// 回收失败时报 git/trash_failed，文件原样保留，不退回永久删除。
    #[test]
    fn discard_keeps_untracked_file_when_trash_fails() {
        let sb = Sandbox::new("discard-trash-fail");
        let repo = sb.repo("r");
        std::fs::write(repo.join("new.txt"), "keep me
").unwrap();
        let err = discard_with(&s(&repo), &["new.txt".into()], fake_trash_fail).unwrap_err();
        assert!(err.starts_with("git/trash_failed"), "{err}");
        assert_eq!(std::fs::read_to_string(repo.join("new.txt")).unwrap(), "keep me
");
        assert_eq!(statuses(&repo), vec![st("new.txt", None, Some("U"))]);
    }

    /// 回收成功后原路径不存在。
    #[test]
    fn discard_trashes_untracked_file() {
        let sb = Sandbox::new("discard-trash-ok");
        let repo = sb.repo("r");
        std::fs::write(repo.join("new.txt"), "n
").unwrap();
        discard_with(&s(&repo), &["new.txt".into()], fake_trash_ok).unwrap();
        assert!(std::fs::symlink_metadata(repo.join("new.txt")).is_err());
        assert!(statuses(&repo).is_empty());
    }

    /// 没有未跟踪文件要回收时不调回收站：只放弃已跟踪文件的改动，注入必败回收站也得成功。
    /// 整批化后曾对空清单照调回收站，CI 的 Linux/macOS 上符号链接用例因此挂掉；本机 Windows
    /// 建不了链接，这条用例在任何平台都能钉住同一个缺陷。
    #[test]
    fn discard_tracked_only_never_touches_recycle_bin() {
        let sb = Sandbox::new("discard-no-trash");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "changed\n").unwrap();
        discard_with(&s(&repo), &["README.md".into()], fake_trash_fail).unwrap();
        assert!(statuses(&repo).is_empty());
    }

    /// 放弃多个未跟踪文件只调一次回收站（整批）：逐个调时上千个文件要串行上千轮 COM + IFileOperation。
    #[test]
    fn discard_trashes_untracked_files_in_one_batch() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CALLS: AtomicUsize = AtomicUsize::new(0);
        static FILES: AtomicUsize = AtomicUsize::new(0);
        fn counting_trash(paths: &[std::path::PathBuf]) -> Result<(), String> {
            CALLS.fetch_add(1, Ordering::SeqCst);
            FILES.fetch_add(paths.len(), Ordering::SeqCst);
            fake_trash_ok(paths)
        }
        let sb = Sandbox::new("discard-trash-batch");
        let repo = sb.repo("r");
        let names = ["a.txt", "b.txt", "sub/c.txt"];
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        for name in names {
            std::fs::write(repo.join(name), "n\n").unwrap();
        }
        let paths: Vec<String> = names.iter().map(|n| n.to_string()).collect();
        discard_with(&s(&repo), &paths, counting_trash).unwrap();
        assert_eq!(CALLS.load(Ordering::SeqCst), 1, "应整批调一次回收站");
        assert_eq!(FILES.load(Ordering::SeqCst), 3);
        assert!(statuses(&repo).is_empty());
    }

    /// 未跟踪的符号链接不走回收站，只删链接本身，指向的文件不动。
    /// 注入必败的回收站：链接若误走回收站这里就会报错。Windows 无开发者模式时建不了链接，跳过。
    #[test]
    fn discard_untracked_symlink_removes_link_only() {
        let sb = Sandbox::new("discard-symlink");
        let repo = sb.repo("r");
        let target = sb.0.join("target.txt");
        std::fs::write(&target, "precious
").unwrap();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, repo.join("link.txt")).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target, repo.join("link.txt")).is_ok();
        if !made {
            eprintln!("建不了符号链接（Windows 未开开发者模式），跳过");
            return;
        }
        discard_with(&s(&repo), &["link.txt".into()], fake_trash_fail).unwrap();
        assert!(std::fs::symlink_metadata(repo.join("link.txt")).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "precious
");
    }

    /// 真回收站：放弃的未跟踪文件必须能在回收站里找回，而不是永久删掉。
    /// ignored：会真往本机回收站放东西（测完按原路径 purge 掉自己那条），CI 无桌面回收站。
    /// macOS 的 trash crate 不提供 list/purge，无法自检，只在 Windows/Linux 跑。
    #[cfg(any(windows, all(unix, not(target_os = "macos"))))]
    #[test]
    #[ignore]
    fn discard_moves_untracked_to_real_recycle_bin() {
        let sb = Sandbox::new("discard-trash");
        let repo = sb.repo("r");
        let name = format!("meowo-trash-probe-{}.txt", std::process::id());
        std::fs::write(repo.join(&name), "probe\n").unwrap();
        discard_blocking(&s(&repo), std::slice::from_ref(&name)).unwrap();
        assert!(!repo.join(&name).exists());
        // 调用线程的 COM apartment 不能被 trash 染成 STA：同一线程池里 focus_session 的
        // UIAutomation 要 MTA。必须放在下面 os_limited::list 之前——那两个调用本身也会染线程。
        #[cfg(windows)]
        assert!(
            uiautomation::UIAutomation::new().is_ok(),
            "放弃更改之后，同一线程上 UIAutomation 起不来（trash 把线程染成了 STA）"
        );
        let parent = dunce::canonicalize(&repo).unwrap();
        // 查回收站也要换线程：上面 UIAutomation 已把本线程初始化成 MTA，trash 在这里要 STA 会 panic。
        let found = std::thread::spawn(move || {
            let ours: Vec<_> = trash::os_limited::list()
                .unwrap()
                .into_iter()
                .filter(|item| item.name == name.as_str() && dunce::simplified(&item.original_parent) == parent)
                .collect();
            let found = !ours.is_empty();
            let _ = trash::os_limited::purge_all(ours);
            found
        })
        .join()
        .unwrap();
        assert!(found, "放弃的未跟踪文件没进回收站（被永久删除了）");
    }

    /// 清单外的路径一律拒，包括 `..`、绝对路径、干净的已跟踪文件——放弃不能被借去删任意文件。
    #[test]
    fn write_ops_reject_paths_outside_change_list() {
        let sb = Sandbox::new("reject");
        let repo = sb.repo("r");
        std::fs::write(sb.0.join("outside.txt"), "keep\n").unwrap();
        let dir = s(&repo);
        let abs = s(&sb.0.join("outside.txt"));
        for path in ["README.md", "../outside.txt", abs.as_str(), "nope.txt", ""] {
            assert_eq!(discard_blocking(&dir, &[path.into()]).unwrap_err(), "git/not_in_changes", "{path}");
            assert_eq!(stage_blocking(&dir, &[path.into()]).unwrap_err(), "git/not_in_changes", "{path}");
            assert_eq!(unstage_blocking(&dir, &[path.into()]).unwrap_err(), "git/not_in_changes", "{path}");
        }
        assert!(sb.0.join("outside.txt").exists());
        assert!(repo.join("README.md").exists());
    }

    /// 还没有任何提交的仓库：取消暂存走 rm --cached，文件留在工作区。
    #[test]
    fn unstage_works_before_first_commit() {
        let sb = Sandbox::new("unborn");
        let repo = sb.0.join("r");
        std::fs::create_dir_all(&repo).unwrap();
        sb.git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        stage_blocking(&s(&repo), &["a.txt".into()]).unwrap();
        assert_eq!(statuses(&repo), vec![st("a.txt", Some("A"), None)]);
        unstage_blocking(&s(&repo), &["a.txt".into()]).unwrap();
        assert_eq!(statuses(&repo), vec![st("a.txt", None, Some("U"))]);
        assert!(repo.join("a.txt").exists());
    }

    #[test]
    fn commit_staged_and_commit_all() {
        let sb = Sandbox::new("commit");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        std::fs::write(repo.join("new.txt"), "n\n").unwrap();
        assert_eq!(commit_isolated(&sb, &repo, "  \n ", false, true).unwrap_err(), "git/empty_message");
        // 没暂存、不是「全部」：没东西可提交。
        assert_eq!(commit_isolated(&sb, &repo, "x", false, true).unwrap_err(), "git/nothing_to_commit");
        // 只提交暂存的那一个，未跟踪的留着。
        stage_blocking(&s(&repo), &["README.md".into()]).unwrap();
        let sha = commit_isolated(&sb, &repo, "第一行\n\n第二段 \"引号\"", false, true).unwrap();
        assert!(is_commit_sha(&sha), "{sha}");
        assert_eq!(statuses(&repo), vec![st("new.txt", None, Some("U"))]);
        let log = git_output(&s(&repo), &["log", "-1", "--format=%B"]).unwrap();
        assert_eq!(stdout_text(&log.stdout).trim(), "第一行\n\n第二段 \"引号\"");
        // 提交全部：未跟踪的也进去。
        commit_isolated(&sb, &repo, "all", true, true).unwrap();
        assert!(statuses(&repo).is_empty());
        assert_eq!(commit_isolated(&sb, &repo, "again", true, true).unwrap_err(), "git/nothing_to_commit");
    }

    #[test]
    fn commit_without_identity_reports_identity() {
        let sb = Sandbox::new("ident");
        let repo = sb.repo("r");
        // useConfigOnly：不许 git 拿系统用户名 / 主机名凑一个身份（CI 机器的主机名可能带域名，凑得出来）。
        sb.git(&repo, &["config", "user.useConfigOnly", "true"]);
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        assert_eq!(commit_isolated(&sb, &repo, "x", true, false).unwrap_err(), "git/identity");
    }

    /// 做空复现：「提交全部」先暂存再提交，提交没成时暂存不能留下（用户没做过这个暂存）。
    #[test]
    fn failed_commit_all_rolls_back_staging() {
        let sb = Sandbox::new("rollback");
        let repo = sb.repo("r");
        sb.git(&repo, &["config", "user.useConfigOnly", "true"]);
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        std::fs::write(repo.join("new.txt"), "n\n").unwrap();
        let before = statuses(&repo);
        assert_eq!(commit_isolated(&sb, &repo, "x", true, false).unwrap_err(), "git/identity");
        assert_eq!(statuses(&repo), before);
    }

    /// 做空复现：merge 到一半有冲突时，「提交全部」把带冲突标记的内容当成已解决提交了。
    #[test]
    fn commit_refuses_unmerged_conflicts() {
        let sb = Sandbox::new("conflict");
        let repo = sb.repo("r");
        let env = isolated_env(&sb, true);
        let env: Vec<(&str, Option<&str>)> = env.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        let git = |args: &[&str]| git_run(&s(&repo), args, &env, None).unwrap();
        git(&["checkout", "-q", "-b", "other"]);
        std::fs::write(repo.join("README.md"), "other\n").unwrap();
        git(&["commit", "-qam", "other"]);
        git(&["checkout", "-q", "-"]);
        std::fs::write(repo.join("README.md"), "mine\n").unwrap();
        git(&["commit", "-qam", "mine"]);
        assert!(!git(&["merge", "-q", "other"]).status.success());
        assert_eq!(commit_isolated(&sb, &repo, "x", true, true).unwrap_err(), "git/conflict");
        assert_eq!(commit_isolated(&sb, &repo, "x", false, true).unwrap_err(), "git/conflict");
        // 做空第 2 轮复现：先点「暂存」，git 就当冲突已解决，之后的提交检查拦不住。暂存 / 放弃都要拒。
        assert_eq!(statuses(&repo), vec![st("README.md", None, Some("!"))]);
        assert_eq!(stage_blocking(&s(&repo), &["README.md".into()]).unwrap_err(), "git/conflict");
        assert_eq!(discard_blocking(&s(&repo), &["README.md".into()]).unwrap_err(), "git/conflict");
        assert_eq!(commit_isolated(&sb, &repo, "x", false, true).unwrap_err(), "git/conflict");
    }

    /// 做空第 2 轮：「提交全部」按界面上显示的文件收口，列表外（agent 刚写出、还没轮询到）的不进提交。
    #[test]
    fn commit_all_only_commits_shown_files() {
        let sb = Sandbox::new("shown");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        std::fs::write(repo.join("late.log"), "x\n").unwrap();
        let env = isolated_env(&sb, true);
        let env: Vec<(&str, Option<&str>)> = env.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        commit_blocking(&s(&repo), "shown", true, Some(&["README.md".to_string()]), &env).unwrap();
        assert_eq!(statuses(&repo), vec![st("late.log", None, Some("U"))]);
        // 列表里的文件已经不在改动清单里（被别人提交了）：拒绝，前端刷新后再来。
        assert_eq!(
            commit_blocking(&s(&repo), "x", true, Some(&["README.md".to_string()]), &env).unwrap_err(),
            "git/not_in_changes"
        );
    }

    /// 做空复现：未跟踪的嵌套仓库（Claude Code 的 .claude/worktrees/x）列表里不显示，
    /// `add -A` 却会把它当 gitlink 提交。「提交全部」只提交列表里的文件。
    #[test]
    fn commit_all_skips_untracked_nested_repo() {
        let sb = Sandbox::new("nested_commit");
        let repo = sb.repo("r");
        let nested = repo.join(".claude").join("worktrees").join("feat");
        std::fs::create_dir_all(&nested).unwrap();
        sb.git(&nested, &["init", "-q"]);
        std::fs::write(nested.join("x.txt"), "x\n").unwrap();
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        commit_isolated(&sb, &repo, "all", true, true).unwrap();
        let tree = git_output(&s(&repo), &["ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
        let tree = stdout_text(&tree.stdout);
        assert!(!tree.contains(".claude"), "{tree}");
        assert!(tree.contains("README.md"));
    }

    /// agent 正在跑 git（index.lock 在）：暂存 / 提交都报 git/locked，不是一串原始报错。
    #[test]
    fn index_lock_reports_locked() {
        let sb = Sandbox::new("lock");
        let repo = sb.repo("r");
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        std::fs::write(repo.join(".git").join("index.lock"), "").unwrap();
        assert_eq!(stage_blocking(&s(&repo), &["README.md".into()]).unwrap_err(), "git/locked");
        assert_eq!(commit_isolated(&sb, &repo, "x", true, true).unwrap_err(), "git/locked");
        std::fs::remove_file(repo.join(".git").join("index.lock")).unwrap();
    }

    /// 提交钩子失败：git/failed 带出钩子的输出。
    #[test]
    fn failing_hook_is_reported_with_output() {
        let sb = Sandbox::new("hook");
        let repo = sb.repo("r");
        let hook = repo.join(".git").join("hooks").join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\necho lint broke >&2\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        let err = commit_isolated(&sb, &repo, "x", true, true).unwrap_err();
        assert!(err.starts_with("git/failed: ") && err.contains("lint broke"), "{err}");
    }

    /// 命令里提到的 worktree 上榜：主仓库 + 两个 worktree，只有命令里出现过的 wt1 进清单；
    /// `wt1x` 这个前缀相同的名字不误命中。
    #[test]
    fn overview_lists_worktrees_mentioned_in_commands() {
        let sb = Sandbox::new("cmds");
        let main_dir = sb.repo("main");
        for name in ["wt1", "wt2", "wt1x"] {
            let path = sb.0.join(name);
            sb.git(&main_dir, &["worktree", "add", "-q", "-b", name, path.to_str().unwrap()]);
        }
        // 用 git 自己认的根当 cwd 和命令里的路径（macOS 临时目录有 /private 软链，两种写法不一样）。
        let main = repo_root(&s(&main_dir)).unwrap();
        let wt1 = repo_root(&s(&sb.0.join("wt1"))).unwrap();
        let names = |commands: &[&str]| -> Vec<String> {
            let commands: Vec<String> = commands.iter().map(|c| c.to_string()).collect();
            changes_overview_blocking(&main, &[], &commands).unwrap().into_iter().map(|r| r.name).collect()
        };
        assert_eq!(names(&[]), ["main"]);
        assert_eq!(names(&["cargo test"]), ["main"]);
        let back = wt1.replace('/', "\\");
        assert_eq!(names(&[&format!("cd {wt1} && cargo test")]), ["main", "wt1"]);
        assert_eq!(names(&[&format!("cd \"{back}\"; ls")]), ["main", "wt1"]);
        assert_eq!(names(&["git -C ../wt1 status"]), ["main", "wt1"]);
        assert_eq!(names(&["cat ../wt1/src/a.rs"]), ["main", "wt1"]);
        // 只提到 wt1x：wt1 不上榜，wt1x 上榜。
        assert_eq!(names(&["cd ../wt1x && ls"]), ["main", "wt1x"]);
        assert_eq!(names(&["echo ../wt1-backup ../wt12"]), ["main"]);
        // 命令识别的顺序：先出现的先列。
        assert_eq!(names(&["cd ../wt2", "cd ../wt1"]), ["main", "wt2", "wt1"]);
        if cfg!(target_os = "windows") {
            // Git Bash 写法：C:/Users/... → /c/Users/...（大小写不敏感）。
            let bash = format!("/{}{}", wt1[..1].to_ascii_lowercase(), &wt1[2..]);
            assert_eq!(names(&[&format!("cd {bash}")]), ["main", "wt1"]);
            assert_eq!(names(&[&format!("cd {}", wt1.to_uppercase())]), ["main", "wt1"]);
        }
    }

    /// 仓库里面的 worktree（.claude/worktrees/x）用相对 cwd 的写法提到也认。
    #[test]
    fn overview_recognizes_relative_nested_worktree_in_commands() {
        let sb = Sandbox::new("cmds-nested");
        let main_dir = sb.repo("main");
        std::fs::write(main_dir.join(".gitignore"), ".claude/\n").unwrap();
        sb.commit(&main_dir, "ignore");
        let wt = main_dir.join(".claude").join("worktrees").join("x");
        sb.git(&main_dir, &["worktree", "add", "-q", "-b", "x", wt.to_str().unwrap()]);
        let main = repo_root(&s(&main_dir)).unwrap();
        let names = |command: &str| -> Vec<String> {
            changes_overview_blocking(&main, &[], &[command.to_string()]).unwrap().into_iter().map(|r| r.name).collect()
        };
        assert_eq!(names("cd .claude/worktrees/x && git status"), ["main", "x"]);
        assert_eq!(names("cd ./.claude/worktrees/x"), ["main", "x"]);
        assert_eq!(names("ls .claude/worktrees"), ["main"]);
    }

    /// 前后端分仓的大项目（用户 2026-11-09）：主目录不是仓库，frontend / backend 在下一层，apps/web 在
    /// 下两层，frontend 里还有 agent 开的 worktree。只用命令干活也要认出来；裸目录名当普通词时不误认。
    #[test]
    fn overview_in_multi_repo_project_dir() {
        let sb = Sandbox::new("bigproj");
        let proj = sb.0.join("bigproj");
        std::fs::create_dir_all(proj.join("apps")).unwrap();
        let mk = |rel: &str| {
            let dir = proj.join(rel);
            std::fs::create_dir_all(&dir).unwrap();
            sb.git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("README.md"), "hi\n").unwrap();
            sb.commit(&dir, "init");
            dir
        };
        let frontend = mk("frontend");
        mk("backend");
        mk("apps/web");
        // node_modules 里带 .git 的第三方包不当候选。
        mk("node_modules/pkg");
        std::fs::write(frontend.join(".gitignore"), ".claude/\n").unwrap();
        sb.commit(&frontend, "ignore");
        let wt = frontend.join(".claude").join("worktrees").join("x");
        sb.git(&frontend, &["worktree", "add", "-q", "-b", "x", wt.to_str().unwrap()]);
        let cwd = s(&proj);
        let names = |command: &str| -> Vec<String> {
            let mut got: Vec<String> =
                changes_overview_blocking(&cwd, &[], &[command.to_string()]).unwrap().into_iter().map(|r| r.name).collect();
            got.sort();
            got
        };
        let none: Vec<String> = Vec::new();
        assert_eq!(names("cd apps/web && npm test"), ["web"]);
        assert_eq!(names("cd frontend/.claude/worktrees/x && git status"), ["x"]);
        assert_eq!(names("git -C backend status"), ["backend"]);
        assert_eq!(names("cat backend/src/a.rs"), ["backend"]);
        assert_eq!(names("cd backend"), ["backend"]);
        assert_eq!(names("pushd frontend/ && ls"), ["frontend"]);
        // 裸目录名当普通词：不认。都没认出来时走「下一层有改动的仓库」兜底，这几个仓库都干净，列表为空。
        assert_eq!(names("git commit -m \"fix backend\""), none);
        assert_eq!(names("npm run backend"), none);
        assert_eq!(names("ls frontend"), none);
        assert_eq!(names("cd node_modules/pkg"), none);
        // 手机端放行范围：两层仓库、子仓库的 worktree 都在，node_modules 里的不在。
        // 走产品里同一个判定（is_related_root 会对齐软链 / 8.3 短名两种写法；逐字比在 macOS CI 上对不上）。
        let related = related_roots(&cwd);
        let has = |p: &Path| is_related_root(&cwd, &s(p));
        assert!(has(&proj.join("apps").join("web")), "{related:?}");
        assert!(has(&wt), "{related:?}");
        assert!(!has(&proj.join("node_modules").join("pkg")), "{related:?}");
    }

    /// 用户 2026-11-09：主目录下有改动的仓库多于 8 个时，此前只列前 8 个，后面的看不到。不设上限。
    #[test]
    fn overview_fallback_lists_every_dirty_child_repo() {
        let sb = Sandbox::new("many");
        let ws = sb.0.join("ws");
        for i in 0..11 {
            let dir = ws.join(format!("p{i:02}"));
            std::fs::create_dir_all(&dir).unwrap();
            sb.git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("a.txt"), "x
").unwrap();
        }
        let repos = changes_overview_blocking(&s(&ws), &[], &[]).unwrap();
        assert_eq!(repos.len(), 11);
    }

    /// CI 复现：会话目录和 git 记录的写法不同（Windows 8.3 短名、macOS /var 软链），worktree 认不出、
    /// 手机端放行判定也不对。这里用目录联接给项目起个别名，会话开在别名上。
    #[test]
    fn related_roots_match_through_path_aliases() {
        if !cfg!(windows) {
            return;
        }
        let sb = Sandbox::new("alias");
        let real = sb.0.join("real");
        let frontend = real.join("frontend");
        std::fs::create_dir_all(&frontend).unwrap();
        sb.git(&frontend, &["init", "-q", "-b", "main"]);
        std::fs::write(frontend.join("README.md"), "hi
").unwrap();
        sb.commit(&frontend, "init");
        std::fs::write(frontend.join(".gitignore"), ".claude/
").unwrap();
        sb.commit(&frontend, "ignore");
        let wt = frontend.join(".claude").join("worktrees").join("x");
        sb.git(&frontend, &["worktree", "add", "-q", "-b", "x", wt.to_str().unwrap()]);
        let alias = sb.0.join("alias");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", alias.to_str().unwrap(), real.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
        let cwd = s(&alias);
        let names = |command: &str| -> Vec<String> {
            changes_overview_blocking(&cwd, &[], &[command.to_string()]).unwrap().into_iter().map(|r| r.name).collect()
        };
        assert_eq!(names("cd frontend/.claude/worktrees/x && git status"), ["x"]);
        assert_eq!(names(&format!("cd {}/frontend/.claude/worktrees/x", cwd)), ["x"]);
        assert!(is_related_root(&cwd, &s(&alias.join("frontend").join(".claude").join("worktrees").join("x"))));
        assert!(is_related_root(&cwd, &s(&wt)));
        // 只拆联接，不进去删（联接里是真目录）。
        std::fs::remove_dir(&alias).unwrap();
    }

    /// 用户 2026-11-09：先显示目录，具体内容点击后异步。只有点名（或默认）的那个仓库算行数和已提交段。
    #[test]
    fn overview_details_only_the_selected_repo() {
        let sb = Sandbox::new("detail");
        let ws = sb.0.join("ws");
        for name in ["a", "b", "c"] {
            let dir = ws.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            sb.git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("x.txt"), "1\n2\n").unwrap();
        }
        let cwd = s(&ws);
        let shape = |detail: Option<&str>| -> Vec<(String, bool, bool)> {
            changes_overview_detail(&cwd, &[], &[], detail)
                .unwrap()
                .into_iter()
                .map(|r| (r.name, r.detailed, r.summary.files[0].additions.is_some()))
                .collect()
        };
        let row = |n: &str, d: bool| (n.to_string(), d, d);
        // 不带 detail（旧前端）：全算。
        assert_eq!(shape(None), [row("a", true), row("b", true), row("c", true)]);
        // 空串：默认第一个（工作目录不是仓库）。
        assert_eq!(shape(Some("")), [row("a", true), row("b", false), row("c", false)]);
        // 点名 b：只有 b 算，其余照样列出（分支、文件清单都在）。
        let b = s(&ws.join("b"));
        assert_eq!(shape(Some(&b)), [row("a", false), row("b", true), row("c", false)]);
        // 点名的已不在清单里：退回默认。
        assert_eq!(shape(Some("C:/gone")), [row("a", true), row("b", false), row("c", false)]);
    }

    #[test]
    fn path_spellings_cover_relative_and_git_bash_forms() {
        let forms = path_spellings("c:/w/meowo-wt", "c:/w/meowo");
        assert!(forms.contains(&"/c/w/meowo-wt".to_string()), "{forms:?}");
        assert!(forms.contains(&"../meowo-wt".to_string()), "{forms:?}");
        let forms = path_spellings("c:/w/meowo/.claude/worktrees/x", "c:/w/meowo");
        assert!(forms.contains(&".claude/worktrees/x".to_string()) && forms.contains(&"./.claude/worktrees/x".to_string()));
        // 不同盘没有相对写法。
        assert_eq!(path_spellings("d:/x", "c:/w").len(), 2);
        // Unix 只共享根 `/` 不给相对写法。
        assert_eq!(path_spellings("/opt/x", "/home/me"), ["/opt/x"]);
    }

    #[test]
    fn truncate_utf8_respects_char_boundaries() {
        let (text, truncated) = truncate_utf8("abc".repeat(4), 5);
        assert_eq!(text, "abcab");
        assert!(truncated);
        let (text, truncated) = truncate_utf8("中文测试".into(), 4);
        assert_eq!(text, "中");
        assert!(truncated);
        let (text, truncated) = truncate_utf8("short".into(), 100);
        assert_eq!(text, "short");
        assert!(!truncated);
    }

    // ─── 未跟踪目录的折叠（用户实测：没进 .gitignore 的构建目录 15612 个文件，「更改」面板卡死）───

    /// 在 dir 下写 n 个文件，每 10 个放进一层子目录（构建目录的样子：文件散在多层里）。
    fn fill(dir: &Path, n: usize) {
        for i in 0..n {
            let sub = dir.join(format!("d{}", i / 10)).join("deep");
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(format!("f{i}.js")), "x\n").unwrap();
        }
    }

    fn folded_of(files: &[GitChangedFileDto]) -> Vec<(String, u32)> {
        files.iter().filter_map(|f| f.untracked_files.map(|n| (f.path.clone(), n))).collect()
    }

    /// 摘要里：多于 50 个文件的未跟踪目录折成一条（带文件数），不多于 50 个的照旧逐个文件；
    /// 目录名带空格、中文、嵌在已跟踪目录里都一样；只含嵌套仓库的目录不出现；被忽略的文件不算数。
    #[test]
    fn summary_folds_large_untracked_dirs_and_expands_small_ones() {
        let sb = Sandbox::new("fold-summary");
        let repo = sb.repo("r");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src").join("lib.rs"), "\n").unwrap();
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        sb.commit(&repo, "src");
        fill(&repo.join("big dir"), 60);
        std::fs::write(repo.join("big dir").join("x.log"), "ignored\n").unwrap();
        fill(&repo.join("src").join("gen"), 51);
        fill(&repo.join("fifty"), 50);
        std::fs::create_dir_all(repo.join("小目录").join("子")).unwrap();
        std::fs::write(repo.join("小目录").join("a.txt"), "a\n").unwrap();
        std::fs::write(repo.join("小目录").join("子").join("b.txt"), "b\n").unwrap();
        std::fs::create_dir_all(repo.join("src").join("new")).unwrap();
        std::fs::write(repo.join("src").join("new").join("one.txt"), "1\n").unwrap();
        let nested = repo.join("nest").join("inner");
        std::fs::create_dir_all(&nested).unwrap();
        sb.git(&nested, &["init", "-q"]);
        std::fs::write(nested.join("x.txt"), "x\n").unwrap();

        let files = git_diff_summary_blocking(&s(&repo)).unwrap().files;
        assert_eq!(folded_of(&files), vec![("big dir/".to_string(), 60), ("src/gen/".to_string(), 51)]);
        // 折叠行仍是未跟踪的「更改」（在「更改」组，能暂存 / 放弃），不数行数。
        let big = files.iter().find(|f| f.path == "big dir/").unwrap();
        assert_eq!((big.status.as_str(), big.worktree_status.as_deref(), big.additions), ("U", Some("U"), None));
        let expanded: Vec<&str> = files.iter().filter(|f| f.untracked_files.is_none()).map(|f| f.path.as_str()).collect();
        assert_eq!(expanded.len(), 50 + 3, "{expanded:?}");
        assert!(expanded.contains(&"fifty/d4/deep/f49.js"));
        assert!(expanded.contains(&"小目录/a.txt") && expanded.contains(&"小目录/子/b.txt"));
        assert!(expanded.contains(&"src/new/one.txt"));
        // 展开的文件不是目录条目，也没有嵌套仓库、被忽略的文件。
        assert!(expanded.iter().all(|p| !p.ends_with('/') && !p.starts_with("nest") && !p.ends_with(".log")));
        // 展开的未跟踪文件照旧有行数。
        assert_eq!(files.iter().find(|f| f.path == "src/new/one.txt").unwrap().additions, Some(1));
    }

    /// 折叠行上的数字缓存着（轮询不必每轮扫整个目录），写操作之后立刻对上。缓存命中本身不在这里断言：
    /// 缓存是全局的，并行跑的别的测试做写操作会清掉它（命中的效果看基准的「热」一行）。
    #[test]
    fn fold_count_refreshes_after_write_ops() {
        let sb = Sandbox::new("fold-cache");
        let repo = sb.repo("r");
        fill(&repo.join("out"), 60);
        let dir = s(&repo);
        assert_eq!(folded_of(&git_diff_summary_blocking(&dir).unwrap().files), vec![("out/".to_string(), 60)]);
        std::fs::write(repo.join("out").join("more.js"), "x\n").unwrap();
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        stage_blocking(&dir, &["README.md".into()]).unwrap();
        assert_eq!(folded_of(&git_diff_summary_blocking(&dir).unwrap().files), vec![("out/".to_string(), 61)]);
    }

    /// 点开折叠目录：按需列出其中的文件；只认此刻清单里的未跟踪目录条目。
    #[test]
    fn untracked_dir_files_lists_folded_dir_on_demand() {
        let sb = Sandbox::new("fold-list");
        let repo = sb.repo("r");
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        std::fs::create_dir_all(repo.join("sub")).unwrap();
        std::fs::write(repo.join("sub").join("t.txt"), "t\n").unwrap();
        sb.commit(&repo, "ignore");
        fill(&repo.join("big dir"), 60);
        std::fs::write(repo.join("big dir").join("x.log"), "ignored\n").unwrap();
        let dir = s(&repo);
        let listed = untracked_dir_files_blocking(&dir, "big dir/").unwrap();
        assert_eq!(listed.len(), 60);
        assert!(listed.iter().all(|f| f.path.starts_with("big dir/d") && f.worktree_status.as_deref() == Some("U") && f.untracked_files.is_none()));
        for bad in ["README.md", "big dir", "nope/", "../big dir/", "big dir/d0/", ""] {
            assert_eq!(untracked_dir_files_blocking(&dir, bad).unwrap_err(), "git/not_in_changes", "{bad}");
        }
        // 从仓库子目录调用（会话 cwd 不是仓库根）路径也相对仓库根。
        assert_eq!(untracked_dir_files_blocking(&s(&repo.join("sub")), "big dir/").unwrap().len(), 60);
    }

    /// 折叠行整目录暂存 / 放弃，展开后的文件逐个暂存 / 放弃。
    #[test]
    fn stage_and_discard_folded_dir_and_files_inside() {
        let sb = Sandbox::new("fold-write");
        let repo = sb.repo("r");
        fill(&repo.join("a b"), 60);
        fill(&repo.join("gen"), 60);
        let dir = s(&repo);
        // 展开后逐个：暂存一个、放弃一个。
        stage_blocking(&dir, &["a b/d0/deep/f0.js".into()]).unwrap();
        discard_with(&dir, &["a b/d0/deep/f1.js".into()], fake_trash_ok).unwrap();
        assert!(!repo.join("a b/d0/deep/f1.js").exists());
        assert!(statuses(&repo).contains(&st("a b/d0/deep/f0.js", Some("A"), None)));
        // 整目录暂存：60 个都进暂存区。
        stage_blocking(&dir, &["gen/".into()]).unwrap();
        let staged = statuses(&repo).into_iter().filter(|(p, i, _)| p.starts_with("gen/") && i.as_deref() == Some("A")).count();
        assert_eq!(staged, 60);
        // 整目录放弃：目录一次进回收站（假回收站收到的是目录本身，一条）。
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DIRS: AtomicUsize = AtomicUsize::new(0);
        fn dir_trash(paths: &[std::path::PathBuf]) -> Result<(), String> {
            DIRS.fetch_add(paths.iter().filter(|p| p.is_dir()).count(), Ordering::SeqCst);
            fake_trash_ok(paths)
        }
        // 「a b/」里还剩 58 个未跟踪文件，仍是折叠的目录条目吗？不是：有一个已暂存，整个目录不再是
        // 「整个未跟踪」，status 改列逐个文件。挑另一个纯未跟踪的目录试整目录放弃。
        fill(&repo.join("trash me"), 60);
        discard_with(&dir, &["trash me/".into()], dir_trash).unwrap();
        assert_eq!(DIRS.load(Ordering::SeqCst), 1);
        assert!(!repo.join("trash me").exists());
        // 已暂存的目录不再是未跟踪目录条目：整目录放弃被拒。
        assert_eq!(discard_with(&dir, &["gen/".into()], fake_trash_ok).unwrap_err(), "git/not_in_changes");
    }

    /// 未跟踪目录下面的路径也要逐个核对：被忽略的、不存在的、带 `..` 的一律拒，文件不动。
    #[test]
    fn write_ops_reject_unlisted_paths_under_untracked_dir() {
        let sb = Sandbox::new("fold-reject");
        let repo = sb.repo("r");
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        sb.commit(&repo, "ignore");
        fill(&repo.join("out"), 60);
        std::fs::write(repo.join("out").join("keep.log"), "keep\n").unwrap();
        let dir = s(&repo);
        for path in ["out/keep.log", "out/nope.js", "out/../README.md", "out/d0/", "out"] {
            assert_eq!(discard_with(&dir, &[path.into()], fake_trash_ok).unwrap_err(), "git/not_in_changes", "{path}");
            assert_eq!(stage_blocking(&dir, &[path.into()]).unwrap_err(), "git/not_in_changes", "{path}");
        }
        assert!(repo.join("out").join("keep.log").exists());
        assert!(!untracked_file_listed(&dir, "out/keep.log"));
        assert!(!untracked_file_listed(&dir, "out/"));
        assert!(untracked_file_listed(&dir, "out/d0/deep/f0.js"));
    }

    /// 目录里有嵌套仓库：整目录暂存跳过它（不加 gitlink），整目录放弃拒绝（git/dir_has_repo），文件不动。
    #[test]
    fn folded_dir_with_nested_repo() {
        let sb = Sandbox::new("fold-nested");
        let repo = sb.repo("r");
        fill(&repo.join("pkg"), 60);
        let inner = repo.join("pkg").join("vendor");
        std::fs::create_dir_all(&inner).unwrap();
        sb.git(&inner, &["init", "-q"]);
        std::fs::write(inner.join("v.txt"), "v\n").unwrap();
        let dir = s(&repo);
        assert_eq!(folded_of(&git_diff_summary_blocking(&dir).unwrap().files), vec![("pkg/".to_string(), 60)]);
        assert_eq!(discard_with(&dir, &["pkg/".into()], fake_trash_ok).unwrap_err(), "git/dir_has_repo");
        assert!(inner.join("v.txt").exists());
        stage_blocking(&dir, &["pkg/".into()]).unwrap();
        let index = git_output(&dir, &["ls-files", "-s"]).unwrap();
        let index = stdout_text(&index.stdout);
        assert!(!index.contains("160000"), "嵌套仓库被当 gitlink 暂存了：{index}");
        assert_eq!(index.lines().filter(|l| l.contains("pkg/d")).count(), 60);
    }

    /// 在 link 处建一个指向 target 目录的 junction（Windows，不要权限）或目录符号链接（Unix）。
    fn make_dir_link(target: &Path, link: &Path) -> bool {
        #[cfg(windows)]
        {
            Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
    }

    /// 整目录放弃：目录里藏着 junction / 符号链接就拒（git/dir_has_links），一个文件都不动。
    /// Windows 上 `FileType::is_symlink` 对 junction 也为 true，这条就是实测。
    #[test]
    fn discard_dir_refuses_junction() {
        let sb = Sandbox::new("fold-junction");
        let repo = sb.repo("r");
        let outside = sb.0.join("precious");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("data.txt"), "precious\n").unwrap();
        fill(&repo.join("out"), 60);
        let link = repo.join("out").join("d3").join("link");
        assert!(make_dir_link(&outside, &link), "建不了 junction / 目录链接");
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "junction 的 is_symlink 不为 true");
        let dir = s(&repo);
        assert!(folded_of(&git_diff_summary_blocking(&dir).unwrap().files).iter().any(|(p, _)| p == "out/"));
        assert_eq!(discard_with(&dir, &["out/".into()], fake_trash_ok).unwrap_err(), "git/dir_has_links");
        assert!(repo.join("out").join("d0").join("deep").join("f0.js").exists());
        assert_eq!(std::fs::read_to_string(outside.join("data.txt")).unwrap(), "precious\n");
        // 拆掉链接本身（不进入），沙盒清理时 remove_dir_all 才不会顺着 junction 删到外面。
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
    }

    /// 文件符号链接同样拒。Windows 没开开发者模式时建不了文件链接，跳过并说明。
    #[test]
    fn discard_dir_refuses_file_symlink() {
        let sb = Sandbox::new("fold-symlink");
        let repo = sb.repo("r");
        let target = sb.0.join("target.txt");
        std::fs::write(&target, "precious\n").unwrap();
        fill(&repo.join("out"), 60);
        let link = repo.join("out").join("d1").join("deep").join("link.txt");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target, &link).is_ok();
        if !made {
            eprintln!("建不了文件符号链接（Windows 未开开发者模式），跳过");
            return;
        }
        let dir = s(&repo);
        assert_eq!(discard_with(&dir, &["out/".into()], fake_trash_ok).unwrap_err(), "git/dir_has_links");
        assert!(repo.join("out").join("d0").join("deep").join("f0.js").exists());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "precious\n");
    }

    /// 目录的上级是 junction（仓库里一个链出去的目录）：同样拒，不顺着链接去回收站。
    #[test]
    fn discard_dir_refuses_linked_parent() {
        let sb = Sandbox::new("fold-linked-parent");
        let repo = sb.repo("r");
        let outside = sb.0.join("elsewhere");
        fill(&outside.join("build"), 60);
        let link = repo.join("linked");
        assert!(make_dir_link(&outside, &link), "建不了 junction / 目录链接");
        // git 不穿过链接列文件，清单里不会有 linked/build/；直接拿底层检查验证上级链接也会被拒。
        assert_eq!(untracked_dir_to_trash(&s(&repo), "linked/build").unwrap_err(), "git/dir_has_links");
        assert!(outside.join("build").join("d0").join("deep").join("f0.js").exists());
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
    }

    /// 加入 .gitignore：追加 `/目录/`（没有就新建），之后目录从清单消失；已有同一行不重复加；
    /// 原文件的 CRLF、末尾有没有换行照旧；通配符转义；只认此刻清单里的未跟踪目录。
    #[test]
    fn ignore_dir_appends_rule_and_dir_disappears() {
        let sb = Sandbox::new("fold-ignore");
        let repo = sb.repo("r");
        let dir = s(&repo);
        let gi = repo.join(".gitignore");
        fill(&repo.join("big dir"), 60);
        ignore_dir_blocking(&dir, "big dir/").unwrap();
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), "/big dir/\n");
        let paths: Vec<String> = git_diff_summary_blocking(&dir).unwrap().files.into_iter().map(|f| f.path).collect();
        assert_eq!(paths, vec![".gitignore".to_string()]);

        // CRLF、末尾没有换行：照原样续上。嵌套目录写完整路径；方括号转义成字面量。
        std::fs::write(&gi, "/big dir/\r\n*.log").unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src").join("lib.rs"), "\n").unwrap();
        sb.commit(&repo, "src");
        fill(&repo.join("src").join("a[1]"), 3);
        ignore_dir_blocking(&dir, "src/a[1]/").unwrap();
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), "/big dir/\r\n*.log\r\n/src/a\\[1]/");
        let paths: Vec<String> = git_diff_summary_blocking(&dir).unwrap().files.into_iter().map(|f| f.path).collect();
        assert!(paths.iter().all(|p| !p.starts_with("src/a[1]")), "{paths:?}");

        // 已有同一行（后面被 ! 规则反选，所以目录还在清单里）：不再追加。
        std::fs::write(&gi, "/gen/\n!/gen/\n").unwrap();
        fill(&repo.join("gen"), 60);
        ignore_dir_blocking(&dir, "gen/").unwrap();
        assert_eq!(std::fs::read_to_string(&gi).unwrap(), "/gen/\n!/gen/\n");

        // 清单外的一律拒：已跟踪文件、文件、不带斜杠、`..`。
        for bad in ["README.md", "gen", "gen/d0/deep/f0.js", "../gen/", "nope/"] {
            assert_eq!(ignore_dir_blocking(&dir, bad).unwrap_err(), "git/not_in_changes", "{bad}");
        }
    }

    #[test]
    fn status_entries_keep_untracked_dirs_only() {
        let raw = b"?? target-check/\0?? src/new.rs\0 M a.rs\0";
        let got: Vec<String> = parse_status_entries(raw)
            .into_iter()
            .map(|entry| match entry {
                StatusEntry::File(f) => format!("file:{}", f.path),
                StatusEntry::UntrackedDir(d) => format!("dir:{d}"),
            })
            .collect();
        assert_eq!(got, vec!["dir:target-check/", "file:src/new.rs", "file:a.rs"]);
    }

    /// 摘要起的 git 进程数（二期 #28）：Windows 上每个 git 进程固定约 30ms，冷态 15000 个未跟踪文件
    /// 时 -unormal + 折叠比改前多一个 ls-files，冷耗时 186→199ms。分支名、↑↓ 改由 status -b 一并给出，
    /// 有折叠目录时共 4 个（rev-parse、status、ls-files、diff），改前 6 个。
    #[test]
    fn summary_spawns_at_most_four_git_processes() {
        let sb = Sandbox::new("spawns");
        let repo = sb.repo("r");
        for i in 0..60 {
            std::fs::create_dir_all(repo.join("build")).unwrap();
            std::fs::write(repo.join("build").join(format!("f{i}")), "x\n").unwrap();
        }
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        invalidate_caches();
        GIT_SPAWNS.with(|n| n.set(0));
        let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
        let spawned = GIT_SPAWNS.with(|n| n.get());
        assert_eq!(folded_of(&summary.files), vec![("build/".to_string(), 60)]);
        assert!(spawned <= 4, "摘要起了 {spawned} 个 git 进程");
    }

    #[test]
    fn branch_header_covers_every_tracking_shape() {
        // (分支, 上游, 领先, 落后)
        type Want<'a> = (Option<&'a str>, Option<&'a str>, Option<u32>, Option<u32>);
        let cases: [(&str, Want); 11] = [
            ("main", (Some("main"), None, None, None)),
            ("main...origin/main", (Some("main"), Some("origin/main"), Some(0), Some(0))),
            ("main...origin/main [ahead 3]", (Some("main"), Some("origin/main"), Some(3), Some(0))),
            ("main...origin/main [behind 2]", (Some("main"), Some("origin/main"), Some(0), Some(2))),
            (
                "feat/a.b...origin/feat/a.b [ahead 1, behind 12]",
                (Some("feat/a.b"), Some("origin/feat/a.b"), Some(1), Some(12)),
            ),
            ("main...origin/main [gone]", (Some("main"), None, None, None)),
            // aheadBehind 关掉时只说「不同」：上游在，数字未知，由调用方退回 rev-list。
            ("main...origin/main [different]", (Some("main"), Some("origin/main"), None, None)),
            ("HEAD (no branch)", (None, None, None, None)),
            ("No commits yet on main", (None, None, None, None)),
            ("Initial commit on main", (None, None, None, None)),
            ("", (None, None, None, None)),
        ];
        for (header, (branch, upstream, ahead, behind)) in cases {
            let want = BranchHeader {
                branch: branch.map(str::to_string),
                upstream: upstream.map(str::to_string),
                ahead,
                behind,
            };
            assert_eq!(parse_branch_header(header), want, "{header}");
        }
        let (header, rest) = split_branch_header(b"## main...o/main [ahead 1]  M a.rs ");
        assert_eq!((header.as_deref(), rest), (Some("main...o/main [ahead 1]"), &b" M a.rs "[..]));
        assert_eq!(split_branch_header(b" M a.rs "), (None, &b" M a.rs "[..]));
    }

    /// 分支名与 ↑↓ 与改前（rev-parse --abbrev-ref HEAD + rev-list --left-right）逐项一致：
    /// 无上游、同步、领先、领先且落后、上游已删、detached、还没有提交。
    #[test]
    fn summary_branch_and_divergence_match_rev_parse_and_rev_list() {
        let sb = Sandbox::new("branch-ab");
        let origin = sb.repo("origin");
        let clone = sb.0.join("clone");
        sb.git(&sb.0, &["clone", "-q", "-b", "main", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        let empty_commit = |dir: &Path, message: &str| {
            sb.git(dir, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-q", "-m", message]);
        };
        let check = |dir: &Path, label: &str| {
            let d = s(dir);
            let summary = git_diff_summary_blocking(&d).unwrap();
            let branch = git_output(&d, &["rev-parse", "--abbrev-ref", "HEAD"])
                .ok()
                .filter(|o| o.status.success())
                .map(|o| stdout_text(&o.stdout).trim().to_string())
                .filter(|name| !name.is_empty() && name != "HEAD");
            let ab = upstream_divergence(&d);
            assert_eq!(summary.branch, branch, "{label}");
            assert_eq!(summary.ahead.zip(summary.behind), ab, "{label}");
            ab
        };
        assert_eq!(check(&origin, "无上游"), None);
        assert_eq!(check(&clone, "同步"), Some((0, 0)));
        empty_commit(&clone, "local");
        assert_eq!(check(&clone, "领先"), Some((1, 0)));
        empty_commit(&origin, "remote");
        sb.git(&clone, &["fetch", "-q"]);
        assert_eq!(check(&clone, "领先且落后"), Some((1, 1)));
        // 上游不是同名分支（跟踪 origin/main）：推送不会推到它，↑↓ 不按它算（做空审查：以前照它推进了 main）。
        sb.git(&clone, &["checkout", "-q", "-b", "feature/x.y", "--track", "origin/main"]);
        let mismatched = git_diff_summary_blocking(&s(&clone)).unwrap();
        assert_eq!((mismatched.upstream, mismatched.ahead, mismatched.push_target.as_deref()), (None, None, Some("origin/feature/x.y")), "上游不同名");
        sb.git(&origin, &["branch", "-q", "feature/x.y"]);
        sb.git(&clone, &["fetch", "-q"]);
        sb.git(&clone, &["branch", "-q", "--set-upstream-to=origin/feature/x.y"]);
        assert_eq!(check(&clone, "带斜杠和点的分支名"), Some((0, 0)));
        sb.git(&origin, &["branch", "-q", "gone"]);
        sb.git(&clone, &["fetch", "-q"]);
        sb.git(&clone, &["checkout", "-q", "-b", "g", "--track", "origin/gone"]);
        sb.git(&origin, &["branch", "-q", "-D", "gone"]);
        sb.git(&clone, &["fetch", "-q", "--prune"]);
        assert_eq!(check(&clone, "上游已删"), None);
        sb.git(&clone, &["checkout", "-q", "--detach"]);
        assert_eq!(check(&clone, "detached"), None);
        let unborn = sb.0.join("unborn");
        std::fs::create_dir_all(&unborn).unwrap();
        sb.git(&unborn, &["init", "-q", "-b", "main"]);
        std::fs::write(unborn.join("a.txt"), "a\n").unwrap();
        assert_eq!(check(&unborn, "还没有提交"), None);
    }

    /// 基准（#25）：15000 个文件的未跟踪目录，改前（-uall 逐个文件）与改后（-unormal + 折叠）的摘要耗时。
    /// ignored：要造 15000 个文件，跑 release：
    /// `cargo test --release -p meowo-app --lib git::tests::bench_summary_with_15000_untracked_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_summary_with_15000_untracked_files() {
        // 改前的摘要（HEAD 上 summary_blocking 的等价实现）：-uall 把目录展开到每个文件，再补行数。
        fn before(dir: &str) -> Vec<GitChangedFileDto> {
            let _ = git_output(dir, &["rev-parse", "--is-inside-work-tree"]).unwrap();
            let _ = git_output(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
            let status = git_output(dir, &["status", "--porcelain", "-z", "--no-renames", "-uall"]).unwrap();
            let mut files = parse_porcelain_z(&status.stdout);
            fill_line_counts(dir, &mut files);
            let _ = upstream_divergence(dir);
            files
        }
        let sb = Sandbox::new("fold-bench");
        let repo = sb.repo("r");
        let size: usize = std::env::var("MEOWO_BENCH_FILE_BYTES").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        let body = "x".repeat(size.saturating_sub(1)) + "\n";
        for i in 0..15000 {
            let sub = repo.join("target-x").join(format!("d{}", i / 100));
            if i % 100 == 0 {
                std::fs::create_dir_all(&sub).unwrap();
            }
            std::fs::write(sub.join(format!("f{i}.rlib")), &body).unwrap();
        }
        std::fs::write(repo.join("README.md"), "edited\n").unwrap();
        let dir = s(&repo);
        let time = |label: &str, run: &dyn Fn() -> Vec<GitChangedFileDto>| {
            let mut best = std::time::Duration::MAX;
            let mut entries = 0;
            let mut bytes = 0;
            for _ in 0..5 {
                let start = std::time::Instant::now();
                let files = run();
                best = best.min(start.elapsed());
                entries = files.len();
                bytes = serde_json::to_vec(&files).unwrap().len();
            }
            eprintln!("{label}: 最快 {:.1} ms，{entries} 条，JSON {bytes} 字节", best.as_secs_f64() * 1000.0);
            best
        };
        let old = time("改前 -uall", &|| before(&dir));
        let cold = time("改后（每轮清缓存）", &|| {
            invalidate_caches();
            git_diff_summary_blocking(&dir).unwrap().files
        });
        let warm = time("改后（轮询，数字走缓存）", &|| git_diff_summary_blocking(&dir).unwrap().files);
        eprintln!(
            "改后/改前：冷 {:.2}，热 {:.2}",
            cold.as_secs_f64() / old.as_secs_f64(),
            warm.as_secs_f64() / old.as_secs_f64()
        );
        let files = git_diff_summary_blocking(&dir).unwrap().files;
        assert_eq!(folded_of(&files), vec![("target-x/".to_string(), 15000)]);
        assert_eq!(files.len(), 2);
    }
}
