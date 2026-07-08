use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::macos::panel;

/// 菜单栏空闲图标（全空闲时回落）：单色 36×36 条形图，黑+alpha 掩码、其余透明，照真实 app logo 比例复刻。
/// macOS 按菜单栏明暗自动反色。原始 RGBA（行优先、上到下），不走 PNG 解码以免引入 image-png feature。
const MENUBAR_ICON_RGBA: &[u8] = include_bytes!("../../icons/menubar-template.rgba");
const MENUBAR_ICON_SIZE: u32 = 36;

// ── 状态图标：运行/待办的 SF Symbol + 数字，拼成动态单色模板图 ──
// 离线（Swift）把数字 0-9 + arrow.triangle.2.circlepath(运行) + hand.raised.fill(待办)
// 渲染成定高 36 的 alpha 掩码并拼接成图集；运行时按计数切片拼接，set_icon 动态更新。
const GLYPH_H: usize = 36;
/// 各字形宽度，顺序：0-9、运行、待办。须与 menubar-glyphs.bin 的生成顺序一致。
const GLYPH_W: [usize; 12] = [15, 9, 14, 14, 15, 14, 15, 13, 15, 15, 31, 21];
const GLYPH_RUN: usize = 10;
const GLYPH_WAIT: usize = 11;
/// 定高 alpha 图集（按 GLYPH_W 顺序拼接，每字形 GLYPH_H*W 字节）。
const GLYPH_ATLAS: &[u8] = include_bytes!("../../icons/menubar-glyphs.bin");

const SYM_NUM_GAP: usize = 5; // 图标与其数字之间（留一点呼吸间隔）
const DIGIT_GAP: usize = 1; // 数字之间
const PAIR_GAP: usize = 16; // 运行组与待办组之间（两个状态间留明显间隔）

const DOT_W: usize = 34; // 圆点占位槽宽（替代原 ↻/✋ 符号槽）
const DOT_D: f32 = 32.0; // 圆点直径（接近画布满高 GLYPH_H=36，留少量上下边）
const RUN_RGB: [u8; 3] = [0x34, 0xd3, 0x99]; // 运行 绿 #34d399（同 .dot-run/.dot-active）
const WAIT_RGB: [u8; 3] = [0xfb, 0xbf, 0x24]; // 待交互 黄 #fbbf24（同 .dot-wait）

const GLYPH_PLUS: usize = 12; // 合成「+」字形（不在图集，运行时用 fg 色画）；数字超 99 显示 99+
const PLUS_W: usize = 15; // 「+」占位槽宽
const PLUS_ARM: f32 = 7.5; // 「+」臂半长（中心到端点）
const PLUS_TH: f32 = 3.0; // 「+」笔画半宽（与数字笔重接近，别太方）

/// 槽宽：运行/待交互 → 圆点槽 DOT_W；「+」→ PLUS_W；其余（数字 0-9）→ 图集字形宽。
fn slot_w(idx: usize) -> usize {
    if idx == GLYPH_RUN || idx == GLYPH_WAIT {
        DOT_W
    } else if idx == GLYPH_PLUS {
        PLUS_W
    } else {
        GLYPH_W[idx]
    }
}

/// 在 rgba 的 [x0, x0+PLUS_W) 槽内画一个竖向居中的「+」，颜色为前景色 fg。
fn blit_plus(rgba: &mut [u8], total: usize, x0: usize, fg: u8) {
    let cx = x0 as f32 + PLUS_W as f32 / 2.0;
    let cy = GLYPH_H as f32 / 2.0;
    for y in 0..GLYPH_H {
        for c in 0..PLUS_W {
            let px = x0 + c;
            let dx = (px as f32 + 0.5 - cx).abs();
            let dy = (y as f32 + 0.5 - cy).abs();
            // 横臂与竖臂各自 0.5px 软边抗锯齿，取覆盖度较大者。
            let cov_h = (PLUS_ARM + 0.5 - dx).clamp(0.0, 1.0) * (PLUS_TH + 0.5 - dy).clamp(0.0, 1.0);
            let cov_v = (PLUS_TH + 0.5 - dx).clamp(0.0, 1.0) * (PLUS_ARM + 0.5 - dy).clamp(0.0, 1.0);
            let a = cov_h.max(cov_v);
            if a > 0.0 {
                let i = (y * total + px) * 4;
                rgba[i] = fg;
                rgba[i + 1] = fg;
                rgba[i + 2] = fg;
                rgba[i + 3] = (a * 255.0) as u8;
            }
        }
    }
}

