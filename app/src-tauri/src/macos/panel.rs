// tauri-nspanel v2 分支基于已弃用的 cocoa crate；其常量与 panel_delegate! 宏在 -D warnings 下会触发
// deprecated 与 unexpected_cfgs(cargo-clippy) 告警。这些来自上游、我们无法消除，按需放行。
#![allow(deprecated, unexpected_cfgs)]
use tauri::{AppHandle, Emitter, Manager};
use tauri_nspanel::{
    cocoa::appkit::NSWindowCollectionBehavior,
    panel_delegate, ManagerExt, WebviewWindowExt,
};

#[allow(non_upper_case_globals)]
const NS_NONACTIVATING_PANEL: i32 = 1 << 7; // NSWindowStyleMaskNonActivatingPanel
/// NSMainMenuWindowLevel（24）+ 3：压过菜单栏，岛才能盖在刘海两侧的菜单栏上。
/// 同类刘海应用的通行取值；再高就压过系统弹出菜单了。cocoa 0.26 没导出，手写数值。
const NS_ISLAND_WINDOW_LEVEL: i32 = 27;

const RESIGN_EVENT: &str = "menubar_panel_did_resign_key";

/// 把已存在的 main 窗口原地转成 NonactivatingPanel，并接好失焦 -> emit 事件。
pub fn convert_main_to_panel(app: &AppHandle) {
    let window = match app.get_webview_window("main") {
        Some(w) => w,
        None => return,
    };

    // 灵动岛（2026-11-05 起 macOS 也取消贴纸）是纯黑实心形状，窗口透明只为露出耳朵外的桌面，
    // 不再垫贴纸那层 Popover 毛玻璃，也不要系统阴影（岛自己画）。
    let _ = window.set_shadow(false);

    let panel = match window.to_panel() {
        Ok(p) => p,
        Err(_) => return,
    };

    let delegate = panel_delegate!(CcPanelDelegate {
        window_did_resign_key
    });
    let handle = app.clone();
    delegate.set_listener(Box::new(move |name: String| {
        if name == "window_did_resign_key" {
            let _ = handle.emit(RESIGN_EVENT, ());
        }
    }));

    // 岛要包住刘海、压在菜单栏之上，层级得高过菜单栏（24）。贴纸那会儿刻意用 Floating（3）
    // 不盖菜单栏，现在反过来。尺寸全由程序给，不要 Resizable。
    panel.set_level(NS_ISLAND_WINDOW_LEVEL);
    panel.set_style_mask(NS_NONACTIVATING_PANEL);
    panel.set_collection_behaviour(
        NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
    );
    panel.set_delegate(delegate);

    // 岛的配色写死成深色，面板外观（决定毛玻璃深浅）也钉成深色，不等前端推主题。
    set_appearance(app, Some("dark"));
    // 启动先藏着：前端探明落点（有没有刘海）、摆好位置后自己调 island_set_visible 显示。
    panel.order_out(None);
}

/// 贴纸面板的 NSAppearance 跟应用主题走：Some("dark"/"light") 强制，None 跟随系统。
/// 只设在面板这一扇窗上，不走 tao 的 set_theme——那是改 NSApplication.appearance，
/// 会把托盘菜单等系统控件一起染色，也会让 window.theme() 读不到真实系统明暗
/// （window_background_color 在 theme=system 时靠它）。
/// 原生材质（Popover）与 WKWebView 的 prefers-color-scheme 都随窗口外观变，后者无害：
/// 前端主题由 data-theme 决定，不读媒体查询（system 模式下本就传 None 跟系统）。
pub fn set_appearance(app: &AppHandle, theme: Option<&str>) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(ptr) = window.ns_window() else {
        return;
    };
    let name = match theme {
        Some("dark") => Some("NSAppearanceNameDarkAqua"),
        Some("light") => Some("NSAppearanceNameAqua"),
        _ => None,
    };
    // SAFETY：ns_window 返回的是当前存活的 NSWindow（面板仍是同一对象，nspanel 只换了 class）；
    // appearanceNamed: 返回 autoreleased 对象，setAppearance: 内部自行 retain，
    // 这里只传裸指针不接管所有权。命令在主线程执行（sync command），满足 AppKit 线程要求。
    unsafe {
        let win: *mut AnyObject = ptr.cast();
        let appearance: *mut AnyObject = match name {
            Some(n) => {
                let s = NSString::from_str(n);
                msg_send![class!(NSAppearance), appearanceNamed: &*s]
            }
            None => std::ptr::null_mut(),
        };
        let _: () = msg_send![win, setAppearance: appearance];
    }
}

/// 显示 / 隐藏岛。`focus` = 显示时顺手成为 key window：展开的面板要接 Esc、失焦收起；
/// 收起的胶囊不能抢键盘（非激活面板成了 key 照样会吃掉前台应用的输入），只摆到最前。
pub fn set_visible(app: &AppHandle, visible: bool, focus: bool) {
    let Ok(panel) = app.get_webview_panel("main") else {
        return;
    };
    if !visible {
        panel.order_out(None);
    } else if focus {
        panel.show();
    } else {
        panel.order_front_regardless();
    }
}

/// 「找回」：通知点击落空等兜底入口。贴纸没了，找回就是让岛展开面板（前端 recall-sticker 监听）。
pub fn recall_panel(app: &AppHandle) {
    let _ = app.emit_to("main", "recall-sticker", ());
}
