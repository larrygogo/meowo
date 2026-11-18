//! 对话窗右侧面板的「浏览器」页签：在 chat 窗口里叠一个原生子 webview，大小和位置
//! 由前端按面板里那块占位区域实时下发。
//!
//! 为什么不用 iframe：大多数站点用 X-Frame-Options / CSP 禁止被嵌，iframe 里除了自己的
//! localhost 基本什么都打不开。Kimi Code 1.3.1 同样是原生视图叠在面板上（2026-11-06
//! 拆包：`WebContentsView` + `kimi:browser-set-bounds`）。
//!
//! 安全边界：
//! - 子 webview 加载的是外部页面。Tauri 对非本地来源的 IPC 一律走 ACL，项目的 capability
//!   没有任何 `remote` 放行，所以这些页面调不到 Meowo 的命令（tauri 2.12.5
//!   webview/mod.rs 的 `!is_local` 分支）。
//! - 但 `*.localhost` 是 Meowo 自己的本地来源（Windows 上前端就跑在 http://tauri.localhost），
//!   导航到那里就成了「本地页面」，IPC 全开。所以地址栏和页内跳转都拦掉 `*.localhost`
//!   与 dev 构建的 devUrl，只放行 http/https。
//! - 手机端不给：命令全列进 remote.rs 的拒绝清单。
//!
//! 生命周期同面板终端：页签跟着会话走，换会话前端只是把它藏起来；关页签才销毁。
//! agent 用过的页签不真藏，而是挪到窗口外接着渲染（见 `Placement`）：agent 多数时候是在
//! 面板收起、用户看不到的时候操作网页，藏起来的页面不渲染，滚动检测和截图都会失灵。
//! 子 webview 随 chat 窗口一起销毁，窗口销毁时这里只清表。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{
    Emitter, LogicalPosition, LogicalSize, Manager, Rect, Url, WebviewBuilder, WebviewUrl,
};

const BLANK: &str = "about:blank";

/// Windows 在每个子浏览器自己的 `WRY_WEBVIEW` 父句柄上禁用输入，整棵 HWND 树（含 iframe）
/// 都收不到人工键鼠；Agent 的 eval、渲染和 CapturePreview 不受影响。页面 JS 看不见也改不了
/// 这个原生状态，避免了主世界输入门被网页调用 `set(false)` 或替换全局对象绕过。
#[cfg(windows)]
pub(crate) fn set_agent_input_locked(app: &tauri::AppHandle, id: u32, locked: bool) {
    let Ok(webview) = webview(app, id) else {
        return;
    };
    if let Err(error) = webview.with_webview(move |platform| unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
        let controller = platform.controller();
        let mut hwnd = HWND::default();
        if let Err(error) = controller.ParentWindow(&mut hwnd) {
            eprintln!("[browser] 页签 {id} 原生输入开关找不到句柄：{error}");
            return;
        }
        let _ = EnableWindow(hwnd, !locked);
    }) {
        eprintln!("[browser] 页签 {id} 原生输入开关下发失败：{error}");
    }
}

#[cfg(all(feature = "e2e", windows))]
#[tauri::command]
pub(crate) fn browser_pane_e2e_native_click(
    app: tauri::AppHandle,
    id: u32,
    x: f64,
    y: f64,
) -> Result<String, String> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        mouse_event, IsWindowEnabled, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, SetCursorPos, WindowFromPoint,
    };

    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return Err("无效的测试点击坐标".into());
    }
    let webview = webview(&app, id)?;
    let window = app.get_window("chat").ok_or("chat 窗口不存在")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let diagnostic = Arc::new(Mutex::new(String::new()));
    let sink = diagnostic.clone();
    // WDIO 不枚举原生子 WebView；E2E 用真实用户态鼠标事件验证 EnableWindow，
    // 不能用 SendMessage（它会绕过禁用窗口的输入语义）。生产构建不注册此命令。
    webview.with_webview(move |platform| unsafe {
        use windows::Win32::Foundation::HWND;
        let controller = platform.controller();
        let mut hwnd = HWND::default();
        if let Err(error) = controller.ParentWindow(&mut hwnd) {
            *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = format!("找不到原生句柄：{error}");
            return;
        }
        let mut origin = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd.0, &mut origin) == 0 {
            *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = "客户区坐标换算失败".into();
            return;
        }
        let screen_x = origin.x + (x * scale).round() as i32;
        let screen_y = origin.y + (y * scale).round() as i32;
        let class_of = |hwnd| {
            let mut buffer = [0u16; 96];
            let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            String::from_utf16_lossy(&buffer[..len.max(0) as usize])
        };
        let target = WindowFromPoint(POINT { x: screen_x, y: screen_y });
        *sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = format!(
            "id={id} enabled={} class={} target={} origin=({}, {}) point=({}, {})",
            IsWindowEnabled(hwnd.0), class_of(hwnd.0), class_of(target), origin.x, origin.y, screen_x, screen_y
        );
        if SetCursorPos(screen_x, screen_y) == 0 {
            return;
        }
        mouse_event(MOUSEEVENTF_MOVE, 0, 0, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(80));
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(40));
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
    }).map_err(|e| e.to_string())?;
    let result = diagnostic.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    Ok(result)
}

/// WKWebView 没有等价的逐视图输入开关；其他平台仍禁用工具栏并保留接管，但不注入可被
/// 网页篡改的 JS 输入门。macOS 的原生视图隔离需另行实机实现与验证。
#[cfg(not(windows))]
pub(crate) fn set_agent_input_locked(app: &tauri::AppHandle, id: u32, locked: bool) {
    let _ = (app, id, locked);
}

fn control_allows_human_input(agent_active: bool, agent_paused: bool) -> bool {
    !agent_active || agent_paused
}

