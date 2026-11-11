//! 应用窗口（设置 / 更新 / 新建会话）与系统托盘/菜单的创建与本地化。从 lib.rs 抽出。

use crate::settings::{load_settings, tr, ui_lang};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};
use tauri::Manager;
// Emitter：仅非 macOS 的 recall_sticker 用 w.emit("recall-sticker")；open_new_session 的 ns-prefill
// emit 在其块内另有本地 use（macOS 也走那条），故模块级 Emitter 在 macOS 上无用、按平台门控。
#[cfg(not(target_os = "macos"))]
use tauri::menu::{MenuBuilder, MenuItemBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(not(target_os = "macos"))]
use tauri::Emitter;

/// 前端调用：打开设置窗口（贴纸 tab 栏的设置按钮）。
/// 必须在子线程创建：同步 command 跑在主线程，直接 build() 会阻塞主线程消息泵，
/// 而 WebView2 初始化依赖消息泵运转 → 卡在初始化 → 白屏。子线程里 build() 把创建
/// dispatch 回主线程异步执行，泵不被阻塞。
#[tauri::command]
pub(crate) fn open_settings(app: tauri::AppHandle) {
    std::thread::spawn(move || open_settings_window(&app));
}

/// `visible:false` 创建的窗口的兜底显示。窗口创建即可见的话，WebView 画出首帧前是
/// 默认白底——打开瞬间闪一下白框；故所有窗口（贴纸/设置/对话/更新/引导/新建会话）都
/// 隐藏创建，由前端首帧后自行 show（useShowWhenReady）。但前端脚本没起来
/// （加载失败/崩溃）时窗口会永久隐身、用户以为点了没反应——到点仍不可见就强制显示，
/// 宁可白屏也要可见可关。`focus` 对贴纸主窗口传 false（窗口配置 focus:false，
/// 开机自启不得抢焦点），其余弹出窗口传 true。
pub(crate) fn show_after_grace(window: &tauri::WebviewWindow, focus: bool) {
    let w = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2000));
        if !w.is_visible().unwrap_or(true) {
            if focus {
                let _ = w.show();
                let _ = w.set_focus();
            } else {
                // focus:false 的窗口（贴纸主窗口）显示也不能激活——普通 show 在
                // Windows 上会抢焦点（W-7），走 SW_SHOWNOACTIVATE 路径。
                show_window_no_activate(&w.as_ref().window());
            }
        }
    });
}

