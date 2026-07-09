mod account;
mod fsutil;
#[cfg(target_os = "macos")]
mod macos;
mod settings;
pub mod snap;
mod term_script;
#[cfg(target_os = "windows")]
mod wezterm;

use settings::{
    get_autostart, get_settings, load_settings, open_url, set_autostart, set_settings, tr, ui_lang,
};
use snap::{
    cursor_over_window, pointer_left_down, snap_collapse, snap_expand, snap_restore, unsnap,
};
// 出屏约束/吸边检测（run 的窗口事件闭包）只在非 macOS 用这些几何符号。
#[cfg(not(target_os = "macos"))]
use snap::{clamp_xy_to_work, edge_for_rect, Rect, SnapPayload, SNAP_THRESHOLD};
#[cfg(target_os = "windows")]
use snap::pull_on_screen;

use meowo_store::{LiveSession, ProjectOverview, Store, TaskCard};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
// HashSet 仅被 Windows 专属的终端窗口枚举使用（console_group_pids / find_window_for_pids）。
#[cfg(target_os = "windows")]
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};

pub mod setup;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
#[cfg(target_os = "windows")]
use sysinfo::Pid;
#[cfg(not(target_os = "macos"))]
use tauri::menu::{MenuBuilder, MenuItemBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};

/// 托管状态只持有库路径。每个命令按需开短连接——库暂时不可用（被独占锁/损坏/
/// 无权限）时只让该次刷新返回错误，不会在启动时 panic 把整个 app 打挂；
/// 下次 board-changed 事件刷新即自动恢复。
struct AppState {
    db_path: PathBuf,
    /// transcript 增量解析缓存（与后台轮询线程共享 Arc）：避免每次刷新重读整文件。
    tx_cache: Arc<Mutex<meowo_store::TranscriptCache>>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("MEOWO_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".meowo").join("board.db")
}

/// 从旧品牌 cc-kanban 迁移本地数据目录到 ~/.meowo。
/// 仅在 MEOWO_DB 未覆盖、~/.meowo 不存在而 ~/.cc-kanban 存在时执行一次。
pub(crate) fn migrate_legacy_data() {
    if std::env::var("MEOWO_DB").is_ok() {
        return;
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let old_dir = PathBuf::from(&home).join(".cc-kanban");
    let new_dir = PathBuf::from(&home).join(".meowo");
    if !old_dir.exists() || new_dir.exists() {
        return;
    }
    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            let dest = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_all(&path, &dest)?;
            } else {
                std::fs::copy(&path, &dest)?;
            }
        }
        Ok(())
    }
    if let Err(e) = copy_dir_all(&old_dir, &new_dir) {
        eprintln!("Meowo 迁移旧数据目录失败: {e}");
    } else {
        println!("Meowo 已迁移旧数据目录: {} -> {}", old_dir.display(), new_dir.display());
    }
}

