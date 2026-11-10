//! 新建会话时「在新 worktree 里开」（包 5，定案见关卡 B-20261110-p5-design）。
//!
//! 两步：
//! 1. [`worktree_create`]：同步做完 prune → 检查目录与分支 → `worktree add` → 复制被忽略的
//!    `.env*` 等文件，立即返回（目标 ≤2s，不含 setup）。同一 requestId 重复调用返回同一结果，
//!    手机网络抖动重试时不会建出第二个 worktree。
//! 2. setup 脚本在后台跑（[`setup`]），前端随后照常调 `new_session`（cwd = 新 worktree），
//!    `new_session` 的语义一点不改。setup 失败不挡会话，会话页顶部能看日志、跳过、重跑。
//!
//! 中途取消：add 之前取消什么都不留；add 之后保留 worktree——新建、没提交过的 worktree 在
//! worktree.rs 的本地判定里算「已合并」，归档后能直接清理。

pub(crate) mod config;
pub(crate) mod copy;
pub(crate) mod origin;
pub(crate) mod setup;

use meowo_protocol::wtnew::{WorktreeCreateDto, WorktreePreviewDto, WorktreeSetupDto};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub(crate) fn command(program: &str) -> Command {
    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
    let mut command = Command::new(program);
    // GUI 进程起控制台子进程会闪黑窗，压掉（同 worktree.rs / git.rs）。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("worktree/git_failed: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ═══ 仓库与命名 ═══

/// 选中目录所在的仓库。
#[derive(Debug, Clone)]
pub(crate) struct Repo {
    /// 选中目录所在 checkout 的根（复制、读配置都从这里）。
    pub(crate) top: PathBuf,
    /// 公共 .git 目录（信任键、worktree 列表都按它）。
    pub(crate) common: PathBuf,
    /// 主检出根目录：选的本身就是 worktree 时也回到主检出，新 worktree 和它放一起、归到它下面。
    pub(crate) origin: PathBuf,
    pub(crate) name: String,
}

pub(crate) fn repo_of(dir: &Path) -> Result<Repo, String> {
    let top =
        git(dir, &["rev-parse", "--show-toplevel"]).map_err(|_| "worktree/not_repo".to_string())?;
    let top = PathBuf::from(top);
    let common = git(dir, &["rev-parse", "--git-common-dir"])
        .map_err(|_| "worktree/not_repo".to_string())?;
    let common = {
        let p = PathBuf::from(&common);
        let p = if p.is_absolute() { p } else { dir.join(p) };
        dunce::canonicalize(&p).unwrap_or(p)
    };
    let origin = if common.file_name().is_some_and(|n| n == ".git") {
        common
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| top.clone())
    } else {
        top.clone()
    };
    let name = origin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    Ok(Repo {
        top,
        common,
        origin,
        name,
    })
}

/// 用途名归一成 kebab 小写：字母数字保留，其余（空格、斜杠、中文标点）变 `-`，首尾和连续的 `-` 收掉。
/// 中文等非 ASCII 字母保留：目录名和分支名都允许，硬转拼音反而让人认不出。
pub(crate) fn kebab(raw: &str) -> String {
    let mut out = String::new();
    for c in raw.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-')
        .chars()
        .take(60)
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// 没填用途时的兜底名 `MMDD-HHmm`（UTC）。前端会按本机时区算好传 `fallback_slug`，这里只防旧前端。
fn utc_slug(now_ms: i64) -> String {
    let secs = now_ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Howard Hinnant 的 civil_from_days。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{m:02}{d:02}-{:02}{:02}", tod / 3600, (tod % 3600) / 60)
}

/// 建在哪：默认和主检出并排 `<父目录>/<仓库名>-<用途>`；设置里可改到 `~/.meowo/worktrees/<仓库名>/<用途>`。
#[derive(Debug, Clone)]
pub(crate) enum Location {
    Sibling,
    Under(PathBuf),
}

/// 一次建立的环境（测试注入，桌面从设置读）。
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) location: Location,
    pub(crate) branch_prefix: String,
    pub(crate) shells: setup::Shells,
    pub(crate) log_dir: PathBuf,
    /// 继承给 setup 的额外环境（代理等，来自 launch_env_for_profile）。
    pub(crate) extra_env: Vec<(String, String)>,
}

