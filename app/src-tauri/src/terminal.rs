//! 终端集成：定位并聚焦会话所在的终端标签页（Windows UIA+Win32 / macOS AppleScript），
//! 以及在指定目录拉起 resume / 新建会话的终端进程。从 lib.rs 抽出。

use crate::proc::*;
use crate::settings::load_settings;
use crate::watch::emit_board_changed;
#[cfg(target_os = "windows")]
use crate::wezterm;
use crate::{db_path, is_safe_id, now_ms, open_store};
#[cfg(target_os = "windows")]
use std::collections::HashSet;
use std::path::PathBuf;

/// 点击连接中会话后的实际定位结果。前端必须区分“会话已断开”和“进程仍在、但终端无法定位”，
/// 否则后者会表现成毫无反应，用户还会误以为重启 Meowo 能解决。
#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // macOS 专属失败原因在 Windows 构建中不会构造，反之亦然。
pub(crate) enum FocusSessionResult {
    Focused,
    HostFocused,
    AliveButNotFound,
    PermissionDenied,
    UnsupportedTerminal,
    ProcessEnded,
}

/// 枚举可见顶层窗口，返回第一个进程 pid 命中 targets 的窗口 HWND。
#[cfg(target_os = "windows")]
pub(crate) fn find_window_for_pids(
    targets: &HashSet<u32>,
) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Ctx<'a> {
        targets: &'a HashSet<u32>,
        found: Option<HWND>,
    }

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut Ctx);
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let mut wpid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if ctx.targets.contains(&wpid) {
            ctx.found = Some(hwnd);
            return 0; // FALSE：停止枚举
        }
        TRUE
    }

    let mut ctx = Ctx {
        targets,
        found: None,
    };
    unsafe {
        EnumWindows(Some(cb), &mut ctx as *mut Ctx as LPARAM);
    }
    ctx.found
}

/// 用纯 Win32 EnumWindows+GetClassNameW 收集所有可见的 Windows Terminal 顶层窗口 HWND(as isize)。
/// 替代 UIA matcher 从桌面根逐节点跨进程爬树找窗口——后者默认 depth=7、每访问一个元素一次
/// CurrentClassName RPC，几十~上百窗口累计可达数百 ms；本函数纯进程内，微秒级。
#[cfg(target_os = "windows")]
pub(crate) fn enum_wt_hwnds() -> Vec<isize> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, IsWindowVisible,
    };

    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return TRUE;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if len > 0 {
            let cls = String::from_utf16_lossy(&buf[..len as usize]);
            if cls == "CASCADIA_HOSTING_WINDOW_CLASS" {
                let out = &mut *(lparam as *mut Vec<isize>);
                out.push(hwnd as isize);
            }
        }
        TRUE
    }

    let mut out: Vec<isize> = Vec::new();
    unsafe {
        EnumWindows(Some(cb), &mut out as *mut Vec<isize> as LPARAM);
    }
    out
}

/// claude 会把任务标题写进 Windows Terminal 标签页，并加一个**会随状态变化**的前缀符号：
/// 运行时是 braille spinner(⠐⠂…)，空闲/待输入时是 ✳(U+2733)，可能还有其它符号。
/// 归一化：剥掉开头所有「非字母数字」字符（覆盖任意状态符号 + 空格；任务标题几乎总以
/// 字母/数字/CJK 开头），并去掉尾部空白与截断省略号(…/...)。纯函数，便于单测。
#[allow(dead_code)] // 跨平台纯函数：Windows 上 WT/WezTerm 聚焦共用，非 Windows 仅单测使用
pub(crate) fn normalize_tab_title(s: &str) -> &str {
    s.trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end()
        .trim_end_matches(['…', '.'])
        .trim_end()
}

/// 标签页标题 `tab_name` 与会话标题 `want` 的匹配强度：2=精确(归一化后相等)，1=单向包含，0=不匹配。
/// 包含是**双向**的：兼容 claude 对长标题的截断(tab 标题是 want 的前缀)与轻微漂移。
/// `want` 为空或占位("(未命名会话)")时不参与匹配(返回 0)，避免误命中无关标签页。纯函数。
#[allow(dead_code)] // 同上：Windows 上 WT/WezTerm 聚焦共用，非 Windows 仅单测调用
pub(crate) fn tab_match_score(tab_name: &str, want: &str) -> u8 {
    let want = want.trim();
    if want.is_empty() || want == "(未命名会话)" {
        return 0;
    }
    let norm = normalize_tab_title(tab_name);
    if norm.is_empty() {
        return 0;
    }
    if norm == want {
        2
    } else if norm.contains(want) || want.contains(norm) {
        1
    } else {
        0
    }
}