fn open_store(path: &PathBuf) -> Result<Store, String> {
    Store::open(path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_overview(state: State<'_, AppState>) -> Result<Vec<ProjectOverview>, String> {
    // 与 get_live_sessions 一致：SQLite I/O 放 blocking 线程池，不占主线程事件循环。
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_store(&db_path)?.overview().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 「新建会话」面板的最近目录（去重+倒序）。
#[tauri::command]
async fn recent_cwds(state: State<'_, AppState>, limit: usize) -> Result<Vec<String>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_store(&db_path)?.recent_cwds(limit).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_project_tasks(
    state: State<'_, AppState>,
    project_id: i64,
) -> Result<Vec<TaskCard>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_store(&db_path)?
            .project_tasks(project_id)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
struct LiveItem {
    #[serde(flatten)]
    inner: LiveSession,
    connected: bool,
    errored: bool,
    error_label: Option<String>,
    error_raw: Option<String>,
    // 最近一条 AI 正文的轻推预览（清洗+截断），卡片 hover 速览用。
    preview: Option<String>,
    // 注：context_pct / context_window 来自 inner(LiveSession)，由 statusline 写库、flatten 输出。
}

#[tauri::command]
async fn get_live_sessions_counts(
    state: State<'_, AppState>,
) -> Result<meowo_store::query::LiveSessionCounts, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        open_store(&db_path)?.live_sessions_counts().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_live_sessions_page(
    state: State<'_, AppState>,
    filter: String,
    search: Option<String>,
    before_last_event_at: Option<i64>,
    before_id: Option<i64>,
    limit: usize,
) -> Result<Vec<LiveItem>, String> {
    // 重逻辑（SQLite、进程枚举、transcript 解析）放 blocking 线程池，不占主线程事件循环。
    let db_path = state.db_path.clone();
    let tx_cache = state.tx_cache.clone();
    let filter = if ["all", "running", "waiting", "archived"].contains(&filter.as_str()) {
        filter
    } else {
        "all".into()
    };
    tauri::async_runtime::spawn_blocking(move || {
        live_sessions_blocking(&db_path, &tx_cache, &filter, search.as_deref(), before_last_event_at, before_id, limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 看板 resume 后「乐观连接」的宽限期(ms)。pid 未知(resume 清空待认领)的会话仅在此窗口内显示已连接，
/// 给 codex 这类「resume 不发 hook、要到首条消息才触发」的 agent 留出「启动 + 用户发首条消息」的窗口；
/// 超时仍无 hook 认领真 pid 则回落未连接——避免没真正起来的会话(终端没开/被秒关)长期假连接。
/// 同一阈值也驱动 spawn_liveness_watch 里的 end_orphaned_idle 兜底收尾（见该处调用）：
/// 若只改「已连接」显示而不收尾 DB 的 status，会话会长期停在错误的运行中/待交互 tab 里——
/// 卡片已显示断开、tab 分类却对不上（如 kimi 卸载后 resume 失败：终端进程本身起来了，
/// spawn_in_terminal 判定为成功、不触发失败回滚，但 kimi 从未真正启动、永远不会有 hook 认领 pid）。
const RESUME_GRACE_MS: i64 = 120_000;

/// 卡片「已连接」判定（纯函数，便于单测）：
/// - 已结束 → 断开。
/// - pid 是存活 agent 进程 → 连接（严格校验防 Windows pid 复用，旧 pid 被 esbuild 等占用误判）。
/// - pid 未知 → 仅 resume 宽限期内（last_event_at 距 now 在 RESUME_GRACE_MS 内）乐观连接，否则断开。
///   pid 未知只可能是看板 resume 清空待认领（hook 一旦认领 pid 即走上一分支），故宽限即「刚 resume」。
fn session_connected(status: &str, pid: Option<i64>, pid_alive: bool, last_event_at: i64, now: i64) -> bool {
    if status == "ended" {
        return false;
    }
    if pid_alive {
        return true;
    }
    pid.is_none() && now.saturating_sub(last_event_at) < RESUME_GRACE_MS
}

fn live_sessions_blocking(
    db_path: &PathBuf,
    tx_cache: &Mutex<meowo_store::TranscriptCache>,
    filter: &str,
    search: Option<&str>,
    before_last_event_at: Option<i64>,
    before_id: Option<i64>,
    limit: usize,
) -> Result<Vec<LiveItem>, String> {
    let store = open_store(db_path)?;
    let sessions = store
        .live_sessions(Some(filter), search, before_last_event_at, before_id, limit)
        .map_err(|e| e.to_string())?;
    // connected 校验：Windows 走 sysinfo 进程表；macOS/Unix 一次 ps 批量快照
    // （sysinfo 在 macOS 上不可靠，逐 pid spawn ps 又太慢——一批会话只扫一次）。
    #[cfg(target_os = "windows")]
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    #[cfg(target_os = "windows")]
    let is_claude = |pid: i64| pid_is_agent(&sys, pid);
    #[cfg(not(target_os = "windows"))]
    let claude_pids = claude_pids_snapshot();
    #[cfg(not(target_os = "windows"))]
    let is_claude = |pid: i64| pid > 0 && claude_pids.contains(&pid);

    // 先算 connected（廉价，仅查进程表）并据此排序，再解析 transcript 标题。
    // 标题解析要 read_to_string 整个 JSONL（可达数 MB）；走增量缓存后后续刷新接近 0 成本，
    // 首次加载即使会话较多也能接受——前端用虚拟列表消化，不再做 20 条截断。
    let now = now_ms();
    let mut ranked: Vec<(LiveSession, bool)> = sessions
        .into_iter()
        .map(|s| {
            // 已结束=断开；pid 是存活 agent=连接(防 pid 复用)；pid 未知=仅 resume 宽限期内乐观连接。
            let connected = session_connected(
                &s.session.status,
                s.pid,
                is_claude(s.pid.unwrap_or(0)),
                s.session.last_event_at,
                now,
            );
            (s, connected)
        })
        .collect();
    // 连接中优先，其次最近活跃。live_sessions() 已按 last_event_at DESC 返回，
    // 稳定排序按 connected 分组即保留组内的时间序。
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));

    // 逐条解析标题并过滤，不再做 20 条截断。连接中的会话排在最前，
    // 它们（正在活跃、文件确实在变）优先拿到实时标题；断开的会话继续解析并全部返回。
    let mut items: Vec<LiveItem> = Vec::with_capacity(ranked.len());
    for (mut s, connected) in ranked {
        // 一次读 transcript 拿标题与错误（断开/历史会话不触发 hook，DB 可能是旧值）。
        // 走增量缓存：只解析新追加的行，避免每轮重读整文件（大 transcript 可达数百 ms，会拖慢整窗）。
        // 上下文百分比不在这里算——它由 statusline 写库、随 LiveSession flatten 输出。
        let mut error_label: Option<String> = None;
        let mut error_raw: Option<String> = None;
        let mut preview: Option<String> = None;
        // 注：此处仅按 transcript() 是否存在解析；下方对 info.title 的覆盖不再单独 gate
        // resolves_transcript_title。当前只有 claude 有 spec（且 resolves_transcript_title=true），
        // codex/kimi transcript()=None 不进此分支，故零影响。将来若引入「有 spec 但标题走首条
        // prompt」的 provider，需回到这里与 dispatch::apply_title 一致地按 resolves_transcript_title 门控标题。
        // analyze_shared：文件 IO 在缓存锁外进行，大 transcript 首读（数百 ms）不会把
        // liveness 线程/本函数互相阻塞在同一把锁上。
        let info = meowo_reporter::agent::for_provider(meowo_store::ProviderKey::parse(Some(&s.provider)))
            .transcript()
            .and_then(|spec| {
                spec.resolve_transcript_path(None, s.cwd.as_deref(), &s.session.cc_session_id)
                    .and_then(|p| p.to_str().map(str::to_string))
                    .map(|path| meowo_store::TranscriptCache::analyze_shared(tx_cache, spec, &path))
            });
        if let Some(info) = info {
            if let Some(t) = info.title {
                s.task_title = t;
            }
            if let Some(e) = info.error {
                error_label = Some(e.label);
                error_raw = Some(e.raw);
            }
            preview = info.preview;
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
            preview,
        });
    }
    Ok(items)
}

/// Toolhelp 进程快照：pid -> (父 pid, 可执行名小写)。只读元数据、不开任何进程句柄，数百进程通常
/// 1-3ms。取代 sysinfo 全进程刷新——后者在 ProcessInner::new 里对每个进程无条件 OpenProcess+
/// GetProcessTimes（与 ProcessRefreshKind 无关、关字段也省不掉），数百进程下 30-120ms。
#[cfg(target_os = "windows")]
fn snapshot_processes() -> std::collections::HashMap<u32, (u32, String)> {
    use std::collections::HashMap;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut map: HashMap<u32, (u32, String)> = HashMap::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return map;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name =
                    String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                map.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    map
}

/// 收集与 root_pid 同控制台组的进程 pid：root + 所有祖先(上溯到终端宿主为止) + 所有子孙。
/// 基于 Toolhelp 快照在内存里上溯/BFS，不做全进程句柄刷新（见 snapshot_processes）。
#[cfg(target_os = "windows")]
fn console_group_pids(root_pid: u32) -> HashSet<u32> {
    let snapshot = snapshot_processes();
    let mut set: HashSet<u32> = HashSet::new();
    set.insert(root_pid);
    // 祖先：向上到「终端宿主」为止。遇到桌面壳/系统进程(explorer/sihost/...)就停，
    // 否则会把桌面、任务栏的窗口也算进来，点击时误聚焦到桌面。
    let boundary = [
        "explorer.exe", "sihost.exe", "svchost.exe", "services.exe", "wininit.exe",
        "winlogon.exe", "csrss.exe", "runtimebroker.exe", "dwm.exe",
    ];
    let terminal_host = [
        "windowsterminal.exe", "conhost.exe", "openconsole.exe", "wt.exe", "wezterm-gui.exe",
    ];
    let mut cur = root_pid;
    for _ in 0..32 {
        let Some(&(ppid, _)) = snapshot.get(&cur) else { break };
        if ppid == 0 {
            break;
        }
        let pname = snapshot.get(&ppid).map(|(_, n)| n.as_str()).unwrap_or("");
        if boundary.contains(&pname) {
            break; // 到桌面/系统边界，停止上溯且不纳入
        }
        set.insert(ppid);
        if terminal_host.contains(&pname) {
            break; // 已纳入终端宿主，不再继续上溯
        }
        cur = ppid;
    }
    // 子孙：只从 root 自身往下 BFS（不经过祖先），否则会把终端宿主的「其它标签页」全抓进来。
    let mut frontier = vec![root_pid];
    while let Some(x) = frontier.pop() {
        for (&pid, (ppid, _)) in &snapshot {
            if *ppid == x && set.insert(pid) {
                frontier.push(pid);
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

/// 用纯 Win32 EnumWindows+GetClassNameW 收集所有可见的 Windows Terminal 顶层窗口 HWND(as isize)。
/// 替代 UIA matcher 从桌面根逐节点跨进程爬树找窗口——后者默认 depth=7、每访问一个元素一次
/// CurrentClassName RPC，几十~上百窗口累计可达数百 ms；本函数纯进程内，微秒级。
#[cfg(target_os = "windows")]
fn enum_wt_hwnds() -> Vec<isize> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, IsWindowVisible,
    };

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            if cls == "CASCADIA_HOSTING_WINDOW_CLASS" {
                let out = &mut *(lparam as *mut Vec<isize>);
                out.push(hwnd as isize);
            }
        }
        TRUE
    }

    let mut out: Vec<isize> = Vec::new();
    unsafe {
        EnumWindows(Some(cb), &mut out as *mut Vec<isize> as LPARAM);
    }
    out
}

/// claude 会把任务标题写进 Windows Terminal 标签页，并加一个**会随状态变化**的前缀符号：
/// 运行时是 braille spinner(⠐⠂…)，空闲/待输入时是 ✳(U+2733)，可能还有其它符号。
/// 归一化：剥掉开头所有「非字母数字」字符（覆盖任意状态符号 + 空格；任务标题几乎总以
/// 字母/数字/CJK 开头），并去掉尾部空白与截断省略号(…/...)。纯函数，便于单测。
#[allow(dead_code)] // 跨平台纯函数：Windows 上 WT/WezTerm 聚焦共用，非 Windows 仅单测使用
fn normalize_tab_title(s: &str) -> &str {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end()
        .trim_end_matches(['…', '.'])
        .trim_end()
}

/// 标签页标题 `tab_name` 与会话标题 `want` 的匹配强度：2=精确(归一化后相等)，1=单向包含，0=不匹配。
/// 包含是**双向**的：兼容 claude 对长标题的截断(tab 标题是 want 的前缀)与轻微漂移。
/// `want` 为空或占位("(未命名会话)")时不参与匹配(返回 0)，避免误命中无关标签页。纯函数。
#[allow(dead_code)] // 同上：Windows 上 WT/WezTerm 聚焦共用，非 Windows 仅单测调用
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
fn focus_terminal_tab(root_pid: u32, want: &str, token: Option<&str>) -> bool {
    use uiautomation::patterns::UISelectionItemPattern;
    use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    let Ok(automation) = UIAutomation::new() else { return false };

    // WT 顶层窗口：先用纯 Win32 EnumWindows+GetClassNameW 直接拿 HWND（进程内、微秒级），再
    // element_from_handle 只进入这几个窗口做 UIA。绕开 crate matcher 从桌面根逐节点 RPC 爬树
    // （默认 depth=7、每节点一次 CurrentClassName 跨进程调用，几十~上百窗口下可达 50-300ms）。
    // 保留 HWND 与 UIElement 配对：HWND 用于 GetWindowThreadProcessId 取窗口 pid（消歧用）与置前，
    // UIElement 用于 UIA 枚举标签页。
    let wt_windows: Vec<(isize, UIElement)> = enum_wt_hwnds()
        .into_iter()
        .filter_map(|h| automation.element_from_handle(Handle::from(h)).ok().map(|el| (h, el)))
        .collect();
    if wt_windows.is_empty() {
        return false;
    }

    // 标签页条件(TabItem)；其容器条件(TabView=ControlType::Tab)用于把搜索根收窄到标签条子树。
    let Ok(tab_cond) = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(ControlType::TabItem as i32),
        None,
    ) else {
        return false;
    };
    let tabview_cond = automation
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Tab as i32),
            None,
        )
        .ok();
    // 缓存请求：让 FindAll 随元素一次性带回 Name，用 get_cached_name 读取，免每个 TabItem 一次
    // CurrentName 跨进程 RPC。
    let cache_req = automation.create_cache_request().ok();
    if let Some(ref cr) = cache_req {
        let _ = cr.add_property(UIProperty::Name);
    }

    // 取某 WT 窗口的 (TabItem, name) 列表。关键提速：先 find_first 定位 TabView 容器(ControlType::Tab，
    // 命中即停)，把 FindAll 的根从整窗收窄到标签条子树——避免对整窗 Descendants 全扫(含终端内容面板，
    // 实测每窗口 ~20ms)。容器内优先直接子(Children)，拿不到再容器 Descendants(兼容 TabItem 嵌套)；
    // 连容器都没有才退化为整窗 Descendants(异常布局兜底)。name 优先走缓存(get_cached_name)。
    let collect_tabs = |win: &UIElement| -> Vec<(UIElement, String)> {
        let find_tabitems = |root: &UIElement, scope: TreeScope| -> Vec<UIElement> {
            match &cache_req {
                Some(cr) => root.find_all_build_cache(scope, &tab_cond, cr).unwrap_or_default(),
                None => root.find_all(scope, &tab_cond).unwrap_or_default(),
            }
        };
        let mut tabs: Vec<UIElement> = Vec::new();
        if let Some(tv) = tabview_cond
            .as_ref()
            .and_then(|c| win.find_first(TreeScope::Descendants, c).ok())
        {
            tabs = find_tabitems(&tv, TreeScope::Children);
            if tabs.is_empty() {
                tabs = find_tabitems(&tv, TreeScope::Descendants);
            }
        }
        if tabs.is_empty() {
            tabs = find_tabitems(win, TreeScope::Descendants);
        }
        tabs.into_iter()
            .map(|t| {
                let name = if cache_req.is_some() {
                    t.get_cached_name().or_else(|_| t.get_name()).unwrap_or_default()
                } else {
                    t.get_name().unwrap_or_default()
                };
                (t, name)
            })
            .collect()
    };

    // 收集所有命中标签页：(匹配分, 窗口 HWND, 窗口 pid, 标签元素)。【不短路】——同一标题在多个窗口/标签
    // 出现时，按 console_group_pids(root_pid) 消歧到本会话所属窗口，否则会聚焦到错的同名标签。
    // want 来源因 agent 而异：claude=任务标题、kimi=cwd 末段目录名(配 token 精确)、codex=cwd 末段目录名
    // (匹配 codex 自己写的 project-name 标签标题)。单个会话即精确命中；多个同名标签退窗口级。
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut matches: Vec<(u8, isize, u32, UIElement)> = Vec::new();
    for (hwnd, win) in &wt_windows {
        let mut win_pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(*hwnd as HWND, &mut win_pid);
        }
        for (tab, name) in collect_tabs(win) {
            // token(=session_id 末 8 位，meowo-reporter 写进 kimi 标签) 命中即最高优先级 3、全局唯一——
            // 压倒按标题的语义匹配，且无需进程组消歧。否则退回标题匹配(0-2，含 codex 的 project-name)。
            let score = match token {
                Some(t) if !t.is_empty() && name.contains(t) => 3,
                _ => tab_match_score(&name, want),
            };
            if score > 0 {
                matches.push((score, *hwnd, win_pid, tab));
            }
        }
    }
    let max_score = matches.iter().map(|m| m.0).max().unwrap_or(0);
    if max_score == 0 {
        return false;
    }
    // 只保留最高分候选。
    matches.retain(|m| m.0 == max_score);
    // 唯一候选直接用；多个同分时按 console_group_pids(root_pid) 选与本会话同进程组的窗口（窗口宿主
    // WindowsTerminal.exe 是本会话进程的祖先，故其 pid 落在进程组里）——修「两个同名终端点击跳错」。
    // 选出本会话所属窗口(进程组含其窗口 pid)的候选。同一窗口里多个同名标签无法区分（UIA 不暴露
    // tab→进程），此时【不猜】——返回 false 让上层走窗口级定位，避免切到错的同名标签
    // （如 codex/kimi 同在某目录、标签都显示该目录名时，点哪个都别误切到另一个）。
    let idx = if matches.len() == 1 {
        0
    } else {
        let group = console_group_pids(root_pid);
        let in_group: Vec<usize> =
            (0..matches.len()).filter(|&i| group.contains(&matches[i].2)).collect();
        match in_group.as_slice() {
            [i] => *i,         // 唯一属于本会话窗口的候选 → 精确命中
            _ => return false, // 0 个或多个(同窗口多同名标签) → 不猜，退回窗口级
        }
    };
    let (_, hwnd, _, tab) = &matches[idx];
    // 选中该标签页（即使其窗口当前在后台也会切换激活标签页），再置前其窗口（直接用 HWND，免再取 native handle）。
    if let Ok(p) = tab.get_pattern::<UISelectionItemPattern>() {
        let _ = p.select();
    }
    force_foreground(*hwnd as HWND);
    true
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

/// 聚焦某会话的终端。`title_based`=该 agent 是否把任务标题写进 WT 标签（claude 写→按任务标题精确切标签；
/// codex/kimi 不写→改用 cwd 末段目录名匹配它们的目录名/project-name 标签）。无论哪种，最终都能按进程组
/// 找到宿主窗口置前。
/// 放后台线程 fire-and-forget（保证干净 COM apartment + 不阻塞调用方）。供 focus_session 命令与
/// 「点击通知」回调共用。仅 Windows（两个调用点均 cfg-gated，故函数整体也 gate）。
#[cfg(target_os = "windows")]
fn focus_session_terminal(
    pid: i64,
    title: Option<String>,
    cwd: Option<String>,
    token: Option<String>,
    title_based: bool,
) {
    std::thread::spawn(move || {
        // 匹配 WT 标签优先级：token(session_id 末 8 位，仅 kimi：meowo-reporter 写进其标签) > 任务标题(claude)
        // > cwd 末段目录名(codex 匹配其 project-name 标题 / kimi 无 token 时) > 窗口级兜底。
        // token 全局唯一，能区分同窗口同目录的同名标签——这是 kimi 精确聚焦的关键；codex 暂无此手段(见 agent.rs)。
        let want = if title_based {
            title
        } else {
            cwd_tab_hint(cwd.as_deref())
        };
        let want_str = want.as_deref().unwrap_or("");
        let has_token = token.as_deref().is_some_and(|t| !t.is_empty());
        if (!want_str.is_empty() || has_token)
            && focus_terminal_tab(pid as u32, want_str, token.as_deref())
        {
            return;
        }
        // 兜底：按进程组找宿主顶层窗口置前（命中正确窗口，但不保证切到具体标签）。宿主
        // WindowsTerminal.exe/conhost 是会话进程的祖先，其窗口 pid 落在进程组里 → 可靠命中正确窗口。
        let targets = console_group_pids(pid as u32);
        // WezTerm 宿主：自绘 GUI 无 UIA TabItem，上面的 WT 标签定位必然不中；组内探到
        // wezterm-gui 就走 wezterm cli 精确切 pane(内含窗口置前)，不再落通用兜底。
        if wezterm::focus_pane(&targets, want_str, token.as_deref(), cwd.as_deref()) {
            return;
        }
        if let Some(hwnd) = find_window_for_pids(&targets) {
            force_foreground(hwnd);
        }
    });
}

/// 从 cwd 取末段目录名，作为「不写标签标题」的 agent(codex/kimi) 的 WT 标签匹配线索——这类会话的
/// 标签默认显示当前目录名。空/根目录返回 None（退回窗口级定位）。
#[cfg(target_os = "windows")]
fn cwd_tab_hint(cwd: Option<&str>) -> Option<String> {
    let c = cwd?.trim_end_matches(['/', '\\']);
    std::path::Path::new(c)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
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
    provider: Option<String>,
) -> Result<(), String> {
    if pid <= 0 {
        return Err("无效 pid".into());
    }
    // session_id 经 is_safe_id 校验（仅 `[A-Za-z0-9_-]`，杜绝注入：macOS 分支会把 id 注入 AppleScript）。
    // 必须用宽松校验——kimi 的 `session_<uuid>` 不合 UUID 形态，用严格 is_session_id 会把连接态的
    // kimi 卡挡在定位之前（Windows 上 session_id 实际并不参与 focus，仅 pid+title）。
    if let Some(id) = session_id.as_deref() {
        if !is_safe_id(id) {
            return Err("无效 session_id".into());
        }
    }
    #[cfg(target_os = "windows")]
    {
        // 该 provider 是否把任务标题写进 WT 标签：决定按标题切标签还是按 cwd 目录名切标签。缺省 claude。
        let title_based = meowo_reporter::agent::for_provider(meowo_store::ProviderKey::parse(provider.as_deref()))
            .sets_terminal_tab_title();
        // token = session_id 末 8 位(全局唯一)，用于精确切到带该 token 的标签(meowo-reporter 写的 kimi 标签
        // / codex 原生 session_id 标题)，可区分同窗口同目录的同名标签。
        let token = session_id
            .as_deref()
            .map(meowo_reporter::tabtitle::short_sid)
            .filter(|s| !s.is_empty());
        focus_session_terminal(pid, title, cwd, token, title_based);
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = provider;
    #[cfg(target_os = "macos")]
    {
        let _ = title;
        let provider_key = meowo_store::ProviderKey::parse(provider.as_deref());
        // ps/osascript（含首次 TCC 授权弹窗）可能长时间阻塞，放后台线程 fire-and-forget，
        // 与 Windows 的 focus_session_terminal 模式对齐，不挡主线程事件循环。
        std::thread::spawn(move || {
            // resume 回退命令按 provider 分发（不再硬编码 claude）；是否允许回退由
            // focus_session_terminal 校验进程死活后决定（进程存活时绝不 resume，防 fork 重复会话）。
            let resume_argv = session_id
                .as_deref()
                .map(|id| meowo_reporter::agent::for_provider(provider_key).resume_args(id))
                .unwrap_or_default();
            crate::macos::terminal::focus_session_terminal(
                pid,
                cwd.as_deref(),
                &resume_argv,
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

/// 在系统文件管理器中打开会话的项目目录（卡片右键菜单用）。
/// 目录须真实存在——DB 记录的 cwd 可能过期（项目被移动/删除），不存在时明确报错而非静默无事发生。
/// 不经 shell 直接 spawn 文件管理器，目录路径作为独立 argv 传入，无注入面。
#[tauri::command]
fn open_project_dir(cwd: String) -> Result<(), String> {
    let dir = cwd.trim();
    if dir.is_empty() || !std::path::Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    #[cfg(target_os = "windows")]
    {
        // kimi 等 provider 写入的 cwd 可能是正斜杠形式，explorer 对正斜杠路径会打开默认目录而非目标。
        let dir = dir.replace('/', "\\");
        std::process::Command::new("explorer").arg(&dir).spawn().map_err(|e| e.to_string())?;
    }
    // macOS：open 偶发慢（Finder 冷启动），放后台线程；status() 等待回收，避免僵尸进程。
    #[cfg(target_os = "macos")]
    {
        let dir = dir.to_string();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("open").arg(&dir).status();
        });
    }
    Ok(())
}

/// 可安全作为命令参数的会话 id：非空、≤128、仅 `[A-Za-z0-9_-]`（无引号/分号/空格等 shell/wt 元字符，
/// 也无 `/`\`.` 杜绝路径穿越）。兼容 claude 的 UUID 与 kimi 的 `session_<uuid>`。resume 用此宽松校验。纯函数。
fn is_safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
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

/// PowerShell 7（pwsh.exe）是否在 PATH 上。进程内缓存，同 wt_available。
/// 一键安装用它优先于 Windows PowerShell 5.1（见 build_install_command 说明）。
#[cfg(target_os = "windows")]
fn pwsh_available() -> bool {
    use std::sync::OnceLock;
    static PWSH_ON_PATH: OnceLock<bool> = OnceLock::new();
    *PWSH_ON_PATH.get_or_init(|| {
        std::env::var_os("PATH").is_some_and(|p| path_has_exe(&p, "pwsh.exe"))
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

/// 把 resume 命令 argv 拼成交给 `powershell -Command` / `cmd /k` 的单行命令串。
/// kimi/codex 的可执行是 USERPROFILE 下的绝对路径，用户名可含空格 / $ / ' / % 等合法字符：
/// - PowerShell：含空白或 $ ` ' 的参数用**单引号字面量**包裹（内嵌单引号翻倍）——双引号内 $ 与反引号
///   仍会被插值展开（如 C:\Users\a$b 被吞成 C:\Users\a），单引号内一切按字面处理；带引号的命令路径
///   需以调用运算符 `&` 前缀。
/// - cmd：含空白的参数加双引号。cmd 没有字面量引用机制，引号内成对的 %VAR% 仍会展开——属 cmd 本身
///   限制，用户名含 % 的机器请改用 wt/powershell（此处不做 ^ 转义：引号内 ^ 会按字面残留）。
///
/// 纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn shell_join_for_windows(args: &[String], powershell: bool) -> String {
    if powershell {
        let quoted: Vec<String> = args
            .iter()
            .map(|a| {
                if a.chars().any(char::is_whitespace) || a.contains(['$', '`', '\'']) {
                    format!("'{}'", a.replace('\'', "''"))
                } else {
                    a.clone()
                }
            })
            .collect();
        let joined = quoted.join(" ");
        if quoted.first().is_some_and(|f| f.starts_with('\'')) {
            format!("& {joined}")
        } else {
            joined
        }
    } else {
        args.iter()
            .map(|a| {
                if a.chars().any(char::is_whitespace) {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 单 pid 判活（廉价版，resume 前奏专用）：Windows 走 Toolhelp 快照（1-3ms，避免 sysinfo 全进程
/// OpenProcess 刷新的 30-120ms 拖慢「点下即显示已连接」），Unix 走一次 ps。
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn pid_alive_agent_quick(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        snapshot_processes()
            .get(&(pid as u32))
            .map(|(_, name)| meowo_reporter::agent::is_agent_process(name))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        pid_is_agent_ps(pid)
    }
}

/// resume 的跨平台前奏（须在后台线程调用）：乐观复活 → 兜底刷新 → 解析 cwd → 按 provider 取
/// resume 命令 argv。返回 (真的复活了才是 Some(sid)——供 spawn 失败回滚,绝不回滚未被本次复活的
/// 真连接会话、resolved_cwd、resume_argv)。
/// 乐观复活:resume 是看板主动发起的,已知恢复哪个会话——先复活并清旧 pid,卡片即刻显示已连接,
/// 不必等 hook(尤其 codex 的 session_start hook 要到首个 turn 才触发)。旧 pid 死活经
/// pid_alive_agent_quick 校验后以 dead_pid 传入,由 store 层 `pid=?` 守卫原子闭合 TOCTOU
/// (见 revive_for_resume)。emit 兜底刷新,不依赖 db watcher 存活。
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn prepare_resume(
    app: &tauri::AppHandle,
    session_id: &str,
    cwd: Option<&str>,
    provider: &str,
) -> (Option<i64>, Option<String>, Vec<String>) {
    let revived = (|| {
        let store = open_store(&db_path()).ok()?;
        let sid = store.find_session_id_pub(session_id).ok().flatten()?;
        let dead_pid = store
            .session_pid(sid)
            .ok()
            .flatten()
            .filter(|&p| p > 0 && !pid_alive_agent_quick(p));
        match store.revive_for_resume(sid, now_ms(), dead_pid) {
            Ok(true) => Some(sid),
            _ => None,
        }
    })();
    let _ = app.emit("board-changed", ());
    // claude --resume 必须在会话原项目目录下运行才找得到会话。DB 的 cwd 可能为空(旧会话/
    // 压缩漏 SessionStart)，故用 resolve_cwd 从 transcript 兜底解析真实 cwd。
    let resolved = meowo_store::title::resolve_cwd(cwd, session_id);
    // 恢复命令按 provider 取（claude --resume / kimi -r …）；可执行名+参数均来自受信 agent 定义。
    let resume = meowo_reporter::agent::for_provider(meowo_store::ProviderKey::parse(Some(provider)))
        .resume_args(session_id);
    (revived, resolved, resume)
}

/// resume 的终端 spawn 失败时回滚乐观复活（收尾回 ended）：GUI 构建下 stderr 不可见，
/// 至少让卡片立即回落「已断开」，而不是假显示「已连接」直到 120s 宽限过期。
/// 只对 prepare_resume 返回 Some(确实复活过)的会话调用——未被本次复活的真连接会话不得误收尾。
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn rollback_failed_resume(sid: i64) {
    if let Ok(store) = open_store(&db_path()) {
        let _ = store.end_session(sid, now_ms());
    }
}

/// 在 `cwd` 打开一个终端并运行 `argv`，终端类型由 `terminal`（同 settings.resume_terminal 取值）决定。
/// resume（`claude --resume <id>`）与 new（裸 `claude`）共用——唯一区别是传入的 argv。成功返回 true。
/// Windows：powershell/cmd/wezterm/wt，缺失回退链同 resume 旧逻辑；wt 分支独立传 argv 不拼 shell 串。
#[cfg(target_os = "windows")]
fn spawn_in_terminal(argv: &[String], cwd: Option<&str>, terminal: &str) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let dir = safe_cwd(cwd);
    // 选了 wt/默认但没装 wt → 回退 PowerShell；选了 wezterm 但已卸载 → 落回 wt/powershell。
    let eff = match terminal {
        "powershell" => "powershell",
        "cmd" => "cmd",
        "wezterm" if wezterm::available() => "wezterm",
        _ if wt_available() => "wt",
        _ => "powershell",
    };
    let spawned: std::io::Result<()> = match eff {
        "powershell" => {
            let mut c = Command::new("powershell");
            c.args(["-NoExit", "-Command", &shell_join_for_windows(argv, true)]);
            if let Some(d) = &dir {
                c.current_dir(d);
            }
            c.creation_flags(CREATE_NEW_CONSOLE).spawn().map(|_| ())
        }
        "cmd" => {
            // cmd /k 跑完命令后保留窗口；工作目录走 current_dir。
            // 必须 raw_arg：cmd.exe 不按 CommandLineToArgvW 规则解析，经 args() 传入时
            // std 会把命令串整体加引号并把内嵌 " 转义成 \"，cmd 收到畸形命令行、路径解析失败。
            let mut c = Command::new("cmd");
            c.raw_arg("/k").raw_arg(shell_join_for_windows(argv, false));
            if let Some(d) = &dir {
                c.current_dir(d);
            }
            c.creation_flags(CREATE_NEW_CONSOLE).spawn().map(|_| ())
        }
        "wezterm" => wezterm::resume(dir.as_deref(), argv),
        _ => {
            let mut args: Vec<String> = vec!["-w".into(), "0".into(), "nt".into()];
            if let Some(p) = wt_default_profile() {
                args.push("-p".into());
                args.push(p);
            }
            if let Some(d) = &dir {
                args.push("-d".into());
                args.push(d.clone());
            }
            args.extend(argv.iter().cloned());
            Command::new("wt").args(&args).spawn().map(|_| ())
        }
    };
    match spawned {
        Ok(()) => true,
        Err(e) => {
            eprintln!("打开终端 {eff} 失败：{e}");
            false
        }
    }
}

/// macOS 版：按 terminal 选 Terminal.app/iTerm2（iTerm2 未装回退 Terminal），走 AppleScript。成功 true。
#[cfg(target_os = "macos")]
fn spawn_in_terminal(argv: &[String], cwd: Option<&str>, terminal: &str) -> bool {
    use crate::term_script::TermKind;
    let kind = match crate::term_script::resume_kind_from_setting(terminal) {
        TermKind::ITerm2 if iterm_installed() => TermKind::ITerm2,
        TermKind::ITerm2 => TermKind::Terminal,
        other => other,
    };
    crate::macos::terminal::resume_session_mac(cwd, argv, kind)
}

/// 其它平台无终端集成。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn spawn_in_terminal(_argv: &[String], _cwd: Option<&str>, _terminal: &str) -> bool {
    false
}

/// 校验并归一「新建会话」的工作目录：非空、真实存在的目录。返回 trim 后的路径。
fn validate_new_session_cwd(cwd: &str) -> Result<String, String> {
    let d = cwd.trim();
    if d.is_empty() {
        return Err("请选择工作目录".into());
    }
    if !std::path::Path::new(d).is_dir() {
        return Err("目录不存在".into());
    }
    Ok(d.to_string())
}

/// 新建一个全新会话：在 `cwd` 打开终端裸启动指定 provider 的 CLI（无 session_id）。
/// 会话入库仍靠该 CLI 自己的 hook（claude/kimi 秒级，codex 首条消息后）——本命令只负责 spawn。
/// terminal 缺省用 settings.resume_terminal。spawn 放 blocking 线程池并 await，失败回传前端面板。
#[tauri::command]
async fn new_session(
    cwd: String,
    provider: String,
    terminal: Option<String>,
) -> Result<(), String> {
    let dir = validate_new_session_cwd(&cwd)?;
    let key = meowo_store::ProviderKey::parse(Some(&provider));
    let argv = meowo_reporter::agent::for_provider(key).launch_args();
    let term = terminal
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| load_settings().resume_terminal);
    // 冷启动首次 spawn 控制台子进程可达数秒；放 blocking 池不挡事件循环，同时能 await 结果回传。
    let ok = tauri::async_runtime::spawn_blocking(move || spawn_in_terminal(&argv, Some(&dir), &term))
        .await
        .map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
        Err("当前平台不支持从看板新建会话".into())
    } else {
        Err("启动终端失败：请确认所选 agent 已安装并在 PATH 中".into())
    }
}

/// 后台安装结束事件：ok=true 表示进程 0 退出；code 为退出码（无法取得时 None）。
#[derive(Clone, serde::Serialize)]
struct InstallDone {
    provider: String,
    ok: bool,
    code: Option<i32>,
}

/// 把安装脚本写进临时文件（按 provider 命名，允许并行安装互不覆盖），返回其路径。
/// Windows：try/catch 捕获终止错误，打印 `Installation failed: …` 并 exit 1。
#[cfg(target_os = "windows")]
fn write_install_script(provider: &str, script: &str) -> std::io::Result<String> {
    let body = format!(
        "Write-Host 'Installing, please wait...'\r\n\
         try {{ {script} }} catch {{ Write-Host ('Installation failed: ' + $_.ToString()); exit 1 }}\r\n"
    );
    let p = std::env::temp_dir().join(format!("meowo-install-{provider}.ps1"));
    std::fs::write(&p, body)?;
    Ok(p.to_string_lossy().into_owned())
}

/// macOS/Linux：子 shell 跑安装串，失败打印统一行并以原退出码退出。
#[cfg(not(target_os = "windows"))]
fn write_install_script(provider: &str, script: &str) -> std::io::Result<String> {
    let body = format!(
        "echo 'Installing, please wait...'\n\
         ( {script} ) || {{ rc=$?; echo \"Installation failed: exit code $rc\"; exit $rc; }}\n"
    );
    let p = std::env::temp_dir().join(format!("meowo-install-{provider}.sh"));
    std::fs::write(&p, body)?;
    Ok(p.to_string_lossy().into_owned())
}

/// 构造后台安装子进程（不弹窗口）。平台差异只在此：Windows 用 pwsh(优先)/powershell + CREATE_NO_WINDOW，
/// 其它平台用 bash。stdin/stdout/stderr 由调用方统一设。
#[cfg(target_os = "windows")]
fn build_install_command(script_path: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let shell = if pwsh_available() { "pwsh" } else { "powershell" };
    let mut c = std::process::Command::new(shell);
    c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script_path])
        .creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(target_os = "windows"))]
fn build_install_command(script_path: &str) -> std::process::Command {
    let mut c = std::process::Command::new("bash");
    c.arg(script_path);
    c
}

/// 一键安装某 agent：后台跑其官方安装脚本（不弹终端窗口），装完 emit install-done、
/// 前端重查检测转「已装」。安装命令是受信硬编码串（Agent::install_script），非用户输入。
#[tauri::command]
async fn install_agent(app: tauri::AppHandle, provider: String) -> Result<(), String> {
    let key = meowo_store::ProviderKey::parse(Some(&provider));
    let provider = key.as_str().to_string(); // 归一：文件名/emit 全用规范串，消除路径注入面+大小写不一致
    let script = meowo_reporter::agent::for_provider(key)
        .install_script(cfg!(target_os = "windows"))
        .ok_or("该 agent 没有可用的一键安装命令")?;
    let path = write_install_script(&provider, &script).map_err(|e| e.to_string())?;

    // spawn 放 blocking 线程：GUI 进程首次 spawn 子进程可能被杀软扫描拖慢，勿堵事件循环。
    // spawn 成功即返回 Ok；结果走 install-done 事件；spawn 失败回传 Err，前端立即显示错误。
    // 进度不透传（前端只显示本地化「安装中…」），故 stdout/stderr 丢弃、不读管道。
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        use std::process::Stdio;
        let mut child = build_install_command(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("CODEX_NON_INTERACTIVE", "1")
            .spawn()
            .map_err(|e| format!("启动安装失败：{e}"))?;
        // 等退出 + emit done 放独立线程，让 spawn_blocking 尽快归还线程池。
        std::thread::spawn(move || {
            use tauri::Emitter;
            let code = child.wait().ok().and_then(|s| s.code());
            let _ = app.emit(
                "install-done",
                InstallDone { provider, ok: code == Some(0), code },
            );
        });
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// provider 的 meowo-reporter hooks 接入状态（供「新建会话」面板引导）。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum HooksStatus {
    Installed,
    Missing,
    Unknown,
}

/// claude/codex 同款 hooks JSON 里，`SessionStart` 事件下是否存在指向 meowo-reporter 且带
/// `--provider <p>` 的 command。纯函数。启发式：命令含 "meowo-reporter"（basename）+
/// "--provider <p>"——该组合不会误配用户自有 hook。
/// 只看 SessionStart（而非扫描全部事件）：「新会话能否入库」只取决于 SessionStart 是否挂了钩子，
/// 只在别的事件（如 Stop）挂了 meowo-reporter 不能保证新会话会被记录，不应误判成 Installed（审查发现）。
fn hooks_json_has_reporter(v: &serde_json::Value, provider: &str) -> bool {
    let Some(arr) = v
        .get("hooks")
        .and_then(|h| h.get("SessionStart"))
        .and_then(|s| s.as_array())
    else {
        return false;
    };
    let want = format!("--provider {provider}");
    for entry in arr {
        for h in entry.get("hooks").and_then(|x| x.as_array()).into_iter().flatten() {
            if let Some(cmd) = h.get("command").and_then(|x| x.as_str()) {
                if cmd.to_ascii_lowercase().contains("meowo-reporter") && cmd.contains(&want) {
                    return true;
                }
            }
        }
    }
    false
}

/// claude hooks 接入状态：读 claude settings.json 判断 meowo-reporter hooks 是否登记。
/// 文件不存在=Missing；读/解析失败=Unknown（暂时不可读/损坏不误报成未装，与 codex/kimi 对称）；
/// 有 meowo-reporter hook=Installed。
fn claude_hooks_status() -> HooksStatus {
    claude_hooks_status_at(&setup::claude::claude_settings_path())
}

/// 纯路径版，便于用临时文件单测三态（不碰真实 ~/.claude）。
fn claude_hooks_status_at(path: &std::path::Path) -> HooksStatus {
    if !path.exists() {
        return HooksStatus::Missing;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return HooksStatus::Unknown;
    };
    match setup::claude::parse_settings(&text) {
        Some(v) if setup::claude::session_start_has_reporter(&v) => HooksStatus::Installed,
        Some(_) => HooksStatus::Missing,
        None => HooksStatus::Unknown,
    }
}

/// codex hooks 接入状态：读 ~/.codex/hooks.json。文件不存在=Missing；读/解析失败=Unknown（不误报）。
fn codex_hooks_status() -> HooksStatus {
    let Some(home) = meowo_reporter::codex::codex_home() else {
        return HooksStatus::Unknown;
    };
    let path = home.join("hooks.json");
    if !path.exists() {
        return HooksStatus::Missing;
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HooksStatus::Unknown;
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) if hooks_json_has_reporter(&v, "codex") => HooksStatus::Installed,
        Ok(_) => HooksStatus::Missing,
        Err(_) => HooksStatus::Unknown,
    }
}

/// kimi hooks 接入状态：读实况变体的 config.toml（新版 ~/.kimi-code 或旧版 ~/.kimi），
/// 判定交给该变体的配置格式适配器。文件不存在=Missing；读失败=Unknown（损坏不误报成未装）。
fn kimi_hooks_status() -> HooksStatus {
    let Some(inst) = meowo_reporter::kimi::kimi_install() else {
        return HooksStatus::Unknown;
    };
    let path = inst.config_path();
    if !path.exists() {
        return HooksStatus::Missing;
    }
    match std::fs::read_to_string(&path) {
        Ok(text) if inst.hooks.has_reporter(&text, "kimi") => HooksStatus::Installed,
        Ok(_) => HooksStatus::Missing,
        Err(_) => HooksStatus::Unknown,
    }
}

/// 检测某 provider 的 meowo-reporter hooks 是否已接入（新建会话面板据此提示是否会入库）。
#[tauri::command]
fn check_provider_hooks(provider: String) -> HooksStatus {
    match meowo_store::ProviderKey::parse(Some(&provider)) {
        meowo_store::ProviderKey::Claude => claude_hooks_status(),
        meowo_store::ProviderKey::Codex => codex_hooks_status(),
        meowo_store::ProviderKey::Kimi => kimi_hooks_status(),
    }
}

/// 「修复连接」结果：最新接线状态 + 失败原因（None = 成功/已是目标状态）。
/// reason 供前端给出精准提示（如 kimi 未登录 → 「请先登录」）而非泛化文案。
#[derive(Debug, serde::Serialize)]
struct RepairResult {
    status: HooksStatus,
    reason: Option<setup::RepairReason>,
}

/// 手动修复某 provider 的 hooks：立即执行一次 setup::apply_provider，然后返回最新状态与失败原因。
/// 用于「新建会话」面板或设置里的「修复连接」按钮，无需重启 Meowo。
#[tauri::command]
fn repair_provider_hooks(provider: String) -> RepairResult {
    let key = meowo_store::ProviderKey::parse(Some(&provider));
    match key {
        meowo_store::ProviderKey::Claude
        | meowo_store::ProviderKey::Codex
        | meowo_store::ProviderKey::Kimi => {
            eprintln!("Meowo repair[{provider}]: 开始修复接线…");
            let reason = setup::apply_provider(key);
            let status = check_provider_hooks(provider.clone());
            eprintln!("Meowo repair[{provider}]: reason={reason:?} → 状态={status:?}");
            RepairResult { status, reason }
        }
    }
}

/// 恢复一个已断开的会话：在其原工作目录 `cwd` 新开一个终端跑 `claude --resume <session_id>`。
/// 终端按设置 `resume_terminal` 选择——Windows：wt(默认)/wezterm/powershell/cmd；macOS：Terminal/iTerm2。
/// `cwd` 缺失/非法(旧会话)时不带 cwd，尽力按 id 恢复。
///
/// 恢复命令由 `provider` 决定（claude: `claude --resume <id>` / kimi: `kimi -r <id>`，见 agent::resume_args）。
/// 安全：`session_id` 经 is_safe_id 校验（仅 `[A-Za-z0-9_-]`，无空格/元字符）；可执行名与参数来自受信的
/// agent::resume_args（非用户输入）；wt 分支各 argv 独立传入，powershell/cmd 命令串只由这些受信片段拼成，从源头杜绝注入。
#[tauri::command]
fn resume_session(
    app: tauri::AppHandle,
    cwd: Option<String>,
    session_id: String,
    provider: String,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    #[cfg(target_os = "windows")]
    {
        // 冷启动后首次 spawn 控制台子进程可达数秒（新建 conhost + 杀软扫描），resolve_cwd 还要读
        // transcript；同步命令跑在主线程，整段挪后台线程，命令立即返回。
        std::thread::spawn(move || {
            let (revived, resolved_cwd, resume) =
                prepare_resume(&app, &session_id, cwd.as_deref(), &provider);
            let ok = spawn_in_terminal(&resume, resolved_cwd.as_deref(), &load_settings().resume_terminal);
            if !ok {
                // GUI 构建 stderr 不可见：回滚乐观复活，卡片立即回落「已断开」而非假连接 120s。
                if let Some(sid) = revived {
                    rollback_failed_resume(sid);
                }
                let _ = app.emit("board-changed", ());
            }
        });
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // resolve_cwd 读 transcript、osascript 可能等 TCC 授权，整段放后台线程不挡主线程。
        // resume 命令按 provider 分发（与 Windows 同一事实源），不再硬编码 claude——
        // 否则 macOS 上恢复 codex/kimi 会话会执行错误命令。
        std::thread::spawn(move || {
            let (revived, resolved, resume) =
                prepare_resume(&app, &session_id, cwd.as_deref(), &provider);
            let ok = spawn_in_terminal(&resume, resolved.as_deref(), &load_settings().resume_terminal);
            if !ok {
                eprintln!("恢复会话：终端启动失败");
                if let Some(sid) = revived {
                    rollback_failed_resume(sid);
                }
                let _ = app.emit("board-changed", ());
            }
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (app, cwd, provider);
        Err("当前平台不支持".into())
    }
}

/// 在贴纸上重命名会话：把新名字落到该 agent 自己的持久层（claude 写 transcript custom-title、
/// kimi 写 session state.json 的 title+isCustomTitle，见各 agent 的 write_rename），并同步更新 DB
/// 标题让卡片/总览即时一致。agent 侧失败（找不到 transcript/state.json）不阻断 DB 更新——卡片仍
/// 显示新名，仅 agent 自身列表可能不同步。
///
/// 安全：session_id 用 is_safe_id 校验（仅 [A-Za-z0-9_-]，杜绝注入与路径穿越，兼容 kimi 的
/// session_<uuid>）；title 经 trim + 截断。
#[tauri::command]
fn rename_session(
    app: tauri::AppHandle,
    state: State<AppState>,
    cwd: Option<String>,
    session_id: String,
    title: String,
    provider: Option<String>,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    let title: String = title.trim().chars().take(80).collect();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }

    // 落到 agent 自己的持久层（best-effort）。provider 缺省 claude（兼容旧调用方）。
    let provider = meowo_store::ProviderKey::parse(provider.as_deref());
    let _ = meowo_reporter::agent::for_provider(provider).write_rename(&session_id, cwd.as_deref(), &title);

    // 同步 DB 标题：卡片/总览即时显示新名。kimi 的 on_user_prompt 仅在占位标题时命名，不会覆盖；
    // claude 的 apply_title 会从 transcript 重读 custom-title（优先级高于 ai-title）维持一致。
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

/// 写入/清除某会话的便签（按 cc_session_id）。便签是用户私有备忘，存本地 DB；session_id 用 is_safe_id
/// 校验（兼容 kimi 的 session_<uuid>、仍杜绝注入），正文截断到 500 字符（store 内 trim 后空则删除该行）。
#[tauri::command]
fn set_session_note(
    app: tauri::AppHandle,
    state: State<AppState>,
    session_id: String,
    note: String,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    let note: String = note.chars().take(500).collect();
    let store = open_store(&state.db_path)?;
    store
        .set_session_note(&session_id, &note, now_ms())
        .map_err(|e| e.to_string())?;
    let _ = app.emit("board-changed", ());
    Ok(())
}

/// watch 建立失败/监听死亡后的重建间隔。
const WATCH_RETRY: Duration = Duration::from_secs(5);

/// 监听 board.db 所在目录变更，去抖后向前端发 "board-changed"。
/// watch 建立失败（全新安装时 ~/.meowo 由 ccsetup/liveness 等并发线程创建，watcher 可能抢先执行
/// 而目录尚不存在）或监听中途死亡（目录被删、notify 后端出错）都不放弃：先确保目录存在、失败 5s 后
/// 重建——否则首启一次失败会让 DB 变更监听在整个进程生命周期内静默失效，前端无轮询兜底、看板冻结。
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
    std::thread::spawn(move || loop {
        let _ = std::fs::create_dir_all(&watch_dir);
        let (tx, rx) = channel();
        let mut watcher: RecommendedWatcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => {
                std::thread::sleep(WATCH_RETRY);
                continue;
            }
        };
        if watcher.watch(&watch_dir, RecursiveMode::NonRecursive).is_err() {
            std::thread::sleep(WATCH_RETRY);
            continue;
        }
        run_db_watch_loop(&app, &rx, &db_name);
        // 返回即监听已死（通道断开或错误事件）→ 稍后重建 watcher。
        std::thread::sleep(WATCH_RETRY);
    });
}

/// db watcher 的事件循环：trailing debounce（收到相关事件后 drain 到 300ms 静默再 emit——SQLite 提交
/// 是 db/-wal/-shm 多个事件的爆发，前沿触发会丢掉尾部事件），但设 1s 总上限：statusline/hook 以
/// ~300ms 节奏持续写库、多会话事件流相位交错时可能永无静默间隙，无上限会让 board-changed 饥饿、
/// 贴纸冻结在旧数据，恰恰是多会话高活跃期最需要刷新的时候。
/// 返回即表示监听已死（通道断开或收到 notify 错误事件，如目录被删），由调用方重建。
fn run_db_watch_loop(
    app: &tauri::AppHandle,
    rx: &std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
    db_name: &str,
) {
    let is_board = |res: &Result<notify::Event, notify::Error>| -> bool {
        let Ok(ev) = res else { return false };
        ev.paths.iter().any(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.strip_prefix(db_name)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
            })
        })
    };
    let debounce = Duration::from_millis(300);
    let max_wait = Duration::from_millis(1000);
    loop {
        let Ok(first) = rx.recv() else { return }; // watcher 关闭/内部线程死亡 → 重建
        if first.is_err() {
            return; // notify 错误事件（目录被删等）→ 重建
        }
        let mut relevant = is_board(&first);
        let mut broken = false;
        let deadline = std::time::Instant::now() + max_wait;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break; // 事件持续不断也到点先 emit 一次，防饥饿
            }
            match rx.recv_timeout(debounce.min(remaining)) {
                Ok(ev) => {
                    if ev.is_err() {
                        broken = true;
                        break;
                    }
                    relevant = relevant || is_board(&ev);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    broken = true;
                    break;
                }
            }
        }
        if relevant {
            let _ = app.emit("board-changed", ());
        }
        if broken {
            return;
        }
    }
}

