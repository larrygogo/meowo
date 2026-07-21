//! Live-session query service and Tauri adapters.

use meowo_store::LiveSession;
use std::path::Path;
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
    db_path: &Path,
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
    // 每批都比 limit 多取：enrich 之后还要丢弃 ping / 空会话（以及 waiting|running 下
    // 的未连接项），按 limit 取批会让一页严重缩水——侧栏没有「加载更多」，缩水多少就
    // 少显示多少（曾出现 60 条里只剩 11 条，用户以为会话丢了）。
    let batch_limit = page.limit.max(100);
    let mut cursor_ts = page.before_last_event_at;
    let mut cursor_id = page.before_id;
    let mut items: Vec<LiveItem> = Vec::with_capacity(page.limit);
    'fill: loop {
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
            if connectivity_filtered && !connected {
                continue;
            }
            if let Some(item) = enrich(tx_cache, session, connected) {
                items.push(item);
                if items.len() >= page.limit {
                    break 'fill;
                }
            }
        }
        // 这一批没取满 batch_limit，说明后面没有数据了；补页到此为止。
        if raw_len < batch_limit {
            break;
        }
        let Some((timestamp, id)) = next_cursor else {
            break;
        };
        cursor_ts = Some(timestamp);
        cursor_id = Some(id);
    }
    // 稳定排序：已连接的顶到最前，同组内保持 SQL 的时间倒序。
    items.sort_by_key(|item| std::cmp::Reverse(item.connected));
    Ok(items)
}

/// 补上 transcript 里的标题/错误/预览。返回 `None` 表示这条不该出现在列表里：
/// 健康探测（ping），或者一条既没标题、没 todo 又已经断开的空壳会话。
fn enrich(
    tx_cache: &Mutex<meowo_agent::TranscriptCache>,
    mut session: LiveSession,
    connected: bool,
) -> Option<LiveItem> {
    let mut error_label = None;
    let mut error_raw = None;
    let mut preview = None;
    let info = super::agent_transcript(&session.provider)
        .filter(|spec| spec.supports_analysis())
        .and_then(|spec| {
            spec.resolve_transcript_path(None, session.cwd.as_deref(), &session.session.cc_session_id)
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
        return None;
    }
    let unnamed = title.is_empty() || title == "(未命名会话)";
    if !connected && unnamed && session.todos.is_empty() {
        return None;
    }
    Some(LiveItem {
        inner: session,
        connected,
        errored: error_label.is_some(),
        error_label,
        error_raw,
        preview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_filters_degrade_to_all() {
        assert_eq!(normalize_filter("unknown".into()), "all");
        assert_eq!(normalize_filter("waiting".into()), "waiting");
    }

    /// 侧栏没有「加载更多」，一次请求拿到多少就显示多少。空会话是在取完一批**之后**
    /// 才被 `enrich` 丢掉的，所以补页循环必须继续往下取直到凑满 limit——否则一批
    /// 空壳会话就能把整个列表压到个位数（真实案例：60 条里只剩 11 条）。
    #[test]
    fn empty_sessions_do_not_shrink_the_page() {
        let dir = std::env::temp_dir().join(format!("meowo-page-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("page.db");
        let _ = std::fs::remove_file(&db);
        let old = super::super::now_ms() - RESUME_GRACE_MS * 10; // 早于宽限期 → 一定判为未连接

        {
            let store = meowo_store::Store::open(&db).unwrap();
            let project = store.upsert_project_by_root("/tmp/proj", "proj", old).unwrap();
            // 先塞 80 条空壳（无标题、无 todo、已结束），再塞 20 条有标题的。按时间倒序，
            // 空壳排在前面，第一批 100 条里只有 20 条能通过过滤。
            for i in 0..100 {
                let cc = format!("cc-{i:03}");
                let (sid, _) = store.start_session(project, &cc, old + i).unwrap();
                if i >= 80 {
                    store.set_session_title(sid, &format!("真实会话 {i}"), old + i).unwrap();
                }
                store.set_session_status(sid, meowo_store::SessionStatus::Ended, old + i).unwrap();
            }
            // 再补 60 条有标题的、更早的会话，用来验证补页确实翻到了第二批。
            for i in 0..60 {
                let cc = format!("older-{i:03}");
                let (sid, _) = store.start_session(project, &cc, old - 1000 - i).unwrap();
                store.set_session_title(sid, &format!("更早会话 {i}"), old - 1000 - i).unwrap();
                store.set_session_status(sid, meowo_store::SessionStatus::Ended, old - 1000 - i).unwrap();
            }
        }

        let cache = Mutex::new(meowo_agent::TranscriptCache::default());
        let alive = std::collections::HashSet::new();
        let items = live_sessions_blocking(
            &db,
            &cache,
            &alive,
            "all",
            None,
            PageReq { before_last_event_at: None, before_id: None, limit: 60 },
        )
        .unwrap();

        assert_eq!(items.len(), 60, "补页应凑满一整页，而不是被空会话压缩");
        assert!(
            items.iter().all(|item| !item.inner.task_title.trim().is_empty()),
            "空壳会话不该出现在结果里",
        );
        let _ = std::fs::remove_file(&db);
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
