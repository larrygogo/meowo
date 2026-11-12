//! 「更改」面板的 push / 开 PR / 看 CI / 合并（二期包 5，定案设计「四、push / PR / CI / 合并」）。
//!
//! 对标 Conductor / Paradev：从提交到合并都在 Meowo 里做完。几条底线：
//! - git 联网调用一律不弹凭据框、不等终端输入（GIT_TERMINAL_PROMPT=0、GCM_INTERACTIVE=never、
//!   `-c credential.interactive=false`），push 120 秒超时，永不 force。GUI 进程没有终端，卡在密码提示上
//!   用户只会看到按钮一直转。
//! - 没装 gh（产品事实：Meowo 的用户不一定装了 gh）：push 照常，开 PR 改为打开 GitHub 的 compare 网页，
//!   不显示 CI、不能应用内合并（leader 关卡 Q-no-gh 选 1）。
//! - 合并前后端再判一次检查（只有 none / pass 放行），带 `--match-head-commit`：确认框打开之后有人
//!   推了新提交，GitHub 会拒，不会把用户没看过的提交合进去。不加 `--delete-branch`，worktree 和分支
//!   的清理交给 worktree.rs 那套判定，只提示不自动删。
//!
//! gh 调用全部经 [`GhExec`] 注入：单测喂录制的 gh JSON（录自真实仓库的只读 `gh pr view`，见
//! forge/fixtures），不碰任何真实远端。