/// Win11 的 DWM 只给带标题栏或 thick frame 的窗口自动圆角；`decorations(false)` +
/// `resizable(false)` 的小窗（设置/引导/更新/新建会话/确认）两者皆无，默认直角——必须
/// 显式声明圆角偏好。对话窗 resizable(true) 带 thick frame 故天然圆角；贴纸主窗
/// transparent + 前端 CSS 圆角，都不需要这里。Win10 不认这个属性（返回错误码），
/// 静默忽略：那代系统全窗直角是系统规范，不强造。macOS 无此概念（transparent +
/// 前端圆角），空实现。
pub(crate) fn round_window_corners(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        };
        if let Ok(handle) = window.hwnd() {
            let preference = DWMWCP_ROUND;
            unsafe {
                DwmSetWindowAttribute(
                    handle.0 as _,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    std::ptr::addr_of!(preference).cast(),
                    std::mem::size_of_val(&preference) as u32,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

/// 生产构建整族关掉 WebView2 的「浏览器加速键」（Ctrl+P 打印 / Ctrl+R·F5 刷新 /
/// Ctrl+S 另存 / Ctrl+O 打开文件 / F7 光标浏览 / Ctrl+F 原生查找条等）：这些默认行为
/// 在桌面应用里全是缺陷,前端 devtools-guard 只能逐键追。编辑键（Ctrl+C/V/X/A/Z）
/// 不属于此列,不受影响;按键事件照常到达页面,应用内的 Ctrl+F 搜索接管不变。
/// tauri 2.11 没透传 wry 的 browser_accelerator_keys,故建成后经 with_webview 自己
/// 调 WebView2 设置。dev 构建放行——F12/Ctrl+R 是日常调试路径,与 devtools-guard 同界。
/// 每次页面加载都会被调一遍（on_page_load 挂载）,重复设置无害。
/// cfg! 而非 #[cfg] 做 debug 判定:release 专属代码 dev 也要过编译(同 single-instance)。
pub(crate) fn disable_browser_accelerators(webview: &tauri::Webview) {
    if cfg!(debug_assertions) {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        let _ = webview.with_webview(|platform| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
            use windows::core::Interface;
            let apply = || -> windows::core::Result<()> {
                // SAFETY: with_webview 保证闭包在主线程执行,controller 是活的 COM 指针。
                unsafe {
                    platform
                        .controller()
                        .CoreWebView2()?
                        .Settings()?
                        .cast::<ICoreWebView2Settings3>()?
                        .SetAreBrowserAcceleratorKeysEnabled(false)
                }
            };
            if let Err(error) = apply() {
                eprintln!("关闭 WebView2 浏览器加速键失败: {error}");
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    let _ = webview;
}

/// 独立窗口原生底色的两档取值，必须与 styles.css 的 `--cc-window-bg` 逐位相同
/// （深 #111210 / 浅 #f3f4ef）；本文件末尾的 window_bg_tests 把两边钉在一起。
const WINDOW_BG_DARK: (u8, u8, u8) = (0x11, 0x12, 0x10);
const WINDOW_BG_LIGHT: (u8, u8, u8) = (0xf3, 0xf4, 0xef);

/// 应用主题此刻是不是深色。theme=system 时借主窗口的 theme()（跟随 OS，本应用不调
/// set_theme）判定明暗。原生底色与窗口材质（material.rs）共用这一处判定。
pub(crate) fn theme_is_dark(app: &tauri::AppHandle, theme: &str) -> bool {
    match theme {
        "light" => false,
        "dark" => true,
        _ => app
            .get_webview_window("main")
            .and_then(|w| w.theme().ok())
            .map(|t| t == tauri::Theme::Dark)
            // 拿不到就按深色：应用默认主题即深色，闪浅不如闪深。
            .unwrap_or(true),
    }
}

/// 独立窗口的**原生**底色，与 styles.css 的 --cc-window-bg 对齐（见 WINDOW_BG_DARK / LIGHT）。
///
/// 隐藏创建 + 首帧后 show 只消除了「窗口先于首帧显示」的那类白框；show 的瞬间 WebView2
/// 的合成帧仍可能没上屏（rAF 提交 ≠ 已呈现），这一两帧露出的是原生窗口底色——默认白，
/// 于是还是闪。把原生底色设成与页面主题同色，即便露出来也是同色纯色帧，肉眼不可辨。
/// 全平台的独立窗口都用：macOS 的 Overlay 原生标题栏窗口是不透明的，同样要防闪白。
/// 两类窗口不经此函数：贴纸主窗（transparent）与系统材质生效的窗口（transparent + 窗口
/// 效果，露出来的是材质本身；分流在 material::dress）。
pub(crate) fn window_background_color(app: &tauri::AppHandle) -> tauri::webview::Color {
    let (r, g, b) = if theme_is_dark(app, &load_settings().theme) {
        WINDOW_BG_DARK
    } else {
        WINDOW_BG_LIGHT
    };
    tauri::webview::Color(r, g, b, 0xff)
}

/// 打开（或聚焦）设置窗口。窗口 label 为 "about"（main.tsx 按此 label 路由到设置页）。
/// 托盘左键点击与右键菜单「设置」共用此逻辑。
pub(crate) fn open_settings_window(app: &tauri::AppHandle) {
    // macOS：打开设置窗口前临时切到 Regular 激活策略，否则纯托盘 App 的窗口无法获焦。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app, "about");

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
        // 只放开纵向缩放（S-2）：620 宽按设置页双列布局定死；归档列表等长内容需要更多
        // 纵向空间时，不再被困在 460 高的嵌套滚动里。max 高度给个远超屏幕的值，实际由
        // 系统钳到工作区高度。
        .max_inner_size(620.0, 100_000.0)
        .resizable(true)
        // 隐藏创建，前端首帧后自行显示（见 show_after_grace 注释），消除白框闪烁。
        .visible(false)
        .center();
        // 非 macOS：无边框 + 前端自绘标题栏（Windows 圆角由 DWM 处理，见 round_window_corners）。
        // 窗口底交给 material::dress：Win11 上是 Mica（透明窗口 + DWM 背板），其余回退成
        // 对齐主题的不透明原生底色，show 瞬间合成帧未上屏也不露白（见 window_background_color）。
        #[cfg(not(target_os = "macos"))]
        let builder =
            crate::material::dress(builder.decorations(false), app, "about");
        // macOS：原生标题栏 Overlay——系统红绿灯在左上，符合平台惯例（前端按平台隐藏
        // 自绘 ✕ 并给红绿灯留位）；hidden_title 不画原生标题文字，标题仍由前端自绘。
        // 原生窗口自带圆角与阴影，不需要 CSS 圆角。窗口底同样交给 material::dress：
        // 设置窗与对话窗垫系统材质（26+ Liquid Glass，旧版 Sidebar 材质），其余小窗不透明。
        #[cfg(target_os = "macos")]
        let builder = crate::material::dress(
            builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true),
            app,
            "about",
        );
        match builder.build() {
            Ok(_about_window) => {
                round_window_corners(&_about_window);
                show_after_grace(&_about_window, true);
                // macOS：设置窗口关闭后归还激活策略计数，最后一个窗口关闭才切回 Accessory（重新隐藏 Dock 图标）。
                #[cfg(target_os = "macos")]
                {
                    let app_handle = app.clone();
                    _about_window.on_window_event(move |e| {
                        if matches!(
                            e,
                            tauri::WindowEvent::CloseRequested { .. }
                                | tauri::WindowEvent::Destroyed
                        ) {
                            crate::macos::menubar::settings_window_did_close(&app_handle, "about");
                        }
                    });
                }
            }
            Err(e) => {
                eprintln!("创建设置窗口失败: {e}");
                // 建窗失败也要还激活策略的计数:will_open 已经把 label 记进集合,不还就
                // 永远 is_empty()==false,之后关掉所有窗口也切不回 Accessory,Dock 图标
                // 从此常驻(纯托盘应用的明显退化)。下同,五个失败分支各还一次。
                #[cfg(target_os = "macos")]
                crate::macos::menubar::settings_window_did_close(app, "about");
            }
        }
    }
}

/// 前端调用：打开使用引导窗口（首次启动自动弹 / 托盘「使用引导」/ 设置页「使用引导」按钮）。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
pub(crate) fn open_onboarding(app: tauri::AppHandle) {
    std::thread::spawn(move || open_onboarding_window(&app));
}

/// 打开（或聚焦）引导窗口。label 为 "onboarding"（main.tsx 按此 label 路由到引导页）。
pub(crate) fn open_onboarding_window(app: &tauri::AppHandle) {
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app, "onboarding");

    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.set_focus();
        return;
    }
    let builder = tauri::WebviewWindowBuilder::new(
        app,
        "onboarding",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title(tr(ui_lang(&load_settings()), "window.onboarding"))
    .inner_size(460.0, 600.0)
    .min_inner_size(460.0, 600.0)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    // 隐藏创建，前端首帧后自行显示（见 show_after_grace 注释），消除白框闪烁。
    .visible(false)
    .center();
    // 平台分支的理由见 open_settings_window 的同款注释。
    #[cfg(not(target_os = "macos"))]
    let builder = crate::material::dress(builder.decorations(false), app, "onboarding");
    #[cfg(target_os = "macos")]
    let builder = crate::material::dress(
        builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true),
        app,
        "onboarding",
    );
    match builder.build() {
        Ok(_onboarding_window) => {
            round_window_corners(&_onboarding_window);
            show_after_grace(&_onboarding_window, true);
            // macOS：引导窗口关闭后归还激活策略计数，最后一个窗口关闭才切回 Accessory（同设置/更新窗口）。
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _onboarding_window.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle, "onboarding");
                    }
                });
            }
        }
        Err(e) => {
            eprintln!("创建引导窗口失败: {e}");
            #[cfg(target_os = "macos")]
            crate::macos::menubar::settings_window_did_close(app, "onboarding");
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
    crate::macos::menubar::settings_window_will_open(app, "updater");

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
    // 隐藏创建，前端首帧后自行显示（见 show_after_grace 注释），消除白框闪烁。
    .visible(false)
    .center();
    // 平台分支的理由见 open_settings_window 的同款注释。
    #[cfg(not(target_os = "macos"))]
    let builder = crate::material::dress(builder.decorations(false), app, "updater");
    #[cfg(target_os = "macos")]
    let builder = crate::material::dress(
        builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true),
        app,
        "updater",
    );
    match builder.build() {
        Ok(_update_window) => {
            round_window_corners(&_update_window);
            show_after_grace(&_update_window, true);
            // macOS：更新窗口关闭后归还激活策略计数，最后一个窗口关闭才切回 Accessory（同设置窗口）。
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _update_window.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(&app_handle, "updater");
                    }
                });
            }
        }
        Err(e) => {
            eprintln!("创建更新窗口失败: {e}");
            #[cfg(target_os = "macos")]
            crate::macos::menubar::settings_window_did_close(app, "updater");
        }
    }
}

/// 前端调用：新建会话（贴纸底栏 + 按钮 / 空状态 CTA / 会话卡片菜单）。
/// 传入 cwd/provider 时，新建表单会预填该路径并选中该模型。
/// 对话功能开着时不再弹独立小窗：打开对话窗、主区切到新建会话页（对照 Kimi，用户要求
/// 「新建会话的时候不要再弹新的窗口」）；关掉对话功能（轻量模式）时没有对话窗可去，仍走小窗。
/// 与 open_settings 同理由走子线程创建：同步 command 在主线程 build 会阻塞消息泵致白屏。
#[tauri::command]
pub(crate) fn open_new_session_window(
    app: tauri::AppHandle,
    cwd: Option<String>,
    provider: Option<String>,
) {
    std::thread::spawn(move || open_new_session_window_impl(&app, cwd, provider));
}

/// 前端调用（设置页「会话 · 已完成会话」）：打开对话窗的会话管理页、预选「已完成」。
/// 与 open_new_session_window 同理由走子线程。对话功能关闭（轻量模式）时 open_chat_window_target
/// 自己回落设置窗——设置页里点这个入口就是原地不动，入口本身由前端在轻量模式下隐藏。
#[tauri::command]
pub(crate) fn open_session_admin(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        if let Err(error) = open_chat_window_target(&app, ChatTarget::AdminDone, true) {
            eprintln!("打开会话管理失败: {error}");
        }
    });
}