/// 从没在面板里显示过的页签给页面的视口（逻辑像素）。agent 新开的页签常常一次都没露过面，
/// 以前建出来是 1×1，页面里量到 `innerWidth×innerHeight = 2x2`（2026-11-09 实测，150% 缩放），
/// 要求「滚到底才能点」的页面（Discord OAuth 授权页）永远判不到底。
pub(crate) const DEFAULT_VIEWPORT: (f64, f64) = (1280.0, 800.0);

/// 原生视图现在摆在哪。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Placement {
    /// `hide()` 藏起来了。WebView2 收到 `SetIsVisible(false)` 后页面进入
    /// `document.hidden`：requestAnimationFrame、IntersectionObserver 都停，CapturePreview
    /// 永远不回调。光把尺寸改大不够——2026-11-09 用 wry 0.57.1 单独实测三种摆法：
    /// 藏着 1×1 → `2x2 / hidden / rAF 不走 / 截图无回调`；藏着 1280×800 →
    /// `1280x800 / hidden / rAF 不走 / 截图无回调`；挪到窗口外但可见 →
    /// `1280x800 / visible / rAF 正常 / 截图 15KB`。macOS 26.6（WKWebView）同样只有挪到窗口外
    /// 能保持 visible + rAF，旧做法视口是 1×1；但 takeSnapshot 隐藏时也能截到图。锁屏时
    /// WebKit 把整个窗口判为不可见，连正常显示的视图也 hidden，那种情况这里管不了。
    #[default]
    Hidden,
    /// 仍然「可见」，按原尺寸挪到窗口内容区外面（被父窗口裁掉，用户看不到）。页面照常渲染，
    /// 滚动检测、IntersectionObserver、截图都正常。agent 在用的页签收起时摆这里。
    Parked,
    /// 显示在面板占位区域上。
    OnScreen,
}

/// 收起后视口的下限（逻辑像素）。面板可以拖得很窄，或者布局变动时量到一个瞬时的小矩形；
/// 原样沿用就又回到「视口太小」的老问题（窄到页面切手机版、滚到底判不出来）。
const MIN_PARKED_VIEWPORT: (f64, f64) = (480.0, 480.0);

/// 面板里量到的尺寸 → 收起后给页面的视口：按面板尺寸来，排版不跳；太小的维度抬到下限。
pub(crate) fn parked_viewport((width, height): (f64, f64)) -> (f64, f64) {
    (
        width.max(MIN_PARKED_VIEWPORT.0),
        height.max(MIN_PARKED_VIEWPORT.1),
    )
}

/// 挪到窗口外时的位置和大小：保持视口尺寸，整块放到内容区左边外侧。
pub(crate) fn parked_rect((width, height): (f64, f64)) -> Rect {
    Rect {
        position: LogicalPosition::new(-(width + 64.0), 0.0).into(),
        size: LogicalSize::new(width, height).into(),
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPaneInfo {
    pub(crate) id: u32,
    /// 归属的会话 id。
    pub(crate) owner: i64,
    url: String,
    title: String,
    pub(crate) loading: bool,
    /// agent 正在操作这个页签（最近几秒内调过浏览器工具）。前端据此在页签和工具栏上提示。
    pub(crate) agent_active: bool,
    /// 用户点了「接管」：agent 的浏览器操作一律拒绝，直到用户交还。
    pub(crate) agent_paused: bool,
    /// 每次接管/交还都换代；排队中的旧调用即使看到已交还也不能续跑。
    #[serde(skip)]
    pub(crate) agent_generation: u64,
    /// 页面缩放比例（1.0 = 100%），「…」菜单里调。
    pub(crate) zoom: f64,
    /// 最近几次下载，旧的在前（见 `download_started`）。
    pub(crate) downloads: Vec<BrowserDownload>,
    /// 网页在等用户答复的权限请求，只给排在最前的一条（见 browser_permission.rs）。
    pub(crate) permission: Option<BrowserPermission>,
    /// 用户刚阻止的请求。WebView2 在同一页面里不会再问，之后被拦的下载连事件都没有（做空审查
    /// 实测：阻止后再下两次，既无权限请求也无下载事件），不留这条提示用户又会以为没下成而
    /// 反复点。有新下载开始（说明不拦了）、页面重新加载或跳走、用户点 × 时消失。
    pub(crate) blocked: Option<BrowserPermission>,
    /// 设备模拟（视口、UA、附加请求头），没模拟是 None。见 browser_emulation.rs。
    pub(crate) emulation: Option<crate::browser_emulation::Emulation>,
    // 以下几项只在后端用，不发给前端。
    /// 原生视图眼下摆在哪。
    #[serde(skip)]
    placement: Placement,
    /// 收起时给页面留的视口：上一次在面板里显示时的尺寸（经 `parked_viewport` 抬到下限），
    /// 没显示过就是 DEFAULT_VIEWPORT。
    #[serde(skip)]
    viewport: (f64, f64),
    /// 面板里实际显示的尺寸（不抬下限），设备模拟按它算缩放比例。
    #[serde(skip)]
    shown: (f64, f64),
    /// agent 用过这个页签：收起时挪到窗口外接着渲染，而不是藏起来。见 `Placement`。
    #[serde(skip)]
    keep_rendering: bool,
    /// 同一个页签的 Agent 工具调用串行；锁随页签状态释放，不留全局残留。
    #[serde(skip)]
    pub(crate) operation: Arc<Mutex<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DownloadState {
    Progress,
    Done,
    Failed,
}

/// 面板浏览器里的一次下载。以前不接下载事件，WebView2 静默存进「下载」文件夹、什么都不提示，
/// 用户以为没下成又连点（2026-11-10 实拍：同一份文件下了 6 份）。现在开始、完成、失败都在
/// 面板里提示一条。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserDownload {
    pub(crate) id: u32,
    pub(crate) file_name: String,
    pub(crate) state: DownloadState,
    /// 网址的哈希，只用来给完成事件对号。不存原文：data: 网址可能有几 MB，每次推状态都要复制。
    #[serde(skip)]
    url_key: u64,
    /// 存到哪。完成前是预定路径（macOS 完成时拿不到路径，只能用这个）。
    #[serde(skip)]
    pub(crate) path: Option<PathBuf>,
}

/// 最多记几次下载。面板里只显示最新一条，留几条是为了并发下载时完成事件对得上号。
const MAX_DOWNLOADS: usize = 5;

static NEXT_DOWNLOAD: AtomicU32 = AtomicU32::new(1);

fn url_key(url: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

fn file_name_of(path: &Path, url: &str) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url).to_string())
}

