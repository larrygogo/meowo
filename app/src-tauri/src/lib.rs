mod account;
#[cfg(target_os = "macos")]
mod macos;
mod term_script;

use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
// HashSet 仅被 Windows 专属的终端窗口枚举使用（console_group_pids / find_window_for_pids）。
#[cfg(target_os = "windows")]
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

pub mod ccsetup;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
#[cfg(target_os = "windows")]
use sysinfo::Pid;
#[cfg(not(target_os = "macos"))]
use tauri::menu::{MenuBuilder, MenuItemBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

/// 吸边判定阈值（物理像素）：窗口边缘距工作区边缘不超过此值即认为贴边。
#[cfg(not(target_os = "macos"))]
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

/// 两个矩形的相交面积（无重叠为 0）。用于判断窗口是否还落在某个显示器工作区内。纯函数。
pub fn intersection_area(a: Rect, b: Rect) -> i64 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w <= 0 || h <= 0 {
        0
    } else {
        w as i64 * h as i64
    }
}

/// 把窗口左上角最小幅度地钳进工作区，使窗口完整落在 `work` 内，返回钳制后的 (x, y)。
/// 窗口比工作区还大时，左上角对齐工作区原点。纯函数，便于单测。
pub fn clamp_xy_to_work(win: Rect, work: Rect) -> (i32, i32) {
    let clamp_axis = |pos: i32, size: i32, wpos: i32, wsize: i32| -> i32 {
        if size >= wsize {
            wpos
        } else {
            pos.clamp(wpos, wpos + wsize - size)
        }
    };
    (
        clamp_axis(win.x, win.w, work.x, work.w),
        clamp_axis(win.y, win.h, work.y, work.h),
    )
}

/// 把窗口拉回可视区，防止「多显示器拔插/分辨率变化/拖到屏外」后贴纸消失在所有屏幕之外。
/// - `force=false`（救援）：仅当窗口与所有显示器工作区**完全无交集**时才移回主显示器，不打扰正常摆放。
/// - `force=true`（显式找回）：钳进「相交面积最大／主」显示器工作区，确保完整可见。
#[cfg(target_os = "windows")]
fn pull_on_screen(window: &tauri::WebviewWindow, force: bool) {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let win = Rect { x: pos.x, y: pos.y, w: size.width as i32, h: size.height as i32 };
    let Ok(monitors) = window.available_monitors() else { return };
    if monitors.is_empty() {
        return;
    }
    let to_work = |m: &tauri::window::Monitor| {
        let wa = m.work_area();
        Rect { x: wa.position.x, y: wa.position.y, w: wa.size.width as i32, h: wa.size.height as i32 }
    };
    // 找与窗口相交面积最大的显示器工作区。
    let mut best: Option<(i64, Rect)> = None;
    for m in &monitors {
        let work = to_work(m);
        let area = intersection_area(win, work);
        if best.is_none_or(|(a, _)| area > a) {
            best = Some((area, work));
        }
    }
    let (best_area, best_work) = best.unwrap();
    // 救援模式下，只要还跟某个屏有交集就不动。
    if !force && best_area > 0 {
        return;
    }
    // 目标工作区：有交集就用相交最大的那个；完全在屏外则用主显示器（兜底用第一个）。
    let target = if best_area > 0 {
        best_work
    } else {
        window
            .primary_monitor()
            .ok()
            .flatten()
            .map(|m| to_work(&m))
            .unwrap_or(best_work)
    };
    let (x, y) = clamp_xy_to_work(win, target);
    if (x, y) != (win.x, win.y) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// snap-changed 事件负载：当前检测到的吸附边（None 表示不贴边）。
#[cfg(not(target_os = "macos"))]
#[derive(Clone, serde::Serialize)]
struct SnapPayload {
    edge: Option<Edge>,
}

/// 竖条物理宽度：逻辑宽度 * 显示器缩放，至少 1px。
fn strip_width_phys(scale: f64) -> i32 {
    ((STRIP_W_LOGICAL * scale).round() as i32).max(1)
}

/// 仅在置顶状态实际变化时调用 set_always_on_top：避免在透明窗口上重复 SetWindowPos
/// 触发额外重绘闪烁（展开/收起/重测尺寸会反复进入这些命令，但置顶状态多数时候没变）。
fn set_top_if_changed(window: &tauri::WebviewWindow, desired: bool) -> Result<(), String> {
    if window.is_always_on_top().map_err(|e| e.to_string())? != desired {
        window.set_always_on_top(desired).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 真实光标是否在主窗口外接矩形内。用于展开态下判定是否该收回——DOM 的 mouseleave 在
/// 窗口缩放时会误报一串假 leave/enter，不可信；改问 GetCursorPos vs 窗口物理矩形。
/// 取不到坐标/尺寸时一律当作"在内"，避免误折叠。非 Windows 暂恒为 true（不收回）。
#[tauri::command]
fn cursor_over_window(window: tauri::WebviewWindow) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut p) } == 0 {
            return true;
        }
        let (Ok(pos), Ok(sz)) = (window.outer_position(), window.outer_size()) else {
            return true;
        };
        p.x >= pos.x
            && p.x < pos.x + sz.width as i32
            && p.y >= pos.y
            && p.y < pos.y + sz.height as i32
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        true
    }
}

/// 交叉轴居中：让新长度 `new_len` 的窗以原窗口（起点 prev_start、长度 prev_len）中心对齐，
/// 再夹进工作区 [work_start, work_start+work_len) 内，避免居中后越界。纯函数便于单测。
fn center_on(prev_start: i32, prev_len: i32, new_len: i32, work_start: i32, work_len: i32) -> i32 {
    let centered = prev_start + (prev_len - new_len) / 2;
    centered.clamp(work_start, (work_start + work_len - new_len).max(work_start))
}

