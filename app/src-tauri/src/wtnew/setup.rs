//! setup 脚本在后台跑：会话不等它，会话页顶部显示进度，可看日志、跳过、重跑。
//!
//! 日志写 `~/.meowo/worktree-setup/<任务号>.log`，绝不写进 worktree（写进去就成了未跟踪文件，
//! worktree 清理会因此判成「有新文件没提交」）。

use meowo_protocol::wtnew::WorktreeSetupDto;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ═══ 选 shell（纯函数）═══

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Runner {
    /// PowerShell 7。
    Pwsh(PathBuf),
    /// Windows 自带的 PowerShell 5.1：不认 `&&` / `||`。
    WinPowershell(PathBuf),
    /// Git for Windows 的 bash，或 Unix 的 $SHELL / sh。
    Bash(PathBuf),
    Sh(PathBuf),
}

impl Runner {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Runner::Pwsh(_) => "pwsh",
            Runner::WinPowershell(_) => "powershell",
            Runner::Bash(_) => "bash",
            Runner::Sh(_) => "sh",
        }
    }
}

/// 机器上找得到的 shell。
#[derive(Debug, Clone, Default)]
pub(crate) struct Shells {
    pub(crate) pwsh: Option<PathBuf>,
    pub(crate) win_powershell: Option<PathBuf>,
    pub(crate) git_bash: Option<PathBuf>,
    /// Unix：$SHELL。
    pub(crate) login_shell: Option<PathBuf>,
}

/// 脚本里有没有 `&&` / `||`（PowerShell 5.1 解析不了这两个，会整段报语法错）。
fn uses_chain_operators(script: &str) -> bool {
    script.contains("&&") || script.contains("||")
}

/// 选用哪个 shell 跑脚本。Windows：pwsh 7 优先；只有 5.1 时，脚本含 `&&`/`||` 交给 Git Bash，
/// 不含就用 5.1；都没有报 `worktree/setup_shell_unsupported`。Unix：$SHELL，没有就 /bin/sh。
pub(crate) fn choose_runner(
    windows: bool,
    script: &str,
    shells: &Shells,
) -> Result<Runner, &'static str> {
    if !windows {
        return Ok(match &shells.login_shell {
            Some(p) if p.file_name().is_some_and(|n| n == "bash" || n == "zsh") => {
                Runner::Bash(p.clone())
            }
            Some(p) => Runner::Sh(p.clone()),
            None => Runner::Sh(PathBuf::from("/bin/sh")),
        });
    }
    if let Some(p) = &shells.pwsh {
        return Ok(Runner::Pwsh(p.clone()));
    }
    if uses_chain_operators(script) {
        return shells
            .git_bash
            .clone()
            .map(Runner::Bash)
            .ok_or("worktree/setup_shell_unsupported");
    }
    if let Some(p) = &shells.win_powershell {
        return Ok(Runner::WinPowershell(p.clone()));
    }
    shells
        .git_bash
        .clone()
        .map(Runner::Bash)
        .ok_or("worktree/setup_shell_unsupported")
}

/// 拼出命令行参数。PowerShell 先把错误设成终止：否则中间一条命令失败，后面照跑，
/// 最后一条成功就报 0，用户看到「setup 成功」实际依赖没装上。
/// `$PSNativeCommandUseErrorActionPreference` 让外部命令（bun / npm）退出码非 0 也算错（pwsh 7.3+，
/// 5.1 上只是一个没人读的变量）。bash 同理加 `set -e`。
pub(crate) fn runner_args(runner: &Runner, script: &str) -> Vec<String> {
    match runner {
        Runner::Pwsh(_) | Runner::WinPowershell(_) => vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!(
                "$ErrorActionPreference = 'Stop'; $PSNativeCommandUseErrorActionPreference = $true\n{script}"
            ),
        ],
        Runner::Bash(_) | Runner::Sh(_) => vec!["-lc".into(), format!("set -e\n{script}")],
    }
}

fn runner_path(runner: &Runner) -> &Path {
    match runner {
        Runner::Pwsh(p) | Runner::WinPowershell(p) | Runner::Bash(p) | Runner::Sh(p) => p,
    }
}