use crate::git_target::{resolve, GitConfig, PushTarget};
use meowo_protocol::ipc::{
    ForgeRepoChoiceDto, ForgeStatusDto, GitJobDto, MergeResultDto, PrCheckDto, PrDefaultsDto, PullRequestDto, PullResultDto, PushResultDto,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(crate) mod badge;
pub(crate) use badge::sidebar_git_badges_for;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod drill;

/// push 的超时：大仓库首推、慢网络留足余量，又不至于让按钮无限转。
const PUSH_TIMEOUT: Duration = Duration::from_secs(120);
/// 单次 gh 调用的超时（gh 自己没有总超时，网络半死时会一直挂着）。
const GH_TIMEOUT: Duration = Duration::from_secs(60);
/// gh 登录状态缓存：`gh auth status` 要联网验 token，不能每分钟一次。
const AUTH_TTL: Duration = Duration::from_secs(300);
/// 仓库允许的合并方式很少改，缓存 1 小时。
const METHODS_TTL: Duration = Duration::from_secs(3600);
/// PR 与检查：每个仓库最多 60 秒查一次 GitHub（规格「每仓库 ≥60s 一次」）。
const PR_TTL: Duration = Duration::from_secs(60);
/// 手动刷新（窗口重新聚焦、点刷新）也至少隔 10 秒，连点不会连打 API。
const FORCE_MIN: Duration = Duration::from_secs(10);
/// 后台推送任务的结果保留时长（手机轮询断了一会儿再回来还能拿到）。
const JOB_KEEP: Duration = Duration::from_secs(600);

/// 环境变量覆盖：Some 设值、None 删掉。正式调用为空，测试用它隔离用户的全局 git 配置。
pub(crate) type GitEnv<'a> = &'a [(&'a str, Option<&'a str>)];

// ═══ 子进程：超时 + 不弹窗 ═══

/// 一次子进程的结果。`code` 为 None：被超时杀掉或被信号结束。
#[derive(Debug, Clone, Default)]
pub(crate) struct CmdOut {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}

impl CmdOut {
    fn ok(&self) -> bool {
        !self.timed_out && self.code == Some(0)
    }
    fn both(&self) -> String {
        format!("{}\n{}", self.stderr, self.stdout)
    }
}

fn command(program: &Path) -> Command {
    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
    let mut command = Command::new(program);
    // Windows：GUI 进程 spawn 控制台子进程会闪现 conhost 黑窗，压掉（同 git.rs）。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

/// 带超时地跑完一个进程。stdin 接空：任何「等输入」都立刻读到 EOF 失败，而不是挂住。
/// 超时先杀子孙（git push 底下是 git-remote-https / ssh，只杀 git 本身它们还握着管道），再杀自己。
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<CmdOut> {
    use std::io::Read;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let pipe = |reader: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut reader) = reader {
                let _ = reader.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = pipe(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_rx = pipe(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(status) => break Some(status),
            None if start.elapsed() >= timeout => {
                crate::proc::kill_descendants(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // 孙进程可能还握着管道没退：最多再等 2 秒收输出，读不到就算了，不能因此挂住。
    let grace = Duration::from_secs(2);
    let text = |rx: std::sync::mpsc::Receiver<Vec<u8>>| crate::fsutil::decode_text_bytes(&rx.recv_timeout(grace).unwrap_or_default());
    Ok(CmdOut {
        code: status.and_then(|s| s.code()),
        stdout: text(out_rx),
        stderr: text(err_rx),
        timed_out: status.is_none(),
    })
}

/// 本地 git（不联网）：失败返回 None。
fn git(dir: &str, args: &[&str], env: GitEnv) -> Option<String> {
    let mut cmd = command(Path::new("git"));
    cmd.arg("-C").arg(dir).args(args);
    apply_env(&mut cmd, env);
    let out = cmd.stdin(Stdio::null()).output().ok()?;
    out.status
        .success()
        .then(|| crate::fsutil::decode_text_bytes(&out.stdout).trim().to_string())
}

fn apply_env(cmd: &mut Command, env: GitEnv) {
    for (key, value) in env {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
}

/// 联网的 git（push）。不弹凭据框的三道：GIT_TERMINAL_PROMPT=0 关掉 git 自己的命令行提示；
/// GCM_INTERACTIVE=never 与 credential.interactive=false 关掉 Git Credential Manager 的登录窗
/// （Windows 版 Git 默认带 GCM，凭据失效时会弹浏览器登录框，GUI 后台调用没人点就一直等）。
/// LC_ALL=C：失败归类靠匹配 git 的英文提示，中文系统上 git 会输出翻译过的文字。
fn git_net(dir: &str, args: &[&str], env: GitEnv) -> Command {
    let mut cmd = command(Path::new("git"));
    cmd.arg("-C")
        .arg(dir)
        .args(["-c", "credential.interactive=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .env("LANGUAGE", "C");
    apply_env(&mut cmd, env);
    cmd
}

fn existing_dir(cwd: &str) -> Result<&str, String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    Ok(dir)
}

/// 输出最后几行，错误提示里带上（钩子、服务器给的原因），不撑爆界面。
fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

// ═══ push ═══

/// 推到哪：远端名、refspec、是否顺手设上游。
#[derive(Debug, PartialEq, Eq)]
struct PushPlan {
    remote: String,
    branch: String,
    refspec: String,
    set_upstream: bool,
}

/// 公共 .git 目录与本 worktree 的 .git 目录（绝对路径）。
fn git_dirs(dir: &str, env: GitEnv) -> Option<(PathBuf, PathBuf)> {
    let out = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir", "--git-dir"], env)?;
    let mut lines = out.lines().map(str::trim);
    Some((PathBuf::from(lines.next()?), PathBuf::from(lines.next()?)))
}

/// 读配置：与摘要（git.rs）同一个读法（git_target::GitConfig::load），环境变量先看注入的 env 再看进程的
///（测试把全局配置隔离到空文件，判定也得跟着隔离）。
fn load_config(common: &Path, git_dir: &Path, env: GitEnv) -> GitConfig {
    let lookup = |key: &str| -> Option<String> {
        match env.iter().find(|(k, _)| *k == key) {
            Some((_, value)) => value.map(str::to_string),
            None => std::env::var(key).ok(),
        }
    };
    GitConfig::load(common, Some(git_dir), &lookup)
}

/// 推到哪：git_target::resolve 的结论（与摘要的 pushTarget 同一个判定，做空第二轮）。refspec 写死
/// `HEAD:refs/heads/<远端分支>`（不写裸 `HEAD`，remote.<名>.push 的映射不生效）。
fn plan_push(dir: &str, env: GitEnv) -> Result<PushPlan, String> {
    let branch = git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"], env)
        .filter(|b| !b.is_empty())
        .ok_or("git/detached")?;
    let (common, git_dir) = git_dirs(dir, env).ok_or("git/no_remote")?;
    let target = resolve(&load_config(&common, &git_dir, env), &common, &branch).ok_or("git/no_remote")?;
    Ok(PushPlan {
        refspec: format!("HEAD:refs/heads/{}", target.branch),
        remote: target.remote,
        branch: target.branch,
        set_upstream: target.set_upstream,
    })
}

/// push 失败归类（LC_ALL=C 下 git 的英文输出；remote: 开头的是服务器原话，不受语言影响）。
/// 先判服务器拒绝（保护分支、pre-receive），再判 non-fast-forward，再凭据、网络，最后本地钩子。
pub(crate) fn classify_push_failure(out: &CmdOut) -> String {
    if out.timed_out {
        return "git/push_timeout".into();
    }
    let both = out.both();
    let low = both.to_lowercase();
    let has = |needle: &str| low.contains(needle);
    if has("gh006") || has("gh013") || has("protected branch") || has("pre-receive hook declined") || (has("[remote rejected]") && has("hook declined")) {
        return format!("git/push_protected: {}", tail(&remote_lines(&both), 4));
    }
    if has("[rejected]") && (has("fetch first") || has("non-fast-forward") || has("stale info") || has("already exists")) {
        return "git/push_rejected".into();
    }
    let auth = [
        "authentication failed",
        "could not read username",
        "could not read password",
        // GIT_TERMINAL_PROMPT=0 下 http 远端要密码时的原话（本机 401 服务实测，git 2.5x）。
        "unable to get password",
        "unable to get username",
        "terminal prompts disabled",
        "invalid username or password",
        "permission denied (publickey",
        "host key verification failed",
        "returned error: 401",
        "returned error: 403",
        "permission to ",
        "access denied",
    ];
    if auth.iter().any(|needle| has(needle)) {
        return "git/push_auth".into();
    }
    let network = [
        "could not resolve host",
        "could not resolve hostname",
        "failed to connect",
        "connection timed out",
        "connection refused",
        "connection reset",
        "network is unreachable",
        "operation timed out",
        "ssl_connect",
        "unable to access",
        "could not read from remote repository",
    ];
    if network.iter().any(|needle| has(needle)) {
        return "git/push_network".into();
    }
    // 本地 pre-push 钩子失败：git 只说「failed to push some refs」，没有任何 [rejected] 行和 fatal。
    if has("failed to push some refs") && !has("[rejected]") && !has("[remote rejected]") && !has("fatal:") {
        return format!("git/push_hook: {}", tail(&both.replace("error: failed to push some refs", ""), 6));
    }
    format!("git/push_failed: {}", tail(&both, 6))
}

/// 服务器的原话（`remote: ...`）：保护分支被拒时这几行说清楚是哪条规则。没有就用全部输出。
fn remote_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter_map(|l| l.trim_start().strip_prefix("remote:")).map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() { text.to_string() } else { lines.join("\n") }
}

/// 推送当前分支。永不 force：refspec 不带 `+`、不传 --force；命令行给了 refspec，配置里的
/// remote.<名>.push（可能带 +）不生效。不加 --no-verify，用户的 pre-push 钩子照跑。
pub(crate) fn push_with(cwd: &str, env: GitEnv, timeout: Duration) -> Result<PushResultDto, String> {
    let dir = existing_dir(cwd)?;
    let plan = plan_push(dir, env)?;
    // 推之前数一次「不在这个远端上的提交」，结果里告诉用户推了几个。
    let pushed = git(dir, &["rev-list", "--count", "HEAD", "--not", &format!("--remotes={}", plan.remote)], env)
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let mut args: Vec<&str> = vec!["push"];
    if plan.set_upstream {
        args.push("-u");
    }
    args.push(&plan.remote);
    args.push(&plan.refspec);
    let out = run_with_timeout(git_net(dir, &args, env), timeout).map_err(|e| format!("git/push_failed: {e}"))?;
    invalidate_after_write(dir);
    if !out.ok() {
        return Err(classify_push_failure(&out));
    }
    Ok(PushResultDto { remote: plan.remote, branch: plan.branch, set_upstream: plan.set_upstream, pushed })
}

#[tauri::command]
pub(crate) async fn git_push(cwd: String) -> Result<PushResultDto, String> {
    tauri::async_runtime::spawn_blocking(move || push_with(&cwd, &[], PUSH_TIMEOUT))
        .await
        .map_err(|e| e.to_string())?
}

// ═══ 后台任务（手机端推送）═══

struct Job {
    at: Instant,
    /// `push` / `pull`：同一个仓库同种任务在跑时复用任务号，推送和拉取互不顶替。
    kind: &'static str,
    repo: String,
    dto: GitJobDto,
}

static JOBS: Mutex<Option<HashMap<String, Job>>> = Mutex::new(None);

fn job_id(kind: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{kind}-{:x}-{:x}", nanos, SEQ.fetch_add(1, Ordering::Relaxed))
}

/// 起一个后台推送，立刻返回任务号。手机 RPC 有 20–90 秒的超时，首推大仓库会超过它（定案设计「五」）。
/// 同一个仓库已经有推送在跑：返回那个任务号，手机上连点两下不会推两次。
pub(crate) fn start_push_job(cwd: String, run: impl FnOnce(&str) -> Result<PushResultDto, String> + Send + 'static) -> String {
    start_job("push", cwd, move |dir| run(dir).map(|push| GitJobDto { state: "ok".into(), push: Some(push), pull: None, error: None }))
}

/// 起一个后台拉取（推送被拒后的「拉取并重试」，walkthrough P-1）。联网同推送，手机 RPC 等不了，走同一套任务。
pub(crate) fn start_pull_job(cwd: String, run: impl FnOnce(&str) -> Result<PullResultDto, String> + Send + 'static) -> String {
    start_job("pull", cwd, move |dir| run(dir).map(|pull| GitJobDto { state: "ok".into(), push: None, pull: Some(pull), error: None }))
}

fn start_job(kind: &'static str, cwd: String, run: impl FnOnce(&str) -> Result<GitJobDto, String> + Send + 'static) -> String {
    let repo = cwd.trim().replace('\\', "/").to_lowercase();
    let mut guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    let jobs = guard.get_or_insert_with(Default::default);
    jobs.retain(|_, job| job.dto.state == "running" || job.at.elapsed() < JOB_KEEP);
    if let Some((id, _)) = jobs.iter().find(|(_, job)| job.kind == kind && job.repo == repo && job.dto.state == "running") {
        return id.clone();
    }
    let id = job_id(kind);
    jobs.insert(
        id.clone(),
        Job { at: Instant::now(), kind, repo, dto: GitJobDto { state: "running".into(), push: None, pull: None, error: None } },
    );
    drop(guard);
    let job = id.clone();
    std::thread::spawn(move || {
        let result = run(&cwd);
        let mut guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_or_insert_with(Default::default).get_mut(&job) {
            entry.at = Instant::now();
            entry.dto = match result {
                Ok(dto) => dto,
                Err(error) => GitJobDto { state: "failed".into(), push: None, pull: None, error: Some(error) },
            };
        }
    });
    id
}

pub(crate) fn job_status(job_id: &str) -> Result<GitJobDto, String> {
    let guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .and_then(|jobs| jobs.get(job_id))
        .map(|job| job.dto.clone())
        .ok_or_else(|| "git/job_not_found".into())
}

#[tauri::command]
pub(crate) async fn git_push_start(cwd: String) -> Result<String, String> {
    existing_dir(&cwd)?;
    Ok(start_push_job(cwd, |dir| push_with(dir, &[], PUSH_TIMEOUT)))
}

#[tauri::command]
pub(crate) async fn git_job_status(job_id: String) -> Result<GitJobDto, String> {
    job_status(&job_id)
}

// ═══ pull（推送被拒后的「拉取并重试」）═══
//
// walkthrough P-1（2026-11-12）：推送被拒只显示「远端有新提交，先拉取再推」，面板里没有拉取，用户只能
// 去终端。这里补一个拉取：从推送目标（与推送同一个 git_target::resolve 判定）拉回那条分支并接到本地。
// 底线：
// - 有冲突不合并：分叉时先用 `merge-tree --write-tree` 在不碰工作区、不碰暂存区的前提下判一次，有冲突
//   直接报 git/pull_conflict，仓库原样不动；老 git 不支持就照试，试出冲突立刻 --abort 退回。
// - 不用 autostash：autostash 会往用户的 stash 列表里塞条目，冲突时还会留在那里（`--no-autostash`
//   显式关掉，用户配了 merge.autoStash / rebase.autoStash 也不生效）。
// - 照用户的 pull.rebase：配了变基就变基（要求已跟踪文件没有未提交的改动），否则合并（--no-edit）。

/// 联网之外的各步也用同一套环境（LC_ALL=C 归类、不弹凭据框），另外不许开编辑器：GUI 后台没人关它。
fn git_step(dir: &str, args: &[&str], env: GitEnv, timeout: Duration) -> Result<CmdOut, String> {
    let mut cmd = git_net(dir, args, env);
    cmd.env("GIT_EDITOR", "true").env("GIT_SEQUENCE_EDITOR", "true");
    run_with_timeout(cmd, timeout).map_err(|e| format!("git/pull_failed: {e}"))
}

/// 拉取失败归类（LC_ALL=C 下 git 的英文输出）。联网失败与推送同一套判据，原因码单列（文案说的是拉取）。
pub(crate) fn classify_pull_failure(out: &CmdOut) -> String {
    if out.timed_out {
        return "git/pull_timeout".into();
    }
    let both = out.both();
    let low = both.to_lowercase();
    let has = |needle: &str| low.contains(needle);
    if has("index.lock") {
        return "git/locked".into();
    }
    if has("would be overwritten") {
        return "git/pull_dirty".into();
    }
    if has("please tell me who you are") || has("unable to auto-detect email") || has("empty ident") {
        return "git/identity".into();
    }
    if has("conflict") || has("could not apply") {
        return "git/pull_conflict".into();
    }
    if has("couldn't find remote ref") {
        return "git/pull_no_branch".into();
    }
    match classify_push_failure(out).as_str() {
        "git/push_auth" => "git/pull_auth".into(),
        "git/push_network" => "git/pull_network".into(),
        _ => format!("git/pull_failed: {}", tail(&both, 6)),
    }
}

/// 拉取当前分支在推送目标上的那条远端分支，接到本地。返回拉下来几个提交、怎么接的。
pub(crate) fn pull_with(cwd: &str, env: GitEnv, timeout: Duration) -> Result<PullResultDto, String> {
    let dir = existing_dir(cwd)?;
    let plan = plan_push(dir, env)?;
    let src = format!("refs/heads/{}", plan.branch);
    let fetched = git_step(dir, &["fetch", "--no-tags", &plan.remote, &src], env, timeout)?;
    invalidate_after_write(dir);
    if !fetched.ok() {
        return Err(classify_pull_failure(&fetched));
    }
    // 立刻把 FETCH_HEAD 解成提交号：后面几步都对着这个提交，别处再 fetch 也不串。
    let theirs = git(dir, &["rev-parse", "--verify", "--quiet", "FETCH_HEAD^{commit}"], env)
        .filter(|sha| !sha.is_empty())
        .ok_or_else(|| "git/pull_failed: FETCH_HEAD".to_string())?;
    let count = |range: String| git(dir, &["rev-list", "--count", &range], env).and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    let pulled = count(format!("HEAD..{theirs}"));
    let done = |mode: &str| PullResultDto { remote: plan.remote.clone(), branch: plan.branch.clone(), pulled, mode: mode.into() };
    if pulled == 0 {
        return Ok(done("none"));
    }
    let step = |args: &[&str]| git_step(dir, args, env, timeout);
    if count(format!("{theirs}..HEAD")) == 0 {
        // 本地没有远端以外的提交：快进。工作区里有会被覆盖的改动时 git 自己拒（would be overwritten），什么都不动。
        let out = step(&["merge", "--ff-only", "--no-autostash", &theirs])?;
        invalidate_after_write(dir);
        return if out.ok() { Ok(done("ff")) } else { Err(classify_pull_failure(&out)) };
    }
    // 分叉了：先判冲突（不动工作区）。退出码 1 = 有冲突；0 = 干净；别的（git < 2.38 不认 --write-tree）照试。
    let probe = step(&["merge-tree", "--write-tree", "--no-messages", "HEAD", &theirs])?;
    if !probe.timed_out && probe.code == Some(1) {
        return Err("git/pull_conflict".into());
    }
    // 合并 / 变基都要写提交：没配身份先说清楚，不让 git 半路失败。
    for ident in ["GIT_COMMITTER_IDENT", "GIT_AUTHOR_IDENT"] {
        if git(dir, &["var", ident], env).is_none() {
            return Err("git/identity".into());
        }
    }
    let rebase = git(dir, &["config", "--get", "pull.rebase"], env)
        .map(|v| v.to_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "true" | "yes" | "on" | "1" | "merges" | "m" | "interactive" | "i"));
    let result = if rebase {
        // 变基要求已跟踪文件没有未提交的改动（git 自己也会拒，这里先拦，错误说得更清楚）。
        let dirty = git(dir, &["status", "--porcelain", "--untracked-files=no"], env).is_none_or(|out| !out.trim().is_empty());
        if dirty {
            return Err("git/pull_dirty".into());
        }
        let out = step(&["rebase", "--no-autostash", &theirs])?;
        if out.ok() {
            Ok(done("rebase"))
        } else {
            // 变基停在半路（冲突、钩子）：退回变基前，不把仓库留在变基中间。
            let _ = step(&["rebase", "--abort"]);
            Err(classify_pull_failure(&out))
        }
    } else {
        let message = format!("Merge branch '{}' of {}", plan.branch, plan.remote);
        let out = step(&["merge", "--no-edit", "--no-autostash", "-m", &message, &theirs])?;
        if out.ok() {
            Ok(done("merge"))
        } else {
            // 合并停在半路（老 git 没探到的冲突、钩子失败）：退回合并前。合并开始前工作区就挡住了的
            //（would be overwritten）git 根本没开始，没有 MERGE_HEAD，不用退。
            if git(dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"], env).is_some() {
                let _ = step(&["merge", "--abort"]);
            }
            Err(classify_pull_failure(&out))
        }
    };
    invalidate_after_write(dir);
    result
}

#[tauri::command]
pub(crate) async fn git_pull_start(cwd: String) -> Result<String, String> {
    existing_dir(&cwd)?;
    Ok(start_pull_job(cwd, |dir| pull_with(dir, &[], PUSH_TIMEOUT)))
}

// ═══ git 身份（提交 / 合并时报 git/identity 的就地出路）═══

/// 写全局的 user.name / user.email（git 自己提示的就是 `git config --global`）。walkthrough P-1：此前只叫人
/// 「先在终端里运行 git config」，面板里没有出路。只收一行的名字和带 @ 的邮箱，别的不写。
pub(crate) fn set_identity_with(cwd: &str, name: &str, email: &str, env: GitEnv) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    let (name, email) = (name.trim(), email.trim());
    let bad = |text: &str| text.is_empty() || text.chars().any(char::is_control) || text.len() > 200;
    if bad(name) || bad(email) || !email.contains('@') || email.contains(char::is_whitespace) {
        return Err("git/identity_invalid".into());
    }
    for (key, value) in [("user.name", name), ("user.email", email)] {
        let out = git_step(dir, &["config", "--global", key, value], env, Duration::from_secs(10))?;
        if !out.ok() {
            return Err(format!("git/failed: {}", tail(&out.both(), 4)));
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn git_set_identity(cwd: String, name: String, email: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || set_identity_with(&cwd, &name, &email, &[]))
        .await
        .map_err(|e| e.to_string())?
}

// ═══ gh：找得到、登录了没 ═══

/// gh 调用的注入点：`(目录, 参数) -> 结果`；None = 没装 gh。
pub(crate) type GhExec<'a> = &'a (dyn Fn(&str, &[&str]) -> Option<CmdOut> + Sync);

/// 找 gh：进程 PATH 之外再看持久 PATH（Windows，Meowo 启动后才装的 gh）和常见安装目录
/// （macOS 从 Finder 启动的 App 只有 /usr/bin:/bin，Homebrew 的 /opt/homebrew/bin 不在里面）。
fn gh_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    #[cfg(target_os = "windows")]
    {
        dirs.extend(crate::envpath::persistent_path_dirs().into_iter().map(PathBuf::from));
        for (var, sub) in [
            ("ProgramFiles", r"GitHub CLI"),
            ("LOCALAPPDATA", r"Microsoft\WinGet\Links"),
            ("USERPROFILE", r"scoop\shims"),
        ] {
            if let Some(base) = std::env::var_os(var) {
                dirs.push(PathBuf::from(base).join(sub));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/home/linuxbrew/.linuxbrew/bin", "/opt/local/bin", "/usr/bin"] {
            dirs.push(PathBuf::from(dir));
        }
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/bin"));
        }
    }
    dirs
}

fn find_gh() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") { "gh.exe" } else { "gh" };
    gh_search_dirs().into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

/// gh 子进程的 PATH：在原 PATH 后补上找 gh 时看过的目录（gh 内部要调 git，macOS 上 git 也可能在 Homebrew 里）。
fn gh_child_path() -> std::ffi::OsString {
    std::env::join_paths(gh_search_dirs()).unwrap_or_default()
}

/// 真实的 gh。路径找过一次就记住；没找到隔 5 分钟再找（期间装上了也能认出来）。
pub(crate) fn real_gh(dir: &str, args: &[&str]) -> Option<CmdOut> {
    static FOUND: Mutex<Option<(Instant, Option<PathBuf>)>> = Mutex::new(None);
    let path = {
        let mut guard = FOUND.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some((_, Some(path))) if path.is_file() => Some(path.clone()),
            Some((at, None)) if at.elapsed() < AUTH_TTL => None,
            _ => {
                let found = find_gh();
                *guard = Some((Instant::now(), found.clone()));
                found
            }
        }
    }?;
    let mut cmd = command(&path);
    cmd.current_dir(dir)
        .args(args)
        .env("PATH", gh_child_path())
        // gh 也不许交互：没登录时直接报错，不进 `gh auth login` 的问答。
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    Some(run_with_timeout(cmd, GH_TIMEOUT).unwrap_or_else(|e| CmdOut { code: None, stderr: e.to_string(), ..Default::default() }))
}

/// gh 报的是「没登录 / token 失效」（实测 gh 2.96：`Try authenticating with:  gh auth login`、
/// `HTTP 401: Bad credentials`、未登录时 `To get started with GitHub CLI, please run:  gh auth login`）。
fn says_unauthed(out: &CmdOut) -> bool {
    let low = out.both().to_lowercase();
    low.contains("gh auth login") || low.contains("http 401") || low.contains("bad credentials")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GhState {
    Missing,
    Unauthed,
    Ok,
}

/// 进程内的各项缓存。测试各建一份，互不串。
#[derive(Default)]
pub(crate) struct Caches {
    auth: Mutex<Option<(Instant, GhState)>>,
    too_old: Mutex<Option<bool>>,
    methods: Mutex<HashMap<String, (Instant, Vec<String>)>>,
    prs: Mutex<HashMap<String, (Instant, Option<PullRequestDto>)>>,
    /// 本机发起合并后进了合并队列的 PR：(owner/repo, 号, 头提交)。GitHub 的 pr view 没有「在队列里」这个字段，
    /// 不记的话下一轮刷新只看到 OPEN，界面会退回「合并」按钮。
    queued: Mutex<HashSet<(String, u32, String)>>,
}

static CACHES: std::sync::LazyLock<Caches> = std::sync::LazyLock::new(Caches::default);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Caches {
    fn gh_state(&self, gh: GhExec, refresh: bool) -> GhState {
        if !refresh {
            if let Some((at, state)) = *lock(&self.auth) {
                if at.elapsed() < AUTH_TTL {
                    return state;
                }
            }
        }
        let state = match gh(".", &["auth", "status", "--hostname", "github.com"]) {
            None => GhState::Missing,
            Some(out) if out.ok() => GhState::Ok,
            Some(_) => GhState::Unauthed,
        };
        *lock(&self.auth) = Some((Instant::now(), state));
        state
    }

    fn mark_unauthed(&self) {
        *lock(&self.auth) = Some((Instant::now(), GhState::Unauthed));
    }

    /// gh 太旧：`gh pr merge` 没有 `--match-head-commit`（看 help 文本，不靠版本号猜）。
    fn gh_too_old(&self, gh: GhExec) -> bool {
        if let Some(known) = *lock(&self.too_old) {
            return known;
        }
        let Some(out) = gh(".", &["pr", "merge", "--help"]) else { return false };
        let old = !out.both().contains("--match-head-commit");
        *lock(&self.too_old) = Some(old);
        old
    }

    fn merge_methods(&self, gh: GhExec, dir: &str, slug: &str) -> Vec<String> {
        if let Some((at, methods)) = lock(&self.methods).get(slug) {
            if at.elapsed() < METHODS_TTL {
                return methods.clone();
            }
        }
        let Some(out) = gh(dir, &["repo", "view", slug, "--json", "mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed"]) else {
            return Vec::new();
        };
        if !out.ok() {
            if says_unauthed(&out) {
                self.mark_unauthed();
            }
            return Vec::new();
        }
        let methods = parse_merge_methods(&out.stdout);
        lock(&self.methods).insert(slug.to_string(), (Instant::now(), methods.clone()));
        methods
    }

    fn invalidate_pr(&self, key_prefix: &str) {
        lock(&self.prs).retain(|key, _| !key.starts_with(key_prefix));
    }
}

/// `gh repo view --json mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed` → 允许的方式，
/// 按 GitHub 网页上的顺序（merge、squash、rebase）。
pub(crate) fn parse_merge_methods(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    [("mergeCommitAllowed", "merge"), ("squashMergeAllowed", "squash"), ("rebaseMergeAllowed", "rebase")]
        .into_iter()
        .filter(|(key, _)| v.get(key).and_then(|x| x.as_bool()).unwrap_or(false))
        .map(|(_, name)| name.to_string())
        .collect()
}

// ═══ PR 与检查 ═══

const PR_FIELDS: &str = "number,state,isDraft,url,title,baseRefName,headRefName,headRefOid,mergeable,mergeStateStatus,statusCheckRollup";

/// 一项检查 → pass / fail / pending / skipped。CheckRun（GitHub Actions 等）看 status + conclusion；
/// StatusContext（老式 commit status，如 prow 的 tide、CircleCI）看 state。实测样例见 forge/fixtures。
fn check_state(item: &serde_json::Value) -> &'static str {
    let get = |key: &str| item.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_ascii_uppercase();
    if item.get("__typename").and_then(|v| v.as_str()) == Some("StatusContext") {
        return match get("state").as_str() {
            "SUCCESS" => "pass",
            "FAILURE" | "ERROR" => "fail",
            _ => "pending", // PENDING / EXPECTED
        };
    }
    if get("status") != "COMPLETED" {
        return "pending"; // QUEUED / IN_PROGRESS / WAITING / PENDING / REQUESTED
    }
    match get("conclusion").as_str() {
        "SUCCESS" | "NEUTRAL" => "pass",
        "SKIPPED" => "skipped",
        _ => "fail", // FAILURE / CANCELLED / TIMED_OUT / ACTION_REQUIRED / STARTUP_FAILURE / STALE
    }
}

/// 检查列表与汇总。没有任何检查 = `none`（仓库没配 CI，允许合并）；有失败 = fail；有在跑 = pending。
pub(crate) fn map_checks(rollup: Option<&serde_json::Value>) -> (Vec<PrCheckDto>, String) {
    let items = rollup.and_then(|v| v.as_array()).cloned().unwrap_or_default();
    let checks: Vec<PrCheckDto> = items
        .iter()
        .map(|item| {
            let s = |key: &str| item.get(key).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_string);
            PrCheckDto {
                name: s("name").or_else(|| s("context")).unwrap_or_default(),
                workflow: s("workflowName"),
                state: check_state(item).to_string(),
                url: s("detailsUrl").or_else(|| s("targetUrl")),
            }
        })
        .collect();
    let summary = if checks.is_empty() {
        "none"
    } else if checks.iter().any(|c| c.state == "fail") {
        "fail"
    } else if checks.iter().any(|c| c.state == "pending") {
        "pending"
    } else {
        "pass"
    };
    (checks, summary.to_string())
}

/// `gh pr view --json <PR_FIELDS>` → DTO（含检查）。
pub(crate) fn parse_pr_full(json: &str) -> Option<PullRequestDto> {
    let mut pr = crate::git::parse_pull_request(json)?;
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let s = |key: &str| v.get(key).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
    pr.base_ref = s("baseRefName");
    pr.head_ref = s("headRefName");
    pr.head_oid = s("headRefOid");
    pr.mergeable = s("mergeable");
    pr.merge_state = s("mergeStateStatus");
    let (checks, summary) = map_checks(v.get("statusCheckRollup"));
    pr.checks = checks;
    pr.checks_summary = Some(summary);
    Some(pr)
}

/// 查一个分支（或 PR 号）的 PR。Ok(None) = 没有 PR；Err = 查不了（没登录、断网），调用方保留上一次的结果。
/// 知道 owner/repo 就显式带 `--repo`：fork + upstream 两个远端又没设 gh default repo 时，gh 不带它会
/// 报 set-default（做空审查实测）。带了 --repo 的 `gh pr view` 不再看当前分支，必须把分支名作参数。
fn view_pr(gh: GhExec, caches: &Caches, dir: &str, slug: Option<&str>, target: Option<&str>) -> Result<Option<PullRequestDto>, String> {
    let mut args = vec!["pr", "view"];
    args.extend(target);
    if let (Some(slug), Some(_)) = (slug, target) {
        args.extend(["--repo", slug]);
    }
    args.extend(["--json", PR_FIELDS]);
    let Some(out) = gh(dir, &args) else { return Err("gh/not_installed".into()) };
    if out.ok() {
        return parse_pr_full(&out.stdout).map(Some).ok_or_else(|| "gh/failed: bad json".into());
    }
    if out.both().to_lowercase().contains("no pull requests found") {
        return Ok(None);
    }
    if says_unauthed(&out) {
        caches.mark_unauthed();
        return Err("gh/not_authed".into());
    }
    Err(format!("gh/failed: {}", tail(&out.both(), 4)))
}

// ═══ 仓库信息（本地 git）═══

struct RepoInfo {
    /// 公共 .git 目录（worktree 共用），缓存键、按仓库记的选择用。
    common: String,
    branch: Option<String>,
    /// 推送去向（git_target::resolve）。PR 的 head 分支就是它的远端分支。detached / 没远端时 None。
    target: Option<PushTarget>,
    gh: GhRepo,
    has_remote: bool,
    /// PR 的目标分支（PR 所在仓库的默认分支，`main`）。
    base: Option<String>,
}

/// 一个 GitHub 远端：远端名 + owner/repo。
#[derive(Debug, Clone, PartialEq, Eq)]
struct GhRemote {
    remote: String,
    slug: String,
}

/// PR 在哪个仓库开、head 在哪个仓库（单点判定，见 [`resolve_gh_repo`]）。
#[derive(Debug, Clone, Default)]
struct GhRepo {
    /// PR 所在仓库（gh 的 `--repo`）。没有 GitHub 远端时 None。
    base: Option<GhRemote>,
    /// head 分支所在仓库（推送远端）。和 base 不同就是 fork：`--head <owner>:<分支>`。
    head: Option<GhRemote>,
    /// 多个 GitHub 远端、又没用 gh repo set-default 定过时，界面上可选的远端。
    choices: Vec<GhRemote>,
    /// 还没选：查状态先按 origin 看，开 PR / 合并返回 `gh/ambiguous_repo`，不静默挑一个。
    needs_choice: bool,
}

impl GhRepo {
    fn slug(&self) -> Option<&str> {
        self.base.as_ref().map(|b| b.slug.as_str())
    }
    /// gh 认的 head 写法：同仓库是分支名，fork 是 `<fork owner>:<分支>`（`gh pr create --head`、
    /// `gh pr view <owner>:<分支>` 都认这个写法）。
    fn head_ref(&self, branch: &str) -> String {
        match (&self.base, &self.head) {
            (Some(base), Some(head)) if base.slug != head.slug => {
                format!("{}:{branch}", head.slug.split_once('/').map(|(owner, _)| owner).unwrap_or(&head.slug))
            }
            _ => branch.to_string(),
        }
    }
}

/// PR 所在仓库：
/// 1. 远端上有 `gh-resolved = base`（`gh repo set-default`、`gh repo fork --clone` 写的）就用它，与 gh 一致；
/// 2. 用户在 Meowo 里选过（按仓库记在 forge.json）；
/// 3. 只有一个 GitHub 远端就是它；
/// 4. 多个又都没定：先按 origin（没有就第一个）查状态，标 needs_choice，界面给出可选列表。
///
/// 做空第二轮实测：以前一律 `--repo` 推送远端（origin），fork 工作流 set-default 到 upstream 后，PR 开进了自己的 fork。
fn resolve_gh_repo(cfg: &GitConfig, chosen: Option<&str>, target: Option<&PushTarget>) -> GhRepo {
    let github: Vec<GhRemote> = cfg
        .remotes()
        .into_iter()
        .filter_map(|remote| {
            let slug = github_slug(cfg.get("remote", Some(&remote), "url")?)?;
            Some(GhRemote { remote, slug })
        })
        .collect();
    let by_name = |name: &str| github.iter().find(|g| g.remote == name).cloned();
    let resolved = github
        .iter()
        .find(|g| cfg.get("remote", Some(&g.remote), "gh-resolved") == Some("base"))
        .cloned();
    let mut repo = GhRepo::default();
    repo.base = match (&resolved, chosen.and_then(by_name)) {
        (Some(base), _) => Some(base.clone()),
        (None, Some(base)) => Some(base),
        (None, None) if github.len() <= 1 => github.first().cloned(),
        (None, None) => {
            repo.needs_choice = true;
            by_name("origin").or_else(|| github.first().cloned())
        }
    };
    if resolved.is_none() && github.len() > 1 {
        repo.choices = github.clone();
    }
    repo.head = target.and_then(|t| by_name(&t.remote)).or_else(|| repo.base.clone());
    repo
}

/// 公共 .git 目录 → 缓存 / 选择记录的键（斜杠统一、不分大小写）。
fn common_key(common_dir: &Path) -> String {
    common_dir.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn repo_info(dir: &str, env: GitEnv, prefs: &Path) -> Option<RepoInfo> {
    let (mut info, _, _) = repo_head(dir, env, prefs, false)?;
    info.base = default_branch(dir, info.gh.base.as_ref().map(|b| b.remote.as_str()), env);
    Some(info)
}

/// 不含默认分支（base 为 None）的 [`repo_info`]，另回 (公共 .git 目录, 本 worktree 的 .git 目录)。
/// 侧栏只要 [`pr_key`]，键里没有 base；默认分支要再起 2～4 个 git 进程（远端 HEAD、main、master 逐个问）。
/// head_file = 先直接读 .git/HEAD 拿分支名（侧栏用，省一个进程），读不懂再问 symbolic-ref。
fn repo_head(dir: &str, env: GitEnv, prefs: &Path, head_file: bool) -> Option<(RepoInfo, PathBuf, PathBuf)> {
    let (common_dir, git_dir) = git_dirs(dir, env)?;
    let common = common_key(&common_dir);
    let branch = match head_file.then(|| branch_from_head_file(&git_dir)).flatten() {
        Some(read) => read,
        None => git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"], env).filter(|b| !b.is_empty()),
    };
    let cfg = load_config(&common_dir, &git_dir, env);
    let target = branch.as_deref().and_then(|b| resolve(&cfg, &common_dir, b));
    let gh = resolve_gh_repo(&cfg, chosen_remote(prefs, &common).as_deref(), target.as_ref());
    let info = RepoInfo { common, branch, target, gh, has_remote: !cfg.remotes().is_empty(), base: None };
    Some((info, common_dir, git_dir))
}

/// 从 .git/HEAD 读当前分支：Some(Some(分支)) / Some(None) = detached；None = 读不懂，交给 symbolic-ref。
/// reftable 仓库的 HEAD 是占位（`ref: refs/heads/.invalid`），一律交回 git。与 `symbolic-ref --short` 只在
/// 分支名同时也是别的引用短名（歧义，git 会写成 heads/x）时不同，这时读出的是真分支名。
fn branch_from_head_file(git_dir: &Path) -> Option<Option<String>> {
    if git_dir.join("reftable").exists() {
        return None;
    }
    let text = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let text = text.trim();
    if let Some(name) = text.strip_prefix("ref: refs/heads/") {
        return (!name.is_empty()).then(|| Some(name.to_string()));
    }
    let detached = matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit());
    detached.then_some(None)
}

/// 默认分支名：PR 所在远端的 HEAD 指向的，否则它的 main、master；再看 origin；最后本地的 main、master。
fn default_branch(dir: &str, remote: Option<&str>, env: GitEnv) -> Option<String> {
    let remotes: Vec<&str> = remote.into_iter().chain((remote != Some("origin")).then_some("origin")).collect();
    for remote in &remotes {
        if let Some(head) = git(dir, &["symbolic-ref", "-q", "--short", &format!("refs/remotes/{remote}/HEAD")], env) {
            if let Some(name) = head.strip_prefix(&format!("{remote}/")) {
                return Some(name.to_string());
            }
        }
        for name in ["main", "master"] {
            if git(dir, &["rev-parse", "-q", "--verify", &format!("refs/remotes/{remote}/{name}")], env).is_some() {
                return Some(name.to_string());
            }
        }
    }
    for name in ["main", "master"] {
        if git(dir, &["rev-parse", "-q", "--verify", &format!("refs/heads/{name}")], env).is_some() {
            return Some(name.to_string());
        }
    }
    None
}

/// 远端地址 → GitHub 的 owner/repo。认 https / ssh / scp 三种写法；GitHub Enterprise 与 ssh 主机别名不认
/// （按 not_github 降级：push 照常，只是不显示 PR 区块）。
pub(crate) fn github_slug(url: &str) -> Option<String> {
    let url = url.trim();
    let path = if let Some(rest) = url.strip_prefix("git@github.com:").or_else(|| url.strip_prefix("github.com:")) {
        rest
    } else {
        let (_, after) = url.split_once("://")?;
        let (authority, path) = after.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?.to_ascii_lowercase();
        if !matches!(host.as_str(), "github.com" | "www.github.com" | "ssh.github.com") {
            return None;
        }
        path
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    (!owner.is_empty() && !repo.is_empty() && !repo.contains('/')).then(|| format!("{owner}/{repo}"))
}

/// compare 网页的分支段：路径里能原样出现的字符之外都转义（分支名里的 `/` 保留，GitHub 认）。
fn url_path_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 开 PR 的默认值：base..HEAD 只有一个提交时取它的标题，否则用分支名；正文是提交列表。
/// base 用 PR 所在远端的默认分支（本地 main 可能落后），再看 origin 的，没有就本地的。
pub(crate) fn pr_defaults(dir: &str, branch: &str, base: &str, base_remote: Option<&str>, env: GitEnv) -> PrDefaultsDto {
    let range_base = base_remote
        .filter(|r| *r != "origin")
        .map(|r| format!("refs/remotes/{r}/{base}"))
        .into_iter()
        .chain([format!("refs/remotes/origin/{base}"), format!("refs/heads/{base}")])
        .find(|r| git(dir, &["rev-parse", "-q", "--verify", r], env).is_some());
    let subjects: Vec<String> = range_base
        .and_then(|b| git(dir, &["log", "--reverse", "--format=%s", "--no-merges", "-n", "50", &format!("{b}..HEAD")], env))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let title = if subjects.len() == 1 { subjects[0].clone() } else { branch.to_string() };
    let body = subjects.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n");
    PrDefaultsDto { title, body, base: base.to_string() }
}

// ═══ 本机记住的选择：上次的合并方式（按 owner/repo）、PR 所在仓库（按公共 .git 目录）═══

/// 记在 `~/.meowo/forge.json`，不进 settings.json：settings 由设置窗口整份回写（set_settings），窗口开着期间
/// 记下的值会被旧快照盖掉，要再加一条「独立维护字段」的保护；单独一个小文件没有这个坑。
fn prefs_path() -> PathBuf {
    crate::db_path().with_file_name("forge.json")
}

fn read_pref(path: &Path, field: &str, key: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get(field)?.get(key)?.as_str().map(str::to_string)
}

fn save_pref(path: &Path, field: &str, key: &str, value: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = lock(&LOCK);
    let mut v: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(|v: &serde_json::Value| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    if !v.get(field).is_some_and(|m| m.is_object()) {
        v[field] = serde_json::json!({});
    }
    v[field][key] = serde_json::Value::String(value.to_string());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string_pretty(&v) {
        let _ = crate::fsutil::write_atomic(path, &body);
    }
}

pub(crate) fn last_merge_method(path: &Path, slug: &str) -> Option<String> {
    read_pref(path, "lastMergeMethod", slug)
}

pub(crate) fn save_merge_method(path: &Path, slug: &str, method: &str) {
    save_pref(path, "lastMergeMethod", slug, method);
}

/// 用户在界面上选的 PR 所在远端（多个 GitHub 远端、没用 gh repo set-default 定过时）。
fn chosen_remote(path: &Path, common: &str) -> Option<String> {
    read_pref(path, "repoRemote", common)
}

// ═══ 状态 / 开 PR / 合并 ═══

/// 一次调用要用到的外部依赖。正式命令用 [`Ctx::real`]，测试换成录制的 gh 与临时目录。
pub(crate) struct Ctx<'a> {
    pub(crate) gh: GhExec<'a>,
    pub(crate) caches: &'a Caches,
    pub(crate) prefs: PathBuf,
    pub(crate) env: GitEnv<'a>,
}

impl Ctx<'static> {
    fn real() -> Self {
        Ctx { gh: &real_gh, caches: &CACHES, prefs: prefs_path(), env: &[] }
    }
}

/// PR 缓存键：公共 .git 目录 + PR 所在仓库 + head。换了仓库（界面上选了别的远端）就是另一份缓存。
fn pr_key(info: &RepoInfo) -> Option<String> {
    Some(format!("{}\u{0}{}\u{0}{}", info.common, info.gh.slug()?, info.head_ref()?))
}

impl RepoInfo {
    /// PR 的 head 分支：推送去的远端分支（跟踪 origin/bugfix-123 的 fix 就是 bugfix-123）。
    fn head_branch(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.branch.as_str()).or(self.branch.as_deref())
    }
    /// gh 认的 head（fork 时带 owner）。
    fn head_ref(&self) -> Option<String> {
        self.head_branch().map(|b| self.gh.head_ref(b))
    }
    /// (head, base)：同一仓库里 head 就是目标分支时没有 PR 可开（本地就在默认分支上），返回 None。
    fn pr_branches(&self) -> Option<(&str, &str)> {
        let head = self.head_branch()?;
        let base = self.base.as_deref()?;
        let same_repo = self.gh.base.as_ref().map(|b| &b.slug) == self.gh.head.as_ref().map(|h| &h.slug);
        (head != base || !same_repo).then_some((head, base))
    }
}

/// push、开 PR、合并之后作废 PR 缓存（同一个公共 .git 目录下的全部分支）。
fn invalidate_after_write(dir: &str) {
    if let Some(common) = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"], &[]) {
        CACHES.invalidate_pr(&format!("{}\u{0}", common.replace('\\', "/").to_lowercase()));
    }
}

fn gh_label(state: GhState) -> &'static str {
    match state {
        GhState::Missing => "missing",
        GhState::Unauthed => "unauthenticated",
        GhState::Ok => "ok",
    }
}

/// 「更改」面板 GitHub 区块的数据。force：窗口重新聚焦 / 手动刷新，跳过 60s 缓存（但最短 10s）并重查登录状态。
pub(crate) fn forge_status_with(ctx: &Ctx, cwd: &str, force: bool) -> Result<ForgeStatusDto, String> {
    let dir = existing_dir(cwd)?;
    let info = repo_info(dir, ctx.env, &ctx.prefs).ok_or("worktree/not_repo")?;
    let mut dto = ForgeStatusDto {
        gh: "no_remote".into(),
        gh_too_old: false,
        repo_url: None,
        compare_url: None,
        merge_methods: Vec::new(),
        last_merge_method: None,
        pr: None,
        pr_defaults: None,
        repo_choices: Vec::new(),
        repo_remote: None,
        repo_needs_choice: false,
    };
    if !info.has_remote {
        return Ok(dto);
    }
    let Some(slug) = info.gh.slug().map(str::to_string) else {
        dto.gh = "not_github".into();
        return Ok(dto);
    };
    dto.repo_choices = info.gh.choices.iter().map(|c| ForgeRepoChoiceDto { remote: c.remote.clone(), slug: c.slug.clone() }).collect();
    dto.repo_remote = info.gh.base.as_ref().map(|b| b.remote.clone());
    dto.repo_needs_choice = info.gh.needs_choice;
    let repo_url = format!("https://github.com/{slug}");
    if let (Some((head, base)), Some(head_ref)) = (info.pr_branches(), info.head_ref()) {
        // fork 的 head 写 `<owner>:<分支>`：GitHub 的 compare 页就是这个写法，冒号原样保留。
        let head = match head_ref.split_once(':') {
            Some((owner, _)) => format!("{owner}:{}", url_path_escape(head)),
            None => url_path_escape(head),
        };
        dto.compare_url = Some(format!("{repo_url}/compare/{}...{head}?expand=1", url_path_escape(base)));
    }
    dto.repo_url = Some(repo_url);
    dto.last_merge_method = last_merge_method(&ctx.prefs, &slug);
    let state = ctx.caches.gh_state(ctx.gh, force);
    dto.gh = gh_label(state).into();
    let base_remote = info.gh.base.as_ref().map(|b| b.remote.clone());
    let defaults = |info: &RepoInfo| {
        info.pr_branches().map(|(head, base)| pr_defaults(dir, head, base, base_remote.as_deref(), ctx.env))
    };
    if state != GhState::Ok {
        dto.pr_defaults = defaults(&info);
        return Ok(dto);
    }
    dto.gh_too_old = ctx.caches.gh_too_old(ctx.gh);
    dto.merge_methods = ctx.caches.merge_methods(ctx.gh, dir, &slug);
    if let Some(key) = pr_key(&info) {
        let cached = lock(&ctx.caches.prs).get(&key).cloned();
        let ttl = if force { FORCE_MIN } else { PR_TTL };
        let pr = match cached {
            Some((at, pr)) if at.elapsed() < ttl => pr,
            _ => match view_pr(ctx.gh, ctx.caches, dir, Some(&slug), info.head_ref().as_deref()) {
                Ok(pr) => {
                    lock(&ctx.caches.prs).insert(key, (Instant::now(), pr.clone()));
                    pr
                }
                // 查不了（断网、限流）：保留上一次的，PR 区块不会一闪而没。
                Err(_) => cached.and_then(|(_, pr)| pr),
            },
        };
        dto.pr = pr.map(|mut pr| {
            let mark = (slug.clone(), pr.number, pr.head_oid.clone().unwrap_or_default());
            pr.queued = pr.state == "open" && lock(&ctx.caches.queued).contains(&mark);
            pr
        });
        if ctx.caches.gh_state(ctx.gh, false) == GhState::Unauthed {
            dto.gh = "unauthenticated".into();
        }
    }
    if dto.pr.is_none() {
        dto.pr_defaults = defaults(&info);
    }
    Ok(dto)
}

#[tauri::command]
pub(crate) async fn git_forge_status(cwd: String, force: Option<bool>) -> Result<ForgeStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || forge_status_with(&Ctx::real(), &cwd, force.unwrap_or(false)))
        .await
        .map_err(|e| e.to_string())?
}

