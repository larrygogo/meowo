use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

/// stale 阈值：超过此时长无事件的会话标记为 stale。
const STALE_THRESHOLD_MS: i64 = 10 * 60 * 1000;

/// 托管状态只持有库路径。每个命令按需开短连接——库暂时不可用（被独占锁/损坏/
/// 无权限）时只让该次刷新返回错误，不会在启动时 panic 把整个 app 打挂；
/// 下次 board-changed 事件刷新即自动恢复。
struct AppState {
    db_path: PathBuf,
}

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

fn open_store(path: &PathBuf) -> Result<Store, String> {
    Store::open(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_overview(state: State<AppState>) -> Result<Vec<ProjectOverview>, String> {
    let store = open_store(&state.db_path)?;
    store.overview().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_project_tasks(state: State<AppState>, project_id: i64) -> Result<Vec<TaskCard>, String> {
    let store = open_store(&state.db_path)?;
    store.project_tasks(project_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_live_sessions(state: State<AppState>) -> Result<Vec<LiveSession>, String> {
    let store = open_store(&state.db_path)?;
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

/// 结束 PID 已死的 live 会话（终端关了/被杀）；另把无 pid 且 stale 超过 30 分钟的也判定结束（清历史僵尸）。
fn reap_dead_sessions(db_path: &PathBuf, now_ms: i64) -> usize {
    let store = match Store::open(db_path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let live = match store.live_session_liveness() {
        Ok(v) => v,
        Err(_) => return 0,
    };
    if live.is_empty() {
        return 0;
    }
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let no_pid_stale_cutoff: i64 = 30 * 60 * 1000;
    let mut reaped = 0;
    for (sid, pid, last_event_at) in live {
        let dead = match pid {
            Some(p) if p > 0 => sys.process(Pid::from_u32(p as u32)).is_none(),
            _ => (now_ms - last_event_at) > no_pid_stale_cutoff,
        };
        if dead && store.end_session(sid, now_ms).is_ok() {
            reaped += 1;
        }
    }
    reaped
}

/// 每 60s：先结束 PID 已死的会话，再把超时会话标记为 stale。
fn spawn_stale_sweeper(app: tauri::AppHandle, db_path: PathBuf) {
    std::thread::spawn(move || loop {
        let now = now_ms();
        let reaped = reap_dead_sessions(&db_path, now);
        let staled = match Store::open(&db_path) {
            Ok(store) => store.mark_stale(STALE_THRESHOLD_MS, now).unwrap_or(0),
            Err(_) => 0,
        };
        if reaped > 0 || staled > 0 {
            let _ = app.emit("board-changed", ());
        }
        std::thread::sleep(Duration::from_secs(60));
    });
}

/// 构建系统托盘：显示/隐藏贴纸、开机自启开关、退出。
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let toggle = MenuItemBuilder::with_id("toggle", "显示/隐藏贴纸").build(app)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id("autostart", "开机自启")
        .checked(autostart_on)
        .build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&toggle, &autostart, &quit]).build()?;

    let autostart_item = autostart.clone();
    TrayIconBuilder::with_id("cc-kanban-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("cc-kanban")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle" => {
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                    }
                }
            }
            "autostart" => {
                let mgr = app.autolaunch();
                let now_on = if mgr.is_enabled().unwrap_or(false) {
                    let _ = mgr.disable();
                    false
                } else {
                    let _ = mgr.enable();
                    true
                };
                let _ = autostart_item.set_checked(now_on);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let path = db_path();
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState { db_path: path.clone() })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_project_tasks,
            get_live_sessions
        ])
        .setup(move |app| {
            setup_tray(app)?;
            spawn_db_watcher(app.handle().clone(), path.clone());
            spawn_stale_sweeper(app.handle().clone(), path.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
