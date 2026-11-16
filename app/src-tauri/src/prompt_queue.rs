//! 尚未交给官方 CLI 的消息由宿主持久持有；领取与取消的原子性在 meowo-store。
//! 已被别的客户端领取时返回 false，界面据此映射 queue/not_editable 与 queue/not_cancellable，
//! 不再把已开始写入的消息说成像仍在本地一样可改可删。

use meowo_protocol::ipc::{PromptQueueChangedDto, QueueDraftDto, QueuedPromptDto};
use meowo_store::QueuedPrompt;
use tauri::{Emitter, Manager};

fn dto(item: QueuedPrompt) -> Result<QueuedPromptDto, String> {
    let draft = serde_json::from_str(&item.draft_json).map_err(|_| "queue/draft_invalid".to_string())?;
    Ok(QueuedPromptDto {
        id: item.id,
        session_id: item.session_id,
        draft,
        state: item.state,
        created_at: item.created_at,
        updated_at: item.updated_at,
    })
}

fn emit(app: &tauri::AppHandle, session_id: i64) {
    let _ = app.emit("prompt-queue-changed", PromptQueueChangedDto { session_id });
}

fn validate(draft: &QueueDraftDto) -> Result<(), String> {
    if draft.prompt.trim().is_empty() && draft.attachments.is_empty() {
        return Err("queue/empty".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn prompt_queue_list(state: tauri::State<'_, crate::AppState>, session_id: i64) -> Result<Vec<QueuedPromptDto>, String> {
    crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .queued_prompts(session_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(dto)
        .collect()
}

#[tauri::command]
pub(crate) fn prompt_queue_enqueue(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    draft: QueueDraftDto,
) -> Result<QueuedPromptDto, String> {
    validate(&draft)?;
    let draft_json = serde_json::to_string(&draft).map_err(|e| e.to_string())?;
    let item = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .enqueue_prompt(session_id, &draft_json, crate::now_ms())
        .map_err(|e| e.to_string())?;
    emit(&app, session_id);
    dto(item)
}

#[tauri::command]
pub(crate) fn prompt_queue_edit(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    id: i64,
    draft: QueueDraftDto,
) -> Result<bool, String> {
    validate(&draft)?;
    let draft_json = serde_json::to_string(&draft).map_err(|e| e.to_string())?;
    let changed = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .edit_queued_prompt(session_id, id, &draft_json, crate::now_ms())
        .map_err(|e| e.to_string())?;
    if changed {
        emit(&app, session_id);
    }
    Ok(changed)
}

#[tauri::command]
pub(crate) fn prompt_queue_cancel(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    id: i64,
) -> Result<bool, String> {
    let changed = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .cancel_queued_prompt(session_id, id, crate::now_ms())
        .map_err(|e| e.to_string())?;
    if changed {
        emit(&app, session_id);
    }
    Ok(changed)
}

#[tauri::command]
pub(crate) fn prompt_queue_claim(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    id: i64,
) -> Result<Option<QueuedPromptDto>, String> {
    let item = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .claim_queued_prompt(session_id, id, crate::now_ms())
        .map_err(|e| e.to_string())?;
    if item.is_some() {
        emit(&app, session_id);
    }
    item.map(dto).transpose()
}

/// 拖拽排序（对照 Kimi Code 客户端的队列）。`ids` 是排好的新顺序，只含还在等待的消息；
/// 有一条已被别处领走或取消就整批不动、返回 false，界面映射 queue/not_reorderable 并刷新列表。
/// 碰数据库，按线程纪律走 async + spawn_blocking（见 src-tauri/CLAUDE.md）。
#[tauri::command]
pub(crate) async fn prompt_queue_reorder(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    ids: Vec<i64>,
) -> Result<bool, String> {
    let db = state.db_path.clone();
    let changed = tauri::async_runtime::spawn_blocking(move || {
        crate::open_store(&db)
            .map_err(|e| e.to_string())?
            .reorder_queued_prompts(session_id, &ids, crate::now_ms())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if changed {
        emit(&app, session_id);
    }
    Ok(changed)
}

#[tauri::command]
pub(crate) fn prompt_queue_finish(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    id: i64,
    delivered: bool,
) -> Result<bool, String> {
    let changed = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .finish_prompt_delivery(session_id, id, delivered, crate::now_ms())
        .map_err(|e| e.to_string())?;
    if changed {
        emit(&app, session_id);
    }
    Ok(changed)
}

#[tauri::command]
pub(crate) fn prompt_queue_retry(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    session_id: i64,
    id: i64,
) -> Result<bool, String> {
    let changed = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .retry_queued_prompt(session_id, id, crate::now_ms())
        .map_err(|e| e.to_string())?;
    if changed {
        emit(&app, session_id);
    }
    Ok(changed)
}

#[tauri::command]
pub(crate) fn prompt_queue_orphans(state: tauri::State<'_, crate::AppState>) -> Result<Vec<QueuedPromptDto>, String> {
    crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .orphaned_prompts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(dto)
        .collect()
}

#[tauri::command]
pub(crate) fn prompt_queue_restore_orphan(state: tauri::State<'_, crate::AppState>, session_id: i64, id: i64) -> Result<Option<QueuedPromptDto>, String> {
    let item = crate::open_store(&state.db_path)
        .map_err(|e| e.to_string())?
        .take_orphaned_prompt(session_id, id)
        .map_err(|e| e.to_string())?;
    item.map(dto).transpose()
}

#[cfg(not(test))]
pub(crate) fn move_prompt_queue(db: &std::path::Path, from: i64, to: i64) {
    if let Ok(store) = crate::open_store(db) {
        let _ = store.move_queued_prompts(from, to);
    }
}

pub(crate) fn recover_prompt_deliveries(app: &tauri::AppHandle) {
    let db = app.state::<crate::AppState>().db_path.clone();
    if let Ok(store) = crate::open_store(&db) {
        let _ = store.recover_prompt_deliveries();
    }
}
