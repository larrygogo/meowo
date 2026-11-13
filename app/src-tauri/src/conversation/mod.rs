//! 一个托管会话只拥有一个原生root引擎；投递、审批、停止与恢复不经过用户的终端输入框。

pub(crate) mod commands;
mod lead_runtime;
mod owner;
mod preflight;
#[cfg(test)]
mod probe;
#[cfg(test)]
mod tests;
mod transport;

use meowo_agent::conversation::{
    ConversationAction as Action, ConversationAttachment, ConversationCodec,
    ConversationEvent as Event, ConversationInit, ConversationInput as Input,
    ConversationToolStatus,
};
use meowo_protocol::conversation::*;
use meowo_protocol::ipc::ChatItem;
use meowo_store::{
    managed::{ManagedDelivery, ManagedSession},
    Store,
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    mpsc::{self, SyncSender},
    Arc, LazyLock, Mutex,
};
use std::time::{Duration, Instant};
use transport::{Transport, TransportEvent};

type Key = (PathBuf, i64);
static WORKERS: LazyLock<Mutex<HashMap<Key, Arc<Handle>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
enum Message {
    Wake,
    Input(Input),
    Native(TransportEvent),
}
struct Handle {
    owner: String,
    sender: SyncSender<Message>,
}

fn open(db: &Path) -> Result<Store, String> {
    crate::open_store(db)
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn now() -> i64 {
    crate::now_ms()
}
fn epoch(owner: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}:", Sha256::digest(owner.as_bytes()))
}

fn retire_interactions(store: &Store, id: i64) -> Result<(), String> {
    for row in store.managed_pending_interactions(id).map_err(error)? {
        if let Ok(Event::Interaction { interaction_id, .. }) =
            serde_json::from_str(&row.payload_json)
        {
            let closed = Event::InteractionResolved { interaction_id };
            store
                .managed_record_event(
                    id,
                    &serde_json::to_string(&closed).map_err(error)?,
                    None,
                    now(),
                )
                .map_err(error)?;
        }
    }
    store.clear_pending_review(id, now() + 1).map_err(error)
}

fn dto(state: ManagedSession) -> ManagedConversationDto {
    ManagedConversationDto {
        session_id: state.session_id,
        provider: state.provider,
        cwd: state.cwd,
        profile_id: state.profile_id,
        native_session_id: state.native_session_id,
        phase: state.phase,
        adopted: state.adopted,
        runtime_owner: None,
        runtime_pid: state.runtime_pid,
        last_error: state.last_error,
        created_at: state.created_at,
        updated_at: state.updated_at,
    }
}

fn delivery_dto(item: ManagedDelivery) -> Result<ManagedDeliveryDto, String> {
    Ok(ManagedDeliveryDto {
        session_id: item.session_id,
        delivery_id: item.delivery_id,
        ordinal: item.ordinal,
        text: item.text,
        origin: item.origin,
        attachments: serde_json::from_str(&item.attachments_json)
            .map_err(|_| "conversation/persisted_payload_invalid")?,
        gate_ref: item
            .gate_ref_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|_| "conversation/persisted_payload_invalid")?,
        state: item.state,
        native_message_id: item.native_message_id,
        last_error: item.last_error,
        accepted_at: item.accepted_at,
        consumed_at: item.consumed_at,
        completed_at: item.completed_at,
        created_at: item.created_at,
        updated_at: item.updated_at,
    })
}

pub(crate) fn state(db: &Path, id: i64) -> Result<Option<ManagedConversationDto>, String> {
    open(db)?
        .managed_state(id)
        .map(|state| state.map(dto))
        .map_err(error)
}

pub(crate) fn deliveries(db: &Path, id: i64) -> Result<Vec<ManagedDeliveryDto>, String> {
    open(db)?
        .managed_deliveries(id)
        .map_err(error)?
        .into_iter()
        .map(delivery_dto)
        .collect()
}

pub(crate) fn pending_deliveries(db: &Path, id: i64) -> Result<Vec<ManagedDeliveryDto>, String> {
    open(db)?
        .managed_pending_deliveries(id, 40)
        .map_err(error)?
        .into_iter()
        .map(delivery_dto)
        .collect()
}

pub(crate) fn recoveries(
    db: &Path,
    id: i64,
    cursor: Option<&ManagedNativeRecoveryCursorDto>,
    limit: usize,
) -> Result<ManagedNativeRecoveryPageDto, String> {
    let cursor = cursor.map(|cursor| meowo_store::managed::ManagedNativeRecoveryCursor {
        sequence: cursor.sequence,
        message_index: cursor.message_index,
        text_byte_offset: cursor.text_byte_offset,
    });
    let page = open(db)?
        .managed_native_recovery_page(id, cursor.as_ref(), limit)
        .map_err(error)?;
    Ok(ManagedNativeRecoveryPageDto {
        chunks: page
            .chunks
            .into_iter()
            .map(|chunk| ManagedNativeRecoveryChunkDto {
                sequence: chunk.sequence,
                created_at: chunk.created_at,
                message_index: chunk.message_index,
                role: chunk.role,
                text: chunk.text,
                text_byte_offset: chunk.text_byte_offset,
                complete: chunk.complete,
            })
            .collect(),
        next_cursor: page
            .next_cursor
            .map(|cursor| ManagedNativeRecoveryCursorDto {
                sequence: cursor.sequence,
                message_index: cursor.message_index,
                text_byte_offset: cursor.text_byte_offset,
            }),
    })
}