/// 打开（或聚焦）新建会话窗口。label 为 "new-session"（main.tsx 按此 label 路由到面板页）。
pub(crate) fn open_new_session_window_impl(
    app: &tauri::AppHandle,
    cwd: Option<String>,
    provider: Option<String>,
) {
    if crate::settings::load_settings().chat_enabled {
        let target = ChatTarget::NewSession { cwd, provider };
        if let Err(error) = open_chat_window_target(app, target, true) {
            eprintln!("打开新建会话页失败: {error}");
        }
        return;
    }
    // macOS：纯托盘 App 的窗口需临时切 Regular 激活策略才能获焦（同设置窗口）。
    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app, "new-session");

    if let Some(w) = app.get_webview_window("new-session") {
        // 窗口已开：若从另一张卡片带了 cwd/provider 预填，通知面板更新表单（不重开窗口），再聚焦。
        if cwd.is_some() || provider.is_some() {
            use tauri::Emitter;
            let _ = app.emit(
                "ns-prefill",
                serde_json::json!({ "cwd": cwd, "provider": provider }),
            );
        }
        let _ = w.set_focus();
        return;
    }
    let url = match (&cwd, &provider) {
        (None, None) => "index.html".to_string(),
        _ => {
            let mut params = Vec::new();
            if let Some(c) = &cwd {
                params.push(format!(
                    "cwd={}",
                    percent_encode(c.as_bytes(), NON_ALPHANUMERIC)
                ));
            }
            if let Some(p) = &provider {
                params.push(format!(
                    "provider={}",
                    percent_encode(p.as_bytes(), NON_ALPHANUMERIC)
                ));
            }
            format!("index.html?{}", params.join("&"))
        }
    };
    let builder =
        tauri::WebviewWindowBuilder::new(app, "new-session", tauri::WebviewUrl::App(url.into()))
            .title(tr(ui_lang(&load_settings()), "window.newSession"))
            .inner_size(460.0, 520.0)
            .min_inner_size(460.0, 520.0)
            .resizable(false)
            // 隐藏创建，前端首帧后自行显示（见 show_after_grace 注释），消除白框闪烁。
            .visible(false)
            .center();
    // 平台分支的理由见 open_settings_window 的同款注释。
    #[cfg(not(target_os = "macos"))]
    let builder = crate::material::dress(builder.decorations(false), app, "new-session");
    #[cfg(target_os = "macos")]
    let builder = crate::material::dress(
        builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true),
        app,
        "new-session",
    );
    match builder.build() {
        Ok(_win) => {
            round_window_corners(&_win);
            show_after_grace(&_win, true);
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.clone();
                _win.on_window_event(move |e| {
                    if matches!(
                        e,
                        tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
                    ) {
                        crate::macos::menubar::settings_window_did_close(
                            &app_handle,
                            "new-session",
                        );
                    }
                });
            }
        }
        Err(e) => {
            eprintln!("创建新建会话窗口失败: {e}");
            #[cfg(target_os = "macos")]
            crate::macos::menubar::settings_window_did_close(app, "new-session");
        }
    }
}

/// macOS：给 Overlay 标题栏窗口挂一个空 NSToolbar（unified 风格），系统据此把红绿灯
/// 垂直居中到加高的标题栏区（约 52px），与同高的自绘工具栏行对齐。这是 AppKit 的
/// 官方语义，布局由系统持续维护；逐帧挪按钮 frame 的做法会在窗口重排（show/聚焦/
/// 全屏往返）时被 AppKit 重置，builder 的 traffic_light_position 正是因此不生效。
/// toolbar 无任何 item，titlebarAppearsTransparent 下不画任何东西，只改按钮布局。
#[cfg(target_os = "macos")]
pub(crate) fn unify_titlebar_toolbar(window: &tauri::WebviewWindow) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    let w = window.clone();
    // AppKit 对象只能在主线程操作；build() 可能发生在任意子线程。
    let _ = window.run_on_main_thread(move || {
        if let Ok(ns_window) = w.ns_window() {
            let ns_window = ns_window as *mut AnyObject;
            unsafe {
                // `new` 是 +1 引用计数的所有权返回,接成裸指针那份所有权就没人释放了
                // (setToolbar: 自己另外 retain 一次)——每建一个对话窗漏一个 NSToolbar。
                // 用 Retained 接管:作用域结束自动 release,窗口那份 retain 不受影响。
                let toolbar: objc2::rc::Retained<AnyObject> = msg_send![class!(NSToolbar), new];
                let _: () = msg_send![ns_window, setToolbar: &*toolbar];
                // NSWindowToolbarStyleUnified = 3（macOS 11+，本应用最低 14.0）。
                let _: () = msg_send![ns_window, setToolbarStyle: 3isize];
            }
        }
    });
}

/// 打开单例会话对话窗口。再次点击另一张卡片时复用窗口并发事件切换会话。
/// 创建/切换失败必须传播回前端：静默失败时用户「点了没反应」，再点会重复起会话。
#[tauri::command]
pub(crate) async fn open_chat_window(app: tauri::AppHandle, session_id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_chat_window_impl(&app, session_id, true))
        .await
        .map_err(|e| e.to_string())?
}

/// 无处上报错误的入口（通知/托盘点击等**用户主动**动作）用的 fire-and-forget 变体；
/// 失败仅留日志。点击是明确意图，窗口正常抢焦点。
pub(crate) fn open_chat_window_detached(app: tauri::AppHandle, session_id: i64) {
    std::thread::spawn(move || {
        if let Err(error) = open_chat_window_impl(&app, session_id, true) {
            eprintln!("打开对话窗口失败: {error}");
        }
    });
}

/// 审批/提问唤醒专用的**安静**变体：窗口要出现（否则审批无处可答），但**不抢焦点**——
/// 用户可能正在外部终端里打字，焦点被抢走一次就是一次输入错位（实拍反馈）。
/// 召唤注意力由调用方的任务栏闪烁（request_user_attention）承担。
pub(crate) fn open_chat_window_quiet(app: tauri::AppHandle, session_id: i64) {
    std::thread::spawn(move || {
        if let Err(error) = open_chat_window_impl(&app, session_id, false) {
            eprintln!("打开对话窗口失败: {error}");
        }
    });
}

/// 对话窗此刻是否「在用户眼前」：存在、可见、且未最小化。审批/提问的召唤策略据此
/// 分流——在眼前时窗口里必有用户正在看的会话，强行切会话是夺屏（实拍反馈：正在 A
/// 会话里阅读/打字，B 会话一来审批就被瞬间拽走）；不在眼前（隐藏/最小化/未创建）时
/// 切换无打断，仍走切会话召唤。查询失败按「不在眼前」处理：宁可多召唤一次，
/// 不可让审批悄悄没入后台。
pub(crate) fn chat_window_in_view(app: &tauri::AppHandle) -> bool {
    app.get_window("chat")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// 显示但不激活：Windows 用 SW_SHOWNOACTIVATE（普通 show 会激活并抢焦点）。
/// 其余平台退化为普通 show——macOS 上显示非激活 App 的窗口本就不跨应用抢焦点
/// （激活策略未变），够用。
///
/// Windows 上原生 `ShowWindow` 之后**必须**再补一次 `window.show()` 同步 tao 的标志位，
/// 否则贴纸会在后续某次窗口标志变更时凭空消失，详见函数内注释。
pub(crate) fn show_window_no_activate(window: &tauri::Window) {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        if let Ok(handle) = window.hwnd() {
            unsafe {
                ShowWindow(handle.0 as _, SW_SHOWNOACTIVATE);
            }
            // 裸 ShowWindow 绕过了 tao 的 WindowFlags：VISIBLE 位只在 `set_visible` 里维护，
            // tao 从不监听 WM_SHOWWINDOW 回填，于是它一直以为贴纸是隐藏的。而
            // `WindowFlags::apply_diff` 末尾有一句**无条件**的「新标志里没有 VISIBLE 就
            // ShowWindow(SW_HIDE)」——此后任何改窗口标志的调用都会顺手把贴纸藏掉：
            // island_layout 的 `set_resizable` / `set_always_on_top`、点击穿透的
            // `set_ignore_cursor_events` 全在此列。
            // 实拍症状（旧贴边缩略条时代）：悬停偷看展开第一句就是 set_resizable(true)，
            // 贴纸整个不见——它 skipTaskbar + transparent，任务栏也没入口，只能托盘「找回
            // 贴纸」；而找回走的 `recall_center` 里正好有一句 tauri 的 `show()`，把 VISIBLE
            // 补上后当次会话就不再复现（「有时候才出现」的由来）。
            // 补这一次 show() 把标志对齐：窗口此刻已可见，且贴纸建窗时 focused:false 让 tao 的
            // MARKER_DONT_FOCUS 常驻，apply_diff 里同样走 SW_SHOWNOACTIVATE，不抢焦点（W-7）。
            let _ = window.show();
            return;
        }
    }
    let _ = window.show();
}