/// 下载开始：记一条「下载中」，超出上限丢最旧的。
pub(crate) fn download_started(list: &mut Vec<BrowserDownload>, url: &str, destination: &Path) {
    list.push(BrowserDownload {
        id: NEXT_DOWNLOAD.fetch_add(1, Ordering::Relaxed),
        file_name: file_name_of(destination, url),
        state: DownloadState::Progress,
        url_key: url_key(url),
        path: Some(destination.to_path_buf()),
    });
    if list.len() > MAX_DOWNLOADS {
        let excess = list.len() - MAX_DOWNLOADS;
        list.drain(..excess);
    }
}

/// 下载结束：找同网址、还在下载中的最近一条改状态。完成事件给了路径就以它为准（WebView2
/// 重名时可能另起名字）。对不上号（已被挤出列表）返回 false。
pub(crate) fn download_finished(
    list: &mut [BrowserDownload],
    url: &str,
    path: Option<PathBuf>,
    success: bool,
) -> bool {
    let key = url_key(url);
    let Some(entry) = list
        .iter_mut()
        .rev()
        .find(|entry| entry.url_key == key && entry.state == DownloadState::Progress)
    else {
        return false;
    };
    entry.state = if success {
        DownloadState::Done
    } else {
        DownloadState::Failed
    };
    if let Some(path) = path.filter(|path| !path.as_os_str().is_empty()) {
        entry.file_name = file_name_of(&path, url);
        entry.path = Some(path);
    }
    true
}

/// 目标文件已存在（或正被别的下载占用）就在文件名后加「 (1)」「 (2)」…，不覆盖用户已有的文件
/// （与浏览器同习惯）。WebView2 拿到已存在的路径会直接覆盖。只看磁盘不够：同一页面连着下同名
/// 文件时，前一个还没写到磁盘，后一个会挑中同一个名字（测试程序实测 4 次下载只剩 3 个文件）。
pub(crate) fn unique_path(path: &Path, taken: impl Fn(&Path) -> bool) -> PathBuf {
    let busy = |candidate: &Path| candidate.exists() || taken(candidate);
    if !busy(path) {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|n| path.with_file_name(format!("{stem} ({n}){ext}")))
        .find(|candidate| !busy(candidate))
        .unwrap_or_else(|| path.to_path_buf())
}

/// 下载开始：在一次加锁里挑好不撞名的路径并登记，并发的下载才不会挑中同一个名字。
/// 「正被占用」看所有页签里还在下载中的路径（都存进同一个「下载」文件夹）。
fn start_download(
    app: &tauri::AppHandle,
    panes: &Arc<Mutex<HashMap<u32, BrowserPaneInfo>>>,
    id: u32,
    url: &str,
    destination: &mut PathBuf,
) {
    let info = {
        let Ok(mut map) = panes.lock() else { return };
        let in_progress: Vec<PathBuf> = map
            .values()
            .flat_map(|info| info.downloads.iter())
            .filter(|entry| entry.state == DownloadState::Progress)
            .filter_map(|entry| entry.path.clone())
            .collect();
        *destination = unique_path(destination, |candidate| {
            in_progress.iter().any(|busy| busy == candidate)
        });
        let Some(info) = map.get_mut(&id) else { return };
        download_started(&mut info.downloads, url, destination);
        if info
            .blocked
            .as_ref()
            .is_some_and(|blocked| blocked.kind == "downloads")
        {
            info.blocked = None;
        }
        info.clone()
    };
    emit_state(app, &info);
}

/// 网页在等用户答复的权限请求（见 browser_permission.rs）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BrowserPermission {
    /// 每条请求一个编号。前端靠它判断「答复送达前锁住按钮」该不该解开：只比对象引用的话，
    /// 标题、加载状态这些无关推送也会提前解锁（做空审查）。
    pub(crate) id: u32,
    /// downloads / camera / microphone / location / notifications / clipboard / files / other。
    pub(crate) kind: String,
    /// 提示里显示的网站名（主机名）。
    pub(crate) host: String,
}

// 只有 Windows 的 browser_permission::imp 建请求；macOS 下这两项没有调用方，clippy -D warnings
// 会报从未使用（CI macOS 矩阵，第二轮做空审查用最小 crate 模拟复现）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
static NEXT_PERMISSION: AtomicU32 = AtomicU32::new(1);

impl BrowserPermission {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn new(kind: &str, uri: &str) -> Self {
        Self {
            id: NEXT_PERMISSION.fetch_add(1, Ordering::Relaxed),
            kind: kind.to_string(),
            host: Url::parse(uri)
                .ok()
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_else(|| uri.to_string()),
        }
    }

    /// 给 agent 看的一句话：「httpbin.org 想要下载多个文件」。前端有自己的双语文案。
    pub(crate) fn describe(&self) -> String {
        let what = match self.kind.as_str() {
            "downloads" => "下载多个文件",
            "camera" => "使用摄像头",
            "microphone" => "使用麦克风",
            "location" => "获取位置",
            "notifications" => "发送通知",
            "clipboard" => "读取剪贴板",
            "files" => "读写文件",
            _ => "使用额外权限",
        };
        format!("{} 想要{what}", self.host)
    }
}