/// pid 对应的进程是否确实是 claude。
///
/// Windows 会复用 pid：会话结束后它的旧 pid 可能被别的进程（如 esbuild）占用，
/// 只判断「pid 是否存在」会把已结束的会话误判为仍连接。故按进程名甄别是否仍是 agent 本体——
/// 复用 meowo_reporter::agent::is_agent_process（取 basename **精确**匹配 claude/kimi 白名单，
/// 与 owner_pid 写入侧同一事实源），避免子串误匹配（如名字恰含 kimi 的无关进程）。
fn pid_is_agent(sys: &System, pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        sys.process(Pid::from_u32(pid as u32))
            .map(|p| meowo_reporter::agent::is_agent_process(&p.name().to_string_lossy()))
            .unwrap_or(false)
    }
    // macOS/Unix：sysinfo 对进程的可见性不稳（实测 parent() 会过早返回 None、
    // 最小刷新下 name 是否可靠也无保证），改用 ps 校验，与 meowo-reporter::owner_pid 一致。
    // 仅对「非 ended 的活跃会话」调用，每轮就几个，ps 开销可忽略。
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sys;
        pid_is_agent_ps(pid)
    }
}

/// macOS/Unix：单 pid 的 agent 判活（一次 ps 按 comm 校验）。pid_is_agent 的 Unix 分支与
/// macos::terminal 的 resume 回退守卫共用此单一实现，避免判活口径分叉（进程存活却被判死 →
/// 回退 resume 对运行中会话 fork 出重复会话）。
/// ps 自身 spawn 失败（瞬时故障）时保守地当「存活/未知」——调用方把 false 当「确认已死」：
/// reaper 会误收尾、聚焦回退会对运行中会话 fork 重复 resume、resume 前奏会把活 pid 当死 pid
/// 传给 revive。只有 ps 成功返回且 comm 不是 agent（含 pid 不存在时的空输出）才判死。
#[cfg(not(target_os = "windows"))]
pub(crate) fn pid_is_agent_ps(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    let Ok(out) = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
    else {
        return true; // 查不了 ≠ 已死：宁可暂当存活，等下一轮能查时再判
    };
    meowo_reporter::agent::is_agent_process(String::from_utf8_lossy(&out.stdout).trim())
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
        if meowo_reporter::agent::is_agent_process(&comm) {
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
                if pid_is_agent(sys, p) {
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

/// 是否应为「当前错误指纹」弹通知：仅当当前有错误且指纹与上次通知过的不同。
/// 同一错误不反复弹；错误消失（cur=None）不弹（清除条目交给调用方）。纯函数，便于单测。
fn should_notify(prev: Option<&str>, cur: Option<&str>) -> bool {
    match cur {
        None => false,
        Some(c) => prev != Some(c),
    }
}

/// 待交互通知指纹:errored 或 has_pending 时不发(None,让位错误/待审批);
/// status==waiting 且无错无 pending 时用 last_event_at 作指纹;其它状态 None。纯函数。
fn waiting_fingerprint(errored: bool, has_pending: bool, status: &str, last_event_at: i64) -> Option<String> {
    if errored || has_pending || status != "waiting" {
        None
    } else {
        Some(last_event_at.to_string())
    }
}

/// 待审批通知指纹:errored 时 None(错误优先);pending 为 Some(kind) 时 "{kind}:{last_event_at}";
/// 否则 None。纯函数,便于单测。
fn pending_fingerprint(errored: bool, pending_review: Option<&str>, last_event_at: i64) -> Option<String> {
    if errored {
        return None;
    }
    pending_review.map(|kind| format!("{kind}:{last_event_at}"))
}

/// 弹一条「点击即聚焦该会话终端」的桌面通知。构建+show 放主线程：winrt toast 的 show() 需在
/// COM STA 上调用，Tauri 主线程即 STA；on_activated 回调由 OS 经 COM 激活机制投递（与消息泵无关），
/// show() 后 Rust 端 Toast 可安全释放（OS 持有通知引用）。回调里调 focus_session_terminal
/// （它自己 spawn 干净线程做 UIA，不阻塞主线程）。app 仅 Windows。
#[cfg(target_os = "windows")]
// 参数数量超限（8 个）是现有设计需要；重构签名风险大，暂以 allow 豁免。
#[allow(clippy::too_many_arguments)]
fn show_session_notification(
    app: &tauri::AppHandle,
    title: String,
    body: String,
    pid: i64,
    focus_title: String,
    focus_cwd: Option<String>,
    focus_token: Option<String>,
    title_based: bool,
) {
    use tauri_winrt_notification::Toast;
    // 安装版用 bundle identifier（解析到开始菜单快捷方式 → 显示 Meowo+图标 + 点击可激活）；
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
                focus_session_terminal(
                    pid,
                    Some(focus_title.clone()),
                    focus_cwd.clone(),
                    focus_token.clone(),
                    title_based,
                );
                Ok(())
            })
            .show();
    });
}