/// 用 UI Automation 把对应会话的 Windows Terminal 标签页切到前台。
///
/// WT 单进程托管多标签/多窗口，按进程 PID 无法区分标签页（所有标签页同一个 HWND）。
/// 但 claude 会把任务标题写进标签页标题，故按标题精确定位标签页：枚举所有 WT 窗口的
/// TabItem，取匹配分最高的标签页，`Select` 选中后置前其窗口。命中返回 true；失败/无匹配返回 false。
///
/// 性能：仅当出现「多个同分标签页」需要消歧时，才用 `console_group_pids(root_pid)` 做一次进程扫描
/// (昂贵，要枚举系统所有进程)；常见的唯一精确匹配走纯 UIA 路径(~十几 ms)，不扫进程。
///
/// 注意：本函数必须在「干净 COM apartment 的线程」上调用（见 `focus_session` 的后台线程）。
/// `UIAutomation::new()` 会 CoInitialize 当前线程，Tauri 主线程已是 STA，复用会因 apartment 冲突失败。
#[cfg(target_os = "windows")]
pub(crate) fn focus_terminal_tab(root_pid: u32, want: &str, token: Option<&str>) -> bool {
    use uiautomation::patterns::UISelectionItemPattern;
    use uiautomation::types::{ControlType, Handle, TreeScope, UIProperty};
    use uiautomation::variants::Variant;
    use uiautomation::{UIAutomation, UIElement};

    let Ok(automation) = UIAutomation::new() else {
        return false;
    };

    // WT 顶层窗口：先用纯 Win32 EnumWindows+GetClassNameW 直接拿 HWND（进程内、微秒级），再
    // element_from_handle 只进入这几个窗口做 UIA。绕开 crate matcher 从桌面根逐节点 RPC 爬树
    // （默认 depth=7、每节点一次 CurrentClassName 跨进程调用，几十~上百窗口下可达 50-300ms）。
    // 保留 HWND 与 UIElement 配对：HWND 用于 GetWindowThreadProcessId 取窗口 pid（消歧用）与置前，
    // UIElement 用于 UIA 枚举标签页。
    let wt_windows: Vec<(isize, UIElement)> = enum_wt_hwnds()
        .into_iter()
        .filter_map(|h| {
            automation
                .element_from_handle(Handle::from(h))
                .ok()
                .map(|el| (h, el))
        })
        .collect();
    if wt_windows.is_empty() {
        return false;
    }

    // 标签页条件(TabItem)；其容器条件(TabView=ControlType::Tab)用于把搜索根收窄到标签条子树。
    let Ok(tab_cond) = automation.create_property_condition(
        UIProperty::ControlType,
        Variant::from(ControlType::TabItem as i32),
        None,
    ) else {
        return false;
    };
    let tabview_cond = automation
        .create_property_condition(
            UIProperty::ControlType,
            Variant::from(ControlType::Tab as i32),
            None,
        )
        .ok();
    // 缓存请求：让 FindAll 随元素一次性带回 Name，用 get_cached_name 读取，免每个 TabItem 一次
    // CurrentName 跨进程 RPC。
    let cache_req = automation.create_cache_request().ok();
    if let Some(ref cr) = cache_req {
        let _ = cr.add_property(UIProperty::Name);
    }

    // 取某 WT 窗口的 (TabItem, name) 列表。关键提速：先 find_first 定位 TabView 容器(ControlType::Tab，
    // 命中即停)，把 FindAll 的根从整窗收窄到标签条子树——避免对整窗 Descendants 全扫(含终端内容面板，
    // 实测每窗口 ~20ms)。容器内优先直接子(Children)，拿不到再容器 Descendants(兼容 TabItem 嵌套)；
    // 连容器都没有才退化为整窗 Descendants(异常布局兜底)。name 优先走缓存(get_cached_name)。
    let collect_tabs = |win: &UIElement| -> Vec<(UIElement, String)> {
        let find_tabitems = |root: &UIElement, scope: TreeScope| -> Vec<UIElement> {
            match &cache_req {
                Some(cr) => root
                    .find_all_build_cache(scope, &tab_cond, cr)
                    .unwrap_or_default(),
                None => root.find_all(scope, &tab_cond).unwrap_or_default(),
            }
        };
        let mut tabs: Vec<UIElement> = Vec::new();
        if let Some(tv) = tabview_cond
            .as_ref()
            .and_then(|c| win.find_first(TreeScope::Descendants, c).ok())
        {
            tabs = find_tabitems(&tv, TreeScope::Children);
            if tabs.is_empty() {
                tabs = find_tabitems(&tv, TreeScope::Descendants);
            }
        }
        if tabs.is_empty() {
            tabs = find_tabitems(win, TreeScope::Descendants);
        }
        tabs.into_iter()
            .map(|t| {
                let name = if cache_req.is_some() {
                    t.get_cached_name()
                        .or_else(|_| t.get_name())
                        .unwrap_or_default()
                } else {
                    t.get_name().unwrap_or_default()
                };
                (t, name)
            })
            .collect()
    };

    // 收集所有命中标签页：(匹配分, 窗口 HWND, 窗口 pid, 标签元素)。【不短路】——同一标题在多个窗口/标签
    // 出现时，按 console_group_pids(root_pid) 消歧到本会话所属窗口，否则会聚焦到错的同名标签。
    // want 来源因 agent 而异：claude/kimi=任务标题（kimi 另配 token 精确）、codex=cwd 末段目录名
    // (匹配 codex 自己写的 project-name 标签标题)。单个会话即精确命中；多个同名标签退窗口级。
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    let mut matches: Vec<(u8, isize, u32, UIElement)> = Vec::new();
    for (hwnd, win) in &wt_windows {
        let mut win_pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(*hwnd as HWND, &mut win_pid);
        }
        for (tab, name) in collect_tabs(win) {
            // token(=session_id 末 8 位，meowo-reporter 写进 kimi 标签) 命中即最高优先级 3、全局唯一——
            // 压倒按标题的语义匹配，且无需进程组消歧。否则退回标题匹配(0-2，含 codex 的 project-name)。
            let score = match token {
                Some(t) if !t.is_empty() && name.contains(t) => 3,
                _ => tab_match_score(&name, want),
            };
            if score > 0 {
                matches.push((score, *hwnd, win_pid, tab));
            }
        }
    }
    let max_score = matches.iter().map(|m| m.0).max().unwrap_or(0);
    if max_score == 0 {
        return false;
    }
    // 只保留最高分候选。
    matches.retain(|m| m.0 == max_score);
    // 唯一候选直接用；多个同分时按 console_group_pids(root_pid) 选与本会话同进程组的窗口（窗口宿主
    // WindowsTerminal.exe 是本会话进程的祖先，故其 pid 落在进程组里）——修「两个同名终端点击跳错」。
    // 选出本会话所属窗口(进程组含其窗口 pid)的候选。同一窗口里多个同名标签无法区分（UIA 不暴露
    // tab→进程），此时【不猜】——返回 false 让上层走窗口级定位，避免切到错的同名标签
    // （如 codex/kimi 同在某目录、标签都显示该目录名时，点哪个都别误切到另一个）。
    let idx = if matches.len() == 1 {
        0
    } else {
        let group = console_group_pids(root_pid);
        let in_group: Vec<usize> = (0..matches.len())
            .filter(|&i| group.contains(&matches[i].2))
            .collect();
        match in_group.as_slice() {
            [i] => *i,         // 唯一属于本会话窗口的候选 → 精确命中
            _ => return false, // 0 个或多个(同窗口多同名标签) → 不猜，退回窗口级
        }
    };
    let (_, hwnd, _, tab) = &matches[idx];
    // 选中该标签页（即使其窗口当前在后台也会切换激活标签页），再置前其窗口（直接用 HWND，免再取 native handle）。
    if let Ok(p) = tab.get_pattern::<UISelectionItemPattern>() {
        let _ = p.select();
    }
    force_foreground(*hwnd as HWND);
    true
}

/// 用 AttachThreadInput 绕过 Windows 后台进程 SetForegroundWindow 限制，可靠置顶目标窗口。
#[cfg(target_os = "windows")]
pub(crate) fn force_foreground(hwnd: windows_sys::Win32::Foundation::HWND) {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };
    unsafe {
        let target_thread = GetWindowThreadProcessId(hwnd, null_mut());
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, null_mut())
        };
        let cur = GetCurrentThreadId();

        if fg_thread != 0 && fg_thread != cur {
            AttachThreadInput(cur, fg_thread, 1);
        }
        if target_thread != 0 && target_thread != cur {
            AttachThreadInput(cur, target_thread, 1);
        }

        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);

        if target_thread != 0 && target_thread != cur {
            AttachThreadInput(cur, target_thread, 0);
        }
        if fg_thread != 0 && fg_thread != cur {
            AttachThreadInput(cur, fg_thread, 0);
        }
    }
}

/// 聚焦某会话的终端。`title_based`=该 agent 是否把任务标题写进 WT 标签（claude 写→按任务标题精确切标签；
/// codex 不写→改用 cwd 末段目录名匹配它的 project-name 标签）。无论哪种，最终都能按进程组
/// 找到宿主窗口置前。
/// 必须在后台线程调用（保证干净 COM apartment + 不阻塞调用方）。返回实际定位结果；
/// focus_session 会把结果交给贴纸提示，「点击通知」回调则忽略结果。仅 Windows。
#[cfg(target_os = "windows")]
pub(crate) fn focus_session_terminal(
    pid: i64,
    title: Option<String>,
    cwd: Option<String>,
    token: Option<String>,
    title_based: bool,
) -> FocusSessionResult {
    // 匹配 WT 标签优先级：token(session_id 末 8 位，仅 kimi：meowo-reporter 写进其标签)
    // > 任务标题(claude/kimi)；codex 使用 cwd 末段目录名；最后才做窗口级兜底。
    // token 全局唯一，能区分同窗口同目录的同名标签——这是 kimi 精确聚焦的关键；codex 暂无此手段(见 agent.rs)。
    let want = if title_based {
        title
    } else {
        cwd_tab_hint(cwd.as_deref())
    };
    let want_str = want.as_deref().unwrap_or("");
    let has_token = token.as_deref().is_some_and(|t| !t.is_empty());
    if (!want_str.is_empty() || has_token)
        && focus_terminal_tab(pid as u32, want_str, token.as_deref())
    {
        return FocusSessionResult::Focused;
    }
    // 兜底：按进程组找宿主顶层窗口置前（命中正确窗口，但不保证切到具体标签）。宿主
    // WindowsTerminal.exe/conhost 是会话进程的祖先，其窗口 pid 落在进程组里 → 可靠命中正确窗口。
    let targets = console_group_pids(pid as u32);
    // WezTerm 宿主：自绘 GUI 无 UIA TabItem，上面的 WT 标签定位必然不中；组内探到
    // wezterm-gui 就走 wezterm cli 精确切 pane(内含窗口置前)，不再落通用兜底。
    match wezterm::focus_pane(&targets, want_str, token.as_deref(), cwd.as_deref()) {
        wezterm::FocusPaneResult::Focused => return FocusSessionResult::Focused,
        wezterm::FocusPaneResult::HostFocused => return FocusSessionResult::HostFocused,
        wezterm::FocusPaneResult::NotWezTerm => {}
    }
    if let Some(hwnd) = find_window_for_pids(&targets) {
        force_foreground(hwnd);
        return FocusSessionResult::HostFocused;
    }
    FocusSessionResult::UnsupportedTerminal
}

