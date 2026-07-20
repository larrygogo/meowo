//! Chat history application service and its thin Tauri adapter.
//!
//! The command only schedules blocking work. Database reads, transcript resolution,
//! incremental file parsing, paging, and mtime concurrency control live here so the
//! crate root no longer owns a second chat state machine.

use meowo_protocol::ipc::{AgentModeDto, ChatHistoryDto as ChatHistory, PendingReviewKind};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::State;

/// Per-session transcript mtimes used to detect same-length rewrites.
#[derive(Default)]
pub(crate) struct ChatMtimes {
    entries: std::collections::HashMap<i64, (std::time::SystemTime, u64)>,
    tick: u64,
}

impl ChatMtimes {
    const CAP: usize = 32;

    fn get(&self, session_id: i64) -> Option<(std::time::SystemTime, u64)> {
        self.entries.get(&session_id).copied()
    }

    /// Compare-and-swap prevents a slower read from overwriting a newer observation.
    fn put_if_current(
        &mut self,
        session_id: i64,
        seen_version: Option<u64>,
        mtime: std::time::SystemTime,
    ) {
        if self.entries.get(&session_id).map(|(_, v)| *v) != seen_version {
            return;
        }
        self.put(session_id, mtime);
    }

    fn put(&mut self, session_id: i64, mtime: std::time::SystemTime) {
        self.tick += 1;
        self.entries.insert(session_id, (mtime, self.tick));
        if self.entries.len() > Self::CAP {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (_, tick))| *tick)
                .map(|(id, _)| *id);
            if let Some(id) = oldest {
                self.entries.remove(&id);
            }
        }
    }
}

/// Far more than one screen, while keeping first-open IPC and DOM work bounded.
const FIRST_PAGE_ITEMS: usize = 200;

fn trim_first_page<T>(items: &mut Vec<T>, full: bool, full_read: bool) -> bool {
    if full || !full_read || items.len() <= FIRST_PAGE_ITEMS {
        return false;
    }
    items.drain(..items.len() - FIRST_PAGE_ITEMS);
    true
}

fn load_chat_history(
    db_path: &Path,
    chat_mtimes: &Mutex<ChatMtimes>,
    session_id: i64,
    offset: u64,
    full: bool,
) -> Result<ChatHistory, String> {
    let prev = chat_mtimes
        .lock()
        .ok()
        .and_then(|seen| seen.get(session_id));
    let prev_mtime = prev.map(|(mtime, _)| mtime);
    let prev_version = prev.map(|(_, version)| version);
    let store = super::open_store(db_path)?;
    let header = store
        .session_header(session_id)
        .map_err(|e| e.to_string())?;
    let context = store
        .session_context(&header.cc_session_id)
        .map_err(|e| e.to_string())?;
    let mut history = ChatHistory {
        session_id,
        title: header
            .title
            .clone()
            .unwrap_or_else(|| "(未命名会话)".to_string()),
        status: header.status.clone(),
        provider: header.provider.clone(),
        cwd: header.cwd.clone(),
        supported: false,
        items: Vec::new(),
        offset,
        reset: false,
        pending_review: header
            .pending_review
            .as_deref()
            .and_then(PendingReviewKind::from_stored),
        model: context.model,
        agent_modes: Vec::new(),
        context_pct: context.used_pct,
        context_window: context.window_size,
        current_activity: header.current_activity.clone(),
        has_more: false,
        last_user_text: header.last_user_text.clone(),
        last_ai_text: header.last_ai_text.clone(),
    };
    let spec = meowo_agent::by_id(&history.provider)
        .and_then(|agent| agent.telemetry())
        .and_then(|telemetry| telemetry.transcript());
    let Some(spec) = spec.filter(|spec| spec.supports_chat()) else {
        return Ok(history);
    };
    history.supported = true;
    let Some(path) =
        spec.resolve_transcript_path(None, history.cwd.as_deref(), &header.cc_session_id)
    else {
        history.reset = offset > 0;
        return Ok(history);
    };
    let delta = meowo_agent::read_chat_delta(spec, &path, offset, prev_mtime);
    if let (Ok(mut seen), Some(mtime)) = (chat_mtimes.lock(), delta.mtime) {
        seen.put_if_current(session_id, prev_version, mtime);
    }
    history.offset = delta.offset;
    history.reset = delta.reset;
    history.agent_modes = delta
        .agent_modes
        .into_iter()
        .map(|mode| AgentModeDto {
            dimension: mode.dimension,
            value: mode.value,
        })
        .collect();
    let mut items = delta.items;
    history.has_more = trim_first_page(&mut items, full, offset == 0 || delta.reset);
    history.items = items;
    Ok(history)
}

#[tauri::command]
pub(crate) async fn get_chat_history(
    state: State<'_, super::AppState>,
    session_id: i64,
    offset: u64,
    full: Option<bool>,
) -> Result<ChatHistory, String> {
    let db_path = state.db_path.clone();
    let chat_mtimes: Arc<Mutex<ChatMtimes>> = state.chat_mtimes.clone();
    tauri::async_runtime::spawn_blocking(move || {
        load_chat_history(
            &db_path,
            &chat_mtimes,
            session_id,
            offset,
            full.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_mtime_observations_cannot_overwrite_newer_ones() {
        let base = std::time::SystemTime::UNIX_EPOCH;
        let newer = base + std::time::Duration::from_secs(10);
        let mut cache = ChatMtimes::default();
        cache.put(7, base);
        let version_a = cache.get(7).map(|(_, version)| version);
        let version_b = version_a;
        cache.put_if_current(7, version_b, newer);
        cache.put_if_current(7, version_a, base);
        assert_eq!(cache.get(7).map(|(mtime, _)| mtime), Some(newer));
    }

    #[test]
    fn mtime_cache_evicts_the_stalest_entry_but_keeps_a_hot_session() {
        let base = std::time::SystemTime::UNIX_EPOCH;
        let mut cache = ChatMtimes::default();
        let hot = 1_i64;
        cache.put(hot, base);
        for i in 0..(ChatMtimes::CAP as i64 + 5) {
            cache.put(100 + i, base);
            cache.put(hot, base + std::time::Duration::from_secs(i as u64 + 1));
        }
        assert!(cache.entries.len() <= ChatMtimes::CAP);
        assert!(cache.get(hot).is_some());
        assert_eq!(cache.get(100), None);
        assert!(cache.get(100 + ChatMtimes::CAP as i64 + 4).is_some());
    }

    #[test]
    fn first_page_keeps_the_latest_items_only() {
        let mut items: Vec<_> = (0..FIRST_PAGE_ITEMS + 3).collect();
        assert!(trim_first_page(&mut items, false, true));
        assert_eq!(items.len(), FIRST_PAGE_ITEMS);
        assert_eq!(items[0], 3);

        let mut incremental: Vec<_> = (0..FIRST_PAGE_ITEMS + 3).collect();
        assert!(!trim_first_page(&mut incremental, false, false));
        assert_eq!(incremental.len(), FIRST_PAGE_ITEMS + 3);
    }
}
