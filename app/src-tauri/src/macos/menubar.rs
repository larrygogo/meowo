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

fn glyph_offset(idx: usize) -> usize {
    (0..idx).map(|i| GLYPH_H * GLYPH_W[i]).sum()
}

/// 把一组（图标 + 多位数字）追加进拼接序列，元素为 (字形下标, 前置间隔)。
fn push_pair(out: &mut Vec<(usize, usize)>, sym: usize, n: usize) {
    out.push((sym, if out.is_empty() { 0 } else { PAIR_GAP }));
    for (i, ch) in n.to_string().bytes().enumerate() {
        out.push(((ch - b'0') as usize, if i == 0 { SYM_NUM_GAP } else { DIGIT_GAP }));
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

/// 把 (运行, 待办) 拼成菜单栏模板图的 RGBA（黑 + alpha 掩码）。全零返回 None（用空闲 logo 回落）。
fn render_status_rgba(running: usize, waiting: usize) -> Option<(Vec<u8>, u32, u32)> {
    let seq = status_seq(running, waiting);
    if seq.is_empty() {
        return None;
    }
    let total: usize = seq.iter().map(|(idx, gap)| gap + GLYPH_W[*idx]).sum();
    let mut rgba = vec![0u8; total * GLYPH_H * 4];
    let mut x = 0;
    for (idx, gap) in seq {
        x += gap;
        let off = glyph_offset(idx);
        let w = GLYPH_W[idx];
        for y in 0..GLYPH_H {
            for c in 0..w {
                // 模板图只用 alpha；RGB 留 0，macOS 按菜单栏明暗反色。
                rgba[(y * total + x + c) * 4 + 3] = GLYPH_ATLAS[off + y * w + c];
            }
        }
        x += w;
    }
    Some((rgba, total as u32, GLYPH_H as u32))
}

/// 按 (运行, 待办) 更新菜单栏图标：有计数 → 图标+数字模板图；全零 → 回落单色 logo。
/// set_icon_with_as_template 原子换图标+模板态（避免闪烁）；清掉标题文字。
pub fn update_tray_status(app: &AppHandle, running: usize, waiting: usize) {
    let Some(tray) = app.tray_by_id("cc-kanban-tray") else {
        return;
    };
    match render_status_rgba(running, waiting) {
        Some((rgba, w, h)) => {
            let _ = tray.set_icon_with_as_template(Some(tauri::image::Image::new(&rgba, w, h)), true);
        }
        None => {
            let logo = tauri::image::Image::new(MENUBAR_ICON_RGBA, MENUBAR_ICON_SIZE, MENUBAR_ICON_SIZE);
            let _ = tray.set_icon_with_as_template(Some(logo), true);
        }
    }
    let _ = tray.set_title(None::<&str>);
}

/// 创建 macOS 状态栏托盘：左键切换面板，右键弹「设置 / 退出」菜单。
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItemBuilder::with_id("settings", "设置").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&settings, &quit]).build()?;

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
    fn render_status_none_when_idle_else_sized() {
        assert!(render_status_rgba(0, 0).is_none());
        let (rgba, w, h) = render_status_rgba(3, 2).unwrap();
        assert_eq!(h, GLYPH_H as u32);
        assert_eq!(rgba.len(), (w as usize) * GLYPH_H * 4);
        // 图集长度自洽：等于各字形 H*W 之和。
        assert_eq!(GLYPH_ATLAS.len(), GLYPH_W.iter().map(|w| GLYPH_H * w).sum::<usize>());
    }
}