/// 贴纸主窗口的首帧显示（前端 useShowWhenReady 在 focus:false 时调用，W-7）：
/// 普通 show() 在 Windows 上会激活并抢焦点——贴纸配置 focus:false（开机自启不得抢焦点），
/// 首显同样必须无声显形，走 SW_SHOWNOACTIVATE 路径。只接受 main 标签，防误用于其它窗口。
#[tauri::command]
pub(crate) fn show_sticker(window: tauri::WebviewWindow) {
    if window.label() == "main" {
        show_window_no_activate(&window.as_ref().window());
    }
}

/// macOS 贴纸面板的「保持打开」（W-12）。macOS 贴纸已换成灵动岛（2026-11-05），面板不再
/// 失焦自动隐藏；命令留着只为旧前端调用不报错，空操作。
#[tauri::command]
pub(crate) fn set_panel_keep_open(open: bool) {
    let _ = open;
}

/// 贴纸面板外观跟应用主题（macOS 原生材质的深/浅）：theme 为 "dark"/"light" 强制，
/// None（前端 theme=system）跟随系统。非 macOS 空操作，前端只在 macOS 贴纸窗调用。
#[tauri::command]
pub(crate) fn set_panel_appearance(app: tauri::AppHandle, theme: Option<String>) {
    // 灵动岛（2026-11-05 起 macOS 也是）的白字黑底配色写死在 CSS 里，不跟主题：弹出面板的毛玻璃
    // 也只能是深色，浅色材质上白字看不清。前端还会照旧推主题，这里一律钉成深色。
    #[cfg(target_os = "macos")]
    {
        let _ = theme;
        crate::macos::panel::set_appearance(&app, Some("dark"));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, theme);
}

/// 把窗口可靠带到前台。普通 set_focus 在 Windows 上受前台锁限制：托盘点击授予的
/// SetForegroundWindow 权限只在点击后的极短窗口内有效，稍有延迟（查库、WebView 首帧）
/// 就被拒——窗口显示了却垫在当前前台窗口身后，用户看到的就是「点了没反应」
/// （实拍：dev/安装版并存时双击托盘「呼不出」对话窗，其实它开在另一实例的对话窗底下）。
/// 走 AttachThreadInput 的 force_foreground（点卡片拉终端置前的同一套）强置前。
fn raise_window(window: &tauri::Window) {
    #[cfg(target_os = "windows")]
    if let Ok(handle) = window.hwnd() {
        crate::terminal::force_foreground(handle.0 as _);
        return;
    }
    let _ = window.set_focus();
}

/// 托盘点击入口：打开最近活跃会话的对话窗口。托盘没有「当前会话」上下文，
/// 取 last_event_at 最新的未归档会话最贴近「看看现在在跑什么」。
/// 一条会话都没有（新装/全归档）时回落到设置窗口，避免点了毫无反应。
/// 入口：两平台托盘菜单「打开对话窗口」；贴纸底栏按钮（open_latest_chat）。
pub(crate) fn open_latest_chat_window(app: &tauri::AppHandle) {
    let app = app.clone();
    // 查库放子线程：与 open_settings_window 同理由，主线程同步 IO 会阻塞消息泵。
    std::thread::spawn(move || {
        let latest = crate::open_store(&crate::db_path())
            .ok()
            .and_then(|store| store.latest_session_id().ok().flatten());
        // 托盘入口没有 UI 可上报；至少留日志。
        if let Err(error) = open_chat_window_target(&app, latest_chat_target(latest), true) {
            eprintln!("打开对话窗口失败: {error}");
        }
    });
}

/// 托盘单击/菜单「打开对话窗口」落在哪。库里一个会话都没有（全新安装）时也开对话窗新建页：
/// 引导最后一步教的就是「单击托盘图标打开对话窗口」，原先回落设置「通用」分区与引导对不上
/// （UI 调研 shell #1：全新 ~/.meowo 走完引导单击托盘弹出设置窗）。新建页自带「未检测到 CLI · 去安装」。
fn latest_chat_target(latest: Option<i64>) -> ChatTarget {
    match latest {
        Some(session_id) => ChatTarget::Session(session_id),
        None => ChatTarget::NewSession {
            cwd: None,
            provider: None,
        },
    }
}

/// 前端调用（贴纸底栏「打开对话窗口」按钮）：打开最近活跃会话的对话窗口。
/// 同步 command 仅做 thread::spawn（open_latest_chat_window 内部自带子线程查库），不阻塞主线程。
#[tauri::command]
pub(crate) fn open_latest_chat(app: tauri::AppHandle) {
    open_latest_chat_window(&app);
}

/// 对话窗打开后停在哪：某个会话，或新建会话页（带可选预填）。
pub(crate) enum ChatTarget {
    Session(i64),
    NewSession {
        cwd: Option<String>,
        provider: Option<String>,
    },
    /// 会话管理页，预选「已完成」（设置页「已完成会话」的入口：那里不再自带一份列表）。
    AdminDone,
}

impl ChatTarget {
    /// 新建对话窗时的页面地址（前端 ChatWindow 按 query 决定首屏）。
    fn url(&self) -> String {
        match self {
            ChatTarget::Session(id) => format!("index.html?sessionId={id}"),
            ChatTarget::NewSession { cwd, provider } => {
                let mut url = "index.html?newSession=1".to_string();
                if let Some(c) = cwd {
                    url.push_str(&format!(
                        "&cwd={}",
                        percent_encode(c.as_bytes(), NON_ALPHANUMERIC)
                    ));
                }
                if let Some(p) = provider {
                    url.push_str(&format!(
                        "&provider={}",
                        percent_encode(p.as_bytes(), NON_ALPHANUMERIC)
                    ));
                }
                url
            }
            ChatTarget::AdminDone => "index.html?admin=done".to_string(),
        }
    }
}

/// `activate`：true = 用户主动打开（点卡片/托盘/通知），照常抢焦点；
/// false = 审批/提问唤醒——窗口要出现但不夺焦（用户可能正在外部终端打字，实拍反馈）。
pub(crate) fn open_chat_window_impl(
    app: &tauri::AppHandle,
    session_id: i64,
    activate: bool,
) -> Result<(), String> {
    open_chat_window_target(app, ChatTarget::Session(session_id), activate)
}

