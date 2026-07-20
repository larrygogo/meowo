//! Live-session query service and Tauri adapters.

use meowo_store::LiveSession;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;

const PROCESS_SNAPSHOT_TTL_MS: i64 = 300;

/// Counts and list queries share one process-table observation during a UI refresh.
#[derive(Default)]
pub(crate) struct ProcessSnapshotCache {
    slot: Mutex<Option<(i64, Arc<std::collections::HashSet<i64>>)>>,
}

impl ProcessSnapshotCache {
    pub(crate) fn snapshot(&self) -> Arc<std::collections::HashSet<i64>> {
        self.snapshot_with(super::now_ms(), super::proc::agent_pids_snapshot)
    }

    fn snapshot_with(
        &self,
        now: i64,
        sample: impl FnOnce() -> std::collections::HashSet<i64>,
    ) -> Arc<std::collections::HashSet<i64>> {
        let mut slot = self.slot.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((sampled_at, pids)) = slot.as_ref() {
            if now.saturating_sub(*sampled_at) < PROCESS_SNAPSHOT_TTL_MS {
                return pids.clone();
            }
        }
        let pids = Arc::new(sample());
        *slot = Some((now, pids.clone()));
        pids
    }
}

#[tauri::command]
pub(crate) async fn get_overview(
    state: State<'_, super::AppState>,
) -> Result<Vec<meowo_store::ProjectOverview>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::open_store(&db_path)?
            .overview()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn recent_cwds(
    state: State<'_, super::AppState>,
    limit: usize,
) -> Result<Vec<String>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::open_store(&db_path)?
            .recent_cwds(limit)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn get_project_tasks(
    state: State<'_, super::AppState>,
    project_id: i64,
) -> Result<Vec<meowo_store::TaskCard>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::open_store(&db_path)?
            .project_tasks(project_id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(serde::Serialize)]
pub(crate) struct LiveItem {
    #[serde(flatten)]
    pub(crate) inner: LiveSession,
    pub(crate) connected: bool,
    errored: bool,
    error_label: Option<String>,
    error_raw: Option<String>,
    preview: Option<String>,
}