fn require_gh(ctx: &Ctx) -> Result<(), String> {
    match ctx.caches.gh_state(ctx.gh, false) {
        GhState::Missing => Err("gh/not_installed".into()),
        GhState::Unauthed => Err("gh/not_authed".into()),
        GhState::Ok => Ok(()),
    }
}

/// gh 的失败输出 → `gh/代码`。
pub(crate) fn classify_gh_failure(out: &CmdOut, pr_url: Option<&str>) -> String {
    let both = out.both();
    let low = both.to_lowercase();
    let has = |needle: &str| low.contains(needle);
    if out.timed_out {
        return "gh/failed: timeout".into();
    }
    if says_unauthed(out) {
        return "gh/not_authed".into();
    }
    if has("head branch was modified") || has("head ref must match") || (has("match-head-commit") && has("does not match")) {
        return "gh/head_changed".into();
    }
    if has("merge conflict") || has("cannot be cleanly created") || has("conflicts") {
        return format!("gh/merge_conflict: {}", pr_url.unwrap_or_default());
    }
    if has("approving review") || has("review required") || has("changes requested") {
        return "gh/review_required".into();
    }
    if has("not allowed on this repository") || has("merges are not allowed") || (has("merge method") && has("not allowed")) {
        return "gh/method_not_allowed".into();
    }
    if has("not mergeable") || has("base branch policy") || has("required status check") || has("is not up to date") {
        return format!("gh/merge_blocked: {}", tail(&both, 3));
    }
    format!("gh/failed: {}", tail(&both, 4))
}

