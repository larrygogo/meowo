use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::macos::{notch, panel};

/// 菜单栏空闲图标（全空闲时回落）：单色 36×36 条形图，黑+alpha 掩码、其余透明，照真实 app logo 比例复刻。
/// macOS 按菜单栏明暗自动反色。原始 RGBA（行优先、上到下），不走 PNG 解码以免引入 image-png feature。
const MENUBAR_ICON_RGBA: &[u8] = include_bytes!("../../icons/menubar-template.rgba");
const MENUBAR_ICON_SIZE: u32 = 36;

/// 最近一次的 (运行, 待办)。watch 线程写，图标动画线程读。
static LAST_COUNTS: Mutex<(usize, usize)> = Mutex::new((0, 0));

/// 落点变了（插拔外接屏、合盖）后重设菜单栏图标。图标两种落点同一种，这里只是按最近计数重发一次。
pub fn refresh_tray_icon(app: &AppHandle) {
    let (running, waiting) = *LAST_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
    update_tray_status(app, running, waiting);
}

/// 按 (运行, 待办) 更新菜单栏：图标的动画交给 [`spawn_icon_animator`]（见 tray_anim.rs），
/// 这里只记下计数、更新标题：有会话要你处理时图标右边显示数量（系统菜单栏字体，自动跟深浅色），
/// 没有就不显示文字。原来的彩色计数徽章静止不动、和系统图标风格也不搭（用户 2026-11-05 要「灵动」）。
pub fn update_tray_status(app: &AppHandle, running: usize, waiting: usize) {
    *LAST_COUNTS.lock().unwrap_or_else(|e| e.into_inner()) = (running, waiting);
    let Some(tray) = app.tray_by_id("meowo-tray") else {
        return;
    };
    let title = (waiting > 0).then(|| waiting.min(99).to_string());
    let _ = tray.set_title(title.as_deref());
}