/// 在 PATH 里找可执行文件。
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// Git for Windows 的 bash：从 PATH 上的 git.exe 推（`Git\cmd\git.exe` → `Git\bin\bash.exe`）。
/// 不用 PATH 上的 bash.exe：`System32\bash.exe` 是 WSL，跑起来在 Linux 里，路径全对不上。
fn git_bash_from(git: &Path) -> Option<PathBuf> {
    let dir = git.parent()?;
    let root = dir.parent()?;
    [
        root.join("bin").join("bash.exe"),
        root.join("usr").join("bin").join("bash.exe"),
        dir.join("bash.exe"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

pub(crate) fn detect_shells() -> Shells {
    if cfg!(target_os = "windows") {
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let pwsh = which("pwsh.exe").or_else(|| {
            program_files
                .as_ref()
                .map(|p| p.join("PowerShell").join("7").join("pwsh.exe"))
                .filter(|p| p.is_file())
        });
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let win_powershell = Some(
            system_root
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        )
        .filter(|p| p.is_file());
        let git_bash = which("git.exe")
            .and_then(|g| git_bash_from(&g))
            .or_else(|| {
                program_files
                    .as_ref()
                    .map(|p| p.join("Git").join("bin").join("bash.exe"))
                    .filter(|p| p.is_file())
            });
        Shells {
            pwsh,
            win_powershell,
            git_bash,
            login_shell: None,
        }
    } else {
        Shells {
            login_shell: std::env::var_os("SHELL")
                .map(PathBuf::from)
                .filter(|p| p.is_file()),
            ..Default::default()
        }
    }
}

// ═══ 任务 ═══

/// 一次建立对应的 setup 任务。
pub(crate) struct Task {
    pub(crate) id: String,
    /// 创建顺序（同一路径重建过时取最新的）。
    pub(crate) seq: u64,
    pub(crate) path: PathBuf,
    /// 信任键：仓库公共 .git 目录（归一后）。同一仓库的所有 worktree 共用一份信任。
    pub(crate) repo_key: String,
    /// 读配置的 checkout 根目录（建立时选中的那个仓库）。
    pub(crate) config_root: PathBuf,
    pub(crate) log_path: PathBuf,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) inner: Mutex<TaskState>,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskState {
    pub(crate) state: &'static str,
    pub(crate) reason: Option<String>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) started_at: Option<i64>,
    pub(crate) finished_at: Option<i64>,
    pub(crate) script: Option<String>,
    pub(crate) hash: Option<String>,
    pub(crate) source: Option<&'static str>,
    pub(crate) timeout: Duration,
    pub(crate) runner: Option<Runner>,
    /// 正在跑的进程 pid，停止 / 跳过时按它杀进程树。
    pub(crate) pid: Option<u32>,
    /// 每次（重）跑加一。后台线程收尾时核对代数，旧的一轮不能覆盖新一轮的状态。
    pub(crate) generation: u64,
}

static TASKS: Mutex<Option<HashMap<String, Arc<Task>>>> = Mutex::new(None);

pub(crate) fn register(task: Arc<Task>) {
    let mut guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(HashMap::new)
        .insert(task.id.clone(), task);
}

/// 按任务号或 worktree 路径找任务（会话页只知道会话 cwd）。同一路径有多个任务时取最新的。
pub(crate) fn find(task_id: Option<&str>, path: Option<&str>) -> Option<Arc<Task>> {
    let guard = TASKS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.as_ref()?;
    if let Some(id) = task_id {
        return map.get(id).cloned();
    }
    let want = crate::worktree::norm(Path::new(path?));
    map.values()
        .filter(|t| {
            let top = crate::worktree::norm(&t.path);
            crate::worktree::is_under(&want, &top)
        })
        .max_by_key(|t| t.seq)
        .cloned()
}

pub(crate) fn log_dir() -> PathBuf {
    crate::db_path().with_file_name("worktree-setup")
}

/// 日志最后 `max_lines` 行（最多读最后 16KB）。
pub(crate) fn log_tail(path: &Path, max_lines: usize) -> String {
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(16 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

pub(crate) fn dto(task: &Task) -> WorktreeSetupDto {
    let s = task.inner.lock().unwrap_or_else(|e| e.into_inner()).clone();
    WorktreeSetupDto {
        task_id: task.id.clone(),
        path: task.path.to_string_lossy().into_owned(),
        state: s.state.to_string(),
        exit_code: s.exit_code,
        log_tail: log_tail(&task.log_path, 40),
        log_path: task
            .log_path
            .exists()
            .then(|| task.log_path.to_string_lossy().into_owned()),
        started_at: s.started_at,
        finished_at: s.finished_at,
        reason: s.reason,
        script: s.script,
        config_hash: s.hash,
        config_source: s.source.map(str::to_string),
        runner: s.runner.as_ref().map(|r| r.name().to_string()),
    }
}

fn append_log(path: &Path, line: &str) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 起一轮 setup（调用方已确认可以跑：有脚本、已信任、版本支持）。立即返回，后台线程等进程结束。
/// `on_change`：状态变化时通知前端（桌面发事件）；测试里传空。
pub(crate) fn start(task: &Arc<Task>, on_change: Arc<dyn Fn(&str) + Send + Sync>) {
    let (script, runner, timeout, generation) = {
        let mut s = task.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(script) = s.script.clone() else {
            return;
        };
        let Some(runner) = s.runner.clone() else {
            return;
        };
        s.generation += 1;
        s.state = "running";
        s.reason = None;
        s.exit_code = None;
        s.started_at = Some(crate::now_ms());
        s.finished_at = None;
        s.pid = None;
        (script, runner, s.timeout, s.generation)
    };
    append_log(
        &task.log_path,
        &format!(
            "── setup 开始（{}）：{}\n{}\n──",
            runner.name(),
            task.path.display(),
            script
        ),
    );
    let mut cmd = super::command(&runner_path(&runner).to_string_lossy());
    cmd.args(runner_args(&runner, &script))
        .current_dir(&task.path);
    for (k, v) in &task.env {
        cmd.env(k, v);
    }
    // Git Bash 的登录 profile 默认会 cd 到 $HOME，设了 CHERE_INVOKING 才留在当前目录。
    cmd.env("CHERE_INVOKING", "1");
    cmd.stdin(std::process::Stdio::null());
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&task.log_path);
    let (out, err) = match log.and_then(|f| Ok((f.try_clone()?, f))) {
        Ok((a, b)) => (std::process::Stdio::from(a), std::process::Stdio::from(b)),
        Err(_) => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    cmd.stdout(out).stderr(err);
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            append_log(&task.log_path, &format!("启动 shell 失败：{e}"));
            finish(task, generation, "failed", None, None);
            on_change(&task.id);
            return;
        }
    };
    {
        let mut s = task.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.pid = Some(child.id());
    }
    on_change(&task.id);
    let task = task.clone();
    std::thread::spawn(move || {
        let mut child = child;
        let started = std::time::Instant::now();
        let outcome = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status.code()),
                Ok(None) => {}
                Err(e) => break Err(e.to_string()),
            }
            if started.elapsed() >= timeout {
                kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break Err("timeout".into());
            }
            // 被停止 / 跳过：那边已经杀了进程、改了状态，这里只收尸。
            if task
                .inner
                .lock()
                .map(|s| s.generation != generation)
                .unwrap_or(true)
            {
                let _ = child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let (state, code) = match outcome {
            Ok(Some(0)) => ("ok", Some(0)),
            Ok(code) => ("failed", code),
            Err(e) if e == "timeout" => ("timeout", None),
            Err(e) => {
                append_log(&task.log_path, &format!("等待进程失败：{e}"));
                ("failed", None)
            }
        };
        let note = match state {
            "ok" => "── setup 完成".to_string(),
            "timeout" => format!("── setup 超时（{}s），已结束进程", timeout.as_secs()),
            _ => format!(
                "── setup 失败，退出码 {}",
                code.map_or("?".into(), |c| c.to_string())
            ),
        };
        // 被停止后进程刚好自己退出：stop 已经记过了，不再写。
        if task
            .inner
            .lock()
            .map(|s| s.generation == generation && s.state == "running")
            .unwrap_or(false)
        {
            append_log(&task.log_path, &note);
        }
        if finish(&task, generation, state, code, None) {
            on_change(&task.id);
        }
    });
}

/// 收尾：只有代数对得上、且仍在 running 才写（停止 / 跳过 / 重跑已经接管了状态）。
fn finish(
    task: &Task,
    generation: u64,
    state: &'static str,
    code: Option<i32>,
    reason: Option<String>,
) -> bool {
    let mut s = task.inner.lock().unwrap_or_else(|e| e.into_inner());
    if s.generation != generation || s.state != "running" {
        return false;
    }
    s.state = state;
    s.exit_code = code;
    s.reason = reason;
    s.finished_at = Some(crate::now_ms());
    s.pid = None;
    true
}

fn kill_tree(pid: u32) {
    crate::proc::kill_descendants(pid);
    crate::proc::kill_pid(pid);
}

/// 停止或跳过正在跑的一轮（进程树一起结束）。不在跑时：跳过照样记成 skipped（needs_trust、
/// failed 的都能跳过），停止只对 running 有意义。返回状态有没有变。
pub(crate) fn halt(task: &Task, target: &'static str) -> bool {
    let pid = {
        let mut s = task.inner.lock().unwrap_or_else(|e| e.into_inner());
        let running = s.state == "running";
        if target == "stopped" && !running {
            return false;
        }
        if target == "skipped" && s.state == "skipped" {
            return false;
        }
        let pid = s.pid.take();
        s.generation += 1;
        s.state = target;
        s.reason = (target == "skipped").then(|| "user".to_string());
        if running {
            s.finished_at = Some(crate::now_ms());
        }
        pid
    };
    if let Some(pid) = pid {
        kill_tree(pid);
    }
    append_log(
        &task.log_path,
        if target == "stopped" {
            "── 已停止 setup"
        } else {
            "── 已跳过 setup"
        },
    );
    true
}
