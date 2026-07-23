//! Live-session query service and Tauri adapters.

use meowo_store::LiveSession;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::State;

/// 必须 ≥ 对话窗 650ms 的历史轮询周期:此前 300ms 意味着单开对话窗时每一轮都击穿缓存,
/// Windows 上每 650ms 付一次 30-120ms 的全进程扫描(proc.rs 注释实测)。900ms 让相邻
/// 轮询共享同一次采样;存活展示容忍 ~1s 的陈旧(RESUME_GRACE 是 120s 量级)。
const PROCESS_SNAPSHOT_TTL_MS: i64 = 900;

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
    /// 本 GUI 进程正托管该会话的 PTY。门控卡片菜单「结束会话」的可见性——外部终端里
    /// 跑的会话杀不了（要先走接管），与对话窗 ChatHistoryDto.pty_managed 同口径。
    pty_managed: bool,
    errored: bool,
    error_label: Option<String>,
    error_raw: Option<String>,
    preview: Option<String>,
}

/// 翻页游标：**SQL 扫描位置**（排序前）。响应里的 items 会做 connected-first 排序，
/// 末项不再是本页时间上最旧的一条——拿末项当游标会重复/漏页（旧版 loadMore 的坑）。
/// 调用方翻下一页必须回传这里给出的 cursor，而不是自己从 items 推。
#[derive(serde::Serialize, Clone, Copy)]
pub(crate) struct PageCursor {
    pub(crate) last_event_at: i64,
    pub(crate) id: i64,
}

#[derive(serde::Serialize)]
pub(crate) struct LiveSessionsPage {
    pub(crate) items: Vec<LiveItem>,
    /// None = 已扫到底，没有下一页。Some 也可能恰好是最后一行——下一页会拿到空 items + None。
    pub(crate) next_cursor: Option<PageCursor>,
}