/// 折叠成缩略条：贴到指定边，左/右为竖条、顶为横条。交叉轴以原窗口中心对齐
/// （吸顶=水平居中，吸左/右=垂直居中）。`extent` 是沿条主轴的逻辑长度，由前端按内容给出。
#[tauri::command]
fn snap_collapse(window: tauri::WebviewWindow, edge: Edge, extent: f64) -> Result<(), String> {
    let extent = extent.clamp(1.0, 20000.0); // 钳上界，防 *scale 后 f64→i32 回绕
    let m = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    let wa = m.work_area();
    let scale = m.scale_factor();
    let strip = strip_width_phys(scale); // 条的厚度（物理像素）
    let ext = ((extent * scale).round() as i32).max(1); // 条的主轴长度
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let sz = window.outer_size().map_err(|e| e.to_string())?;
    let (cur_w, cur_h) = (sz.width as i32, sz.height as i32);
    let (ww, wh) = (wa.size.width as i32, wa.size.height as i32);
    // (min_w, min_h, w, h, x, y)
    let (min_w, min_h, w, h, x, y) = match edge {
        Edge::Left => (
            strip,
            0,
            strip,
            ext,
            wa.position.x,
            center_on(pos.y, cur_h, ext, wa.position.y, wh),
        ),
        Edge::Right => (
            strip,
            0,
            strip,
            ext,
            wa.position.x + ww - strip,
            center_on(pos.y, cur_h, ext, wa.position.y, wh),
        ),
        Edge::Top => (
            0,
            strip,
            ext,
            strip,
            center_on(pos.x, cur_w, ext, wa.position.x, ww),
            wa.position.y,
        ),
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
    // 吸附态强制置顶，保证缩略条始终可见（仅状态变化时设，避免重复触发重绘闪烁）。
    set_top_if_changed(&window, true)?;
    Ok(())
}

/// 偷看展开成全尺寸（仍贴边、保持置顶）：宽高恢复为记住的正常尺寸。
#[tauri::command]
fn snap_expand(window: tauri::WebviewWindow, edge: Edge, width: f64, height: f64) -> Result<(), String> {
    let (width, height) = (width.clamp(1.0, 20000.0), height.clamp(1.0, 20000.0)); // 钳上界防回绕
    let m = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    let wa = m.work_area();
    let scale = m.scale_factor();
    let phys_w = ((width * scale).round() as i32).max(1);
    let phys_h = ((height * scale).round() as u32).max(1);
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let sz = window.outer_size().map_err(|e| e.to_string())?;
    let (cur_w, cur_h) = (sz.width as i32, sz.height as i32);
    let (ww, wh) = (wa.size.width as i32, wa.size.height as i32);
    // 交叉轴以当前（缩略条）中心对齐展开，与折叠态保持同一中心，不跳回左/上对齐。
    let (x, y) = match edge {
        Edge::Left => (wa.position.x, center_on(pos.y, cur_h, phys_h as i32, wa.position.y, wh)),
        Edge::Right => (
            wa.position.x + ww - phys_w,
            center_on(pos.y, cur_h, phys_h as i32, wa.position.y, wh),
        ),
        Edge::Top => (center_on(pos.x, cur_w, phys_w, wa.position.x, ww), wa.position.y),
    };
    // 恢复正常最小尺寸（与 tauri.conf minWidth/minHeight 一致）再展开，就地放大到贴边位置。
    window
        .set_min_size(Some(tauri::LogicalSize::new(320.0, 80.0)))
        .map_err(|e| e.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(phys_w as u32, phys_h))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    set_top_if_changed(&window, true)?;
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
    set_top_if_changed(&window, pinned)?;
    Ok(())
}

/// 拖角缩放触发的「解除吸附」：保留用户当前拖出的尺寸/位置，只复位最小尺寸与置顶（按 pin 偏好）。
/// 解除后窗口即普通浮动窗口，再拖到屏幕边缘仍会被吸附逻辑重新吸附。
#[tauri::command]
fn unsnap(window: tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
    window
        .set_min_size(Some(tauri::LogicalSize::new(320.0, 80.0)))
        .map_err(|e| e.to_string())?;
    set_top_if_changed(&window, pinned)?;
    Ok(())
}

/// 托管状态只持有库路径。每个命令按需开短连接——库暂时不可用（被独占锁/损坏/
/// 无权限）时只让该次刷新返回错误，不会在启动时 panic 把整个 app 打挂；
/// 下次 board-changed 事件刷新即自动恢复。
struct AppState {
    db_path: PathBuf,
    /// transcript 增量解析缓存（与后台轮询线程共享 Arc）：避免每次刷新重读整文件。
    tx_cache: Arc<Mutex<cc_store::TranscriptCache>>,
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
    errored: bool,
    error_label: Option<String>,
    error_raw: Option<String>,
    // 注：context_pct / context_window 来自 inner(LiveSession)，由 statusline 写库、flatten 输出。
}

/// 贴纸最多展示的会话数。
const LIVE_LIMIT: usize = 20;

#[tauri::command]
async fn get_live_sessions(state: State<'_, AppState>) -> Result<Vec<LiveItem>, String> {
    // 重逻辑（SQLite、进程枚举、transcript 解析）放 blocking 线程池，不占主线程事件循环。
    let db_path = state.db_path.clone();
    let tx_cache = state.tx_cache.clone();
    tauri::async_runtime::spawn_blocking(move || live_sessions_blocking(&db_path, &tx_cache))
        .await
        .map_err(|e| e.to_string())?
}

fn live_sessions_blocking(
    db_path: &PathBuf,
    tx_cache: &Mutex<cc_store::TranscriptCache>,
) -> Result<Vec<LiveItem>, String> {
    let store = open_store(db_path)?;
    let sessions = store.live_sessions().map_err(|e| e.to_string())?;
    // connected 校验：Windows 走 sysinfo 进程表；macOS/Unix 一次 ps 批量快照
    // （sysinfo 在 macOS 上不可靠，逐 pid spawn ps 又太慢——一批会话只扫一次）。
    #[cfg(target_os = "windows")]
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    #[cfg(target_os = "windows")]
    let is_claude = |pid: i64| pid_is_claude(&sys, pid);
    #[cfg(not(target_os = "windows"))]
    let claude_pids = claude_pids_snapshot();
    #[cfg(not(target_os = "windows"))]
    let is_claude = |pid: i64| pid > 0 && claude_pids.contains(&pid);

    // 先算 connected（廉价，仅查进程表）并据此排序，再只对「将要展示」的会话解析
    // transcript 标题——标题解析要 read_to_string 整个 JSONL（可达数 MB），对最多 100 个
    // 会话全做一遍再截断到 20 是巨大的无谓 I/O（每 ~300ms 一次）。
    let mut ranked: Vec<(LiveSession, bool)> = sessions
        .into_iter()
        .map(|s| {
            // 已结束(ended)的会话一律视为断开；并校验 pid 确属 claude，
            // 防 Windows pid 复用（旧 pid 被 esbuild 等占用）误判为「连接中」。
            let connected =
                s.session.status != "ended" && is_claude(s.pid.unwrap_or(0));
            (s, connected)
        })
        .collect();
    // 连接中优先，其次最近活跃。live_sessions() 已按 last_event_at DESC 返回，
    // 稳定排序按 connected 分组即保留组内的时间序。
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));

    // 逐条解析标题并过滤，凑满 LIVE_LIMIT 即停。连接中的会话排在最前、必然保留，
    // 故它们（正在活跃、文件确实在变）总能拿到实时标题；断开的只解析到补满列表为止。
    let mut items: Vec<LiveItem> = Vec::with_capacity(LIVE_LIMIT);
    for (mut s, connected) in ranked {
        if items.len() >= LIVE_LIMIT {
            break;
        }
        // 一次读 transcript 拿标题与错误（断开/历史会话不触发 hook，DB 可能是旧值）。
        // 走增量缓存：只解析新追加的行，避免每轮重读整文件（大 transcript 可达数百 ms，会拖慢整窗）。
        // 上下文百分比不在这里算——它由 statusline 写库、随 LiveSession flatten 输出。
        let mut error_label: Option<String> = None;
        let mut error_raw: Option<String> = None;
        let info = cc_store::title::resolve_transcript_path(
            None,
            s.cwd.as_deref(),
            &s.session.cc_session_id,
        )
        .and_then(|p| p.to_str().map(str::to_string))
        .map(|path| tx_cache.lock().unwrap_or_else(|e| e.into_inner()).analyze(&path));
        if let Some(info) = info {
            if let Some(t) = info.title {
                s.task_title = t;
            }
            if let Some(e) = info.error {
                error_label = Some(e.label);
                error_raw = Some(e.raw);
            }
        }
        // 清噪声：过滤 ping 连通性测试 + 未命名无 todo 已断开的旧残留。
        let t = s.task_title.trim();
        if t.eq_ignore_ascii_case("ping") {
            continue;
        }
        let unnamed = t.is_empty() || t == "(未命名会话)";
        if !connected && unnamed && s.todos.is_empty() {
            continue;
        }
        items.push(LiveItem {
            inner: s,
            connected,
            errored: error_label.is_some(),
            error_label,
            error_raw,
        });
    }
    Ok(items)
}

/// 收集与 root_pid 同控制台组的进程 pid：root + 所有祖先 + 所有子孙。
#[cfg(target_os = "windows")]
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

/// claude 会把任务标题写进 Windows Terminal 标签页，并加一个**会随状态变化**的前缀符号：
/// 运行时是 braille spinner(⠐⠂…)，空闲/待输入时是 ✳(U+2733)，可能还有其它符号。
/// 归一化：剥掉开头所有「非字母数字」字符（覆盖任意状态符号 + 空格；任务标题几乎总以
/// 字母/数字/CJK 开头），并去掉尾部空白与截断省略号(…/...)。纯函数，便于单测。
#[allow(dead_code)] // 跨平台纯函数：非 Windows 上无运行时调用方，仅单测使用
fn normalize_tab_title(s: &str) -> &str {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end()
        .trim_end_matches(['…', '.'])
        .trim_end()
}

/// 标签页标题 `tab_name` 与会话标题 `want` 的匹配强度：2=精确(归一化后相等)，1=单向包含，0=不匹配。
/// 包含是**双向**的：兼容 claude 对长标题的截断(tab 标题是 want 的前缀)与轻微漂移。
/// `want` 为空或占位("(未命名会话)")时不参与匹配(返回 0)，避免误命中无关标签页。纯函数。
#[allow(dead_code)] // 同上：跨平台纯函数，仅单测调用
fn tab_match_score(tab_name: &str, want: &str) -> u8 {
    let want = want.trim();
    if want.is_empty() || want == "(未命名会话)" {
        return 0;
    }
    let norm = normalize_tab_title(tab_name);
    if norm.is_empty() {
        return 0;
    }
    if norm == want {
        2
    } else if norm.contains(want) || want.contains(norm) {
        1
    } else {
        0
    }
}

