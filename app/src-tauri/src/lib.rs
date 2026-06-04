use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
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

#[derive(serde::Serialize)]
struct LiveItem {
    #[serde(flatten)]
    inner: LiveSession,
    connected: bool,
}

#[tauri::command]
fn get_live_sessions(state: State<AppState>) -> Result<Vec<LiveItem>, String> {
    let store = open_store(&state.db_path)?;
    let sessions = store.live_sessions().map_err(|e| e.to_string())?;
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let mut items: Vec<LiveItem> = sessions
        .into_iter()
        .map(|s| {
            let connected = match s.pid {
                Some(p) if p > 0 => sys.process(Pid::from_u32(p as u32)).is_some(),
                _ => false,
            };
            LiveItem { inner: s, connected }
        })
        // 清噪声：过滤 ping 连通性测试 + 未命名无 todo 已断开的旧残留
        .filter(|item| {
            let t = item.inner.task_title.trim();
            // 连通性测试等噪声：标题就是 "ping"
            if t.eq_ignore_ascii_case("ping") {
                return false;
            }
            // 未命名 + 无 todo + 已断开 的旧残留隐藏；连接中的保留
            let unnamed = t.is_empty() || t == "(未命名会话)";
            item.connected || !(unnamed && item.inner.todos.is_empty())
        })
        .collect();
    items.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.inner.session.last_event_at.cmp(&a.inner.session.last_event_at))
    });
    items.truncate(20);
    Ok(items)
}

/// 收集与 root_pid 同控制台组的进程 pid：root + 所有祖先 + 所有子孙。
fn console_group_pids(root_pid: u32) -> HashSet<u32> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let mut set: HashSet<u32> = HashSet::new();
    set.insert(root_pid);
    // 祖先：向上到「终端宿主」为止。遇到桌面壳/系统进程(explorer/sihost/...)就停，
    // 否则会把桌面、任务栏的窗口也算进来，点击时误聚焦到桌面。
    let boundary = [
        "explorer.exe", "sihost.exe", "svchost.exe", "services.exe", "wininit.exe",
        "winlogon.exe", "csrss.exe", "runtimebroker.exe", "dwm.exe",
    ];
    let terminal_host = [
        "windowsterminal.exe", "conhost.exe", "openconsole.exe", "wt.exe",
    ];
    let mut cur = Pid::from_u32(root_pid);
    for _ in 0..32 {
        let Some(parent) = sys.process(cur).and_then(|p| p.parent()) else { break };
        let pname = sys
            .process(parent)
            .map(|p| p.name().to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if boundary.iter().any(|s| pname == *s) {
            break; // 到桌面/系统边界，停止上溯且不纳入
        }
        set.insert(parent.as_u32());
        if terminal_host.iter().any(|s| pname == *s) {
            break; // 已纳入终端宿主，不再继续上溯
        }
        cur = parent;
    }
    // 子孙（BFS：反复扫描，把 parent 在 set 里的进程加入）
    loop {
        let mut added = false;
        for (pid, proc_) in sys.processes() {
            if let Some(parent) = proc_.parent() {
                if set.contains(&parent.as_u32()) && !set.contains(&pid.as_u32()) {
                    set.insert(pid.as_u32());
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    set
}

/// 枚举可见顶层窗口，返回第一个进程 pid 命中 targets 的窗口 HWND。
#[cfg(target_os = "windows")]
fn find_window_for_pids(targets: &HashSet<u32>) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

    struct Ctx<'a> {
        targets: &'a HashSet<u32>,
        found: Option<HWND>,
    }

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let mut wpid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if ctx.targets.contains(&wpid) {
            ctx.found = Some(hwnd);
            return 0; // FALSE：停止枚举
        }
        TRUE
    }

    let mut ctx = Ctx { targets, found: None };
    unsafe {
        EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM);
    }
    ctx.found
}

/// 用 AttachThreadInput 绕过 Windows 后台进程 SetForegroundWindow 限制，可靠置顶目标窗口。
#[cfg(target_os = "windows")]
fn force_foreground(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };
    unsafe {
        let target_thread = GetWindowThreadProcessId(hwnd, null_mut());
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, null_mut())
        };
        let cur = GetCurrentThreadId();

        if fg_thread != 0 && fg_thread != cur {
            AttachThreadInput(cur, fg_thread, 1);
        }
        if target_thread != 0 && target_thread != cur {
            AttachThreadInput(cur, target_thread, 1);
        }

        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);

        if target_thread != 0 && target_thread != cur {
            AttachThreadInput(cur, target_thread, 0);
        }
        if fg_thread != 0 && fg_thread != cur {
            AttachThreadInput(cur, fg_thread, 0);
        }
    }
}

#[tauri::command]
fn focus_session(pid: i64) -> Result<(), String> {
    if pid <= 0 {
        return Err("无效 pid".into());
    }
    #[cfg(target_os = "windows")]
    {
        let targets = console_group_pids(pid as u32);
        let hwnd = find_window_for_pids(&targets).ok_or("未找到该会话的窗口".to_string())?;
        force_foreground(hwnd);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        return Err("仅支持 Windows".into());
    }
    Ok(())
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

/// 每 60s：把超时会话标记为 stale（空闲停转圈）。
/// 不再自动 end 关闭的会话，保留历史会话供贴纸持久显示。
fn spawn_stale_sweeper(app: tauri::AppHandle, db_path: PathBuf) {
    std::thread::spawn(move || loop {
        let now = now_ms();
        let staled = match Store::open(&db_path) {
            Ok(store) => store.mark_stale(STALE_THRESHOLD_MS, now).unwrap_or(0),
            Err(_) => 0,
        };
        if staled > 0 {
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
            get_live_sessions,
            focus_session
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