/// 开 PR。有没推的提交先推（新分支第一次推会设上游）；标题、正文空着就用默认值。
/// 已经有 PR：返回 `gh/pr_exists: <链接>`，前端刷新状态就能看到那个 PR。
pub(crate) fn pr_create_with(
    ctx: &Ctx,
    cwd: &str,
    title: Option<&str>,
    body: Option<&str>,
    base: Option<&str>,
    draft: bool,
    push: &dyn Fn(&str) -> Result<PushResultDto, String>,
) -> Result<PullRequestDto, String> {
    let dir = existing_dir(cwd)?;
    require_gh(ctx)?;
    let info = repo_info(dir, ctx.env, &ctx.prefs).ok_or("worktree/not_repo")?;
    info.branch.as_ref().ok_or("git/detached")?;
    // 多个 GitHub 远端、没定 PR 开在哪：不静默挑 origin，界面拿到这个码就弹出远端列表让用户选。
    if info.gh.needs_choice {
        return Err("gh/ambiguous_repo".into());
    }
    // 远端不是 GitHub（企业版、ssh 别名）时不带 --repo，交给 gh 自己按远端认（同以前）。
    let slug = info.gh.slug().map(str::to_string);
    let base = base.map(str::trim).filter(|b| !b.is_empty()).map(str::to_string).or(info.base.clone()).ok_or("gh/failed: no base")?;
    let summary = crate::git::git_diff_summary_blocking(dir)?;
    if summary.upstream.is_none() || summary.unpushed.unwrap_or(0) > 0 {
        push(dir)?;
    }
    // PR 的 head = 推送去的远端分支（git_target::resolve，与 push 同一个判定）：上游是 origin/main 时是同名分支
    // （第一轮做空：以前 head 成了 main），跟踪 origin/bugfix-123 时是 bugfix-123，fork 时带 owner。
    let head_branch = info.head_branch().ok_or("git/detached")?.to_string();
    let head = info.gh.head_ref(&head_branch);
    let base_remote = info.gh.base.as_ref().map(|b| b.remote.clone());
    let defaults = pr_defaults(dir, &head_branch, &base, base_remote.as_deref(), ctx.env);
    let title = title.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or(defaults.title);
    let body = body.map(str::to_string).unwrap_or(defaults.body);
    let mut args = vec!["pr", "create", "--title", &title, "--body", &body, "--base", &base, "--head", &head];
    if draft {
        args.push("--draft");
    }
    if let Some(slug) = slug.as_deref() {
        args.extend(["--repo", slug]);
    }
    let out = (ctx.gh)(dir, &args).ok_or("gh/not_installed")?;
    if let Some(key) = pr_key(&info) {
        ctx.caches.invalidate_pr(&key);
    }
    if !out.ok() {
        if out.both().contains("already exists") {
            let url = out.both().lines().map(str::trim).find(|l| l.starts_with("https://")).unwrap_or_default().to_string();
            return Err(format!("gh/pr_exists: {url}"));
        }
        if says_unauthed(&out) {
            ctx.caches.mark_unauthed();
        }
        return Err(classify_gh_failure(&out, None));
    }
    let url = out.stdout.lines().map(str::trim).find(|l| l.starts_with("https://")).unwrap_or_default().to_string();
    match view_pr(ctx.gh, ctx.caches, dir, slug.as_deref(), Some(&head)) {
        Ok(Some(pr)) => Ok(pr),
        // 刚建好就查不到（GitHub 偶有延迟）：用 gh 打出的链接拼一个最小的。
        _ => Ok(PullRequestDto {
            number: url.rsplit('/').next().and_then(|n| n.parse().ok()).unwrap_or(0),
            state: if draft { "draft".into() } else { "open".into() },
            url,
            title,
            ..Default::default()
        }),
    }
}