/// 用 UI Automation 把对应会话的 Windows Terminal 标签页切到前台。
///
/// WT 单进程托管多标签/多窗口，按进程 PID 无法区分标签页（所有标签页同一个 HWND）。
/// 但 claude 会把任务标题写进标签页标题，故按标题精确定位标签页：枚举所有 WT 窗口的
/// TabItem，取匹配分最高的标签页，`Select` 选中后置前其窗口。命中返回 true；失败/无匹配返回 false。
///
/// 性能：仅当出现「多个同分标签页」需要消歧时，才用 `console_group_pids(root_pid)` 做一次进程扫描
/// (昂贵，要枚举系统所有进程)；常见的唯一精确匹配走纯 UIA 路径(~十几 ms)，不扫进程。
///
/// 注意：本函数必须在「干净 COM apartment 的线程」上调用（见 `focus_session` 的后台线程）。
/// `UIAutomation::new()` 会 CoInitialize 当前线程，Tauri 主线程已是 STA，复用会因 apartment 冲突失败。
#[cfg(target_os = "windows")]
fn focus_terminal_tab(root_pid: u32, want: &str) -> bool {
    use uiautomation::patterns::UISelectionItemPattern;
    use uiautomation::types::{ControlType, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    let Ok(automation) = UIAutomation::new() else { return false };
    let Ok(root) = automation.get_root_element() else { return false };

    // 所有 Windows Terminal 顶层窗口（窗口类名固定）。timeout(0) 避免无匹配时阻塞重试。
    let Ok(wt_windows) = automation
        .create_matcher()
        .from(root)
        .classname("CASCADIA_HOSTING_WINDOW_CLASS")
        .timeout(0)
        .find_all()
    else {
        return false;
    };

    // 用 UIA 原生 FindAll(Descendants, ControlType==TabItem)：进程内优化执行，快且不会爬进
    // 终端文本树（crate 的 matcher 是 Rust 侧手动遍历，深度大时可能很慢）。
    let Ok(tab_cond) = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(ControlType::TabItem as i32),
        None,
    ) else {
        return false;
    };

    // 收集所有命中标签页：(匹配分, 所属窗口 pid, 标签页元素, 所属窗口元素)。先不算进程组。
    let mut matches: Vec<(u8, u32, UIElement, UIElement)> = Vec::new();
    for win in wt_windows {
        let win_pid = win
            .get_property_value(UIProperty::ProcessId)
            .ok()
            .and_then(|v| TryInto::<i32>::try_into(v).ok())
            .map(|i| i as u32)
            .unwrap_or(0);
        let Ok(tabs) = win.find_all(TreeScope::Descendants, &tab_cond) else {
            continue;
        };
        for tab in tabs {
            let score = tab_match_score(&tab.get_name().unwrap_or_default(), want);
            if score > 0 {
                matches.push((score, win_pid, tab, win.clone()));
            }
        }
    }

    let max_score = matches.iter().map(|m| m.0).max().unwrap_or(0);
    if max_score == 0 {
        return false;
    }
    // 只保留最高分候选。
    matches.retain(|m| m.0 == max_score);
    // 唯一候选直接用；多个同分时才扫进程组，优先选与本会话同进程组的窗口。
    let idx = if matches.len() == 1 {
        0
    } else {
        let group = console_group_pids(root_pid);
        matches.iter().position(|m| group.contains(&m.1)).unwrap_or(0)
    };
    let (_, _, tab, win) = &matches[idx];

    // 选中该标签页（即使其窗口当前在后台也会切换激活标签页）。
    if let Ok(p) = tab.get_pattern::<UISelectionItemPattern>() {
        let _ = p.select();
    }
    // 置前其所属窗口（这一步也解决了多 WT 窗口共用 PID 时聚焦错窗口的问题）。
    if let Ok(handle) = win.get_native_window_handle() {
        let hwnd_isize: isize = handle.into();
        force_foreground(hwnd_isize as windows_sys::Win32::Foundation::HWND);
        return true;
    }
    false
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

/// 聚焦某会话的终端：优先按标题用 UIA 精确切到对应 WT 标签页，否则按进程组找窗口置前。
/// 放后台线程 fire-and-forget（保证干净 COM apartment + 不阻塞调用方）。
/// 供 focus_session 命令与「点击通知」回调共用。仅 Windows（两个调用点均 cfg-gated，故函数整体也 gate）。
#[cfg(target_os = "windows")]
fn focus_session_terminal(pid: i64, title: Option<String>) {
    std::thread::spawn(move || {
        // 首选：按标题用 UIA 精确切到对应 WT 标签页（解决单进程多标签/多窗口下按 PID 对应不上）。
        if let Some(t) = title.as_deref() {
            if focus_terminal_tab(pid as u32, t) {
                return;
            }
        }
        // 兜底：传统 conhost（每窗口独立进程）等场景，扫进程组按 PID 找顶层窗口置前。
        let targets = console_group_pids(pid as u32);
        if let Some(hwnd) = find_window_for_pids(&targets) {
            force_foreground(hwnd);
        }
    });
}

/// iTerm2 是否安装（任意常见位置）：先查标准路径，再用 mdfind 按 bundle id 兜底。
#[cfg(target_os = "macos")]
fn iterm_installed() -> bool {
    use std::path::Path;
    if Path::new("/Applications/iTerm.app").exists() {
        return true;
    }
    if let Ok(home) = std::env::var("HOME") {
        if Path::new(&home).join("Applications/iTerm.app").exists() {
            return true;
        }
    }
    std::process::Command::new("mdfind")
        .arg("kMDItemCFBundleIdentifier == 'com.googlecode.iterm2'")
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// 读设置得出「打开未连接会话」用的终端宿主（macOS）。缺省 Terminal.app；
/// 选了 iTerm2 但未安装时回退 Terminal.app（避免 AppleScript 静默失败）。
#[cfg(target_os = "macos")]
fn resume_terminal_kind() -> crate::term_script::TermKind {
    use crate::term_script::TermKind;
    match crate::term_script::resume_kind_from_setting(&load_settings().resume_terminal) {
        TermKind::ITerm2 if iterm_installed() => TermKind::ITerm2,
        TermKind::ITerm2 => TermKind::Terminal,
        other => other,
    }
}

#[tauri::command]
fn focus_session(
    pid: i64,
    title: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
) -> Result<(), String> {
    if pid <= 0 {
        return Err("无效 pid".into());
    }
    // 与 resume_session 同一契约：凡可能进入 `claude --resume <id>` 路径的 session_id 一律先校验
    // 为 UUID 形态，杜绝注入（macOS 分支会把 id 经 osascript 注入 AppleScript）。
    if let Some(id) = session_id.as_deref() {
        if !is_session_id(id) {
            return Err("无效 session_id".into());
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (cwd, session_id);
        focus_session_terminal(pid, title);
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = title;
        // ps/osascript（含首次 TCC 授权弹窗）可能长时间阻塞，放后台线程 fire-and-forget，
        // 与 Windows 的 focus_session_terminal 模式对齐，不挡主线程事件循环。
        std::thread::spawn(move || {
            crate::macos::terminal::focus_session_terminal(
                pid,
                cwd.as_deref(),
                session_id.as_deref(),
                resume_terminal_kind(),
            );
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (pid, title, cwd, session_id);
        Err("当前平台不支持".into())
    }
}

/// 会话 id 是否为合法 UUID 形态（仅十六进制与连字符，长度 36）。
/// 用于命令注入防护：通过校验即保证 id 不含引号/分号/空格等任何 shell/wt 元字符。纯函数。
fn is_session_id(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// 把 `cwd` 收敛成「可安全传给 wt -d」的目录：必须非空、真实存在的目录，且不含会破坏 wt
/// 命令行解析的元字符(`;` `"`)。不满足则返回 None（调用方退化为不带 -d）。
/// 在 PATH 各目录中查找指定文件是否存在。不 spawn `where` 子进程——GUI 进程冷启动后
/// 首次 spawn 控制台子进程要数秒（新建 conhost + 杀软扫描），而同步命令跑在主线程，
/// 会把整个事件循环（所有窗口）堵死，这正是 0.2.0 设置页在 Windows 上"卡死"的根因。
/// 用 symlink_metadata 而非 exists()：wt.exe 通常是 App Execution Alias
/// （APPEXECLINK reparse point），fs::metadata 跟随它会失败、误判为不存在。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn path_has_exe(path_var: &std::ffi::OsStr, exe: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join(exe).symlink_metadata().is_ok())
}

/// Windows Terminal（wt.exe）是否在 PATH 上。进程内缓存：安装状态运行期间基本不变，
/// resume_session 每次恢复会话都要查询，保持微秒级。
#[cfg(target_os = "windows")]
fn wt_available() -> bool {
    use std::sync::OnceLock;
    static WT_ON_PATH: OnceLock<bool> = OnceLock::new();
    *WT_ON_PATH.get_or_init(|| {
        std::env::var_os("PATH").is_some_and(|p| path_has_exe(&p, "wt.exe"))
    })
}

/// 定位 Windows Terminal 的 settings.json（Store 版 / Preview / 未打包版三处）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn wt_settings_path() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    [
        r"Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json",
        r"Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
        r"Microsoft\Windows Terminal\settings.json",
    ]
    .into_iter()
    .map(|rel| base.join(rel))
    .find(|p| p.is_file())
}

/// 去掉 JSONC 注释（WT settings.json 允许 // 与 /* */，且字符串里常有 URL 的 //）。
/// 按字节扫描、正确跳过字符串与转义，不破坏多字节 UTF-8（profile 名可能含中文）。纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn strip_jsonc_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]); // 保留转义字符，避免把 \" 误判为字符串结束
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
        } else if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            i += 2;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// 从 WT settings.json 的 JSON 取默认 profile 名：defaultProfile 为 GUID 时在 profiles.list
/// 按 guid 找 name（大小写不敏感）；本身是名字则直接用。找不到则 None。纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_wt_default_profile(v: &serde_json::Value) -> Option<String> {
    let def = v.get("defaultProfile").and_then(|x| x.as_str())?.trim();
    if def.is_empty() {
        return None;
    }
    if !def.starts_with('{') {
        return Some(def.to_string()); // 直接配的是 profile 名
    }
    // 新格式 profiles.list 是数组；老格式 profiles 直接是数组。
    let list = v
        .get("profiles")
        .and_then(|p| p.get("list").and_then(|l| l.as_array()).or_else(|| p.as_array()))?;
    list.iter().find_map(|prof| {
        let guid = prof.get("guid").and_then(|g| g.as_str())?;
        guid.eq_ignore_ascii_case(def)
            .then(|| prof.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .flatten()
    })
}

/// 用户 WT 默认 profile 名（多为 PowerShell）。进程内缓存：与 wt_available 一致，运行期基本不变
/// （改了默认 profile 需重启 app 才生效）。读不到/解析失败/无匹配 → None，调用方退化为不带 -p。
#[cfg(target_os = "windows")]
fn wt_default_profile() -> Option<String> {
    use std::sync::OnceLock;
    static PROFILE: OnceLock<Option<String>> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            let raw = std::fs::read_to_string(wt_settings_path()?).ok()?;
            let v: serde_json::Value = serde_json::from_str(&strip_jsonc_comments(&raw)).ok()?;
            parse_wt_default_profile(&v)
        })
        .clone()
}

#[cfg(target_os = "windows")]
fn safe_cwd(cwd: Option<&str>) -> Option<String> {
    let d = cwd?.trim();
    // 含 ; " 会破坏命令行解析；以 - 开头会被 wt 当成选项（真实 Windows 路径不会以 - 开头）。
    if d.is_empty() || d.contains([';', '"']) || d.starts_with('-') {
        return None;
    }
    std::path::Path::new(d).is_dir().then(|| d.to_string())
}