#[derive(Default)]
pub(crate) struct BrowserPanes {
    next_id: Mutex<u32>,
    pub(crate) panes: Arc<Mutex<HashMap<u32, BrowserPaneInfo>>>,
}

impl BrowserPanes {
    /// 会话换了编号（新会话认领、/clear 换代）：浏览器归属跟过去。
    pub(crate) fn reassign_owner(&self, from: i64, to: i64) {
        if let Ok(mut map) = self.panes.lock() {
            for info in map.values_mut().filter(|info| info.owner == from) {
                info.owner = to;
            }
        }
    }

    /// 会话结束 / 关闭时关掉它在面板里开的所有浏览器。
    pub(crate) fn close_owner(&self, app: &tauri::AppHandle, owner: i64) {
        let ids: Vec<u32> = match self.panes.lock() {
            Ok(mut map) => {
                let ids: Vec<u32> = map
                    .values()
                    .filter(|info| info.owner == owner)
                    .map(|info| info.id)
                    .collect();
                for id in &ids {
                    map.remove(id);
                }
                ids
            }
            Err(_) => return,
        };
        for id in ids {
            crate::browser_permission::forget(app, id);
            if let Ok(webview) = webview(app, id) {
                let _ = webview.close();
            }
        }
    }

    /// chat 窗口销毁时调用：子 webview 已随窗口没了，表里的记录也作废。
    pub(crate) fn clear(&self) {
        if let Ok(mut panes) = self.panes.lock() {
            panes.clear();
        }
    }
}

pub(crate) fn label_of(id: u32) -> String {
    format!("chat-browser-{id}")
}

/// 地址栏输入 → 要打开的网址。没有协议的：像主机名就补协议（本机地址补 http，其余补
/// https），否则当搜索词交给 Bing（国内直连可用）。
pub(crate) fn normalize_input(raw: &str) -> Option<String> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("://") {
        return Some(text.to_string());
    }
    let host = text.split(['/', '?', '#']).next().unwrap_or("");
    let host_only = host.rsplit_once(':').map_or(host, |(name, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            name
        } else {
            host
        }
    });
    let local = host_only == "localhost"
        || host_only.starts_with("127.")
        || host_only == "0.0.0.0"
        || host_only == "[::1]";
    if local {
        return Some(format!("http://{text}"));
    }
    if !text.contains(char::is_whitespace) && host_only.contains('.') {
        return Some(format!("https://{text}"));
    }
    let mut url = Url::parse("https://www.bing.com/search").ok()?;
    url.query_pairs_mut().append_pair("q", text);
    Some(url.to_string())
}

/// 这个网址能不能在面板浏览器里打开。见模块头「安全边界」。
pub(crate) fn allowed(url: &Url, dev_url: Option<&Url>) -> bool {
    if url.as_str() == BLANK {
        return true;
    }
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.to_ascii_lowercase().ends_with(".localhost") {
        return false;
    }
    if let Some(dev) = dev_url {
        if dev.origin() == url.origin() {
            return false;
        }
    }
    true
}

pub(crate) fn emit_state(app: &tauri::AppHandle, info: &BrowserPaneInfo) {
    let _ = app.emit_to(
        tauri::EventTarget::webview("chat"),
        "browser-pane-state",
        info,
    );
}

pub(crate) fn update(
    app: &tauri::AppHandle,
    panes: &Arc<Mutex<HashMap<u32, BrowserPaneInfo>>>,
    id: u32,
    change: impl FnOnce(&mut BrowserPaneInfo),
) {
    let info = {
        let Ok(mut map) = panes.lock() else { return };
        let Some(info) = map.get_mut(&id) else { return };
        change(info);
        info.clone()
    };
    emit_state(app, &info);
}

pub(crate) fn webview(app: &tauri::AppHandle, id: u32) -> Result<tauri::Webview, String> {
    app.get_webview(&label_of(id))
        .ok_or_else(|| "browser_pane/not_found".to_string())
}

pub(crate) fn ensure_human_action_allowed(state: &BrowserPanes, id: u32) -> Result<(), String> {
    let map = state.panes.lock().map_err(|e| e.to_string())?;
    let info = map.get(&id).ok_or("browser_pane/not_found")?;
    if control_allows_human_input(info.agent_active, info.agent_paused) {
        Ok(())
    } else {
        Err("browser_pane/agent_active".into())
    }
}

pub(crate) fn parse_target(app: &tauri::AppHandle, raw: &str) -> Result<Url, String> {
    let text = normalize_input(raw).ok_or("browser_pane/bad_url")?;
    let url = Url::parse(&text).map_err(|_| "browser_pane/bad_url".to_string())?;
    if !allowed(&url, app.config().build.dev_url.as_ref()) {
        return Err("browser_pane/blocked_url".into());
    }
    Ok(url)
}

/// 新开一个浏览器页签。先藏着建出来，前端量好占位区域后再经 set_bounds 显示。
/// async：add_child 要回主线程建 webview 并等结果，同步命令本身就跑在主线程上会卡死。
#[tauri::command]
pub(crate) async fn browser_pane_open(
    app: tauri::AppHandle,
    window: tauri::Window,
    state: tauri::State<'_, BrowserPanes>,
    owner: i64,
    url: Option<String>,
) -> Result<BrowserPaneInfo, String> {
    if window.label() != "chat" {
        return Err("browser_pane/not_chat_window".into());
    }
    open_pane(&app, &window, &state, owner, url.as_deref())
}

/// 在 chat 窗口里建一个藏着的浏览器子 webview 并登记。前端的「新建浏览器」和 agent
/// 第一次用浏览器（browser_agent.rs）共用。不能在主线程上调：add_child 要回主线程等结果。
/// 同时开着的面板浏览器上限（同 Kimi Code 的 16）。每个都是一个 WebView2 实例，
/// 多个会话各开几个很快就吃掉几个 G 内存。
const MAX_PANES: usize = 16;

