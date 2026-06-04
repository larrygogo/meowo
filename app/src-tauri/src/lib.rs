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

/// 吸边判定阈值（物理像素）：窗口边缘距工作区边缘不超过此值即认为贴边。
const SNAP_THRESHOLD: i32 = 20;
/// 竖条逻辑宽度（实际物理宽度 = 该值 * 显示器 scale_factor）。
const STRIP_W_LOGICAL: f64 = 20.0;

/// 矩形（物理像素），用于吸边判定的纯计算。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 吸附的边（左/右/顶）。JS 侧序列化为 "left"/"right"/"top"。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
}

/// 判定窗口 `win` 是否贴在工作区 `work` 的左/右/顶边缘（阈值 `threshold`）。
/// 取在阈值内且最近的一边（平局按 左>右>顶 优先）；都不满足返回 None。纯函数，便于单测。
pub fn edge_for_rect(win: Rect, work: Rect, threshold: i32) -> Option<Edge> {
    let left_gap = (win.x - work.x).abs();
    let right_gap = ((work.x + work.w) - (win.x + win.w)).abs();
    let top_gap = (win.y - work.y).abs();
    let mut best: Option<(Edge, i32)> = None;
    for (edge, gap) in [
        (Edge::Left, left_gap),
        (Edge::Right, right_gap),
        (Edge::Top, top_gap),
    ] {
        if gap <= threshold && best.is_none_or(|(_, b)| gap < b) {
            best = Some((edge, gap));
        }
    }
    best.map(|(e, _)| e)
}

/// snap-changed 事件负载：当前检测到的吸附边（None 表示不贴边）。
#[derive(Clone, serde::Serialize)]
struct SnapPayload {
    edge: Option<Edge>,
}

/// 竖条物理宽度：逻辑宽度 * 显示器缩放，至少 1px。
fn strip_width_phys(scale: f64) -> i32 {
    ((STRIP_W_LOGICAL * scale).round() as i32).max(1)
}