/// 在 rgba 的 [x0, x0+DOT_W) 槽内画一个竖向居中的实心抗锯齿圆，颜色 rgb。
fn blit_dot(rgba: &mut [u8], total: usize, x0: usize, rgb: [u8; 3]) {
    let cx = x0 as f32 + DOT_W as f32 / 2.0;
    let cy = GLYPH_H as f32 / 2.0;
    let r = DOT_D / 2.0;
    for y in 0..GLYPH_H {
        for c in 0..DOT_W {
            let px = x0 + c;
            let dx = px as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            // 边缘 1px 抗锯齿：dist≤r-0.5 全实，≥r+0.5 全透，其间线性。
            let a = (r + 0.5 - dist).clamp(0.0, 1.0);
            if a > 0.0 {
                let i = (y * total + px) * 4;
                rgba[i] = rgb[0];
                rgba[i + 1] = rgb[1];
                rgba[i + 2] = rgb[2];
                rgba[i + 3] = (a * 255.0) as u8;
            }
        }
    }
}

fn glyph_offset(idx: usize) -> usize {
    (0..idx).map(|i| GLYPH_H * GLYPH_W[i]).sum()
}

/// 把一组（图标 + 多位数字）追加进拼接序列，元素为 (字形下标, 前置间隔)。
fn push_pair(out: &mut Vec<(usize, usize)>, sym: usize, n: usize) {
    out.push((sym, if out.is_empty() { 0 } else { PAIR_GAP }));
    // 数字最多显示两位；超过 99 显示 99+（末尾追加合成「+」字形）。
    for (i, ch) in n.min(99).to_string().bytes().enumerate() {
        out.push(((ch - b'0') as usize, if i == 0 { SYM_NUM_GAP } else { DIGIT_GAP }));
    }
    if n > 99 {
        out.push((GLYPH_PLUS, DIGIT_GAP));
    }
}

/// (运行, 待办) → 字形拼接序列（运行组在前）；全零返回空 = 回落 app logo。纯函数，便于单测。
fn status_seq(running: usize, waiting: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if running > 0 {
        push_pair(&mut out, GLYPH_RUN, running);
    }
    if waiting > 0 {
        push_pair(&mut out, GLYPH_WAIT, waiting);
    }
    out
}

/// 把 (运行, 待办) 拼成菜单栏**彩色** RGBA：运行/待交互各一个彩色圆点 + 数字。
/// 数字按菜单栏明暗着色（dark → 白，否则黑）；圆点颜色固定。全零返回 None（回落 logo）。
fn render_status_rgba(running: usize, waiting: usize, dark: bool) -> Option<(Vec<u8>, u32, u32)> {
    let seq = status_seq(running, waiting);
    if seq.is_empty() {
        return None;
    }
    let total: usize = seq.iter().map(|(idx, gap)| gap + slot_w(*idx)).sum();
    let mut rgba = vec![0u8; total * GLYPH_H * 4];
    let fg: u8 = if dark { 255 } else { 0 };
    let mut x = 0;
    for (idx, gap) in seq {
        x += gap;
        if idx == GLYPH_RUN || idx == GLYPH_WAIT {
            let rgb = if idx == GLYPH_RUN { RUN_RGB } else { WAIT_RGB };
            blit_dot(&mut rgba, total, x, rgb);
        } else if idx == GLYPH_PLUS {
            blit_plus(&mut rgba, total, x, fg);
        } else {
            let off = glyph_offset(idx);
            let w = GLYPH_W[idx];
            for y in 0..GLYPH_H {
                for c in 0..w {
                    let a = GLYPH_ATLAS[off + y * w + c];
                    if a > 0 {
                        let i = (y * total + x + c) * 4;
                        rgba[i] = fg;
                        rgba[i + 1] = fg;
                        rgba[i + 2] = fg;
                        rgba[i + 3] = a;
                    }
                }
            }
        }
        x += slot_w(idx);
    }
    Some((rgba, total as u32, GLYPH_H as u32))
}