pub(crate) fn open_chat_window_target(
    app: &tauri::AppHandle,
    target: ChatTarget,
    activate: bool,
) -> Result<(), String> {
    // 对话功能关闭（轻量模式）时的唯一闸门：四个开窗变体（command/detached/quiet/latest）
    // 全部汇于此，一处拦截全覆盖。用户主动点击（activate）回落设置窗——那里有重新打开
    // 的开关，点了不能毫无反应；审批/提问的安静唤醒则无声吞掉，调用方已各自回落 TUI。
    if !crate::settings::load_settings().chat_enabled {
        if activate {
            open_settings_window(app);
        }
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    crate::macos::menubar::settings_window_will_open(app, "chat");

    // 用 get_window 而不是 get_webview_window：面板里开了浏览器页签后 chat 窗口不再是
    // 「单 webview 窗口」，后者会返回 None，这里就会去再建一个同名窗口然后报错。
    if let Some(w) = app.get_window("chat") {
        use tauri::Emitter;
        // emit 失败绝不能静默：窗口会照常聚焦，但停留在旧会话——用户点了卡片 A，
        // 看到的却是会话 B，比不弹窗更具误导性。
        match &target {
            ChatTarget::Session(session_id) => w
                .emit("chat-session-changed", *session_id)
                .map_err(|e| format!("切换会话失败: {e}"))?,
            ChatTarget::NewSession { cwd, provider } => w
                .emit(
                    "chat-new-session",
                    serde_json::json!({ "cwd": cwd, "provider": provider }),
                )
                .map_err(|e| format!("打开新建会话页失败: {e}"))?,
            ChatTarget::AdminDone => w
                .emit("chat-open-admin", "done")
                .map_err(|e| format!("打开会话管理失败: {e}"))?,
        }
        if activate {
            let _ = w.show();
            raise_window(&w);
        } else if !w.is_visible().unwrap_or(false) {
            // 隐藏（关到托盘等）时无声显形；最小化的留在任务栏——闪烁已在召唤，
            // 强行还原窗口同样是夺屏。
            show_window_no_activate(&w);
        }
        return Ok(());
    }
    let url = target.url();
    let builder = tauri::WebviewWindowBuilder::new(app, "chat", tauri::WebviewUrl::App(url.into()))
        .title("Meowo · Chat")
        // 首开 1200 宽（用户要求加大）：侧栏 + 阅读列 + 改动面板 + 标题栏一排工具，960 挤。
        // 高度不动：768 高的笔记本屏连 760 都已贴边。窗口状态插件不记尺寸（只记位置），每次开窗都是它。
        .inner_size(1200.0, 760.0)
        .min_inner_size(620.0, 480.0)
        .resizable(true)
        // 安静唤醒的新窗口不带初始焦点（冷启动首帧 show 的激活行为随之收敛）。
        .focused(activate)
        // 隐藏创建，前端首帧后自行显示（见 show_after_grace 注释），消除白框闪烁。
        .visible(false)
        .center();
    // 平台分支的理由见 open_settings_window 的同款注释。
    #[cfg(not(target_os = "macos"))]
    let builder = crate::material::dress(builder.decorations(false), app, "chat");
    #[cfg(target_os = "macos")]
    let builder = crate::material::dress(
        builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true),
        app,
        "chat",
    );
    let win = builder.build().map_err(|e| {
        // 同上:失败也要还激活策略计数,否则 Dock 图标永久常驻。
        #[cfg(target_os = "macos")]
        crate::macos::menubar::settings_window_did_close(app, "chat");
        format!("创建对话窗口失败: {e}")
    })?;
    // 对话窗标题栏行高 52px（.chat-bar/.chat-sidebar-head），红绿灯默认贴顶会和同行的
    // 标题/按钮错位；挂空 toolbar 让系统把它垂直居中（备忘录式侧栏惯例）。
    #[cfg(target_os = "macos")]
    unify_titlebar_toolbar(&win);
    show_after_grace(&win, activate);
    let app_handle = app.clone();
    win.on_window_event(move |e| {
        // 只在真正销毁时收尾，不碰 CloseRequested：前端在面板终端有程序在跑时会拦下关闭
        // 让用户确认（chat/windowClose.ts），用户点取消窗口还在。若这里在 CloseRequested
        // 就交还审批、清「正在看」、减 macOS 激活计数，取消后审批已落回 TUI 收不回、Dock
        // 计数也错减。全仓没有把对话窗关闭改成隐藏的路径，真关闭必然走到 Destroyed。
        if matches!(e, tauri::WindowEvent::Destroyed) {
            let ptys = &app_handle.state::<crate::AppState>().ptys;
            // 关窗兜底：清桌面租约，再把无人看的挂起审批交还 TUI。手机端（remote:）
            // 的新鲜租约与桌面窗生命周期无关，不被连坐——手机上正等着批的卡不能
            // 因为桌面关了个窗就被悄悄 pass 掉。
            ptys.release_desktop_consumers();
            // 同属关窗兜底：本窗的「正在看」注册随窗失效，清掉让 pty-output emitter
            // 跳过已无消费者的 base64 + emit（app 级 emit 后 emit 侧查不到窗口存在性）。
            ptys.reset_viewer();
            #[cfg(target_os = "macos")]
            crate::macos::menubar::settings_window_did_close(&app_handle, "chat");
            // 面板里开的普通终端只属于这扇窗：窗没了就再没人能看到或关掉它们。
            if let Some(shells) = app_handle.try_state::<crate::shell_terminal::ShellTerminals>() {
                shells.kill_all();
            }
            if let Some(panes) = app_handle.try_state::<crate::browser_pane::BrowserPanes>() {
                panes.clear();
            }
        }
    });
    // 置前兜底：真正 show 由前端首帧触发（useShowWhenReady），距点击已过去数秒
    // （懒加载 chunk；dev 下 vite 现场编译更久）——前端那句 setFocus 到达时托盘点击
    // 授予的前台权限早已过期，窗口默默垫在当前前台窗口身后。盯到首次可见即强置前；
    // 只对用户主动打开（activate）做，安静唤醒不夺焦的语义不变。
    if activate {
        let w = win.clone();
        std::thread::spawn(move || {
            for _ in 0..300 {
                if w.is_visible().unwrap_or(false) {
                    raise_window(&w.as_ref().window());
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    }
    Ok(())
}

/// 「找回贴纸」：把主窗口按当前尺寸居中到主显示器工作区，并显示/取消最小化/聚焦。
/// 置顶是**临时**的（W-6）：先置顶确保浮到所有窗口（含其它置顶窗口）之上，聚焦后按调用方给的
/// 用户 pin 偏好还原——「找回」只把窗口带到眼前，不替用户改写置顶偏好。
/// 只剩托盘找回（recall_sticker）这一个调用方，前端不再直接调，故不再注册为命令。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recall_center(window: tauri::WebviewWindow, pinned: bool) -> Result<(), String> {
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
    if !pinned {
        // 聚焦完成后撤掉临时置顶：z-order 已抬到最前，窗口保持可见但不长期压住其它窗口。
        window.set_always_on_top(false).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 托盘「找回贴纸」：唤起主窗口并通知前端执行找回。
/// Windows 上主窗口是灵动岛（贴纸已删，2026-11-05 用户定案）：只把岛显示出来并让前端展开
/// 面板，不做下面的改尺寸 + 居中——那会把岛拽到屏幕正中，前端没活着时就一直停在那里。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recall_sticker(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        if cfg!(target_os = "windows") {
            let _ = w.emit("recall-sticker", ());
            return;
        }
        // 原生层几何救援，不依赖 webview 活着：WebView2 浏览器进程崩溃后窗口 JS 全灭，
        // 下面 emit 的事件无人处理，「找回」就彻底失灵（实拍：吸附细条卡在屏边，重启
        // 才恢复）。窗口还是细条尺寸就先撑回正常最小尺寸，再居中主屏；前端若活着，
        // 随后收到 recall-sticker 会用 island_layout 重新贴顶摆成完整列表，终态一致。
        let scale = w.scale_factor().unwrap_or(1.0);
        if let Ok(size) = w.outer_size() {
            let logical = size.to_logical::<f64>(scale);
            if logical.width < crate::snap::STICKER_MIN_W || logical.height < crate::snap::STICKER_MIN_H
            {
                let _ = w.set_size(tauri::LogicalSize::new(
                    crate::snap::STICKER_MIN_W,
                    crate::snap::STICKER_MIN_H,
                ));
            }
        }
        // 原生救援路径读不到前端的 pin 偏好（localStorage 在 webview 里，可能已崩）——
        // 传 false：只做临时置顶带到眼前，不假设用户偏好；前端活着时随后会按真实偏好还原。
        let _ = recall_center(w.clone(), false);
        let _ = w.emit("recall-sticker", ());
    }
}

/// 托盘右键菜单（打开对话窗口 / 找回贴纸 / 设置 / 官网 / 退出），按语言构建；切语言时由 rebuild_tray_menu 重建。
/// 菜单构成与 macOS（menubar::build_tray_menu）对齐：同一组项、同一顺序，`chat_enabled`
/// 为 false（轻量模式）时不放「打开对话窗口」项——入口都不该出现，而不是点了被拦。
#[cfg(not(target_os = "macos"))]
pub(crate) fn build_tray_menu(
    app: &tauri::AppHandle,
    lang: &str,
    chat_enabled: bool,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let chat = if chat_enabled {
        Some(MenuItemBuilder::with_id("chat", tr(lang, "tray.chat")).build(app)?)
    } else {
        None
    };
    // Windows 的贴纸已删、只剩灵动岛（2026-11-05 用户定案），岛恒在屏幕顶上谈不上「找回」，
    // 菜单里不放这一项（用户实拍定案）。
    let recall = if cfg!(target_os = "windows") {
        None
    } else {
        Some(MenuItemBuilder::with_id("recall", tr(lang, "tray.recall")).build(app)?)
    };
    let guide = MenuItemBuilder::with_id("guide", tr(lang, "tray.guide")).build(app)?;
    let settings = MenuItemBuilder::with_id("settings", tr(lang, "tray.settings")).build(app)?;
    let website = MenuItemBuilder::with_id("website", tr(lang, "tray.website")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(lang, "tray.quit")).build(app)?;
    let mut builder = MenuBuilder::new(app);
    if let Some(chat) = &chat {
        builder = builder.item(chat);
    }
    if let Some(recall) = &recall {
        builder = builder.item(recall);
    }
    builder.items(&[&guide, &settings, &website, &quit]).build()
}

/// 切语言/改设置后让已存在的系统 UI 跟上：重建托盘菜单、改已开设置窗口的标题。
/// `chat_enabled` 决定两平台托盘菜单是否包含「打开对话窗口」项（主线程不读设置文件，
/// 由调用方从已持有的 Settings 传入——线程纪律）。
pub(crate) fn apply_language(app: &tauri::AppHandle, lang: &str, chat_enabled: bool) {
    if let Some(tray) = app.tray_by_id("meowo-tray") {
        #[cfg(not(target_os = "macos"))]
        if let Ok(menu) = build_tray_menu(app, lang, chat_enabled) {
            let _ = tray.set_menu(Some(menu));
        }
        #[cfg(target_os = "macos")]
        if let Ok(menu) = crate::macos::menubar::build_tray_menu(app, lang, chat_enabled) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    // macOS 顶部应用主菜单同样按语言重建（见 menubar::build_app_menu）。
    #[cfg(target_os = "macos")]
    if let Ok(menu) = crate::macos::menubar::build_app_menu(app, lang) {
        let _ = app.set_menu(menu);
    }
    if let Some(w) = app.get_webview_window("about") {
        let _ = w.set_title(tr(lang, "window.settings"));
    }
    if let Some(w) = app.get_webview_window("updater") {
        let _ = w.set_title(tr(lang, "window.updater"));
    }
    // chat 窗口的标题由前端随会话状态动态维护（ChatWindow 的 setTitle effect），
    // 这里不再按语言重设——两处写同一标题会互相覆盖。
    // Windows 托盘 tooltip 含本地化文案，但 watch 侧只在计数变化时重发：置位 dirty
    // 让下一轮 liveness 强制刷新，否则旧语言一直挂到下次计数变化。
    #[cfg(target_os = "windows")]
    crate::watch::mark_tray_tooltip_dirty();
}

/// dev 构建的托盘角标：右下角叠一枚琥珀圆点，带深色描边与抗锯齿边缘——托盘图标
/// 只有 16~32px，无描边的硬边色块糊成一团锯齿。像素级绘制而非另备图标资源：
/// 角标只存在于 debug 构建，不值得为它维护一套打包文件。
/// （待交互角标/数字徽章两轮迭代都被实拍否掉、已撤；dev 角标是用户点名要保留的。）
#[cfg(not(target_os = "macos"))]
fn dev_badge_icon(icon: &tauri::image::Image<'_>) -> tauri::image::Image<'static> {
    /// 按覆盖率把颜色混进像素（普通 alpha over），cov ∈ [0,1]。
    fn blend(px: &mut [u8], color: [u8; 3], cov: f32) {
        for c in 0..3 {
            px[c] = (f32::from(color[c]) * cov + f32::from(px[c]) * (1.0 - cov)).round() as u8;
        }
        let a = f32::from(px[3]) / 255.0;
        px[3] = ((cov + a * (1.0 - cov)) * 255.0).round() as u8;
    }
    let (w, h) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    let r = (w.min(h) as f32) * 0.26;
    let rim = (r * 0.22).max(1.0); // 描边厚度随尺寸缩放，最小 1px 保证 16px 下仍有分离感
    let (cx, cy) = (w as f32 - r - 1.0, h as f32 - r - 1.0);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let d = (dx * dx + dy * dy).sqrt();
            let Some(px) = rgba.get_mut(((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize)
            else {
                continue;
            };
            // 半像素过渡的圆盘覆盖率：外层深色圆先铺，内层琥珀圆压上，留出的环即描边。
            let outer = (r + 0.5 - d).clamp(0.0, 1.0);
            if outer > 0.0 {
                blend(px, [40, 34, 28], outer);
            }
            let inner = (r - rim + 0.5 - d).clamp(0.0, 1.0);
            if inner > 0.0 {
                blend(px, [245, 158, 11], inner);
            }
        }
    }
    tauri::image::Image::new_owned(rgba, w, h)
}

/// 单击/双击消抖世代：双击抬起前必然先到来一次单击抬起，单击动作延迟一个双击窗口期，
/// 世代变了（双击到达）就作废——否则双击会先把贴纸开合一遍再开对话窗。
#[cfg(not(target_os = "macos"))]
static TRAY_CLICK_GEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 最近一次双击的时刻。Windows 双击的消息序列是「抬起 → 双击 → 抬起」：上面的世代只作废了
/// 双击**之前**那次单击，双击**之后**还会再来一次左键抬起，它另起一个单击定时器、一个双击
/// 窗口期后把贴纸开合一遍——用户看到的就是「双击托盘弹出的是贴纸」（实拍反馈）。紧跟在双击
/// 之后（一个双击窗口期内）的抬起不算单击。
#[cfg(not(target_os = "macos"))]
static TRAY_LAST_DOUBLE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// 这次左键抬起是不是双击的尾巴（双击之后一个双击窗口期内）。
#[cfg(not(target_os = "macos"))]
fn tray_click_trails_double(now: std::time::Instant) -> bool {
    TRAY_LAST_DOUBLE
        .lock()
        .ok()
        .and_then(|last| *last)
        .is_some_and(|at| now.saturating_duration_since(at) <= tray_click_debounce())
}

/// 单击消抖窗口取**系统双击时间**（Windows 默认 500ms，控制面板可调）。曾写死 300ms：
/// 正常手速的双击（间隔 300~500ms）在 300ms 定时器到点时第二击还没到，先漏出一次
/// 单击把贴纸开合了，随后系统仍认出双击、对话窗也开——两窗齐弹（实拍反馈）。
/// Linux 无统一的系统双击时间，跟随 Windows 默认值。
#[cfg(not(target_os = "macos"))]
fn tray_click_debounce() -> std::time::Duration {
    #[cfg(target_os = "windows")]
    let ms = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() };
    #[cfg(not(target_os = "windows"))]
    let ms: u32 = 500;
    std::time::Duration::from_millis(u64::from(ms))
}

/// 构建系统托盘：单击左键开合主界面（贴纸窗，与 macOS 左键开合面板同语义）、双击左键
/// 打开对话窗口（W-11 前的左键语义，老用户肌肉记忆）；右键菜单提供打开对话窗口 /
/// 找回贴纸 / 使用引导 / 设置 / 官网 / 退出。
/// macOS 走 `macos::menubar::setup_tray`（面板模式），故此实现仅用于非 macOS 平台。
#[cfg(not(target_os = "macos"))]
pub(crate) fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let boot_settings = load_settings();
    let menu = build_tray_menu(
        app.handle(),
        ui_lang(&boot_settings),
        boot_settings.chat_enabled,
    )?;

    let mut builder = TrayIconBuilder::with_id("meowo-tray");
    // 图标恒由打包提供，但缺失时不该 unwrap panic 把启动打挂——没图标就建无图标托盘。
    // dev 构建叠琥珀角标：共库后 dev 与安装版托盘并排且内容全同，只能靠图标分辨实例
    // （tooltip 的 "(dev)" 也在，但鼠标不悬停看不见）。
    if let Some(icon) = app.default_window_icon() {
        if cfg!(debug_assertions) {
            builder = builder.icon(dev_badge_icon(icon));
        } else {
            builder = builder.icon(icon.clone());
        }
    }
    builder
        .tooltip(if cfg!(debug_assertions) { "Meowo (dev)" } else { "Meowo" })
        .menu(&menu)
        // 左键留给「开合主界面」，菜单仅在右键弹出（打开对话窗口/设置都在右键菜单里）。
        .show_menu_on_left_click(false)
        // 托盘菜单事件在**主线程**派发(与下面的左键回调同源)。开窗口必须丢子线程:
        // 直接 build() 会占住消息泵,而 WebView2 初始化依赖泵运转 → 卡在初始化 → 白屏
        // (理由与 open_settings 命令版同源,见本文件顶部)。此前这两项是裸调,右键
        // 「设置」/「使用引导」会把贴纸与对话窗一起卡住。
        .on_menu_event(|app, event| match event.id().as_ref() {
            // open_latest_chat_window 内部自带子线程查库，主线程直调不阻塞泵。
            "chat" => open_latest_chat_window(app),
            "recall" => recall_sticker(app),
            "guide" => {
                let app = app.clone();
                std::thread::spawn(move || open_onboarding_window(&app));
            }
            "settings" => {
                let app = app.clone();
                std::thread::spawn(move || open_settings_window(&app));
            }
            "website" => {
                let _ = crate::settings::open_url(crate::settings::SITE_URL.to_string());
            }
            "quit" => request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            use std::sync::atomic::Ordering;
            match event {
                // 仅左键「抬起」时触发，避免按下+抬起各触发一次。
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    // 单击 = 开合主界面（贴纸窗），与 macOS 左键开合面板同语义；窗口 API
                    // 只投递消息不读文件，主线程直调（同 macOS panel::toggle_panel）。
                    // 延迟一个系统双击窗口期：紧随其后是双击则这次开合作废
                    // （见 TRAY_CLICK_GEN；窗口期取值理由见 tray_click_debounce）。
                    if tray_click_trails_double(std::time::Instant::now()) {
                        return;
                    }
                    let gen = TRAY_CLICK_GEN.fetch_add(1, Ordering::SeqCst) + 1;
                    let app = tray.app_handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(tray_click_debounce());
                        if TRAY_CLICK_GEN.load(Ordering::SeqCst) == gen {
                            toggle_main_window(&app);
                        }
                    });
                }
                // 双击 = 打开对话窗（老用户肌肉记忆，W-11 前的左键语义）；对话功能关闭
                // （轻量模式）时找回贴纸——轻量用户的主界面就是贴纸。load_settings 是文件
                // IO，托盘回调在主线程，放子线程读（线程纪律）。
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    TRAY_CLICK_GEN.fetch_add(1, Ordering::SeqCst);
                    if let Ok(mut last) = TRAY_LAST_DOUBLE.lock() {
                        *last = Some(std::time::Instant::now());
                    }
                    let app = tray.app_handle().clone();
                    std::thread::spawn(move || {
                        if crate::settings::load_settings().chat_enabled {
                            open_latest_chat_window(&app);
                        } else {
                            recall_sticker(&app);
                        }
                    });
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

/// 托盘左键开合主界面（贴纸窗）：可见且聚焦 → 隐藏；可见未聚焦 → 聚焦（用户在找它）；
/// 隐藏/最小化 → 显示并聚焦。贴纸 skipTaskbar，藏起来后任务栏没有入口，左键必须可逆，
/// 不能像旧行为（左键只开对话窗）那样把「找回贴纸」只留在右键菜单深处。
/// Windows 例外：贴纸已删，只剩灵动岛（2026-11-05 用户定案），单击改为打开对话窗口；
/// 对话功能关闭（轻量模式）时没有对话窗，退而把岛显示出来并展开面板。
#[cfg(not(target_os = "macos"))]
pub(crate) fn toggle_main_window(app: &tauri::AppHandle) {
    if cfg!(target_os = "windows") {
        // 调用点已在托盘单击消抖子线程，读设置文件不堵消息泵。
        if crate::settings::load_settings().chat_enabled {
            open_latest_chat_window(app);
        } else {
            recall_sticker(app);
        }
        return;
    }
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let visible = w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false);
    if !visible {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    } else if w.is_focused().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.set_focus();
    }
}

/// Windows：把待交互/运行中会话数摘要写进托盘悬浮提示，鼠标移到托盘一眼可见，
/// 弥补桌面端无菜单栏标题。计数为 0 时回落到纯品牌名。
#[cfg(target_os = "windows")]
pub(crate) fn update_tray_tooltip(
    app: &tauri::AppHandle,
    running: usize,
    waiting: usize,
    lang: &str,
) {
    let Some(tray) = app.tray_by_id("meowo-tray") else {
        return;
    };
    // dev 标记在调用点补而不进 tray_tooltip_text：那是带精确断言的纯函数，
    // 单测在 debug 下跑，函数内 cfg! 会让断言随构建漂移。
    let mut tip = tray_tooltip_text(lang, running, waiting);
    if cfg!(debug_assertions) {
        tip = tip.replacen("Meowo", "Meowo (dev)", 1);
    }
    let _ = tray.set_tooltip(Some(tip));
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

/// 托盘「退出」。退出会杀掉 Meowo 托管的全部 agent 进程（见 lib.rs 的 RunEvent::Exit 收尾），
/// 跑到一半的任务就断了，此前一点即退、没有任何提示。对照 Kimi Code 的退出确认：有托管会话
/// 正在运行时先问一句，没有就直接退。等你回答 / 审批的会话不拦，恢复后进度不丢。
/// 面板终端同理：RunEvent::Exit 会 kill_all，里面跑着的 dev server、构建一起结束，判定与写法
/// 同关对话窗（chat/windowClose.ts）。确认走原生小窗（confirm::ask），没开对话窗也能问。
/// 更新安装重启不经过这里（updater 的 on_before_exit），照旧不问。
pub(crate) fn request_quit(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    // 确认框开着时再点一次「退出」不再弹第二个。
    static ASKING: AtomicBool = AtomicBool::new(false);
    if ASKING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let count_app = app.clone();
        // 查库、读设置都是文件 IO，放进 blocking 池。统计失败按「没在跑」处理：宁可照常退出，
        // 也不能让「退出」点了没反应。
        let (message, lang) = tauri::async_runtime::spawn_blocking(move || {
            let running = crate::session_query::managed_running_now(&count_app).unwrap_or(0);
            let busy = count_app
                .try_state::<crate::shell_terminal::ShellTerminals>()
                .map(|shells| shells.busy_all())
                .unwrap_or_default();
            let lang = ui_lang(&load_settings());
            let busy: Vec<String> = busy.iter().map(|shell| shell.describe()).collect();
            (quit_confirm_message(lang, running, &busy), lang)
        })
        .await
        .unwrap_or((None, "zh"));
        let quit = confirm_quit(message, |message| {
            crate::confirm::ask(
                app.clone(),
                None,
                tr(lang, "quit.confirmTitle").to_string(),
                message,
                true,
                Some(tr(lang, "quit.confirmQuit").to_string()),
            )
        })
        .await;
        ASKING.store(false, Ordering::SeqCst);
        if quit {
            app.exit(0);
        }
    });
}