/// 恢复一个已断开的会话：在其原工作目录 `cwd` 新开一个终端跑 `claude --resume <session_id>`。
/// 终端按设置 `resume_terminal` 选择——Windows：wt(默认)/powershell/cmd；macOS：Terminal/iTerm2。
/// `cwd` 缺失/非法(旧会话)时不带 cwd，尽力按 id 恢复。
///
/// 安全：`session_id` 严格校验为 UUID 形态（无空格/元字符）；wt 分支把 claude/--resume/id 作为**独立 argv**
/// 传入；powershell/cmd 分支用 `current_dir` 传工作目录（不进命令串）、命令串只含已校验的 id，从源头杜绝注入。
#[tauri::command]
fn resume_session(cwd: Option<String>, session_id: String) -> Result<(), String> {
    if !is_session_id(&session_id) {
        return Err("无效 session_id".into());
    }
    #[cfg(target_os = "windows")]
    {
        // 冷启动后首次 spawn 控制台子进程可达数秒（新建 conhost + 杀软扫描），resolve_cwd 还要读
        // transcript；同步命令跑在主线程，整段挪后台线程，命令立即返回。spawn 失败仅打印日志。
        std::thread::spawn(move || {
            use std::os::windows::process::CommandExt;
            use std::process::Command;
            const CREATE_NEW_CONSOLE: u32 = 0x0000_0010; // 让 pwsh/cmd 各自独立成窗

            // claude --resume 必须在会话原项目目录下运行才找得到会话。DB 的 cwd 可能为空(旧会话/
            // 压缩漏 SessionStart)，故用 resolve_cwd 从 transcript 兜底解析真实 cwd。
            let resolved_cwd = cc_store::title::resolve_cwd(cwd.as_deref(), &session_id);
            let dir = safe_cwd(resolved_cwd.as_deref()); // Option<String>：真实存在的目录
            // 选了 wt（或默认/旧值映射到 wt）但机器上没装 wt 时，回退 PowerShell（Windows 必有）。
            let eff = match load_settings().resume_terminal.as_str() {
                "powershell" => "powershell",
                "cmd" => "cmd",
                _ if wt_available() => "wt",
                _ => "powershell",
            };
            let spawned = match eff {
                // 新开独立控制台窗口跑 PowerShell；-NoExit 保留窗口，claude 在 current_dir 下启动。
                "powershell" => {
                    let mut c = Command::new("powershell");
                    c.args(["-NoExit", "-Command", &format!("claude --resume {session_id}")]);
                    if let Some(d) = &dir {
                        c.current_dir(d);
                    }
                    c.creation_flags(CREATE_NEW_CONSOLE).spawn()
                }
                // cmd /k 跑完命令后保留窗口；工作目录走 current_dir。
                "cmd" => {
                    let mut c = Command::new("cmd");
                    c.args(["/k", &format!("claude --resume {session_id}")]);
                    if let Some(d) = &dir {
                        c.current_dir(d);
                    }
                    c.creation_flags(CREATE_NEW_CONSOLE).spawn()
                }
                // eff == "wt"：Windows Terminal。wt -w 0 nt [-d <cwd>] claude --resume <id>，
                // 在最近 WT 窗口新开标签页，独立 argv 不拼 shell 串。
                _ => {
                    let mut args: Vec<String> = vec!["-w".into(), "0".into(), "nt".into()];
                    // 用用户默认 profile（多为 PowerShell）渲染标签：否则 WT 对裸命令行套用 cmd
                    // 基础配置，图标/配色/环境都是 cmd，看起来"还是 cmd"。读不到则不带 -p，沿用旧行为。
                    if let Some(p) = wt_default_profile() {
                        args.push("-p".into());
                        args.push(p);
                    }
                    if let Some(d) = &dir {
                        args.push("-d".into());
                        args.push(d.clone());
                    }
                    args.push("claude".into());
                    args.push("--resume".into());
                    args.push(session_id.clone());
                    Command::new("wt").args(&args).spawn()
                }
            };
            if let Err(e) = spawned {
                eprintln!("恢复会话：启动 {eff} 失败：{e}");
            }
        });
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // 与 Windows 一致：DB 的 cwd 可能为空，用 resolve_cwd 从 transcript 兜底解析。
        // resolve_cwd 读 transcript、osascript 可能等 TCC 授权，整段放后台线程不挡主线程。
        std::thread::spawn(move || {
            let resolved = cc_store::title::resolve_cwd(cwd.as_deref(), &session_id);
            crate::macos::terminal::resume_session_mac(
                resolved.as_deref(),
                &session_id,
                resume_terminal_kind(),
            );
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = cwd;
        Err("当前平台不支持".into())
    }
}

/// 在贴纸上重命名会话：往该会话 transcript 追加一条 custom-title 记录
/// （与 Claude Code `/rename` 写入格式完全一致），并同步更新 DB 标题。
/// custom-title 优先级高于 ai-title，故贴纸与 Claude Code `/resume` 列表都会显示新名字。
///
/// 安全：session_id 严格校验为 UUID 形态（同时杜绝路径穿越），title 经 trim + 截断；
/// 写入用 serde_json 序列化，转义由库保证。
#[tauri::command]
fn rename_session(
    app: tauri::AppHandle,
    state: State<AppState>,
    cwd: Option<String>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    if !is_session_id(&session_id) {
        return Err("无效 session_id".into());
    }
    let title: String = title.trim().chars().take(80).collect();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }

    // 定位 transcript：优先用 cwd 重建路径，否则按 session_id 全局查找。
    let path = cc_store::title::resolve_cwd(cwd.as_deref(), &session_id)
        .and_then(|c| cc_store::title::reconstruct_transcript_path(&c, &session_id))
        .filter(|p| p.exists())
        .or_else(|| cc_store::title::find_transcript_by_session(&session_id))
        .ok_or("找不到该会话的 transcript")?;

    let record = serde_json::json!({
        "type": "custom-title",
        "customTitle": title,
        "sessionId": session_id,
    });
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| format!("打开 transcript 失败：{e}"))?;
        writeln!(f, "{record}").map_err(|e| format!("写入 transcript 失败：{e}"))?;
    }

    // 同步 DB 标题，让总览等非贴纸视图也一致（best-effort）。
    if let Ok(store) = open_store(&state.db_path) {
        if let Ok(Some(sid)) = store.find_session_id_pub(&session_id) {
            let _ = store.set_session_title(sid, &title, now_ms());
        }
    }
    let _ = app.emit("board-changed", ());
    Ok(())
}

#[tauri::command]
fn set_archived(state: State<AppState>, session_id: i64, archived: bool) -> Result<(), String> {
    let store = open_store(&state.db_path)?;
    store.set_session_archived(session_id, archived, now_ms()).map_err(|e| e.to_string())
}

fn default_true() -> bool {
    true
}
/// 外观默认值（与前端 appearance.ts / styles.css 的初值保持一致）。
fn default_theme() -> String {
    "dark".to_string()
}
fn default_opacity() -> u32 {
    94
}
fn default_ui_scale() -> u32 {
    100
}
fn default_resume_terminal() -> String {
    "terminal".to_string()
}
fn default_language() -> String {
    "auto".to_string()
}

/// 应用设置（持久化到 ~/.cc-kanban/settings.json）。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Settings {
    /// 归档条目自动隐藏的天数；0 = 永不隐藏。
    #[serde(default)]
    archive_hide_days: u32,
    /// 桌面通知总开关（待交互 + 错误）。缺省为开启，兼容老 settings.json。
    #[serde(default = "default_true")]
    notifications_enabled: bool,
    /// 外观模式：dark / light / system（跟随系统）。缺省 dark，兼容老 settings.json。
    #[serde(default = "default_theme")]
    theme: String,
    /// 贴纸背景不透明度（百分比 60–100）。缺省 94，与原视觉一致。
    #[serde(default = "default_opacity")]
    opacity: u32,
    /// 界面密度/字号缩放（百分比，紧凑 90 / 标准 100 / 宽松 112）。
    #[serde(default = "default_ui_scale")]
    ui_scale: u32,
    /// 打开未连接会话用的终端（macOS）：terminal = Terminal.app，iterm = iTerm2。缺省 terminal，兼容老 settings.json。
    #[serde(default = "default_resume_terminal")]
    resume_terminal: String,
    /// 界面/通知语言：auto（跟随系统）/ zh / en。缺省 auto，兼容老 settings.json。
    #[serde(default = "default_language")]
    language: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            archive_hide_days: 0,
            notifications_enabled: true,
            theme: default_theme(),
            opacity: default_opacity(),
            ui_scale: default_ui_scale(),
            resume_terminal: default_resume_terminal(),
            language: default_language(),
        }
    }
}

/// 解析生效语言：settings.language 为 zh/en 用之；auto 按系统 locale（zh* → zh，其余 en）。
fn ui_lang(settings: &Settings) -> &'static str {
    match settings.language.as_str() {
        "zh" => "zh",
        "en" => "en",
        _ => {
            if sys_locale::get_locale().map(|l| l.starts_with("zh")).unwrap_or(false) {
                "zh"
            } else {
                "en"
            }
        }
    }
}

/// Rust 侧用户可见文案（仅通知/托盘/窗口标题数条，不引 i18n 库）。
fn tr(lang: &str, key: &str) -> &'static str {
    match (lang, key) {
        ("en", "notify.error") => "Session error",
        ("en", "notify.waiting") => "Waiting for your reply",
        ("en", "tray.settings") => "Settings",
        ("en", "tray.quit") => "Quit",
        ("en", "window.settings") => "Settings",
        (_, "notify.error") => "会话出错",
        (_, "notify.waiting") => "等待你回复",
        (_, "tray.settings") => "设置",
        (_, "tray.quit") => "退出",
        (_, "window.settings") => "设置",
        _ => "",
    }
}

fn settings_path() -> PathBuf {
    db_path().with_file_name("settings.json")
}

fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn get_settings() -> Settings {
    load_settings()
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, mut settings: Settings) -> Result<(), String> {
    // 后端兜底钳值（与前端 appearance.ts 一致），防越界值落盘后被 5s 轮询线程读到。
    settings.opacity = settings.opacity.clamp(25, 100);
    settings.ui_scale = settings.ui_scale.clamp(50, 200);
    let body = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // 原子写（tmp + rename）：后台轮询线程每 5s 裸读本文件，直写可能被读到半截而回退默认值。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp); // best-effort 清理，避免遗留 .tmp
        return Err(e.to_string());
    }
    // 切语言后重建托盘菜单/窗口标题（无条件重建，菜单仅两项，幂等且廉价）。
    apply_language(&app, ui_lang(&settings));
    // 通知贴纸窗口实时套用新设置。
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

/// 设置窗口用：读取/切换开机自启（原来只在托盘，托盘精简后搬到设置页）。
#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    Ok(app.autolaunch().is_enabled().unwrap_or(false))
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
}

/// 设置/关于页用：在默认浏览器打开本项目链接。仅允许本仓库的 https 链接（白名单），
/// Windows 用 explorer、macOS 用 open 打开（均不经 shell），杜绝被滥用打开任意/恶意目标。
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/larrygogo/cc-kanban") {
        return Err("不允许的链接".into());
    }
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    // macOS：open 偶发慢（默认浏览器冷启动），放后台线程不挡主线程。
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    });
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = url;
    Ok(())
}

