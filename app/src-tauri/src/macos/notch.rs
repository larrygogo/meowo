//! macOS 版灵动岛的落点（2026-11-05 用户定案：macOS 也取消贴纸，换成灵动岛）。
//!
//! 两种落点，按「此刻有没有带刘海的屏」实时切换：
//! - 有刘海（2021 年后的 MacBook）：岛包住刘海，钉死不能拖。刘海那块是摄像头，什么也画不出来，
//!   前端把内容分到刘海左右两翼（见 Island.tsx 的 notch 形态）。
//! - 没刘海（外接屏、合盖、老机型）：屏幕上不画岛，点菜单栏图标在下方弹出面板。用户明确不要在
//!   屏幕正中模拟一个刘海。这时面板按 macOS 菜单栏弹出面板的样子画（与菜单栏留缝、四角圆、
//!   系统深色毛玻璃 + 阴影），不再是贴着菜单栏的一块黑板（2026-11-05 实拍：用户嫌「太丑」）。
//!
//! 刘海几何来自 NSScreen（macOS 12+）：safeAreaInsets.top 是刘海高度，auxiliaryTopLeftArea /
//! auxiliaryTopRightArea 是刘海左右两块菜单栏，屏宽减去两块就是刘海宽。
//! 未经本机编译（Windows 开发机），靠 CI macOS 矩阵编译、真机验证。

use std::sync::Mutex;

use objc2::encode::{Encode, Encoding};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send, sel};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Point {
    x: f64,
    y: f64,
}
// SAFETY：与 CGPoint 同布局（两个 f64，repr(C)）。
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Size {
    width: f64,
    height: f64,
}
// SAFETY：与 CGSize 同布局。
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Rect {
    origin: Point,
    size: Size,
}
// SAFETY：与 CGRect（NSRect）同布局。
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct EdgeInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
}
// SAFETY：与 NSEdgeInsets 同布局（四个 f64）。
unsafe impl Encode for EdgeInsets {
    const ENCODING: Encoding = Encoding::Struct(
        "NSEdgeInsets",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

/// 刘海在全局坐标里的位置（逻辑点，左上角原点，与 tauri 的 LogicalPosition 同一套）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Notch {
    /// 刘海左边缘。
    pub x: f64,
    /// 刘海所在屏的顶边。
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 最近一次探测结果。只能在主线程问 NSScreen，而菜单栏图标是 watch 线程在刷，
/// 那边读这份缓存。Some(None) = 探过、没刘海；None = 还没探过。
static CACHE: Mutex<Option<Option<Notch>>> = Mutex::new(None);

/// 问 NSScreen 哪块屏有刘海。必须在主线程调（同步 tauri 命令、托盘事件、setup 都在主线程）。
fn detect() -> Option<Notch> {
    // SAFETY：只读 NSScreen 的类属性与实例属性，返回的对象由 AppKit 持有，这里只借用裸指针、
    // 不接管所有权；调用方保证在主线程。safeAreaInsets 等是 macOS 12 才有的属性，先问
    // respondsToSelector，老系统直接当没刘海。
    unsafe {
        let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
        if screens.is_null() {
            return None;
        }
        let count: usize = msg_send![screens, count];
        if count == 0 {
            return None;
        }
        // Cocoa 坐标原点在主屏（screens[0]）左下角、y 向上；换成左上角原点要用主屏高度翻一下。
        let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
        let primary_frame: Rect = msg_send![primary, frame];
        for i in 0..count {
            let screen: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            let has_insets: bool = msg_send![screen, respondsToSelector: sel!(safeAreaInsets)];
            if !has_insets {
                continue;
            }
            let insets: EdgeInsets = msg_send![screen, safeAreaInsets];
            if insets.top <= 0.0 {
                continue;
            }
            let frame: Rect = msg_send![screen, frame];
            let left: Rect = msg_send![screen, auxiliaryTopLeftArea];
            let right: Rect = msg_send![screen, auxiliaryTopRightArea];
            let width = frame.size.width - left.size.width - right.size.width;
            if width <= 0.0 {
                continue;
            }
            return Some(Notch {
                x: frame.origin.x + left.size.width,
                y: primary_frame.size.height - (frame.origin.y + frame.size.height),
                width,
                height: insets.top,
            });
        }
        None
    }
}

/// 重新探测并更新缓存（主线程）。返回探测结果，以及与上次相比变没变。
pub(crate) fn refresh() -> (Option<Notch>, bool) {
    let now = detect();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let changed = *cache != Some(now);
    *cache = Some(now);
    (now, changed)
}

/// 缓存里的刘海（任意线程）。还没探过时按「没刘海」答。
pub(crate) fn cached() -> Option<Notch> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).flatten()
}

