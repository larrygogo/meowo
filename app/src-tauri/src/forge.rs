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

use meowo_protocol::ipc::{
    ForgeStatusDto, GitJobDto, MergeResultDto, PrCheckDto, PrDefaultsDto, PullRequestDto, PushResultDto,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// 有上游就推到上游（`HEAD:<上游分支>`，上游分支名和本地不同也对）；没有就 `-u <远端> HEAD`。
/// 远端优先 remote.pushDefault，再 origin，再唯一的那个。
fn plan_push(dir: &str, env: GitEnv) -> Result<PushPlan, String> {
    let branch = git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"], env)
        .filter(|b| !b.is_empty())
        .ok_or("git/detached")?;
    let remotes: Vec<String> = git(dir, &["remote"], env)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if remotes.is_empty() {
        return Err("git/no_remote".into());
    }
    let up_remote = git(dir, &["config", "--get", &format!("branch.{branch}.remote")], env);
    let up_merge = git(dir, &["config", "--get", &format!("branch.{branch}.merge")], env);
    if let (Some(remote), Some(merge)) = (&up_remote, &up_merge) {
        // 上游是「.」（跟踪的是本地分支）不算远端上游。
        if remotes.iter().any(|r| r == remote) && merge.starts_with("refs/heads/") {
            return Ok(PushPlan {
                remote: remote.clone(),
                branch: branch.clone(),
                refspec: format!("HEAD:{merge}"),
                set_upstream: false,
            });
        }
    }
    let preferred = git(dir, &["config", "--get", "remote.pushDefault"], env).filter(|r| remotes.contains(r));
    let remote = preferred
        .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
        .unwrap_or_else(|| remotes[0].clone());
    Ok(PushPlan { remote, branch, refspec: "HEAD".into(), set_upstream: true })
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
    repo: String,
    dto: GitJobDto,
}

static JOBS: Mutex<Option<HashMap<String, Job>>> = Mutex::new(None);

fn job_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("push-{:x}-{:x}", nanos, SEQ.fetch_add(1, Ordering::Relaxed))
}