/// 从 cwd 取末段目录名，作为「不写标签标题」的 agent(codex) 的 WT 标签匹配线索——这类会话的
/// 标签默认显示当前目录名。空/根目录返回 None（退回窗口级定位）。
#[cfg(target_os = "windows")]
pub(crate) fn cwd_tab_hint(cwd: Option<&str>) -> Option<String> {
    let c = cwd?.trim_end_matches(['/', '\\']);
    std::path::Path::new(c)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// iTerm2 是否安装（任意常见位置）：先查标准路径，再用 mdfind 按 bundle id 兜底。
#[cfg(target_os = "macos")]
pub(crate) fn iterm_installed() -> bool {
    use std::path::Path;
    if Path::new("/Applications/iTerm.app").exists() {
        return true;
    }
    if let Ok(home) = std::env::var("HOME") {
        if Path::new(&home).join("Applications/iTerm.app").exists() {
            return true;
        }
    }
    std::process::Command::new("mdfind")
        .arg("kMDItemCFBundleIdentifier == 'com.googlecode.iterm2'")
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// 读设置得出「打开未连接会话」用的终端宿主（macOS）。缺省 Terminal.app；
/// 选了 iTerm2 但未安装时回退 Terminal.app（避免 AppleScript 静默失败）。
#[cfg(target_os = "macos")]
pub(crate) fn resume_terminal_kind() -> crate::term_script::TermKind {
    use crate::term_script::TermKind;
    match crate::term_script::resume_kind_from_setting(&load_settings().resume_terminal) {
        TermKind::ITerm2 if iterm_installed() => TermKind::ITerm2,
        TermKind::ITerm2 => TermKind::Terminal,
        other => other,
    }
}

/// 聚焦终端时的 resume 回退命令：按 provider 分发（不再硬编码 claude）。是否真的回退由
/// `focus_session_terminal` 校验进程死活后决定（进程存活时绝不 resume，防 fork 重复会话）。
/// 未知 agent、或该 agent 未声明 resume 子命令 → 空 argv：只聚焦终端，不回退 resume。
///
/// **刻意定义在 `cfg` 之外**：它只用平台无关的 agent API，目前仅 macOS 调用。若把它埋进
/// `#[cfg(target_os = "macos")]` 块里，Windows 上的编译器根本不会看它——Phase 2 改了
/// `resume_args` 的签名，正是这样一路漏到 macOS CI 才炸的。逻辑留在 cfg 外，cfg 块里只放
/// 平台专属的 API 调用。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn resume_argv_for(provider: Option<&str>, session_id: Option<&str>) -> Vec<String> {
    session_id
        .zip(meowo_agent::resolve(provider))
        .and_then(|(session_id, a)| {
            let sub = a.resume_args();
            if sub.is_empty() {
                return None;
            }
            let mut argv = crate::relay::augment_argv(a.id(), a.launch_argv());
            argv.extend(sub.iter().map(|s| s.to_string()));
            argv.push(session_id.to_string());
            Some(argv)
        })
        .unwrap_or_default()
}

/// 拉起该 provider 时要注入的代理环境变量。未知 agent → 空（不注入）。
///
/// claude 也返回空——它的代理已写进 settings.json 的 `env` 块，对所有启动方式生效，无须再注入。
/// 详见 [`crate::proxy::launch_env`]。
///
/// 与 `resume_argv_for` 同理**刻意定义在 cfg 之外**：只用平台无关的 agent API，
/// 埋进 cfg 块会让另一平台的编译器看不到它，改签名时一路漏到对方 CI 才炸。
/// **恢复某个会话**时要注入的环境变量：代理 + 该会话**自己所属账号**的隔离变量。
///
/// 刻意不用「当前活跃账号」：用户切到账号 B 之后再打开一个属于账号 A 的旧会话，若按 B 注入，
/// 就是拿错误的身份去续一段不属于它的对话——而且不会有任何报错。
pub(crate) fn launch_env_for_session(
    provider: Option<&str>,
    session_id: &str,
) -> Vec<(String, String)> {
    let profile = profile_of_session(session_id);
    launch_env_for_profile(provider, profile.as_deref())
}

/// 该会话（按 agent 的 session id）跑在哪个账号上。查不到 → None（默认账号）。
fn profile_of_session(session_id: &str) -> Option<String> {
    let store = meowo_store::Store::open(crate::db_path()).ok()?;
    let sid = store.find_session_id_pub(session_id).ok()??;
    store.session_profile(sid).ok().flatten()
}

fn ensure_session_profile_available(provider: &str, session_id: &str) -> Result<(), String> {
    let Some(profile) = profile_of_session(session_id) else {
        return Ok(());
    };
    let agent = meowo_agent::resolve(Some(provider)).ok_or("未知 agent")?;
    validate_session_profile_reference(
        Some(&profile),
        crate::profile::exists(agent.id().as_str(), &profile),
    )
}

fn validate_session_profile_reference(profile: Option<&str>, exists: bool) -> Result<(), String> {
    match profile {
        None => Ok(()),
        Some(_) if exists => Ok(()),
        Some(profile) => Err(format!("该会话所属账号“{profile}”已被删除，无法恢复")),
    }
}

#[cfg(test)]
mod session_profile_tests {
    use super::validate_session_profile_reference;

    #[test]
    fn deleted_profile_blocks_resume_but_default_profile_does_not() {
        assert!(validate_session_profile_reference(None, false).is_ok());
        assert!(validate_session_profile_reference(Some("work"), true).is_ok());
        let error = validate_session_profile_reference(Some("deleted"), false).unwrap_err();
        assert!(error.contains("deleted"));
        assert!(error.contains("无法恢复"));
    }
}

/// 同上，但指定账号（profile）。`profile = None` → 用该 agent **当前活跃**的账号。
pub(crate) fn launch_env_for_profile(
    provider: Option<&str>,
    profile: Option<&str>,
) -> Vec<(String, String)> {
    let Some(a) = meowo_agent::resolve(provider) else {
        return Vec::new();
    };
    let mut env = crate::proxy::launch_env(a.id());
    // 中转接入（relay）的环境变量：API base / key。与账号隔离变量正交，两者都要。
    env.extend(crate::relay::launch_env(a.id()));
    let id = match profile {
        Some(p) => Some(p.to_string()),
        None => crate::profile::active_id(a.id().as_str()),
    };
    env.extend(crate::profile::env_of(a.id(), id.as_deref()));
    env
}

#[tauri::command]
pub(crate) async fn focus_session(
    pid: i64,
    title: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    provider: Option<String>,
) -> Result<FocusSessionResult, String> {
    if pid <= 0 {
        return Err("无效 pid".into());
    }
    // session_id 经 is_safe_id 校验（仅 `[A-Za-z0-9_-]`，杜绝注入：macOS 分支会把 id 注入 AppleScript）。
    // 必须用宽松校验——kimi 的 `session_<uuid>` 不合 UUID 形态，用严格 is_session_id 会把连接态的
    // kimi 卡挡在定位之前（Windows 上 session_id 实际并不参与 focus，仅 pid+title）。
    if let Some(id) = session_id.as_deref() {
        if !is_safe_id(id) {
            return Err("无效 session_id".into());
        }
        // 前端列表可能已过期，PID 也可能被同类 Agent 进程复用。只校验“这个 PID 是 Agent”不足以
        // 证明它仍属于用户点击的会话；必须与 DB 当前绑定一致，避免精准打开跳到另一会话。
        let id = id.to_string();
        let owns_pid = tauri::async_runtime::spawn_blocking(move || {
            let store = open_store(&db_path())?;
            let Some(sid) = store.find_session_id_pub(&id).map_err(|e| e.to_string())? else {
                return Ok(false);
            };
            store
                .session_pid(sid)
                .map(|bound| bound == Some(pid))
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        if !owns_pid {
            return Ok(FocusSessionResult::ProcessEnded);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if !pid_alive_agent_quick(pid) {
            return Ok(FocusSessionResult::ProcessEnded);
        }
        // 该 provider 是否把任务标题写进 WT 标签：决定按标题切标签还是按 cwd 目录名切标签。
        // 缺省(None)→默认 agent；未知 agent → false，走窗口级定位兜底（不按标题瞎切标签）。
        let title_based =
            meowo_agent::resolve(provider.as_deref()).is_some_and(|a| a.sets_terminal_tab_title());
        // 只有声明 writes_tab_token 的 agent 才拿 session token 匹配；盲传 sid8 可能偶然命中
        // 别的标签文本，并以最高分跳错会话。Codex 首条消息后会覆盖 token，此时自然回退 cwd 匹配。
        let token = meowo_agent::resolve(provider.as_deref())
            .filter(|a| a.writes_tab_token())
            .and(session_id.as_deref())
            .map(meowo_reporter::tabtitle::short_sid)
            .filter(|s| !s.is_empty());
        tauri::async_runtime::spawn_blocking(move || {
            focus_session_terminal(pid, title, cwd, token, title_based)
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = provider;
    #[cfg(target_os = "macos")]
    {
        let _ = title;
        if !pid_alive_agent_quick(pid) {
            return Ok(FocusSessionResult::ProcessEnded);
        }
        // ps/osascript（含首次 TCC 授权弹窗）可能长时间阻塞，放 blocking 池，不挡主线程事件循环；
        // 与旧 fire-and-forget 不同，这里 await 结果，让贴纸能解释“为什么没有跳转”。
        tauri::async_runtime::spawn_blocking(move || {
            let resume_argv = resume_argv_for(provider.as_deref(), session_id.as_deref());
            // 保留恢复参数供聚焦期间进程退出的判定路径使用；实际恢复改由前端明确确认。
            // 账号按**该会话自己的**取（没有 session_id 就退回当前活跃账号）。
            let env = match session_id.as_deref() {
                Some(sid) => launch_env_for_session(provider.as_deref(), sid),
                None => launch_env_for_profile(provider.as_deref(), None),
            };
            let env_prefix = env_prefix_posix(&env);
            crate::macos::terminal::focus_session_terminal(
                pid,
                cwd.as_deref(),
                &resume_argv,
                resume_terminal_kind(),
                &env_prefix,
            )
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (pid, title, cwd, session_id);
        Err("当前平台不支持".into())
    }
}

/// 在系统文件管理器中打开会话的项目目录（卡片右键菜单用）。
/// 目录须真实存在——DB 记录的 cwd 可能过期（项目被移动/删除），不存在时明确报错而非静默无事发生。
/// 不经 shell 直接 spawn 文件管理器，目录路径作为独立 argv 传入，无注入面。
#[tauri::command]
pub(crate) fn open_project_dir(cwd: String) -> Result<(), String> {
    let dir = cwd.trim();
    if dir.is_empty() || !std::path::Path::new(dir).is_dir() {
        return Err("目录不存在".into());
    }
    #[cfg(target_os = "windows")]
    {
        // kimi 等 provider 写入的 cwd 可能是正斜杠形式，explorer 对正斜杠路径会打开默认目录而非目标。
        let dir = dir.replace('/', "\\");
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    // macOS：open 偶发慢（Finder 冷启动），放后台线程；status() 等待回收，避免僵尸进程。
    #[cfg(target_os = "macos")]
    {
        let dir = dir.to_string();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("open").arg(&dir).status();
        });
    }
    Ok(())
}

/// 把 `cwd` 收敛成「可安全传给 wt -d」的目录：必须非空、真实存在的目录，且不含会破坏 wt
/// 命令行解析的元字符(`;` `"`)。不满足则返回 None（调用方退化为不带 -d）。
/// 在 PATH 各目录中查找指定文件是否存在。不 spawn `where` 子进程——GUI 进程冷启动后
/// 首次 spawn 控制台子进程要数秒（新建 conhost + 杀软扫描），而同步命令跑在主线程，
/// 会把整个事件循环（所有窗口）堵死，这正是 0.2.0 设置页在 Windows 上"卡死"的根因。
/// 用 symlink_metadata 而非 exists()：wt.exe 通常是 App Execution Alias
/// （APPEXECLINK reparse point），fs::metadata 跟随它会失败、误判为不存在。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn path_has_exe(path_var: &std::ffi::OsStr, exe: &str) -> bool {
    std::env::split_paths(path_var).any(|dir| dir.join(exe).symlink_metadata().is_ok())
}

/// Windows Terminal（wt.exe）是否在 PATH 上。进程内缓存：安装状态运行期间基本不变，
/// resume_session 每次恢复会话都要查询，保持微秒级。
#[cfg(target_os = "windows")]
pub(crate) fn wt_available() -> bool {
    use std::sync::OnceLock;
    static WT_ON_PATH: OnceLock<bool> = OnceLock::new();
    *WT_ON_PATH.get_or_init(|| std::env::var_os("PATH").is_some_and(|p| path_has_exe(&p, "wt.exe")))
}

/// PowerShell 7（pwsh.exe）是否在 PATH 上。进程内缓存，同 wt_available。
/// 一键安装用它优先于 Windows PowerShell 5.1（见 build_install_command 说明）。
#[cfg(target_os = "windows")]
pub(crate) fn pwsh_available() -> bool {
    use std::sync::OnceLock;
    static PWSH_ON_PATH: OnceLock<bool> = OnceLock::new();
    *PWSH_ON_PATH
        .get_or_init(|| std::env::var_os("PATH").is_some_and(|p| path_has_exe(&p, "pwsh.exe")))
}

/// 定位 Windows Terminal 的 settings.json（Store 版 / Preview / 未打包版三处）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn wt_settings_path() -> Option<PathBuf> {
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    [
        r"Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json",
        r"Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json",
        r"Microsoft\Windows Terminal\settings.json",
    ]
    .into_iter()
    .map(|rel| base.join(rel))
    .find(|p| p.is_file())
}

/// 去掉 JSONC 注释（WT settings.json 允许 // 与 /* */，且字符串里常有 URL 的 //）。
/// 按字节扫描、正确跳过字符串与转义，不破坏多字节 UTF-8（profile 名可能含中文）。纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn strip_jsonc_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]); // 保留转义字符，避免把 \" 误判为字符串结束
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
        } else if c == b'"' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            i += 2;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| src.to_string())
}

/// 从 WT settings.json 的 JSON 取默认 profile 名：defaultProfile 为 GUID 时在 profiles.list
/// 按 guid 找 name（大小写不敏感）；本身是名字则直接用。找不到则 None。纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn parse_wt_default_profile(v: &serde_json::Value) -> Option<String> {
    let def = v.get("defaultProfile").and_then(|x| x.as_str())?.trim();
    if def.is_empty() {
        return None;
    }
    if !def.starts_with('{') {
        return Some(def.to_string()); // 直接配的是 profile 名
    }
    // 新格式 profiles.list 是数组；老格式 profiles 直接是数组。
    let list = v.get("profiles").and_then(|p| {
        p.get("list")
            .and_then(|l| l.as_array())
            .or_else(|| p.as_array())
    })?;
    list.iter().find_map(|prof| {
        let guid = prof.get("guid").and_then(|g| g.as_str())?;
        guid.eq_ignore_ascii_case(def)
            .then(|| {
                prof.get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
            })
            .flatten()
    })
}

/// 用户 WT 默认 profile 名（多为 PowerShell）。进程内缓存：与 wt_available 一致，运行期基本不变
/// （改了默认 profile 需重启 app 才生效）。读不到/解析失败/无匹配 → None，调用方退化为不带 -p。
#[cfg(target_os = "windows")]
pub(crate) fn wt_default_profile() -> Option<String> {
    use std::sync::OnceLock;
    static PROFILE: OnceLock<Option<String>> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            let raw = std::fs::read_to_string(wt_settings_path()?).ok()?;
            let v: serde_json::Value = serde_json::from_str(&strip_jsonc_comments(&raw)).ok()?;
            parse_wt_default_profile(&v)
        })
        .clone()
}

