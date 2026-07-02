//! 应用设置：持久化、默认值、i18n 文案，以及设置页相关命令。

use crate::{apply_language, db_path};
use std::path::PathBuf;
use tauri::Emitter;
use tauri_plugin_autostart::ManagerExt;

fn default_true() -> bool {
    true
}
/// 外观默认值（与前端 appearance.ts / styles.css 的初值保持一致）。
fn default_theme() -> String {
    "dark".to_string()
}
fn default_opacity() -> u32 {
    94
}
fn default_ui_scale() -> u32 {
    100
}
fn default_resume_terminal() -> String {
    "terminal".to_string()
}
fn default_language() -> String {
    "auto".to_string()
}
/// 打开终端的方式：card = 点击卡片直接打开（默认），button = 卡片上单独的打开按钮。
fn default_terminal_open_mode() -> String {
    "card".to_string()
}
/// 贴纸风格：elevated = 立体感（默认），flat = 扁平。
fn default_sticker_style() -> String {
    "elevated".to_string()
}
/// 贴纸底色预设 key（classic = 经典原色，默认）。
fn default_sticker_color() -> String {
    "classic".to_string()
}
/// 在贴纸底栏显示配额的 provider key 列表，默认仅 claude。
fn default_sticker_quota_providers() -> Vec<String> {
    vec!["claude".to_string()]
}

/// 应用设置（持久化到 ~/.cc-kanban/settings.json）。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Settings {
    /// 归档条目自动隐藏的天数；0 = 永不隐藏。
    #[serde(default)]
    pub(crate) archive_hide_days: u32,
    /// 桌面通知总开关（待交互 + 错误）。缺省为开启，兼容老 settings.json。
    #[serde(default = "default_true")]
    pub(crate) notifications_enabled: bool,
    /// 外观模式：dark / light / system（跟随系统）。缺省 dark，兼容老 settings.json。
    #[serde(default = "default_theme")]
    pub(crate) theme: String,
    /// 贴纸背景不透明度（百分比 25–100）。缺省 94，与原视觉一致。
    #[serde(default = "default_opacity")]
    pub(crate) opacity: u32,
    /// 界面密度/字号缩放（百分比，紧凑 90 / 标准 100 / 宽松 112）。
    #[serde(default = "default_ui_scale")]
    pub(crate) ui_scale: u32,
    /// 打开未连接会话用的终端（macOS）：terminal = Terminal.app，iterm = iTerm2。缺省 terminal，兼容老 settings.json。
    #[serde(default = "default_resume_terminal")]
    pub(crate) resume_terminal: String,
    /// 界面/通知语言：auto（跟随系统）/ zh / en。缺省 auto，兼容老 settings.json。
    #[serde(default = "default_language")]
    pub(crate) language: String,
    /// 打开终端方式：card = 点击卡片（默认），button = 卡片单独打开按钮。兼容老 settings.json。
    #[serde(default = "default_terminal_open_mode")]
    pub(crate) terminal_open_mode: String,
    /// 是否显示卡片 hover「轻推」预览（最近一条 AI 正文）。缺省开启，兼容老 settings.json。
    #[serde(default = "default_true")]
    pub(crate) preview_enabled: bool,
    /// 贴纸风格：elevated = 立体感（默认），flat = 扁平。缺省 elevated，兼容老 settings.json。
    #[serde(default = "default_sticker_style")]
    pub(crate) sticker_style: String,
    /// 贴纸底色预设 key（classic/slate/moss/plum/rose/amber）。缺省 classic，兼容老 settings.json。
    #[serde(default = "default_sticker_color")]
    pub(crate) sticker_color: String,
    /// 在贴纸底栏显示配额的 provider key 列表（如 "claude"/"kimi"/"codex"）。
    /// 缺省 ["claude"]，旧 settings.json 无此字段时反序列化给默认，不 panic。
    #[serde(default = "default_sticker_quota_providers")]
    pub(crate) sticker_quota_providers: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            archive_hide_days: 0,
            notifications_enabled: true,
            theme: default_theme(),
            opacity: default_opacity(),
            ui_scale: default_ui_scale(),
            resume_terminal: default_resume_terminal(),
            language: default_language(),
            terminal_open_mode: default_terminal_open_mode(),
            preview_enabled: true,
            sticker_style: default_sticker_style(),
            sticker_color: default_sticker_color(),
            sticker_quota_providers: default_sticker_quota_providers(),
        }
    }
}

/// 解析生效语言：settings.language 为 zh/en 用之；auto 按系统 locale（zh* → zh，其余 en）。
pub(crate) fn ui_lang(settings: &Settings) -> &'static str {
    match settings.language.as_str() {
        "zh" => "zh",
        "en" => "en",
        _ => {
            if sys_locale::get_locale()
                .map(|l| l.starts_with("zh"))
                .unwrap_or(false)
            {
                "zh"
            } else {
                "en"
            }
        }
    }
}

