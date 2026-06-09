use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_nspanel::{
    cocoa::appkit::{NSMainMenuWindowLevel, NSWindowCollectionBehavior},
    panel_delegate, ManagerExt, WebviewWindowExt,
};
use tauri_plugin_positioner::{Position, WindowExt};

#[allow(non_upper_case_globals)]
const NS_NONACTIVATING_PANEL: i32 = 1 << 7; // NSWindowStyleMaskNonActivatingPanel

const RESIGN_EVENT: &str = "menubar_panel_did_resign_key";

/// 把已存在的 main 窗口原地转成 NonactivatingPanel，并接好失焦 -> emit 事件。
pub fn convert_main_to_panel(app: &AppHandle) {
    let window = match app.get_webview_window("main") {
        Some(w) => w,
        None => return,
    };
    let panel = match window.to_panel() {
        Ok(p) => p,
        Err(_) => return,
    };

    let delegate = panel_delegate!(CcPanelDelegate { window_did_resign_key });
    let handle = app.clone();
    delegate.set_listener(Box::new(move |name: String| {
        if name == "window_did_resign_key" {
            let _ = handle.emit(RESIGN_EVENT, ());
        }
    }));

    panel.set_level(NSMainMenuWindowLevel + 1);
    panel.set_style_mask(NS_NONACTIVATING_PANEL);
    panel.set_collection_behaviour(
        NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary,
    );
    panel.set_delegate(delegate);

    // 启动即隐藏，等托盘点击再显示。
    panel.order_out(None);
}

/// 失焦自动隐藏的监听器（在 setup 里调用一次）。
pub fn setup_resign_listener(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any(RESIGN_EVENT, move |_| {
        if let Ok(panel) = handle.get_webview_panel("main") {
            panel.order_out(None);
        }
    });
}

/// 托盘点击：可见则收起，不可见则定位到图标下方再显示。
pub fn toggle_panel(app: &AppHandle) {
    let panel = match app.get_webview_panel("main") {
        Ok(p) => p,
        Err(_) => return,
    };
    if panel.is_visible() {
        panel.order_out(None);
        return;
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.move_window(Position::TrayCenter); // 先定位
    }
    panel.show(); // 后显示
}