/// 图标动画线程：有会话在跑或要你处理时约 10 帧/秒重画图标；都没有时回到静止 logo 并歇着，
/// 不白白每秒重画十次。
fn spawn_icon_animator(app: AppHandle) {
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut idle_drawn = true; // 建托盘时已经是静止 logo
        loop {
            let (running, waiting) = *LAST_COUNTS.lock().unwrap_or_else(|e| e.into_inner());
            let Some(tray) = app.tray_by_id("meowo-tray") else {
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            };
            if running == 0 && waiting == 0 {
                if !idle_drawn {
                    let logo = tauri::image::Image::new(MENUBAR_ICON_RGBA, MENUBAR_ICON_SIZE, MENUBAR_ICON_SIZE);
                    let _ = tray.set_icon_with_as_template(Some(logo), true);
                    idle_drawn = true;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            }
            idle_drawn = false;
            let t = start.elapsed().as_secs_f32();
            let rgba = crate::tray_anim::render(t, running > 0, waiting > 0);
            let size = crate::tray_anim::SIZE as u32;
            let _ = tray.set_icon_with_as_template(Some(tauri::image::Image::new(&rgba, size, size)), true);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

/// 托盘右键菜单（打开对话窗口 / 使用引导 / 设置 / 官网 / 退出），按语言构建；
/// 「找回贴纸」随贴纸一起去掉了（2026-11-05，与 Windows 同）。
/// 切语言/改设置时由 lib.rs 的 apply_language 重建。菜单构成与 Windows
/// （window::build_tray_menu）对齐：同一组项、同一顺序。`chat_enabled` 为 false
/// （轻量模式）时不放「打开对话窗口」项——入口都不该出现，而不是点了被拦。
/// 设置页切换开关会经 set_settings → apply_language 即时重建。
pub fn build_tray_menu(
    app: &AppHandle,
    lang: &str,
    chat_enabled: bool,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let chat = if chat_enabled {
        Some(MenuItemBuilder::with_id("chat", crate::tr(lang, "tray.chat")).build(app)?)
    } else {
        None
    };
    let guide = MenuItemBuilder::with_id("guide", crate::tr(lang, "tray.guide")).build(app)?;
    let settings =
        MenuItemBuilder::with_id("settings", crate::tr(lang, "tray.settings")).build(app)?;
    let website =
        MenuItemBuilder::with_id("website", crate::tr(lang, "tray.website")).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", crate::tr(lang, "tray.quit")).build(app)?;
    let mut builder = MenuBuilder::new(app);
    if let Some(chat) = &chat {
        builder = builder.item(chat);
    }
    builder.items(&[&guide, &settings, &website, &quit]).build()
}

/// 应用主菜单自定义项的 id。与托盘菜单的 "settings" 等错开：两套菜单的事件都会
/// 经各自 handler 派发，id 不同才不会在托盘 handler 里被重复响应。
const APP_MENU_SETTINGS_ID: &str = "appmenu.settings";
/// 「关于 Meowo」不走系统 About 面板（只有名字和版本号），而是打开本应用的更新窗口：
/// 版本号、检查更新、安装都在那里，和设置 → 关于一致。
const APP_MENU_ABOUT_ID: &str = "appmenu.about";

/// macOS 顶部应用主菜单（Accessory 模式下不可见；打开设置窗/对话窗切到 Regular 时露出）。
/// 不建它 Tauri 会自动装一套写死英文的默认菜单（File / Edit / View / Window / Help），
/// 中文界面下顶栏仍是英文。这里按语言自建，切语言时随托盘菜单一起重建（apply_language）。
/// 编辑菜单的 撤销/剪切/拷贝/粘贴/全选 必须保留：WKWebView 的 ⌘Z/⌘X/⌘C/⌘V/⌘A 靠这些
/// predefined 项派发，没有菜单项快捷键就失灵。File/View/Help 没有本应用用得上的项，不建。
pub fn build_app_menu(app: &AppHandle, lang: &str) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let pkg = app.package_info();
    let t = |key: &str| crate::tr(lang, key);
    let app_menu = SubmenuBuilder::new(app, pkg.name.clone())
        .item(&MenuItemBuilder::with_id(APP_MENU_ABOUT_ID, t("menu.about")).build(app)?)
        .separator()
        .item(
            &MenuItemBuilder::with_id(APP_MENU_SETTINGS_ID, t("menu.settings"))
                .accelerator("Cmd+,")
                .build(app)?,
        )
        .separator()
        .item(&PredefinedMenuItem::services(app, Some(t("menu.services")))?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, Some(t("menu.hide")))?)
        .item(&PredefinedMenuItem::hide_others(app, Some(t("menu.hideOthers")))?)
        .item(&PredefinedMenuItem::show_all(app, Some(t("menu.showAll")))?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some(t("menu.quit")))?)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, t("menu.edit"))
        .item(&PredefinedMenuItem::undo(app, Some(t("menu.undo")))?)
        .item(&PredefinedMenuItem::redo(app, Some(t("menu.redo")))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(t("menu.cut")))?)
        .item(&PredefinedMenuItem::copy(app, Some(t("menu.copy")))?)
        .item(&PredefinedMenuItem::paste(app, Some(t("menu.paste")))?)
        .item(&PredefinedMenuItem::select_all(app, Some(t("menu.selectAll")))?)
        .build()?;
    let window_menu = SubmenuBuilder::new(app, t("menu.window"))
        .item(&PredefinedMenuItem::minimize(app, Some(t("menu.minimize")))?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, Some(t("menu.close")))?)
        .build()?;
    MenuBuilder::new(app)
        .items(&[&app_menu, &edit_menu, &window_menu])
        .build()
}

/// 装上应用主菜单并接自定义项的事件（setup 里调用一次；之后切语言只 set_menu 重建，
/// handler 按 id 匹配不用重挂）。
pub fn setup_app_menu(app: &AppHandle, lang: &str) -> tauri::Result<()> {
    app.set_menu(build_app_menu(app, lang)?)?;
    app.on_menu_event(|app, event| match event.id().as_ref() {
        APP_MENU_SETTINGS_ID => crate::open_settings_window(app),
        APP_MENU_ABOUT_ID => crate::open_update_window(app.clone()),
        _ => {}
    });
    Ok(())
}

/// 创建 macOS 状态栏托盘：左键见 [`on_left_click`]，右键弹菜单。
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let boot_settings = crate::load_settings();
    let menu = build_tray_menu(app, crate::ui_lang(&boot_settings), boot_settings.chat_enabled)?;

    // 菜单栏用单色模板图标（彩色 app 图标在菜单栏里偏花，且不随明暗反色）；Dock/设置页仍用彩色图标。
    let icon = tauri::image::Image::new(MENUBAR_ICON_RGBA, MENUBAR_ICON_SIZE, MENUBAR_ICON_SIZE);
    TrayIconBuilder::with_id("meowo-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Meowo")
        .menu(&menu)
        .show_menu_on_left_click(false) // 左键不弹菜单 => 留给右键
        .on_menu_event(|app, event| match event.id().as_ref() {
            "chat" => crate::open_latest_chat_window(app),
            "guide" => crate::open_onboarding_window(app),
            "settings" => crate::open_settings_window(app),
            "website" => {
                let _ = crate::settings::open_url(crate::settings::SITE_URL.to_string());
            }
            "quit" => crate::window::request_quit(app),
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
                on_left_click(app);
            }
        })
        .build(app)?;
    spawn_icon_animator(app.clone());
    Ok(())
}

