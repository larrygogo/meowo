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
//! 子 webview 随 chat 窗口一起销毁，窗口销毁时这里只清表。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewBuilder, WebviewUrl};

const BLANK: &str = "about:blank";

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
}

#[derive(Default)]
pub(crate) struct BrowserPanes {
    next_id: Mutex<u32>,
    pub(crate) panes: Arc<Mutex<HashMap<u32, BrowserPaneInfo>>>,
}

impl BrowserPanes {
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
pub(crate) fn open_pane(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    state: &BrowserPanes,
    owner: i64,
    url: Option<&str>,
) -> Result<BrowserPaneInfo, String> {
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
    let created = window.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(1.0, 1.0),
    );
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

/// 前端占位区域的位置（CSS 像素，相对窗口内容区左上角）。visible=false 时只藏不动：
/// 页签切走、面板收起、换会话、有菜单或弹窗盖过来，原生视图都得让开。
#[tauri::command]
pub(crate) async fn browser_pane_set_bounds(
    app: tauri::AppHandle,
    id: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    visible: bool,
) -> Result<(), String> {
    let webview = webview(&app, id)?;
    if !visible || width < 1.0 || height < 1.0 {
        return webview.hide().map_err(|e| e.to_string());
    }
    webview
        .set_position(LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    webview
        .set_size(LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    webview.show().map_err(|e| e.to_string())
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
    id: u32,
    action: String,
) -> Result<(), String> {
    let webview = webview(&app, id)?;
    match action.as_str() {
        "back" => webview.eval("history.back()"),
        "forward" => webview.eval("history.forward()"),
        "reload" => webview.reload(),
        _ => return Err("browser_pane/bad_action".into()),
    }
    .map_err(|e| e.to_string())
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
