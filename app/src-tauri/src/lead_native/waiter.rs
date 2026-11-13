//! `InSessionWaiter`：kimi / gemini（任务 .lead/20261113-lead-tui，C 路）。
//!
//! 在线（TUI 进程还在）：等待器（`meowo-reporter lead wait`，TUI 自己在后台跑）看到答复就退出，
//! CLI 收到「后台任务完成」在空闲时自己开新回合，宿主什么都不推——不往 PTY 打字，草稿与审批框不受影响。
//! 等待器不在（模型没启动它、被用户拒了）时宿主也不代投，状态里如实说「在会话里发一句话即可继续」。
//!
//! 离线（TUI 已退出、Meowo 重启后）：按插件声明的方式把答复续写进同一个会话，每个关卡只做一次：
//! - `PrintMode`（kimi `-S <id> -p`）：后台跑完一轮即退出。`-p` 连接失败时 kimi 提示答复已落历史、
//!   重试会重复，所以不自动重试；且 `-p` 会把这一轮的权限强制成 auto（2.6.0 runV2Print），故只让它
//!   核对并消费关卡，实施留给用户回到会话后继续。
//! - `InteractiveInitialPrompt`（gemini `--resume <id> -i <答复>`）：在 Meowo 终端里重新打开 TUI，
//!   复用现成的托管恢复路径（启动选项回放、账号环境、判重），正常交互模式与审批。
//!
//! 并发守卫：会话进程活着（托管 PTY 在跑 / DB 里的 pid 仍是该 agent / kimi 会话锁的持有者活着）时
//! 一律不走离线路径；kimi 的 `-p` 会抢会话锁（takeOverSessionLock），绝不能和开着的 TUI 并发。
use super::{NativeOutcome, NativeTarget};
use meowo_agent::lead_install::OfflineResume;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 离线续写的记录：每个关卡只尝试一次，状态给面板看。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OfflineRecord {
    pub gate: String,
    /// running / done / failed
    pub state: String,
    pub reason: Option<String>,
    pub at: i64,
}

const OFFLINE_FILE: &str = ".native-offline.json";
/// 非交互续写一轮的上限：只做「核对、记决定、消费关卡」，正常一两分钟内结束。
const PRINT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

fn offline_path(task: &Path) -> PathBuf {
    task.join(OFFLINE_FILE)
}

pub(crate) fn offline_record(task: &Path, gate_id: &str) -> Option<OfflineRecord> {
    let bytes = std::fs::read(offline_path(task)).ok()?;
    serde_json::from_slice::<OfflineRecord>(&bytes)
        .ok()
        .filter(|record| record.gate == gate_id)
}

fn write_record(task: &Path, record: &OfflineRecord) {
    if let Ok(text) = serde_json::to_string(record) {
        let _ = meowo_agent::fsutil::write_atomic(&offline_path(task), &text);
    }
}

fn record(task: &Path, gate: &str, state: &str, reason: Option<String>) {
    write_record(
        task,
        &OfflineRecord {
            gate: gate.into(),
            state: state.into(),
            reason,
            at: crate::now_ms(),
        },
    );
}

/// kimi 的会话锁（2.6.0 SessionLockService：`<home>/locks/sessions/<sessionId>.lock`，JSON 里有持有者 pid）。
/// 读不到锁不算证据；读到且 pid 活着就是在线——宁可不续写，也不和开着的 TUI 抢会话。
fn session_lock_held(data_dir: Option<PathBuf>, native: &str) -> bool {
    let Some(dir) = data_dir else {
        return false;
    };
    let path = dir
        .join("locks")
        .join("sessions")
        .join(format!("{native}.lock"));
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| value.get("pid").and_then(serde_json::Value::as_u64))
        .is_some_and(|pid| meowo_reporter::lead_waiter::process_started(pid as u32).is_some())
}