#[cfg(target_os = "windows")]
pub(crate) fn safe_cwd(cwd: Option<&str>) -> Option<String> {
    let d = cwd?.trim();
    // 含 ; " 会破坏命令行解析；以 - 开头会被 wt 当成选项（真实 Windows 路径不会以 - 开头）。
    if d.is_empty() || d.contains([';', '"']) || d.starts_with('-') {
        return None;
    }
    std::path::Path::new(d).is_dir().then(|| d.to_string())
}

/// 把 resume 命令 argv 拼成交给 `powershell -Command` / `cmd /k` 的单行命令串。
/// kimi/codex 的可执行是 USERPROFILE 下的绝对路径，用户名可含空格 / $ / ' / % 等合法字符：
/// - PowerShell：含空白或 $ ` ' 的参数用**单引号字面量**包裹（内嵌单引号翻倍）——双引号内 $ 与反引号
///   仍会被插值展开（如 C:\Users\a$b 被吞成 C:\Users\a），单引号内一切按字面处理；带引号的命令路径
///   需以调用运算符 `&` 前缀。
/// - cmd：含空白的参数加双引号。cmd 没有字面量引用机制，引号内成对的 %VAR% 仍会展开——属 cmd 本身
///   限制，用户名含 % 的机器请改用 wt/powershell（此处不做 ^ 转义：引号内 ^ 会按字面残留）。
///
/// 纯函数便于单测。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn shell_join_for_windows(args: &[String], powershell: bool) -> String {
    if powershell {
        let quoted: Vec<String> = args
            .iter()
            .map(|a| {
                if a.chars().any(char::is_whitespace)
                    || a.contains([
                        '$', '`', '\'', '"', '&', ';', '|', '<', '>', '(', ')', '{', '}',
                    ])
                {
                    format!("'{}'", a.replace('\'', "''"))
                } else {
                    a.clone()
                }
            })
            .collect();
        let joined = quoted.join(" ");
        if quoted.first().is_some_and(|f| f.starts_with('\'')) {
            format!("& {joined}")
        } else {
            joined
        }
    } else {
        args.iter()
            .map(|a| {
                if a.chars().any(char::is_whitespace) {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

// ═══ 代理环境变量的注入前缀 ═══
//
// codex / kimi 没法从配置文件配代理（见 meowo_agent::proxy 的能力表），只认进程环境变量。而
// `Command::env()` 在这里**靠不住**：wt 会把请求交给**已存在的** Windows Terminal 实例去开标签、
// wezterm 交给 mux server、macOS 的 Terminal.app 更是早就在跑了——新进程都不是我们的子进程，
// 继承不到我们设的 env。唯一可靠的办法是把赋值**写进命令串**本身。
//
// 于是代理串（用户填的）会进到 shell 命令里 → 三种 shell 各自的转义必须做对，否则就是注入面。
// 值虽已过 validate（无空格、协议白名单、host/port 合法），仍按「一律正确转义」处理，不赌。

/// 所有启动路径都先清掉继承的代理变量。否则「直连」只是没有新增变量，仍会继承 Meowo
/// 自己启动时的 HTTPS_PROXY / ALL_PROXY；自定义 HTTP 代理也可能被旧的 ALL_PROXY 抢走。
pub(crate) const PROXY_ENV_KEYS: [&str; 8] = [
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "https_proxy",
    "http_proxy",
    "all_proxy",
    "no_proxy",
];

/// PowerShell：先 `$env:K=$null; `，再 `$env:K='v'; `。单引号字面量内一切按字面处理（双引号内 `$`/反引号会插值），
/// 内嵌单引号翻倍转义。与 `shell_join_for_windows` 的引用规则同源。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn env_prefix_powershell(env: &[(String, String)]) -> String {
    let clear: String = PROXY_ENV_KEYS
        .iter()
        .map(|k| format!("$env:{k}=$null; "))
        .collect();
    let set: String = env
        .iter()
        .map(|(k, v)| format!("$env:{k}='{}'; ", v.replace('\'', "''")))
        .collect();
    format!("{clear}{set}")
}

/// POSIX：`K='v' ` —— 命令前缀式赋值（只作用于这一条命令，无需 export）。
///
/// **键名必须不加引号**，否则 shell 不再把它识别为赋值（`'K=v' cmd` 会被当成一个命令名）——
/// 这正是不能把 `K=v` 当作一个 argv 项塞进 AppleScript 的原因（那边每项都套了 `quoted form`）。
/// 键名是我们自己的常量（安全），值按 POSIX 单引号规则转义（`'` → `'\''`）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn env_prefix_posix(env: &[(String, String)]) -> String {
    let clear = format!("unset {}; ", PROXY_ENV_KEYS.join(" "));
    let set: String = env
        .iter()
        .map(|(k, v)| format!("{k}='{}' ", v.replace('\'', r"'\''")))
        .collect();
    format!("{clear}{set}")
}

/// 单 pid 判活（廉价版，resume 前奏专用）：Windows 走 Toolhelp 快照（1-3ms，避免 sysinfo 全进程
/// OpenProcess 刷新的 30-120ms 拖慢「点下即显示已连接」），Unix 走一次 ps。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn pid_alive_agent_quick(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(target_os = "windows")]
    {
        snapshot_processes()
            .get(&(pid as u32))
            .map(|(_, name)| meowo_agent::is_agent_process(name))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        pid_is_agent_ps(pid)
    }
}

/// resume 的跨平台前奏（须在后台线程调用）：乐观复活 → 兜底刷新 → 解析 cwd → 按 provider 取
/// resume 命令 argv。返回 (真的复活了才是 Some(sid)——供 spawn 失败回滚,绝不回滚未被本次复活的
/// 真连接会话、resolved_cwd、resume_argv)。
/// 乐观复活:resume 是看板主动发起的,已知恢复哪个会话——先复活并清旧 pid,卡片即刻显示已连接,
/// 不必等 hook(尤其 codex 的 session_start hook 要到首个 turn 才触发)。旧 pid 死活经
/// pid_alive_agent_quick 校验后以 dead_pid 传入,由 store 层 `pid=?` 守卫原子闭合 TOCTOU
/// (见 revive_for_resume)。emit 兜底刷新,不依赖 db watcher 存活。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn prepare_resume(
    app: &tauri::AppHandle,
    session_id: &str,
    cwd: Option<&str>,
    provider: &str,
) -> (Option<i64>, Option<String>, Vec<String>) {
    let revived = (|| {
        let store = open_store(&db_path()).ok()?;
        let sid = store.find_session_id_pub(session_id).ok().flatten()?;
        let dead_pid = store
            .session_pid(sid)
            .ok()
            .flatten()
            .filter(|&p| p > 0 && !pid_alive_agent_quick(p));
        match store.revive_for_resume(sid, now_ms(), dead_pid) {
            Ok(true) => Some(sid),
            _ => None,
        }
    })();
    emit_board_changed(app, "resume");
    let (resolved, resume) = resolve_resume_plan(session_id, cwd, provider);
    (revived, resolved, resume)
}

