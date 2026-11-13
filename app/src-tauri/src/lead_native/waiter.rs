//! `InSessionWaiter`：kimi / gemini（任务 .lead/20261113-lead-tui，C 路）。
//!
//! 在线（TUI 进程还在）：等待器（`meowo-reporter lead wait`，TUI 自己在后台跑）看到答复就退出，
//! CLI 收到「后台任务完成」在空闲时自己开新回合，宿主什么都不推——不往 PTY 打字，草稿与审批框不受影响。
//! 等待器不在（模型没启动它、被用户拒了）时宿主也不代投，状态里如实说「在会话里发一句话即可继续」。
//!
//! 为什么等待器唤醒不会答掉待批审批、不会发到子代理（做空 H3，2026-11-13 读本机 CLI 源码）：
//! - kimi 2.6.0（kimi.exe 内嵌源码 agent loop）：后台任务结束走 completeDetached → drainDetachedCompletions，
//!   把通知当一条 notify 提示排进**启动该任务的那个 agent loop** 的 mailbox（mailbox.queue），只在
//!   canSchedule()（phase === "idle"、未 held、无待完成任务）时才开新回合。审批走权限服务，不读 mailbox；
//!   回合挂在审批上时 phase 不是 idle，通知只排队。等待器只由主 agent 启动：reporter 的 hook 对带
//!   agent_id / agentId≠main 的载荷一律不处理（lead_provider::normalize），子代理拿不到等待命令。
//! - gemini 0.68.0（interactiveCli-ZJ2ZELJM.js:35141 起的空闲 effect）：只在 streamingState === idle、
//!   MCP 就绪、且 !isToolAwaitingConfirmation(pendingHistoryItems) 时才把 background_completion 注入
//!   提交成新回合；提交的是单独构造的提示，不动输入框缓冲（草稿保留，假模型实测）。注入服务是会话级，
//!   只有主界面消费。**没有守卫的**：/settings、认证等对话框打开时它照样提交（同一 effect 不检查
//!   对话框状态）；等待器在后台进程里看不到 TUI 画面、hook 也拿不到对话框状态，宿主侧做不到事前检测。
//! - 宿主侧：`.owner` 指向的会话在 DB 里找不到（子代理、外部会话）时 lead_conversation 直接不投递，
//!   也就不会走到这里的离线续写。
//!
//! 离线（TUI 已退出、Meowo 重启后）：按插件声明的方式把答复续写进同一个会话，每个关卡只做一次：
//! - `PrintMode`（kimi `-S <id> -p`）：后台跑完一轮即退出。`-p` 会把这一轮的权限强制成 auto
//!   （2.6.0 runV2Print forceAuto，无条件），故只让它核对并消费关卡，实施留给用户回到会话后继续。
//!   kimi 2.6.0 没有「交互式启动并带首条消息」的入口（见 kimi 插件 native_continue 的取证），做不到
//!   gemini 那样的正常权限续写。`-p` 连接失败时 kimi 提示答复可能已落历史、重试会重复，所以失败不自动
//!   重试，只在面板给「重试」由人决定。
//! - `InteractiveInitialPrompt`（gemini `--resume <id> -i <答复>`）：在 Meowo 终端里重新打开 TUI，
//!   复用现成的托管恢复路径（启动选项回放、账号环境、判重），正常交互模式与审批。
//!
//! 并发守卫：
//! - 会话进程活着（托管 PTY 在跑 / DB 里的 pid 仍是该 agent / kimi 会话锁的持有者活着）时一律不走离线
//!   路径；kimi 的 `-p` 会抢会话锁（takeOverSessionLock），绝不能和开着的 TUI 并发。
//! - 答复已交到会话（`.delivered` = 当前关卡：等待器已打印答复 / Stop hook 已送）时不再离线续写（做空 M1：
//!   TUI 拿到答复后、模型消费前被关掉，原先下一次巡检会再 `-p` 或再 `-i` 一遍）。
//! - 离线续写按关卡 `create_new` 原子占位后才起进程（做空 M2：作答入口与 10s 巡检并发会起两个）。
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

