//! 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条）的窗口几何。
//!
//! 岛永远贴在所在显示器工作区的**顶边**，只能左右拖（用户选的）。三种形态（收起胶囊 /
//! 提醒 / 展开成完整列表）只是尺寸不同：前端给逻辑宽高，这里按**当前中心点**左右对称
//! 地放大缩小，所以从胶囊长成列表时不会往一边歪。
//!
//! 拖动不用系统拖拽（`start_dragging` 会连上下一起拖）：前端按下时
//! 调 begin、移动时调 update、松手时调 end，由这里读真实光标位置算横坐标。拖过显示器边界
//! 就落到那块屏的顶边。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// 拖动起点：(光标物理 x, 窗口左上角物理 x)。
static DRAG: Mutex<Option<(f64, i32)>> = Mutex::new(None);
/// 岛的窗口样式（不可缩放、放开最小尺寸）是否已设过。它们不随形态变，只需设一次；每次改
/// 形态都重设，就是每次都让 tao 走一遍风格位比对（window.rs 记过那里 VISIBLE 不同步会把
/// 窗口藏掉），白担风险（审查发现）。
static STYLE_SET: AtomicBool = AtomicBool::new(false);

/// 松手时记下的位置：中心比例 + 所在显示器名。
#[derive(serde::Serialize)]
pub(crate) struct IslandSpot {
    ratio: f64,
    monitor: Option<String>,
}

/// 工作区的物理矩形 (x, y, w, h)。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Work {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn work_of(m: &tauri::window::Monitor) -> Work {
    let wa = m.work_area();
    Work {
        x: wa.position.x,
        y: wa.position.y,
        w: wa.size.width as i32,
        h: wa.size.height as i32,
    }
}

/// 宽 `w` 的岛以 `center_x` 为中心，横向钳进工作区；纵向贴顶。返回左上角 (x, y)。
fn place(work: Work, center_x: f64, w: i32) -> (i32, i32) {
    let w = w.min(work.w);
    let left = (center_x - f64::from(w) / 2.0).round() as i32;
    (left.clamp(work.x, work.x + work.w - w), work.y)
}

/// 中心点在工作区里的横向比例（0..1），用于记住位置：换分辨率/换屏后按比例还原。
fn center_ratio(work: Work, left: i32, w: i32) -> f64 {
    if work.w <= 0 {
        return 0.5;
    }
    (f64::from(left - work.x) + f64::from(w) / 2.0) / f64::from(work.w)
}

/// 光标所在显示器；取不到就用窗口所在的。
fn monitor_at(window: &tauri::WebviewWindow, x: f64, y: f64) -> Option<tauri::window::Monitor> {
    let hit = window.available_monitors().ok()?.into_iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        x >= f64::from(p.x)
            && x < f64::from(p.x) + f64::from(s.width)
            && y >= f64::from(p.y)
            && y < f64::from(p.y) + f64::from(s.height)
    });
    hit.or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
}

