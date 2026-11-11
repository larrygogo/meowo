//! 窗口系统材质：Windows 11 的 Mica、macOS 的 Liquid Glass / NSVisualEffectView 侧栏材质。
//!
//! **macOS 分支未经本机编译**（Windows 开发机编译不到 `cfg(target_os = "macos")`），
//! 逻辑自查 + 靠 CI 的 macOS 矩阵把关；真机效果待局域网 Mac 实测。
//!
//! 做法对照的是本仓锁定版本的源码（tauri 2.12.5 / tauri-runtime-wry 2.12.3 / tao 0.37.2 /
//! wry 0.57.1 / window-vibrancy 0.8.1）：
//! - Mica 本体只是一个 DWM 属性：window-vibrancy `apply_mica` 调
//!   `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW)`（build 22523+；
//!   22000~22522 走未文档化的 1029 号属性，本模块的 22H2 门槛把那段排除在外）。
//! - 但属性设了还得**看得见**：背板画在窗口内容之下，客户区像素 alpha 为 0 才透得出来。
//!   所以窗口必须 `transparent(true)`——tao 据此调 `DwmEnableBlurBehindWindow`（空区域，让
//!   DWM 按像素 alpha 合成），tauri-runtime-wry 用 softbuffer 把客户区刷成全 0，wry 把
//!   WebView2 的默认底色设成 (0,0,0,0)。三者缺一就是一块不透明的黑或白。
//! - 材质生效的窗口**不能再设 `background_color`**：tao 的 WM_ERASEBKGND 与 softbuffer 都会
//!   拿它刷客户区，GDI 写进去的是 alpha=0 的非零 RGB，按预乘 alpha 合成等于往背板上叠加一层
//!   颜色（浅色主题下整窗发白，材质就看不出来了）。首帧露出来的就是材质本身，不会白闪。
//! - macOS 的材质视图（NSGlassEffectView / NSVisualEffectView）由 window-vibrancy 插在
//!   内容视图最底下；`transparent(true)` 让 NSWindow 与 WKWebView 不画底，才透得出来。
//!   tauri 的 `apply_effects` 在 macOS 15 及以下自动从 Liquid Glass 落到列表里的第二个材质。

use std::collections::BTreeMap;
use std::sync::Mutex;

/// 一扇窗实际用上的材质。前端据此决定加不加根类 `mat-on`（`window_material` 命令）。
/// 变体按平台门控：别的平台上永远构造不出来，留着会被 `-D warnings` 按 dead_code 报死。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Material {
    None,
    #[cfg(target_os = "windows")]
    Mica,
    #[cfg(target_os = "macos")]
    Glass,
    #[cfg(target_os = "macos")]
    Vibrancy,
}

impl Material {
    fn as_str(self) -> &'static str {
        match self {
            Material::None => "none",
            #[cfg(target_os = "windows")]
            Material::Mica => "mica",
            #[cfg(target_os = "macos")]
            Material::Glass => "glass",
            #[cfg(target_os = "macos")]
            Material::Vibrancy => "vibrancy",
        }
    }
}

/// 各窗口建窗时定下的材质（label → 材质）。建窗时写、`window_material` 与换主题时读，
/// 都是微秒级临界区，同步命令在主线程读它符合线程纪律。
static APPLIED: Mutex<BTreeMap<String, Material>> = Mutex::new(BTreeMap::new());

fn applied() -> std::sync::MutexGuard<'static, BTreeMap<String, Material>> {
    APPLIED.lock().unwrap_or_else(|e| e.into_inner())
}

/// 强制关材质的环境变量：**真机出问题时的一行开关**。设成任意非空且不是 `0` 的值
/// （如 `MEOWO_NO_MATERIAL=1`），所有窗口回到不透明实色底，等同于材质上线前的行为。
const KILL_SWITCH_ENV: &str = "MEOWO_NO_MATERIAL";

fn kill_switch_on(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let v = v.trim();
        !v.is_empty() && v != "0"
    })
}

