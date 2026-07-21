//! Thin Tauri command adapters for the managed PTY and approval broker.

use tauri::State;

#[tauri::command]
pub(crate) async fn start_managed_terminal(
    app: tauri::AppHandle,
    state: State<'_, super::AppState>,
    session_id: i64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let db_path = state.db_path.clone();
    let broker = state.ptys.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = super::open_store(&db_path)?;
        let session = store.get_session(session_id).map_err(|e| e.to_string())?;
        if !super::session_command::is_safe_id(&session.cc_session_id) {
            return Err("无效 session_id".into());
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            if super::terminal::session_agent_alive(&store, session_id)? {
                return Err("会话仍在外部终端运行，不能重复接管".into());
            }
            let cwd = store.session_cwd(session_id).map_err(|e| e.to_string())?;
            let provider = store
                .session_provider(session_id)
                .map_err(|e| e.to_string())?;
            super::terminal::start_managed_resume_sized(
                app,
                broker,
                session_id,
                cwd,
                session.cc_session_id,
                provider,
                super::pty::TerminalSize::new(cols, rows),
            )
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            let _ = (app, broker, session, cols, rows);
            Err("当前平台暂不支持托管终端".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn managed_terminal_snapshot(
    state: State<'_, super::AppState>,
    session_id: i64,
    since: Option<u64>,
) -> super::pty::PtySnapshot {
    state.ptys.snapshot(session_id, since.unwrap_or(0))
}

#[tauri::command]
pub(crate) fn managed_terminal_binding(
    state: State<'_, super::AppState>,
    session_id: i64,
) -> Option<i64> {
    state.ptys.binding(session_id)
}

#[tauri::command]
pub(crate) fn write_managed_terminal(
    state: State<'_, super::AppState>,
    session_id: i64,
    data: String,
) -> Result<(), String> {
    state.ptys.write(session_id, data.as_bytes())
}

#[tauri::command]
pub(crate) fn resize_managed_terminal(
    state: State<'_, super::AppState>,
    session_id: i64,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.ptys.resize(session_id, cols, rows)
}

#[tauri::command]
pub(crate) fn stop_managed_terminal(
    state: State<'_, super::AppState>,
    session_id: i64,
) -> Result<(), String> {
    state.ptys.stop(session_id)
}

#[tauri::command]
pub(crate) fn get_pending_approval(
    state: State<'_, super::AppState>,
    session_id: i64,
) -> Option<meowo_protocol::ipc::PendingApprovalDto> {
    // 出口走 DTO 而非原始 ApprovalRequest：后者空 suggestions 会被 skip 掉字段，
    // 与 ts-rs 生成的前端类型（字段恒在）不符。缘由见 pty.rs 的 emit_approval。
    state.ptys.pending_approval(session_id).map(Into::into)
}

#[tauri::command]
pub(crate) fn register_approval_consumer(
    state: State<'_, super::AppState>,
    session_id: i64,
    consumer_id: String,
) -> Result<(), String> {
    state
        .ptys
        .register_approval_consumer(session_id, consumer_id)
}

#[tauri::command]
pub(crate) fn unregister_approval_consumer(state: State<'_, super::AppState>, consumer_id: String) {
    state.ptys.unregister_approval_consumer(&consumer_id);
}

#[tauri::command]
pub(crate) async fn resolve_pending_approval(
    state: State<'_, super::AppState>,
    session_id: i64,
    request_id: String,
    choice: String,
) -> Result<(), String> {
    let ptys = state.ptys.clone();
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        ptys.resolve_approval_choice(session_id, &request_id, &choice)?;
        // reporter 收到决策后也会清 pending_review，但 codex 的 hook 可能继承只读沙箱、
        // 清不掉——标记会一直挂到下一个 hook 事件才被顺带清理。app 进程写库没有这种
        // 限制，这里当场兜底清掉（best-effort：清不掉也不影响已送达的决策）。
        if let Ok(store) = super::open_store(&db_path) {
            let _ = store.clear_pending_review(session_id, super::now_ms());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn open_attached_terminal(
    state: State<'_, super::AppState>,
    session_id: i64,
) -> Result<(), String> {
    let broker = state.ptys.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::terminal::attach_in_external_terminal(&broker, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
