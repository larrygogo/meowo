//! `InSessionWaiter`：kimi / gemini（任务 .lead/20261113-lead-tui，C 路）。
//!
//! TUI 开着时：等待器（`meowo-reporter lead wait`，TUI 自己在后台跑）看到答复就退出，
//! CLI 收到「后台任务完成」在空闲时自己开新回合，宿主什么都不推——不往 PTY 打字，草稿与审批框不受影响。
//! 等待器不在（模型没启动它、被用户拒了）时宿主也不代投，状态里如实说「在会话里发一句话即可继续」。
//!
//! TUI 不在时：只保存答复，不起任何后台进程（leader 2026-11-13「不需要离线」：原先 kimi 用 `-S -p`、
//! gemini 用 `--resume -i` 代跑一轮，已删）。面板提示打开会话后说一句话，届时 hook 把答复交给模型
//! （kimi Stop / gemini AfterAgent 的 undelivered_answer）。
//!
//! 为什么等待器唤醒不会答掉待批审批、不会发到子代理（做空 H3，2026-11-13 读本机 CLI 源码）：
//! - kimi 2.6.0（kimi.exe 内嵌源码 agent loop）：后台任务结束走 completeDetached → drainDetachedCompletions，
//!   把通知当一条 notify 提示排进**启动该任务的那个 agent loop** 的 mailbox（mailbox.queue），只在
//!   canSchedule()（phase === "idle"、未 held、无待完成任务）时才开新回合。审批走权限服务，不读 mailbox；
//!   回合挂在审批上时 phase 不是 idle，通知只排队。
//!   **子代理**：原先假设「hook 对带 agent_id 的载荷不处理，子代理拿不到等待命令」不成立——kimi 2.6 的
//!   hook 载荷根本不带 agent 身份，子代理写挂起关卡时照样被要求启动等待器，答复通知进了子代理的 loop
//!   （第二轮真机 run5 D）。现在两道闸：PreToolUse 按主 wire 认出子代理在写、直接拒绝并说明「关卡由主
//!   会话发」（reporter lead_kimi / kimi_gate_guard）；等待器按自己的父进程链对各 agent wire 里的
//!   task.started，认出是子代理启动的就不等、不交付（lead_waiter Guards）。
//! - gemini 0.68.0（interactiveCli-ZJ2ZELJM.js:35141 起的空闲 effect）：只在 streamingState === idle、
//!   MCP 就绪、且 !isToolAwaitingConfirmation(pendingHistoryItems) 时才把 background_completion 注入
//!   提交成新回合；提交的是单独构造的提示，不动输入框缓冲（草稿保留，假模型实测）。注入服务是会话级，
//!   主界面空闲时一定消费（InjectionService 对所有监听者广播，在跑的子代理也会收到一份，但主界面不会漏）。
//!   0.68 子代理与主 agent 共用 hook 载荷，同样分不出谁在写；改按主 agent 的 invoke_agent 起止认，
//!   期间写挂起关卡的 BeforeTool 直接拒绝（reporter gemini_gate_guard）。
//!   **没有守卫的**：/settings、认证等对话框打开时它照样提交（同一 effect 不检查对话框状态）；等待器在后台进程里看不到 TUI 画面、hook 也拿不到对话框状态，宿主侧做不到事前检测。
use super::{NativeOutcome, NativeTarget};
use std::path::{Path, PathBuf};

/// 答复是否已交到会话里（等待器打印了 / Stop hook 送了）。
fn delivered(task: &Path, gate_id: &str) -> bool {
    std::fs::read_to_string(task.join(".delivered")).is_ok_and(|id| id.trim() == gate_id)
}

/// 「已送达」是否作数：等待器交出答复后、模型开新回合前 TUI 被杀（做空 M1 残留），答复其实停在
/// 没开成的通知里。这种情况不算送达，回到会话时由 hook 再交一次（调用方保证关卡仍是 answered）。
fn delivered_and_alive(task: &Path, gate_id: &str) -> bool {
    delivered(task, gate_id) && !meowo_reporter::lead_waiter::handed_to_gone_cli(task, gate_id)
}

/// 宿主对一次作答要做的事：什么都不推。只在「已送达」不作数时撤掉它，让 hook 能再交。
pub(crate) fn deliver(target: &NativeTarget<'_>) -> Result<NativeOutcome, String> {
    let (task, gate_id) = (target.task, target.gate_id);
    if delivered(task, gate_id) {
        if !meowo_reporter::lead_waiter::handed_to_gone_cli(task, gate_id) {
            return Ok(NativeOutcome::Delivered);
        }
        meowo_reporter::lead_waiter::revoke_handed(task, gate_id);
    }
    Ok(NativeOutcome::WaiterWillWake)
}

/// kimi 的会话锁（2.6.0 SessionLockService：`<home>/locks/sessions/<sessionId>.lock`，JSON 里有持有者 pid）。
/// 读不到锁不算证据；读到且 pid 活着就是 TUI 开着（含 Meowo 之外的终端）。
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