/// 哪些窗口要材质。确认框 confirm-* 与灵动岛 main 不在其列：确认框是瞬时小窗，
/// 灵动岛定案纯黑。macOS 只做对话窗与设置窗（SPEC 终态），其余小窗保持不透明。
fn wants_material(label: &str) -> bool {
    if cfg!(target_os = "windows") {
        matches!(label, "chat" | "about" | "onboarding" | "updater" | "new-session")
    } else if cfg!(target_os = "macos") {
        matches!(label, "chat" | "about")
    } else {
        false
    }
}

/// **材质启用与否的唯一判断点**。要整体关掉：设环境变量 `MEOWO_NO_MATERIAL=1`，
/// 或让本函数直接返回 `Material::None`。
/// 会读注册表（Windows）/ 系统版本文件（macOS），只在建窗线程调，不在主线程调。
fn decide(label: &str) -> Material {
    if kill_switch_on(std::env::var(KILL_SWITCH_ENV).ok().as_deref()) || !wants_material(label) {
        return Material::None;
    }
    platform_material()
}

#[cfg(target_os = "windows")]
fn platform_material() -> Material {
    let env = windows_env::read();
    if mica_allowed(env.build, env.transparency, env.high_contrast_flags) {
        Material::Mica
    } else {
        Material::None
    }
}

#[cfg(target_os = "macos")]
fn platform_material() -> Material {
    // 版本只决定报给前端的名字；26 以下由 tauri 自己从 Liquid Glass 落到 Sidebar 材质。
    // 读不到版本按旧系统报：两种材质前端都加 mat-on，名字报错不影响外观。
    let major = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
        .ok()
        .and_then(|plist| macos_major_version(&plist));
    if major.is_some_and(|major| major >= LIQUID_GLASS_MIN_MACOS) {
        Material::Glass
    } else {
        Material::Vibrancy
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn platform_material() -> Material {
    // Linux：tauri 的窗口效果标注 Unsupported，透明窗口在部分显卡上还会把渲染失败变成
    // 「看得见桌面的空窗」，一律实色。
    Material::None
}

/// Windows 11 22H2 的 build 号。更早的 Win11（22000 / 22H1）只有未文档化的 Mica 属性，
/// 且 DWMWA_SYSTEMBACKDROP_TYPE 要 22523+；Windows 10 没有 Mica。
#[cfg(any(target_os = "windows", test))]
const MICA_MIN_BUILD: u32 = 22621;

/// `transparency`：`EnableTransparency`（系统「透明效果」开关，1 开 0 关）。读不到按开着算，
/// 那是系统默认值。`high_contrast_flags`：`HighContrast\Flags`，最低位 HCF_HIGHCONTRASTON。
/// 高对比度下不上材质：系统自己会把背板换成对比主题的纯色，页面那层半透明染色只会添乱。
#[cfg(any(target_os = "windows", test))]
fn mica_allowed(build: u32, transparency: Option<u32>, high_contrast_flags: Option<u32>) -> bool {
    build >= MICA_MIN_BUILD
        && transparency.unwrap_or(1) != 0
        && high_contrast_flags.unwrap_or(0) & 1 == 0
}

#[cfg(target_os = "windows")]
mod windows_env {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    pub(super) struct Env {
        pub build: u32,
        pub transparency: Option<u32>,
        pub high_contrast_flags: Option<u32>,
    }

    /// 每次建窗现读一遍（三次注册表读，微秒级）：不监听变化，用户改了「透明效果」后新开的
    /// 窗口跟上，已开的窗口维持建窗时的决定——开着材质时用户再关透明效果，DWM 自己会把
    /// 背板换成纯色，不需要我们动。
    pub(super) fn read() -> Env {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        // CurrentBuildNumber 是 REG_SZ（如 "26200"）。读不到按 0：不上材质。
        let build = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ)
            .ok()
            .and_then(|key| key.get_value::<String, _>("CurrentBuildNumber").ok())
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0);
        let transparency = hkcu
            .open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
                KEY_READ,
            )
            .ok()
            .and_then(|key| key.get_value::<u32, _>("EnableTransparency").ok());
        // Flags 也是 REG_SZ（十进制数字串）。
        let high_contrast_flags = hkcu
            .open_subkey_with_flags(r"Control Panel\Accessibility\HighContrast", KEY_READ)
            .ok()
            .and_then(|key| key.get_value::<String, _>("Flags").ok())
            .and_then(|text| text.trim().parse().ok());
        Env {
            build,
            transparency,
            high_contrast_flags,
        }
    }
}