/// 只读解析恢复计划；不得改状态。restart 路径必须先确认计划有效，再结束原进程。
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn resolve_resume_plan(
    session_id: &str,
    cwd: Option<&str>,
    provider: &str,
) -> (Option<String>, Vec<String>) {
    let agent = meowo_agent::resolve(Some(provider));
    // resume 必须在会话原项目目录下运行才找得到会话。DB 的 cwd 可能为空或失真（旧会话 / 压缩漏
    // SessionStart / 目录被移动）。能从 transcript 读出权威 cwd 的 agent（claude）据此纠正；
    // 其余原样采信 DB——此前这里无条件走 claude 的解析路径，非 claude 会话靠「在 ~/.claude/projects
    // 里找不到就回退 DB cwd」的巧合才拿到正确结果。
    let resolved = match agent
        .and_then(|a| a.telemetry())
        .and_then(|t| t.transcript())
    {
        Some(spec) => spec.resolve_cwd(cwd, session_id),
        None => meowo_agent::default_resolve_cwd(cwd),
    };
    // 恢复命令按 provider 取（claude --resume / kimi -r …）；可执行名+参数均来自受信 agent 定义。
    // 未知 agent、或该 agent 未声明 resume 子命令 → 空 argv：调用方的 spawn 会失败并回滚复活，
    // 好过拿 claude 的参数去拉起别的 CLI。
    let resume = agent
        .and_then(|a| a.resume_argv(session_id))
        .unwrap_or_default();
    (resolved, resume)
}