#[tauri::command]
pub(crate) async fn get_live_sessions_counts(
    state: State<'_, super::AppState>,
) -> Result<meowo_store::query::LiveSessionCounts, String> {
    let db_path = state.db_path.clone();
    let alive = state.process_snapshots.snapshot();
    tauri::async_runtime::spawn_blocking(move || {
        let store = super::open_store(&db_path)?;
        let (total, archived) = store.live_sessions_totals().map_err(|e| e.to_string())?;
        let candidates = store.live_count_candidates().map_err(|e| e.to_string())?;
        let now = super::now_ms();
        let (mut running, mut waiting) = (0i64, 0i64);
        for candidate in candidates {
            let pid = candidate.pid.unwrap_or(0);
            let connected = session_connected(
                &candidate.status,
                candidate.pid,
                pid > 0 && alive.contains(&pid),
                candidate.last_event_at,
                now,
            );
            match tab_class(
                connected,
                &candidate.status,
                candidate.pending_review.as_deref(),
            ) {
                Some("waiting") => waiting += 1,
                Some("running") => running += 1,
                _ => {}
            }
        }
        Ok(meowo_store::query::LiveSessionCounts {
            total,
            running,
            waiting,
            archived,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn get_live_sessions_page(
    state: State<'_, super::AppState>,
    filter: String,
    search: Option<String>,
    before_last_event_at: Option<i64>,
    before_id: Option<i64>,
    limit: usize,
) -> Result<Vec<LiveItem>, String> {
    let db_path = state.db_path.clone();
    let tx_cache = state.tx_cache.clone();
    let alive = state.process_snapshots.snapshot();
    let filter = normalize_filter(filter);
    tauri::async_runtime::spawn_blocking(move || {
        live_sessions_blocking(
            &db_path,
            &tx_cache,
            &alive,
            &filter,
            search.as_deref(),
            PageReq {
                before_last_event_at,
                before_id,
                limit,
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

fn normalize_filter(filter: String) -> String {
    if ["all", "running", "waiting", "archived"].contains(&filter.as_str()) {
        filter
    } else {
        "all".into()
    }
}

/// Resume and newly-spawned sessions remain optimistically connected while hooks claim the PID.
pub(crate) const RESUME_GRACE_MS: i64 = 120_000;

pub(crate) fn session_connected(
    status: &str,
    _pid: Option<i64>,
    pid_alive: bool,
    last_event_at: i64,
    now: i64,
) -> bool {
    if status == "ended" {
        return false;
    }
    pid_alive || now.saturating_sub(last_event_at) < RESUME_GRACE_MS
}

/// Single definition shared by list filtering and tab counters.
pub(crate) fn tab_class(
    connected: bool,
    status: &str,
    pending_review: Option<&str>,
) -> Option<&'static str> {
    if !connected {
        return None;
    }
    if status == "waiting" || pending_review.is_some() {
        return Some("waiting");
    }
    (status == "running").then_some("running")
}

pub(crate) struct PageReq {
    pub(crate) before_last_event_at: Option<i64>,
    pub(crate) before_id: Option<i64>,
    pub(crate) limit: usize,
}

pub(crate) fn live_sessions_blocking(
    db_path: &PathBuf,
    tx_cache: &Mutex<meowo_agent::TranscriptCache>,
    alive: &std::collections::HashSet<i64>,
    filter: &str,
    search: Option<&str>,
    page: PageReq,
) -> Result<Vec<LiveItem>, String> {
    if page.limit == 0 {
        return Ok(Vec::new());
    }
    let store = super::open_store(db_path)?;
    let now = super::now_ms();
    let connectivity_filtered = matches!(filter, "waiting" | "running");
    let batch_limit = if connectivity_filtered {
        page.limit.max(100)
    } else {
        page.limit
    };
    let mut cursor_ts = page.before_last_event_at;
    let mut cursor_id = page.before_id;
    let mut ranked: Vec<(LiveSession, bool)> = Vec::new();
    loop {
        let sessions = store
            .live_sessions(Some(filter), search, cursor_ts, cursor_id, batch_limit)
            .map_err(|e| e.to_string())?;
        let raw_len = sessions.len();
        let next_cursor = sessions
            .last()
            .map(|session| (session.session.last_event_at, session.session.id));
        for session in sessions {
            let pid = session.pid.unwrap_or(0);
            let connected = session_connected(
                &session.session.status,
                session.pid,
                pid > 0 && alive.contains(&pid),
                session.session.last_event_at,
                now,
            );
            if !connectivity_filtered || connected {
                ranked.push((session, connected));
                if connectivity_filtered && ranked.len() >= page.limit {
                    break;
                }
            }
        }
        if !connectivity_filtered
            || ranked.len() >= page.limit
            || raw_len < batch_limit
            || next_cursor.is_none()
        {
            break;
        }
        let (timestamp, id) = next_cursor.expect("checked above");
        cursor_ts = Some(timestamp);
        cursor_id = Some(id);
    }
    ranked.sort_by_key(|row| std::cmp::Reverse(row.1));

    let mut items = Vec::with_capacity(ranked.len());
    for (mut session, connected) in ranked {
        let mut error_label = None;
        let mut error_raw = None;
        let mut preview = None;
        let info = super::agent_transcript(&session.provider)
            .filter(|spec| spec.supports_analysis())
            .and_then(|spec| {
                spec.resolve_transcript_path(
                    None,
                    session.cwd.as_deref(),
                    &session.session.cc_session_id,
                )
                .and_then(|path| path.to_str().map(str::to_string))
                .map(|path| meowo_agent::TranscriptCache::analyze_shared(tx_cache, spec, &path))
            });
        if let Some(info) = info {
            if super::agent_resolves_transcript_title(&session.provider) {
                if let Some(title) = info.title {
                    session.task_title = title;
                }
            }
            if let Some(error) = info.error {
                error_label = Some(error.label);
                error_raw = Some(error.raw);
            }
            preview = info.preview;
        }
        let title = session.task_title.trim();
        if title.eq_ignore_ascii_case("ping") {
            continue;
        }
        let unnamed = title.is_empty() || title == "(未命名会话)";
        if !connected && unnamed && session.todos.is_empty() {
            continue;
        }
        if connectivity_filtered && !connected {
            continue;
        }
        items.push(LiveItem {
            inner: session,
            connected,
            errored: error_label.is_some(),
            error_label,
            error_raw,
            preview,
        });
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_filters_degrade_to_all() {
        assert_eq!(normalize_filter("unknown".into()), "all");
        assert_eq!(normalize_filter("waiting".into()), "waiting");
    }

    #[test]
    fn process_snapshot_is_reused_within_ttl_and_refreshed_afterwards() {
        let cache = ProcessSnapshotCache::default();
        let first = cache.snapshot_with(1_000, || [7].into_iter().collect());
        let reused = cache.snapshot_with(1_100, || panic!("must reuse the cached sample"));
        let refreshed = cache.snapshot_with(1_301, || [9].into_iter().collect());

        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(*refreshed, [9].into_iter().collect());
    }
}