#[cfg(target_os = "macos")]
// 参数数量超限（8 个）是现有设计需要；重构签名风险大，暂以 allow 豁免。
#[allow(clippy::too_many_arguments)]
fn show_session_notification(
    _app: &tauri::AppHandle,
    title: String,
    body: String,
    pid: i64,
    _focus_title: String, // macOS 按 pid->tty 定位终端，标题用不上
    _focus_cwd: Option<String>,
    _focus_token: Option<String>,
    _title_based: bool,
) {
    crate::macos::notify::post(crate::macos::notify::NotifyJob { title, body, pid });
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
// 参数数量超限（8 个）是现有设计需要；重构签名风险大，暂以 allow 豁免。
#[allow(clippy::too_many_arguments)]
fn show_session_notification(
    _app: &tauri::AppHandle,
    _title: String,
    _body: String,
    _pid: i64,
    _focus_title: String,
    _focus_cwd: Option<String>,
    _focus_token: Option<String>,
    _title_based: bool,
) {
}

/// 周期轮询：收尾进程已死的卡住会话；存活集合变化或有收尾时发 board-changed 让前端刷新。
/// 同时对「连接中」会话做去重桌面通知：出错（优先）或进入待交互时各弹一次。
/// 总开关（settings.notifications_enabled）只门控是否 .show()，去重 map 始终更新，
/// 故中途打开开关不会把积压的旧错误/待交互一次性炸出来。启动首扫只播种不弹。
fn spawn_liveness_watch(
    app: tauri::AppHandle,
    db_path: PathBuf,
    tx_cache: Arc<Mutex<meowo_store::TranscriptCache>>,
) {
    use std::collections::HashMap;
    std::thread::spawn(move || {
        let mut last: Vec<i64> = Vec::new();
        let mut notified: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次错误指纹
        let mut notified_waiting: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次待交互指纹
        let mut notified_pending: HashMap<String, String> = HashMap::new(); // cc_session_id -> 上次待审批指纹
        let mut seeded = false;
        // 托盘状态摘要只在 (运行,待交互) 变化时刷新，避免每轮无谓重画/重设提示。
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let mut last_tray: Option<(usize, usize)> = None;
        loop {
            if let Ok(store) = Store::open(&db_path) {
                let sys = System::new_with_specifics(
                    RefreshKind::new().with_processes(ProcessRefreshKind::new()),
                );
                // 阈值与 session_connected 的 RESUME_GRACE_MS 对齐：pid 未知的会话「已连接」显示
                // 在此窗口后回落断开，DB 的 status 也应同时收尾，否则会话会长期卡在错误的 tab 里
                // （见 RESUME_GRACE_MS 文档：kimi 卸载后 resume「终端起来但命令失败」即命中此路径）。
                let orphaned = store.end_orphaned_idle(RESUME_GRACE_MS, now_ms()).unwrap_or(0);
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
                for s in store.live_sessions(Some("all"), None, None, None, 1000).unwrap_or_default() {
                    if s.session.status == "ended" || !pid_is_agent(&sys, s.pid.unwrap_or(0)) {
                        continue;
                    }
                    let sid = s.session.cc_session_id.clone();
                    present.insert(sid.clone(), String::new()); // 标记本轮已扫描；retain 只清理本轮彻底消失的会话

                    // 注：同 live_sessions_blocking，仅按 transcript() 门控；将来若有「有 spec 但标题走首条
                    // prompt」的 provider，需在此与 dispatch::apply_title 一致地按 resolves_transcript_title 门控标题。
                    let meowo_store::TranscriptInfo { title, error, .. } =
                        meowo_reporter::agent::for_provider(meowo_store::ProviderKey::parse(Some(&s.provider)))
                            .transcript()
                            .and_then(|spec| {
                                spec.resolve_transcript_path(None, s.cwd.as_deref(), &sid)
                                    .and_then(|p| p.to_str().map(str::to_string))
                                    .map(|path| {
                                        // 锁外 IO 版：大文件首读不阻塞 get_live_sessions（见 analyze_shared）。
                                        meowo_store::TranscriptCache::analyze_shared(&tx_cache, spec, &path)
                                    })
                            })
                            .unwrap_or_default();
                    // 会话标题：通知正文用，也作点击聚焦时匹配 WT 标签页的标题。transcript 标题优先，否则 DB 标题。
                    let display_title = title
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| s.task_title.clone());
                    let pid = s.pid.unwrap_or(0); // 连接中必为有效 pid
                    // 该 agent 是否把任务标题写进 WT 标签：决定通知点击是按标题切标签还是窗口级定位。
                    let title_based =
                        meowo_reporter::agent::for_provider(meowo_store::ProviderKey::parse(Some(&s.provider))).sets_terminal_tab_title();
                    // token = session_id 末 8 位(全局唯一)，点击通知聚焦时优先按它精确切标签。
                    let tab_token = {
                        let t = meowo_reporter::tabtitle::short_sid(&s.session.cc_session_id);
                        (!t.is_empty()).then_some(t)
                    };

                    // 菜单栏摘要计数:出错/待交互/待审批 → 需关注(●),运行中 → ○;在线空闲不计入。
                    if error.is_some() || s.session.status == "waiting" || s.pending_review.is_some() {
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
                                s.cwd.clone(),
                                tab_token.clone(),
                                title_based,
                            );
                        }
                        notified.insert(sid.clone(), e.fingerprint.clone());
                    } else {
                        notified.remove(&sid); // 错误消失：下次再错会重新通知
                    }

                    // 待审批通知(错误之后、待交互之前;errored 时 pending_fingerprint 返回 None 自动让位)。
                    match pending_fingerprint(error.is_some(), s.pending_review.as_deref(), s.session.last_event_at) {
                        Some(fp) => {
                            let prev = notified_pending.get(&sid).map(|s| s.as_str());
                            if seeded && notify_on && should_notify(prev, Some(&fp)) {
                                let key = match s.pending_review.as_deref() {
                                    Some("question") => "notify.pending.question",
                                    Some("plan") => "notify.pending.plan",
                                    _ => "notify.pending.approval",
                                };
                                show_session_notification(
                                    &app,
                                    tr(lang, key).into(),
                                    format!("{} · {}", s.project_name, display_title),
                                    pid,
                                    display_title.clone(),
                                    s.cwd.clone(),
                                    tab_token.clone(),
                                    title_based,
                                );
                            }
                            notified_pending.insert(sid.clone(), fp);
                        }
                        None => {
                            notified_pending.remove(&sid);
                        }
                    }

                    // 待交互通知（errored 时 waiting_fingerprint 返回 None，自动让位给错误）。
                    match waiting_fingerprint(error.is_some(), s.pending_review.is_some(), &s.session.status, s.session.last_event_at) {
                        Some(fp) => {
                            let prev = notified_waiting.get(&sid).map(|s| s.as_str());
                            if seeded && notify_on && should_notify(prev, Some(&fp)) {
                                show_session_notification(
                                    &app,
                                    tr(lang, "notify.waiting").into(),
                                    format!("{} · {}", s.project_name, display_title),
                                    pid,
                                    display_title.clone(),
                                    s.cwd.clone(),
                                    tab_token.clone(),
                                    title_based,
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
                notified_pending.retain(|k, _| present.contains_key(k));
                notified_waiting.retain(|k, _| present.contains_key(k));
                seeded = true;

                // macOS：把连接中会话的状态摘要写到菜单栏图标标题旁（一眼可见，弥补无吸边缩略条）。
                #[cfg(target_os = "macos")]
                if last_tray != Some((tray_running, tray_waiting)) {
                    crate::macos::menubar::update_tray_status(&app, tray_running, tray_waiting);
                    last_tray = Some((tray_running, tray_waiting));
                }
                // Windows：把摘要写到托盘悬浮提示，鼠标移到托盘一眼可见，不必打开窗口。
                #[cfg(target_os = "windows")]
                if last_tray != Some((tray_running, tray_waiting)) {
                    update_tray_tooltip(&app, tray_running, tray_waiting, lang);
                    last_tray = Some((tray_running, tray_waiting));
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let _ = (tray_running, tray_waiting);
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

/// 首次启动：~/.meowo/imported.json 不存在时，后台导入近 7 天历史会话并写标记文件。
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
            meowo_reporter::import::import_recent(&store, now, meowo_reporter::import::ImportOpts::default())
        {
            let body = format!("{{\"imported\":{count},\"at\":{now}}}");
            let _ = std::fs::write(&marker, body);
            if count > 0 {
                let _ = app.emit("board-changed", ());
            }
        }
    });
}

/// 返回所有 provider 的账号 + 缓存用量（不联网）。供多 provider 账号面板使用。
/// async + spawn_blocking：account() / usage_supported() 的 claude 分支会调
/// has_oauth_credentials() → read_credentials_root()，macOS 上可 spawn `security` 子进程；
/// 与 refresh_usage 同款写法，确保不占主线程事件循环（防设置页卡死）。
#[tauri::command]
async fn get_accounts() -> Vec<account::ProviderAccountPayload> {
    tauri::async_runtime::spawn_blocking(|| {
        account::all()
            .iter()
            .map(|pa| account::ProviderAccountPayload {
                provider: pa.key().as_str().to_string(),
                account: pa.account(),
                usage: account::read_cached_usage(pa.key()),
                usage_supported: pa.usage_supported(),
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// 刷新指定 provider 的用量（可触发网络请求，含 60s 限频）。
/// None 时按 usage_supported 返回 UNAVAILABLE 或 USAGE_UNSUPPORTED。
#[tauri::command]
async fn refresh_usage(provider: String) -> Result<account::ProviderUsage, String> {
    let key = meowo_store::ProviderKey::parse(Some(&provider));
    tauri::async_runtime::spawn_blocking(move || {
        let pa = account::for_provider(key);
        match pa.usage(true) {
            Some(u) => Ok(u),
            None => {
                if pa.usage_supported() {
                    Err("UNAVAILABLE".into())
                } else {
                    Err(account::USAGE_UNSUPPORTED.into())
                }
            }
        }
    })
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
/// macOS：terminal 必有，iterm 视安装情况；Windows：powershell/cmd 必有，wt/wezterm 视是否在 PATH。
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
        if wezterm::available() {
            v.push("wezterm".to_string());
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

/// 本机实际已安装的 agent（provider key 列表），供各处按安装过滤展示。仿 available_terminals：
/// 检测廉价（PATH/文件查询），仍放 blocking 池避免任何意外阻塞事件循环。
#[tauri::command]
async fn available_agents() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        meowo_reporter::agent::all()
            .iter()
            .filter(|a| a.is_installed())
            .map(|a| a.key().as_str().to_string())
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default()
}

/// 前端调用：打开设置窗口（贴纸 tab 栏的设置按钮）。
/// 必须在子线程创建：同步 command 跑在主线程，直接 build() 会阻塞主线程消息泵，
/// 而 WebView2 初始化依赖消息泵运转 → 卡在初始化 → 白屏。子线程里 build() 把创建
/// dispatch 回主线程异步执行，泵不被阻塞。
#[tauri::command]
fn open_settings(app: tauri::AppHandle) {
    std::thread::spawn(move || open_settings_window(&app));
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

/// 前端调用：打开软件更新窗口（贴纸更新红点 / 设置页「更新到 vX」按钮）。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
fn open_update_window(app: tauri::AppHandle) {
    std::thread::spawn(move || open_update_window_impl(&app));
}

/// 打开（或聚焦）更新窗口。label 为 "updater"（main.tsx 按此 label 路由到更新页）。
/// 更新窗口是检查/下载/安装的唯一所有者——主窗与设置窗只负责把它打开，
/// 不再有跨窗口 trigger-update/update-failed 事件协议。
fn open_update_window_impl(app: &tauri::AppHandle) {
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_focus();
        return;
    }
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "updater",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(tr(ui_lang(&load_settings()), "window.updater"))
    // 紧凑初始高度（检查中/已最新/失败态）；发现新版带更新说明时由前端 setSize 增高。
    .inner_size(400.0, 252.0)
    .min_inner_size(400.0, 252.0)
    .resizable(false)
    .decorations(false)
    .center();
    // macOS：无边框窗口不自动圆角，设透明由前端 .updater 的 border-radius 呈现（同设置窗口）。
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    match builder.build() {
        Ok(_update_window) => {
            // macOS：更新窗口关闭后切回 Accessory，重新隐藏 Dock 图标（同设置窗口）。
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _update_window.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle);
                    }
                });
            }
        }
        Err(e) => eprintln!("创建更新窗口失败: {e}"),
    }
}

/// 前端调用：打开「新建会话」窗口（贴纸底栏 + 按钮 / 空状态 CTA / 会话卡片菜单）。
/// 传入 cwd/provider 时，新建面板会预填该路径并选中该模型。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
fn open_new_session_window(app: tauri::AppHandle, cwd: Option<String>, provider: Option<String>) {
    std::thread::spawn(move || open_new_session_window_impl(&app, cwd, provider));
}

/// 打开（或聚焦）新建会话窗口。label 为 "new-session"（main.tsx 按此 label 路由到面板页）。
fn open_new_session_window_impl(
    app: &tauri::AppHandle,
    cwd: Option<String>,
    provider: Option<String>,
) {
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("new-session") {
        // 窗口已开：若从另一张卡片带了 cwd/provider 预填，通知面板更新表单（不重开窗口），再聚焦。
        if cwd.is_some() || provider.is_some() {
            use tauri::Emitter;
            let _ = app.emit("ns-prefill", serde_json::json!({ "cwd": cwd, "provider": provider }));
        }
        let _ = w.set_focus();
        return;
    }
    let url = match (&cwd, &provider) {
        (None, None) => "index.html".to_string(),
        _ => {
            let mut params = Vec::new();
            if let Some(c) = &cwd {
                params.push(format!("cwd={}", percent_encode(c.as_bytes(), NON_ALPHANUMERIC)));
            }
            if let Some(p) = &provider {
                params.push(format!("provider={}", percent_encode(p.as_bytes(), NON_ALPHANUMERIC)));
            }
            format!("index.html?{}", params.join("&"))
        }
    };
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "new-session",
        tauri::WebviewUrl::App(url.into()),
    )
    .title(tr(ui_lang(&load_settings()), "window.newSession"))
    .inner_size(460.0, 420.0)
    .min_inner_size(460.0, 420.0)
    .resizable(false)
    .decorations(false)
    .center();
    // macOS：无边框窗口不自动圆角，设透明由前端 .ns-window 的 border-radius 呈现（同设置窗口）。
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    match builder.build() {
        Ok(_win) => {
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _win.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle);
                    }
                });
            }
        }
        Err(e) => eprintln!("创建新建会话窗口失败: {e}"),
    }
}

/// 「找回贴纸」：把主窗口按当前尺寸居中到主显示器工作区，并显示/取消最小化/置顶/聚焦。
/// 折叠态的「展开 + 还原正常尺寸」由前端在调用本命令前完成（snap_restore），故这里只按当前尺寸居中。
#[tauri::command]
fn recall_center(window: tauri::WebviewWindow) -> Result<(), String> {
    let _ = window.unminimize();
    let _ = window.show();
    // 优先主显示器（找回的「家」最可预期）；取不到回退当前屏。
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let wa = m.work_area();
        let sz = window.outer_size().map_err(|e| e.to_string())?;
        let x = wa.position.x + (wa.size.width as i32 - sz.width as i32) / 2;
        let y = wa.position.y + (wa.size.height as i32 - sz.height as i32) / 2;
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
    }
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    Ok(())
}