/// Liquid Glass 从 macOS 26 起有（window-vibrancy 按 NSAppKitVersionNumber >= 2685 判）。
#[cfg(any(target_os = "macos", test))]
const LIQUID_GLASS_MIN_MACOS: u32 = 26;

/// 从 SystemVersion.plist 里取主版本号（`<key>ProductVersion</key>` 后面那个 `<string>`）。
/// 手写而不走 NSProcessInfo：后者要给 objc2-foundation 加 feature，本机又编译不到 macOS，
/// 纯字符串解析能在 Windows 上跑单测。
#[cfg(any(target_os = "macos", test))]
fn macos_major_version(plist: &str) -> Option<u32> {
    let after_key = plist.split("<key>ProductVersion</key>").nth(1)?;
    let value = after_key.split("<string>").nth(1)?.split("</string>").next()?;
    value.trim().split('.').next()?.parse().ok()
}

/// 材质对应的 tauri 窗口效果；`None` = 不上材质。
/// Windows 按**应用主题**明暗选 MicaDark / MicaLight（应用主题可以与系统不同，跟系统走的
/// `Mica` 会出现深色界面垫浅色背板）。
fn effects_config(
    material: Material,
    dark: bool,
) -> Option<tauri::utils::config::WindowEffectsConfig> {
    #[cfg(target_os = "windows")]
    let effects = match material {
        Material::None => return None,
        Material::Mica if dark => vec![tauri::utils::WindowEffect::MicaDark],
        Material::Mica => vec![tauri::utils::WindowEffect::MicaLight],
    };
    #[cfg(target_os = "macos")]
    let effects = {
        // macOS 的材质明暗跟窗口外观走，不由这里指定。
        let _ = dark;
        match material {
            Material::None => return None,
            // Liquid Glass 在前、Sidebar 在后：macOS 15 及以下 tauri 自动落到后者。
            Material::Glass | Material::Vibrancy => vec![
                tauri::utils::WindowEffect::LiquidGlassRegular,
                tauri::utils::WindowEffect::Sidebar,
            ],
        }
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let effects: Vec<tauri::utils::WindowEffect> = {
        let _ = (material, dark);
        Vec::new()
    };
    if effects.is_empty() {
        return None;
    }
    Some(tauri::utils::config::WindowEffectsConfig {
        effects,
        // 跟随窗口激活状态（系统默认）：失焦时材质变灰是平台惯例。
        state: None,
        // 圆角交给系统窗口，不在材质视图上另切。
        radius: None,
        color: None,
        interactive: false,
    })
}

/// 给独立窗口的 builder 配底：材质生效就开透明 + 窗口效果，否则设不透明的原生底色
/// （`window_background_color`，防 show 瞬间露白）。两者互斥的原因见文件头。
/// 在建窗线程调（会读注册表 / 设置文件）。
pub(crate) fn dress<'a>(
    builder: tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle>,
    app: &tauri::AppHandle,
    label: &str,
) -> tauri::WebviewWindowBuilder<'a, tauri::Wry, tauri::AppHandle> {
    let material = decide(label);
    let theme = crate::settings::load_settings().theme;
    let dark = crate::window::theme_is_dark(app, &theme);
    let effects = effects_config(material, dark);
    // 记下来的是真正交给 tauri 的结果；建窗失败留下的旧记录无害，下次建窗会覆盖。
    applied().insert(
        label.to_string(),
        if effects.is_some() { material } else { Material::None },
    );
    match effects {
        Some(effects) => builder.transparent(true).effects(effects),
        None => builder.background_color(crate::window::window_background_color(app)),
    }
}