/// resume 的终端 spawn 失败时回滚乐观复活（收尾回 ended）：GUI 构建下 stderr 不可见，
/// 至少让卡片立即回落「已断开」，而不是假显示「已连接」直到 120s 宽限过期。
/// 只对 prepare_resume 返回 Some(确实复活过)的会话调用——未被本次复活的真连接会话不得误收尾。
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub(crate) fn rollback_failed_resume(sid: i64) {
    if let Ok(store) = open_store(&db_path()) {
        let _ = store.end_session(sid, now_ms());
    }
}

/// 在 `cwd` 打开一个终端并运行 `argv`，终端类型由 `terminal`（同 settings.resume_terminal 取值）决定。
/// resume（`claude --resume <id>`）与 new（裸 `claude`）共用——唯一区别是传入的 argv。成功返回 true。
/// Windows：powershell/cmd/wezterm/wt，缺失回退链同 resume 旧逻辑；wt 分支独立传 argv 不拼 shell 串。
#[cfg(target_os = "windows")]
pub(crate) fn spawn_in_terminal(
    argv: &[String],
    cwd: Option<&str>,
    terminal: &str,
    env: &[(String, String)],
) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let dir = safe_cwd(cwd);
    // 选了 wt/默认但没装 wt → 回退 PowerShell；选了 wezterm 但已卸载 → 落回 wt/powershell。
    let eff = match terminal {
        "powershell" => "powershell",
        "cmd" => "cmd",
        "wezterm" if wezterm::available() => "wezterm",
        _ if wt_available() => "wt",
        _ => "powershell",
    };
    let spawned: std::io::Result<()> = match eff {
        "powershell" => {
            let cmd = format!(
                "{}{}",
                env_prefix_powershell(env),
                shell_join_for_windows(argv, true)
            );
            let mut c = Command::new("powershell");
            c.args(["-NoExit", "-Command", &cmd]);
            if let Some(d) = &dir {
                c.current_dir(d);
            }
            c.creation_flags(CREATE_NEW_CONSOLE).spawn().map(|_| ())
        }
        "cmd" => {
            // cmd 没有能覆盖 %, !, ^, 嵌套引号等全部情况的字面 argv 语法。把真实 argv 放进
            // PowerShell EncodedCommand，cmd 只看到固定开关与 base64，避免用户路径/中转模型变成语法。
            let wrapped = wrap_with_env_windows(argv, env);
            let cmd = shell_join_for_windows(&wrapped, false);
            let mut c = Command::new("cmd");
            c.raw_arg("/k").raw_arg(cmd);
            if let Some(d) = &dir {
                c.current_dir(d);
            }
            c.creation_flags(CREATE_NEW_CONSOLE).spawn().map(|_| ())
        }
        // wezterm / wt 都不是我们的子进程（前者由 mux server 起、后者交给已存在的 wt 实例），
        // Command::env() 传不过去 → 有代理要注入时，改成让它们跑一层 PowerShell 来设变量。
        // 无代理时保持原样（直接跑 agent），把行为变更严格限制在用了代理的用户身上。
        "wezterm" => wezterm::resume(dir.as_deref(), &wrap_with_env_windows(argv, env)),
        _ => {
            let mut args: Vec<String> = vec!["-w".into(), "0".into(), "nt".into()];
            if let Some(p) = wt_default_profile() {
                args.push("-p".into());
                args.push(p);
            }
            if let Some(d) = &dir {
                args.push("-d".into());
                args.push(d.clone());
            }
            args.extend(wrap_with_env_windows(argv, env));
            Command::new("wt").args(&args).spawn().map(|_| ())
        }
    };
    match spawned {
        Ok(()) => true,
        Err(e) => {
            eprintln!("打开终端 {eff} 失败：{e}");
            false
        }
    }
}

/// 给「不是我们子进程」的终端（wt / wezterm）用：把 argv 包进一层 PowerShell，好让代理环境变量
/// 能经命令串设进去。即使 `env` 为空也要包一层，以落实「直连」必须清掉继承代理的语义。
///
/// 必须使用 `-EncodedCommand`：Windows Terminal 会把普通 `-Command` 参数里的 `;` 重新解释成
/// 自己的多命令分隔符，于是八条清理环境变量语句会各开一个 tab，并把末尾 agent 路径的引号拆坏。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn wrap_with_env_windows(argv: &[String], env: &[(String, String)]) -> Vec<String> {
    use base64::Engine;

    let cmd = format!(
        "{}{}",
        env_prefix_powershell(env),
        shell_join_for_windows(argv, true)
    );
    let utf16le: Vec<u8> = cmd.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16le);
    vec![
        "powershell".into(),
        "-NoExit".into(),
        "-EncodedCommand".into(),
        encoded,
    ]
}