/// 按 (运行, 待办) 更新菜单栏图标：有计数 → 彩色点+数字（非模板）；全零 → 回落单色 logo（模板）。
/// set_icon_with_as_template 原子换图标+模板态（避免闪烁）；清掉标题文字。
pub fn update_tray_status(app: &AppHandle, running: usize, waiting: usize, dark: bool) {
    let Some(tray) = app.tray_by_id("cc-kanban-tray") else {
        return;
    };
    match render_status_rgba(running, waiting, dark) {
        Some((rgba, w, h)) => {
            let _ = tray.set_icon_with_as_template(Some(tauri::image::Image::new(&rgba, w, h)), false);
        }
        None => {
            let logo = tauri::image::Image::new(MENUBAR_ICON_RGBA, MENUBAR_ICON_SIZE, MENUBAR_ICON_SIZE);
            let _ = tray.set_icon_with_as_template(Some(logo), true);
        }
    }
    let _ = tray.set_title(None::<&str>);
}

/// 读系统菜单栏明暗：Dark 模式 → true。用 `defaults`，无需引入 objc 依赖；
/// 读失败/未设置（即浅色）→ false。在既有 5s 托盘循环里调用，主题切换 ≤5s 生效。
pub fn system_is_dark() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "Dark")
        .unwrap_or(false)
}

/// 托盘右键菜单（设置 / 退出），按语言构建；切语言时由 lib.rs 的 apply_language 重建。
pub fn build_tray_menu(app: &AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let settings = MenuItemBuilder::with_id("settings", crate::tr(lang, "tray.settings")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", crate::tr(lang, "tray.quit")).build(app)?;
    MenuBuilder::new(app).items(&[&settings, &quit]).build()
}

/// 创建 macOS 状态栏托盘：左键切换面板，右键弹「设置 / 退出」菜单。
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app, crate::ui_lang(&crate::load_settings()))?;

    // 菜单栏用单色模板图标（彩色 app 图标在菜单栏里偏花，且不随明暗反色）；Dock/设置页仍用彩色图标。
    let icon = tauri::image::Image::new(MENUBAR_ICON_RGBA, MENUBAR_ICON_SIZE, MENUBAR_ICON_SIZE);
    TrayIconBuilder::with_id("cc-kanban-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("cc-kanban")
        .menu(&menu)
        .show_menu_on_left_click(false) // 左键不弹菜单 => 留给右键
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => crate::open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            // positioner 需要每次托盘事件记录图标坐标
            tauri_plugin_positioner::on_tray_event(app, &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                panel::toggle_panel(app);
            }
        })
        .build(app)?;
    Ok(())
}

/// 打开设置窗口前临时切到 Regular 以便获焦；关闭时切回 Accessory（挂在窗口事件里）。
pub fn settings_window_will_open(app: &AppHandle) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
}