/// 设定岛的尺寸（逻辑宽高）并贴顶摆放。`center_ratio` 给了就按它定中心（启动时还原记住的
/// 位置），否则保持当前中心（形态切换）。
#[tauri::command]
pub(crate) fn island_layout(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    center_ratio: Option<f64>,
    monitor: Option<String>,
) -> Result<(), String> {
    // 启动还原时给了记住的显示器名：那块屏还在就落到它上面，否则用窗口当前所在屏。
    let named = monitor.as_deref().and_then(|name| {
        window
            .available_monitors()
            .ok()?
            .into_iter()
            .find(|m| m.name().map(String::as_str) == Some(name))
    });
    let monitor = named
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
        .ok_or("no monitor")?;
    let work = work_of(&monitor);
    let scale = monitor.scale_factor();
    let phys_w = ((width.clamp(1.0, 4000.0) * scale).round() as i32).max(1);
    let phys_h = ((height.clamp(1.0, 4000.0) * scale).round() as i32).clamp(1, work.h.max(1));
    let center = match center_ratio {
        Some(r) if r.is_finite() => f64::from(work.x) + r.clamp(0.0, 1.0) * f64::from(work.w),
        _ => {
            let pos = window.outer_position().map_err(|e| e.to_string())?;
            let size = window.outer_size().map_err(|e| e.to_string())?;
            f64::from(pos.x) + f64::from(size.width) / 2.0
        }
    };
    let (x, y) = place(work, center, phys_w);
    // 岛的尺寸全由程序给，用户拖边缩放没有意义；最小尺寸也要放开，否则胶囊缩不下去
    // （贴纸的 360×330 下限是给正常卡片的）。
    if !STYLE_SET.swap(true, Ordering::Relaxed) {
        window.set_resizable(false).map_err(|e| e.to_string())?;
        window.set_min_size(None::<tauri::LogicalSize<f64>>).map_err(|e| e.to_string())?;
    }
    set_bounds(&window, x, y, phys_w.min(work.w), phys_h)?;
    if !window.is_always_on_top().map_err(|e| e.to_string())? {
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 尺寸和位置一次设好。分两步的话，中间那一帧窗口从旧的左边缘往右长，展开时看得到
/// 一下往一边歪（审查发现）。Windows 用一次 SetWindowPos；别的平台退回两步。
#[cfg(target_os = "windows")]
fn set_bounds(window: &tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    // SAFETY: hwnd 来自活着的 tauri 窗口；只改位置尺寸，不动 z 序、不抢焦点。
    let ok = unsafe { SetWindowPos(hwnd.0 as _, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
    if ok == 0 {
        return Err("SetWindowPos failed".into());
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn set_bounds(window: &tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
    window
        .set_size(tauri::PhysicalSize::new(w.max(1) as u32, h.max(1) as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

/// 光标相对岛中心的位置（逻辑像素，x 向右、y 向下为正）。眼睛据此看向鼠标。
/// 只读光标与窗口位置，与 cursor_over_window 同量级，同步命令即可。取不到时 None。
#[tauri::command]
pub(crate) fn island_cursor(window: tauri::WebviewWindow) -> Option<(f64, f64)> {
    let cursor = window.cursor_position().ok()?;
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().ok()?.max(0.1);
    let cx = f64::from(pos.x) + f64::from(size.width) / 2.0;
    let cy = f64::from(pos.y) + f64::from(size.height) / 2.0;
    Some(((cursor.x - cx) / scale, (cursor.y - cy) / scale))
}

/// 开始左右拖：记下光标与窗口的起点。
#[tauri::command]
pub(crate) fn island_drag_begin(window: tauri::WebviewWindow) -> Result<(), String> {
    let cursor = window.cursor_position().map_err(|e| e.to_string())?;
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    *DRAG.lock().unwrap_or_else(|e| e.into_inner()) = Some((cursor.x, pos.x));
    Ok(())
}

/// 拖动中：按光标横向位移挪窗口，纵向始终贴光标所在屏的顶边。
#[tauri::command]
pub(crate) fn island_drag_update(window: tauri::WebviewWindow) -> Result<(), String> {
    let Some((start_cursor, start_left)) = *DRAG.lock().unwrap_or_else(|e| e.into_inner()) else {
        return Ok(());
    };
    let cursor = window.cursor_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let w = size.width as i32;
    let Some(monitor) = monitor_at(&window, cursor.x, cursor.y) else {
        return Ok(());
    };
    let center = f64::from(start_left) + (cursor.x - start_cursor) + f64::from(w) / 2.0;
    let (x, y) = place(work_of(&monitor), center, w);
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}

/// 松手：返回中心点在所在屏工作区里的横向比例和那块屏的名字，前端拿去记住位置。
#[tauri::command]
pub(crate) fn island_drag_end(window: tauri::WebviewWindow) -> Result<Option<IslandSpot>, String> {
    let started = DRAG.lock().unwrap_or_else(|e| e.into_inner()).take();
    if started.is_none() {
        return Ok(None);
    }
    let pos = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .ok_or("no monitor")?;
    Ok(Some(IslandSpot {
        ratio: center_ratio(work_of(&monitor), pos.x, size.width as i32),
        monitor: monitor.name().cloned(),
    }))
}

/// 全屏程序在前台时（游戏、全屏视频、演示模式）岛该怎么办。岛恒置顶、贴在屏幕顶部正中，
/// 不藏的话会盖在全屏画面上，还挡住那一块的点击。只恢复我们自己藏的：别的原因隐藏的窗口
/// （启动还没就绪、用户关掉）不该被这里弄出来。
#[cfg(any(target_os = "windows", test))]
#[derive(Debug, PartialEq, Eq)]
enum FullscreenAction {
    Hide,
    Show,
    Nothing,
}

#[cfg(any(target_os = "windows", test))]
fn fullscreen_action(fullscreen: bool, hidden_by_us: bool, visible: bool) -> FullscreenAction {
    match (fullscreen, hidden_by_us) {
        (true, false) if visible => FullscreenAction::Hide,
        (false, true) => FullscreenAction::Show,
        _ => FullscreenAction::Nothing,
    }
}

/// 有全屏程序盖住岛所在的那块屏（游戏、全屏视频、F11 浏览器、放映）。按 z 序从上往下看岛所在
/// 屏上的窗口：第一个普通（非置顶）窗口铺满整块屏且不是最大化，才算全屏；置顶窗口里有铺满的
/// （独占全屏游戏常带 TOPMOST）也算。不看前台窗口：放映在投影屏、人在笔记本屏上操作时，前台
/// 在别的屏，岛会冒到投影画面上（做空复现）。最大化窗口在没有任务栏的屏上也铺满整屏，必须
/// 排除，否则那块屏上一用最大化窗口岛就没了（做空实测：最大化矩形每边比工作区多 11px）。
/// 此前用 SHQueryUserNotificationState 的 BUSY/APP，不分是哪块屏、也不一定真是全屏。
/// 演示模式（Windows「演示设置」）系统层面要求免打扰，照旧算。
/// UWP 全屏（Win+Shift+Enter）EnumWindows 枚举不到，这里判不出；它所在的层高于所有桌面
/// 置顶窗口，岛本来就被盖住、点不到（做空实测），所以不必为它把 QUNS_APP 加回来。
#[cfg(target_os = "windows")]
fn fullscreen_app_running(island: &tauri::WebviewWindow) -> bool {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
    use windows_sys::Win32::UI::Shell::{SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_PRESENTATION_MODE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, GWL_EXSTYLE,
        GWL_STYLE, WS_CAPTION, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    };

    let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
    // SAFETY: 传入有效的输出指针；失败时 state 不可信，按「不是演示模式」处理。
    if unsafe { SHQueryUserNotificationState(&mut state) } >= 0 && state == QUNS_PRESENTATION_MODE {
        return true;
    }
    let Ok(own) = island.hwnd() else { return false };
    let own = own.0 as HWND;

    struct Scan {
        own: HWND,
        monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
        screen: RECT,
        found: bool,
    }
    /// 外壳/输入法这类铺满屏幕但不是全屏程序的窗口（桌面、Win+. 输入体验等）。
    fn shell_class(class: &str) -> bool {
        matches!(
            class,
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow"
                | "XamlExplorerHostIslandWindow" | "ApplicationFrameWindow_Placeholder"
        )
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam 是下面传入的 &mut Scan，EnumWindows 同步回调，期间独占。
        let scan = &mut *(lparam as *mut Scan);
        if hwnd == scan.own || IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let mut cloaked: u32 = 0;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, (&mut cloaked as *mut u32).cast(), 4) >= 0 && cloaked != 0 {
            return 1;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        // 工具窗不算；点击穿透的分层覆盖层（游戏/录屏的 HUD）铺满整屏也不是全屏程序，算了会让
        // 岛在那块屏上一直消失、用户找不到原因（做空复现）。
        if ex & WS_EX_TOOLWINDOW != 0 || ex & (WS_EX_LAYERED | WS_EX_TRANSPARENT) == (WS_EX_LAYERED | WS_EX_TRANSPARENT) {
            return 1;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if shell_class(&String::from_utf16_lossy(&class[..n.max(0) as usize])) {
            return 1;
        }
        if MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) != scan.monitor {
            return 1;
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return 1;
        }
        // 只排除带标题栏的最大化窗口（没有任务栏的屏上它也铺满整屏）。无边框 + 最大化是 WPF/
        // WinForms 程序最常见的全屏写法，矩形与屏幕严格相等，要算全屏（做空复现）。
        let captioned = GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == WS_CAPTION;
        let full = !(IsZoomed(hwnd) != 0 && captioned) && covers(rect, scan.screen);
        if ex & WS_EX_TOPMOST != 0 {
            // 置顶的小工具不挡判定，继续往下看；铺满整屏的置顶窗口就是全屏。
            if full {
                scan.found = true;
                return 0;
            }
            return 1;
        }
        // 第一个普通窗口说了算：它铺满就是全屏，否则这块屏上最上层是个普通窗口。
        scan.found = full;
        0
    }
    // SAFETY: 只读查询；回调里的指针在 EnumWindows 返回前一直有效。
    unsafe {
        let monitor = MonitorFromWindow(own, MONITOR_DEFAULTTONULL);
        if monitor.is_null() {
            return false;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let mut scan = Scan { own, monitor, screen: info.rcMonitor, found: false };
        EnumWindows(Some(visit), &mut scan as *mut Scan as LPARAM);
        scan.found
    }
}

/// 窗口矩形盖住整块屏幕（最大化窗口不盖任务栏，不算）。
#[cfg(any(target_os = "windows", test))]
fn covers(window: windows_rect::Rect, monitor: windows_rect::Rect) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

/// 测试里没有 windows-sys：给 covers 一个同形的矩形。
#[cfg(any(target_os = "windows", test))]
mod windows_rect {
    #[cfg(target_os = "windows")]
    pub(super) type Rect = windows_sys::Win32::Foundation::RECT;
    #[cfg(not(target_os = "windows"))]
    #[derive(Clone, Copy)]
    pub(super) struct Rect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
}

/// 后台每 1.5 秒查一次全屏状态：进全屏把岛藏起来，退出再不抢焦点地显示回来。
#[cfg(target_os = "windows")]
pub(crate) fn spawn_fullscreen_watch(app: tauri::AppHandle) {
    use tauri::Manager as _;
    let _ = std::thread::Builder::new()
        .name("island-fullscreen".into())
        .spawn(move || {
            let mut hidden_by_us = false;
            loop {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let Some(window) = app.get_webview_window("main") else {
                    continue;
                };
                let visible = window.is_visible().unwrap_or(false);
                match fullscreen_action(fullscreen_app_running(&window), hidden_by_us, visible) {
                    FullscreenAction::Hide => {
                        if window.hide().is_ok() {
                            hidden_by_us = true;
                        }
                    }
                    FullscreenAction::Show => {
                        crate::window::show_window_no_activate(&window);
                        hidden_by_us = false;
                    }
                    FullscreenAction::Nothing => {}
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Work = Work { x: 0, y: 0, w: 1920, h: 1040 };

    #[test]
    fn fullscreen_hides_only_visible_island_and_restores_only_what_it_hid() {
        // 进全屏：岛可见就藏；本来就不可见（启动没就绪、用户关掉）不动。
        assert_eq!(fullscreen_action(true, false, true), FullscreenAction::Hide);
        assert_eq!(fullscreen_action(true, false, false), FullscreenAction::Nothing);
        // 全屏中保持藏着。
        assert_eq!(fullscreen_action(true, true, false), FullscreenAction::Nothing);
        // 退出全屏：只恢复我们藏的。
        assert_eq!(fullscreen_action(false, true, false), FullscreenAction::Show);
        assert_eq!(fullscreen_action(false, false, false), FullscreenAction::Nothing);
    }

    #[test]
    fn fullscreen_means_covering_the_whole_monitor() {
        let r = |left, top, right, bottom| windows_rect::Rect { left, top, right, bottom };
        let screen = r(0, 0, 1920, 1080);
        assert!(covers(r(0, 0, 1920, 1080), screen));
        // 无边框全屏常比屏幕大几像素。
        assert!(covers(r(-8, -8, 1928, 1088), screen));
        // 最大化窗口不盖任务栏。
        assert!(!covers(r(0, 0, 1920, 1040), screen));
        // 另一块屏上的全屏窗口。
        assert!(!covers(r(1920, 0, 3840, 1080), screen));
    }

    #[test]
    fn place_centers_and_sticks_to_top() {
        assert_eq!(place(WORK, 960.0, 200), (860, 0));
        // 副屏在左、顶边不在 0：贴的是那块屏工作区的顶。
        let left_screen = Work { x: -1280, y: 200, w: 1280, h: 984 };
        assert_eq!(place(left_screen, -640.0, 100), (-690, 200));
    }

    #[test]
    fn place_clamps_inside_work_area() {
        // 拖到最左/最右：贴着工作区边缘停住，不出屏。
        assert_eq!(place(WORK, 10.0, 200), (0, 0));
        assert_eq!(place(WORK, 1915.0, 200), (1720, 0));
        // 比工作区还宽（极端缩放）：宽度按工作区算，贴左。
        assert_eq!(place(WORK, 960.0, 5000), (0, 0));
    }

    #[test]
    fn center_ratio_round_trips_with_place() {
        let (left, _) = place(WORK, 480.0, 300);
        let ratio = center_ratio(WORK, left, 300);
        assert!((ratio - 0.25).abs() < 1e-9, "{ratio}");
        // 按比例还原到另一种分辨率，中心落在同样的相对位置。
        let wide = Work { x: 0, y: 0, w: 2560, h: 1400 };
        let (left2, _) = place(wide, f64::from(wide.x) + ratio * f64::from(wide.w), 300);
        assert_eq!(left2, 640 - 150);
        assert_eq!(center_ratio(Work { x: 0, y: 0, w: 0, h: 0 }, 0, 10), 0.5);
    }
}
