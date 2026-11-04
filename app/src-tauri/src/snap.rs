//! 贴纸窗口几何的残留部分：尺寸常量、出屏救援（pull_on_screen）与光标命中判定。
//! 旧的贴边吸附/缩略条命令已随灵动岛替换整体移除，岛的定位见 island.rs。

/// 贴纸正常态最小逻辑尺寸。**必须与 tauri.conf.json 的 minWidth/minHeight、windowState.ts 的
/// SIZE_MIN_W/H 三处一致**。W-17 起该值全程恒定（折叠不再放开它，改禁 resizable——
/// 放开 min_size 曾让拖角把吸附态窗口缩成细条，毒化「正常尺寸」基准）。
/// 高度按「至少完整显示两张会话卡」定：窗框+拖拽区+tab+底栏合计约 132px，
/// 标准密度单卡约 84px + 6px 间距，两张约 174px；330 在宽松密度(112%)下也够。
pub(crate) const STICKER_MIN_W: f64 = 360.0;
pub(crate) const STICKER_MIN_H: f64 = 330.0;
/// 逻辑尺寸上界：settings 的贴纸几何（W-17）校验用，防 f64→i32 回绕/异常大值
/// （settings.json 被手改坏）经 set_size 设出极端窗口。与前端 windowState.ts 的 SIZE_MAX 一致。
pub(crate) const SIZE_MAX_LOGICAL: f64 = 20000.0;

/// 矩形（物理像素），用于出屏救援的纯计算。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
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
pub(crate) fn pull_on_screen(window: &tauri::WebviewWindow, force: bool) {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let win = Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    };
    let Ok(monitors) = window.available_monitors() else {
        return;
    };
    if monitors.is_empty() {
        return;
    }
    let to_work = |m: &tauri::window::Monitor| {
        let wa = m.work_area();
        Rect {
            x: wa.position.x,
            y: wa.position.y,
            w: wa.size.width as i32,
            h: wa.size.height as i32,
        }
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

/// 真实光标是否在主窗口外接矩形内。用于展开态下判定是否该收回——DOM 的 mouseleave 在
/// 窗口缩放时会误报一串假 leave/enter，不可信；改问 GetCursorPos vs 窗口物理矩形。
/// 取不到坐标/尺寸时一律当作"在内"，避免误折叠。非 Windows 暂恒为 true（不收回）。
#[tauri::command]
pub(crate) fn cursor_over_window(window: tauri::WebviewWindow) -> bool {
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