/// 应用主题变了（设置页改外观）：已开着的 Mica 窗口换成对应明暗的背板。
/// macOS 的材质跟窗口外观走，这里没有要做的。`set_effects` 自己会切到主线程。
pub(crate) fn sync_theme(app: &tauri::AppHandle, theme: &str) {
    #[cfg(target_os = "windows")]
    {
        use tauri::Manager;
        let dark = crate::window::theme_is_dark(app, theme);
        let labels: Vec<String> = applied()
            .iter()
            .filter(|(_, material)| **material == Material::Mica)
            .map(|(label, _)| label.clone())
            .collect();
        for label in labels {
            // get_window 而不是 get_webview_window：对话窗开了浏览器页签后是多 webview 窗口。
            if let Some(window) = app.get_window(&label) {
                let _ = window.set_effects(effects_config(Material::Mica, dark));
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, theme);
}

/// 「应用主题设置 + 系统此刻的明暗 → Mica 背板该用深色吗」。固定 dark / light 时不看系统，
/// 其余（system 与认不出的值）跟系统走，与 `window::theme_is_dark` 的分档一致。
#[cfg(any(target_os = "windows", test))]
fn backdrop_dark(app_theme: &str, system_dark: bool) -> bool {
    match app_theme {
        "dark" => true,
        "light" => false,
        _ => system_dark,
    }
}

/// 系统切了明暗（`WindowEvent::ThemeChanged`，lib.rs 的 builder 上统一挂）：把这扇窗的 Mica
/// 背板重新套成该有的明暗。
///
/// 为什么固定 dark / light 也要管：Mica 的明暗就是 `DWMWA_USE_IMMERSIVE_DARK_MODE` 这一个
/// DWM 属性（window-vibrancy 0.8.1 `apply_mica`），而 tao 0.37.2 收到 WM_SETTINGCHANGE 时
/// 会按系统主题把它重写一遍（`update_theme` → `refresh_titlebar_theme_color`），应用固定
/// 深色、系统切到浅色，背板就被改成浅的了。
///
/// 不会成环：纠正走 `set_effects` 直接写 DWM 属性，不经 tao 的 `set_theme`，不动 tao 记的
/// `current_theme`；而 ThemeChanged 只在 WM_SETTINGCHANGE 里 `current_theme` 变了才发。
///
/// 明暗取事件带来的这扇窗自己的新主题，不去问主窗口的 `theme()`：各窗各收各的
/// WM_SETTINGCHANGE，主窗口那时可能还没更新。
/// 事件回调在主线程：这里只读内存里的建窗记录，读设置文件挪进 blocking 池（线程纪律）。
pub(crate) fn on_system_theme_changed(window: &tauri::Window, system_theme: tauri::Theme) {
    #[cfg(target_os = "windows")]
    {
        if applied().get(window.label()).copied() != Some(Material::Mica) {
            return;
        }
        let system_dark = system_theme == tauri::Theme::Dark;
        let window = window.clone();
        // 句柄不留：纠正失败（窗口已关）没有可补救的，下次切换或保存设置会再套一次。
        tauri::async_runtime::spawn_blocking(move || {
            let theme = crate::settings::load_settings().theme;
            let dark = backdrop_dark(&theme, system_dark);
            // `set_effects` 自己会切回主线程。
            let _ = window.set_effects(effects_config(Material::Mica, dark));
        });
    }
    // macOS 的材质明暗跟窗口外观走，系统自己会翻；Linux 没有材质。
    #[cfg(not(target_os = "windows"))]
    let _ = (window, system_theme);
}

/// 前端调用（main.tsx 启动时）：本窗口用上了哪种系统材质——`none` / `mica` / `glass` /
/// `vibrancy`。非 `none` 时前端给根元素加 `mat-on`，页面底才换成半透明染色层。
/// 只读内存里的建窗记录，同步命令在主线程跑没问题。不进远程桥白名单：手机页没有窗口材质。
#[tauri::command]
pub(crate) fn window_material(window: tauri::Window) -> &'static str {
    applied()
        .get(window.label())
        .copied()
        .unwrap_or(Material::None)
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_只在_win11_22h2_以上且透明效果开着且非高对比度时启用() {
        assert!(mica_allowed(22621, Some(1), Some(126)));
        assert!(mica_allowed(26200, None, None), "读不到开关按系统默认（开）算");
        assert!(!mica_allowed(22000, Some(1), None), "Win11 21H2 不上");
        assert!(!mica_allowed(19045, Some(1), None), "Win10 不上");
        assert!(!mica_allowed(0, Some(1), None), "读不到 build 不上");
        assert!(!mica_allowed(26200, Some(0), None), "用户关了透明效果");
        assert!(!mica_allowed(26200, Some(1), Some(127)), "高对比度开着");
    }

    #[test]
    fn 环境变量开关_非空且不是_0_就强制回退() {
        assert!(kill_switch_on(Some("1")));
        assert!(kill_switch_on(Some("true")));
        assert!(!kill_switch_on(Some("0")));
        assert!(!kill_switch_on(Some("  ")));
        assert!(!kill_switch_on(None));
    }

    #[test]
    fn 确认框与灵动岛不要材质() {
        assert!(!wants_material("main"));
        assert!(!wants_material("confirm-3"));
        assert_eq!(decide("main"), Material::None);
        assert_eq!(decide("confirm-3"), Material::None);
        if cfg!(any(target_os = "windows", target_os = "macos")) {
            assert!(wants_material("chat") && wants_material("about"));
        }
        if cfg!(target_os = "windows") {
            assert!(
                wants_material("onboarding")
                    && wants_material("updater")
                    && wants_material("new-session")
            );
        }
    }

    #[test]
    fn 没有材质时不给窗口效果_由调用方设不透明底色() {
        assert!(effects_config(Material::None, true).is_none());
        assert_eq!(Material::None.as_str(), "none");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn mica_按应用主题选明暗() {
        use tauri::utils::WindowEffect;
        let dark = effects_config(Material::Mica, true).unwrap();
        let light = effects_config(Material::Mica, false).unwrap();
        assert_eq!(dark.effects, vec![WindowEffect::MicaDark]);
        assert_eq!(light.effects, vec![WindowEffect::MicaLight]);
        assert_eq!(Material::Mica.as_str(), "mica");
    }

    #[test]
    fn 背板明暗_跟随系统时看系统_固定主题时不被系统带跑() {
        // system：系统切到哪边背板就翻到哪边。
        assert!(backdrop_dark("system", true));
        assert!(!backdrop_dark("system", false));
        // 固定深色：系统切浅后仍要纠正回深色。
        assert!(backdrop_dark("dark", true));
        assert!(backdrop_dark("dark", false));
        // 固定浅色：系统切深后仍要纠正回浅色。
        assert!(!backdrop_dark("light", true));
        assert!(!backdrop_dark("light", false));
        // 认不出的值按跟随系统算，与 window::theme_is_dark 的兜底分档一致。
        assert!(backdrop_dark("", true));
        assert!(!backdrop_dark("auto", false));
    }

    #[test]
    fn 从系统版本文件取_macos_主版本号() {
        let plist = "<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t\
                     <key>ProductVersion</key>\n\t<string>26.1</string>\n</dict>";
        assert_eq!(macos_major_version(plist), Some(26));
        assert_eq!(
            macos_major_version("<key>ProductVersion</key><string>15.7.2</string>"),
            Some(15)
        );
        assert_eq!(macos_major_version("<dict></dict>"), None);
        assert_eq!(LIQUID_GLASS_MIN_MACOS, 26);
    }
}
