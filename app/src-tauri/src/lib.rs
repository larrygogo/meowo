use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, State};

struct AppState {
    store: Mutex<Store>,
}

/// stale 阈值：超过此时长无事件的 running 会话标记为 stale。
const STALE_THRESHOLD_MS: i64 = 10 * 60 * 1000;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CC_KANBAN_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cc-kanban").join("board.db")
}

#[tauri::command]
fn get_overview(state: State<AppState>) -> Result<Vec<ProjectOverview>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.overview().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_project_tasks(state: State<AppState>, project_id: i64) -> Result<Vec<TaskCard>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.project_tasks(project_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_live_sessions(state: State<AppState>) -> Result<Vec<LiveSession>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.live_sessions().map_err(|e| e.to_string())
}

/// 监听 board.db 所在目录变更，去抖后向前端发 "board-changed"。
fn spawn_db_watcher(app: tauri::AppHandle, db_path: PathBuf) {
    let watch_dir = db_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    std::thread::spawn(move || {
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        if watcher.watch(&watch_dir, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        let debounce = Duration::from_millis(300);
        // None 表示「还没发过」，首个事件立即发；避免 Instant 减法在进程刚启动时下溢。
        let mut last_emit: Option<Instant> = None;
        for res in rx {
            if res.is_err() {
                continue;
            }
            let due = last_emit.is_none_or(|t| t.elapsed() >= debounce);
            if due {
                let _ = app.emit("board-changed", ());
                last_emit = Some(Instant::now());
            }
        }
    });
}

/// 周期性把超时无事件的 running 会话标记为 stale，让「当前活跃」状态诚实
/// （终端被强杀时收不到 SessionEnd）。有变更时触发前端刷新。
fn spawn_stale_sweeper(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        let changed = {
            let state = app.state::<AppState>();
            let guard = state.store.lock();
            match guard {
                Ok(store) => store.mark_stale(STALE_THRESHOLD_MS, now_ms()).unwrap_or(0),
                Err(_) => 0,
            }
        };
        if changed > 0 {
            let _ = app.emit("board-changed", ());
        }
        std::thread::sleep(Duration::from_secs(60));
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let path = db_path();
    let store = Store::open(&path).expect("打开 board.db 失败");
    tauri::Builder::default()
        .manage(AppState { store: Mutex::new(store) })
        .invoke_handler(tauri::generate_handler![get_overview, get_project_tasks, get_live_sessions])
        .setup(move |app| {
            spawn_db_watcher(app.handle().clone(), path.clone());
            spawn_stale_sweeper(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