pub fn settings_window_did_close(app: &AppHandle) {
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_seq_groups_and_gaps() {
        // 全空闲 → 空（回落 logo）
        assert_eq!(status_seq(0, 0), Vec::<(usize, usize)>::new());
        // 运行组在前、待办组在后；组间 PAIR_GAP，图标→数字 SYM_NUM_GAP。
        assert_eq!(
            status_seq(3, 2),
            vec![(GLYPH_RUN, 0), (3, SYM_NUM_GAP), (GLYPH_WAIT, PAIR_GAP), (2, SYM_NUM_GAP)]
        );
        // 多位数：首位 SYM_NUM_GAP，后续 DIGIT_GAP。
        assert_eq!(status_seq(12, 0), vec![(GLYPH_RUN, 0), (1, SYM_NUM_GAP), (2, DIGIT_GAP)]);
        // 仅待办：待办组打头，前置间隔为 0。
        assert_eq!(status_seq(0, 5), vec![(GLYPH_WAIT, 0), (5, SYM_NUM_GAP)]);
    }

    #[test]
    fn status_seq_caps_at_99_plus() {
        // 恰好 99：两位数，无「+」。
        assert_eq!(
            status_seq(99, 0),
            vec![(GLYPH_RUN, 0), (9, SYM_NUM_GAP), (9, DIGIT_GAP)]
        );
        // 超过 99：显示 99 + 合成「+」。
        assert_eq!(
            status_seq(150, 0),
            vec![(GLYPH_RUN, 0), (9, SYM_NUM_GAP), (9, DIGIT_GAP), (GLYPH_PLUS, DIGIT_GAP)]
        );
        // 待交互超 99 同样封顶。
        assert_eq!(
            status_seq(0, 100),
            vec![(GLYPH_WAIT, 0), (9, SYM_NUM_GAP), (9, DIGIT_GAP), (GLYPH_PLUS, DIGIT_GAP)]
        );
    }

    #[test]
    fn render_status_none_when_idle_else_sized() {
        assert!(render_status_rgba(0, 0, true).is_none());
        let (rgba, w, h) = render_status_rgba(3, 2, true).unwrap();
        assert_eq!(h, GLYPH_H as u32);
        assert_eq!(rgba.len(), (w as usize) * GLYPH_H * 4);
        // 图集长度自洽：等于各字形 H*W 之和。
        assert_eq!(GLYPH_ATLAS.len(), GLYPH_W.iter().map(|w| GLYPH_H * w).sum::<usize>());
    }

    // 扫描：是否存在一个「不透明且 RGB≈target」的像素（容差 8）。
    fn has_opaque_rgb(rgba: &[u8], target: [u8; 3]) -> bool {
        rgba.chunks_exact(4).any(|p| {
            p[3] > 200
                && (p[0] as i32 - target[0] as i32).abs() <= 8
                && (p[1] as i32 - target[1] as i32).abs() <= 8
                && (p[2] as i32 - target[2] as i32).abs() <= 8
        })
    }

    #[test]
    fn render_colors_dot_and_adapts_digits_to_appearance() {
        // 运行 2：应含橙色圆点像素；暗栏数字偏白。
        let (dark_rgba, _, _) = render_status_rgba(2, 0, true).unwrap();
        assert!(has_opaque_rgb(&dark_rgba, RUN_RGB), "缺橙色圆点");
        assert!(has_opaque_rgb(&dark_rgba, [255, 255, 255]), "暗栏数字应为白");
        // 亮栏数字偏黑。
        let (light_rgba, _, _) = render_status_rgba(2, 0, false).unwrap();
        assert!(has_opaque_rgb(&light_rgba, [0, 0, 0]), "亮栏数字应为黑");
        // 待交互 3：应含黄色圆点像素。
        let (wait_rgba, _, _) = render_status_rgba(0, 3, true).unwrap();
        assert!(has_opaque_rgb(&wait_rgba, WAIT_RGB), "缺黄色圆点");
    }

    #[test]
    fn system_is_dark_returns_without_panicking() {
        // 仅验证不 panic、返回一个布尔（无 defaults/未设置时应回落 false，不报错）。
        let _ = system_is_dark();
    }
}