pub(crate) fn events(
    db: &Path,
    id: i64,
    after: i64,
    limit: usize,
) -> Result<ManagedEventPageDto, String> {
    let store = open(db)?;
    let rows = store
        .managed_events_since(id, after, limit)
        .map_err(error)?;
    let next_sequence = rows.last().map_or(after, |row| row.sequence);
    let has_more = !store
        .managed_events_since(id, next_sequence, 1)
        .map_err(error)?
        .is_empty();
    Ok(ManagedEventPageDto {
        events: rows
            .into_iter()
            .map(|row| {
                Ok(ManagedEventDto {
                    session_id: row.session_id,
                    sequence: row.sequence,
                    payload: serde_json::from_str(&row.payload_json)
                        .map_err(|_| "conversation/persisted_payload_invalid")?,
                    created_at: row.created_at,
                })
            })
            .collect::<Result<_, String>>()?,
        next_sequence,
        has_more,
    })
}

pub(crate) fn interaction(db: &Path, id: i64) -> Result<Option<Event>, String> {
    let store = open(db)?;
    let Some(state) = store.managed_state(id).map_err(error)? else {
        return Ok(None);
    };
    let Some(owner) = state
        .runtime_owner
        .as_deref()
        .filter(|_| state.phase == "blocked")
    else {
        return Ok(None);
    };
    if !handle(db, id).is_ok_and(|worker| worker.owner == owner) {
        return Ok(None);
    }
    let prefix = epoch(owner);
    store.managed_pending_interactions(id).map_err(error)?.into_iter().find(|row|
        serde_json::from_str::<Event>(&row.payload_json).ok().is_some_and(|event|
            matches!(event, Event::Interaction { interaction_id, .. } if interaction_id.starts_with(&prefix))))
        .map(|row| serde_json::from_str(&row.payload_json).map_err(error)).transpose()
}

fn fact(db: &Path, id: i64, kind: &str) -> Result<Option<Event>, String> {
    open(db)?
        .managed_latest_fact(id, kind)
        .map_err(error)?
        .map(|row| serde_json::from_str(&row.payload_json).map_err(error))
        .transpose()
}
pub(crate) fn metadata(db: &Path, id: i64) -> Result<Option<Event>, String> {
    fact(db, id, "metadata")
}
pub(crate) fn catalog(db: &Path, id: i64) -> Result<Option<Event>, String> {
    fact(db, id, "catalog")
}

struct Launch {
    argv: Vec<String>,
    env: Vec<(String, String)>,
    spec: meowo_agent::conversation::ConversationSpec,
    codec: Box<dyn ConversationCodec>,
    lead_enabled: bool,
}

fn managed_launch_argv(
    id: meowo_agent::AgentId,
    selected: Option<Vec<String>>,
    fallback: impl FnOnce() -> Vec<String>,
    augment: impl FnOnce(meowo_agent::AgentId, Vec<String>) -> Vec<String>,
) -> Vec<String> {
    augment(id, selected.unwrap_or_else(fallback))
}