/// 托盘退出要问的话；None = 没有会话在跑、面板终端也都空闲，直接退。`busy_shells` 是
/// 有程序在跑的面板终端（[`crate::shell_terminal::ShellBusy::describe`]），逐行列在说明下面。
fn quit_confirm_message(lang: &str, running_sessions: i64, busy_shells: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    if running_sessions > 0 {
        parts.push(tr(lang, "quit.confirmMessage").replace("{n}", &running_sessions.to_string()));
    }
    if !busy_shells.is_empty() {
        parts.push(format!("{}\n\n{}", tr(lang, "quit.shellBusy"), busy_shells.join("\n")));
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// 没什么要问的直接退；要问就等用户回答，取消则不退。确认框建不起来（理论上不发生）
/// 照常退出：用户明确点了「退出」。
async fn confirm_quit<F, Fut, E>(message: Option<String>, ask: F) -> bool
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<bool, E>>,
{
    match message {
        None => true,
        Some(message) => ask(message).await.unwrap_or(true),
    }
}

#[cfg(test)]
mod quit_tests {
    use super::{confirm_quit, quit_confirm_message};

    #[test]
    fn quit_asks_when_panel_terminal_runs_a_program_even_without_sessions() {
        let busy = vec!["pwsh · node.exe (PID 42)".to_string()];
        let message = quit_confirm_message("zh", 0, &busy).expect("面板终端在跑程序要先问");
        assert!(message.contains("面板终端还有程序在跑") && message.contains("node.exe (PID 42)"), "{message}");
        let both = quit_confirm_message("en", 2, &busy).unwrap();
        assert!(both.contains("Sessions still running: 2") && both.contains("panel terminals"), "{both}");
        assert_eq!(quit_confirm_message("zh", 0, &[]), None);
        assert!(quit_confirm_message("zh", 1, &[]).unwrap().contains('1'));
    }

    #[test]
    fn quit_cancelled_keeps_running_and_nothing_to_ask_quits_without_prompt() {
        use tauri::async_runtime::block_on as block;
        assert!(!block(confirm_quit(Some("q".into()), |_| async { Ok::<_, ()>(false) })));
        assert!(block(confirm_quit(Some("q".into()), |_| async { Ok::<_, ()>(true) })));
        assert!(block(confirm_quit(Some("q".into()), |_| async { Err::<bool, _>(()) })));
        let mut asked = false;
        assert!(block(confirm_quit(None, |_| {
            asked = true;
            async { Ok::<_, ()>(false) }
        })));
        assert!(!asked, "没什么要问时不该弹确认");
    }
}

#[cfg(test)]
mod window_bg_tests {
    use super::{WINDOW_BG_DARK, WINDOW_BG_LIGHT};

    /// 原生底色与页面底色差一点，show 瞬间与材质回退时就是一次可见的闪色。
    /// styles.css 里 `--cc-window-bg` 只有两处定义：先深色（:root）后浅色（data-theme=light）。
    #[test]
    fn 原生底色与样式表的窗口底色两档一致() {
        let css = include_str!("../../src/styles.css");
        let defined: Vec<&str> = css
            .lines()
            .filter_map(|line| line.trim().strip_prefix("--cc-window-bg:"))
            .map(|value| value.trim().trim_end_matches(';').trim())
            .collect();
        let hex = |(r, g, b): (u8, u8, u8)| format!("#{r:02x}{g:02x}{b:02x}");
        assert_eq!(
            defined,
            vec![hex(WINDOW_BG_DARK), hex(WINDOW_BG_LIGHT)],
            "styles.css 的 --cc-window-bg（深、浅）要与 window.rs 的 WINDOW_BG_DARK / LIGHT 同值"
        );
    }
}

#[cfg(test)]
mod chat_target_tests {
    use super::ChatTarget;

    #[test]
    fn 新建会话页的开窗地址带预填且转义() {
        assert_eq!(ChatTarget::Session(42).url(), "index.html?sessionId=42");
        assert_eq!(ChatTarget::AdminDone.url(), "index.html?admin=done");
        assert_eq!(
            ChatTarget::NewSession { cwd: None, provider: None }.url(),
            "index.html?newSession=1"
        );
        assert_eq!(
            ChatTarget::NewSession {
                cwd: Some(r"C:\my proj".into()),
                provider: Some("claude".into()),
            }
            .url(),
            "index.html?newSession=1&cwd=C%3A%5Cmy%20proj&provider=claude"
        );
    }
}

#[cfg(test)]
#[path = "window_latest_target_tests.rs"]
mod latest_target_tests;

#[cfg(all(test, not(target_os = "macos")))]
mod tray_click_tests {
    use super::{tray_click_debounce, tray_click_trails_double, TRAY_LAST_DOUBLE};
    use std::time::{Duration, Instant};

    #[test]
    fn 双击之后紧跟的左键抬起不算单击() {
        let at = Instant::now();
        *TRAY_LAST_DOUBLE.lock().unwrap() = Some(at);
        assert!(tray_click_trails_double(at + Duration::from_millis(50)));
        assert!(!tray_click_trails_double(
            at + tray_click_debounce() + Duration::from_millis(50)
        ));
    }
}
