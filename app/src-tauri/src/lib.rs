use cc_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
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
}

/// 贴纸最多展示的会话数。
const LIVE_LIMIT: usize = 20;

#[tauri::command]
fn get_live_sessions(state: State<AppState>) -> Result<Vec<LiveItem>, String> {
    let store = open_store(&state.db_path)?;
    let sessions = store.live_sessions().map_err(|e| e.to_string())?;
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );

    // 先算 connected（廉价，仅查进程表）并据此排序，再只对「将要展示」的会话解析
    // transcript 标题——标题解析要 read_to_string 整个 JSONL（可达数 MB），对最多 100 个
    // 会话全做一遍再截断到 20 是巨大的无谓 I/O（每 ~300ms 一次）。
    let mut ranked: Vec<(LiveSession, bool)> = sessions
        .into_iter()
        .map(|s| {
            // 已结束(ended)的会话一律视为断开；并校验 pid 确属 claude，
            // 防 Windows pid 复用（旧 pid 被 esbuild 等占用）误判为「连接中」。
            let connected =
                s.session.status != "ended" && pid_is_claude(&sys, s.pid.unwrap_or(0));
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
        // 一次读 transcript 同时拿标题与错误（断开/历史会话不触发 hook，DB 可能是旧值）。
        let mut error_label: Option<String> = None;
        let mut error_raw: Option<String> = None;
        if let Some(info) = cc_store::title::resolve_transcript_path(
            None,
            s.cwd.as_deref(),
            &s.session.cc_session_id,
        )
        .and_then(|p| p.to_str().map(cc_store::analyze_transcript))
        {
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

/// 应用设置（持久化到 ~/.cc-kanban/settings.json）。
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct Settings {
    /// 归档条目自动隐藏的天数；0 = 永不隐藏。
    #[serde(default)]
    archive_hide_days: u32,
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
fn set_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    let body = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
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
/// 用 explorer 打开（不经 shell），杜绝被滥用打开任意/恶意目标。
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
    #[cfg(not(target_os = "windows"))]
    let _ = url;
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

/// pid 对应的进程是否确实是 claude。
///
/// Windows 会复用 pid：会话结束后它的旧 pid 可能被别的进程（如 esbuild）占用，
/// 只判断「pid 是否存在」会把已结束的会话误判为仍连接。按进程名含 "claude" 甄别。
fn pid_is_claude(sys: &System, pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    sys.process(Pid::from_u32(pid as u32))
        .map(|p| p.name().to_string_lossy().to_ascii_lowercase().contains("claude"))
        .unwrap_or(false)
}

/// 轮询一次：把「记录了 pid、但该进程已死」的 live 会话收尾为 ended（self-heal），
/// 并返回仍存活的 session id（升序）与本轮收尾的数量。
///
/// 终端被关/被 /clear 打断时 SessionEnd 往往不触发，会话状态会永远卡在 running/waiting；
/// 进程都没了就该收尾。pid 为空的不动（可能是刚启动还没抓到 pid，宁可不臆测）。
fn reap_and_alive_ids(store: &Store, now_ms: i64) -> (Vec<i64>, usize) {
    let sys = System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let mut alive: Vec<i64> = Vec::new();
    let mut reaped = 0usize;
    for (id, pid, _) in store.live_session_liveness().unwrap_or_default() {
        match pid {
            Some(p) if p > 0 => {
                if pid_is_claude(&sys, p) {
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

/// 周期轮询：收尾进程已死的卡住会话；存活集合变化或有收尾时发 board-changed 让前端刷新。
/// 同时对「连接中且出错」的会话做去重桌面通知（同一错误只弹一次，启动首扫只播种不弹）。
fn spawn_liveness_watch(app: tauri::AppHandle, db_path: PathBuf) {
    use std::collections::HashMap;
    use tauri_plugin_notification::NotificationExt;
    std::thread::spawn(move || {
        let mut last: Vec<i64> = Vec::new();
        let mut notified: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次通知指纹
        let mut seeded = false;
        loop {
            if let Ok(store) = Store::open(&db_path) {
                let orphaned = store.end_orphaned_idle(ORPHAN_IDLE_MS, now_ms()).unwrap_or(0);
                let (alive, reaped) = reap_and_alive_ids(&store, now_ms());
                if alive != last || reaped > 0 || orphaned > 0 {
                    let _ = app.emit("board-changed", ());
                    last = alive;
                }

                // 错误检测 + 去重通知：仅扫连接中的会话（活跃，数量少）。
                let sys = System::new_with_specifics(
                    RefreshKind::new().with_processes(ProcessRefreshKind::new()),
                );
                let mut present: HashMap<String, String> = HashMap::new();
                for s in store.live_sessions().unwrap_or_default() {
                    if s.session.status == "ended" || !pid_is_claude(&sys, s.pid.unwrap_or(0)) {
                        continue;
                    }
                    let sid = s.session.cc_session_id.clone();
                    let err = cc_store::title::resolve_transcript_path(
                        None, s.cwd.as_deref(), &sid,
                    )
                    .and_then(|p| p.to_str().map(cc_store::analyze_transcript))
                    .and_then(|info| info.error);

                    match err {
                        Some(e) => {
                            present.insert(sid.clone(), e.fingerprint.clone());
                            let prev = notified.get(&sid).map(|s| s.as_str());
                            if seeded && should_notify(prev, Some(&e.fingerprint)) {
                                let _ = app
                                    .notification()
                                    .builder()
                                    .title("会话出错")
                                    .body(format!("{} · {}", s.project_name, e.label))
                                    .show();
                            }
                            notified.insert(sid, e.fingerprint);
                        }
                        None => {
                            notified.remove(&sid); // 错误消失：下次再错会重新通知
                        }
                    }
                }
                // 清掉已不在连接中集合里的残留条目，防止 map 无限增长。
                notified.retain(|k, _| present.contains_key(k));
                seeded = true;
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
    // 托盘只保留两项：设置（打开设置窗口）、退出。其余（开机自启等）搬进设置窗口。
    let settings = MenuItemBuilder::with_id("settings", "设置").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&settings, &quit]).build()?;

    TrayIconBuilder::with_id("cc-kanban-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("cc-kanban")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "settings" => {
                // 窗口 label 仍叫 "about"（main.tsx 按此 label 路由到设置页），仅标题/入口改名为「设置」。
                if let Some(w) = app.get_webview_window("about") {
                    let _ = w.set_focus();
                } else if let Err(e) = tauri::WebviewWindowBuilder::new(
                    app,
                    "about",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("设置")
                .inner_size(620.0, 460.0)
                .min_inner_size(620.0, 460.0)
                .resizable(false)
                .decorations(false)
                .center()
                .build()
                {
                    eprintln!("创建设置窗口失败: {e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
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
        GetWindowRect, SWP_NOMOVE, SWP_NOSIZE, WINDOWPOS, WM_WINDOWPOSCHANGING,
    };

    const SUBCLASS_ID: usize = 0x00CC_4A0B;

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
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            db_path: path.clone(),
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
            snap_restore
        ])
        .on_window_event(|window, event| {
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
            setup_tray(app)?;
            // window-state 恢复后，若贴纸落在所有显示器之外（多屏拔插/分辨率变化）则救回，避免「找不到」。
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                pull_on_screen(&w, false);
                // 装上位置约束子类：在移动生效前硬钳坐标，彻底拖不出屏幕。
                if let Ok(h) = w.hwnd() {
                    win_constrain::install(h.0 as isize);
                }
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
        pid_is_claude, should_notify, tab_match_score, Edge, Rect,
    };
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};

    const WORK1: Rect = Rect { x: 0, y: 0, w: 2556, h: 1179 };

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
}