#[tauri::command]
pub(crate) async fn gh_pr_create(
    cwd: String,
    title: Option<String>,
    body: Option<String>,
    base: Option<String>,
    draft: Option<bool>,
) -> Result<PullRequestDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        pr_create_with(&Ctx::real(), &cwd, title.as_deref(), body.as_deref(), base.as_deref(), draft.unwrap_or(false), &|dir| {
            push_with(dir, &[], PUSH_TIMEOUT)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 应用内合并。后端重查一次 PR（不走缓存）再判：头提交没变、检查 none/pass、没冲突、方式是仓库允许的。
pub(crate) fn pr_merge_with(ctx: &Ctx, cwd: &str, number: u32, method: &str, expected_head_oid: &str) -> Result<MergeResultDto, String> {
    let dir = existing_dir(cwd)?;
    require_gh(ctx)?;
    if ctx.caches.gh_too_old(ctx.gh) {
        return Err("gh/too_old".into());
    }
    let info = repo_info(dir, ctx.env, &ctx.prefs).ok_or("worktree/not_repo")?;
    if info.gh.needs_choice {
        return Err("gh/ambiguous_repo".into());
    }
    let slug = info.gh.slug().ok_or("gh/merge_blocked: not github")?.to_string();
    let methods = ctx.caches.merge_methods(ctx.gh, dir, &slug);
    if !methods.iter().any(|m| m == method) {
        return Err("gh/method_not_allowed".into());
    }
    let number_text = number.to_string();
    // 查当前分支的 PR（head 与开 PR 同一个判定，再核对号对不对得上）；detached 时按号查。
    let target = info.branch.as_ref().and_then(|_| info.head_ref()).unwrap_or_else(|| number_text.clone());
    let pr = view_pr(ctx.gh, ctx.caches, dir, Some(&slug), Some(&target))?.ok_or("gh/head_changed")?;
    if pr.number != number {
        return Err("gh/head_changed".into());
    }
    match pr.state.as_str() {
        "merged" => return Ok(MergeResultDto { merged: true, queued: false }),
        "closed" => return Err("gh/merge_blocked: closed".into()),
        "draft" => return Err("gh/merge_blocked: draft".into()),
        _ => {}
    }
    let head = pr.head_oid.clone().unwrap_or_default();
    if head.is_empty() || head != expected_head_oid {
        return Err("gh/head_changed".into());
    }
    match pr.checks_summary.as_deref() {
        Some("fail") => return Err("gh/checks_failed".into()),
        Some("pending") => return Err("gh/checks_pending".into()),
        _ => {}
    }
    if pr.mergeable.as_deref() == Some("CONFLICTING") || pr.merge_state.as_deref() == Some("DIRTY") {
        return Err(format!("gh/merge_conflict: {}", pr.url));
    }
    // GitHub 还在算能不能合（UNKNOWN）：按进行中处理，过一会儿再试（定案设计）。
    if pr.mergeable.as_deref() == Some("UNKNOWN") {
        return Err("gh/checks_pending".into());
    }
    let flag = format!("--{method}");
    // 不加 --delete-branch：远端分支留着，worktree 与本地分支的清理走 worktree.rs 的判定，只提示。
    let args = ["pr", "merge", number_text.as_str(), flag.as_str(), "--match-head-commit", head.as_str(), "--repo", slug.as_str()];
    let out = (ctx.gh)(dir, &args).ok_or("gh/not_installed")?;
    if let Some(key) = pr_key(&info) {
        ctx.caches.invalidate_pr(&key);
    }
    if !out.ok() {
        if says_unauthed(&out) {
            ctx.caches.mark_unauthed();
        }
        return Err(classify_gh_failure(&out, Some(&pr.url)));
    }
    save_merge_method(&ctx.prefs, &slug, method);
    // 仓库开了合并队列时 gh 回的是「will be added to the merge queue」，PR 还是 OPEN：不能说已合并。
    // 只认 gh 的这句原话：合并成功后 GitHub 有延迟，立刻 view 常常还是 OPEN，以前据此标「已入队」并记进
    // queued 缓存，之后刷新一直挂着「在队列里」（做空审查）。gh 没说排队就按已合并处理；这次 view 不进
    // PR 缓存（上面已作废），下一次刷新以服务器为准。
    let said_queue = out.both().to_lowercase().contains("merge queue");
    let after = view_pr(ctx.gh, ctx.caches, dir, Some(&slug), Some(&target)).ok().flatten();
    let queued = said_queue && !after.as_ref().is_some_and(|p| p.state == "merged");
    let merged = !queued;
    if queued {
        lock(&ctx.caches.queued).insert((slug, number, head));
    }
    Ok(MergeResultDto { merged, queued })
}

#[tauri::command]
pub(crate) async fn gh_pr_merge(cwd: String, number: u32, method: String, expected_head_oid: String) -> Result<MergeResultDto, String> {
    tauri::async_runtime::spawn_blocking(move || pr_merge_with(&Ctx::real(), &cwd, number, &method, &expected_head_oid))
        .await
        .map_err(|e| e.to_string())?
}

/// 多个 GitHub 远端、没用 gh repo set-default 定过时，用户在界面上选 PR 开在哪个远端的仓库（不推用户去终端跑
/// gh repo set-default）。按公共 .git 目录记在 forge.json，同一仓库的各 worktree 共用这个选择。
pub(crate) fn choose_repo_with(prefs: &Path, cwd: &str, remote: &str, env: GitEnv) -> Result<(), String> {
    let dir = existing_dir(cwd)?;
    let (common_dir, git_dir) = git_dirs(dir, env).ok_or("worktree/not_repo")?;
    let cfg = load_config(&common_dir, &git_dir, env);
    let github = cfg.remotes().iter().any(|r| r == remote) && cfg.get("remote", Some(remote), "url").and_then(github_slug).is_some();
    if !github {
        return Err("git/no_remote".into());
    }
    save_pref(prefs, "repoRemote", &common_key(&common_dir), remote);
    Ok(())
}

#[tauri::command]
pub(crate) async fn forge_choose_repo(cwd: String, remote: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || choose_repo_with(&prefs_path(), &cwd, &remote, &[]))
        .await
        .map_err(|e| e.to_string())?
}