/// 起一个后台推送，立刻返回任务号。手机 RPC 有 20–90 秒的超时，首推大仓库会超过它（定案设计「五」）。
/// 同一个仓库已经有推送在跑：返回那个任务号，手机上连点两下不会推两次。
pub(crate) fn start_push_job(cwd: String, run: impl FnOnce(&str) -> Result<PushResultDto, String> + Send + 'static) -> String {
    let repo = cwd.trim().replace('\\', "/").to_lowercase();
    let mut guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    let jobs = guard.get_or_insert_with(Default::default);
    jobs.retain(|_, job| job.dto.state == "running" || job.at.elapsed() < JOB_KEEP);
    if let Some((id, _)) = jobs.iter().find(|(_, job)| job.repo == repo && job.dto.state == "running") {
        return id.clone();
    }
    let id = job_id();
    jobs.insert(
        id.clone(),
        Job { at: Instant::now(), repo, dto: GitJobDto { state: "running".into(), push: None, error: None } },
    );
    drop(guard);
    let job = id.clone();
    std::thread::spawn(move || {
        let result = run(&cwd);
        let mut guard = JOBS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_or_insert_with(Default::default).get_mut(&job) {
            entry.at = Instant::now();
            entry.dto = match result {
                Ok(push) => GitJobDto { state: "ok".into(), push: Some(push), error: None },
                Err(error) => GitJobDto { state: "failed".into(), push: None, error: Some(error) },
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

/// 查当前分支的 PR。Ok(None) = 没有 PR；Err = 查不了（没登录、断网），调用方保留上一次的结果。
fn view_pr(gh: GhExec, caches: &Caches, dir: &str) -> Result<Option<PullRequestDto>, String> {
    let Some(out) = gh(dir, &["pr", "view", "--json", PR_FIELDS]) else { return Err("gh/not_installed".into()) };
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
    /// 公共 .git 目录（worktree 共用），缓存键用。
    common: String,
    branch: Option<String>,
    /// GitHub 的 owner/repo；远端不是 GitHub 时 None。
    slug: Option<String>,
    has_remote: bool,
    /// 默认分支名（`main`）。
    base: Option<String>,
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

fn repo_info(dir: &str, env: GitEnv) -> Option<RepoInfo> {
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"], env)?;
    let branch = git(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"], env).filter(|b| !b.is_empty());
    let remotes: Vec<String> = git(dir, &["remote"], env).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    // 看哪个远端：分支的上游远端，否则 origin，否则第一个（与 plan_push 一致）。
    let remote = branch
        .as_ref()
        .and_then(|b| git(dir, &["config", "--get", &format!("branch.{b}.remote")], env))
        .filter(|r| remotes.contains(r))
        .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
        .or_else(|| remotes.first().cloned());
    let slug = remote.as_ref().and_then(|r| git(dir, &["remote", "get-url", r], env)).and_then(|u| github_slug(&u));
    let base = default_branch(dir, env);
    Some(RepoInfo { common: common.replace('\\', "/").to_lowercase(), branch, slug, has_remote: !remotes.is_empty(), base })
}

/// 默认分支名：origin/HEAD 指向的，否则 origin/main、origin/master，否则 main。
fn default_branch(dir: &str, env: GitEnv) -> Option<String> {
    if let Some(head) = git(dir, &["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"], env) {
        if let Some(name) = head.strip_prefix("origin/") {
            return Some(name.to_string());
        }
    }
    for name in ["main", "master"] {
        if git(dir, &["rev-parse", "-q", "--verify", &format!("refs/remotes/origin/{name}")], env).is_some() {
            return Some(name.to_string());
        }
    }
    for name in ["main", "master"] {
        if git(dir, &["rev-parse", "-q", "--verify", &format!("refs/heads/{name}")], env).is_some() {
            return Some(name.to_string());
        }
    }
    None
}

/// 开 PR 的默认值：base..HEAD 只有一个提交时取它的标题，否则用分支名；正文是提交列表。
/// base 用远端的默认分支（本地 main 可能落后），没有就本地的。
pub(crate) fn pr_defaults(dir: &str, branch: &str, base: &str, env: GitEnv) -> PrDefaultsDto {
    let range_base = [format!("refs/remotes/origin/{base}"), format!("refs/heads/{base}")]
        .into_iter()
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

// ═══ 上次的合并方式（按 owner/repo 记在本机）═══

/// 记在 `~/.meowo/forge.json`，不进 settings.json：settings 由设置窗口整份回写（set_settings），窗口开着期间
/// 记下的值会被旧快照盖掉，要再加一条「独立维护字段」的保护；单独一个小文件没有这个坑。
fn prefs_path() -> PathBuf {
    crate::db_path().with_file_name("forge.json")
}

pub(crate) fn last_merge_method(path: &Path, slug: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("lastMergeMethod")?.get(slug)?.as_str().map(str::to_string)
}

pub(crate) fn save_merge_method(path: &Path, slug: &str, method: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = lock(&LOCK);
    let mut v: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(|v: &serde_json::Value| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    if !v.get("lastMergeMethod").is_some_and(|m| m.is_object()) {
        v["lastMergeMethod"] = serde_json::json!({});
    }
    v["lastMergeMethod"][slug] = serde_json::Value::String(method.to_string());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(body) = serde_json::to_string_pretty(&v) {
        let _ = crate::fsutil::write_atomic(path, &body);
    }
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

fn pr_key(info: &RepoInfo) -> Option<String> {
    Some(format!("{}\u{0}{}", info.common, info.branch.as_ref()?))
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
    let info = repo_info(dir, ctx.env).ok_or("worktree/not_repo")?;
    let mut dto = ForgeStatusDto {
        gh: "no_remote".into(),
        gh_too_old: false,
        repo_url: None,
        compare_url: None,
        merge_methods: Vec::new(),
        last_merge_method: None,
        pr: None,
        pr_defaults: None,
    };
    if !info.has_remote {
        return Ok(dto);
    }
    let Some(slug) = info.slug.clone() else {
        dto.gh = "not_github".into();
        return Ok(dto);
    };
    let repo_url = format!("https://github.com/{slug}");
    if let (Some(branch), Some(base)) = (&info.branch, &info.base) {
        if branch != base {
            dto.compare_url = Some(format!("{repo_url}/compare/{}...{}?expand=1", url_path_escape(base), url_path_escape(branch)));
        }
    }
    dto.repo_url = Some(repo_url);
    dto.last_merge_method = last_merge_method(&ctx.prefs, &slug);
    let state = ctx.caches.gh_state(ctx.gh, force);
    dto.gh = gh_label(state).into();
    let defaults = |info: &RepoInfo| match (&info.branch, &info.base) {
        (Some(branch), Some(base)) if branch != base => Some(pr_defaults(dir, branch, base, ctx.env)),
        _ => None,
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
            _ => match view_pr(ctx.gh, ctx.caches, dir) {
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
    let info = repo_info(dir, ctx.env).ok_or("worktree/not_repo")?;
    let branch = info.branch.clone().ok_or("git/detached")?;
    let base = base.map(str::trim).filter(|b| !b.is_empty()).map(str::to_string).or(info.base.clone()).ok_or("gh/failed: no base")?;
    let summary = crate::git::git_diff_summary_blocking(dir)?;
    if summary.upstream.is_none() || summary.unpushed.unwrap_or(0) > 0 {
        push(dir)?;
    }
    // 推送可能刚设好上游：PR 的 head 用上游分支名（和本地名不同也对）。
    let head = git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"], ctx.env)
        .and_then(|u| u.split_once('/').map(|(_, b)| b.to_string()))
        .unwrap_or_else(|| branch.clone());
    let defaults = pr_defaults(dir, &branch, &base, ctx.env);
    let title = title.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).unwrap_or(defaults.title);
    let body = body.map(str::to_string).unwrap_or(defaults.body);
    let mut args = vec!["pr", "create", "--title", &title, "--body", &body, "--base", &base, "--head", &head];
    if draft {
        args.push("--draft");
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
    match view_pr(ctx.gh, ctx.caches, dir) {
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
    let info = repo_info(dir, ctx.env).ok_or("worktree/not_repo")?;
    let slug = info.slug.clone().ok_or("gh/merge_blocked: not github")?;
    let methods = ctx.caches.merge_methods(ctx.gh, dir, &slug);
    if !methods.iter().any(|m| m == method) {
        return Err("gh/method_not_allowed".into());
    }
    let pr = view_pr(ctx.gh, ctx.caches, dir)?.ok_or("gh/head_changed")?;
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
    let number_text = number.to_string();
    // 不加 --delete-branch：远端分支留着，worktree 与本地分支的清理走 worktree.rs 的判定，只提示。
    let args = ["pr", "merge", number_text.as_str(), flag.as_str(), "--match-head-commit", head.as_str()];
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
    let said_queue = out.both().to_lowercase().contains("merge queue");
    let after = view_pr(ctx.gh, ctx.caches, dir).ok().flatten();
    let merged = after.as_ref().is_some_and(|p| p.state == "merged");
    let queued = !merged && (said_queue || after.as_ref().is_some_and(|p| p.state == "open"));
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