pub(crate) fn open_pane(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    state: &BrowserPanes,
    owner: i64,
    url: Option<&str>,
) -> Result<BrowserPaneInfo, String> {
    if state.panes.lock().map(|map| map.len()).unwrap_or(0) >= MAX_PANES {
        return Err("browser_pane/too_many".into());
    }
    let app = app.clone();
    let target = match url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(raw) => parse_target(&app, raw)?,
        None => Url::parse(BLANK).map_err(|e| e.to_string())?,
    };
    let id = {
        let mut next = state.next_id.lock().map_err(|e| e.to_string())?;
        *next += 1;
        *next
    };
    let info = BrowserPaneInfo {
        id,
        owner,
        url: target.to_string(),
        title: String::new(),
        loading: target.as_str() != BLANK,
        agent_active: false,
        agent_paused: false,
        agent_generation: 0,
        zoom: 1.0,
        downloads: Vec::new(),
        permission: None,
        blocked: None,
        emulation: None,
        placement: Placement::Hidden,
        viewport: DEFAULT_VIEWPORT,
        shown: DEFAULT_VIEWPORT,
        keep_rendering: false,
        operation: Arc::new(Mutex::new(())),
    };
    state
        .panes
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id, info.clone());

    let dev_url = app.config().build.dev_url.clone();
    let nav_dev = dev_url.clone();
    let load_app = app.clone();
    let load_panes = state.panes.clone();
    let title_app = app.clone();
    let title_panes = state.panes.clone();
    let popup_app = app.clone();
    let download_app = app.clone();
    let download_panes = state.panes.clone();
    let builder = WebviewBuilder::new(label_of(id), WebviewUrl::External(target))
        // 页内跳转同样过一遍白名单：页面自己把 location 改到 tauri.localhost 也拦得住。
        .on_navigation(move |url| allowed(url, nav_dev.as_ref()))
        .on_page_load(move |_, payload| {
            // Started 对应 WebView2 的 ContentLoading：只有新文档开始加载才触发，点附件链接
            // 下载不会触发（wry 0.57.1 webview2/mod.rs）。
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            if loading {
                // 新文档开始加载（刷新、同站或跨站跳转都算），旧页面挂着的请求一律作废。只作废
                // 跨站的不行：同站刷新后旧请求留在队首，新页面的下载全卡在它后面，点「允许」也
                // 下不下来（第二轮做空审查用同版本测试程序实测；作废后新页面会重新问一次）。
                crate::browser_permission::drop_all(&load_app, id);
            }
            let url = payload.url().to_string();
            update(&load_app, &load_panes, id, |info| {
                info.url = url;
                info.loading = loading;
                if loading {
                    info.blocked = None;
                }
            });
        })
        // 接管下载：Windows 上 wry 随之 SetHandled，WebView2 自己的下载框不再出；macOS 上
        // 不接 WKWebView 根本不下（wry 只在有回调时才挂下载代理）。都存进系统「下载」文件夹，
        // 进度和结果在面板里提示（BrowserPane.tsx）。
        .on_download(move |_, event| {
            match event {
                DownloadEvent::Requested { url, destination } => {
                    start_download(
                        &download_app,
                        &download_panes,
                        id,
                        url.as_str(),
                        destination,
                    );
                }
                DownloadEvent::Finished { url, path, success } => {
                    let url = url.to_string();
                    update(&download_app, &download_panes, id, |info| {
                        download_finished(&mut info.downloads, &url, path, success);
                    });
                }
                _ => {}
            }
            true
        })
        .on_document_title_changed(move |_, title| {
            update(&title_app, &title_panes, id, |info| info.title = title);
        })
        // target=_blank 和 window.open：面板里没有「新窗口」，就在当前页签里打开。
        .on_new_window(move |url, _| {
            if allowed(&url, dev_url.as_ref()) {
                if let Some(webview) = popup_app.get_webview(&label_of(id)) {
                    let _ = webview.navigate(url);
                }
            }
            NewWindowResponse::Deny
        });
    // 一出生就是正常视口大小（摆在窗口外），页面第一次排版就拿到合理的 innerWidth/innerHeight。
    let parked = parked_rect(DEFAULT_VIEWPORT);
    let created = window.add_child(builder, parked.position, parked.size);
    match created {
        Ok(webview) => {
            let _ = webview.hide();
            crate::browser_permission::install(&app, &webview, id);
            Ok(info)
        }
        Err(e) => {
            if let Ok(mut panes) = state.panes.lock() {
                panes.remove(&id);
            }
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub(crate) fn browser_pane_list(
    state: tauri::State<'_, BrowserPanes>,
    owner: i64,
) -> Vec<BrowserPaneInfo> {
    let Ok(map) = state.panes.lock() else {
        return Vec::new();
    };
    let mut list: Vec<BrowserPaneInfo> = map
        .values()
        .filter(|info| info.owner == owner)
        .cloned()
        .collect();
    list.sort_by_key(|info| info.id);
    list
}

/// 让原生视图从面板上让开。agent 用过的挪到窗口外接着渲染，其余的藏起来，
/// 不让一堆后台页签白白占着渲染。
/// 调用方持有 panes 锁：agent 的 keep_rendering 在 broker 线程上，「看 placement → 下发」
/// 在锁里做完，agent 那边才不会把用户刚显示出来的页签挪走。（前端自己连发的 hide/show
/// 两次 invoke 谁先拿到锁不归这里管，那是改动前就有的先后问题。）这些 setter 只往主线程
/// 事件队列里投消息、不等结果，锁里调不会卡住主线程（调用方都不在主线程上）。
fn conceal(webview: &tauri::Webview, info: &mut BrowserPaneInfo) -> Result<(), String> {
    let target = if info.keep_rendering {
        Placement::Parked
    } else {
        Placement::Hidden
    };
    if info.placement == target {
        return Ok(());
    }
    if target == Placement::Parked {
        webview
            .set_bounds(parked_rect(info.viewport))
            .map_err(|e| e.to_string())?;
        webview.show().map_err(|e| e.to_string())?;
        // 收起后视口换成 parked 尺寸，模拟的缩放比例跟着重算。
        if let Some(metrics) = info.emulation.as_ref().and_then(|e| e.metrics.as_ref()) {
            crate::browser_emulation::refit(webview, metrics, info.viewport);
        }
    } else {
        webview.hide().map_err(|e| e.to_string())?;
    }
    // 下发都成功了才记：记早了，失败后 keep_rendering 会以为它还在屏上而跳过。
    info.placement = target;
    Ok(())
}

/// agent 要操作这个页签：从此收起时也保持渲染；眼下是藏着的就挪到窗口外露出来
/// （面板收起是常态，agent 多数时候是在用户看不到面板时操作网页）。
pub(crate) fn keep_rendering(
    app: &tauri::AppHandle,
    state: &BrowserPanes,
    id: u32,
) -> Result<(), String> {
    let webview = webview(app, id)?;
    let mut map = state.panes.lock().map_err(|e| e.to_string())?;
    let info = map.get_mut(&id).ok_or("browser_pane/not_found")?;
    info.keep_rendering = true;
    if info.placement == Placement::OnScreen {
        return Ok(());
    }
    conceal(&webview, info)
}

/// 前端占位区域的位置（CSS 像素，相对窗口内容区左上角）。visible=false 时让开：
/// 页签切走、面板收起、换会话、有菜单或弹窗盖过来，原生视图都得让开（见 `conceal`）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn browser_pane_set_bounds(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    visible: bool,
) -> Result<(), String> {
    let webview = webview(&app, id)?;
    let mut map = state.panes.lock().map_err(|e| e.to_string())?;
    let info = map.get_mut(&id).ok_or("browser_pane/not_found")?;
    if !visible || width < 1.0 || height < 1.0 {
        return conceal(&webview, info);
    }
    // 记下面板里的尺寸，收起后 agent 接着用时页面视口不变，排版不会跳。
    info.viewport = parked_viewport((width, height));
    webview
        .set_bounds(Rect {
            position: LogicalPosition::new(x, y).into(),
            size: LogicalSize::new(width, height).into(),
        })
        .map_err(|e| e.to_string())?;
    webview.show().map_err(|e| e.to_string())?;
    // 面板拖宽拖窄：模拟尺寸不变，按新面板重算缩放，整页仍在面板里看得到。
    let resized = info.shown != (width, height) || info.placement != Placement::OnScreen;
    info.shown = (width, height);
    info.placement = Placement::OnScreen;
    if resized {
        if let Some(metrics) = info.emulation.as_ref().and_then(|e| e.metrics.as_ref()) {
            crate::browser_emulation::refit(&webview, metrics, (width, height));
        }
    }
    Ok(())
}

/// 页签此刻给页面的实际视口：在面板上就是面板尺寸，收起时是 parked 尺寸。
fn view_size(info: &BrowserPaneInfo) -> (f64, f64) {
    if info.placement == Placement::OnScreen {
        info.shown
    } else {
        info.viewport
    }
}

/// 把页签的设备模拟整体换成 `next`（None = 还原）。CDP 全部成功才记状态；返回是否需要刷新
/// （UA 或请求头变了）。必须在非主线程调用：CDP 的回调在主线程。
pub(crate) fn set_emulation(
    app: &tauri::AppHandle,
    state: &BrowserPanes,
    id: u32,
    next: Option<crate::browser_emulation::Emulation>,
) -> Result<bool, String> {
    let (before, view) = {
        let map = state.panes.lock().map_err(|e| e.to_string())?;
        let info = map.get(&id).ok_or("browser_pane/not_found")?;
        (info.emulation.clone(), view_size(info))
    };
    let webview = webview(app, id)?;
    crate::browser_emulation::run_calls(
        &webview,
        crate::browser_emulation::calls(next.as_ref(), view),
    )?;
    let reload = crate::browser_emulation::needs_reload(before.as_ref(), next.as_ref());
    update(app, &state.panes, id, |info| info.emulation = next);
    Ok(reload)
}

/// 用户在「…」菜单里选设备或还原（只给预设，自定义尺寸和请求头留给 Agent 的工具）。
#[tauri::command]
pub(crate) async fn browser_pane_emulate(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    device: String,
) -> Result<(), String> {
    ensure_human_action_allowed(&state, id)?;
    let next = crate::browser_emulation::parse(&serde_json::json!({ "device": device }))?;
    if set_emulation(&app, &state, id, next)? {
        webview(&app, id)?.reload().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn browser_pane_navigate(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    url: String,
) -> Result<(), String> {
    ensure_human_action_allowed(&state, id)?;
    let target = parse_target(&app, &url)?;
    webview(&app, id)?
        .navigate(target)
        .map_err(|e| e.to_string())
}

/// 后退 / 前进 / 刷新。前两个 Tauri 没有原生接口，用页面自己的 history。
#[tauri::command]
pub(crate) async fn browser_pane_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    action: String,
) -> Result<(), String> {
    ensure_human_action_allowed(&state, id)?;
    let webview = webview(&app, id)?;
    // 缩放档位同 Chromium（Ctrl+滚轮也能缩，WebView2 自带）。
    const STEPS: &[f64] = &[0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];
    let zoom_to = |next: f64| -> Result<(), String> {
        webview.set_zoom(next).map_err(|e| e.to_string())?;
        update(&app, &state.panes, id, |info| info.zoom = next);
        Ok(())
    };
    let current = state
        .panes
        .lock()
        .ok()
        .and_then(|map| map.get(&id).map(|info| info.zoom))
        .unwrap_or(1.0);
    match action.as_str() {
        "back" => webview.eval("history.back()").map_err(|e| e.to_string()),
        "forward" => webview.eval("history.forward()").map_err(|e| e.to_string()),
        "reload" => webview.reload().map_err(|e| e.to_string()),
        "print" => webview.print().map_err(|e| e.to_string()),
        "zoom_in" => zoom_to(
            STEPS
                .iter()
                .copied()
                .find(|step| *step > current + 0.001)
                .unwrap_or(current),
        ),
        "zoom_out" => zoom_to(
            STEPS
                .iter()
                .rev()
                .copied()
                .find(|step| *step < current - 0.001)
                .unwrap_or(current),
        ),
        "zoom_reset" => zoom_to(1.0),
        _ => Err("browser_pane/bad_action".into()),
    }
}

/// 「接管」/「交还」：接管后立即解开网页人工输入，并拒绝后续 Agent 操作。
#[tauri::command]
pub(crate) fn browser_pane_set_agent_paused(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    paused: bool,
) {
    update(&app, &state.panes, id, |info| {
        info.agent_generation = info.agent_generation.wrapping_add(1);
        info.agent_paused = paused;
        if paused {
            info.agent_active = false;
        }
    });
    let input_locked = state
        .panes
        .lock()
        .ok()
        .and_then(|map| {
            map.get(&id)
                .map(|info| !control_allows_human_input(info.agent_active, info.agent_paused))
        })
        .unwrap_or(false);
    set_agent_input_locked(&app, id, input_locked);
}

#[tauri::command]
pub(crate) async fn browser_pane_close(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
) -> Result<(), String> {
    if let Ok(mut panes) = state.panes.lock() {
        panes.remove(&id);
    }
    crate::browser_permission::forget(&app, id);
    if let Ok(webview) = webview(&app, id) {
        webview.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 面板里对网页权限请求点了「允许 / 阻止」。permission 是用户看到的那条请求的编号：
/// 队首已经换成别的请求（页面刚跳走、新请求顶上来）时不答，免得把「允许」给了没显示的权限。
#[tauri::command]
pub(crate) async fn browser_pane_answer_permission(
    app: tauri::AppHandle,
    id: u32,
    permission: u32,
    allow: bool,
) -> Result<(), String> {
    crate::browser_permission::answer(&app, id, permission, allow)
}

/// 下载完成后的「打开文件夹」：在系统文件管理器里选中下好的文件，不打开也不运行它（下载物
/// 可能是可执行文件）。路径取后端记下的，不信前端传来的。
#[tauri::command]
pub(crate) async fn browser_pane_reveal_download(
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    download: u32,
) -> Result<(), String> {
    let path = state
        .panes
        .lock()
        .map_err(|e| e.to_string())?
        .get(&id)
        .ok_or("browser_pane/not_found")?
        .downloads
        .iter()
        .find(|entry| entry.id == download && entry.state == DownloadState::Done)
        .and_then(|entry| entry.path.clone())
        .ok_or("browser_pane/download_missing")?;
    tauri::async_runtime::spawn_blocking(move || {
        if !path.exists() {
            return Err("browser_pane/download_missing".to_string());
        }
        crate::fsutil::reveal_target(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_list_tracks_start_finish_and_failure() {
        let mut list = Vec::new();
        download_started(&mut list, "https://a.test/f.txt", Path::new("C:/dl/f.txt"));
        download_started(&mut list, "https://a.test/g.zip", Path::new("C:/dl/g.zip"));
        assert_eq!(list[0].file_name, "f.txt");
        assert_eq!(list[0].state, DownloadState::Progress);

        // 完成事件给的路径为准（WebView2 重名时另起名）。
        assert!(download_finished(
            &mut list,
            "https://a.test/f.txt",
            Some(PathBuf::from("C:/dl/f (1).txt")),
            true
        ));
        assert_eq!(list[0].state, DownloadState::Done);
        assert_eq!(list[0].file_name, "f (1).txt");

        // macOS 完成时没有路径：保留开始时记下的。
        assert!(download_finished(
            &mut list,
            "https://a.test/g.zip",
            None,
            false
        ));
        assert_eq!(list[1].state, DownloadState::Failed);
        assert_eq!(list[1].path.as_deref(), Some(Path::new("C:/dl/g.zip")));

        // 已经结束的不会再被改；对不上号返回 false。
        assert!(!download_finished(
            &mut list,
            "https://a.test/f.txt",
            None,
            false
        ));
        assert_eq!(list[0].state, DownloadState::Done);
    }

    #[test]
    fn same_url_downloaded_twice_finishes_the_latest_one() {
        let mut list = Vec::new();
        download_started(&mut list, "https://a.test/f.txt", Path::new("C:/dl/f.txt"));
        download_started(
            &mut list,
            "https://a.test/f.txt",
            Path::new("C:/dl/f (1).txt"),
        );
        download_finished(&mut list, "https://a.test/f.txt", None, true);
        assert_eq!(list[0].state, DownloadState::Progress);
        assert_eq!(list[1].state, DownloadState::Done);
    }

    #[test]
    fn download_list_keeps_only_the_latest() {
        let mut list = Vec::new();
        for n in 0..(MAX_DOWNLOADS + 3) {
            download_started(&mut list, &format!("https://a.test/{n}"), Path::new(""));
        }
        assert_eq!(list.len(), MAX_DOWNLOADS);
        // 没有文件名时取网址最后一段。
        assert_eq!(
            list.last().unwrap().file_name,
            format!("{}", MAX_DOWNLOADS + 2)
        );
        let ids: Vec<u32> = list.iter().map(|entry| entry.id).collect();
        assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn unique_path_never_overwrites_existing_files() {
        let dir = std::env::temp_dir().join(format!("meowo-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let free = |_: &Path| false;
        let first = dir.join("report.csv");
        assert_eq!(unique_path(&first, free), first);
        std::fs::write(&first, "a").unwrap();
        std::fs::write(dir.join("report (1).csv"), "b").unwrap();
        assert_eq!(unique_path(&first, free), dir.join("report (2).csv"));
        // 还在下载、没落盘的也算占用（并发同名下载实测会互相覆盖）。
        let downloading = dir.join("report (2).csv");
        assert_eq!(
            unique_path(&first, |candidate| candidate == downloading),
            dir.join("report (3).csv")
        );
        let fresh = dir.join("fresh.txt");
        assert_eq!(
            unique_path(&fresh, |candidate| candidate == fresh),
            dir.join("fresh (1).txt")
        );
        // 没有扩展名也行。
        let bare = dir.join("README");
        std::fs::write(&bare, "c").unwrap();
        assert_eq!(unique_path(&bare, free), dir.join("README (1)"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn permission_prompt_shows_host_and_has_unique_ids() {
        let prompt = BrowserPermission::new("downloads", "https://httpbin.org/html?x=1");
        assert_eq!(prompt.host, "httpbin.org");
        let json = serde_json::to_value(&prompt).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "id": prompt.id, "kind": "downloads", "host": "httpbin.org" })
        );
        assert_eq!(prompt.describe(), "httpbin.org 想要下载多个文件");
        // 每条请求编号不同（前端据此解锁按钮）。
        assert_ne!(
            BrowserPermission::new("downloads", "https://httpbin.org/").id,
            prompt.id
        );
        assert_eq!(
            BrowserPermission::new("window_management", "https://a.test/").describe(),
            "a.test 想要使用额外权限"
        );
    }

    #[test]
    fn download_serializes_without_url_or_path() {
        let mut list = Vec::new();
        download_started(&mut list, "https://a.test/f.txt", Path::new("C:/dl/f.txt"));
        let json = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(json["fileName"], "f.txt");
        assert_eq!(json["state"], "progress");
        assert!(
            json.get("url").is_none() && json.get("urlKey").is_none() && json.get("path").is_none()
        );
    }

    #[test]
    fn address_bar_input_becomes_a_url() {
        assert_eq!(
            normalize_input("localhost:5173").as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(
            normalize_input("127.0.0.1:8080/app").as_deref(),
            Some("http://127.0.0.1:8080/app")
        );
        assert_eq!(
            normalize_input("github.com/larrygogo").as_deref(),
            Some("https://github.com/larrygogo")
        );
        assert_eq!(
            normalize_input(" https://kimi.com ").as_deref(),
            Some("https://kimi.com")
        );
        assert_eq!(
            normalize_input("tauri 子 webview").as_deref(),
            Some("https://www.bing.com/search?q=tauri+%E5%AD%90+webview")
        );
        assert_eq!(normalize_input("   "), None);
    }

    #[test]
    fn parked_viewport_follows_the_panel_but_never_gets_tiny() {
        // 正常宽度的面板：原样沿用，收起前后排版一致。
        assert_eq!(parked_viewport((520.0, 700.0)), (520.0, 700.0));
        assert_eq!(parked_viewport((1600.0, 900.0)), (1600.0, 900.0));
        // 拖得很窄或布局变动时量到的瞬时小矩形：抬到下限，不回到 2×2 那种视口。
        assert_eq!(parked_viewport((120.0, 700.0)), (480.0, 700.0));
        assert_eq!(parked_viewport((1.0, 1.0)), (480.0, 480.0));
    }

    #[test]
    fn parked_rect_keeps_the_viewport_and_sits_outside_the_window() {
        let rect = parked_rect(DEFAULT_VIEWPORT);
        let size = rect.size.to_logical::<f64>(1.5);
        let position = rect.position.to_logical::<f64>(1.5);
        assert_eq!((size.width, size.height), (1280.0, 800.0));
        // 整块在内容区左侧外面：右边缘 < 0，用户看不到。
        assert!(position.x + size.width < 0.0);
        assert_eq!(position.y, 0.0);

        let rect = parked_rect((520.0, 700.0));
        let size = rect.size.to_logical::<f64>(1.0);
        let position = rect.position.to_logical::<f64>(1.0);
        assert_eq!((size.width, size.height), (520.0, 700.0));
        assert!(position.x + size.width < 0.0);
    }

    #[test]
    fn agent_control_owns_human_navigation_until_takeover() {
        assert!(!control_allows_human_input(true, false));
        assert!(control_allows_human_input(true, true));
        assert!(control_allows_human_input(false, false));
        assert!(control_allows_human_input(false, true));
    }

    #[test]
    fn meowo_own_origins_are_blocked() {
        let url = |s: &str| Url::parse(s).unwrap();
        assert!(allowed(&url("http://localhost:5173/"), None));
        assert!(allowed(&url("https://example.com/"), None));
        assert!(allowed(&url("about:blank"), None));
        // Windows 上 Meowo 前端自己的来源；到了这里页面就能调 Meowo 的命令。
        assert!(!allowed(&url("http://tauri.localhost/"), None));
        assert!(!allowed(&url("http://ipc.localhost/x"), None));
        assert!(!allowed(&url("tauri://localhost/"), None));
        assert!(!allowed(&url("file:///C:/Windows/win.ini"), None));
        assert!(!allowed(&url("javascript:alert(1)"), None));
        let dev = url("http://localhost:1420/");
        assert!(!allowed(
            &url("http://localhost:1420/chat.html"),
            Some(&dev)
        ));
        assert!(allowed(&url("http://localhost:5173/"), Some(&dev)));
    }
}