/// 会话的 CLI 进程此刻是否还在（任一证据成立即在线）。
pub(crate) fn session_online(
    store: &meowo_store::Store,
    session_id: i64,
    provider: &str,
    native: &str,
    app: Option<&tauri::AppHandle>,
) -> bool {
    if let Some(app) = app {
        use tauri::Manager;
        if app
            .try_state::<crate::AppState>()
            .is_some_and(|state| state.ptys.is_managed(session_id))
        {
            return true;
        }
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::terminal::session_agent_alive(store, session_id).unwrap_or(true) {
        return true;
    }
    let profile = store.session_profile(session_id).ok().flatten();
    if session_lock_held(
        crate::profile::data_dir(provider, profile.as_deref()),
        native,
    ) {
        return true;
    }
    // 非 Windows/macOS 没有进程判定：一律按在线处理，不冒险续写。
    !cfg!(any(target_os = "windows", target_os = "macos"))
}

/// 离线续写时交给模型的话。`-p` 这一轮工具审批自动通过，只让它消费关卡。
fn offline_message(message: &str, offline: OfflineResume) -> String {
    match offline {
        OfflineResume::PrintMode(_) => format!(
            "{message}\n（TUI 已退出，这一轮由 Meowo 用非交互模式代跑，工具审批会自动通过：只核对 gate.json、把决定记进 decisions.md、把 status 改成 consumed，然后结束本轮；其余工作等用户回到会话后再继续。）"
        ),
        OfflineResume::InteractiveInitialPrompt(_) => message.to_owned(),
    }
}

fn fill(template: &[&str], session: &str, prompt: &str) -> Vec<String> {
    template
        .iter()
        .map(|part| {
            part.replace("{session}", session)
                .replace("{prompt}", prompt)
        })
        .collect()
}

pub(crate) fn deliver(
    target: &NativeTarget<'_>,
    offline: OfflineResume,
    app: Option<&tauri::AppHandle>,
) -> Result<NativeOutcome, String> {
    let task = target.task;
    if meowo_reporter::lead_waiter::live_waiter(task, target.gate_id).is_some() {
        return Ok(NativeOutcome::WaiterWillWake);
    }
    let store = crate::open_store(target.db)?;
    if session_online(
        &store,
        target.session_id,
        target.provider,
        target.native_session,
        app,
    ) {
        // 在线但没有等待器：不代投（不打字），面板提示在会话里发一句话（native_session_status）。
        return Ok(NativeOutcome::WaiterWillWake);
    }
    if offline_record(task, target.gate_id).is_some() {
        return Ok(NativeOutcome::Delivered); // 每个关卡只续写一次，结果看状态
    }
    let message = offline_message(target.message, offline);
    let gate = meowo_reporter::lead::read_gate(task).ok_or("lead/gate_missing")?;
    match offline {
        OfflineResume::PrintMode(template) => {
            let profile = store.session_profile(target.session_id).ok().flatten();
            let (mut argv, env) =
                crate::terminal::native_print_launch(target.provider, profile.as_deref())
                    .ok_or("lead/native_resume_unavailable")?;
            argv.extend(fill(template, target.native_session, &message));
            record(task, target.gate_id, "running", None);
            // 答复由这一轮送达：标记已送达，免得这一轮里的 Stop hook 再把同一答复送一遍。
            meowo_reporter::lead_waiter::mark_delivered(task, &gate);
            let task = task.to_path_buf();
            let gate_id = target.gate_id.to_owned();
            let cwd = target.cwd.map(str::to_owned);
            std::thread::spawn(move || {
                let (state, reason) = match run_print(&argv, cwd.as_deref(), &env) {
                    Ok(()) => ("done", None),
                    Err(reason) => ("failed", Some(reason)),
                };
                record(&task, &gate_id, state, reason);
            });
            Ok(NativeOutcome::Delivered)
        }
        OfflineResume::InteractiveInitialPrompt(template) => {
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            {
                use tauri::Manager;
                let Some(app) = app else {
                    return Ok(NativeOutcome::WaiterWillWake); // 没有宿主（测试/巡检外）：等下次巡检
                };
                let session = store
                    .get_session(target.session_id)
                    .map_err(|e| e.to_string())?;
                if !crate::session_command::is_safe_id(&session.cc_session_id) {
                    return Err("lead/owner_invalid".into());
                }
                record(task, target.gate_id, "running", None);
                meowo_reporter::lead_waiter::mark_delivered(task, &gate);
                let state = app.state::<crate::AppState>();
                let result = crate::terminal::start_managed_resume_with_args(
                    app.clone(),
                    state.ptys.clone(),
                    target.session_id,
                    target.cwd.map(str::to_owned),
                    session.cc_session_id,
                    target.provider.to_owned(),
                    crate::pty::TerminalSize::new(120, 40),
                    None,
                    fill(template, target.native_session, &message),
                );
                match result {
                    Ok(_) => record(task, target.gate_id, "done", None),
                    Err(error) => {
                        // 没起来就撤掉「已送达」，下次打开会话时 SessionStart/提交 hook 仍会带上答复。
                        let _ = std::fs::remove_file(task.join(".delivered"));
                        record(task, target.gate_id, "failed", Some(error.clone()));
                        return Err(error);
                    }
                }
                Ok(NativeOutcome::Delivered)
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                let _ = (template, message, gate, app);
                Ok(NativeOutcome::WaiterWillWake)
            }
        }
    }
}

/// 面板上的送达状态（state 沿用既有文案键，只新增 native_resuming）：
/// - 离线续写进行中 → native_resuming；失败 → failed + 原因；
/// - 答复已交到会话（等待器退出 / Stop hook / 离线续写已开）→ consumed「模型已收到回答，等待接手关卡」；
/// - 等待器还活着（刚作答、下一秒就会读到）→ sending；
/// - 会话在线但没有等待器 → native_manual「在会话里发一句话即可继续」；
/// - 会话离线、还没续写（等下一次巡检）→ saved。
pub(crate) fn status(
    store: &meowo_store::Store,
    session_id: i64,
    task: &Path,
    gate: &meowo_protocol::ipc::LeadGateDto,
    provider: &str,
    native: &str,
) -> (String, Option<String>) {
    let delivered =
        std::fs::read_to_string(task.join(".delivered")).is_ok_and(|id| id.trim() == gate.id);
    match offline_record(task, &gate.id) {
        Some(record) if record.state == "running" => return ("native_resuming".into(), None),
        Some(record) if record.state == "failed" => return ("failed".into(), record.reason),
        _ => {}
    }
    if delivered {
        return ("consumed".into(), None);
    }
    if meowo_reporter::lead_waiter::live_waiter(task, &gate.id).is_some() {
        return ("sending".into(), None);
    }
    if session_online(store, session_id, provider, native, None) {
        return ("native_manual".into(), None);
    }
    ("saved".into(), None)
}

/// 跑一轮非交互续写。stdin 关掉、不弹控制台窗口；超时就结束它（记失败，不重试：可能已落历史）。
fn run_print(argv: &[String], cwd: Option<&str>, env: &[(String, String)]) -> Result<(), String> {
    let (exe, args) = argv.split_first().ok_or("lead/native_resume_unavailable")?;
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .env("KIMI_CODE_NO_AUTO_UPDATE", "1");
    if let Some(cwd) = cwd.filter(|dir| Path::new(dir).is_dir()) {
        command.current_dir(cwd);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|_| "lead/native_resume_spawn_failed")?;
    let mut stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut text = String::new();
        if let Some(stream) = stderr.as_mut() {
            let _ = stream.take(64 * 1024).read_to_string(&mut text);
        }
        text
    });
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < PRINT_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(500))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("lead/native_resume_timeout".into());
            }
        }
    };
    let tail = reader.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        eprintln!("Lead 离线续写失败（{status}）：{}", tail.trim());
        Err("lead/native_resume_failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_session_and_prompt_without_shell_parsing() {
        let argv = fill(
            &["-S", "{session}", "-p", "{prompt}"],
            "session_1",
            "a \"b\" & c",
        );
        assert_eq!(argv, ["-S", "session_1", "-p", "a \"b\" & c"]);
        let message = offline_message("答复", OfflineResume::PrintMode(&[]));
        assert!(message.starts_with("答复\n") && message.contains("consumed"));
        assert_eq!(
            offline_message("答复", OfflineResume::InteractiveInitialPrompt(&[])),
            "答复"
        );
    }

    #[test]
    fn kimi_session_lock_counts_as_online_only_with_a_live_holder() {
        let dir = std::env::temp_dir().join(format!("meowo-kimi-lock-{}", std::process::id()));
        let locks = dir.join("locks/sessions");
        std::fs::create_dir_all(&locks).unwrap();
        assert!(!session_lock_held(Some(dir.clone()), "session_x"));
        std::fs::write(
            locks.join("session_x.lock"),
            format!("{{\"pid\":{}}}", std::process::id()),
        )
        .unwrap();
        assert!(session_lock_held(Some(dir.clone()), "session_x"));
        std::fs::write(locks.join("session_x.lock"), "{\"pid\":4294967}").unwrap();
        assert!(!session_lock_held(Some(dir.clone()), "session_x"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
