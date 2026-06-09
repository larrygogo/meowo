use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::macos::panel;

/// 创建 macOS 状态栏托盘：左键切换面板，右键弹「设置 / 退出」菜单。
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItemBuilder::with_id("settings", "设置").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&settings, &quit]).build()?;

    TrayIconBuilder::with_id("cc-kanban-tray")
        .icon(app.default_window_icon().unwrap().clone())
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