/// Rust 侧用户可见文案（仅通知/托盘/窗口标题数条，不引 i18n 库）。
pub(crate) fn tr(lang: &str, key: &str) -> &'static str {
    match (lang, key) {
        ("en", "notify.error") => "Session error",
        ("en", "notify.waiting") => "Waiting for your reply",
        ("en", "notify.pending.approval") => "Approve a tool call?",
        ("en", "notify.pending.question") => "A session is asking you a question",
        ("en", "notify.pending.plan") => "Plan awaiting approval",
        ("en", "tray.recall") => "Recall sticker",
        ("en", "tray.settings") => "Settings",
        ("en", "tray.quit") => "Quit",
        ("en", "window.settings") => "Settings",
        (_, "notify.error") => "会话出错",
        (_, "notify.waiting") => "等待你回复",
        (_, "notify.pending.approval") => "需要你批准工具调用",
        (_, "notify.pending.question") => "会话在问你问题",
        (_, "notify.pending.plan") => "计划待批准",
        (_, "tray.recall") => "找回贴纸",
        (_, "tray.settings") => "设置",
        (_, "tray.quit") => "退出",
        (_, "window.settings") => "设置",
        _ => "",
    }
}

fn settings_path() -> PathBuf {
    db_path().with_file_name("settings.json")
}

pub(crate) fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn get_settings() -> Settings {
    load_settings()
}

#[tauri::command]
pub(crate) fn set_settings(app: tauri::AppHandle, mut settings: Settings) -> Result<(), String> {
    // 后端兜底钳值（与前端 appearance.ts 一致），防越界值落盘后被 5s 轮询线程读到。
    settings.opacity = settings.opacity.clamp(25, 100);
    settings.ui_scale = settings.ui_scale.clamp(50, 200);
    let body = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = settings_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // 原子写（tmp + rename）：后台轮询线程每 5s 裸读本文件，直写可能被读到半截而回退默认值。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp); // best-effort 清理，避免遗留 .tmp
        return Err(e.to_string());
    }
    // 切语言后重建托盘菜单/窗口标题（无条件重建，菜单仅两项，幂等且廉价）。
    apply_language(&app, ui_lang(&settings));
    // 通知贴纸窗口实时套用新设置。
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

/// 设置窗口用：读取/切换开机自启（原来只在托盘，托盘精简后搬到设置页）。
#[tauri::command]
pub(crate) fn get_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    // dev 下自启会注册 dev 二进制(开机连不上 dev server → 白屏)，一律视为关闭，避免误导。
    if tauri::is_dev() {
        return Ok(false);
    }
    Ok(app.autolaunch().is_enabled().unwrap_or(false))
}

#[tauri::command]
pub(crate) fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    // dev 下拒绝写入：否则会把 target/debug 的调试二进制注册进开机自启，开机白屏。仅安装版可用。
    if tauri::is_dev() {
        return Err("开机自启仅在安装版可用（dev 下会注册调试二进制，开机连不上 dev server）".into());
    }
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())?;
        // auto-launch 写 Run 项用 format!("{} {}", path, args)——路径不加引号。路径含空格(如用户名
        // "First Last" → C:\Users\First Last\...)会被 Windows 拆成「程序+参数」，开机自启直接失败。
        // enable 成功后把该 Run 值重写为带引号的可执行路径修正（值名与插件一致 = package_info().name）。
        #[cfg(target_os = "windows")]
        quote_autostart_run_value(&app);
        Ok(())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
}

/// 把 HKCU\...\Run 下本应用的自启项值重写为带引号的可执行路径，修正 auto-launch 不加引号、
/// 含空格路径开机自启失败的问题。失败不致命（仅日志），不影响开关状态。
#[cfg(target_os = "windows")]
fn quote_autostart_run_value(app: &tauri::AppHandle) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[autostart] current_exe 失败，跳过路径加引号: {e}");
            return;
        }
    };
    let name = app.package_info().name.clone(); // 与 tauri-plugin-autostart 的 Run 项名一致
    let value = format!("\"{}\"", exe.display());
    let run = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_SET_VALUE);
    match run {
        Ok(run) => {
            if let Err(e) = run.set_value(&name, &value) {
                eprintln!("[autostart] 重写带引号路径失败: {e}");
            }
        }
        Err(e) => eprintln!("[autostart] 打开 Run 注册表键失败: {e}"),
    }
}

/// 设置/关于页用：在默认浏览器打开本项目链接。仅允许本仓库的 https 链接（白名单），
/// Windows 用 explorer、macOS 用 open 打开（均不经 shell），杜绝被滥用打开任意/恶意目标。
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://github.com/larrygogo/cc-kanban") {
        return Err("不允许的链接".into());
    }
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    // macOS：open 偶发慢（默认浏览器冷启动），放后台线程不挡主线程。
    // status() 而非 spawn()：spawn 后不 wait，Unix 上 Child 被 drop 不会 reap，
    // 常驻托盘的本进程会积累 <defunct> 僵尸；已在后台线程，阻塞等待无害。
    #[cfg(target_os = "macos")]
    std::thread::spawn(move || {
        let _ = std::process::Command::new("open").arg(&url).status();
    });
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = url;
    Ok(())
}
