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

fn git_diff_summary_blocking(cwd: &str) -> Result<GitDiffSummaryDto, String> {
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
    };
    // spawn 失败（git 未安装/被拦）不算错误——前端只是不显示按钮。
    let inside = match git_output(dir, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(output) => output,
        Err(_) => return Ok(unavailable()),
    };
    if !inside.status.success() || stdout_text(&inside.stdout).trim() != "true" {
        return Ok(GitDiffSummaryDto {
            is_repo: false,
            ..unavailable()
        });
    }
    let branch = git_output(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .filter(|o| o.status.success())
        .map(|o| stdout_text(&o.stdout).trim().to_string())
        .filter(|name| !name.is_empty() && name != "HEAD");
    // --no-renames：-z 格式下重命名条目会带两个路径，禁掉后每个条目恒为「XY <path>\0」。
    // -uall：整个目录都未跟踪时默认只报一条「?? dir/」，前端会把它当文件渲染
    //（尾斜杠让文件名为空、点开按文件读目录直接报错）；展开成具体文件才是「更改」的语义。
    let status = git_output(dir, &["status", "--porcelain", "-z", "--no-renames", "-uall"])
        .map_err(|e| e.to_string())?;
    if !status.status.success() {
        return Err("git status 执行失败".into());
    }
    let mut files = parse_porcelain_z(&status.stdout);
    fill_line_counts(dir, &mut files);
    let (ahead, behind) = match upstream_divergence(dir) {
        Some((ahead, behind)) => (Some(ahead), Some(behind)),
        None => (None, None),
    };
    Ok(GitDiffSummaryDto {
        git_available: true,
        is_repo: true,
        branch,
        ahead,
        behind,
        files,
    })
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
    for file in files.iter_mut().filter(|file| file.status == "U").take(MAX_UNTRACKED_COUNTED) {
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

/// 面板里最多列这么多个仓库：5 秒一轮的轮询里每个仓库要跑好几条 git，不设上限会拖慢。
const MAX_REPOS: usize = 8;
/// 工作目录下一层最多看这么多个子目录（找有改动的仓库，见 changes_overview_blocking）。
const MAX_CHILD_SCAN: usize = 200;

/// 「更改」面板的仓库清单。`touched` 是会话编辑 / 写入过的文件（绝对路径），按各自所在仓库
/// 归组；工作目录所在仓库排第一。会话还没动过文件、工作目录又不是仓库时，列工作目录下一层
/// 当前有改动的仓库（用户要求，2026-11-09：只列会话动过的，不混入别的会话或手动的改动）。
#[tauri::command]
pub(crate) async fn git_changes_overview(cwd: String, touched: Vec<String>) -> Result<Vec<RepoChangesDto>, String> {
    tauri::async_runtime::spawn_blocking(move || changes_overview_blocking(&cwd, &touched))
        .await
        .map_err(|e| e.to_string())?
}

pub(crate) fn changes_overview_blocking(cwd: &str, touched: &[String]) -> Result<Vec<RepoChangesDto>, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    let cwd_root = repo_root(dir);
    let mut roots: Vec<String> = cwd_root.iter().cloned().collect();
    for path in touched {
        if roots.len() >= MAX_REPOS {
            break;
        }
        let Some(start) = existing_dir_of(path) else { continue };
        if let Some(root) = repo_root(&start) {
            if !roots.iter().any(|known| same_path(known, &root)) {
                roots.push(root);
            }
        }
    }
    let mut out: Vec<RepoChangesDto> = roots
        .iter()
        .filter_map(|root| repo_changes(root, cwd_root.as_deref().is_some_and(|c| same_path(c, root))))
        .collect();
    if out.is_empty() && cwd_root.is_none() {
        out = child_repos(dir)
            .into_iter()
            .filter_map(|root| repo_changes(&root, false))
            .filter(|repo| !repo.summary.files.is_empty())
            .take(MAX_REPOS)
            .collect();
    }
    Ok(out)
}

/// 和会话目录相关的仓库：它所在仓库、这个仓库的各个 worktree、它下一层的仓库。手机远程页
/// 只放行这些（远程端传什么路径都进得来，不能借面板去看任意仓库）。
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
    out.extend(child_repos(cwd));
    out
}

/// 远程端带来的目录是不是 session_dir 的相关仓库之一。
pub(crate) fn is_related_root(session_dir: &str, root: &str) -> bool {
    related_roots(session_dir).iter().any(|known| same_path(known, root))
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
    })
}

/// 「已提交未合并」的基准：merge-base(HEAD, 默认分支)。默认分支先认 origin/HEAD 指向的，
/// 再退到 origin/main、origin/master、本地 main、master。当前就在默认分支上、HEAD 本身就是
/// 基准（没有自己的提交）时返回 None。
fn default_base(root: &str, branch: Option<&str>) -> Option<(String, String)> {
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
    let base_ref = candidates.into_iter().find(|name| {
        git_output(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])
            .is_ok_and(|o| o.status.success())
    })?;
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
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .take(MAX_CHILD_SCAN)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(".git").exists())
        .map(|path| norm_slashes(&path.to_string_lossy()))
        .collect();
    out.sort();
    out
}

fn norm_slashes(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
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
    let output = match base {
        Some(base) => git_output(dir, &["diff", base, "HEAD", "--", path]),
        None => git_output(dir, &["diff", "HEAD", "--", path]),
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
fn parse_pull_request(json: &str) -> Option<PullRequestDto> {
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
    })
}

fn git_output(dir: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args);
    // Windows：GUI 进程 spawn 控制台子进程会闪现 conhost 黑窗，压掉。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command.output()
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

/// 解析 `git status --porcelain -z --no-renames` 输出：每个条目是 `XY <path>\0`。
/// 状态归一化：X/Y 任一为 M/A/D/T 取该字母（X 优先）；`??` 记为 "U"（未跟踪）。
fn parse_porcelain_z(bytes: &[u8]) -> Vec<GitChangedFileDto> {
    let mut files = Vec::new();
    for entry in bytes.split(|b| *b == 0) {
        if entry.len() < 4 {
            continue;
        }
        let x = entry[0];
        let y = entry[1];
        // entry[2] 是空格分隔符；路径原样保留（porcelain 用正斜杠，前端按它展示）。
        let path = String::from_utf8_lossy(&entry[3..]).into_owned();
        // 目录条目(尾斜杠,如 -uall 缺席时的「?? dir/」)不是文件:渲染出来是一行空名,
        // 点开按文件读目录必报错。防御性跳过——正常路径下 -uall 已把目录展开成文件。
        if path.is_empty() || path.ends_with('/') {
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
        files.push(GitChangedFileDto {
            path,
            status,
            additions: None,
            deletions: None,
        });
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
        let repos = changes_overview_blocking(&parent, &[]).unwrap();
        assert_eq!(repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(!repos[0].is_cwd_repo);
        let touched = [s(&a.join("README.md"))];
        let repos = changes_overview_blocking(&parent, &touched).unwrap();
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
        let repos = changes_overview_blocking(&s(&main), &[s(&wt.join("new.rs"))]).unwrap();
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
}