/// 会话的 CLI 进程此刻是否还在（任一证据成立即在线）：决定面板说「在会话里发一句话」还是「打开会话」。
fn session_online(
    store: &meowo_store::Store,
    session_id: i64,
    provider: &str,
    native: &str,
) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if crate::terminal::session_agent_alive(store, session_id).unwrap_or(true) {
        return true;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = session_id;
    let profile = store.session_profile(session_id).ok().flatten();
    if session_lock_held(
        crate::profile::data_dir(provider, profile.as_deref()),
        native,
    ) {
        return true;
    }
    // 非 Windows/macOS 没有进程判定：按在线处理，提示在会话里发一句话。
    !cfg!(any(target_os = "windows", target_os = "macos"))
}

/// 面板上的送达状态（state 是 i18n leadDeliveryState 的键）：
/// - 答复已交到活着的会话（等待器退出 / Stop hook）→ consumed「模型已收到回答，等待接手关卡」；
/// - 等待器还活着（刚作答、下一秒就会读到）→ sending；
/// - 会话在线但没有等待器 → native_manual「在会话里发一句话即可继续」；
/// - 会话不在 → native_offline「打开会话后说一句继续」（不再后台续写）。
pub(crate) fn status(
    store: &meowo_store::Store,
    session_id: i64,
    task: &Path,
    gate: &meowo_protocol::ipc::LeadGateDto,
    provider: &str,
    native: &str,
) -> String {
    status_from(
        delivered_and_alive(task, &gate.id),
        meowo_reporter::lead_waiter::live_waiter(task, &gate.id).is_some(),
        || session_online(store, session_id, provider, native),
    )
}

fn status_from(delivered: bool, live_waiter: bool, online: impl FnOnce() -> bool) -> String {
    if delivered {
        "consumed"
    } else if live_waiter {
        "sending"
    } else if online() {
        "native_manual"
    } else {
        "native_offline"
    }
    .into()
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

    fn target(task: &Path) -> NativeTarget<'_> {
        NativeTarget {
            db: Path::new("Z:/no/such/meowo.db"),
            session_id: 1,
            provider: "kimi",
            native_session: "session_x",
            cwd: None,
            task,
            gate_id: "Q-1",
            message: "答复",
            force: true,
        }
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

    /// 面板如实说下一步：有等待器 = 发送中；在线没等待器 = 在会话里发一句话；不在 = 打开会话后说一句。
    #[test]
    fn status_tells_the_next_step_without_claiming_delivery() {
        assert_eq!(status_from(false, false, || true), "native_manual");
        assert_eq!(
            status_from(false, true, || panic!("有等待器时不必查进程")),
            "sending"
        );
        assert_eq!(status_from(false, false, || false), "native_offline");
        assert_eq!(status_from(true, false, || false), "consumed");
    }

    /// leader 2026-11-13「不需要离线」：TUI 不在时宿主只保存，不开库、不起进程、不留任何续写记录。
    #[test]
    fn answer_for_an_exited_tui_is_only_saved_never_resumed_in_background() {
        let task = temp_task("offline");
        // db 指向不存在的路径：deliver 若尝试开库/起进程会直接报错。
        assert_eq!(deliver(&target(&task)), Ok(NativeOutcome::WaiterWillWake));
        let leftovers: Vec<_> = std::fs::read_dir(&task)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(task);
    }

    /// 做空 M1 残留：等待器交出答复的 CLI 进程已经没了（模型开新回合前 TUI 被杀）——不再显示
    /// 「模型已收到」，巡检撤掉「已送达」，回到会话时 hook 再交；CLI 活着时照旧算已收到。
    #[test]
    fn answer_handed_to_a_killed_tui_is_not_reported_as_received() {
        let task = temp_task("m1");
        std::fs::write(task.join(".delivered"), "Q-1").unwrap();
        assert!(delivered_and_alive(&task, "Q-1"));
        assert_eq!(deliver(&target(&task)), Ok(NativeOutcome::Delivered));
        // 旁注记着一个已退出的 CLI（启动时刻对不上 = 不是同一个进程）。
        let me = std::process::id();
        let started = meowo_reporter::lead_waiter::process_started(me).unwrap();
        std::fs::write(
            task.join(".delivered-to.json"),
            format!(
                "{{\"gate\":\"Q-1\",\"cli_pid\":{me},\"cli_started\":{}}}",
                started + 1000
            ),
        )
        .unwrap();
        assert!(!delivered_and_alive(&task, "Q-1"));
        assert_eq!(deliver(&target(&task)), Ok(NativeOutcome::WaiterWillWake));
        assert!(!delivered(&task, "Q-1"), "撤掉「已送达」，hook 才会再交");
        // 上一个关卡的 .delivered 不算这个关卡送达过。
        std::fs::write(task.join(".delivered"), "Q-0").unwrap();
        assert!(!delivered(&task, "Q-1"));
        let _ = std::fs::remove_dir_all(task);
    }
}