/// 每个关卡一个记录文件：占位靠 `create_new`（文件系统层面原子），单文件覆盖写做不到互斥。
const OFFLINE_PREFIX: &str = ".native-offline-";
/// 非交互续写一轮的上限：只做「核对、记决定、消费关卡」，正常一两分钟内结束。
const PRINT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// 关卡 id 来自模型写的 gate.json，进文件名前把 `[A-Za-z0-9_-]` 以外的字符转成 `.xx` 十六进制。
fn offline_path(task: &Path, gate_id: &str) -> PathBuf {
    let mut name = String::from(OFFLINE_PREFIX);
    for byte in gate_id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
            name.push(byte as char);
        } else {
            name.push_str(&format!(".{byte:02x}"));
        }
    }
    name.push_str(".json");
    task.join(name)
}

pub(crate) fn offline_record(task: &Path, gate_id: &str) -> Option<OfflineRecord> {
    let path = offline_path(task, gate_id);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    // 占位者刚 create_new、还没写完内容时读到的是空/半截：按「进行中」算，别人不能再起。
    Some(
        serde_json::from_slice::<OfflineRecord>(&bytes)
            .ok()
            .filter(|record| record.gate == gate_id)
            .unwrap_or_else(|| OfflineRecord {
                gate: gate_id.into(),
                state: "running".into(),
                reason: None,
                at: 0,
            }),
    )
}

fn running(gate: &str) -> OfflineRecord {
    OfflineRecord {
        gate: gate.into(),
        state: "running".into(),
        reason: None,
        at: crate::now_ms(),
    }
}

/// 原子占位：只有第一个调用者拿到 true，之后才能起进程。
fn claim(task: &Path, gate_id: &str) -> bool {
    use std::io::Write;
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(offline_path(task, gate_id))
    else {
        return false;
    };
    if let Ok(text) = serde_json::to_string(&running(gate_id)) {
        let _ = file.write_all(text.as_bytes());
    }
    true
}

/// 面板「重试」：把**失败**的记录挪走（rename 只有一个调用者成功），之后重新 claim。
/// 进行中 / 已完成的记录不动。
fn release_failed(task: &Path, gate_id: &str) {
    if offline_record(task, gate_id).is_some_and(|record| record.state == "failed") {
        let path = offline_path(task, gate_id);
        let mut aside = path.clone().into_os_string();
        aside.push(format!(".retry-{}", crate::now_ms()));
        if std::fs::rename(&path, &aside).is_ok() {
            let _ = std::fs::remove_file(aside);
        }
    }
}

/// 没起来（L3：PTY 已在跑，什么都没做）：撤掉占位，下次巡检按当时的在线/离线重新判断。
fn release_claim(task: &Path, gate_id: &str) {
    let _ = std::fs::remove_file(offline_path(task, gate_id));
}

fn record(task: &Path, gate: &str, state: &str, reason: Option<String>) {
    let record = OfflineRecord {
        gate: gate.into(),
        state: state.into(),
        reason,
        at: crate::now_ms(),
    };
    if let Ok(text) = serde_json::to_string(&record) {
        let _ = meowo_agent::fsutil::write_atomic(&offline_path(task, gate), &text);
    }
}

/// 答复是否已交到会话里（等待器打印了 / Stop hook 送了 / 离线续写已开）。
fn delivered(task: &Path, gate_id: &str) -> bool {
    std::fs::read_to_string(task.join(".delivered")).is_ok_and(|id| id.trim() == gate_id)
}

/// 续写没成：撤掉「已送达」，下次打开会话时 SessionStart/提交 hook 仍会带上答复。只撤本关卡的。
fn revoke_delivered(task: &Path, gate_id: &str) {
    if delivered(task, gate_id) {
        let _ = std::fs::remove_file(task.join(".delivered"));
    }
}

/// 一次 deliver 的输入事实，抽出来便于把草稿/审批/子代理/并发等组合逐条测。
#[derive(Debug, Clone, Copy, Default)]
struct Facts<'a> {
    /// 会话里有活着的等待器在等这个关卡。
    live_waiter: bool,
    /// `.delivered` = 这个关卡。
    delivered: bool,
    /// 会话进程还在（有草稿、挂着审批、跑着子代理都属于这一类：TUI 活着）。
    online: bool,
    /// 这个关卡的离线续写记录状态（running / done / failed）。
    record: Option<&'a str>,
    /// 面板「重试」。
    force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// 交给会话自己（等待器会唤醒，或在线时提示在会话里发一句话）：宿主什么都不做。
    InSession,
    /// 已经送到 / 已经续写过：不再动。
    Settled,
    /// 离线续写（`retry` = 先挪走失败记录）。
    Resume { retry: bool },
}

