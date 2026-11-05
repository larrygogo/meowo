//! 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条）的窗口几何。
//!
//! 岛永远贴在所在显示器工作区的**顶边**，只能左右拖（用户选的）。三种形态（收起胶囊 /
//! 提醒 / 展开成完整列表）只是尺寸不同：前端给逻辑宽高，这里按**当前中心点**左右对称
//! 地放大缩小，所以从胶囊长成列表时不会往一边歪。
//!
//! 拖动不用系统拖拽（`start_dragging` 会连上下一起拖）：前端按下时
//! 调 begin、移动时调 update、松手时调 end，由这里读真实光标位置算横坐标。拖过显示器边界
//! 就落到那块屏的顶边。

use std::sync::Mutex;

/// 拖动起点：(光标物理 x, 窗口左上角物理 x)。
static DRAG: Mutex<Option<(f64, i32)>> = Mutex::new(None);

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
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    center_ratio: Option<f64>,
) -> Result<(), String> {
    // macOS：落点只有刘海或菜单栏图标下方，不记横向位置；窗口层级由 panel.rs 定（高过菜单栏），
    // 这里不能再 set_always_on_top，那会把层级打回 Floating、钻到菜单栏底下。
    #[cfg(target_os = "macos")]
    {
        let _ = center_ratio;
        crate::macos::notch::layout(&app, &window, width, height)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &app;
        layout_floating(window, width, height, center_ratio)
    }
}

/// Windows：贴所在显示器工作区的顶边，按中心点对称改尺寸。
#[cfg(not(target_os = "macos"))]
fn layout_floating(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    center_ratio: Option<f64>,
) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
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
    window.set_resizable(false).map_err(|e| e.to_string())?;
    window.set_min_size(None::<tauri::LogicalSize<f64>>).map_err(|e| e.to_string())?;
    window
        .set_size(tauri::PhysicalSize::new(phys_w.min(work.w) as u32, phys_h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    if !window.is_always_on_top().map_err(|e| e.to_string())? {
        window.set_always_on_top(true).map_err(|e| e.to_string())?;
    }
    Ok(())
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

/// 岛落在哪：Windows 恒为 floating（顶边，可左右拖）；macOS 有刘海为 notch（钉在刘海上），
/// 没刘海为 menubar（屏幕上不画，菜单栏图标是迷你岛）。前端几秒问一次，插拔外接屏、合盖后跟着换。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IslandHost {
    kind: &'static str,
    /// 刘海宽高（逻辑点），只有 notch 时非零。
    notch_width: f64,
    notch_height: f64,
}

#[tauri::command]
pub(crate) fn island_host(app: tauri::AppHandle) -> IslandHost {
    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        let (notch, _) = crate::macos::notch::refresh();
        match notch {
            Some(n) => IslandHost { kind: "notch", notch_width: n.width, notch_height: n.height },
            None => IslandHost { kind: "menubar", notch_width: 0.0, notch_height: 0.0 },
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        IslandHost { kind: "floating", notch_width: 0.0, notch_height: 0.0 }
    }
}

/// 菜单栏图标的状态（macOS）：tone 是眼睛的神态（Island.tsx 的 islandTone），segments 是图标右边
/// 富文本的一段段（文字 / SF Symbol + 颜色，见 macos::status_title）。Windows 托盘不画这些，空操作。
#[tauri::command]
pub(crate) fn set_menubar_status(app: tauri::AppHandle, tone: String, segments: serde_json::Value) {
    #[cfg(target_os = "macos")]
    {
        let segments: Vec<crate::macos::status_title::Segment> =
            serde_json::from_value(segments).unwrap_or_default();
        crate::macos::menubar::set_status(&app, &tone, &segments);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, tone, segments);
}

/// 显示 / 隐藏岛（macOS 菜单栏落点：收起时藏起来，展开、提醒时再显示）。`focus` 见
/// macos::panel::set_visible。Windows 岛恒可见，用不上，按普通窗口显隐处理。
#[tauri::command]
pub(crate) fn island_set_visible(app: tauri::AppHandle, window: tauri::WebviewWindow, visible: bool, focus: bool) {
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        crate::macos::panel::set_visible(&app, visible, focus);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, focus);
        let _ = if visible { window.show() } else { window.hide() };
    }
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

/// 松手：返回中心点在所在屏工作区里的横向比例，前端拿去记住位置。
#[tauri::command]
pub(crate) fn island_drag_end(window: tauri::WebviewWindow) -> Result<Option<f64>, String> {
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
    Ok(Some(center_ratio(work_of(&monitor), pos.x, size.width as i32)))
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

/// Windows 自己判断「有全屏程序 / 演示模式」的那套状态（系统据此暂停通知横幅）。
#[cfg(target_os = "windows")]
fn fullscreen_app_running() -> bool {
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUERY_USER_NOTIFICATION_STATE, QUNS_APP, QUNS_BUSY,
        QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    let mut state: QUERY_USER_NOTIFICATION_STATE = 0;
    // SAFETY: 传入有效的输出指针；失败时 state 不可信，按「不是全屏」处理。
    let hr = unsafe { SHQueryUserNotificationState(&mut state) };
    // QUNS_APP：应用商店类应用全屏运行（本机实测正常时返回 5 = 可接收通知）。
    hr >= 0
        && matches!(
            state,
            QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_APP
        )
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
                match fullscreen_action(fullscreen_app_running(), hidden_by_us, visible) {
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