fn target_dir(repo: &Repo, opts: &Options, slug: &str) -> PathBuf {
    match &opts.location {
        Location::Sibling => {
            let parent = repo.origin.parent().unwrap_or(&repo.origin);
            parent.join(format!("{}-{}", repo.name, slug))
        }
        Location::Under(root) => root.join(&repo.name).join(slug),
    }
}

fn branch_exists(repo: &Repo, branch: &str) -> bool {
    git(
        &repo.top,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// 已经检出在某个 worktree 里的分支 → 所在目录。
fn checked_out(repo: &Repo) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(list) = git(&repo.top, &["worktree", "list", "--porcelain"]) else {
        return map;
    };
    let mut path = String::new();
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = p.to_string();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            map.insert(b.to_string(), path.clone());
        }
    }
    map
}

/// 基点：本地已有的 `origin/<默认分支>`（不现场 fetch：联网可能卡几十秒，违背 ≤2s），没有就当前 HEAD。
/// 返回（给 git 用的引用，展示名）。
fn base_of(repo: &Repo) -> (String, String) {
    let remote = git(
        &repo.top,
        &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .filter(|r| !r.is_empty())
    .or_else(|| {
        ["refs/remotes/origin/main", "refs/remotes/origin/master"]
            .into_iter()
            .find(|r| git(&repo.top, &["rev-parse", "-q", "--verify", r]).is_ok())
            .map(str::to_string)
    });
    match remote {
        Some(r) => {
            let short = r.strip_prefix("refs/remotes/").unwrap_or(&r).to_string();
            (r, short)
        }
        None => ("HEAD".into(), "HEAD".into()),
    }
}

/// 用户的输入。
#[derive(Debug, Clone, Default)]
pub(crate) struct Request {
    pub(crate) slug: Option<String>,
    pub(crate) branch: Option<String>,
    pub(crate) fallback_slug: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) slug: String,
    pub(crate) branch: String,
    pub(crate) path: PathBuf,
}

fn candidate(repo: &Repo, opts: &Options, slug: &str, branch: &str, n: u32) -> Plan {
    let (slug, branch) = if n <= 1 {
        (slug.to_string(), branch.to_string())
    } else {
        (format!("{slug}-{n}"), format!("{branch}-{n}"))
    };
    let path = target_dir(repo, opts, &slug);
    Plan { slug, branch, path }
}

/// 下一个目录和分支都空着的候选（给用户的建议名）。
fn next_free(
    repo: &Repo,
    opts: &Options,
    slug: &str,
    branch: &str,
    busy: &HashMap<String, String>,
) -> Plan {
    (2..200)
        .map(|n| candidate(repo, opts, slug, branch, n))
        .find(|c| {
            !c.path.exists() && !branch_exists(repo, &c.branch) && !busy.contains_key(&c.branch)
        })
        .unwrap_or_else(|| candidate(repo, opts, slug, branch, 200))
}