/// 监听 board.db 所在目录变更，去抖后向前端发 "board-changed"。
fn spawn_db_watcher(app: tauri::AppHandle, db_path: PathBuf) {
    let watch_dir = db_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    // 只关心 db 本体及其 -wal/-shm/-journal 等伴生文件；同目录的 settings.json、
    // usage-cache.json 写入不应触发看板刷新。
    let db_name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "board.db".to_string());
    std::thread::spawn(move || {
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        if watcher.watch(&watch_dir, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        let is_board = |res: &Result<notify::Event, notify::Error>| -> bool {
            let Ok(ev) = res else { return false };
            ev.paths.iter().any(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n.strip_prefix(db_name.as_str())
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
                })
            })
        };
        // trailing debounce：收到相关事件后 drain 到 300ms 静默再 emit。SQLite 提交是
        // db/-wal/-shm 多个事件的爆发，前沿触发会丢掉尾部事件、让前端停在旧数据。
        let debounce = Duration::from_millis(300);
        loop {
            let Ok(first) = rx.recv() else { return }; // watcher 关闭 → 线程退出
            let mut relevant = is_board(&first);
            loop {
                match rx.recv_timeout(debounce) {
                    Ok(ev) => relevant = relevant || is_board(&ev),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        if relevant {
                            let _ = app.emit("board-changed", ());
                        }
                        return;
                    }
                }
            }
            if relevant {
                let _ = app.emit("board-changed", ());
            }
        }
    });
}

/// pid 对应的进程是否确实是 claude。
///
/// Windows 会复用 pid：会话结束后它的旧 pid 可能被别的进程（如 esbuild）占用，
/// 只判断「pid 是否存在」会把已结束的会话误判为仍连接。按进程名含 "claude" 甄别。
fn pid_is_claude(sys: &System, pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        sys.process(Pid::from_u32(pid as u32))
            .map(|p| p.name().to_string_lossy().to_ascii_lowercase().contains("claude"))
            .unwrap_or(false)
    }
    // macOS/Unix：sysinfo 对进程的可见性不稳（实测 parent() 会过早返回 None、
    // 最小刷新下 name 是否可靠也无保证），改用 ps 校验，与 cc-reporter::owner_pid 一致。
    // 仅对「非 ended 的活跃会话」调用，每轮就几个，ps 开销可忽略。
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sys;
        let Ok(out) = std::process::Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .output()
        else {
            return false;
        };
        String::from_utf8_lossy(&out.stdout)
            .to_ascii_lowercase()
            .contains("claude")
    }
}

/// macOS/Unix：一次 `ps -axo pid=,comm=` 批量取「进程名含 claude」的 pid 集合，
/// 供 live_sessions_blocking 整批校验 connected，替代逐 pid spawn ps。
#[cfg(not(target_os = "windows"))]
fn claude_pids_snapshot() -> std::collections::HashSet<i64> {
    let mut set = std::collections::HashSet::new();
    let Ok(out) = std::process::Command::new("ps")
        .args(["-axo", "pid=,comm="])
        .output()
    else {
        return set;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut it = line.split_whitespace();
        let Some(pid) = it.next().and_then(|p| p.parse::<i64>().ok()) else { continue };
        // comm 在 macOS 上是可执行文件全路径，可能含空格 → 余下字段拼回。
        let comm = it.collect::<Vec<_>>().join(" ");
        if comm.to_ascii_lowercase().contains("claude") {
            set.insert(pid);
        }
    }
    set
}

/// 轮询一次：把「记录了 pid、但该进程已死」的 live 会话收尾为 ended（self-heal），
/// 并返回仍存活的 session id（升序）与本轮收尾的数量。
///
/// 终端被关/被 /clear 打断时 SessionEnd 往往不触发，会话状态会永远卡在 running/waiting；
/// 进程都没了就该收尾。pid 为空的不动（可能是刚启动还没抓到 pid，宁可不臆测）。
fn reap_and_alive_ids(store: &Store, sys: &System, now_ms: i64) -> (Vec<i64>, usize) {
    let mut alive: Vec<i64> = Vec::new();
    let mut reaped = 0usize;
    for (id, pid, _) in store.live_session_liveness().unwrap_or_default() {
        match pid {
            Some(p) if p > 0 => {
                if pid_is_claude(sys, p) {
                    alive.push(id);
                } else if store.end_session(id, now_ms).is_ok() {
                    reaped += 1; // 进程已死 / pid 被复用 → 收尾
                }
            }
            _ => {} // pid 未知：不臆测，留给 SessionEnd / 同进程新会话驱逐处理
        }
    }
    alive.sort_unstable();
    (alive, reaped)
}

/// 无 pid 会话的「空闲废弃」阈值：超过这么久没有任何事件即兜底收尾（终端被直接关掉、
/// SessionEnd 丢失的孤儿会话）。取 30 分钟——足够保守，活跃会话每个事件都会刷新计时，绝不会触及。
const ORPHAN_IDLE_MS: i64 = 30 * 60 * 1000;

/// 是否应为「当前错误指纹」弹通知：仅当当前有错误且指纹与上次通知过的不同。
/// 同一错误不反复弹；错误消失（cur=None）不弹（清除条目交给调用方）。纯函数，便于单测。
fn should_notify(prev: Option<&str>, cur: Option<&str>) -> bool {
    match cur {
        None => false,
        Some(c) => prev != Some(c),
    }
}

/// 待交互通知指纹：errored 时不发（None，错误优先）；status==waiting 且未出错时用
/// last_event_at 作指纹（每个新的等待回合是新指纹）；其它状态返回 None。纯函数，便于单测。
fn waiting_fingerprint(errored: bool, status: &str, last_event_at: i64) -> Option<String> {
    if errored || status != "waiting" {
        None
    } else {
        Some(last_event_at.to_string())
    }
}

/// 弹一条「点击即聚焦该会话终端」的桌面通知。构建+show 放主线程：winrt toast 的 show() 需在
/// COM STA 上调用，Tauri 主线程即 STA；on_activated 回调由 OS 经 COM 激活机制投递（与消息泵无关），
/// show() 后 Rust 端 Toast 可安全释放（OS 持有通知引用）。回调里调 focus_session_terminal
/// （它自己 spawn 干净线程做 UIA，不阻塞主线程）。app 仅 Windows。
#[cfg(target_os = "windows")]
fn show_session_notification(
    app: &tauri::AppHandle,
    title: String,
    body: String,
    pid: i64,
    focus_title: String,
) {
    use tauri_winrt_notification::Toast;
    // 安装版用 bundle identifier（解析到开始菜单快捷方式 → 显示 cc-kanban+图标 + 点击可激活）；
    // dev 下 AUMID 未注册，退回 PowerShell 的 AUMID 仅保证 toast 能弹出；此时 on_activated 回调
    // 根本不会触发（OS 把激活事件投递给 PowerShell 进程而非本进程），点击跳转只在安装版生效。
    let app_id = if tauri::is_dev() {
        Toast::POWERSHELL_APP_ID.to_string()
    } else {
        app.config().identifier.clone()
    };
    let _ = app.run_on_main_thread(move || {
        let _ = Toast::new(&app_id)
            .title(&title)
            .text1(&body)
            .on_activated(move |_| {
                focus_session_terminal(pid, Some(focus_title.clone()));
                Ok(())
            })
            .show();
    });
}