/// macOS 版：按 terminal 选 Terminal.app/iTerm2（iTerm2 未装回退 Terminal），走 AppleScript。成功 true。
#[cfg(target_os = "macos")]
pub(crate) fn spawn_in_terminal(
    argv: &[String],
    cwd: Option<&str>,
    terminal: &str,
    env: &[(String, String)],
) -> bool {
    use crate::term_script::TermKind;
    let kind = match crate::term_script::resume_kind_from_setting(terminal) {
        TermKind::ITerm2 if iterm_installed() => TermKind::ITerm2,
        TermKind::ITerm2 => TermKind::Terminal,
        other => other,
    };
    crate::macos::terminal::resume_session_mac(cwd, argv, kind, &env_prefix_posix(env))
}

/// 其它平台无终端集成。
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn spawn_in_terminal(
    _argv: &[String],
    _cwd: Option<&str>,
    _terminal: &str,
    _env: &[(String, String)],
) -> bool {
    false
}

/// 校验并归一「新建会话」的工作目录：非空、真实存在的目录。返回 trim 后的路径。
pub(crate) fn validate_new_session_cwd(cwd: &str) -> Result<String, String> {
    let d = cwd.trim();
    if d.is_empty() {
        return Err("请选择工作目录".into());
    }
    if !std::path::Path::new(d).is_dir() {
        return Err("目录不存在".into());
    }
    Ok(d.to_string())
}

/// 新建一个全新会话：在 `cwd` 打开终端裸启动指定 provider 的 CLI（无 session_id）。
/// 会话入库仍靠该 CLI 自己的 hook（claude/kimi 秒级，codex 首条消息后）——本命令只负责 spawn。
/// terminal 缺省用 settings.resume_terminal。spawn 放 blocking 线程池并 await，失败回传前端面板。
#[tauri::command]
pub(crate) async fn new_session(
    cwd: String,
    provider: String,
    terminal: Option<String>,
) -> Result<(), String> {
    let dir = validate_new_session_cwd(&cwd)?;
    let agent = meowo_agent::resolve(Some(&provider)).ok_or("未知 agent")?;
    let argv = crate::relay::augment_argv(agent.id(), agent.launch_argv());
    // 代理 + 中转 **+ 当前活跃账号的隔离变量**（`CLAUDE_CONFIG_DIR` 等），三者都在
    // `launch_env_for_profile` 里。
    //
    // 这里曾经只注入代理（`proxy::launch_env`），于是多账号完全不生效：设置页明明切到了另一个
    // 账号，新开的会话却仍跑在默认账号上——而且毫无迹象，用户只能靠 `/status` 里的邮箱才发现。
    // 新建会话是**用户切换账号后最先走的一条路**，漏了它等于整个功能没做。
    let env = launch_env_for_profile(Some(&provider), None);
    let term = terminal
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| load_settings().resume_terminal);
    // 冷启动首次 spawn 控制台子进程可达数秒；放 blocking 池不挡事件循环，同时能 await 结果回传。
    let ok = tauri::async_runtime::spawn_blocking(move || {
        spawn_in_terminal(&argv, Some(&dir), &term, &env)
    })
    .await
    .map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else if cfg!(not(any(target_os = "windows", target_os = "macos"))) {
        Err("当前平台不支持从看板新建会话".into())
    } else {
        Err("启动终端失败：请确认所选 agent 已安装并在 PATH 中".into())
    }
}