#[cfg(test)]
fn probe_gemini_model(value: Option<&str>) -> Option<String> {
    // 模型名只允许CLI的标识符字符；探针环境变量不能绕过argv边界影响其它启动参数。
    value
        .filter(|model| {
            !model.is_empty()
                && model
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .map(str::to_owned)
}

fn launch(state: &ManagedSession) -> Result<Launch, String> {
    let plugin = meowo_agent::by_id(&state.provider).ok_or("conversation/provider_unknown")?;
    let cap = plugin.conversation().ok_or("conversation/unsupported")?;
    let installation = match state.profile_id.as_deref() {
        Some(profile) => crate::profile::installation_of(plugin.id(), profile)
            .ok_or("conversation/profile_missing")?,
        None => plugin.resolve().ok_or("conversation/cli_missing")?,
    };
    let version = crate::probe_cli_version_in(plugin, Some(&installation));
    let lead_enabled = plugin
        .lead()
        .zip(crate::setup::sibling_reporter())
        .is_some_and(|(cap, reporter)| {
            meowo_agent::lead_install::inspect(cap, &installation, version.as_deref(), &reporter)
                .installed
        });
    let mut spec = cap.spec(&installation, version.as_deref())?;
    #[cfg(test)]
    if plugin.id().as_str() == "gemini" {
        if let Some(model) =
            probe_gemini_model(std::env::var("MEOWO_PROBE_GEMINI_MODEL").ok().as_deref())
        {
            // 探针必须显式走指定模型，避免用户默认模型高负载时把服务波动归咎ACP。
            spec.args.extend(["--model".into(), model]);
        }
    }
    let argv = managed_launch_argv(
        plugin.id(),
        installation.launch.clone(),
        || plugin.launch_argv(),
        crate::relay::augment_argv,
    );
    let env = crate::terminal::launch_env_for_exact_profile(plugin, state.profile_id.as_deref());
    let codec = cap.codec(ConversationInit {
        cwd: state.cwd.clone(),
        native_session_id: state.native_session_id.clone(),
        ..Default::default()
    });
    Ok(Launch {
        argv,
        env,
        spec,
        codec,
        lead_enabled,
    })
}

pub(crate) fn create(
    db: &Path,
    provider: &str,
    cwd: &str,
    profile_id: Option<&str>,
    app: Option<tauri::AppHandle>,
) -> Result<ManagedConversationDto, String> {
    let cwd = dunce::canonicalize(cwd).map_err(|_| "conversation/cwd_invalid")?;
    if !cwd.is_dir() {
        return Err("conversation/cwd_invalid".into());
    }
    let profile = profile_id
        .map(str::to_owned)
        .or_else(|| crate::profile::active_id(provider));
    let store = open(db)?;
    let id = store
        .managed_create(provider, &cwd.to_string_lossy(), profile.as_deref(), now())
        .map_err(error)?;
    if let Err(reason) = ensure_worker(db, id, app) {
        if let Ok(Some(state)) = store.managed_state(id) {
            let next = if state.runtime_owner.is_none() {
                "failed"
            } else {
                &state.phase
            };
            let _ = store.managed_set_phase(id, &state.phase, next, Some(&reason), now());
        }
        return Err(reason);
    }
    state(db, id)?.ok_or_else(|| "conversation/session_missing".into())
}

pub(crate) fn adopt(
    db: &Path,
    id: i64,
    app: Option<tauri::AppHandle>,
) -> Result<ManagedConversationDto, String> {
    let store = open(db)?;
    if store.managed_state(id).map_err(error)?.is_none() {
        // 移交不能悄悄变成第二writer：没有停止证据就拒绝，保留原生会话供用户明确处理。
        // 外部 CLI 的缺席/PID 快照不能证明共享 daemon 的 root 已释放；现有会话须另走有凭据的移交协议。
        return Err("conversation/adoption_unverified".into());
    }
    ensure_worker(db, id, app)?;
    state(db, id)?.ok_or_else(|| "conversation/session_missing".into())
}

fn key(db: &Path, id: i64) -> Key {
    (db.to_path_buf(), id)
}
fn handle(db: &Path, id: i64) -> Result<Arc<Handle>, String> {
    WORKERS
        .lock()
        .map_err(|_| "conversation/lock_failed")?
        .get(&key(db, id))
        .cloned()
        .ok_or_else(|| "conversation/not_running".into())
}

fn ensure_worker(db: &Path, id: i64, app: Option<tauri::AppHandle>) -> Result<(), String> {
    let mut workers = WORKERS.lock().map_err(|_| "conversation/lock_failed")?;
    if workers.contains_key(&key(db, id)) {
        return Ok(());
    }
    let store = open(db)?;
    let mut state = store
        .managed_state(id)
        .map_err(error)?
        .ok_or("conversation/session_missing")?;
    if let Some(owner) = state.runtime_owner.as_deref() {
        owner::confirm_orphan_exit(db, owner)?;
        if !store
            .managed_recover_owner(id, owner, now())
            .map_err(error)?
        {
            return Err("conversation/runtime_conflict".into());
        }
        owner::retire(owner);
        state = store
            .managed_state(id)
            .map_err(error)?
            .ok_or("conversation/session_missing")?;
    }
    let launch = launch(&state)?;
    spawn_worker(db, id, launch, app, &mut workers)
}

fn spawn_worker(
    db: &Path,
    id: i64,
    launch: Launch,
    app: Option<tauri::AppHandle>,
    workers: &mut HashMap<Key, Arc<Handle>>,
) -> Result<(), String> {
    let store = open(db)?;
    let owner = owner::new(db)?;
    match store.managed_acquire_runtime(id, &owner, now()) {
        Ok(true) => {}
        Ok(false) => {
            owner::retire(&owner);
            return Err("conversation/runtime_conflict".into());
        }
        Err(reason) => {
            owner::retire(&owner);
            return Err(error(reason));
        }
    }
    if let Err(reason) =
        retire_interactions(&store, id).and_then(|_| phase(&store, id, "starting", None))
    {
        if store
            .managed_release_runtime(id, &owner, now())
            .unwrap_or(false)
        {
            owner::retire(&owner);
        }
        return Err(reason);
    }
    let (sender, receiver) = mpsc::sync_channel(64);
    let entry = Arc::new(Handle {
        owner: owner.clone(),
        sender: sender.clone(),
    });
    workers.insert(key(db, id), entry);
    let db = db.to_path_buf();
    std::thread::spawn(move || {
        let exited = Arc::new(AtomicBool::new(true));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_worker(
                &db,
                id,
                &owner,
                launch,
                sender,
                receiver,
                app.as_ref(),
                exited.clone(),
            )
        }))
        .unwrap_or_else(|_| Err("conversation/worker_panicked".into()));
        let _ = finish_runtime(
            &db,
            id,
            &owner,
            result.as_ref().err().map(String::as_str),
            exited.load(Ordering::Acquire),
        );
        if let Ok(mut workers) = WORKERS.lock() {
            if workers.get(&key(&db, id)).is_some_and(|h| h.owner == owner) {
                workers.remove(&key(&db, id));
            }
        }
        if let Some(app) = app {
            crate::watch::emit_board_changed(&app, "conversation-exit");
        }
    });
    Ok(())
}

fn finish_runtime(
    db: &Path,
    id: i64,
    owner: &str,
    failure: Option<&str>,
    confirmed_exit: bool,
) -> Result<(), String> {
    let store = open(db)?;
    if store
        .managed_state(id)
        .map_err(error)?
        .and_then(|state| state.runtime_owner)
        .as_deref()
        != Some(owner)
    {
        return Err("conversation/runtime_conflict".into());
    }
    retire_interactions(&store, id)?;
    if !confirmed_exit {
        phase(&store, id, "failed", Some("conversation/exit_unverified"))?;
    } else {
        if let Some(reason) = failure {
            if store
                .managed_recover_owner(id, owner, now())
                .map_err(error)?
            {
                owner::retire(owner);
            }
            phase(&store, id, "failed", Some(reason))?;
        }
        // 只有 owned 子树退出证明成立，才能把 in-flight 归为 uncertain 并释放唯一 writer slot。
        if store
            .managed_release_runtime(id, owner, now())
            .map_err(error)?
        {
            owner::retire(owner);
        }
    }
    Ok(())
}