/// 此刻鼠标有没有键按着。展开的面板「点外面才收起」要用，与 Windows 的 GetAsyncKeyState 同义。
pub(crate) fn mouse_buttons_pressed() -> bool {
    // SAFETY：pressedMouseButtons 是 NSEvent 的类属性，任意线程可读，返回位掩码。
    let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
    buttons != 0
}

/// 弹出面板与菜单栏之间的缝、离屏幕左右边的最小距离（逻辑点），照系统菜单栏弹出面板。
const POPOVER_GAP: f64 = 6.0;
const POPOVER_EDGE: f64 = 8.0;
/// 弹出面板的圆角，须与 CSS `:root.island-menubar` 下面板的 border-radius 一致。
const POPOVER_RADIUS: f64 = 14.0;

/// 上次套用的窗口外观是不是弹出面板（Some(true)）还是刘海（Some(false)）。外观只在落点切换时改，
/// 每次摆位都重设毛玻璃会闪。
static STYLED_AS_POPOVER: Mutex<Option<bool>> = Mutex::new(None);

/// 按落点切换窗口外观：弹出面板垫系统深色毛玻璃（HudWindow 材质）；刘海是纯黑实心，
/// 去掉材质和阴影，否则刘海周围一圈发灰。
fn apply_style(window: &tauri::WebviewWindow, popover: bool) {
    let mut styled = STYLED_AS_POPOVER.lock().unwrap_or_else(|e| e.into_inner());
    if *styled == Some(popover) {
        return;
    }
    *styled = Some(popover);
    if popover {
        let _ = window.set_effects(tauri::utils::config::WindowEffectsConfig {
            effects: vec![tauri::utils::WindowEffect::HudWindow],
            state: Some(tauri::utils::WindowEffectState::Active),
            radius: Some(POPOVER_RADIUS),
            color: None,
            interactive: false,
        });
        // 不要系统阴影：透明窗口的阴影按显示那一刻窗口里画着的东西算，面板刚弹出、内容还没画好时
        // 阴影按旧形状算，顶上留一道黑边，过一会儿系统重算才消失（2026-11-05 Mac 实拍）。
        // 面板边缘靠 CSS 那圈细描边。
        let _ = window.set_shadow(false);
    } else {
        let _ = window.set_effects(None::<tauri::utils::config::WindowEffectsConfig>);
        let _ = window.set_shadow(false);
    }
}

/// 按落点摆窗口：有刘海就以刘海中心为中心、贴屏幕顶边；没刘海就挂在菜单栏图标正下方。
pub(crate) fn layout(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let width = width.clamp(1.0, 4000.0);
    let height = height.clamp(1.0, 4000.0);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    if let Some(notch) = cached() {
        apply_style(window, false);
        let x = notch.x + notch.width / 2.0 - width / 2.0;
        return window
            .set_position(tauri::LogicalPosition::new(x, notch.y))
            .map_err(|e| e.to_string());
    }
    apply_style(window, true);
    let scale = window.scale_factor().unwrap_or(2.0).max(0.1);
    let (x, y) = under_tray(app, window, width * scale, scale);
    window
        .set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32))
        .map_err(|e| e.to_string())
}

/// 菜单栏图标正下方、水平居中的左上角（物理像素），与菜单栏留一道缝，横向夹在图标所在屏里
/// （图标靠右时 600 宽的面板居中会伸出屏幕）。图标位置取不到（刚启动、还没点过）时退到主屏右上角。
fn under_tray(app: &tauri::AppHandle, window: &tauri::WebviewWindow, phys_w: f64, scale: f64) -> (f64, f64) {
    let gap = POPOVER_GAP * scale;
    let edge = POPOVER_EDGE * scale;
    let rect = app.tray_by_id("meowo-tray").and_then(|t| t.rect().ok().flatten());
    if let Some(rect) = rect {
        let pos = rect.position.to_physical::<f64>(scale);
        let size = rect.size.to_physical::<f64>(scale);
        if size.width > 0.0 {
            let cx = pos.x + size.width / 2.0;
            let mut x = cx - phys_w / 2.0;
            let screen = window.available_monitors().ok().and_then(|ms| {
                ms.into_iter().find(|m| {
                    let (p, s) = (m.position(), m.size());
                    cx >= f64::from(p.x) && cx < f64::from(p.x) + f64::from(s.width)
                })
            });
            if let Some(m) = screen {
                let left = f64::from(m.position().x) + edge;
                let right = f64::from(m.position().x) + f64::from(m.size().width) - edge - phys_w;
                x = x.min(right).max(left);
            }
            return (x, pos.y + size.height + gap);
        }
    }
    let monitor = window.primary_monitor().ok().flatten();
    match monitor {
        Some(m) => {
            let wa = m.work_area();
            (
                f64::from(wa.position.x) + f64::from(wa.size.width) - phys_w - edge,
                f64::from(wa.position.y) + gap,
            )
        }
        None => (0.0, 0.0),
    }
}