/// 托盘「找回贴纸」：唤起主窗口并通知前端执行完整找回（展开折叠 + 居中到主屏 + 置顶）。
#[cfg(not(target_os = "macos"))]
fn recall_sticker(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("recall-sticker", ());
    }
}

/// 托盘右键菜单（找回贴纸 / 设置 / 退出），按语言构建；切语言时由 rebuild_tray_menu 重建。
#[cfg(not(target_os = "macos"))]
fn build_tray_menu(app: &tauri::AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let recall = MenuItemBuilder::with_id("recall", tr(lang, "tray.recall")).build(app)?;
    let settings = MenuItemBuilder::with_id("settings", tr(lang, "tray.settings")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(lang, "tray.quit")).build(app)?;
    MenuBuilder::new(app).items(&[&recall, &settings, &quit]).build()
}

/// 切语言后让已存在的系统 UI 跟上：重建托盘菜单、改已开设置窗口的标题。
pub(crate) fn apply_language(app: &tauri::AppHandle, lang: &str) {
    if let Some(tray) = app.tray_by_id("meowo-tray") {
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
    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_title(tr(lang, "window.updater"));
    }
}

/// 构建系统托盘：左键点击直接打开设置；右键菜单提供设置 / 退出。
/// macOS 走 `macos::menubar::setup_tray`（面板模式），故此实现仅用于非 macOS 平台。
#[cfg(not(target_os = "macos"))]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app.handle(), ui_lang(&load_settings()))?;

    let mut builder = TrayIconBuilder::with_id("meowo-tray");
    // 图标恒由打包提供，但缺失时不该 unwrap panic 把启动打挂——没图标就建无图标托盘。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .tooltip("Meowo")
        .menu(&menu)
        // 左键留给「打开设置」，菜单仅在右键弹出。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "recall" => recall_sticker(app),
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