/// 定下目录和分支。自动名撞了目录和分支一起加 -2/-3；用户填的名撞了就报错并给建议名，
/// 不悄悄改掉用户写的东西。
pub(crate) fn plan(repo: &Repo, req: &Request, opts: &Options) -> Result<Plan, String> {
    let user_slug = req.slug.as_deref().map(kebab).filter(|s| !s.is_empty());
    let user_branch = req
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(b) = &user_branch {
        if git(&repo.top, &["check-ref-format", "--branch", b]).is_err() {
            return Err("worktree/branch_invalid".into());
        }
    }
    let auto = user_slug.is_none() && user_branch.is_none();
    let slug = user_slug
        .or_else(|| {
            req.fallback_slug
                .as_deref()
                .map(kebab)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| utc_slug(crate::now_ms()));
    let branch = user_branch
        .clone()
        .unwrap_or_else(|| format!("{}{}", opts.branch_prefix, slug));
    let busy = checked_out(repo);
    if auto {
        return Ok((1..200)
            .map(|n| candidate(repo, opts, &slug, &branch, n))
            .find(|c| {
                !c.path.exists() && !branch_exists(repo, &c.branch) && !busy.contains_key(&c.branch)
            })
            .unwrap_or_else(|| candidate(repo, opts, &slug, &branch, 200)));
    }
    let first = candidate(repo, opts, &slug, &branch, 1);
    if busy.contains_key(&first.branch) {
        return Err(format!(
            "worktree/branch_checked_out: {}",
            busy[&first.branch]
        ));
    }
    if first.path.exists() {
        let s = next_free(repo, opts, &slug, &branch, &busy);
        return Err(format!("worktree/dir_exists: {}", s.slug));
    }
    if branch_exists(repo, &first.branch) {
        let s = next_free(repo, opts, &slug, &branch, &busy);
        return Err(format!("worktree/branch_exists: {}", s.branch));
    }
    Ok(first)
}

/// 新建会话页的实时预览。撞名不报错，只把标记带回去，界面上给提示和建议名。
pub(crate) fn preview(
    dir: &Path,
    req: &Request,
    opts: &Options,
) -> Result<WorktreePreviewDto, String> {
    let repo = repo_of(dir)?;
    let (_, base) = base_of(&repo);
    let cfg = config::load(&repo.top);
    let has_setup = cfg
        .setup
        .as_ref()
        .and_then(|s| s.resolve(cfg!(target_os = "windows")))
        .is_some();
    let (plan, dir_exists, branch_exists_flag, branch_checked_out) = match plan(&repo, req, opts) {
        Ok(p) => (p, false, false, false),
        Err(e) => {
            let slug = req
                .slug
                .as_deref()
                .map(kebab)
                .filter(|s| !s.is_empty())
                .unwrap_or_default();
            let branch = req
                .branch
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}{}", opts.branch_prefix, slug));
            let p = candidate(&repo, opts, &slug, &branch, 1);
            (
                p,
                e.starts_with("worktree/dir_exists"),
                e.starts_with("worktree/branch_exists"),
                e.starts_with("worktree/branch_checked_out"),
            )
        }
    };
    Ok(WorktreePreviewDto {
        repo_root: repo.top.to_string_lossy().into_owned(),
        origin_root: repo.origin.to_string_lossy().into_owned(),
        repo_name: repo.name.clone(),
        slug: plan.slug,
        branch: plan.branch,
        path: plan.path.to_string_lossy().into_owned(),
        base,
        dir_exists,
        branch_exists: branch_exists_flag,
        branch_checked_out,
        has_setup,
    })
}

// ═══ 建立 ═══

static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn new_task_id() -> (String, u64) {
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    (format!("wt-{}-{seq}", crate::now_ms()), seq)
}