#[cfg(target_os = "macos")]
fn show_session_notification(
    _app: &tauri::AppHandle,
    title: String,
    body: String,
    pid: i64,
    _focus_title: String, // macOS 按 pid->tty 定位终端，标题用不上
) {
    crate::macos::notify::post(crate::macos::notify::NotifyJob { title, body, pid });
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn show_session_notification(
    _app: &tauri::AppHandle,
    _title: String,
    _body: String,
    _pid: i64,
    _focus_title: String,
) {
}

/// 周期轮询：收尾进程已死的卡住会话；存活集合变化或有收尾时发 board-changed 让前端刷新。
/// 同时对「连接中」会话做去重桌面通知：出错（优先）或进入待交互时各弹一次。
/// 总开关（settings.notifications_enabled）只门控是否 .show()，去重 map 始终更新，
/// 故中途打开开关不会把积压的旧错误/待交互一次性炸出来。启动首扫只播种不弹。
fn spawn_liveness_watch(
    app: tauri::AppHandle,
    db_path: PathBuf,
    tx_cache: Arc<Mutex<cc_store::TranscriptCache>>,
) {
    use std::collections::HashMap;
    std::thread::spawn(move || {
        let mut last: Vec<i64> = Vec::new();
        let mut notified: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次错误指纹
        let mut notified_waiting: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次待交互指纹
        let mut seeded = false;
        // 菜单栏图标只在 (运行,待办) 变化时重画，避免每轮无谓刷新。
        #[cfg(target_os = "macos")]
        let mut last_tray: Option<(usize, usize)> = None;
        loop {
            if let Ok(store) = Store::open(&db_path) {
                let sys = System::new_with_specifics(
                    RefreshKind::new().with_processes(ProcessRefreshKind::new()),
                );
                let orphaned = store.end_orphaned_idle(ORPHAN_IDLE_MS, now_ms()).unwrap_or(0);
                let (alive, reaped) = reap_and_alive_ids(&store, &sys, now_ms());
                if alive != last || reaped > 0 || orphaned > 0 {
                    let _ = app.emit("board-changed", ());
                    last = alive;
                }

                // 通知总开关 + 语言：每轮读一次（文件读极廉价；设置改动 5s 内生效）。
                let settings = load_settings();
                let notify_on = settings.notifications_enabled;
                let lang = ui_lang(&settings);

                // 错误 + 待交互通知：仅扫连接中的会话（活跃，数量少）。同时统计菜单栏状态摘要。
                let mut present: HashMap<String, String> = HashMap::new();
                let (mut tray_running, mut tray_waiting) = (0usize, 0usize);
                for s in store.live_sessions().unwrap_or_default() {
                    if s.session.status == "ended" || !pid_is_claude(&sys, s.pid.unwrap_or(0)) {
                        continue;
                    }
                    let sid = s.session.cc_session_id.clone();
                    present.insert(sid.clone(), String::new()); // 标记本轮已扫描；retain 只清理本轮彻底消失的会话

                    let cc_store::TranscriptInfo { title, error, .. } =
                        cc_store::title::resolve_transcript_path(None, s.cwd.as_deref(), &sid)
                            .and_then(|p| p.to_str().map(str::to_string))
                            .map(|path| {
                                tx_cache.lock().unwrap_or_else(|e| e.into_inner()).analyze(&path)
                            })
                            .unwrap_or_default();
                    // 会话标题：通知正文用，也作点击聚焦时匹配 WT 标签页的标题。transcript 标题优先，否则 DB 标题。
                    let display_title = title
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| s.task_title.clone());
                    let pid = s.pid.unwrap_or(0); // 连接中必为有效 pid

                    // 菜单栏摘要计数：出错或待交互 → 需关注(●)，运行中 → ○；在线空闲不计入。
                    if error.is_some() || s.session.status == "waiting" {
                        tray_waiting += 1;
                    } else if s.session.status == "running" {
                        tray_running += 1;
                    }

                    // 错误通知（优先）。
                    if let Some(e) = &error {
                        let prev = notified.get(&sid).map(|s| s.as_str());
                        if seeded && notify_on && should_notify(prev, Some(&e.fingerprint)) {
                            show_session_notification(
                                &app,
                                tr(lang, "notify.error").into(),
                                format!("{} · {}", s.project_name, e.label),
                                pid,
                                display_title.clone(),
                            );
                        }
                        notified.insert(sid.clone(), e.fingerprint.clone());
                    } else {
                        notified.remove(&sid); // 错误消失：下次再错会重新通知
                    }

                    // 待交互通知（errored 时 waiting_fingerprint 返回 None，自动让位给错误）。
                    match waiting_fingerprint(error.is_some(), &s.session.status, s.session.last_event_at) {
                        Some(fp) => {
                            let prev = notified_waiting.get(&sid).map(|s| s.as_str());
                            if seeded && notify_on && should_notify(prev, Some(&fp)) {
                                show_session_notification(
                                    &app,
                                    tr(lang, "notify.waiting").into(),
                                    format!("{} · {}", s.project_name, display_title),
                                    pid,
                                    display_title.clone(),
                                );
                            }
                            notified_waiting.insert(sid.clone(), fp);
                        }
                        None => {
                            notified_waiting.remove(&sid);
                        }
                    }
                }
                // 清掉本轮彻底消失（已结束/超出 100 条上限）的残留条目，防止 map 无限增长。
                // 边缘情况：会话彻底消失后又带着完全相同的未解决错误/待交互重新出现，会再弹一次——可接受。
                notified.retain(|k, _| present.contains_key(k));
                notified_waiting.retain(|k, _| present.contains_key(k));
                seeded = true;

                // macOS：把连接中会话的状态摘要写到菜单栏图标标题旁（一眼可见，弥补无吸边缩略条）。
                #[cfg(target_os = "macos")]
                if last_tray != Some((tray_running, tray_waiting)) {
                    crate::macos::menubar::update_tray_status(&app, tray_running, tray_waiting);
                    last_tray = Some((tray_running, tray_waiting));
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (tray_running, tray_waiting);
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

#[tauri::command]
fn get_account() -> account::AccountPayload {
    account::get_account_payload()
}

#[tauri::command]
async fn refresh_usage() -> Result<account::Usage, String> {
    // 阻塞 HTTP 放到 blocking 线程，避免占用异步运行时。
    tauri::async_runtime::spawn_blocking(account::refresh_usage_payload)
        .await
        .map_err(|e| e.to_string())?
}

/// 返回宿主操作系统标识，供前端按平台调整 UI / 交互。
#[tauri::command]
fn host_os() -> String {
    #[cfg(target_os = "macos")]
    {
        "macos".into()
    }
    #[cfg(target_os = "windows")]
    {
        "windows".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "other".into()
    }
}

/// 「打开未连接会话」可选且本机确实可用的终端 key（供设置页过滤下拉项）。
/// macOS：terminal 必有，iterm 视安装情况；Windows：powershell/cmd 必有，wt 视是否在 PATH。
/// async：丢到线程池跑。同步命令内联在主线程，探测一旦变慢（如 macOS 的 mdfind）
/// 会冻结整个事件循环；设置页每次打开都调它，绝不能赌探测耗时。
#[tauri::command]
async fn available_terminals() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        // iterm_installed 可能跑 mdfind（秒级），包 spawn_blocking 以免占住 tokio worker。
        tauri::async_runtime::spawn_blocking(|| {
            let mut v = vec!["terminal".to_string()];
            if iterm_installed() {
                v.push("iterm".to_string());
            }
            v
        })
        .await
        .unwrap_or_else(|_| vec!["terminal".to_string()])
    }
    #[cfg(target_os = "windows")]
    {
        let mut v = Vec::new();
        if wt_available() {
            v.push("wt".to_string());
        }
        v.push("powershell".to_string());
        v.push("cmd".to_string());
        v
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::<String>::new()
    }
}

/// 打开（或聚焦）设置窗口。窗口 label 为 "about"（main.tsx 按此 label 路由到设置页）。
/// 托盘左键点击与右键菜单「设置」共用此逻辑。
pub(crate) fn open_settings_window(app: &tauri::AppHandle) {
    // macOS：打开设置窗口前临时切到 Regular 激活策略，否则纯托盘 App 的窗口无法获焦。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("about") {
        let _ = w.set_focus();
    } else {
        let builder = tauri::WebviewWindowBuilder::new(
            app,
            "about",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title(tr(ui_lang(&load_settings()), "window.settings"))
        .inner_size(620.0, 460.0)
        .min_inner_size(620.0, 460.0)
        .resizable(false)
        .decorations(false)
        .center();
        // macOS：无边框窗口不会自动圆角，故设为透明，由前端 .settings 的 border-radius 呈现圆角
        // （系统会按不透明内容自动绘制圆角阴影）。Windows 由 DWM 自动圆角，保持不透明不变。
        #[cfg(target_os = "macos")]
        let builder = builder.transparent(true);
        match builder.build() {
            Ok(_about_window) => {
                // macOS：设置窗口关闭后切回 Accessory，重新隐藏 Dock 图标。
                #[cfg(target_os = "macos")]
                {
                    let app_handle = app.clone();
                    _about_window.on_window_event(move |e| {
                        if matches!(
                            e,
                            tauri::WindowEvent::CloseRequested { .. }
                                | tauri::WindowEvent::Destroyed
                        ) {
                            crate::macos::menubar::settings_window_did_close(&app_handle);
                        }
                    });
                }
            }
            Err(e) => eprintln!("创建设置窗口失败: {e}"),
        }
    }
}

/// 托盘右键菜单（设置 / 退出），按语言构建；切语言时由 rebuild_tray_menu 重建。
#[cfg(not(target_os = "macos"))]
fn build_tray_menu(app: &tauri::AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let settings = MenuItemBuilder::with_id("settings", tr(lang, "tray.settings")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(lang, "tray.quit")).build(app)?;
    MenuBuilder::new(app).items(&[&settings, &quit]).build()
}

/// 切语言后让已存在的系统 UI 跟上：重建托盘菜单、改已开设置窗口的标题。
fn apply_language(app: &tauri::AppHandle, lang: &str) {
    if let Some(tray) = app.tray_by_id("cc-kanban-tray") {
        #[cfg(not(target_os = "macos"))]
        if let Ok(menu) = build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
        #[cfg(target_os = "macos")]
        if let Ok(menu) = crate::macos::menubar::build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    if let Some(w) = app.get_webview_window("about") {
        let _ = w.set_title(tr(lang, "window.settings"));
    }
}

/// 构建系统托盘：左键点击直接打开设置；右键菜单提供设置 / 退出。
/// macOS 走 `macos::menubar::setup_tray`（面板模式），故此实现仅用于非 macOS 平台。
#[cfg(not(target_os = "macos"))]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app.handle(), ui_lang(&load_settings()))?;

    let mut builder = TrayIconBuilder::with_id("cc-kanban-tray");
    // 图标恒由打包提供，但缺失时不该 unwrap panic 把启动打挂——没图标就建无图标托盘。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .tooltip("cc-kanban")
        .menu(&menu)
        // 左键留给「打开设置」，菜单仅在右键弹出。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 仅左键「抬起」时触发，避免按下+抬起各触发一次。
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_settings_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 用 Win32 窗口子类化在「移动生效前」硬约束贴纸位置，彻底拖不出屏幕（零抖动，
/// 优于事后 set_position 拉回）。拦截 WM_WINDOWPOSCHANGING，把目标坐标钳进所有显示器
/// 工作区的并集包围盒。
#[cfg(target_os = "windows")]
mod win_constrain {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_EXITSIZEMOVE, WM_SIZING,
        WM_WINDOWPOSCHANGING,
    };

    const SUBCLASS_ID: usize = 0x00CC_4A0B;

    /// 用户拖边框缩放时通知前端用的 AppHandle（启动时注入）。
    static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
    /// 本次缩放手势是否已通知过（一次拖拽只发一次 user-resized）。
    static RESIZE_EMITTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// 注入 AppHandle（在装子类时一并调用）。
    pub fn set_app(app: tauri::AppHandle) {
        let _ = APP.set(app);
    }

    /// 累积所有显示器工作区(rcWork)的并集包围盒。
    struct Bbox {
        has: bool,
        l: i32,
        t: i32,
        r: i32,
        b: i32,
    }

    unsafe extern "system" fn enum_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _rc: *mut RECT,
        data: LPARAM,
    ) -> i32 {
        let bb = &mut *(data as *mut Bbox);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut mi) != 0 {
            let w = mi.rcWork;
            if !bb.has {
                (bb.l, bb.t, bb.r, bb.b, bb.has) = (w.left, w.top, w.right, w.bottom, true);
            } else {
                bb.l = bb.l.min(w.left);
                bb.t = bb.t.min(w.top);
                bb.r = bb.r.max(w.right);
                bb.b = bb.b.max(w.bottom);
            }
        }
        1 // TRUE：继续枚举
    }

    fn virtual_work_bbox() -> Option<(i32, i32, i32, i32)> {
        let mut bb = Bbox { has: false, l: 0, t: 0, r: 0, b: 0 };
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                Some(enum_proc),
                &mut bb as *mut Bbox as LPARAM,
            );
        }
        bb.has.then_some((bb.l, bb.t, bb.r, bb.b))
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _ref: usize,
    ) -> LRESULT {
        if msg == WM_WINDOWPOSCHANGING {
            let wp = &mut *(lparam as *mut WINDOWPOS);
            // 仅在真正移动时约束（SWP_NOMOVE 表示这次不改位置）。
            if (wp.flags & SWP_NOMOVE) == 0 {
                // 取窗口尺寸：SWP_NOSIZE（纯移动，拖拽就是这种）下 wp.cx/cy 无效，
                // 必须用 GetWindowRect 取真实尺寸，否则右/下边界算错、能拖出屏幕。
                let (w, h) = if (wp.flags & SWP_NOSIZE) != 0 {
                    let mut rc: RECT = std::mem::zeroed();
                    if GetWindowRect(hwnd, &mut rc) != 0 {
                        (rc.right - rc.left, rc.bottom - rc.top)
                    } else {
                        (0, 0)
                    }
                } else {
                    (wp.cx, wp.cy)
                };
                if w > 0 && h > 0 {
                    if let Some((l, t, r, b)) = virtual_work_bbox() {
                        // 钳进包围盒；窗口比包围盒还大时左上对齐。
                        let max_x = (r - w).max(l);
                        let max_y = (b - h).max(t);
                        wp.x = wp.x.clamp(l, max_x);
                        wp.y = wp.y.clamp(t, max_y);
                    }
                }
            }
        } else if msg == WM_SIZING {
            // WM_SIZING 仅在用户拖边框缩放时发（程序 set_size 不发）→ 通知前端解除吸附。
            // 一次拖拽手势只发一次，避免刷屏。
            use std::sync::atomic::Ordering;
            if !RESIZE_EMITTED.swap(true, Ordering::Relaxed) {
                if let Some(app) = APP.get() {
                    use tauri::Emitter;
                    let _ = app.emit("user-resized", ());
                }
            }
        } else if msg == WM_EXITSIZEMOVE {
            // 缩放/移动手势结束：若本次确实缩放过（发过 user-resized），通知前端"缩放结束"，
            // 供其按缩放前的吸附状态重新吸回。复位标志，下次拖拽可再次通知。
            use std::sync::atomic::Ordering;
            if RESIZE_EMITTED.swap(false, Ordering::Relaxed) {
                if let Some(app) = APP.get() {
                    use tauri::Emitter;
                    let _ = app.emit("user-resize-end", ());
                }
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// 给窗口装上位置约束子类（重复调用安全：同 id 覆盖）。`hwnd` 取自 tauri 的 window.hwnd()。
    pub fn install(hwnd: isize) {
        unsafe {
            SetWindowSubclass(hwnd as HWND, Some(subclass_proc), SUBCLASS_ID, 0);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let path = db_path();
    let tx_cache: Arc<Mutex<cc_store::TranscriptCache>> =
        Arc::new(Mutex::new(cc_store::TranscriptCache::new()));
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_positioner::init())
        .manage(AppState {
            db_path: path.clone(),
            tx_cache: tx_cache.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_project_tasks,
            get_live_sessions,
            focus_session,
            resume_session,
            rename_session,
            set_archived,
            get_autostart,
            set_autostart,
            get_settings,
            set_settings,
            open_url,
            snap_collapse,
            snap_expand,
            snap_restore,
            unsnap,
            cursor_over_window,
            get_account,
            refresh_usage,
            host_os,
            available_terminals
        ])
        .on_window_event(|window, event| {
            // macOS：面板模式，无出屏约束/吸边；不处理 Moved（避免与 positioner 抢位置、误发 snap-changed）。
            #[cfg(target_os = "macos")]
            let _ = (window, event);
            #[cfg(not(target_os = "macos"))]
            if let tauri::WindowEvent::Moved(pos) = event {
                // 出屏约束与吸附只作用于贴纸主窗口；设置等其它窗口不受限制。
                if window.label() != "main" {
                    return;
                }
                let Ok(size) = window.outer_size() else { return };
                let win = Rect { x: pos.x, y: pos.y, w: size.width as i32, h: size.height as i32 };

                // 限制贴纸不被拖出屏幕：把窗口钳进「所有显示器工作区的并集包围盒」。
                // 越界就立刻拉回，拖到边缘即停（吸边仍在界内，不受影响）。多显示器下可在并集内自由移动。
                let vwork = window.available_monitors().ok().and_then(|ms| {
                    let mut it = ms.iter().map(|m| {
                        let wa = m.work_area();
                        (
                            wa.position.x,
                            wa.position.y,
                            wa.position.x + wa.size.width as i32,
                            wa.position.y + wa.size.height as i32,
                        )
                    });
                    let (mut ax, mut ay, mut bx, mut by) = it.next()?;
                    for (x0, y0, x1, y1) in it {
                        ax = ax.min(x0);
                        ay = ay.min(y0);
                        bx = bx.max(x1);
                        by = by.max(y1);
                    }
                    Some(Rect { x: ax, y: ay, w: bx - ax, h: by - ay })
                });
                if let Some(vwork) = vwork {
                    let (cx, cy) = clamp_xy_to_work(win, vwork);
                    if (cx, cy) != (win.x, win.y) {
                        let _ = window.set_position(tauri::PhysicalPosition::new(cx, cy));
                        return; // 钳正后会再触发一次 Moved（已在界内），那次再算吸附边
                    }
                }

                // 贴边检测（用当前显示器工作区）。
                if let Ok(Some(m)) = window.current_monitor() {
                    let wa = m.work_area();
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
            // macOS：纯菜单栏 App（隐藏 Dock 图标），main 窗口转 NSPanel，托盘走 menubar 模块。
            #[cfg(target_os = "macos")]
            {
                app.handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory)?;
                // nspanel 插件必须先注册（它 manage(WebviewPanelManager)），to_panel()/get_webview_panel()
                // 才能取到该托管状态；漏注册会在启动时 panic：state() called before manage()。
                // nspanel 是 macOS-only crate，无法放进跨平台 Builder 链，故在此运行时注册。
                app.handle().plugin(tauri_nspanel::init())?;
                crate::macos::panel::convert_main_to_panel(app.handle());
                crate::macos::panel::setup_resign_listener(app.handle());
                crate::macos::menubar::setup_tray(app.handle())?;
                crate::macos::notify::init(app.handle());
            }
            #[cfg(not(target_os = "macos"))]
            {
                setup_tray(app)?;
            }
            // window-state 恢复后，若贴纸落在所有显示器之外（多屏拔插/分辨率变化）则救回，避免「找不到」。
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                pull_on_screen(&w, false);
                // 装上位置约束子类：在移动生效前硬钳坐标，彻底拖不出屏幕。
                if let Ok(h) = w.hwnd() {
                    win_constrain::set_app(app.handle().clone()); // 供子类拖边框缩放时通知前端
                    win_constrain::install(h.0 as isize);
                }
            }
            // 无感适配：幂等把 cc-reporter 接入 Claude Code 设置（hooks + statusLine）。后台跑，失败不影响启动。
            std::thread::spawn(ccsetup::apply);
            spawn_db_watcher(app.handle().clone(), path.clone());
            spawn_liveness_watch(app.handle().clone(), path.clone(), tx_cache.clone());
            spawn_first_import(app.handle().clone(), path.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        center_on, clamp_xy_to_work, edge_for_rect, intersection_area, is_session_id,
        normalize_tab_title, parse_wt_default_profile, path_has_exe, pid_is_claude, should_notify,
        strip_jsonc_comments, tab_match_score, waiting_fingerprint, Edge, Rect, Settings,
    };
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};

    const WORK1: Rect = Rect { x: 0, y: 0, w: 2556, h: 1179 };

    #[test]
    fn path_has_exe_scans_path_dirs_without_spawning() {
        let dir = std::env::temp_dir().join("cc-kanban-test-path-has-exe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wt.exe"), b"stub").unwrap();
        // 单目录命中 / 未命中
        let single = std::env::join_paths([dir.clone()]).unwrap();
        assert!(path_has_exe(&single, "wt.exe"));
        assert!(!path_has_exe(&single, "definitely-absent.exe"));
        // 多目录：前面的目录不存在也不影响后面命中
        let multi =
            std::env::join_paths([std::env::temp_dir().join("cc-kanban-no-such-dir"), dir.clone()])
                .unwrap();
        assert!(path_has_exe(&multi, "wt.exe"));
        // 空 PATH → 找不到
        assert!(!path_has_exe(std::ffi::OsStr::new(""), "wt.exe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pid_is_claude_rejects_non_claude_and_dead() {
        let sys =
            System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
        // 当前测试进程存在但不叫 claude → 不算连接（pid 复用防护）
        assert!(!pid_is_claude(&sys, std::process::id() as i64));
        // 非法 / 已死的 pid
        assert!(!pid_is_claude(&sys, 0));
        assert!(!pid_is_claude(&sys, -1));
        assert!(!pid_is_claude(&sys, 4_000_000_000));
    }

    #[test]
    fn intersection_area_overlap_and_disjoint() {
        let win = Rect { x: 100, y: 100, w: 400, h: 300 };
        assert_eq!(intersection_area(win, WORK1), 400 * 300); // 完全在内
        // 完全在屏外（第二屏被拔掉的旧坐标）
        let off = Rect { x: 3000, y: 200, w: 400, h: 300 };
        assert_eq!(intersection_area(off, WORK1), 0);
        // 部分相交
        let partial = Rect { x: 2400, y: 0, w: 400, h: 300 };
        assert_eq!(intersection_area(partial, WORK1), (2556 - 2400) * 300);
    }

    #[test]
    fn clamp_brings_offscreen_window_fully_in() {
        // 在屏右外 → 钳到右边界内（x = 2556 - 400）
        let off = Rect { x: 3000, y: 200, w: 400, h: 300 };
        assert_eq!(clamp_xy_to_work(off, WORK1), (2556 - 400, 200));
        // 负坐标（屏左上外）→ 钳到原点
        let neg = Rect { x: -50, y: -30, w: 400, h: 300 };
        assert_eq!(clamp_xy_to_work(neg, WORK1), (0, 0));
        // 已在屏内 → 不动
        let inside = Rect { x: 100, y: 100, w: 400, h: 300 };
        assert_eq!(clamp_xy_to_work(inside, WORK1), (100, 100));
    }

    #[test]
    fn clamp_window_larger_than_work_aligns_origin() {
        let big = Rect { x: 500, y: 500, w: 3000, h: 2000 };
        assert_eq!(clamp_xy_to_work(big, WORK1), (0, 0));
    }

    #[test]
    fn strip_jsonc_keeps_strings_and_drops_comments() {
        let src = r#"{
          // 行注释
          "defaultProfile": "{guid}", /* 块注释 */
          "url": "https://example.com/a//b",
          "name": "含 // 的中文 \" 引号"
        }"#;
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc_comments(src)).unwrap();
        assert_eq!(v["defaultProfile"], "{guid}");
        assert_eq!(v["url"], "https://example.com/a//b"); // 字符串里的 // 不能被当注释删掉
        assert_eq!(v["name"], "含 // 的中文 \" 引号"); // 多字节 UTF-8 与转义引号保留
    }

    #[test]
    fn wt_default_profile_resolves_guid_to_name() {
        // GUID 大小写不敏感匹配到 name。
        let v = serde_json::json!({
            "defaultProfile": "{574E775E-4F2A-5B96-AC1E-A2962A402336}",
            "profiles": { "list": [
                {"guid": "{0caa0dad-35be-5f56-a8ff-afceeeaa6101}", "name": "命令提示符"},
                {"guid": "{574e775e-4f2a-5b96-ac1e-a2962a402336}", "name": "PowerShell"}
            ]}
        });
        assert_eq!(parse_wt_default_profile(&v).as_deref(), Some("PowerShell"));
        // defaultProfile 直接是名字。
        let named = serde_json::json!({"defaultProfile": "Ubuntu"});
        assert_eq!(parse_wt_default_profile(&named).as_deref(), Some("Ubuntu"));
        // 老格式 profiles 为数组。
        let legacy = serde_json::json!({
            "defaultProfile": "{abc}", "profiles": [{"guid": "{abc}", "name": "Legacy"}]
        });
        assert_eq!(parse_wt_default_profile(&legacy).as_deref(), Some("Legacy"));
        // 无匹配 / 缺字段 → None。
        assert!(parse_wt_default_profile(&serde_json::json!({"defaultProfile": "{zzz}", "profiles": {"list": []}})).is_none());
        assert!(parse_wt_default_profile(&serde_json::json!({})).is_none());
    }

    #[test]
    fn center_on_centers_clamps_and_preserves_center() {
        // 基本居中：300 长里放 60 → 起点 +120。
        assert_eq!(center_on(100, 300, 60, 0, 1000), 220);
        // 右/下越界 → 夹到工作区末尾内。
        assert_eq!(center_on(950, 300, 60, 0, 1000), 940);
        // 左/上越界（负） → 夹到工作区起点。
        assert_eq!(center_on(-50, 100, 60, 0, 1000), 0);
        // 重测一致：换长度后中心不变（220 中心=250 → 新起点 210，中心仍 250）。
        assert_eq!(center_on(220, 60, 80, 0, 1000) + 80 / 2, 220 + 60 / 2);
    }

    #[test]
    fn session_id_accepts_uuid() {
        assert!(is_session_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(is_session_id("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn session_id_rejects_injection_and_malformed() {
        // 含 shell/wt 元字符 → 拒绝（命令注入防护）。
        assert!(!is_session_id("'; calc; '")); // 注入尝试
        assert!(!is_session_id("abc --resume x; calc"));
        assert!(!is_session_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890 ")); // 尾空格
        assert!(!is_session_id("")); // 空
        assert!(!is_session_id("g1b2c3d4-e5f6-7890-abcd-ef1234567890")); // 'g' 非 hex
        assert!(!is_session_id("a1b2c3d4-e5f6-7890-abcd-ef123456789")); // 长度 35
    }

    #[test]
    fn tab_title_strips_spinner_prefix() {
        // claude 写入的标题：状态符号 + 空格 + 任务标题。前缀符号会随状态变化。
        assert_eq!(normalize_tab_title("⠐ 修复贴纸窗口跳转"), "修复贴纸窗口跳转"); // braille spinner
        assert_eq!(normalize_tab_title("✳ 修复贴纸窗口跳转"), "修复贴纸窗口跳转"); // 空闲 ✳
        assert_eq!(normalize_tab_title("⠙ Allow editing titles"), "Allow editing titles");
        // 无前缀也应原样（仅去首尾空白）。
        assert_eq!(normalize_tab_title("  纯标题  "), "纯标题");
        // 尾部截断省略号应去掉。
        assert_eq!(normalize_tab_title("✳ 修复贴纸窗口…"), "修复贴纸窗口");
    }

    #[test]
    fn tab_match_exact_after_normalize() {
        // 不论前缀是 spinner 还是 ✳，剥离后都应精确命中(=2)，这是修「时好时坏」的关键。
        assert_eq!(tab_match_score("⠐ 修复贴纸窗口跳转", "修复贴纸窗口跳转"), 2);
        assert_eq!(tab_match_score("✳ 修复贴纸窗口跳转", "修复贴纸窗口跳转"), 2);
        assert_eq!(tab_match_score("修复贴纸窗口跳转", "修复贴纸窗口跳转"), 2);
    }

    #[test]
    fn tab_match_contains_is_weaker() {
        // 标签页标题含会话标题但不完全相等（如 claude 追加了后缀）→ 弱匹配。
        assert_eq!(tab_match_score("⠐ 修复贴纸窗口跳转 - done", "修复贴纸窗口跳转"), 1);
        // 长标题被 claude 截断：tab 标题是 want 的前缀 → 双向包含命中(=1)。
        assert_eq!(tab_match_score("✳ 修复贴纸连接中会话窗口…", "修复贴纸连接中会话窗口跳转问题"), 1);
    }

    #[test]
    fn tab_match_no_match() {
        assert_eq!(tab_match_score("npm run build", "修复贴纸窗口跳转"), 0);
    }

    #[test]
    fn tab_match_empty_or_unnamed_never_matches() {
        // 空标题/未命名占位不参与匹配，避免误命中任意标签页。
        assert_eq!(tab_match_score("⠐ 任意标题", ""), 0);
        assert_eq!(tab_match_score("⠐ 任意标题", "  "), 0);
        assert_eq!(tab_match_score("⠐ (未命名会话)", "(未命名会话)"), 0);
    }

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

    #[test]
    fn should_notify_only_on_new_error() {
        assert!(!should_notify(None, None));            // 无错 → 不弹
        assert!(should_notify(None, Some("a")));        // 新错 → 弹
        assert!(!should_notify(Some("a"), Some("a")));  // 同一错误 → 不弹
        assert!(should_notify(Some("a"), Some("b")));   // 换了新错误 → 弹
        assert!(!should_notify(Some("a"), None));       // 错误消失 → 不弹（由清除处理）
    }

    #[test]
    fn waiting_fingerprint_rules() {
        // 连接中、待交互、未出错 → 用 last_event_at 作指纹
        assert_eq!(waiting_fingerprint(false, "waiting", 123), Some("123".to_string()));
        // 出错优先：errored 时不发待交互
        assert_eq!(waiting_fingerprint(true, "waiting", 123), None);
        // 非 waiting 状态不发
        assert_eq!(waiting_fingerprint(false, "running", 123), None);
        assert_eq!(waiting_fingerprint(false, "ended", 123), None);
        // 指纹随 last_event_at 变化（新的等待回合 → 新指纹 → 会再弹一次）
        assert_ne!(
            waiting_fingerprint(false, "waiting", 1),
            waiting_fingerprint(false, "waiting", 2)
        );
    }

    #[test]
    fn settings_defaults_notifications_on() {
        // 空文件 / 老文件缺字段 → 默认开启（向后兼容）
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert!(empty.notifications_enabled);
        let legacy: Settings = serde_json::from_str(r#"{"archive_hide_days":7}"#).unwrap();
        assert!(legacy.notifications_enabled);
        assert_eq!(legacy.archive_hide_days, 7);
        // 显式关闭可被尊重
        let off: Settings = serde_json::from_str(r#"{"notifications_enabled":false}"#).unwrap();
        assert!(!off.notifications_enabled);
        // 整文件缺失/解析失败时用 Default，也应为 ON
        assert!(Settings::default().notifications_enabled);
    }

    #[test]
    fn settings_appearance_defaults_and_back_compat() {
        // 老文件缺外观字段 → 用缺省（dark / 94 / 100），不报错。
        let legacy: Settings = serde_json::from_str(r#"{"archive_hide_days":7}"#).unwrap();
        assert_eq!(legacy.theme, "dark");
        assert_eq!(legacy.opacity, 94);
        assert_eq!(legacy.ui_scale, 100);
        // 显式外观值被尊重。
        let custom: Settings =
            serde_json::from_str(r#"{"theme":"light","opacity":80,"ui_scale":112}"#).unwrap();
        assert_eq!(custom.theme, "light");
        assert_eq!(custom.opacity, 80);
        assert_eq!(custom.ui_scale, 112);
        // Default 与缺省函数一致。
        let d = Settings::default();
        assert_eq!(d.theme, "dark");
        assert_eq!(d.opacity, 94);
        assert_eq!(d.ui_scale, 100);
    }
}
