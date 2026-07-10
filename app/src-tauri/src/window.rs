//! 应用窗口（设置 / 更新 / 新建会话）与系统托盘/菜单的创建与本地化。从 lib.rs 抽出。

use crate::settings::{load_settings, tr, ui_lang};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};
use tauri::{Emitter, Manager};
#[cfg(not(target_os = "macos"))]
use tauri::menu::{MenuBuilder, MenuItemBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

/// 前端调用：打开设置窗口（贴纸 tab 栏的设置按钮）。
/// 必须在子线程创建：同步 command 跑在主线程，直接 build() 会阻塞主线程消息泵，
/// 而 WebView2 初始化依赖消息泵运转 → 卡在初始化 → 白屏。子线程里 build() 把创建
/// dispatch 回主线程异步执行，泵不被阻塞。
#[tauri::command]
pub(crate) fn open_settings(app: tauri::AppHandle) {
    std::thread::spawn(move || open_settings_window(&app));
}

/// 打开（或聚焦）设置窗口。窗口 label 为 "about"（main.tsx 按此 label 路由到设置页）。
/// 托盘左键点击与右键菜单「设置」共用此逻辑。
pub(crate) fn open_settings_window(app: &tauri::AppHandle) {
    // macOS：打开设置窗口前临时切到 Regular 激活策略，否则纯托盘 App 的窗口无法获焦。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("about") {
        let _ = w.set_focus();
    } else {
        let builder = tauri::WebviewWindowBuilder::new(
            app,
            "about",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title(tr(ui_lang(&load_settings()), "window.settings"))
        .inner_size(620.0, 460.0)
        .min_inner_size(620.0, 460.0)
        .resizable(false)
        .decorations(false)
        .center();
        // macOS：无边框窗口不会自动圆角，故设为透明，由前端 .settings 的 border-radius 呈现圆角
        // （系统会按不透明内容自动绘制圆角阴影）。Windows 由 DWM 自动圆角，保持不透明不变。
        #[cfg(target_os = "macos")]
        let builder = builder.transparent(true);
        match builder.build() {
            Ok(_about_window) => {
                // macOS：设置窗口关闭后切回 Accessory，重新隐藏 Dock 图标。
                #[cfg(target_os = "macos")]
                {
                    let app_handle = app.clone();
                    _about_window.on_window_event(move |e| {
                        if matches!(
                            e,
                            tauri::WindowEvent::CloseRequested { .. }
                                | tauri::WindowEvent::Destroyed
                        ) {
                            crate::macos::menubar::settings_window_did_close(&app_handle);
                        }
                    });
                }
            }
            Err(e) => eprintln!("创建设置窗口失败: {e}"),
        }
    }
}

/// 前端调用：打开软件更新窗口（贴纸更新红点 / 设置页「更新到 vX」按钮）。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
pub(crate) fn open_update_window(app: tauri::AppHandle) {
    std::thread::spawn(move || open_update_window_impl(&app));
}

/// 打开（或聚焦）更新窗口。label 为 "updater"（main.tsx 按此 label 路由到更新页）。
/// 更新窗口是检查/下载/安装的唯一所有者——主窗与设置窗只负责把它打开，
/// 不再有跨窗口 trigger-update/update-failed 事件协议。
pub(crate) fn open_update_window_impl(app: &tauri::AppHandle) {
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_focus();
        return;
    }
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "updater",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(tr(ui_lang(&load_settings()), "window.updater"))
    // 紧凑初始高度（检查中/已最新/失败态）；发现新版带更新说明时由前端 setSize 增高。
    .inner_size(400.0, 252.0)
    .min_inner_size(400.0, 252.0)
    .resizable(false)
    .decorations(false)
    .center();
    // macOS：无边框窗口不自动圆角，设透明由前端 .updater 的 border-radius 呈现（同设置窗口）。
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    match builder.build() {
        Ok(_update_window) => {
            // macOS：更新窗口关闭后切回 Accessory，重新隐藏 Dock 图标（同设置窗口）。
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _update_window.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle);
                    }
                });
            }
        }
        Err(e) => eprintln!("创建更新窗口失败: {e}"),
    }
}

/// 前端调用：打开「新建会话」窗口（贴纸底栏 + 按钮 / 空状态 CTA / 会话卡片菜单）。
/// 传入 cwd/provider 时，新建面板会预填该路径并选中该模型。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
pub(crate) fn open_new_session_window(app: tauri::AppHandle, cwd: Option<String>, provider: Option<String>) {
    std::thread::spawn(move || open_new_session_window_impl(&app, cwd, provider));
}