/// 建 worktree、复制文件、登记 setup 任务。`trusted(仓库键, hash)` 查信任；
/// `on_change(任务号)` 在 setup 状态变化时调用。
pub(crate) fn create(
    dir: &Path,
    req: &Request,
    opts: &Options,
    trusted: &dyn Fn(&str, &str) -> bool,
    on_change: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<WorktreeCreateDto, String> {
    let repo = repo_of(dir)?;
    // 先清掉目录已经被手动删掉的 worktree 记录，否则同名分支会被误判成「已在别处检出」。
    let _ = git(&repo.top, &["worktree", "prune"]);
    let plan = plan(&repo, req, opts)?;
    let (base_ref, base) = base_of(&repo);
    let path_s = plan.path.to_string_lossy().into_owned();
    // --no-track：基点是 origin/main 时 git 默认把新分支的上游设成 origin/main，之后推送、
    // 领先落后的计数就全对着主分支算了。新分支第一次推送时再设自己的上游。
    if let Err(stderr) = git(
        &repo.top,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &plan.branch,
            &path_s,
            &base_ref,
        ],
    ) {
        // add -b 先建分支再建目录（git 2.55 实测：上级路径是个文件时报 could not create leading
        // directories，分支 b1 却留下了），删掉这个孤儿分支。上级目录交给 git 自己建，这样失败只有一处。
        // plan 已确认分支原来不存在，这里删的只可能是这次建的。
        if branch_exists(&repo, &plan.branch) {
            let _ = git(&repo.top, &["branch", "-D", &plan.branch]);
        }
        let _ = git(&repo.top, &["worktree", "prune"]);
        return Err(format!("worktree/add_failed: {stderr}"));
    }

    let cfg = config::load(&repo.top);
    let rules = copy::Rules::new(&cfg.copy);
    let files = copy::plan(&repo.top, &rules);
    let report = copy::copy_files(&repo.top, &plan.path, &files, copy::MAX_TOTAL_BYTES);

    let (task_id, seq) = new_task_id();
    let log_path = opts.log_dir.join(format!("{task_id}.log"));
    let origin_s = repo.origin.to_string_lossy().into_owned();
    let mut env = opts.extra_env.clone();
    for (k, v) in [
        ("MEOWO_ROOT_PATH", origin_s.clone()),
        ("MEOWO_WORKTREE_PATH", path_s.clone()),
        ("MEOWO_BRANCH", plan.branch.clone()),
        ("MEOWO_BASE_REF", base.clone()),
        // Cursor / Conductor 脚本里常用的名字，兼容它们的配置时脚本不用改。
        ("ROOT_WORKTREE_PATH", origin_s.clone()),
        ("CONDUCTOR_ROOT_PATH", origin_s.clone()),
    ] {
        env.push((k.to_string(), v));
    }
    env.extend(cfg.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    let repo_key = crate::worktree::norm(&repo.common);
    let task = Arc::new(setup::Task {
        id: task_id,
        seq,
        path: plan.path.clone(),
        repo_key,
        config_root: repo.top.clone(),
        log_path,
        env,
        inner: Mutex::new(setup::TaskState {
            state: "skipped",
            reason: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
            script: None,
            hash: None,
            source: cfg.source.map(config::Source::as_str),
            timeout: std::time::Duration::from_secs(cfg.timeout_sec),
            runner: None,
            pid: None,
            generation: 0,
        }),
    });
    write_copy_log(&task.log_path, &report);
    setup::register(task.clone());
    arm(&task, &cfg, &opts.shells, trusted, on_change);
    origin::remember(&path_s, &origin_s);
    Ok(WorktreeCreateDto {
        task_id: task.id.clone(),
        path: path_s,
        branch: plan.branch,
        base,
        origin_root: origin_s,
        copied: report.copied.len() as u32,
        copy_skipped: report.skipped.len() as u32,
        setup: setup::dto(&task),
    })
}

fn write_copy_log(log: &Path, report: &copy::CopyReport) {
    use std::io::Write;
    if report.copied.is_empty() && report.skipped.is_empty() {
        return;
    }
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(
            f,
            "── 复制了 {} 个被忽略的文件：{}",
            report.copied.len(),
            report.copied.join(", ")
        );
        if !report.skipped.is_empty() {
            let _ = writeln!(f, "── 超过 100MB 上限没复制：{}", report.skipped.join(", "));
        }
    }
}

/// 按配置把任务摆到起点：没脚本 / 版本过高 → skipped；配置读不懂 / 没有能用的 shell → failed；
/// 没信任过 → needs_trust；否则开跑。重跑也走这里（配置可能改过，要重新判信任）。
fn arm(
    task: &Arc<setup::Task>,
    cfg: &config::Config,
    shells: &setup::Shells,
    trusted: &dyn Fn(&str, &str) -> bool,
    on_change: Arc<dyn Fn(&str) + Send + Sync>,
) {
    let windows = cfg!(target_os = "windows");
    let script = cfg.setup.as_ref().and_then(|s| s.resolve(windows));
    let hash = cfg.hash();
    let decided: Option<(&'static str, &'static str)> = if let Some(err) = &cfg.invalid {
        setup_log(task, &format!("── .meowo/worktree.json 读不懂：{err}"));
        Some(("failed", "worktree/config_invalid"))
    } else if script.is_none() {
        Some(("skipped", "none"))
    } else if cfg.version_too_high() {
        setup_log(
            task,
            &format!(
                "── 配置版本 {} 高于支持的 {}，只复制文件，不跑 setup",
                cfg.version,
                config::SUPPORTED_VERSION
            ),
        );
        Some(("skipped", "worktree/config_version"))
    } else {
        None
    };
    let runner = script
        .as_deref()
        .map(|s| setup::choose_runner(windows, s, shells));
    {
        let mut s = task.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.script = script.clone();
        s.hash = script.is_some().then(|| hash.clone());
        s.source = cfg.source.map(config::Source::as_str);
        s.timeout = std::time::Duration::from_secs(cfg.timeout_sec);
        s.exit_code = None;
        s.started_at = None;
        s.finished_at = None;
        s.runner = runner.as_ref().and_then(|r| r.as_ref().ok().cloned());
        s.generation += 1;
        if let Some((state, reason)) = decided {
            s.state = state;
            s.reason = Some(reason.to_string());
            return;
        }
        if let Some(Err(code)) = &runner {
            s.state = "failed";
            s.reason = Some((*code).to_string());
            drop(s);
            setup_log(
                task,
                "── 找不到能跑这段脚本的 shell：需要 PowerShell 7 或 Git Bash",
            );
            return;
        }
        if !trusted(&task.repo_key, &hash) {
            s.state = "needs_trust";
            s.reason = None;
            return;
        }
    }
    setup::start(task, on_change);
}