/// Windows：把待交互/运行中会话数摘要写进托盘悬浮提示，鼠标移到托盘一眼可见，
/// 弥补桌面端无菜单栏标题。计数为 0 时回落到纯品牌名。
#[cfg(target_os = "windows")]
fn update_tray_tooltip(app: &tauri::AppHandle, running: usize, waiting: usize, lang: &str) {
    let Some(tray) = app.tray_by_id("meowo-tray") else {
        return;
    };
    let _ = tray.set_tooltip(Some(tray_tooltip_text(lang, running, waiting)));
}

/// 构建托盘提示文案（本地化）。待交互更紧急，排在运行中之前。
#[cfg(target_os = "windows")]
fn tray_tooltip_text(lang: &str, running: usize, waiting: usize) -> String {
    if running == 0 && waiting == 0 {
        return "Meowo".into();
    }
    let mut parts: Vec<String> = Vec::new();
    if lang == "en" {
        if waiting > 0 {
            parts.push(format!("{waiting} waiting"));
        }
        if running > 0 {
            parts.push(format!("{running} running"));
        }
    } else {
        if waiting > 0 {
            parts.push(format!("{waiting} 个待交互"));
        }
        if running > 0 {
            parts.push(format!("{running} 个运行中"));
        }
    }
    format!("Meowo · {}", parts.join(" · "))
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
    migrate_legacy_data();
    let path = db_path();
    let tx_cache: Arc<Mutex<meowo_store::TranscriptCache>> =
        Arc::new(Mutex::new(meowo_store::TranscriptCache::new()));
    tauri::Builder::default()
        // window-state 只持久化/恢复「位置」等，不恢复「尺寸」：main 窗口尺寸改由前端 localStorage
        // (SIZE_KEY) 单独持有。否则吸附态退出会把「细条几何」存进 window-state，与 localStorage 的吸附态
        // (SNAP_KEY) 两套持久化不同步——重启读不到 SNAP_KEY 却被还原成细条尺寸，渲染完整贴纸而没真正吸附。
        // about 设置窗口固定尺寸(resizable=false)，不受影响；折叠/正常尺寸均由前端 snap 逻辑权威设定。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        .difference(tauri_plugin_window_state::StateFlags::SIZE),
                )
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            db_path: path.clone(),
            tx_cache: tx_cache.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            get_project_tasks,
            get_live_sessions_counts,
            get_live_sessions_page,
            focus_session,
            resume_session,
            open_project_dir,
            rename_session,
            set_archived,
            set_session_note,
            get_autostart,
            set_autostart,
            get_settings,
            set_settings,
            open_settings,
            open_update_window,
            recall_center,
            open_url,
            snap_collapse,
            snap_expand,
            snap_restore,
            unsnap,
            cursor_over_window,
            pointer_left_down,
            get_accounts,
            refresh_usage,
            host_os,
            available_terminals,
            available_agents,
            new_session,
            install_agent,
            check_provider_hooks,
            repair_provider_hooks,
            recent_cwds,
            open_new_session_window
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
                // 开机自启安全网：OS 刚登录时显示器/工作区可能尚未枚举完（多屏、副屏在负坐标、外接屏后上电），
                // 此刻 available_monitors() 为空会让上面的 pull_on_screen 直接跳过救援；而贴纸 skipTaskbar 不进
                // 任务栏，窗口若停在上次副屏(未就绪)的坐标就完全不可见、用户以为「没启动」。这里后台等显示器
                // 就绪后强制(force=true)把窗口钳进相交最大/主显示器工作区，保证可见——且不依赖前端 JS 是否跑起来。
                // clamp 对已在屏内的窗口是 no-op，不会无故移动正常摆放的窗口。
                {
                    let wc = w.clone();
                    std::thread::spawn(move || {
                        for _ in 0..40 {
                            // ~6s
                            if wc.available_monitors().map(|m| !m.is_empty()).unwrap_or(false) {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                        pull_on_screen(&wc, true);
                    });
                }
                // 装上位置约束子类：在移动生效前硬钳坐标，彻底拖不出屏幕。
                if let Ok(h) = w.hwnd() {
                    win_constrain::set_app(app.handle().clone()); // 供子类拖边框缩放时通知前端
                    win_constrain::install(h.0 as isize);
                }
            }
            // 无感适配：幂等把 meowo-reporter 接入各 AI CLI（claude: hooks+statusLine；codex/kimi: hooks）。后台跑，失败不影响启动。
            std::thread::spawn(setup::apply_all);
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
        is_safe_id, normalize_tab_title, parse_wt_default_profile, path_has_exe,
        pending_fingerprint, pid_is_agent, session_connected, shell_join_for_windows, should_notify,
        strip_jsonc_comments, tab_match_score, waiting_fingerprint,
    };
    use crate::settings::Settings;
    use crate::snap::{
        center_on, clamp_xy_to_work, edge_for_rect, intersection_area, Edge, Rect,
    };
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};

    const WORK1: Rect = Rect { x: 0, y: 0, w: 2556, h: 1179 };

    #[cfg(target_os = "windows")]
    #[test]
    fn tray_tooltip_text_localizes_and_orders_waiting_first() {
        use super::tray_tooltip_text;
        // 入参顺序：(lang, running, waiting)。待交互更紧急，排在运行中之前。
        assert_eq!(tray_tooltip_text("zh", 0, 0), "Meowo");
        assert_eq!(tray_tooltip_text("zh", 2, 3), "Meowo · 3 个待交互 · 2 个运行中");
        assert_eq!(tray_tooltip_text("zh", 2, 0), "Meowo · 2 个运行中");
        assert_eq!(tray_tooltip_text("en", 0, 2), "Meowo · 2 waiting");
        assert_eq!(tray_tooltip_text("en", 1, 1), "Meowo · 1 waiting · 1 running");
    }

    #[test]
    fn shell_join_quotes_spaced_paths_for_powershell_and_cmd() {
        let to_vec = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // 无空格（claude）：原样拼接，两种 shell 一致。
        let plain = to_vec(&["claude", "--resume", "ID"]);
        assert_eq!(shell_join_for_windows(&plain, true), "claude --resume ID");
        assert_eq!(shell_join_for_windows(&plain, false), "claude --resume ID");
        // 可执行绝对路径含空格（kimi）：PowerShell 用单引号字面量 + & 调用运算符（双引号内 $/` 会被
        // 插值展开，单引号内一切按字面），cmd 用双引号。
        let spaced = to_vec(&[r"C:\Users\First Last\.kimi-code\bin\kimi.exe", "-r", "session_x"]);
        assert_eq!(
            shell_join_for_windows(&spaced, true),
            r"& 'C:\Users\First Last\.kimi-code\bin\kimi.exe' -r session_x"
        );
        assert_eq!(
            shell_join_for_windows(&spaced, false),
            r#""C:\Users\First Last\.kimi-code\bin\kimi.exe" -r session_x"#
        );
        // node 包装（codex）：命令名无空格、脚本路径参数有空格 → 只 quote 参数，PowerShell 不需要 &。
        let node = to_vec(&["node", r"C:\Users\First Last\AppData\Roaming\npm\codex.js", "resume", "ID"]);
        assert_eq!(
            shell_join_for_windows(&node, true),
            r"node 'C:\Users\First Last\AppData\Roaming\npm\codex.js' resume ID"
        );
        // 用户名含 $（合法字符）：无空格也要单引号包裹，否则 PowerShell 变量插值把路径吞掉。
        let dollar = to_vec(&[r"C:\Users\a$b\.kimi-code\bin\kimi.exe", "-r", "id"]);
        assert_eq!(
            shell_join_for_windows(&dollar, true),
            r"& 'C:\Users\a$b\.kimi-code\bin\kimi.exe' -r id"
        );
        // 路径含单引号（如 O'Brien）：内嵌单引号翻倍。
        let apos = to_vec(&[r"C:\Users\O'Brien\kimi.exe", "-r", "id"]);
        assert_eq!(
            shell_join_for_windows(&apos, true),
            r"& 'C:\Users\O''Brien\kimi.exe' -r id"
        );
    }

    #[test]
    fn path_has_exe_scans_path_dirs_without_spawning() {
        let dir = std::env::temp_dir().join("meowo-test-path-has-exe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("wt.exe"), b"stub").unwrap();
        // 单目录命中 / 未命中
        let single = std::env::join_paths([dir.clone()]).unwrap();
        assert!(path_has_exe(&single, "wt.exe"));
        assert!(!path_has_exe(&single, "definitely-absent.exe"));
        // 多目录：前面的目录不存在也不影响后面命中
        let multi =
            std::env::join_paths([std::env::temp_dir().join("meowo-no-such-dir"), dir.clone()])
                .unwrap();
        assert!(path_has_exe(&multi, "wt.exe"));
        // 空 PATH → 找不到
        assert!(!path_has_exe(std::ffi::OsStr::new(""), "wt.exe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pid_is_agent_rejects_non_claude_and_dead() {
        let sys =
            System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
        // 当前测试进程存在但不叫 claude → 不算连接（pid 复用防护）
        assert!(!pid_is_agent(&sys, std::process::id() as i64));
        // 非法 / 已死的 pid
        assert!(!pid_is_agent(&sys, 0));
        assert!(!pid_is_agent(&sys, -1));
        assert!(!pid_is_agent(&sys, 4_000_000_000));
    }

    #[test]
    fn session_connected_logic() {
        let now = 1_000_000i64;
        // 结束 → 断开（即使 pid 看着是活的）。
        assert!(!session_connected("ended", Some(123), true, now, now));
        // 活着的 agent 进程 → 连接（与时间无关）。
        assert!(session_connected("running", Some(123), true, 0, now));
        // pid 有值但已死/被复用 → 断开（防 pid 复用误判）。
        assert!(!session_connected("running", Some(123), false, now, now));
        // pid 未知 + 在 resume 宽限期内 → 连接（刚 resume，等 codex 首个 hook）。
        assert!(session_connected("running", None, false, now - 1_000, now));
        assert!(session_connected("waiting", None, false, now - 1_000, now));
        // pid 未知 + 超出宽限期 → 断开（终端没起来/被关的僵尸会话，不再假连接）。
        assert!(!session_connected("running", None, false, now - 200_000, now));
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
    fn safe_id_accepts_uuid_and_kimi() {
        // claude 的 UUID 与 kimi 的 session_<uuid> 都应通过（focus/resume/rename/note 共用此校验）。
        assert!(is_safe_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(is_safe_id("00000000-0000-0000-0000-000000000000"));
        assert!(is_safe_id("session_a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    }

    #[test]
    fn safe_id_rejects_injection_and_malformed() {
        // 含 shell/wt 元字符、空格、路径分隔符/点 → 拒绝（命令注入 + 路径穿越防护）。
        assert!(!is_safe_id("'; calc; '")); // 注入尝试
        assert!(!is_safe_id("abc --resume x; calc"));
        assert!(!is_safe_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890 ")); // 尾空格
        assert!(!is_safe_id("../../etc/passwd")); // 路径穿越
        assert!(!is_safe_id("a/b")); // 路径分隔符
        assert!(!is_safe_id("a.b")); // 点（穿越/扩展名）
        assert!(!is_safe_id("")); // 空
        assert!(!is_safe_id(&"a".repeat(129))); // 超长 >128
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
    fn pending_fingerprint_rules() {
        // errored 优先 → None(让位错误)。
        assert_eq!(pending_fingerprint(true, Some("approval"), 100), None);
        // pending 为 Some 且未出错 → Some("{kind}:{last_event_at}")。
        assert_eq!(pending_fingerprint(false, Some("question"), 100).as_deref(), Some("question:100"));
        // 无 pending → None。
        assert_eq!(pending_fingerprint(false, None, 100), None);
        // 指纹随 last_event_at 变化(新回合新指纹)。
        assert_ne!(pending_fingerprint(false, Some("approval"), 100), pending_fingerprint(false, Some("approval"), 200));
    }

    #[test]
    fn waiting_fingerprint_rules() {
        // 错误优先:无指纹。
        assert_eq!(waiting_fingerprint(true, false, "waiting", 100), None);
        // pending 优先:无 waiting 指纹(让位 pending)。
        assert_eq!(waiting_fingerprint(false, true, "waiting", 100), None);
        // 纯 waiting:用 last_event_at 作指纹。
        assert_eq!(waiting_fingerprint(false, false, "waiting", 100).as_deref(), Some("100"));
        // 非 waiting 状态:None。
        assert_eq!(waiting_fingerprint(false, false, "running", 100), None);
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
        // 老文件缺外观字段 → 用缺省（dark / 100 / 100），不报错。
        let legacy: Settings = serde_json::from_str(r#"{"archive_hide_days":7}"#).unwrap();
        assert_eq!(legacy.theme, "dark");
        assert_eq!(legacy.opacity, 100);
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
        assert_eq!(d.opacity, 100);
        assert_eq!(d.ui_scale, 100);
    }

}

#[cfg(test)]
mod new_session_tests {
    use super::*;

    #[test]
    fn validate_cwd_rejects_empty_and_missing() {
        assert!(validate_new_session_cwd("").is_err());
        assert!(validate_new_session_cwd("   ").is_err());
        assert!(validate_new_session_cwd("C:/definitely/not/a/real/dir/xyz123").is_err());
    }

    #[test]
    fn validate_cwd_accepts_existing_dir() {
        let tmp = std::env::temp_dir();
        let got = validate_new_session_cwd(tmp.to_str().unwrap()).unwrap();
        assert_eq!(got, tmp.to_str().unwrap().trim());
    }
}

#[cfg(test)]
mod hooks_check_tests {
    use super::*;

    #[test]
    fn hooks_json_detects_reporter_with_provider() {
        let v: serde_json::Value = serde_json::from_str(r#"{
          "hooks": { "SessionStart": [
            { "matcher": "*", "hooks": [
              { "type": "command", "command": "\"C:/x/meowo-reporter.exe\" --provider codex", "timeout": 5 }
            ]}
          ]}
        }"#).unwrap();
        assert!(hooks_json_has_reporter(&v, "codex"));
        assert!(!hooks_json_has_reporter(&v, "kimi")); // provider 不符
    }

    #[test]
    fn hooks_json_ignores_foreign_hooks() {
        let v: serde_json::Value = serde_json::from_str(r#"{
          "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": "node other.js" }] }
          ]}
        }"#).unwrap();
        assert!(!hooks_json_has_reporter(&v, "codex"));
        // 无 hooks 键。
        let empty: serde_json::Value = serde_json::from_str("{}").unwrap();
        assert!(!hooks_json_has_reporter(&empty, "codex"));
    }

    #[test]
    fn hooks_json_ignores_reporter_on_non_session_start_event() {
        // 只在 Stop 挂了 meowo-reporter：不能保证新会话会入库，不应判定为 Installed（审查发现）。
        let v: serde_json::Value = serde_json::from_str(r#"{
          "hooks": { "Stop": [
            { "hooks": [{ "type": "command", "command": "\"C:/x/meowo-reporter.exe\" --provider codex" }] }
          ]}
        }"#).unwrap();
        assert!(!hooks_json_has_reporter(&v, "codex"));
        // SessionStart 与 Stop 并存：仍应命中。
        let v2: serde_json::Value = serde_json::from_str(r#"{
          "hooks": {
            "Stop": [{ "hooks": [{ "type": "command", "command": "node other.js" }] }],
            "SessionStart": [{ "hooks": [{ "type": "command", "command": "\"C:/x/meowo-reporter.exe\" --provider codex" }] }]
          }
        }"#).unwrap();
        assert!(hooks_json_has_reporter(&v2, "codex"));
    }

    // kimi config.toml 的 SessionStart 判定已迁入 meowo_agent::config::ConfigFormat::KimiToml，
    // 测试随之搬到该 crate（见 has_reporter_only_counts_session_start）。

    #[test]
    fn claude_hooks_status_three_way() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("cckb-claude-hooks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        let _ = std::fs::remove_file(&path);
        assert!(matches!(claude_hooks_status_at(&path), HooksStatus::Missing));

        // Installed：command 用 ccsetup 认可的「带引号 meowo-reporter 路径」格式（参照 ccsetup 既有
        // 测试 reporter_exe_path_strict_matches_only_our_exe / ensure_hooks_* 里的 command 串）。
        let installed = r#"{"hooks":{"SessionStart":[{"matcher":"*","hooks":[{"type":"command","command":"\"C:/x/meowo-reporter.exe\""}]}]}}"#;
        std::fs::File::create(&path).unwrap().write_all(installed.as_bytes()).unwrap();
        assert!(matches!(claude_hooks_status_at(&path), HooksStatus::Installed));

        let foreign = r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"node other.js"}]}]}}"#;
        std::fs::File::create(&path).unwrap().write_all(foreign.as_bytes()).unwrap();
        assert!(matches!(claude_hooks_status_at(&path), HooksStatus::Missing));

        // 损坏 JSON → Unknown（核心不变量：不误报 Missing）
        std::fs::File::create(&path).unwrap().write_all(b"{not json").unwrap();
        assert!(matches!(claude_hooks_status_at(&path), HooksStatus::Unknown));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