fn decide(facts: Facts<'_>) -> Step {
    if facts.live_waiter {
        return Step::InSession;
    }
    if facts.delivered {
        return Step::Settled; // M1：模型已收到（或续写已开），不再离线送一遍
    }
    if facts.online {
        return Step::InSession; // 在线一律不代投：不打字、不抢会话锁
    }
    match facts.record {
        None => Step::Resume { retry: false },
        Some("failed") if facts.force => Step::Resume { retry: true },
        Some(_) => Step::Settled, // 每个关卡只自动续写一次；失败等人点重试
    }
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
        OfflineResume::PrintMode { .. } => format!(
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
    let gate_id = target.gate_id;
    let mut facts = Facts {
        live_waiter: meowo_reporter::lead_waiter::live_waiter(task, gate_id).is_some(),
        delivered: delivered(task, gate_id),
        force: target.force,
        ..Facts::default()
    };
    // 前两条不需要开库：等待器在等 / 已送到，直接收手。
    match decide(facts) {
        Step::InSession => return Ok(NativeOutcome::WaiterWillWake),
        Step::Settled => return Ok(NativeOutcome::Delivered),
        Step::Resume { .. } => {}
    }
    let store = crate::open_store(target.db)?;
    facts.online = session_online(
        &store,
        target.session_id,
        target.provider,
        target.native_session,
        app,
    );
    let existing = offline_record(task, gate_id);
    facts.record = existing.as_ref().map(|record| record.state.as_str());
    let retry = match decide(facts) {
        // 在线但没有等待器：不代投（不打字），面板提示在会话里发一句话（status → native_manual）。
        Step::InSession => return Ok(NativeOutcome::WaiterWillWake),
        Step::Settled => return Ok(NativeOutcome::Delivered),
        Step::Resume { retry } => retry,
    };
    let message = offline_message(target.message, offline);
    let gate = meowo_reporter::lead::read_gate(task).ok_or("lead/gate_missing")?;
    match offline {
        OfflineResume::PrintMode { args, env: extra } => {
            let profile = store.session_profile(target.session_id).ok().flatten();
            let (mut argv, mut env) =
                crate::terminal::native_print_launch(target.provider, profile.as_deref())
                    .ok_or("lead/native_resume_unavailable")?;
            argv.extend(fill(args, target.native_session, &message));
            env.extend(
                extra
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
            );
            if retry {
                release_failed(task, gate_id);
            }
            if !claim(task, gate_id) {
                return Ok(NativeOutcome::Delivered); // 另一路（作答入口 / 巡检）已占位在跑
            }
            // 答复由这一轮送达：标记已送达，免得这一轮里的 Stop hook 再把同一答复送一遍。
            meowo_reporter::lead_waiter::mark_delivered(task, &gate);
            let task = task.to_path_buf();
            let gate_id = gate_id.to_owned();
            let cwd = target.cwd.map(str::to_owned);
            std::thread::spawn(move || match run_print(&argv, cwd.as_deref(), &env) {
                Ok(()) => record(&task, &gate_id, "done", None),
                Err(reason) => {
                    // M3：没续写成就撤掉「已送达」，用户回到会话时提交 hook 仍会带上答复；面板给重试。
                    revoke_delivered(&task, &gate_id);
                    record(&task, &gate_id, "failed", Some(reason));
                }
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
                if retry {
                    release_failed(task, gate_id);
                }
                if !claim(task, gate_id) {
                    return Ok(NativeOutcome::Delivered);
                }
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
                    Ok(true) => {
                        record(task, gate_id, "done", None);
                        Ok(NativeOutcome::Delivered)
                    }
                    Ok(false) => {
                        // L3：PTY 已在跑（判重收敛），本次什么都没起——不记 done、不标送达，
                        // 撤掉占位按在线处理：等待器/在会话里发一句话。
                        revoke_delivered(task, gate_id);
                        release_claim(task, gate_id);
                        Ok(NativeOutcome::WaiterWillWake)
                    }
                    Err(error) => {
                        revoke_delivered(task, gate_id);
                        record(task, gate_id, "failed", Some(error.clone()));
                        Err(error)
                    }
                }
            }
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                let _ = (template, message, gate, app, retry);
                Ok(NativeOutcome::WaiterWillWake)
            }
        }
    }
}