fn setup_log(task: &setup::Task, line: &str) {
    use std::io::Write;
    if let Some(parent) = task.log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&task.log_path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 重跑时重新读配置（从建立时读配置的那个 checkout）：用户可能刚改了脚本要再试一次。
fn config_root_for(task: &setup::Task) -> PathBuf {
    task.config_root.clone()
}

/// status / rerun / skip / stop。
pub(crate) fn setup_action(
    task: &Arc<setup::Task>,
    action: &str,
    shells: &setup::Shells,
    trusted: &dyn Fn(&str, &str) -> bool,
    on_change: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<WorktreeSetupDto, String> {
    match action {
        "status" => {}
        "rerun" => {
            let running = task
                .inner
                .lock()
                .map(|s| s.state == "running")
                .unwrap_or(false);
            if running {
                setup::halt(task, "stopped");
            }
            let cfg = config::load(&config_root_for(task));
            arm(task, &cfg, shells, trusted, on_change.clone());
            on_change(&task.id);
        }
        "skip" => {
            if setup::halt(task, "skipped") {
                on_change(&task.id);
            }
        }
        "stop" => {
            if setup::halt(task, "stopped") {
                on_change(&task.id);
            }
        }
        _ => return Err("worktree/bad_action".into()),
    }
    Ok(setup::dto(task))
}

/// 用户在确认框里看过脚本、点了信任：hash 必须和此刻要跑的一致（确认框打开期间配置被改了就拒绝，
/// 让用户看新的那份）。记下信任后开跑。
pub(crate) fn trust_and_run(
    task: &Arc<setup::Task>,
    hash: &str,
    shells: &setup::Shells,
    remember: &dyn Fn(&str, &str) -> Result<(), String>,
    on_change: Arc<dyn Fn(&str) + Send + Sync>,
) -> Result<WorktreeSetupDto, String> {
    let cfg = config::load(&config_root_for(task));
    if cfg.hash() != hash {
        return Err("worktree/trust_stale".into());
    }
    remember(&task.repo_key, hash)?;
    let want = hash.to_string();
    arm(task, &cfg, shells, &|_, h| h == want, on_change.clone());
    on_change(&task.id);
    Ok(setup::dto(task))
}

// ═══ 幂等 ═══

type Slot = Arc<Mutex<Option<WorktreeCreateDto>>>;
static REQUESTS: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);

/// 同一 requestId 只建一次：并发的第二次等第一次做完，拿同一份结果（setup 状态取此刻的）。
/// 失败不记：用户改了名字再点，应该真的再试一次。
pub(crate) fn idempotent(
    request_id: &str,
    f: impl FnOnce() -> Result<WorktreeCreateDto, String>,
) -> Result<WorktreeCreateDto, String> {
    if request_id.trim().is_empty() {
        return f();
    }
    let slot = {
        let mut guard = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() > 256 {
            map.retain(|_, s| s.lock().map(|v| v.is_some()).unwrap_or(false));
        }
        map.entry(request_id.to_string()).or_default().clone()
    };
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(done) = guard.as_ref() {
        let mut done = done.clone();
        if let Some(task) = setup::find(Some(&done.task_id), None) {
            done.setup = setup::dto(&task);
        }
        return Ok(done);
    }
    let result = f();
    if let Ok(dto) = &result {
        *guard = Some(dto.clone());
    }
    result
}

// ═══ Tauri 命令 ═══

fn desktop_options(provider: Option<&str>) -> Options {
    let settings = crate::settings::load_settings();
    let wt = &settings.worktree;
    let location = if wt.location == "meowo" {
        Location::Under(crate::db_path().with_file_name("worktrees"))
    } else {
        Location::Sibling
    };
    Options {
        location,
        branch_prefix: wt.branch_prefix.clone().unwrap_or_else(|| "meowo/".into()),
        shells: setup::detect_shells(),
        log_dir: setup::log_dir(),
        extra_env: crate::terminal::launch_env_for_profile(provider, None),
    }
}

fn desktop_trusted(repo_key: &str, hash: &str) -> bool {
    crate::settings::load_settings()
        .worktree
        .trusted
        .get(repo_key)
        .is_some_and(|h| h == hash)
}

fn desktop_remember(repo_key: &str, hash: &str) -> Result<(), String> {
    let (k, h) = (repo_key.to_string(), hash.to_string());
    crate::settings::update_settings(move |s| {
        s.worktree.trusted.insert(k, h);
        Ok(())
    })
}

fn notifier(app: &tauri::AppHandle) -> Arc<dyn Fn(&str) + Send + Sync> {
    use tauri::Emitter;
    let app = app.clone();
    Arc::new(move |task_id: &str| {
        let _ = app.emit("worktree-setup-changed", task_id.to_string());
    })
}

fn request(slug: Option<String>, branch: Option<String>, fallback_slug: Option<String>) -> Request {
    Request {
        slug,
        branch,
        fallback_slug,
    }
}

#[tauri::command]
pub(crate) async fn worktree_preview(
    repo_cwd: String,
    slug: Option<String>,
    branch: Option<String>,
    fallback_slug: Option<String>,
) -> Result<WorktreePreviewDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = crate::terminal::validate_new_session_cwd(&repo_cwd)?;
        preview(
            Path::new(&dir),
            &request(slug, branch, fallback_slug),
            &desktop_options(None),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // 参数形状即前端 invoke 契约（同 new_session 的豁免理由）
pub(crate) async fn worktree_create(
    app: tauri::AppHandle,
    repo_cwd: String,
    slug: Option<String>,
    branch: Option<String>,
    fallback_slug: Option<String>,
    provider: Option<String>,
    request_id: String,
) -> Result<WorktreeCreateDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        idempotent(&request_id, || {
            let dir = crate::terminal::validate_new_session_cwd(&repo_cwd)?;
            create(
                Path::new(&dir),
                &request(slug, branch, fallback_slug),
                &desktop_options(provider.as_deref()),
                &desktop_trusted,
                notifier(&app),
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 没有任务（不是 Meowo 建的 worktree，或 Meowo 重启过）返回 None：会话页就不显示进度条。
#[tauri::command]
pub(crate) async fn worktree_setup(
    app: tauri::AppHandle,
    task_id: Option<String>,
    path: Option<String>,
    action: Option<String>,
) -> Result<Option<WorktreeSetupDto>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(task) = setup::find(task_id.as_deref(), path.as_deref()) else {
            return Ok(None);
        };
        let action = action.unwrap_or_else(|| "status".into());
        // 只有 rerun 用得到 shell 探测，status 每秒轮询不该每次都扫一遍 PATH。
        let shells = if action == "rerun" {
            setup::detect_shells()
        } else {
            setup::Shells::default()
        };
        setup_action(&task, &action, &shells, &desktop_trusted, notifier(&app)).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn worktree_setup_trust(
    app: tauri::AppHandle,
    path: String,
    hash: String,
) -> Result<WorktreeSetupDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let task = setup::find(None, Some(&path)).ok_or("worktree/no_task")?;
        trust_and_run(
            &task,
            &hash,
            &setup::detect_shells(),
            &desktop_remember,
            notifier(&app),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