/// 打开（或聚焦）新建会话窗口。label 为 "new-session"（main.tsx 按此 label 路由到面板页）。
pub(crate) fn open_new_session_window_impl(
    app: &tauri::AppHandle,
    cwd: Option<String>,
    provider: Option<String>,
) {
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app);

    if let Some(w) = app.get_webview_window("new-session") {
        // 窗口已开：若从另一张卡片带了 cwd/provider 预填，通知面板更新表单（不重开窗口），再聚焦。
        if cwd.is_some() || provider.is_some() {
            use tauri::Emitter;
            let _ = app.emit("ns-prefill", serde_json::json!({ "cwd": cwd, "provider": provider }));
        }
        let _ = w.set_focus();
        return;
    }
    let url = match (&cwd, &provider) {
        (None, None) => "index.html".to_string(),
        _ => {
            let mut params = Vec::new();
            if let Some(c) = &cwd {
                params.push(format!("cwd={}", percent_encode(c.as_bytes(), NON_ALPHANUMERIC)));
            }
            if let Some(p) = &provider {
                params.push(format!("provider={}", percent_encode(p.as_bytes(), NON_ALPHANUMERIC)));
            }
            format!("index.html?{}", params.join("&"))
        }
    };
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "new-session",
        tauri::WebviewUrl::App(url.into()),
    )
    .title(tr(ui_lang(&load_settings()), "window.newSession"))
    .inner_size(460.0, 420.0)
    .min_inner_size(460.0, 420.0)
    .resizable(false)
    .decorations(false)
    .center();
    // macOS：无边框窗口不自动圆角，设透明由前端 .ns-window 的 border-radius 呈现（同设置窗口）。
    #[cfg(target_os = "macos")]
    let builder = builder.transparent(true);
    match builder.build() {
        Ok(_win) => {
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _win.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle);
                    }
                });
            }
        }
        Err(e) => eprintln!("创建新建会话窗口失败: {e}"),
    }
}

/// 「找回贴纸」：把主窗口按当前尺寸居中到主显示器工作区，并显示/取消最小化/置顶/聚焦。
/// 折叠态的「展开 + 还原正常尺寸」由前端在调用本命令前完成（snap_restore），故这里只按当前尺寸居中。
#[tauri::command]
pub(crate) fn recall_center(window: tauri::WebviewWindow) -> Result<(), String> {
    let _ = window.unminimize();
    let _ = window.show();
    // 优先主显示器（找回的「家」最可预期）；取不到回退当前屏。
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let wa = m.work_area();
        let sz = window.outer_size().map_err(|e| e.to_string())?;
        let x = wa.position.x + (wa.size.width as i32 - sz.width as i32) / 2;
        let y = wa.position.y + (wa.size.height as i32 - sz.height as i32) / 2;
        window
            .set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
    }
    window.set_always_on_top(true).map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    Ok(())
}

/// 托盘「找回贴纸」：唤起主窗口并通知前端执行完整找回（展开折叠 + 居中到主屏 + 置顶）。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recall_sticker(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("recall-sticker", ());
    }
}

/// 托盘右键菜单（找回贴纸 / 设置 / 退出），按语言构建；切语言时由 rebuild_tray_menu 重建。
#[cfg(not(target_os = "macos"))]
pub(crate) fn build_tray_menu(app: &tauri::AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let recall = MenuItemBuilder::with_id("recall", tr(lang, "tray.recall")).build(app)?;
    let settings = MenuItemBuilder::with_id("settings", tr(lang, "tray.settings")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(lang, "tray.quit")).build(app)?;
    MenuBuilder::new(app).items(&[&recall, &settings, &quit]).build()
}

/// 切语言后让已存在的系统 UI 跟上：重建托盘菜单、改已开设置窗口的标题。
pub(crate) fn apply_language(app: &tauri::AppHandle, lang: &str) {
    if let Some(tray) = app.tray_by_id("meowo-tray") {
        #[cfg(not(target_os = "macos"))]
        if let Ok(menu) = build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
        #[cfg(target_os = "macos")]
        if let Ok(menu) = crate::macos::menubar::build_tray_menu(app, lang) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    if let Some(w) = app.get_webview_window("about") {
        let _ = w.set_title(tr(lang, "window.settings"));
    }
    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_title(tr(lang, "window.updater"));
    }
}

/// 构建系统托盘：左键点击直接打开设置；右键菜单提供设置 / 退出。
/// macOS 走 `macos::menubar::setup_tray`（面板模式），故此实现仅用于非 macOS 平台。
#[cfg(not(target_os = "macos"))]
pub(crate) fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = build_tray_menu(app.handle(), ui_lang(&load_settings()))?;

    let mut builder = TrayIconBuilder::with_id("meowo-tray");
    // 图标恒由打包提供，但缺失时不该 unwrap panic 把启动打挂——没图标就建无图标托盘。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .tooltip("Meowo")
        .menu(&menu)
        // 左键留给「打开设置」，菜单仅在右键弹出。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "recall" => recall_sticker(app),
            "settings" => open_settings_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 仅左键「抬起」时触发，避免按下+抬起各触发一次。
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_settings_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Windows：把待交互/运行中会话数摘要写进托盘悬浮提示，鼠标移到托盘一眼可见，
/// 弥补桌面端无菜单栏标题。计数为 0 时回落到纯品牌名。
#[cfg(target_os = "windows")]
pub(crate) fn update_tray_tooltip(app: &tauri::AppHandle, running: usize, waiting: usize, lang: &str) {
    let Some(tray) = app.tray_by_id("meowo-tray") else {
        return;
    };
    let _ = tray.set_tooltip(Some(tray_tooltip_text(lang, running, waiting)));
}

/// 构建托盘提示文案（本地化）。待交互更紧急，排在运行中之前。
#[cfg(target_os = "windows")]
pub(crate) fn tray_tooltip_text(lang: &str, running: usize, waiting: usize) -> String {
    if running == 0 && waiting == 0 {
        return "Meowo".into();
    }
    let mut parts: Vec<String> = Vec::new();
    if lang == "en" {
        if waiting > 0 {
            parts.push(format!("{waiting} waiting"));
        }
        if running > 0 {
            parts.push(format!("{running} running"));
        }
    } else {
        if waiting > 0 {
            parts.push(format!("{waiting} 个待交互"));
        }
        if running > 0 {
            parts.push(format!("{running} 个运行中"));
        }
    }
    format!("Meowo · {}", parts.join(" · "))
}