pub(crate) fn send(
    db: &Path,
    id: i64,
    delivery_id: &str,
    text: &str,
    attachments: &[ManagedAttachmentDto],
    gate_ref: Option<&ManagedGateRefDto>,
    app: Option<tauri::AppHandle>,
) -> Result<ManagedDeliveryDto, String> {
    meowo_store::managed::validate_delivery_id(delivery_id).map_err(error)?;
    if text.len() > 512 * 1024
        || attachments.len() > 16
        || (text.trim().is_empty() && attachments.is_empty())
    {
        return Err("conversation/message_invalid".into());
    }
    let store = open(db)?;
    let item = store
        .managed_enqueue(
            id,
            delivery_id,
            text,
            &serde_json::to_string(attachments).map_err(error)?,
            gate_ref
                .map(serde_json::to_string)
                .transpose()
                .map_err(error)?
                .as_deref(),
            now(),
        )
        .map_err(error)?;
    if let Err(reason) = ensure_worker(db, id, app) {
        // 入队已经提交，失败不能伪装成未接收；调用方仍用同一个 deliveryId 查回执。
        if let Ok(Some(state)) = store.managed_state(id) {
            let next = if state.runtime_owner.is_none() {
                "failed"
            } else {
                &state.phase
            };
            let _ = store.managed_set_phase(id, &state.phase, next, Some(&reason), now());
        }
    } else if let Ok(worker) = handle(db, id) {
        // worker 每轮也会读队列，Wake 满只影响唤醒延迟，不能撤销持久化回执。
        let _ = worker.sender.try_send(Message::Wake);
    }
    delivery_dto(item)
}

pub(crate) fn control(
    db: &Path,
    id: i64,
    action: &str,
    app: Option<tauri::AppHandle>,
) -> Result<ManagedConversationDto, String> {
    match action {
        "resume" => {
            stop_failed_worker_for_resume(db, id)?;
            ensure_worker(db, id, app)?;
        }
        "stop" if handle(db, id).is_err() => {
            let store = open(db)?;
            let current = store
                .managed_state(id)
                .map_err(error)?
                .ok_or("conversation/session_missing")?;
            if current.runtime_owner.is_some() {
                return Err("conversation/previous_process_unverified".into());
            }
            phase(&store, id, "stopped", None)?;
        }
        "cancel" | "stop" => {
            let input = if action == "cancel" {
                Input::Cancel
            } else {
                Input::Shutdown
            };
            handle(db, id)?
                .sender
                .try_send(Message::Input(input))
                .map_err(|_| "conversation/command_queue_full")?;
        }
        _ => return Err("conversation/control_invalid".into()),
    }
    state(db, id)?.ok_or_else(|| "conversation/session_missing".into())
}