/// 单击菜单栏图标：
/// - 有刘海：岛就在刘海上，图标和 Windows 托盘一样打开对话窗口（轻量模式下展开岛）。
/// - 没刘海：点它在下方弹出 / 收起岛的面板（前端 island-tray-toggle）。
///
/// 托盘事件在主线程，这里可以直接重新探测刘海。
fn on_left_click(app: &AppHandle) {
    use tauri::Emitter;
    let (has_notch, changed) = notch::refresh();
    if changed {
        refresh_tray_icon(app);
    }
    if has_notch.is_some() && crate::load_settings().chat_enabled {
        crate::open_latest_chat_window(app);
        return;
    }
    if has_notch.is_some() {
        panel::recall_panel(app);
        return;
    }
    let _ = app.emit_to("main", "island-tray-toggle", ());
}

/// macOS 激活策略的引用计数：当前处于「需要 Regular」状态的窗口 label 集合。
/// 设置/引导/更新/新建会话/对话窗口各自在打开时切 Regular、关闭时归还计数；两个窗口同开时
/// 关掉任一个，不能把仍开着的另一个误切回 Accessory（Dock 图标消失、剩余窗口无法获焦），
/// 故按 label 计数、归零才切回。
fn regular_policy_windows() -> &'static Mutex<HashSet<&'static str>> {
    static SET: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 引用计数纯逻辑（便于单测）：登记一个窗口打开。按 label 去重——窗口已存在时打开路径
/// 只是聚焦、会重入，同一窗口不得产生两笔计数。
fn track_regular_open(set: &mut HashSet<&'static str>, label: &'static str) {
    set.insert(label);
}

/// 引用计数纯逻辑（便于单测）：登记一个窗口关闭，返回是否可以切回 Accessory（最后一个
/// 窗口关闭才为 true）。同一窗口的 CloseRequested 与 Destroyed 会各来一次：set 语义下
/// 第二次是空操作，不会提前归零。
fn track_regular_close(set: &mut HashSet<&'static str>, label: &'static str) -> bool {
    set.remove(label);
    set.is_empty()
}

/// 打开设置类窗口前临时切到 Regular 以便获焦（挂在各窗口的创建/聚焦路径上）。
pub fn settings_window_will_open(app: &AppHandle, label: &'static str) {
    if let Ok(mut set) = regular_policy_windows().lock() {
        track_regular_open(&mut set, label);
    }
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
}

/// 归还激活策略计数（挂在各窗口的关闭事件里）：最后一个这类窗口关闭才切回 Accessory。
pub fn settings_window_did_close(app: &AppHandle, label: &'static str) {
    let may_restore = match regular_policy_windows().lock() {
        Ok(mut set) => track_regular_close(&mut set, label),
        // 锁损坏：退化为旧行为（无条件切回），好过永远不切、Dock 图标一直挂着。
        Err(_) => true,
    };
    if may_restore {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 激活策略引用计数：两个窗口同开时关掉任一个不得切回 Accessory；同一窗口的
    /// CloseRequested/Destroyed 双回调与「已存在窗口的重复打开」都不得扰乱计数。
    #[test]
    fn regular_policy_reference_counting() {
        let mut set = HashSet::new();
        track_regular_open(&mut set, "about");
        track_regular_open(&mut set, "chat");
        // 已存在的窗口再次「打开」（聚焦路径）重入，不产生额外计数。
        track_regular_open(&mut set, "chat");
        // 关掉任一个：仍有窗口开着，不切回。
        assert!(!track_regular_close(&mut set, "chat"));
        // 同一窗口的 CloseRequested + Destroyed 双回调：第二次是空操作，不得提前归零。
        assert!(!track_regular_close(&mut set, "chat"));
        // 最后一个窗口关闭 → 归零，切回 Accessory。
        assert!(track_regular_close(&mut set, "about"));
    }
}
