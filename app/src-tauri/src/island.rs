//! 灵动岛（Windows 贴纸的新形态，2026-11-04 用户决定替换贴边缩略条）的窗口几何。
//!
//! 岛永远贴在所在显示器工作区的**顶边**，只能左右拖（用户选的）。三种形态（收起胶囊 /
//! 提醒 / 展开成完整列表）只是尺寸不同：前端给逻辑宽高，这里按**当前中心点**左右对称
//! 地放大缩小，所以从胶囊长成列表时不会往一边歪。
//!
//! 拖动不用系统拖拽（`start_dragging` 会连上下一起拖，还会进旧的吸边判定）：前端按下时
//! 调 begin、移动时调 update、松手时调 end，由这里读真实光标位置算横坐标。拖过显示器边界
//! 就落到那块屏的顶边。
//!
//! 岛模式下 lib.rs 的 Moved 吸边逻辑整段跳过（见 [`active`]），两套定位互不打架。

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use tauri::Manager as _;
use std::sync::Mutex;

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// 岛模式是否已接管主窗口定位。lib.rs 的 Moved 吸边据此让路。
pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

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
/// 位置），否则保持当前中心（形态切换）。首次调用即进入岛模式。
#[tauri::command]
pub(crate) fn island_layout(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
    center_ratio: Option<f64>,
) -> Result<(), String> {
    ACTIVE.store(true, Ordering::Relaxed);
    #[cfg(target_os = "windows")]
    crate::snap_preview::hide(window.app_handle());
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

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Work = Work { x: 0, y: 0, w: 1920, h: 1040 };

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