/// 折叠成缩略条：贴到指定边，左/右为竖条、顶为横条。
/// `extent` 是沿条主轴的逻辑长度（竖条=高，横条=宽），由前端按内容（连接中会话数）给出。
#[tauri::command]
fn snap_collapse(window: tauri::WebviewWindow, edge: Edge, extent: f64) -> Result<(), String> {
    let m = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    let wa = m.work_area();
    let scale = m.scale_factor();
    let strip = strip_width_phys(scale); // 条的厚度（物理像素）
    let ext = ((extent * scale).round() as i32).max(1); // 条的主轴长度
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    // (min_w, min_h, w, h, x, y)
    let (min_w, min_h, w, h, x, y) = match edge {
        Edge::Left => (strip, 0, strip, ext, wa.position.x, pos.y),
        Edge::Right => (
            strip,
            0,
            strip,
            ext,
            wa.position.x + wa.size.width as i32 - strip,
            pos.y,
        ),
        Edge::Top => (0, strip, ext, strip, pos.x, wa.position.y),
    };
    // 放开最小宽高限制（tauri.conf 配了 minWidth=320/minHeight=80），否则缩不到缩略条尺寸。
    window
        .set_min_size(Some(tauri::PhysicalSize::new(min_w as u32, min_h as u32)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(w as u32, h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    // 吸附态强制置顶，保证缩略条始终可见。
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    Ok(())
}

/// 偷看展开成全尺寸（仍贴边、保持置顶）：宽高恢复为记住的正常尺寸。
#[tauri::command]
fn snap_expand(window: tauri::WebviewWindow, edge: Edge, width: f64, height: f64) -> Result<(), String> {
    let m = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    let wa = m.work_area();
    let scale = m.scale_factor();
    let phys_w = ((width * scale).round() as i32).max(1);
    let phys_h = ((height * scale).round() as u32).max(1);
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let (x, y) = match edge {
        Edge::Left => (wa.position.x, pos.y),
        Edge::Right => (wa.position.x + wa.size.width as i32 - phys_w, pos.y),
        Edge::Top => (pos.x, wa.position.y),
    };
    // 恢复正常最小尺寸（与 tauri.conf minWidth/minHeight 一致）再展开。
    window
        .set_min_size(Some(tauri::LogicalSize::new(320.0, 80.0)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(phys_w as u32, phys_h))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    Ok(())
}

/// 恢复正常浮动：尺寸设回记住的逻辑宽高，位置维持用户当前拖到的地方。
#[tauri::command]
fn snap_restore(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    pinned: bool,
) -> Result<(), String> {
    // 恢复正常最小尺寸限制，再设回记住的宽高，置顶还原为用户的 pin 偏好。
    window
        .set_min_size(Some(tauri::LogicalSize::new(320.0, 80.0)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window.set_always_on_top(pinned).map_err(|e| e.to_string())?;
    Ok(())
}

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
        .map(|mut s| {
            let connected = match s.pid {
                Some(p) if p > 0 => sys.process(Pid::from_u32(p as u32)).is_some(),
                _ => false,
            };
            // 展示时实时从 transcript 解析 AI 标题：断开/历史会话不会触发 hook，
            // DB 里可能还是旧的首条 prompt。cwd 可能为空（旧会话），resolve_title
            // 会兜底按 session_id 全局查找 transcript 文件。
            if let Some(t) =
                cc_store::title::resolve_title(None, s.cwd.as_deref(), &s.session.cc_session_id)
            {
                s.task_title = t;
            }
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
    // 子孙：只从 root 自身往下 BFS（不经过祖先），否则会把终端宿主的「其它标签页」全抓进来。
    let mut frontier = vec![root_pid];
    while let Some(x) = frontier.pop() {
        for (pid, proc_) in sys.processes() {
            if proc_.parent().map(|p| p.as_u32()) == Some(x) {
                let u = pid.as_u32();
                if set.insert(u) {
                    frontier.push(u);
                }
            }
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

#[tauri::command]
fn set_archived(state: State<AppState>, session_id: i64, archived: bool) -> Result<(), String> {
    let store = open_store(&state.db_path)?;
    store.set_session_archived(session_id, archived).map_err(|e| e.to_string())
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

/// 取当前 live(running/waiting) 会话里进程仍存活的 session id（升序）。
/// 只查「进程在不在」这个外部事实，不按时间臆测状态。
fn alive_session_ids(store: &Store) -> Vec<i64> {
    let sys = System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let mut ids: Vec<i64> = store
        .live_session_liveness()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, pid, _)| match pid {
            Some(p) if *p > 0 => sys.process(Pid::from_u32(*p as u32)).is_some(),
            _ => false,
        })
        .map(|(id, _, _)| id)
        .collect();
    ids.sort_unstable();
    ids
}

/// 周期轮询进程存活：存活集合变化（有会话进程退出）时才发 board-changed，
/// 让前端重算 connected。进程退出不改 DB、notify 监听不到，故需这个轮询兜底。
fn spawn_liveness_watch(app: tauri::AppHandle, db_path: PathBuf) {
    std::thread::spawn(move || {
        let mut last: Vec<i64> = Vec::new();
        loop {
            if let Ok(store) = Store::open(&db_path) {
                let alive = alive_session_ids(&store);
                if alive != last {
                    let _ = app.emit("board-changed", ());
                    last = alive;
                }
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

/// 首次启动：~/.cc-kanban/imported.json 不存在时，后台导入近 7 天历史会话并写标记文件。
/// 出错仅静默（下次启动重试），绝不阻塞窗口创建。
fn spawn_first_import(app: tauri::AppHandle, db_path: PathBuf) {
    std::thread::spawn(move || {
        let Some(dir) = db_path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let marker = dir.join("imported.json");
        if marker.exists() {
            return; // 已导入过，跳过
        }
        let store = match Store::open(&db_path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let now = now_ms();
        if let Ok(count) =
            cc_reporter::import::import_recent(&store, now, cc_reporter::import::ImportOpts::default())
        {
            let body = format!("{{\"imported\":{count},\"at\":{now}}}");
            let _ = std::fs::write(&marker, body);
            if count > 0 {
                let _ = app.emit("board-changed", ());
            }
        }
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
            focus_session,
            set_archived,
            snap_collapse,
            snap_expand,
            snap_restore
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Moved(pos) = event {
                if let (Ok(Some(m)), Ok(size)) = (window.current_monitor(), window.outer_size()) {
                    let wa = m.work_area();
                    let win = Rect { x: pos.x, y: pos.y, w: size.width as i32, h: size.height as i32 };
                    let work = Rect {
                        x: wa.position.x,
                        y: wa.position.y,
                        w: wa.size.width as i32,
                        h: wa.size.height as i32,
                    };
                    let edge = edge_for_rect(win, work, SNAP_THRESHOLD);
                    let _ = window.emit("snap-changed", SnapPayload { edge });
                }
            }
        })
        .setup(move |app| {
            setup_tray(app)?;
            spawn_db_watcher(app.handle().clone(), path.clone());
            spawn_liveness_watch(app.handle().clone(), path.clone());
            spawn_first_import(app.handle().clone(), path.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{edge_for_rect, Edge, Rect};

    const WORK: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };

    // L/R 用例统一用 y=400（远离顶部），避免被顶部判定干扰。
    #[test]
    fn left_within_threshold() {
        let win = Rect { x: 5, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), Some(Edge::Left));
    }

    #[test]
    fn right_within_threshold() {
        let win = Rect { x: 1920 - 300 - 5, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), Some(Edge::Right));
    }

    #[test]
    fn top_within_threshold() {
        let win = Rect { x: 800, y: 8, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), Some(Edge::Top));
    }

    #[test]
    fn center_is_none() {
        let win = Rect { x: 800, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), None);
    }

    #[test]
    fn threshold_boundary_inclusive() {
        let win = Rect { x: 20, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), Some(Edge::Left));
    }

    #[test]
    fn just_outside_threshold_none() {
        let win = Rect { x: 21, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), None);
    }

    #[test]
    fn picks_nearer_edge() {
        // 左距 5 < 右距 10，y 远离顶部 → 取左。
        let work = Rect { x: 0, y: 0, w: 320, h: 1040 };
        let win = Rect { x: 5, y: 400, w: 305, h: 400 };
        assert_eq!(edge_for_rect(win, work, 20), Some(Edge::Left));
    }

    #[test]
    fn top_nearer_than_left() {
        // 左上角附近：顶距 3 < 左距 10 → 取顶。
        let win = Rect { x: 10, y: 3, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, WORK, 20), Some(Edge::Top));
    }

    #[test]
    fn respects_work_area_offset() {
        let work = Rect { x: 100, y: 0, w: 1000, h: 1040 };
        let win = Rect { x: 110, y: 400, w: 300, h: 400 };
        assert_eq!(edge_for_rect(win, work, 20), Some(Edge::Left));
    }
}
