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
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    Emitter, LogicalPosition, LogicalSize, Manager, Rect, Url, WebviewBuilder, WebviewUrl,
};

const BLANK: &str = "about:blank";

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
    /// 页面缩放比例（1.0 = 100%），「…」菜单里调。
    pub(crate) zoom: f64,
    // 以下三项只在后端用，不发给前端。
    /// 原生视图眼下摆在哪。
    #[serde(skip)]
    placement: Placement,
    /// 收起时给页面留的视口：上一次在面板里显示时的尺寸（经 `parked_viewport` 抬到下限），
    /// 没显示过就是 DEFAULT_VIEWPORT。
    #[serde(skip)]
    viewport: (f64, f64),
    /// agent 用过这个页签：收起时挪到窗口外接着渲染，而不是藏起来。见 `Placement`。
    #[serde(skip)]
    keep_rendering: bool,
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
        zoom: 1.0,
        placement: Placement::Hidden,
        viewport: DEFAULT_VIEWPORT,
        keep_rendering: false,
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
    let builder = WebviewBuilder::new(label_of(id), WebviewUrl::External(target))
        // 页内跳转同样过一遍白名单：页面自己把 location 改到 tauri.localhost 也拦得住。
        .on_navigation(move |url| allowed(url, nav_dev.as_ref()))
        .on_page_load(move |_, payload| {
            let url = payload.url().to_string();
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            update(&load_app, &load_panes, id, |info| {
                info.url = url;
                info.loading = loading;
            });
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
    info.placement = Placement::OnScreen;
    Ok(())
}

#[tauri::command]
pub(crate) async fn browser_pane_navigate(
    app: tauri::AppHandle,
    id: u32,
    url: String,
) -> Result<(), String> {
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

/// 「接管」/「交还」：接管后 agent 的浏览器操作一律拒绝，用户自己操作页面不受影响。
#[tauri::command]
pub(crate) fn browser_pane_set_agent_paused(
    app: tauri::AppHandle,
    state: tauri::State<'_, BrowserPanes>,
    id: u32,
    paused: bool,
) {
    update(&app, &state.panes, id, |info| {
        info.agent_paused = paused;
        if paused {
            info.agent_active = false;
        }
    });
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
    if let Ok(webview) = webview(&app, id) {
        webview.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