fn stop_failed_worker_for_resume(db: &Path, id: i64) -> Result<(), String> {
    let Ok(worker) = handle(db, id) else {
        return Ok(());
    };
    let store = open(db)?;
    if !store
        .managed_state(id)
        .map_err(error)?
        .is_some_and(|state| state.phase == "failed")
    {
        return Ok(());
    }
    match worker.sender.try_send(Message::Input(Input::Shutdown)) {
        Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => {}
        Err(mpsc::TrySendError::Full(_)) => return Err("conversation/command_queue_full".into()),
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        let persisted = store
            .managed_state(id)
            .map_err(error)?
            .and_then(|state| state.runtime_owner);
        let local = handle(db, id).is_ok_and(|current| current.owner == worker.owner);
        if persisted.as_deref() != Some(worker.owner.as_str()) && !local {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("conversation/exit_unverified".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn delivery_action(
    db: &Path,
    id: i64,
    delivery_id: &str,
    action: &str,
    replacement_delivery_id: Option<&str>,
) -> Result<ManagedDeliveryDto, String> {
    let store = open(db)?;
    delivery_dto(
        store
            .managed_delivery_action(id, delivery_id, action, replacement_delivery_id, now())
            .map_err(error)?,
    )
}

pub(crate) fn resolve(
    db: &Path,
    id: i64,
    interaction_id: &str,
    option_id: Option<&str>,
    answers: Value,
) -> Result<(), String> {
    let worker = handle(db, id)?;
    let native_id = interaction_id
        .strip_prefix(&epoch(&worker.owner))
        .ok_or("conversation/interaction_stale")?
        .to_owned();
    let store = open(db)?;
    let pending = store.managed_pending_interactions(id).map_err(error)?.into_iter()
        .any(|item| serde_json::from_str::<Event>(&item.payload_json).ok().is_some_and(|event|
            matches!(event, Event::Interaction { interaction_id: ref pending, .. } if pending == interaction_id)));
    if !pending {
        return Err("conversation/interaction_stale".into());
    }
    worker
        .sender
        .try_send(Message::Input(Input::Resolve {
            interaction_id: native_id,
            option_id: option_id.map(str::to_owned),
            answers,
        }))
        .map_err(|_| "conversation/command_queue_full".into())
}

pub(crate) fn set_model(db: &Path, id: i64, model: &str) -> Result<(), String> {
    handle(db, id)?
        .sender
        .try_send(Message::Input(Input::SetModel {
            model: model.to_owned(),
        }))
        .map_err(|_| "conversation/command_queue_full".into())
}
pub(crate) fn set_mode(db: &Path, id: i64, dimension: &str, mode: &str) -> Result<(), String> {
    handle(db, id)?
        .sender
        .try_send(Message::Input(Input::SetMode {
            dimension: dimension.to_owned(),
            mode: mode.to_owned(),
        }))
        .map_err(|_| "conversation/command_queue_full".into())
}

fn phase(store: &Store, id: i64, next: &str, reason: Option<&str>) -> Result<(), String> {
    let current = store
        .managed_state(id)
        .map_err(error)?
        .ok_or("conversation/session_missing")?;
    if !store
        .managed_set_phase(id, &current.phase, next, reason, now())
        .map_err(error)?
    {
        return Err("conversation/state_conflict".into());
    }
    Ok(())
}

struct Active {
    delivery: ManagedDelivery,
    state: String,
    written: bool,
}

fn transition(
    store: &Store,
    id: i64,
    active: &mut Active,
    next: &str,
    native: Option<&str>,
    reason: Option<&str>,
) -> Result<(), String> {
    if active.state == next {
        return Ok(());
    }
    if !store
        .managed_set_delivery_state(
            id,
            &active.delivery.delivery_id,
            &active.state,
            next,
            native,
            reason,
            now(),
        )
        .map_err(error)?
    {
        return Err("conversation/delivery_conflict".into());
    }
    active.state = next.to_owned();
    Ok(())
}

fn record_consumption(
    store: &Store,
    id: i64,
    request_id: &str,
    native: Option<&str>,
    live: bool,
) -> Result<(), String> {
    let Some(delivery) = store.managed_delivery(id, request_id).map_err(error)? else {
        return Ok(());
    };
    if delivery
        .native_message_id
        .as_deref()
        .zip(native)
        .is_some_and(|(old, new)| old != new)
    {
        return Err("conversation/native_receipt_conflict".into());
    }
    if matches!(
        delivery.state.as_str(),
        "sending" | "accepted" | "uncertain"
    ) && !store
        .managed_set_delivery_state(
            id,
            request_id,
            &delivery.state,
            "consumed",
            native,
            None,
            now(),
        )
        .map_err(error)?
    {
        return Err("conversation/delivery_conflict".into());
    }
    if delivery.origin == "lead" {
        return Ok(());
    }
    let payload = Event::UserMessage {
        request_id: request_id.into(),
        text: delivery.text.clone(),
        native_message_id: native.map(str::to_owned),
    };
    let item = chat_item(&payload, 0)
        .map(|item| serde_json::to_string(&item))
        .transpose()
        .map_err(error)?;
    store
        .managed_record_event(
            id,
            &serde_json::to_string(&payload).map_err(error)?,
            item.as_deref(),
            now(),
        )
        .map_err(error)?;
    if live && delivery.consumed_at.is_none() {
        store
            .on_user_prompt(id, &delivery.text, now())
            .map_err(error)?;
        store
            .set_last_user_text(id, &delivery.text, now())
            .map_err(error)?;
    }
    Ok(())
}

fn reconcile_history(
    store: &Store,
    id: i64,
    messages: &[meowo_agent::conversation::ConversationHistoryMessage],
) -> Result<bool, String> {
    let nonempty = !store
        .managed_chat_window(id, None, 1)
        .map_err(error)?
        .items_json
        .is_empty();
    let mut anchored = !nonempty;
    let mut separate_view = false;
    for message in messages {
        let delivery = if let Some(request) = &message.request_id {
            store.managed_delivery(id, request).map_err(error)?
        } else if let Some(native) = &message.native_message_id {
            store
                .managed_delivery_by_native(id, native)
                .map_err(error)?
        } else {
            None
        };
        if let Some(delivery) = delivery.as_ref().filter(|_| message.role == "user") {
            record_consumption(
                store,
                id,
                &delivery.delivery_id,
                message.native_message_id.as_deref(),
                false,
            )?;
            if delivery.origin == "lead" {
                continue;
            }
        }
        if message.role == "user" && delivery.is_none() {
            separate_view = true;
            continue;
        }
        let display_text = if message.role == "user" {
            delivery
                .as_ref()
                .map_or(&message.text, |delivery| &delivery.text)
        } else {
            &message.text
        };
        let item_id = if message.role == "user" {
            delivery
                .as_ref()
                .map(|delivery| format!("managed-user-{}", delivery.delivery_id))
                .or_else(|| {
                    message
                        .native_message_id
                        .as_ref()
                        .filter(|id| !id.is_empty())
                        .map(|native| format!("managed-history-{native}"))
                })
        } else {
            message
                .segment_id
                .as_ref()
                .filter(|id| !id.is_empty())
                .map(|segment| format!("managed-output-{segment}"))
        };
        let item_id = match item_id {
            Some(id) => id,
            None => {
                separate_view = true;
                continue;
            }
        };
        let current = store.managed_chat_item(id, &item_id).map_err(error)?;
        if current.is_some() {
            anchored = true;
        }
        if !anchored {
            separate_view = true;
            continue;
        }
        let item = if message.role == "user" {
            ChatItem::UserText {
                id: item_id.clone(),
                timestamp: None,
                text: display_text.clone(),
                interjected: false,
                to_agent: None,
            }
        } else if message.role == "assistant" {
            ChatItem::AssistantText {
                id: item_id.clone(),
                timestamp: None,
                text: message.text.clone(),
            }
        } else if message.role == "reasoning" {
            ChatItem::Reasoning {
                id: item_id.clone(),
                timestamp: None,
                text: message.text.clone(),
            }
        } else {
            continue;
        };
        if let Some(current) = current {
            let current: Value = serde_json::from_str(&current).map_err(error)?;
            let Some(previous) = current["text"].as_str() else {
                separate_view = true;
                continue;
            };
            if previous == display_text {
                continue;
            }
            if matches!(message.role.as_str(), "assistant" | "reasoning")
                && previous.starts_with(&message.text)
            {
                continue;
            }
            // 只补已确认同一原生段的后缀，绝不拿历史全文覆盖重写或更晚的现场输出。
            if !matches!(message.role.as_str(), "assistant" | "reasoning")
                || !message.text.starts_with(previous)
            {
                separate_view = true;
                continue;
            }
        }
        // History 事件让增量消费者重取快照；这里是强 ID 对账后的完整消息，不是重复 delta。
        match store.managed_record_event(
            id,
            "{\"kind\":\"history\",\"messages\":[],\"reconciled\":true}",
            Some(&serde_json::to_string(&item).map_err(error)?),
            now(),
        ) {
            Ok(_) => {}
            Err(meowo_store::StoreError::InvalidInput(reason))
                if reason == "conversation/payload_too_large" =>
            {
                separate_view = true;
            }
            Err(reason) => return Err(error(reason)),
        }
    }
    Ok(separate_view)
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    db: &Path,
    id: i64,
    owner: &str,
    launch: Launch,
    sender: SyncSender<Message>,
    receiver: mpsc::Receiver<Message>,
    app: Option<&tauri::AppHandle>,
    exited: Arc<AtomicBool>,
) -> Result<(), String> {
    let store = open(db)?;
    let state = store
        .managed_state(id)
        .map_err(error)?
        .ok_or("conversation/session_missing")?;
    let mut transport = Transport::spawn_owned(
        &launch.argv,
        &launch.spec,
        &state.cwd,
        &launch.env,
        exited.clone(),
        owner,
    )?;
    if let Some(pid) = transport.pid() {
        if !store
            .managed_set_runtime_pid(id, owner, pid, now())
            .map_err(error)?
        {
            return Err("conversation/runtime_conflict".into());
        }
        store.set_session_pid(id, pid, now()).map_err(error)?;
    }
    let native_events = std::mem::replace(&mut transport.events, mpsc::channel().1);
    std::thread::spawn(move || {
        while let Ok(event) = native_events.recv() {
            if sender.send(Message::Native(event)).is_err() {
                break;
            }
        }
    });
    let mut lead = lead_runtime::LeadRuntime::new(launch.lead_enabled, db, &state.cwd);
    lead.load_binding(&store, id)?;
    let mut codec = launch.codec;
    let mut actions = VecDeque::from(codec.initialize()?);
    let mut active: Option<Active> = None;
    let mut ready = false;
    let initialized_at = Instant::now();
    let ready_timeout = Duration::from_millis(launch.spec.ready_timeout_ms);
    let mut stopping: Option<Instant> = None;
    loop {
        while let Some(action) = actions.pop_front() {
            if matches!(action, Action::Send(_) | Action::Http { .. }) {
                if let Some(current) = active.as_mut().filter(|current| !current.written) {
                    let invalid = match preflight::gate(&store, id, &current.delivery) {
                        Ok(Some(reason)) => Some(("cancelled", reason.to_owned())),
                        Err(reason) => Some(("failed", reason)),
                        Ok(None) => None,
                    };
                    if let Some((next, reason)) = invalid {
                        transition(&store, id, current, next, None, Some(&reason))?;
                        active = None;
                        actions.clear();
                        ready = false;
                        phase(&store, id, "failed", Some(&reason))?;
                        break;
                    }
                }
            }
            match action {
                Action::Send(value) => {
                    transport.send(value)?;
                    if let Some(current) = active.as_mut() {
                        current.written = true;
                    }
                }
                Action::Http {
                    tag,
                    method,
                    path,
                    body,
                } => {
                    if let Some(current) = active.as_mut() {
                        current.written = true;
                    }
                    actions.extend(codec.receive(transport.request(
                        &tag,
                        &method,
                        &path,
                        body.as_ref(),
                    )?)?);
                }
                Action::Emit(mut event) => {
                    match &mut event {
                        Event::Interaction { interaction_id, .. }
                        | Event::InteractionResolved { interaction_id } => {
                            *interaction_id = format!("{}{interaction_id}", epoch(owner));
                        }
                        Event::Metadata { model, mode } => {
                            if let Some(previous) =
                                store.managed_latest_fact(id, "metadata").map_err(error)?
                            {
                                if let Ok(Event::Metadata {
                                    model: old_model,
                                    mode: old_mode,
                                }) = serde_json::from_str(&previous.payload_json)
                                {
                                    if model.is_none() {
                                        *model = old_model;
                                    }
                                    if mode.is_none() {
                                        *mode = old_mode;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    // 先保存完整原生恢复事实；弱ID/重排不能使历史在对账前被?丢弃。
                    let history_sequence = if matches!(event, Event::History { .. }) {
                        Some(
                            store
                                .managed_record_event(
                                    id,
                                    &serde_json::to_string(&event).map_err(error)?,
                                    None,
                                    now(),
                                )
                                .map_err(error)?,
                        )
                    } else {
                        None
                    };
                    match &event {
                        Event::Bound { native_session_id } => {
                            store
                                .managed_bind(id, native_session_id, now())
                                .map_err(error)?;
                            lead.bound(native_session_id);
                        }
                        Event::Ready if stopping.is_none() && active.is_none() => {
                            ready = store
                                .managed_pending_interactions(id)
                                .map_err(error)?
                                .is_empty();
                            phase(&store, id, if ready { "ready" } else { "blocked" }, None)?;
                        }
                        Event::Accepted {
                            request_id,
                            native_message_id,
                        } => {
                            if let Some(active) = active
                                .as_mut()
                                .filter(|a| a.delivery.delivery_id == *request_id)
                            {
                                transition(
                                    &store,
                                    id,
                                    active,
                                    "accepted",
                                    native_message_id.as_deref(),
                                    None,
                                )?;
                            }
                        }
                        Event::Consumed {
                            request_id,
                            native_message_id,
                        } => {
                            let live = active
                                .as_ref()
                                .is_some_and(|active| active.delivery.delivery_id == *request_id);
                            record_consumption(
                                &store,
                                id,
                                request_id,
                                native_message_id.as_deref(),
                                live,
                            )?;
                            if let Some(active) = active
                                .as_mut()
                                .filter(|a| a.delivery.delivery_id == *request_id)
                            {
                                active.state = store
                                    .managed_delivery(id, request_id)
                                    .map_err(error)?
                                    .ok_or("conversation/delivery_missing")?
                                    .state;
                            }
                        }
                        Event::TaskWrite {
                            request_id,
                            tool_id,
                            call,
                        } => {
                            if active
                                .as_ref()
                                .is_some_and(|current| current.delivery.delivery_id == *request_id)
                            {
                                lead.wrote(&store, id, request_id, tool_id, call)?;
                            }
                        }
                        Event::Interaction { questions, .. } => {
                            phase(&store, id, "blocked", None)?;
                            store
                                .set_pending_review(
                                    id,
                                    if questions.is_empty() {
                                        meowo_store::PendingReview::Approval
                                    } else {
                                        meowo_store::PendingReview::Question
                                    },
                                    now(),
                                )
                                .map_err(error)?;
                        }
                        Event::InteractionResolved { .. } => {}
                        Event::Completed {
                            request_id,
                            stop_reason,
                        } => {
                            if let Some(current) = active
                                .as_mut()
                                .filter(|a| a.delivery.delivery_id == *request_id)
                            {
                                transition(
                                    &store,
                                    id,
                                    current,
                                    if stop_reason == "cancelled" {
                                        "cancelled"
                                    } else {
                                        "completed"
                                    },
                                    None,
                                    None,
                                )?;
                                lead.completed(&store, id, &current.delivery, stop_reason)?;
                                active = None;
                                let pending = !store
                                    .managed_pending_interactions(id)
                                    .map_err(error)?
                                    .is_empty();
                                ready = stopping.is_none() && !pending;
                                if stopping.is_none() {
                                    phase(
                                        &store,
                                        id,
                                        if pending { "blocked" } else { "ready" },
                                        None,
                                    )?;
                                }
                            }
                        }
                        Event::Failed { reason, .. } => {
                            phase(&store, id, "failed", Some(reason))?;
                            ready = false;
                        }
                        Event::History { messages } => {
                            let separate_view = reconcile_history(&store, id, messages)?;
                            if let (false, Some(sequence)) = (separate_view, history_sequence) {
                                store
                                    .managed_resolve_native_recovery(id, sequence)
                                    .map_err(error)?;
                            }
                        }
                        _ => {}
                    }
                    let item = chat_item(&event, 0)
                        .map(|item| serde_json::to_string(&item))
                        .transpose()
                        .map_err(error)?;
                    if history_sequence.is_none() {
                        store
                            .managed_record_event(
                                id,
                                &serde_json::to_string(&event).map_err(error)?,
                                item.as_deref(),
                                now(),
                            )
                            .map_err(error)?;
                    }
                    // 失败后的 codec 可能不再发 Ready；回收本引擎后由显式 resume 重新初始化，不能留一个永不工作的 slot。
                    if let Event::Failed { reason, .. } = &event {
                        return Err(reason.clone());
                    }
                    if matches!(event, Event::InteractionResolved { .. })
                        && store
                            .managed_pending_interactions(id)
                            .map_err(error)?
                            .is_empty()
                    {
                        store.clear_pending_review(id, now() + 1).map_err(error)?;
                        if stopping.is_none() {
                            ready = active.is_none();
                            phase(&store, id, if ready { "ready" } else { "running" }, None)?;
                        }
                    }
                    if let Some(app) = app {
                        crate::watch::emit_board_changed(app, "conversation-event");
                    }
                }
            }
        }
        if ready && active.is_none() && stopping.is_none() {
            if let Some(delivery) = store.managed_next_queued(id).map_err(error)? {
                match preflight::gate(&store, id, &delivery) {
                    Ok(Some(reason)) => {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "queued",
                                "cancelled",
                                None,
                                Some(reason),
                                now(),
                            )
                            .map_err(error)?;
                        continue;
                    }
                    Err(reason) => {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "queued",
                                "failed",
                                None,
                                Some(&reason),
                                now(),
                            )
                            .map_err(error)?;
                        continue;
                    }
                    Ok(None) => {}
                }
                let prepared =
                    preflight::prompt(lead.prompt(&delivery), &delivery.attachments_json).and_then(
                        |(text, attachments)| {
                            codec.validate_prompt(&text, &attachments)?;
                            Ok(Input::Prompt {
                                request_id: delivery.delivery_id.clone(),
                                text,
                                attachments,
                            })
                        },
                    );
                let input = match prepared {
                    Ok(input) => input,
                    Err(reason) => {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "queued",
                                "failed",
                                None,
                                Some(&reason),
                                now(),
                            )
                            .map_err(error)?;
                        continue;
                    }
                };
                // 文件读取与能力校验可能耗时，临claim前重验关卡，不能把旧答案排进原生协议。
                match preflight::gate(&store, id, &delivery) {
                    Ok(Some(reason)) => {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "queued",
                                "cancelled",
                                None,
                                Some(reason),
                                now(),
                            )
                            .map_err(error)?;
                        continue;
                    }
                    Err(reason) => {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "queued",
                                "failed",
                                None,
                                Some(&reason),
                                now(),
                            )
                            .map_err(error)?;
                        continue;
                    }
                    Ok(None) => {}
                }
                if store
                    .managed_set_delivery_state(
                        id,
                        &delivery.delivery_id,
                        "queued",
                        "sending",
                        None,
                        None,
                        now(),
                    )
                    .map_err(error)?
                {
                    let prepared = match codec.input(input) {
                        Ok(prepared) => prepared,
                        Err(reason) => {
                            store
                                .managed_set_delivery_state(
                                    id,
                                    &delivery.delivery_id,
                                    "sending",
                                    "failed",
                                    None,
                                    Some(&reason),
                                    now(),
                                )
                                .map_err(error)?;
                            continue;
                        }
                    };
                    if let Err(reason) = preflight::actions_fit(&prepared) {
                        store
                            .managed_set_delivery_state(
                                id,
                                &delivery.delivery_id,
                                "sending",
                                "failed",
                                None,
                                Some(&reason),
                                now(),
                            )
                            .map_err(error)?;
                        // codec已建立prepared状态但一字未写；不能谎称ready，保留引擎等用户明确stop/resume重建。
                        ready = false;
                        phase(&store, id, "failed", Some(&reason))?;
                        continue;
                    }
                    active = Some(Active {
                        delivery,
                        state: "sending".into(),
                        written: false,
                    });
                    ready = false;
                    phase(&store, id, "running", None)?;
                    actions.extend(prepared);
                    continue;
                }
            }
        }
        if stopping.is_some_and(|at| at.elapsed() > Duration::from_secs(2)) {
            break;
        }
        if !ready
            && active.is_none()
            && stopping.is_none()
            && initialized_at.elapsed() > ready_timeout
            && store
                .managed_state(id)
                .map_err(error)?
                .is_some_and(|state| state.phase == "starting")
        {
            return Err("conversation/ready_timeout".into());
        }
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(Message::Wake) => {}
            Ok(Message::Input(input)) => {
                if matches!(input, Input::Shutdown) {
                    stopping = Some(Instant::now());
                    ready = false;
                    phase(&store, id, "stopping", None)?;
                }
                actions.extend(codec.input(input)?);
            }
            Ok(Message::Native(TransportEvent::Wire(value))) => {
                actions.extend(codec.receive(value)?)
            }
            Ok(Message::Native(TransportEvent::Exited)) => {
                if stopping.is_none() {
                    return Err("conversation/process_exited".into());
                }
                break;
            }
            Ok(Message::Native(TransportEvent::Fault(_))) if stopping.is_some() => break,
            Ok(Message::Native(TransportEvent::Fault(reason))) => return Err(reason.into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    drop(transport);
    if !exited.load(Ordering::Acquire) {
        return Err("conversation/exit_unverified".into());
    }
    if let Some(current) = active.as_mut() {
        transition(
            &store,
            id,
            current,
            "uncertain",
            None,
            Some("conversation/stopped_during_delivery"),
        )?;
    }
    phase(&store, id, "stopped", None)?;
    Ok(())
}

pub(crate) fn chat_item(event: &Event, _sequence: i64) -> Option<ChatItem> {
    match event {
        Event::UserMessage {
            request_id, text, ..
        } => Some(ChatItem::UserText {
            id: format!("managed-user-{request_id}"),
            timestamp: None,
            text: text.clone(),
            interjected: false,
            to_agent: None,
        }),
        Event::Output {
            segment_id,
            text,
            reasoning,
            ..
        } => Some(if *reasoning {
            ChatItem::ReasoningDelta {
                id: format!("managed-output-{segment_id}"),
                timestamp: None,
                text: text.clone(),
            }
        } else {
            ChatItem::AssistantDelta {
                id: format!("managed-output-{segment_id}"),
                timestamp: None,
                text: text.clone(),
            }
        }),
        Event::Tool {
            tool_id,
            name,
            input,
            output,
            status,
            ..
        } => Some(match status {
            ConversationToolStatus::Started => ChatItem::ToolUse {
                id: format!("managed-tool-{tool_id}"),
                timestamp: None,
                name: name.clone(),
                summary: input.to_string(),
                subagent: None,
                detail: None,
                plan: None,
            },
            ConversationToolStatus::Completed | ConversationToolStatus::Failed => {
                ChatItem::ToolResult {
                    id: format!("managed-result-{tool_id}"),
                    timestamp: None,
                    tool_use_id: Some(format!("managed-tool-{tool_id}")),
                    text: output.clone().unwrap_or_default(),
                    is_error: *status == ConversationToolStatus::Failed,
                    subagent: None,
                    patch: None,
                }
            }
            ConversationToolStatus::Running => return None,
        }),
        Event::Failed { request_id, reason } => Some(ChatItem::TurnError {
            id: format!(
                "managed-error-{}-{}",
                request_id.as_deref().unwrap_or("session"),
                epoch(reason)
            ),
            timestamp: None,
            label: "错误".into(),
            text: reason.clone(),
        }),
        _ => None,
    }
}