#[tauri::command]
pub(crate) async fn get_live_sessions_counts(
    state: State<'_, super::AppState>,
) -> Result<meowo_store::query::LiveSessionCounts, String> {
    let db_path = state.db_path.clone();
    // 进程表采样在 spawn_blocking 里做:冷采样 Windows 上要 30-120ms,不能挂在
    // async-runtime 线程上(且采样持缓存互斥锁,并发命令会排队其后)。
    let snapshots = state.process_snapshots.clone();
    // 托管 PTY 活跃 = 进程必在(meowo 自己 spawn 的),hook 未认领 pid / 事件宽限过期时兜底。
    let pty_live = state.ptys.active_session_ids();
    tauri::async_runtime::spawn_blocking(move || {
        let alive = snapshots.snapshot();
        let store = super::open_store(&db_path)?;
        let (total, archived) = store.live_sessions_totals().map_err(|e| e.to_string())?;
        let candidates = store.live_count_candidates().map_err(|e| e.to_string())?;
        let now = super::now_ms();
        let (mut running, mut waiting) = (0i64, 0i64);
        for candidate in candidates {
            let connected = session_connected(
                &candidate.status,
                candidate.pid,
                process_alive(candidate.pid, &alive, pty_live.contains(&candidate.id)),
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
) -> Result<LiveSessionsPage, String> {
    let db_path = state.db_path.clone();
    let tx_cache = state.tx_cache.clone();
    // 采样进 spawn_blocking,理由同 get_live_sessions_counts。
    let snapshots = state.process_snapshots.clone();
    let pty_live = state.ptys.active_session_ids();
    let filter = normalize_filter(filter);
    tauri::async_runtime::spawn_blocking(move || {
        let alive = snapshots.snapshot();
        live_sessions_blocking(
            &db_path,
            &tx_cache,
            &alive,
            &pty_live,
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

/// 存活判定的唯一 OR:进程表命中,或该会话的托管 PTY 活跃(meowo 自己 spawn 的进程必在)。
/// 对话窗(chat.rs)、看板计数、会话列表三条通道共用——此前三处手抄,措辞已开始分叉。
pub(crate) fn process_alive(
    pid: Option<i64>,
    alive: &std::collections::HashSet<i64>,
    pty_live: bool,
) -> bool {
    pid.is_some_and(|p| p > 0 && alive.contains(&p)) || pty_live
}

/// transcript 分析的唯一接线:supports_analysis 门控 + 路径解析 + 共享 mtime 缓存。
/// 侧栏/贴纸(enrich)与对话窗(errored)同源于此,「同口径」不再靠手工维持。
pub(crate) fn analyze_transcript(
    tx_cache: &Mutex<meowo_agent::TranscriptCache>,
    provider: &str,
    cwd: Option<&str>,
    cc_session_id: &str,
) -> Option<meowo_agent::transcript::TranscriptInfo> {
    let spec = super::agent_transcript(provider).filter(|spec| spec.supports_analysis())?;
    let path = spec.resolve_transcript_path(None, cwd, cc_session_id)?;
    let path = path.to_str()?;
    Some(meowo_agent::TranscriptCache::analyze_shared(
        tx_cache, spec, path,
    ))
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
        // ended 只让位于**实证**存活(进程表命中 / 本 GUI 托管 PTY 活跃):恢复会话的
        // 窗口期 DB 还挂着旧 'ended' 而 PTY 已在跑,不给存活兜底就会「已结束」徽标配
        // 「结束会话」按钮同框。事件宽限不适用——刚结束的会话天然在宽限期内,套用它
        // 会让所有 ended 会话诈尸 120 秒。
        return pid_alive;
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
    // 托管 PTY 活跃的会话集合:hook 未认领 pid / 事件宽限过期时的存活兜底,
    // 与对话窗口(chat.rs 的 connected)同口径,两边不再各说各话。
    pty_live: &std::collections::HashSet<i64>,
    filter: &str,
    search: Option<&str>,
    page: PageReq,
) -> Result<LiveSessionsPage, String> {
    if page.limit == 0 {
        return Ok(LiveSessionsPage {
            items: Vec::new(),
            next_cursor: None,
        });
    }
    let store = super::open_store(db_path)?;
    let now = super::now_ms();
    let connectivity_filtered = matches!(filter, "waiting" | "running");
    // 每批都比 limit 多取：enrich 之后还要丢弃 ping / 空会话（以及 waiting|running 下
    // 的未连接项），按 limit 取批会让一页严重缩水——侧栏没有「加载更多」，缩水多少就
    // 少显示多少（曾出现 60 条里只剩 11 条，用户以为会话丢了）。
    let batch_limit = page.limit.max(100);
    // 单次请求的扫描上限：补页不能变成无界全表扫描（几乎全被过滤的库——大量空壳、
    // 或 waiting|running 下大量断开——会让循环翻到表尾，每行都过一遍 enrich）。
    // 到达上限就带着扫描位置返回，调用方拿 next_cursor 继续，代价摊到多次请求。
    let scan_cap = batch_limit.saturating_mul(10);
    let mut scanned = 0usize;
    let mut cursor_ts = page.before_last_event_at;
    let mut cursor_id = page.before_id;
    let mut items: Vec<LiveItem> = Vec::with_capacity(page.limit);
    // Some = 页满/到达扫描上限时的续扫位置；None = 已扫到底。
    let mut next_cursor: Option<PageCursor> = None;
    'fill: loop {
        let sessions = store
            .live_sessions(Some(filter), search, cursor_ts, cursor_id, batch_limit)
            .map_err(|e| e.to_string())?;
        let raw_len = sessions.len();
        let batch_tail = sessions
            .last()
            .map(|session| (session.session.last_event_at, session.session.id));
        for session in sessions {
            scanned += 1;
            let scan_pos = PageCursor {
                last_event_at: session.session.last_event_at,
                id: session.session.id,
            };
            let pty_managed = pty_live.contains(&session.session.id);
            let connected = session_connected(
                &session.session.status,
                session.pid,
                process_alive(session.pid, alive, pty_managed),
                session.session.last_event_at,
                now,
            );
            if !connectivity_filtered || connected {
                if let Some(item) = enrich(tx_cache, session, connected, pty_managed) {
                    items.push(item);
                }
            }
            if items.len() >= page.limit || scanned >= scan_cap {
                // 游标取当前行的 SQL 位置（严格小于/大于续查，当前行不会重复出现）。
                next_cursor = Some(scan_pos);
                break 'fill;
            }
        }
        // 这一批没取满 batch_limit，说明后面没有数据了；补页到此为止。
        if raw_len < batch_limit {
            break;
        }
        let Some((timestamp, id)) = batch_tail else {
            break;
        };
        cursor_ts = Some(timestamp);
        cursor_id = Some(id);
    }
    // 稳定排序：已连接的顶到最前，同组内保持 SQL 的时间倒序。
    items.sort_by_key(|item| std::cmp::Reverse(item.connected));
    Ok(LiveSessionsPage { items, next_cursor })
}

/// 列表的丢弃规则（与 store 的 live_sessions_totals / live_count_candidates 口径对齐）：
/// 健康探测（ping），或一条既没标题、没 todo 又已经断开的空壳会话。
fn dropped_from_list(title: &str, connected: bool, todos: &[meowo_store::Todo]) -> bool {
    if title.eq_ignore_ascii_case("ping") {
        return true;
    }
    let unnamed = title.is_empty() || title == "(未命名会话)";
    !connected && unnamed && todos.is_empty()
}

/// 补上 transcript 里的标题/错误/预览。返回 `None` 表示这条不该出现在列表里
/// （规则见 [`dropped_from_list`]）。
fn enrich(
    tx_cache: &Mutex<meowo_agent::TranscriptCache>,
    mut session: LiveSession,
    connected: bool,
    pty_managed: bool,
) -> Option<LiveItem> {
    // DB 数据已能定夺去留时先裁决，省掉 transcript 的文件 IO：只有会从 transcript
    // 补标题的 provider（目前 claude），未命名会话才可能被翻案；其余 provider 标题
    // 以 DB 为准，注定被丢的行不必为解析 transcript 付一次 open/read。
    let resolves_title = super::agent_resolves_transcript_title(&session.provider);
    if !resolves_title && dropped_from_list(session.task_title.trim(), connected, &session.todos) {
        return None;
    }
    let mut error_label = None;
    let mut error_raw = None;
    let mut preview = None;
    let info = analyze_transcript(
        tx_cache,
        &session.provider,
        session.cwd.as_deref(),
        &session.session.cc_session_id,
    );
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
    if dropped_from_list(session.task_title.trim(), connected, &session.todos) {
        return None;
    }
    Some(LiveItem {
        inner: session,
        connected,
        pty_managed,
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
            let project = store
                .upsert_project_by_root("/tmp/proj", "proj", old)
                .unwrap();
            // 先塞 80 条空壳（无标题、无 todo、已结束），再塞 20 条有标题的。按时间倒序，
            // 空壳排在前面，第一批 100 条里只有 20 条能通过过滤。
            for i in 0..100 {
                let cc = format!("cc-{i:03}");
                let (sid, _) = store.start_session(project, &cc, old + i).unwrap();
                if i >= 80 {
                    store
                        .set_session_title(sid, &format!("真实会话 {i}"), old + i)
                        .unwrap();
                }
                store
                    .set_session_status(sid, meowo_store::SessionStatus::Ended, old + i)
                    .unwrap();
            }
            // 再补 60 条有标题的、更早的会话，用来验证补页确实翻到了第二批。
            for i in 0..60 {
                let cc = format!("older-{i:03}");
                let (sid, _) = store.start_session(project, &cc, old - 1000 - i).unwrap();
                store
                    .set_session_title(sid, &format!("更早会话 {i}"), old - 1000 - i)
                    .unwrap();
                store
                    .set_session_status(sid, meowo_store::SessionStatus::Ended, old - 1000 - i)
                    .unwrap();
            }
        }

        let cache = Mutex::new(meowo_agent::TranscriptCache::default());
        let alive = std::collections::HashSet::new();
        let pty_none = std::collections::HashSet::new();
        let page = live_sessions_blocking(
            &db,
            &cache,
            &alive,
            &pty_none,
            "all",
            None,
            PageReq {
                before_last_event_at: None,
                before_id: None,
                limit: 60,
            },
        )
        .unwrap();

        assert_eq!(page.items.len(), 60, "补页应凑满一整页，而不是被空会话压缩");
        assert!(
            page.items
                .iter()
                .all(|item| !item.inner.task_title.trim().is_empty()),
            "空壳会话不该出现在结果里",
        );
        assert!(
            page.next_cursor.is_some(),
            "库里还有第 61+ 条有效会话，next_cursor 不该是 None",
        );
        let _ = std::fs::remove_file(&db);
    }

    /// 排序会把已连接的会话顶到页首，页内末项不再是时间上最旧的一条——旧版调用方拿
    /// 末项当游标，翻页要么重复要么打转。现在后端在响应里带回**扫描位置**游标，调用方
    /// 必须回传它。这里把整个翻页过程跑完，断言既收全、也不重复。
    #[test]
    fn cursor_paging_collects_every_session() {
        let dir = std::env::temp_dir().join(format!("meowo-cursor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("cursor.db");
        let _ = std::fs::remove_file(&db);
        let old = super::super::now_ms() - RESUME_GRACE_MS * 10;
        const TOTAL: i64 = 250;
        let mut alive = std::collections::HashSet::new();

        {
            let store = meowo_store::Store::open(&db).unwrap();
            let project = store.upsert_project_by_root("/tmp/p", "p", old).unwrap();
            for i in 0..TOTAL {
                let (sid, _) = store
                    .start_session(project, &format!("cc-{i:04}"), old + i)
                    .unwrap();
                store
                    .set_session_title(sid, &format!("会话 {i}"), old + i)
                    .unwrap();
                // 每 7 条留一个「活着」的会话散布在时间轴各处：它们会被排序顶到每页最前，
                // 正是压垮游标单调性的那批。其余标记 ended。
                if i % 7 == 0 {
                    let pid = 9000 + i;
                    store.set_session_pid(sid, pid, old + i).unwrap();
                    alive.insert(pid);
                } else {
                    store
                        .set_session_status(sid, meowo_store::SessionStatus::Ended, old + i)
                        .unwrap();
                }
            }
        }

        let cache = Mutex::new(meowo_agent::TranscriptCache::default());
        let pty_none = std::collections::HashSet::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor: Option<(i64, i64)> = None;
        const PAGE: usize = 100;
        for _ in 0..20 {
            let page = live_sessions_blocking(
                &db,
                &cache,
                &alive,
                &pty_none,
                "all",
                None,
                PageReq {
                    before_last_event_at: cursor.map(|c| c.0),
                    before_id: cursor.map(|c| c.1),
                    limit: PAGE,
                },
            )
            .unwrap();
            for item in &page.items {
                assert!(
                    seen.insert(item.inner.session.id),
                    "游标翻页重复返回了会话 {}",
                    item.inner.session.id,
                );
            }
            let Some(next) = page.next_cursor else {
                break;
            };
            cursor = Some((next.last_event_at, next.id));
        }

        assert_eq!(seen.len(), TOTAL as usize, "游标翻页漏掉了会话");
        let _ = std::fs::remove_file(&db);
    }

    /// 拿本机真实 board.db 对账：tab 计数与列表实际能显示的条数差多少。totals 现已按
    /// 列表口径剔除 ping/已结束空壳，差额应接近 0；残余是「非 ended 的断开空壳」（多算）
    /// 与「transcript 补出标题的未命名会话」（少算）两类小头。
    ///   cargo test --lib counts_versus_visible -- --ignored --nocapture
    #[test]
    #[ignore = "读本机真实数据；仅供手动对账"]
    fn counts_versus_visible_rows() {
        let Some(home) = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
        else {
            return;
        };
        let db = std::path::PathBuf::from(home)
            .join(".meowo")
            .join("board.db");
        if !db.exists() {
            eprintln!("本机没有 ~/.meowo/board.db，跳过");
            return;
        }
        let (total, archived) = super::super::open_store(&db)
            .unwrap()
            .live_sessions_totals()
            .unwrap();
        let cache = Mutex::new(meowo_agent::TranscriptCache::default());
        // alive 传空集 → 全部按「未连接」判定，正是列表过滤最狠的情形（可见条数下界）。
        let alive = std::collections::HashSet::new();
        let pty_none = std::collections::HashSet::new();
        let visible = live_sessions_blocking(
            &db,
            &cache,
            &alive,
            &pty_none,
            "all",
            None,
            PageReq {
                before_last_event_at: None,
                before_id: None,
                limit: total as usize + 100,
            },
        )
        .unwrap()
        .items
        .len();
        // total 含归档，列表 filter="all" 不含——对账要拿 total - archived 去比。
        println!("counts.total (含归档，已剔除空壳) = {total}");
        println!("counts.archived                   = {archived}");
        println!("未归档                            = {}", total - archived);
        println!("列表实际可见                      = {visible}");
        println!(
            "差额（应接近 0）                  = {}",
            total - archived - visible as i64
        );
    }

    #[test]
    fn process_snapshot_is_reused_within_ttl_and_refreshed_afterwards() {
        let cache = ProcessSnapshotCache::default();
        let first = cache.snapshot_with(1_000, || [7].into_iter().collect());
        // TTL 必须罩得住对话窗 650ms 的轮询周期:651ms 后仍复用同一次采样,
        // 否则单开对话窗时每轮都冷采样(Windows 上一次 30-120ms)。
        let reused = cache.snapshot_with(1_651, || panic!("must reuse the cached sample"));
        let refreshed = cache.snapshot_with(1_000 + PROCESS_SNAPSHOT_TTL_MS + 1, || {
            [9].into_iter().collect()
        });

        assert!(Arc::ptr_eq(&first, &reused));
        assert_eq!(*refreshed, [9].into_iter().collect());
    }
}