/// 恢复一个已断开的会话：在其原工作目录 `cwd` 新开一个终端跑 `claude --resume <session_id>`。
/// 终端按设置 `resume_terminal` 选择——Windows：wt(默认)/wezterm/powershell/cmd；macOS：Terminal/iTerm2。
/// `cwd` 缺失/非法(旧会话)时不带 cwd，尽力按 id 恢复。
///
/// 恢复命令由 `provider` 决定（claude: `claude --resume <id>` / kimi: `kimi -r <id>`，见 agent::resume_args）。
/// 安全：`session_id` 经 is_safe_id 校验（仅 `[A-Za-z0-9_-]`，无空格/元字符）；可执行名与参数来自受信的
/// agent::resume_args（非用户输入）；wt 分支各 argv 独立传入，powershell/cmd 命令串只由这些受信片段拼成，从源头杜绝注入。
#[tauri::command]
pub(crate) fn resume_session(
    app: tauri::AppHandle,
    cwd: Option<String>,
    session_id: String,
    provider: String,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    ensure_session_profile_available(&provider, &session_id)?;
    #[cfg(target_os = "windows")]
    {
        // 冷启动后首次 spawn 控制台子进程可达数秒（新建 conhost + 杀软扫描），resolve_cwd 还要读
        // transcript；同步命令跑在主线程，整段挪后台线程，命令立即返回。
        std::thread::spawn(move || {
            let (revived, resolved_cwd, resume) =
                prepare_resume(&app, &session_id, cwd.as_deref(), &provider);
            let env = launch_env_for_session(Some(&provider), &session_id);
            let ok = spawn_in_terminal(
                &resume,
                resolved_cwd.as_deref(),
                &load_settings().resume_terminal,
                &env,
            );
            if !ok {
                // GUI 构建 stderr 不可见：回滚乐观复活，卡片立即回落「已断开」而非假连接 120s。
                if let Some(sid) = revived {
                    rollback_failed_resume(sid);
                }
                emit_board_changed(&app, "resume-failed");
            }
        });
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // resolve_cwd 读 transcript、osascript 可能等 TCC 授权，整段放后台线程不挡主线程。
        // resume 命令按 provider 分发（与 Windows 同一事实源），不再硬编码 claude——
        // 否则 macOS 上恢复 codex/kimi 会话会执行错误命令。
        std::thread::spawn(move || {
            let (revived, resolved, resume) =
                prepare_resume(&app, &session_id, cwd.as_deref(), &provider);
            let env = launch_env_for_session(Some(&provider), &session_id);
            let ok = spawn_in_terminal(
                &resume,
                resolved.as_deref(),
                &load_settings().resume_terminal,
                &env,
            );
            if !ok {
                eprintln!("恢复会话：终端启动失败");
                if let Some(sid) = revived {
                    rollback_failed_resume(sid);
                }
                emit_board_changed(&app, "resume-failed");
            }
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = (app, cwd, provider);
        Err("当前平台不支持".into())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn terminate_agent_for_restart(pid: i64) -> Result<(), String> {
    // 确认弹窗停留期间进程可能已经自然结束；此时无需报错，直接进入恢复流程。
    if !pid_alive_agent_quick(pid) {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let sent = {
        let sys = sysinfo::System::new_all();
        sys.process(sysinfo::Pid::from_u32(pid as u32))
            .filter(|p| meowo_agent::is_agent_process(&p.name().to_string_lossy()))
            .is_some_and(|p| p.kill())
    };
    // macOS 上 sysinfo 的进程可见性不稳定（判活本来也走 ps），直接以独立 argv 发送 TERM，不经 shell。
    #[cfg(target_os = "macos")]
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success());
    if !sent {
        return Err("无法结束原会话进程".into());
    }

    // 给 Agent 的退出清理/SessionEnd hook 留出时间；若仍存活再强制结束，避免恢复出双进程。
    for _ in 0..30 {
        if !pid_alive_agent_quick(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if pid_alive_agent_quick(pid) {
        #[cfg(target_os = "windows")]
        let forced = {
            let sys = sysinfo::System::new_all();
            sys.process(sysinfo::Pid::from_u32(pid as u32))
                .filter(|p| meowo_agent::is_agent_process(&p.name().to_string_lossy()))
                .is_some_and(|p| p.kill())
        };
        #[cfg(target_os = "macos")]
        let forced = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success());
        if !forced {
            return Err("原会话仍在运行，未重新打开".into());
        }
        for _ in 0..20 {
            if !pid_alive_agent_quick(pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if pid_alive_agent_quick(pid) {
            return Err("原会话仍在运行，未重新打开".into());
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    Ok(())
}

/// 用户确认后结束“仍存活但无法定位终端”的原 Agent，并在设置中的受支持终端恢复同一会话。
/// PID 必须仍属于该 session，且进程名仍是受支持 Agent；两层校验避免 PID 复用或过期 UI 误杀进程。
#[tauri::command]
pub(crate) async fn restart_session_supported(
    app: tauri::AppHandle,
    pid: i64,
    cwd: Option<String>,
    session_id: String,
    provider: String,
) -> Result<(), String> {
    if pid <= 0 || !is_safe_id(&session_id) {
        return Err("无效会话".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let store = open_store(&db_path()).map_err(|e| e.to_string())?;
        let sid = store
            .find_session_id_pub(&session_id)
            .map_err(|e| e.to_string())?
            .ok_or("会话不存在")?;
        if store.session_pid(sid).map_err(|e| e.to_string())? != Some(pid) {
            return Err("会话进程已变化，请刷新后重试".into());
        }

        // 先验证完整恢复计划，再动原进程；未知 provider/无恢复能力时必须保持原会话原样。
        let (resolved, resume) = resolve_resume_plan(&session_id, cwd.as_deref(), &provider);
        if resume.is_empty() {
            return Err("该 Agent 不支持恢复会话".into());
        }
        // 账号目录可能已被删除。必须在终止仍可用的原进程之前拦住，否则恢复失败还会顺手杀掉会话。
        ensure_session_profile_available(&provider, &session_id)?;
        terminate_agent_for_restart(pid)?;

        // 原进程确认结束后才复活 DB 状态；恢复计划沿用终止前已验证的结果。
        let (revived, _, _) = prepare_resume(&app, &session_id, cwd.as_deref(), &provider);
        let env = launch_env_for_session(Some(&provider), &session_id);
        let ok = spawn_in_terminal(
            &resume,
            resolved.as_deref(),
            &load_settings().resume_terminal,
            &env,
        );
        if ok {
            Ok(())
        } else {
            if let Some(id) = revived {
                rollback_failed_resume(id);
            }
            emit_board_changed(&app, "restart-failed");
            Err("启动受支持的终端失败".into())
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod proxy_env_tests {
    use super::*;

    fn decode_wrapped_command(wrapped: &[String]) -> String {
        use base64::Engine;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&wrapped[3])
            .unwrap();
        let utf16: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16(&utf16).unwrap()
    }

    fn env(v: &str) -> Vec<(String, String)> {
        vec![
            ("HTTPS_PROXY".into(), v.into()),
            ("HTTP_PROXY".into(), v.into()),
        ]
    }

    #[test]
    fn powershell_prefix_sets_vars_before_the_command() {
        let p = env_prefix_powershell(&env("http://127.0.0.1:7890"));
        assert!(p.starts_with("$env:HTTPS_PROXY=$null; "));
        assert!(p.ends_with(
            "$env:HTTPS_PROXY='http://127.0.0.1:7890'; $env:HTTP_PROXY='http://127.0.0.1:7890'; "
        ));
        // 与命令串拼起来必须是「先赋值、再执行」。
        let cmd = format!(
            "{p}{}",
            shell_join_for_windows(&["claude".to_string()], true)
        );
        assert!(cmd.starts_with("$env:HTTPS_PROXY="));
        assert!(cmd.ends_with("claude"));
    }

    #[test]
    fn posix_prefix_leaves_the_name_unquoted() {
        // 关键：**键名不能带引号**——`'K=v' cmd` 会被 shell 当成一个命令名而不是赋值。
        // 值则必须单引号包裹（AppleScript 会把它原样拼进 shell 命令串）。
        let p = env_prefix_posix(&env("http://127.0.0.1:7890"));
        assert!(p.starts_with("unset HTTPS_PROXY HTTP_PROXY ALL_PROXY NO_PROXY "));
        assert!(
            p.ends_with("HTTPS_PROXY='http://127.0.0.1:7890' HTTP_PROXY='http://127.0.0.1:7890' ")
        );
        assert!(!p.starts_with('\''), "键名不得被引起来");
    }

    /// 代理串是用户输入，会被拼进 shell 命令串——三种 shell 的转义都必须挡住「闭合引号后接命令」
    /// 的注入。值虽已过 validate（无空格、协议白名单），这里仍按「一律正确转义」把关，不赌。
    #[test]
    fn quoting_survives_a_value_with_quotes() {
        let evil = vec![("HTTPS_PROXY".to_string(), "http://a'b;calc".to_string())];

        // PowerShell：单引号翻倍 → 引号无法闭合，`;calc` 留在字符串里。
        let ps = env_prefix_powershell(&evil);
        assert!(ps.ends_with("$env:HTTPS_PROXY='http://a''b;calc'; "));

        // POSIX：`'` → `'\''`，同样无法闭合。
        let sh = env_prefix_posix(&evil);
        assert!(sh.ends_with(r"HTTPS_PROXY='http://a'\''b;calc' "));
    }

    #[test]
    fn empty_env_clears_inherited_proxy_before_launching() {
        // off 的意义是直连，故空 env 也必须清掉继承的代理，而不是原样启动。
        assert!(env_prefix_powershell(&[]).contains("$env:ALL_PROXY=$null;"));
        assert!(env_prefix_posix(&[]).starts_with("unset HTTPS_PROXY"));
        let argv = vec![
            "claude".to_string(),
            "--resume".to_string(),
            "abc".to_string(),
        ];
        let wrapped = wrap_with_env_windows(&argv, &[]);
        assert_eq!(wrapped[0], "powershell");
        assert_eq!(wrapped[2], "-EncodedCommand");
        assert!(
            !wrapped[3].contains(';'),
            "WT 可见的参数里不能再出现命令分隔符"
        );
        assert!(decode_wrapped_command(&wrapped).contains("$env:ALL_PROXY=$null;"));
    }

    /// wt / wezterm 不是我们的子进程（前者交给已存在的 wt 实例、后者交给 mux server），
    /// Command::env() 传不过去 → 必须包一层 PowerShell 把赋值写进命令串。
    #[test]
    fn wt_and_wezterm_get_wrapped_in_a_shell_when_proxied() {
        let argv = vec![
            "C:/x/codex.exe".to_string(),
            "resume".to_string(),
            "sid".to_string(),
        ];
        let w = wrap_with_env_windows(&argv, &env("http://127.0.0.1:7890"));
        assert_eq!(w[0], "powershell");
        assert_eq!(w[1], "-NoExit");
        assert_eq!(w[2], "-EncodedCommand");
        assert!(!w[3].contains(';'), "WT 不得把脚本拆成多个 tab：{}", w[3]);
        let decoded = decode_wrapped_command(&w);
        assert!(decoded.starts_with("$env:HTTPS_PROXY=$null; "));
        assert!(decoded.contains("$env:HTTPS_PROXY='http://127.0.0.1:7890'; "));
        assert!(decoded.contains("codex.exe"), "原命令必须还在：{decoded}");
        assert!(decoded.ends_with("resume sid"));
    }
}
