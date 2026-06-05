use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItem, MenuItemBuilder};
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
    /// 托盘「更新」菜单项句柄，供前端检查到新版后回写文案（在主线程上 set_text）。
    update_item: std::sync::Mutex<Option<MenuItem<tauri::Wry>>>,
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

/// claude 会把任务标题写进 Windows Terminal 标签页，并加一个**会随状态变化**的前缀符号：
/// 运行时是 braille spinner(⠐⠂…)，空闲/待输入时是 ✳(U+2733)，可能还有其它符号。
/// 归一化：剥掉开头所有「非字母数字」字符（覆盖任意状态符号 + 空格；任务标题几乎总以
/// 字母/数字/CJK 开头），并去掉尾部空白与截断省略号(…/...)。纯函数，便于单测。
fn normalize_tab_title(s: &str) -> &str {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end()
        .trim_end_matches(['…', '.'])
        .trim_end()
}

/// 标签页标题 `tab_name` 与会话标题 `want` 的匹配强度：2=精确(归一化后相等)，1=单向包含，0=不匹配。
/// 包含是**双向**的：兼容 claude 对长标题的截断(tab 标题是 want 的前缀)与轻微漂移。
/// `want` 为空或占位("(未命名会话)")时不参与匹配(返回 0)，避免误命中无关标签页。纯函数。
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