/// 面板上的送达状态（state 沿用既有文案键，只新增 native_resuming）：
/// - 离线续写进行中 → native_resuming；失败 → failed + 原因（可重试）；
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
    let record = offline_record(task, &gate.id);
    status_from(
        Facts {
            live_waiter: meowo_reporter::lead_waiter::live_waiter(task, &gate.id).is_some(),
            delivered: delivered(task, &gate.id),
            record: record.as_ref().map(|r| r.state.as_str()),
            ..Facts::default()
        },
        record.as_ref().and_then(|r| r.reason.clone()),
        || session_online(store, session_id, provider, native, None),
    )
}

fn status_from(
    facts: Facts<'_>,
    reason: Option<String>,
    online: impl FnOnce() -> bool,
) -> (String, Option<String>) {
    match facts.record {
        Some("running") => return ("native_resuming".into(), None),
        Some("failed") => return ("failed".into(), reason),
        _ => {}
    }
    if facts.delivered {
        return ("consumed".into(), None);
    }
    if facts.live_waiter {
        return ("sending".into(), None);
    }
    if online() {
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
        .stderr(std::process::Stdio::piped());
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

    fn temp_task(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "meowo-waiter-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn templates_fill_session_and_prompt_without_shell_parsing() {
        let argv = fill(
            &["-S", "{session}", "-p", "{prompt}"],
            "session_1",
            "a \"b\" & c",
        );
        assert_eq!(argv, ["-S", "session_1", "-p", "a \"b\" & c"]);
        let print = OfflineResume::PrintMode {
            args: &[],
            env: &[],
        };
        let message = offline_message("答复", print);
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

    /// 做空 H3：TUI 活着时（输入框里有草稿、挂着待批审批、主 agent 正在跑子代理，宿主看到的都是
    /// 「会话在线」）不论有没有等待器、有没有失败记录、是不是面板重试，一律不离线续写、不代投。
    #[test]
    fn online_session_with_draft_or_pending_approval_is_never_resumed_offline() {
        for live_waiter in [false, true] {
            for record in [None, Some("failed"), Some("done"), Some("running")] {
                for force in [false, true] {
                    let facts = Facts {
                        live_waiter,
                        online: true,
                        record,
                        force,
                        ..Facts::default()
                    };
                    let step = decide(facts);
                    assert!(!matches!(step, Step::Resume { .. }), "{facts:?} → {step:?}");
                }
            }
        }
        // 面板如实说下一步：有等待器 = 发送中；没有 = 在会话里发一句话（不说「已收到」冒充）。
        let manual = status_from(Facts::default(), None, || true);
        assert_eq!(manual.0, "native_manual");
        let sending = status_from(
            Facts {
                live_waiter: true,
                ..Facts::default()
            },
            None,
            || panic!("有等待器时不必查进程"),
        );
        assert_eq!(sending.0, "sending");
    }

    /// 做空 M1：等待器已把答复交给 TUI（.delivered = 本关卡）后用户关了 TUI，离线路径不能再送一遍；
    /// 面板显示模型已收到。
    #[test]
    fn answer_already_handed_to_the_tui_is_not_resumed_again_offline() {
        let facts = Facts {
            delivered: true,
            ..Facts::default()
        };
        assert_eq!(decide(facts), Step::Settled);
        assert_eq!(
            decide(Facts {
                force: true,
                ..facts
            }),
            Step::Settled
        );
        assert_eq!(status_from(facts, None, || false).0, "consumed");

        // 走真实入口：不开库、不起进程、不留续写记录。
        let task = temp_task("m1");
        std::fs::write(task.join(".delivered"), "Q-1").unwrap();
        let target = NativeTarget {
            db: Path::new("Z:/no/such/meowo.db"),
            session_id: 1,
            provider: "kimi",
            native_session: "session_x",
            cwd: None,
            task: &task,
            gate_id: "Q-1",
            message: "答复",
            force: true,
        };
        let print = OfflineResume::PrintMode {
            args: &["-p", "{prompt}"],
            env: &[],
        };
        assert_eq!(deliver(&target, print, None), Ok(NativeOutcome::Delivered));
        assert!(offline_record(&task, "Q-1").is_none());
        // 上一个关卡的 .delivered 不算这个关卡送达过。
        std::fs::write(task.join(".delivered"), "Q-0").unwrap();
        assert!(!delivered(&task, "Q-1"));
        let _ = std::fs::remove_dir_all(task);
    }

    /// 做空 M2：作答入口与巡检并发时只有一个能占位起进程。
    #[test]
    fn offline_resume_claim_is_exclusive_across_threads() {
        let task = temp_task("m2");
        let winners: usize = (0..16)
            .map(|_| {
                let task = task.clone();
                std::thread::spawn(move || claim(&task, "Q-1"))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum();
        assert_eq!(winners, 1);
        assert_eq!(offline_record(&task, "Q-1").unwrap().state, "running");
        // 占位了就算进行中：之后的巡检不再起。
        assert_eq!(
            decide(Facts {
                record: Some("running"),
                ..Facts::default()
            }),
            Step::Settled
        );
        // 别的关卡互不影响；怪字符的关卡 id 进不了路径分隔。
        assert!(claim(&task, "Q-2"));
        assert!(claim(&task, "../x"));
        assert_eq!(offline_path(&task, "../x").parent(), Some(task.as_path()));
        assert_eq!(
            offline_path(&task, "../x").file_name().unwrap(),
            ".native-offline-.2e.2e.2fx.json"
        );
        // 占位者还没写完内容时读到空文件，也按进行中算。
        std::fs::write(offline_path(&task, "Q-3"), "").unwrap();
        assert_eq!(offline_record(&task, "Q-3").unwrap().state, "running");
        let _ = std::fs::remove_dir_all(task);
    }

    /// 做空 M3（kimi 只能走 -p 时的兜底）：失败不自动重试，撤掉「已送达」让用户回到会话时 hook 仍带上
    /// 答复；面板点重试（force）才挪走失败记录重新占位。
    #[test]
    fn failed_offline_resume_waits_for_an_explicit_retry() {
        let task = temp_task("m3");
        record(
            &task,
            "Q-1",
            "failed",
            Some("lead/native_resume_failed".into()),
        );
        let failed = Facts {
            record: Some("failed"),
            ..Facts::default()
        };
        assert_eq!(decide(failed), Step::Settled, "不自动重试：-p 可能已落历史");
        assert_eq!(
            decide(Facts {
                force: true,
                ..failed
            }),
            Step::Resume { retry: true }
        );
        assert_eq!(
            status_from(failed, Some("lead/native_resume_failed".into()), || false),
            ("failed".into(), Some("lead/native_resume_failed".into()))
        );
        // 进行中、已完成的记录不会被重试挪走。
        assert!(!claim(&task, "Q-1"));
        release_failed(&task, "Q-1");
        assert!(claim(&task, "Q-1"), "失败记录挪走后可重新占位");
        release_failed(&task, "Q-1");
        assert!(!claim(&task, "Q-1"), "进行中的不挪");
        // 撤「已送达」只撤本关卡的。
        std::fs::write(task.join(".delivered"), "Q-1").unwrap();
        revoke_delivered(&task, "Q-2");
        assert!(delivered(&task, "Q-1"));
        revoke_delivered(&task, "Q-1");
        assert!(!delivered(&task, "Q-1"));
        let _ = std::fs::remove_dir_all(task);
    }

    /// 做空 L3：恢复入口判重返回「没起新进程」时撤掉占位，下一次巡检能重新判断，而不是记成 done。
    #[test]
    fn released_claim_lets_the_next_patrol_decide_again() {
        let task = temp_task("l3");
        assert!(claim(&task, "Q-1"));
        release_claim(&task, "Q-1");
        assert!(offline_record(&task, "Q-1").is_none());
        assert_eq!(decide(Facts::default()), Step::Resume { retry: false });
        let _ = std::fs::remove_dir_all(task);
    }
}