#[tauri::command]
fn focus_session(pid: i64, title: Option<String>) -> Result<(), String> {
    if pid <= 0 {
        return Err("无效 pid".into());
    }
    #[cfg(target_os = "windows")]
    {
        // 全部放后台线程并 fire-and-forget（前端本就忽略返回），原因有二：
        // 1) 干净 COM apartment：Tauri 同步命令在主线程执行，主线程已是 STA，
        //    复用会让 `UIAutomation::new()` 因 apartment 冲突失败。
        // 2) 不阻塞主线程：若 join 等待，`force_foreground` 的 AttachThreadInput 会附着到
        //    「被阻塞、不再泵消息」的主线程（贴纸窗口正是当前前台）→ 死锁卡死。
        //    立即返回让主线程继续泵消息，后台线程再 AttachThreadInput 才安全。
        std::thread::spawn(move || {
            // 首选：按标题用 UIA 精确切到对应 WT 标签页（解决单进程多标签/多窗口下按 PID 对应不上）。
            // 此路径不做进程扫描，仅靠标题匹配，快。
            if let Some(t) = title.as_deref() {
                if focus_terminal_tab(pid as u32, t) {
                    return;
                }
            }
            // 兜底：传统 conhost（每窗口独立进程）等场景，才扫进程组按 PID 找顶层窗口置前。
            let targets = console_group_pids(pid as u32);
            if let Some(hwnd) = find_window_for_pids(&targets) {
                force_foreground(hwnd);
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (pid, title);
        return Err("仅支持 Windows".into());
    }
    Ok(())
}

/// 会话 id 是否为合法 UUID 形态（仅十六进制与连字符，长度 36）。
/// 用于命令注入防护：通过校验即保证 id 不含引号/分号/空格等任何 shell/wt 元字符。纯函数。
fn is_session_id(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// 把 `cwd` 收敛成「可安全传给 wt -d」的目录：必须非空、真实存在的目录，且不含会破坏 wt
/// 命令行解析的元字符(`;` `"`)。不满足则返回 None（调用方退化为不带 -d）。
#[cfg(target_os = "windows")]
fn safe_cwd(cwd: Option<&str>) -> Option<String> {
    let d = cwd?.trim();
    if d.is_empty() || d.contains([';', '"']) {
        return None;
    }
    std::path::Path::new(d).is_dir().then(|| d.to_string())
}

/// 恢复一个已断开的会话：在其原工作目录 `cwd` 新开一个 Windows Terminal 标签页，跑
/// `claude --resume <session_id>`。`cwd` 缺失/非法(旧会话)时不带 `-d`，尽力按 id 恢复。
///
/// 安全：`session_id` 严格校验为 UUID 形态，`claude`/`--resume`/id 作为**独立 argv** 传给 wt
/// (不拼接 shell 命令字符串)，从源头杜绝命令注入；`cwd` 经 `safe_cwd` 校验为真实目录且无元字符。
#[tauri::command]
fn resume_session(cwd: Option<String>, session_id: String) -> Result<(), String> {
    if !is_session_id(&session_id) {
        return Err("无效 session_id".into());
    }
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // claude --resume 必须在会话原项目目录下运行才找得到会话。DB 的 cwd 可能为空(旧会话/
        // 压缩漏 SessionStart)，故用 resolve_cwd 从 transcript 兜底解析真实 cwd。
        let resolved_cwd = cc_store::title::resolve_cwd(cwd.as_deref(), &session_id);
        // wt -w 0 nt [-d <cwd>] claude --resume <id>
        //   -w 0 nt：在最近的 WT 窗口新开标签页(无则自动建)；-d：在会话原目录打开。
        //   claude 是 PATH 上的 claude.exe，直接拉起，--resume 与 id 各为独立参数。
        let mut args: Vec<String> = vec!["-w".into(), "0".into(), "nt".into()];
        if let Some(dir) = safe_cwd(resolved_cwd.as_deref()) {
            args.push("-d".into());
            args.push(dir);
        }
        args.push("claude".into());
        args.push("--resume".into());
        args.push(session_id);

        Command::new("wt")
            .args(&args)
            .spawn()
            .map_err(|e| format!("启动 Windows Terminal 失败：{e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cwd;
        return Err("仅支持 Windows".into());
    }
    Ok(())
}

#[tauri::command]
fn set_archived(state: State<AppState>, session_id: i64, archived: bool) -> Result<(), String> {
    let store = open_store(&state.db_path)?;
    store.set_session_archived(session_id, archived).map_err(|e| e.to_string())
}

/// 前端检查更新后回写托盘「更新」菜单项：有新版 → 可点击「更新到 vX」；无 → 「已是最新版本」(禁用)。
/// 菜单变更必须在主线程执行。
#[tauri::command]
fn set_update_menu(app: tauri::AppHandle, state: State<AppState>, version: Option<String>) -> Result<(), String> {
    let item = state.update_item.lock().unwrap().clone();
    if let Some(item) = item {
        app.run_on_main_thread(move || match &version {
            Some(v) => {
                let _ = item.set_text(format!("⬇ 更新到 v{v}"));
            }
            None => {
                // 无更新/检查失败：保留为可点的「检查更新」，便于手动重试。
                let _ = item.set_text("检查更新");
            }
        })
        .map_err(|e| e.to_string())?;
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
    let recenter = MenuItemBuilder::with_id("recenter", "回到屏幕").build(app)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItemBuilder::with_id("autostart", "开机自启")
        .checked(autostart_on)
        .build(app)?;
    let ver = app.package_info().version.to_string();
    let about = MenuItemBuilder::with_id("about", format!("关于 v{ver}")).build(app)?;
    let update = MenuItemBuilder::with_id("update", "检查更新").build(app)?;
    // 存句柄：前端检查到结果后通过 set_update_menu 回写文案/可用性。
    app.state::<AppState>()
        .update_item
        .lock()
        .unwrap()
        .replace(update.clone());
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&toggle, &recenter, &autostart, &about, &update, &quit])
        .build()?;

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
                        // 显示时若贴纸在屏外则救回，否则「显示」对丢失的窗口没用。
                        #[cfg(target_os = "windows")]
                        pull_on_screen(&w, false);
                        let _ = w.set_focus();
                    }
                }
            }
            "recenter" => {
                // 显式找回：强制把贴纸钳进可视区并显示置前，无论它当前在不在屏内。
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    #[cfg(target_os = "windows")]
                    pull_on_screen(&w, true);
                    let _ = w.set_focus();
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
            "about" => {
                if let Some(w) = app.get_webview_window("about") {
                    let _ = w.set_focus();
                } else if let Err(e) = tauri::WebviewWindowBuilder::new(
                    app,
                    "about",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("关于 cc-kanban")
                .inner_size(340.0, 400.0)
                .resizable(false)
                .center()
                .build()
                {
                    eprintln!("创建关于窗口失败: {e}");
                }
            }
            "update" => {
                // 显示主窗并通知它处理（有新版→安装；否则→重新检查）。单一来源在前端。
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                }
                let _ = app.emit("tray-update-clicked", ());
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            db_path: path.clone(),
            update_item: std::sync::Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_project_tasks,
            get_live_sessions,
            focus_session,
            resume_session,
            set_archived,
            set_update_menu,
            snap_collapse,
            snap_expand,
            snap_restore
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Moved(pos) = event {
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
            setup_tray(app)?;
            // window-state 恢复后，若贴纸落在所有显示器之外（多屏拔插/分辨率变化）则救回，避免「找不到」。
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                pull_on_screen(&w, false);
            }
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
    use super::{
        clamp_xy_to_work, edge_for_rect, intersection_area, is_session_id, normalize_tab_title,
        tab_match_score, Edge, Rect,
    };

    const WORK1: Rect = Rect { x: 0, y: 0, w: 2556, h: 1179 };

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
}
