//! agent 操作对话窗面板里的浏览器（用户要求：「像 Kimi Code 那样 agent 能自己操作这个浏览器」）。
//!
//! 链路：agent ⇄ `meowo-reporter mcp`（stdio MCP 服务器，agent 自己拉起）⇄ broker 的 TCP
//! 通道（`BrokerRequest::Browser`，凭注入 agent 环境的 scoped token）⇄ 这里 ⇄ 面板里的
//! 原生子 webview（browser_pane.rs）。
//!
//! - 会话归属来自 scoped token：agent 只能操作自己会话的面板浏览器，碰不到别的会话的。
//! - 读页面、点击、输入都是往页面里注入一段脚本，用 `eval_with_callback` 把结果取回来。
//!   页面元素按出现顺序编号（`data-meowo-ref="e12"`），agent 先 `browser_snapshot` 拿编号，
//!   再按编号操作——和 Playwright MCP 的做法一样，不让 agent 自己拼 CSS 选择器。
//! - 用户点了「接管」（`agent_paused`），所有操作直接拒绝。
//! - 动作类工具做完自动回一份新快照，agent 不用再多调一次。

use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::Manager;

use crate::browser_pane::{self, BrowserPaneInfo, BrowserPanes};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub(crate) fn init(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// 一次脚本执行最多等多久。页面卡死（死循环、同步弹窗）时不能把 agent 一直挂着。
const EVAL_TIMEOUT: Duration = Duration::from_secs(15);
/// 跳转后等页面加载完的上限。
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// 「agent 正在操作」的提示在最后一次操作后保留多久。
const ACTIVE_LINGER: Duration = Duration::from_secs(6);

/// 每会话 agent 正在用的那个浏览器页签（会话 id → 页签编号）。
fn agent_panes() -> &'static Mutex<std::collections::HashMap<i64, u32>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<i64, u32>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// broker 收到 `BrokerRequest::Browser` 后调这里。返回给 agent 的文字；出错返回原因。
/// 返回给 agent 的内容：`{"text": …}`，截图另带 `{"image": base64, "mime": …}`。
pub(crate) fn handle(session_id: i64, tool: &str, args: &Value) -> Result<Value, String> {
    // 开关关掉后，已经在跑的会话手上还挂着这组工具（启动参数/配置文件是开的时候给的），
    // 每次调用都要再看一眼，不然「关掉」管不住正在跑的 agent。
    if !crate::settings::load_settings().browser_agent {
        return Err("用户在 Meowo 设置里关掉了「允许 agent 操作面板浏览器」，这组工具现在不能用。".into());
    }
    let app = APP.get().ok_or("Meowo 还没启动完")?;
    let panes = app.state::<BrowserPanes>();
    let id = pane_for(app, &panes, session_id, tool == "browser_open")?;
    let paused = panes
        .panes
        .lock()
        .ok()
        .and_then(|map| map.get(&id).map(|info| info.agent_paused))
        .unwrap_or(false);
    if paused {
        return Err("用户接管了面板浏览器，暂时不能操作。等用户在页签上点「交还」后再试。".into());
    }
    // 面板收起时页签是藏着的：页面进 document.hidden、视口可能只有 2×2，滚动检测、
    // IntersectionObserver、截图全不工作（2026-11-09 Discord 授权页实测）。先让它接着渲染。
    // 放在接管检查之后：被拒的调用不该改动页签。挪不动也照常执行，快照、点击这些不靠可见。
    if let Err(error) = browser_pane::keep_rendering(app, &panes, id) {
        eprintln!("[browser] 页签 {id} 保持渲染失败：{error}");
    }
    mark_active(app, &panes, id);
    let result = if tool == "browser_screenshot" {
        screenshot(app, id)
    } else {
        run(app, id, tool, args).map(|text| json!({ "text": text }))
    };
    schedule_idle(app, id);
    result
}

/// 截下面板浏览器当前的画面（JPEG）。agent 读快照只拿得到文字，看排版、样式、图表得靠它。
fn screenshot(app: &tauri::AppHandle, id: u32) -> Result<Value, String> {
    let webview = browser_pane::webview(app, id)?;
    let bytes = capture_jpeg(&webview)?;
    use base64::Engine;
    let title = eval(app, id, "document.title + '\n' + location.href")
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    Ok(json!({
        "text": format!("面板浏览器截图（{} KB）
    {title}", bytes.len() / 1024),
        "image": base64::engine::general_purpose::STANDARD.encode(&bytes),
        "mime": "image/jpeg",
    }))
}

/// WebView2 的 CapturePreview：截的是网页本身的渲染结果（不含面板外的界面），网页被
/// 菜单挡着时也截得到。回调在主线程，这里在 broker 线程上等结果。
#[cfg(windows)]
fn capture_jpeg(webview: &tauri::Webview) -> Result<Vec<u8>, String> {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG;
    use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
    use windows::Win32::UI::Shell::SHCreateMemStream;

    fn read_all(stream: &IStream) -> windows::core::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        unsafe {
            stream.Seek(0, STREAM_SEEK_SET, None)?;
            loop {
                let mut read = 0u32;
                let _ = stream.Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&mut read),
                );
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..read as usize]);
            }
        }
        Ok(bytes)
    }

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    let fail = tx.clone();
    webview
        .with_webview(move |platform| {
            let started = (|| -> windows::core::Result<()> {
                let core = unsafe { platform.controller().CoreWebView2()? };
                let stream = unsafe { SHCreateMemStream(None) }
                    .ok_or_else(windows::core::Error::from_thread)?;
                let reader = stream.clone();
                let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                    let _ = tx.send(
                        result
                            .and_then(|_| read_all(&reader))
                            .map_err(|e| e.to_string()),
                    );
                    Ok(())
                }));
                unsafe {
                    core.CapturePreview(
                        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
                        &stream,
                        &handler,
                    )
                }
            })();
            if let Err(error) = started {
                let _ = fail.send(Err(error.to_string()));
            }
        })
        .map_err(|e| e.to_string())?;
    let bytes = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "截图超时".to_string())??;
    if bytes.is_empty() {
        return Err("截图是空的（面板浏览器可能被收起了）".into());
    }
    Ok(bytes)
}

/// macOS：WKWebView 的 takeSnapshotWithConfiguration:completionHandler: 拿 NSImage，
/// 经 NSBitmapImageRep 转成 JPEG。回调在主线程，这里在 broker 线程上等结果。
/// （未经本机编译：Windows 开发机编不到 cfg(macos)，靠 CI 的 macOS 任务把关。）
#[cfg(target_os = "macos")]
fn capture_jpeg(webview: &tauri::Webview) -> Result<Vec<u8>, String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    webview
        .with_webview(move |platform| {
            let wk = platform.inner() as *mut AnyObject;
            if wk.is_null() {
                let _ = tx.send(Err("拿不到网页视图".into()));
                return;
            }
            let done = tx.clone();
            let handler = RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
                let result = unsafe { jpeg_from_nsimage(image) };
                let _ = done.send(result);
            });
            unsafe {
                let _: () = msg_send![
                    wk,
                    takeSnapshotWithConfiguration: std::ptr::null_mut::<AnyObject>(),
                    completionHandler: &*handler
                ];
            }
        })
        .map_err(|e| e.to_string())?;

    /// NSImage → TIFF → NSBitmapImageRep → JPEG（NSBitmapImageFileTypeJPEG = 3）。
    unsafe fn jpeg_from_nsimage(image: *mut AnyObject) -> Result<Vec<u8>, String> {
        if image.is_null() {
            return Err("截图失败（面板浏览器可能被收起了）".into());
        }
        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        if tiff.is_null() {
            return Err("截图转码失败".into());
        }
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        if rep.is_null() {
            return Err("截图转码失败".into());
        }
        let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let data: *mut AnyObject =
            msg_send![rep, representationUsingType: 3usize, properties: props];
        if data.is_null() {
            return Err("截图转码失败".into());
        }
        let len: usize = msg_send![data, length];
        let ptr: *const u8 = msg_send![data, bytes];
        if ptr.is_null() || len == 0 {
            return Err("截图是空的".into());
        }
        Ok(std::slice::from_raw_parts(ptr, len).to_vec())
    }

    let bytes = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "截图超时".to_string())??;
    Ok(bytes)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn capture_jpeg(_webview: &tauri::Webview) -> Result<Vec<u8>, String> {
    Err("面板浏览器截图暂不支持这个平台".into())
}

/// 找到（必要时新建）这个会话给 agent 用的浏览器页签。
fn pane_for(
    app: &tauri::AppHandle,
    panes: &BrowserPanes,
    session_id: i64,
    open: bool,
) -> Result<u32, String> {
    let alive = |id: u32| {
        panes
            .panes
            .lock()
            .ok()
            .is_some_and(|map| map.get(&id).is_some_and(|info| info.owner == session_id))
    };
    if let Some(id) = agent_panes()
        .lock()
        .ok()
        .and_then(|map| map.get(&session_id).copied())
    {
        if alive(id) {
            return Ok(id);
        }
    }
    // agent 还没用过：用户自己开着的最新一个浏览器页签就给它用；一个都没有再新建。
    if !open {
        let existing = panes.panes.lock().ok().and_then(|map| {
            map.values()
                .filter(|info| info.owner == session_id)
                .map(|info| info.id)
                .max()
        });
        if let Some(id) = existing {
            remember(session_id, id);
            return Ok(id);
        }
    }
    let window = chat_window(app, session_id)?;
    let info = browser_pane::open_pane(app, &window, panes, session_id, None).map_err(|e| {
        if e == "browser_pane/too_many" {
            "面板浏览器开太多了（最多 16 个），请用户先关掉一些页签".to_string()
        } else {
            e
        }
    })?;
    remember(session_id, info.id);
    announce(app, &info);
    Ok(info.id)
}

fn remember(session_id: i64, id: u32) {
    if let Ok(mut map) = agent_panes().lock() {
        map.insert(session_id, id);
    }
}

/// 新建的页签要让前端知道：它会加进这个会话的面板，正在看这个会话的话直接切过去显示。
fn announce(app: &tauri::AppHandle, info: &BrowserPaneInfo) {
    use tauri::Emitter;
    let _ = app.emit_to(
        tauri::EventTarget::webview("chat"),
        "browser-pane-created",
        info,
    );
}

/// chat 窗口。没开就安静地开一个（不抢焦点），等它建好。
fn chat_window(app: &tauri::AppHandle, session_id: i64) -> Result<tauri::Window, String> {
    if let Some(window) = app.get_window("chat") {
        return Ok(window);
    }
    crate::window::open_chat_window_quiet(app.clone(), session_id);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(window) = app.get_window("chat") {
            // 前端挂上 browser-pane-created 监听要一点时间，太早发事件会丢。
            std::thread::sleep(Duration::from_millis(1500));
            return Ok(window);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("对话窗没能打开，面板浏览器用不了".into())
}

fn mark_active(app: &tauri::AppHandle, panes: &BrowserPanes, id: u32) {
    browser_pane::update(app, &panes.panes, id, |info| info.agent_active = true);
    if let Ok(mut last) = last_used().lock() {
        last.insert(id, Instant::now());
    }
}

fn last_used() -> &'static Mutex<std::collections::HashMap<u32, Instant>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<u32, Instant>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// 最后一次操作过去 ACTIVE_LINGER 后撤掉「正在操作」提示（期间又有操作就顺延）。
fn schedule_idle(app: &tauri::AppHandle, id: u32) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ACTIVE_LINGER);
        let idle = last_used()
            .lock()
            .ok()
            .and_then(|map| map.get(&id).copied())
            .is_none_or(|at| at.elapsed() >= ACTIVE_LINGER);
        if idle {
            let panes = app.state::<BrowserPanes>();
            browser_pane::update(&app, &panes.panes, id, |info| info.agent_active = false);
            // agent 停手了，页面上的 agent 鼠标也淡出。
            let _ = eval(&app, id, "H.hideCursor()");
        }
    });
}

/// 每个页签上 agent 鼠标最后停在哪（视口坐标）。跳到新页面后鼠标元素没了，从这里接着画，
/// 不会每次都从角落飞过来。
fn cursor_positions() -> &'static Mutex<std::collections::HashMap<u32, (f64, f64)>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<u32, (f64, f64)>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// 让页面上的 agent 鼠标移到元素上，等动画走完再动手（用户要求「能看到 agent 的鼠标」，
/// 同 Kimi Code 的 browserControlPointer）。鼠标只是画出来的，不影响真正的点击。
fn point(app: &tauri::AppHandle, id: u32, r: &str) -> Result<(), String> {
    let last = cursor_positions()
        .lock()
        .ok()
        .and_then(|map| map.get(&id).copied());
    let place = match last {
        Some((x, y)) => format!("H.place({x}, {y}), "),
        None => String::new(),
    };
    let at = eval(app, id, &format!("({place}H.point({}))", js_str(r)))?;
    if let (Some(x), Some(y)) = (
        at.get("x").and_then(Value::as_f64),
        at.get("y").and_then(Value::as_f64),
    ) {
        if let Ok(mut map) = cursor_positions().lock() {
            map.insert(id, (x, y));
        }
    }
    // 和页面里鼠标的移动动画（CURSOR_MOVE_MS）对齐，再多留一点让人看清落点。
    std::thread::sleep(Duration::from_millis(480));
    Ok(())
}

fn run(app: &tauri::AppHandle, id: u32, tool: &str, args: &Value) -> Result<String, String> {
    let str_arg = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
    match tool {
        "browser_open" | "browser_navigate" => {
            let Some(url) = str_arg("url") else {
                return if tool == "browser_open" {
                    snapshot(app, id)
                } else {
                    Err("缺少 url".into())
                };
            };
            let target = browser_pane::parse_target(app, &url).map_err(|e| match e.as_str() {
                "browser_pane/blocked_url" => "面板浏览器只能打开 http/https 网页".to_string(),
                _ => format!("网址无效：{url}"),
            })?;
            browser_pane::webview(app, id)?
                .navigate(target)
                .map_err(|e| e.to_string())?;
            wait_loaded(app, id);
            snapshot(app, id)
        }
        "browser_back" | "browser_forward" => {
            let js = if tool == "browser_back" {
                "history.back()"
            } else {
                "history.forward()"
            };
            browser_pane::webview(app, id)?
                .eval(js)
                .map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(400));
            wait_loaded(app, id);
            snapshot(app, id)
        }
        "browser_reload" => {
            browser_pane::webview(app, id)?
                .reload()
                .map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(400));
            wait_loaded(app, id);
            snapshot(app, id)
        }
        "browser_snapshot" => snapshot(app, id),
        "browser_click" => {
            let r = ref_arg(args)?;
            point(app, id, &r)?;
            act(app, id, &format!("H.click({})", js_str(&r)))
        }
        "browser_type" => {
            let r = ref_arg(args)?;
            point(app, id, &r)?;
            let text = str_arg("text").unwrap_or_default();
            let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
            act(
                app,
                id,
                &format!("H.type({}, {}, {})", js_str(&r), js_str(&text), submit),
            )
        }
        "browser_select" => {
            let r = ref_arg(args)?;
            point(app, id, &r)?;
            let value = str_arg("value").unwrap_or_default();
            act(
                app,
                id,
                &format!("H.select({}, {})", js_str(&r), js_str(&value)),
            )
        }
        "browser_press" => {
            let key = str_arg("key").ok_or("缺少 key")?;
            act(app, id, &format!("H.press({})", js_str(&key)))
        }
        "browser_scroll" => {
            let dy = args.get("dy").and_then(Value::as_f64).unwrap_or(600.0);
            let target = str_arg("ref");
            let expr = match target {
                Some(r) => {
                    if !r.chars().all(|c| c.is_ascii_alphanumeric()) {
                        return Err(format!("元素编号无效：{r}"));
                    }
                    point(app, id, &r)?;
                    format!("H.scrollTo({})", js_str(&r))
                }
                None => format!("(window.scrollBy(0, {dy}), 'scrolled')"),
            };
            act(app, id, &expr)
        }
        "browser_wait_for" => {
            let text = str_arg("text");
            let seconds = args
                .get("seconds")
                .and_then(Value::as_f64)
                .unwrap_or(10.0)
                .clamp(0.1, 60.0);
            let deadline = Instant::now() + Duration::from_secs_f64(seconds);
            match text {
                None => {
                    std::thread::sleep(Duration::from_secs_f64(seconds));
                    snapshot(app, id)
                }
                Some(text) => loop {
                    let found = eval(
                        app,
                        id,
                        &format!(
                            "(document.body && document.body.innerText || '').includes({})",
                            js_str(&text)
                        ),
                    )
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                    if found {
                        return snapshot(app, id);
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("{seconds} 秒内页面上没有出现「{text}」"));
                    }
                    std::thread::sleep(Duration::from_millis(300));
                },
            }
        }
        "browser_evaluate" => {
            let expr = str_arg("expression").ok_or("缺少 expression")?;
            // 支持异步表达式（await fetch(...) 这类）：eval_with_callback 不等 Promise，直接拿
            // 只会得到 {}。先在页面里把结果（同步的也一样）存进一个槽位，再轮询取回。
            let slot = format!(
                "e{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            eval(
                app,
                id,
                &format!(
                    "(() => {{ const box = (window.__meowoEval = window.__meowoEval || {{}});                      Promise.resolve().then(async () => ({expr})).then(                        (v) => {{ box['{slot}'] = {{ ok: true, value: v === undefined ? null : v }}; }},                        (e) => {{ box['{slot}'] = {{ ok: false, error: String(e && e.message || e) }}; }});                      return true; }})()"
                ),
            )?;
            let deadline = Instant::now() + EVAL_TIMEOUT;
            let value = loop {
                let got = eval(app, id, &format!("(() => {{ const box = window.__meowoEval || {{}}; const r = box['{slot}']; if (r) delete box['{slot}']; return r || null; }})()"))?;
                if !got.is_null() {
                    if got.get("ok").and_then(Value::as_bool) == Some(true) {
                        break got.get("value").cloned().unwrap_or(Value::Null);
                    }
                    return Err(got
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("脚本出错")
                        .to_string());
                }
                if Instant::now() >= deadline {
                    return Err("脚本 15 秒内没有返回结果".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            Ok(match value {
                Value::String(s) => s,
                other => other.to_string(),
            })
        }
        other => Err(format!("不认识的浏览器工具：{other}")),
    }
}

fn ref_arg(args: &Value) -> Result<String, String> {
    let r = args
        .get("ref")
        .and_then(Value::as_str)
        .ok_or("缺少 ref（先用 browser_snapshot 拿元素编号）")?;
    if !r.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!("元素编号无效：{r}"));
    }
    Ok(r.to_string())
}

/// 动作做完等页面稳一下，回一份新快照（动作本身的提示放在最前面）。
fn act(app: &tauri::AppHandle, id: u32, expr: &str) -> Result<String, String> {
    let value = eval(app, id, expr)?;
    let note = match value {
        Value::String(s) => s,
        other => other.to_string(),
    };
    std::thread::sleep(Duration::from_millis(500));
    wait_loaded(app, id);
    Ok(format!("{note}\n\n{}", snapshot(app, id)?))
}

fn snapshot(app: &tauri::AppHandle, id: u32) -> Result<String, String> {
    let page = match eval(app, id, "H.snapshot()")? {
        Value::String(s) => s,
        other => other.to_string(),
    };
    let (waiting, blocked) = app
        .state::<BrowserPanes>()
        .panes
        .lock()
        .ok()
        .and_then(|map| {
            map.get(&id)
                .map(|info| (info.permission.clone(), info.blocked.clone()))
        })
        .unwrap_or((None, None));
    Ok(format!(
        "{}{page}",
        permission_note(waiting.as_ref(), blocked.as_ref())
    ))
}

/// 网页的权限请求只能由用户在面板里答（agent 不能替用户决定）。面板收起是常态，用户多半看不到
/// 提示，网页就一直卡在那（做空审查）：在 agent 拿到的页面快照最前面说清楚，让它转告用户。
fn permission_note(
    waiting: Option<&browser_pane::BrowserPermission>,
    blocked: Option<&browser_pane::BrowserPermission>,
) -> String {
    if let Some(prompt) = waiting {
        return format!(
            "注意：网页在等用户答复「{}」。只有用户能在 Meowo 面板浏览器里点允许或阻止，在此之前相关操作（如下载）会卡住。请告诉用户去处理，不要反复重试。\n\n",
            prompt.describe()
        );
    }
    if let Some(prompt) = blocked {
        return format!(
            "注意：用户已阻止「{}」，这个页面里同类操作会被静默拦下。需要的话请用户刷新页面后重试。\n\n",
            prompt.describe()
        );
    }
    String::new()
}

/// 等页面加载完（readyState complete），最多 LOAD_TIMEOUT。等不到也不报错：页面可能一直在
/// 长连接，照样能读能点。
fn wait_loaded(app: &tauri::AppHandle, id: u32) {
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while Instant::now() < deadline {
        let loading = app
            .state::<BrowserPanes>()
            .panes
            .lock()
            .ok()
            .and_then(|map| map.get(&id).map(|info| info.loading))
            .unwrap_or(false);
        let ready = !loading
            && eval(app, id, "document.readyState")
                .ok()
                .and_then(|v| v.as_str().map(|s| s == "complete"))
                .unwrap_or(false);
        if ready {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 在页面里执行一段表达式，取回它的 JSON 值。辅助脚本每次都带上（页面跳转后就没了），
/// 异常在页面里自己接住转成 `{error}`——Windows 上 eval_with_callback 会吞掉异常。
fn eval(app: &tauri::AppHandle, id: u32, expr: &str) -> Result<Value, String> {
    let webview = browser_pane::webview(app, id)?;
    let script = format!(
        "(() => {{ try {{ {HELPER}; const H = window.__meowoAgent; return JSON.stringify({{ ok: true, value: ({expr}) }}); }} catch (e) {{ return JSON.stringify({{ ok: false, error: String(e && e.message || e) }}); }} }})()"
    );
    let (tx, rx) = mpsc::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    webview
        .eval_with_callback(script, move |raw| {
            if let Some(tx) = tx.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = tx.send(raw);
            }
        })
        .map_err(|e| e.to_string())?;
    let raw = rx
        .recv_timeout(EVAL_TIMEOUT)
        .map_err(|_| "页面没有响应（可能卡住了或正在弹窗）".to_string())?;
    decode(&raw)
}

/// eval_with_callback 回来的是「结果的 JSON」，我们的结果本身又是 JSON 字符串，所以拆两层。
fn decode(raw: &str) -> Result<Value, String> {
    let inner: String = serde_json::from_str(raw)
        .map_err(|_| format!("页面返回无法解析：{}", truncate(raw, 200)))?;
    let outer: Value = serde_json::from_str(&inner)
        .map_err(|_| format!("页面返回无法解析：{}", truncate(&inner, 200)))?;
    if outer.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(outer.get("value").cloned().unwrap_or(Value::Null))
    } else {
        Err(outer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("页面脚本出错")
            .to_string())
    }
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn js_str(text: &str) -> String {
    json!(text).to_string()
}

/// 会话结束或被关闭（标记完成）时，把它在面板里开的终端和浏览器一起关掉（用户要求），
/// 再通知前端清掉这个会话的面板页签。
pub(crate) fn close_session_panels(app: &tauri::AppHandle, session_id: i64) {
    use tauri::Emitter;
    if let Some(shells) = app.try_state::<crate::shell_terminal::ShellTerminals>() {
        shells.close_owner(session_id);
    }
    if let Some(panes) = app.try_state::<BrowserPanes>() {
        panes.close_owner(app, session_id);
    }
    if let Ok(mut map) = agent_panes().lock() {
        map.remove(&session_id);
    }
    let _ = app.emit_to(
        tauri::EventTarget::webview("chat"),
        "panel-session-closed",
        session_id,
    );
}

/// 会话换了编号（新会话认领真实编号、/clear 换代）：面板里的终端和浏览器跟着换归属，
/// 再通知前端把页签记录也挪过去（broker 的 handle_claim 调）。
pub(crate) fn reassign_panels(from: i64, to: i64) {
    use tauri::Emitter;
    if from == to {
        return;
    }
    let Some(app) = APP.get() else { return };
    if let Some(shells) = app.try_state::<crate::shell_terminal::ShellTerminals>() {
        shells.reassign_owner(from, to);
    }
    if let Some(panes) = app.try_state::<BrowserPanes>() {
        panes.reassign_owner(from, to);
    }
    if let Ok(mut map) = agent_panes().lock() {
        if let Some(id) = map.remove(&from) {
            map.insert(to, id);
        }
    }
    let _ = app.emit_to(
        tauri::EventTarget::webview("chat"),
        "panel-session-moved",
        serde_json::json!({ "from": from, "to": to }),
    );
}

// ── 把 MCP 服务器交给 agent ─────────────────────────────

/// MCP 服务器在 agent 那边的名字；claude 里工具名就是 `mcp__meowo__browser_*`。
const SERVER_NAME: &str = "meowo";

/// claude 免审批的浏览器工具：打开、读、点、填这些照 Kimi Code 直接做，用户随时能接管。
/// `browser_evaluate` 不在里面——它能在已登录的网站上跑任意脚本，照常走 claude 自己的审批。
/// 和 meowo-reporter mcp.rs 的工具清单对应，加工具时两边一起改。
const AUTO_ALLOWED: &[&str] = &[
    "browser_open",
    "browser_navigate",
    "browser_snapshot",
    "browser_click",
    "browser_type",
    "browser_select",
    "browser_press",
    "browser_scroll",
    "browser_wait_for",
    "browser_back",
    "browser_forward",
    "browser_reload",
    "browser_screenshot",
];

/// 托管会话启动时追加的参数（pty.rs 的 start_spawned，外部终端启动不走这里）。各 agent 怎么接
/// 由插件的 `mcp_attach()` 声明（meowo_agent::McpAttach）：
/// - ConfigFileArg：启动参数指向配置文件，可选同时放行 AUTO_ALLOWED 里的工具——和 Kimi Code 一样
///   agent 直接操作，用户随时能在页签上「接管」。
/// - TomlOverride：配置覆盖参数，值用 TOML 单引号字符串。
/// - DataDirJson：没有启动参数可用，走数据目录里的配置文件（见 sync_mcp_files），这里不追加。
pub(crate) fn launch_args(provider: &str) -> Vec<String> {
    if !crate::settings::load_settings().browser_agent {
        return Vec::new();
    }
    let Some(reporter) = crate::setup::sibling_reporter() else {
        return Vec::new();
    };
    let Some(plugin) = meowo_agent::by_id(provider) else {
        return Vec::new();
    };
    // 怎么交给它由插件声明（McpAttach），宿主不按 agent 身份分支。
    match plugin.mcp_attach() {
        Some(meowo_agent::McpAttach::ConfigFileArg {
            file_flag,
            allow_flag,
            allow_prefix,
        }) => {
            let Some(path) = config_file(plugin.id().as_str(), &reporter) else {
                return Vec::new();
            };
            let mut args = vec![file_flag.to_string(), path];
            if let Some(flag) = allow_flag {
                args.push(flag.to_string());
                args.extend(
                    AUTO_ALLOWED
                        .iter()
                        .map(|tool| format!("{allow_prefix}{SERVER_NAME}__{tool}")),
                );
            }
            args
        }
        // TOML 单引号字符串里不能再有单引号；路径带单引号的极少见，那就不挂。
        Some(meowo_agent::McpAttach::TomlOverride { flag }) if !reporter.contains('\'') => vec![
            flag.to_string(),
            format!("mcp_servers.{SERVER_NAME}.command='{reporter}'"),
            flag.to_string(),
            format!("mcp_servers.{SERVER_NAME}.args=['mcp']"),
        ],
        _ => Vec::new(),
    }
}

fn server_entry(reporter: &str) -> Value {
    json!({ "type": "stdio", "command": reporter, "args": ["mcp"] })
}

/// `~/.meowo/mcp/<agent>-browser.json`，每次启动按当前 reporter 路径重写一遍。
fn config_file(agent: &str, reporter: &str) -> Option<String> {
    let dir = crate::setup::meowo_dir().join("mcp");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{agent}-browser.json"));
    let body = json!({ "mcpServers": { SERVER_NAME: server_entry(reporter) } });
    std::fs::write(&path, serde_json::to_vec_pretty(&body).ok()?).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// 没有启动参数可用、只读数据目录里 MCP 配置文件的 agent（McpAttach::DataDirJson，如 kimi 2.x）。
/// 开关开着就登记 `meowo` 这一项，关了就摘掉；文件里别的服务器原样保留。
/// 这份文件 Kimi Code 桌面版和外部终端里的 kimi 也会读——服务器在那些地方拿不到 Meowo 的
/// 会话令牌，工具列表是空的（见 meowo-reporter 的 mcp.rs），不会添乱。
/// 只在自动接线开着时调（dev 构建默认不碰全局配置，同 hooks）。
pub(crate) fn sync_mcp_files() {
    let settings = crate::settings::load_settings();
    let reporter = crate::setup::sibling_reporter();
    for plugin in meowo_agent::all() {
        let Some(meowo_agent::McpAttach::DataDirJson { rel }) = plugin.mcp_attach() else {
            continue;
        };
        // 默认账号 + Meowo 里建的每个账号：多账号整个换数据目录（kimi 的 KIMI_CODE_HOME），
        // 只登记默认目录的话，非默认账号启动的会话拿不到这组工具。
        let agent = plugin.id().as_str();
        let mut homes: Vec<std::path::PathBuf> = Vec::new();
        homes.extend(crate::profile::data_dir(agent, None));
        for profile in settings.profiles.get(agent).into_iter().flatten() {
            homes.extend(crate::profile::data_dir(agent, Some(&profile.id)));
        }
        homes.sort();
        homes.dedup();
        for home in homes.into_iter().filter(|dir| dir.is_dir()) {
            sync_mcp_file(&home.join(rel), settings.browser_agent, reporter.as_deref());
        }
    }
}

fn sync_mcp_file(path: &std::path::Path, enabled: bool, reporter: Option<&str>) {
    let path = path.to_path_buf();
    let mut root: Value = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            // 用户手写坏了的文件不动，免得把他的配置冲掉。
            Err(_) => return,
        },
        Err(_) if enabled => json!({}),
        Err(_) => return,
    };
    if !root.is_object() {
        return;
    }
    let servers = root.as_object_mut().and_then(|obj| {
        obj.entry("mcpServers").or_insert_with(|| json!({}));
        obj.get_mut("mcpServers").and_then(Value::as_object_mut)
    });
    let Some(servers) = servers else { return };
    let changed = match (enabled, reporter) {
        (true, Some(reporter)) => {
            let entry = server_entry(reporter);
            if servers.get(SERVER_NAME) == Some(&entry) {
                false
            } else {
                servers.insert(SERVER_NAME.into(), entry);
                true
            }
        }
        _ => servers.remove(SERVER_NAME).is_some(),
    };
    if !changed {
        return;
    }
    let Ok(body) = serde_json::to_vec_pretty(&root) else {
        return;
    };
    let tmp = path.with_extension("json.meowo-tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// 页面里的辅助对象。每次 eval 都带上，已有就跳过。
const HELPER: &str = r##"
if (!window.__meowoAgent) window.__meowoAgent = (() => {
  let next = 1;
  const INTERACTIVE = 'a[href],button,input,textarea,select,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=switch],[role=combobox],[role=textbox],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) return false;
    const style = getComputedStyle(el);
    return style.visibility !== 'hidden' && style.display !== 'none' && style.opacity !== '0';
  };
  const clean = (text, max) => {
    const s = (text || '').replace(/\s+/g, ' ').trim();
    return s.length > max ? s.slice(0, max) + '…' : s;
  };
  const refOf = (el) => {
    if (!el.dataset.meowoRef) el.dataset.meowoRef = 'e' + (next++);
    return el.dataset.meowoRef;
  };
  const find = (ref) => {
    const el = document.querySelector('[data-meowo-ref="' + ref + '"]');
    if (!el) throw new Error('找不到元素 ' + ref + '，页面可能变了，先重新 browser_snapshot');
    return el;
  };
  const roleOf = (el) => el.getAttribute('role') || (el.tagName === 'A' ? 'link' : el.tagName === 'INPUT' ? (el.type || 'text') : el.tagName.toLowerCase());
  // 下拉框的 innerText 是全部选项拼起来的，不当名字（选中项另外列出）。
  const nameOf = (el) => clean(el.getAttribute('aria-label') || (el.tagName === 'SELECT' ? '' : el.innerText) || el.getAttribute('title') || el.getAttribute('placeholder') || el.getAttribute('alt') || (el.tagName === 'SELECT' ? '' : el.value) || '', 80);
  const describe = (el) => {
    let line = '[' + refOf(el) + '] ' + roleOf(el);
    const name = nameOf(el);
    if (name) line += ' "' + name + '"';
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
      if (el.type === 'checkbox' || el.type === 'radio') line += el.checked ? ' (已选)' : ' (未选)';
      else if (el.type !== 'password' && el.value) line += ' 值="' + clean(el.value, 60) + '"';
      if (el.placeholder && name !== clean(el.placeholder, 80)) line += ' 提示="' + clean(el.placeholder, 40) + '"';
    }
    if (el.tagName === 'SELECT') line += ' 选中="' + clean(el.options[el.selectedIndex] && el.options[el.selectedIndex].text, 40) + '"';
    if (el.tagName === 'A' && el.getAttribute('href')) line += ' → ' + clean(el.getAttribute('href'), 80);
    if (el.disabled) line += ' (禁用)';
    return line;
  };
  const snapshot = () => {
    const lines = [];
    let size = 0;
    const LIMIT = 14000;
    const push = (line) => { if (size < LIMIT) { lines.push(line); size += line.length + 1; } };
    const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
    let skipUntil = null;
    let node;
    while ((node = walker.nextNode())) {
      if (skipUntil && skipUntil.contains(node)) continue;
      skipUntil = null;
      if (node.nodeType === 3) {
        const parent = node.parentElement;
        if (!parent || ['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE'].includes(parent.tagName)) continue;
        const text = clean(node.textContent, 200);
        if (text && visible(parent)) push(text);
        continue;
      }
      const el = node;
      if (['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG'].includes(el.tagName.toUpperCase())) { skipUntil = el; continue; }
      if (/^H[1-6]$/.test(el.tagName) && visible(el)) { push('#'.repeat(+el.tagName[1]) + ' ' + clean(el.innerText, 120)); skipUntil = el; continue; }
      if (el.matches(INTERACTIVE) && visible(el)) { push(describe(el)); skipUntil = el; continue; }
    }
    const head = '页面：' + document.title + '\n网址：' + location.href + '\n';
    return head + '\n' + lines.join('\n') + (size >= LIMIT ? '\n…（内容太长，后面省略了；可以先滚动或用 browser_evaluate 读取具体部分）' : '');
  };
  const center = (el) => {
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect();
    // 和 agent 鼠标落点同一个位置（宽元素取靠左 40px 处，不在一整行的正中间点）。
    return { clientX: Math.round(r.left + Math.min(r.width / 2, 40)), clientY: Math.round(r.top + r.height / 2), bubbles: true, cancelable: true, view: window };
  };
  // ── agent 鼠标：画在一个封闭 shadow root 里，网页样式碰不到它，它也不接收点击 ──
  const CURSOR_ID = '__meowo_agent_cursor';
  const CURSOR_MOVE_MS = 420;
  let cursorRoot = null;
  const cursor = () => {
    let host = document.getElementById(CURSOR_ID);
    if (host && cursorRoot) return { host, root: cursorRoot, fresh: false };
    if (host) host.remove();
    host = document.createElement('div');
    host.id = CURSOR_ID;
    host.setAttribute('style', 'all: initial; position: fixed; inset: 0 auto auto 0; width: 0; height: 0; z-index: 2147483647; pointer-events: none;');
    cursorRoot = host.attachShadow({ mode: 'closed' });
    cursorRoot.innerHTML = '<style>'
      + '.c{position:fixed;left:0;top:0;transform:translate(-100px,-100px);transition:transform ' + CURSOR_MOVE_MS + 'ms cubic-bezier(.22,.9,.3,1),opacity .25s;opacity:0;will-change:transform;}'
      + '.c.on{opacity:1}'
      + '.c svg{display:block;filter:drop-shadow(0 1px 2px rgba(0,0,0,.35))}'
      + '.tag{position:absolute;left:16px;top:18px;padding:2px 6px;border-radius:6px;background:#55d6ae;color:#0b1f18;font:600 11px/1.4 system-ui,sans-serif;white-space:nowrap}'
      + '.r{position:fixed;left:0;top:0;width:28px;height:28px;margin:-14px 0 0 -14px;border-radius:50%;border:2px solid #55d6ae;opacity:.9;animation:r .5s ease-out forwards}'
      + '@keyframes r{from{transform:var(--at) scale(.3);opacity:.9}to{transform:var(--at) scale(1.6);opacity:0}}'
      + '</style><div class="c"><svg width="22" height="24" viewBox="0 0 22 24"><path d="M2 2l7.5 19 2.6-7.4L19.5 11z" fill="#55d6ae" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg><span class="tag">Agent</span></div>';
    (document.body || document.documentElement).appendChild(host);
    return { host, root: cursorRoot, fresh: true };
  };
  const moveCursor = (x, y, animate) => {
    const { root } = cursor();
    const c = root.querySelector('.c');
    if (!animate) c.style.transition = 'none';
    c.style.transform = 'translate(' + x + 'px,' + y + 'px)';
    c.classList.add('on');
    if (!animate) { c.getBoundingClientRect(); c.style.transition = ''; }
  };
  // 新页面上先把鼠标放回上次的位置（不带动画），再从那里移到目标。
  const place = (x, y) => { if (cursor().fresh) moveCursor(x, y, false); };
  const point = (ref) => {
    const el = find(ref);
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect();
    const x = Math.round(r.left + Math.min(r.width / 2, 40));
    const y = Math.round(r.top + r.height / 2);
    moveCursor(x, y, true);
    return { x, y };
  };
  const ripple = (x, y) => {
    const { root } = cursor();
    const dot = document.createElement('div');
    dot.className = 'r';
    dot.style.setProperty('--at', 'translate(' + x + 'px,' + y + 'px)');
    root.appendChild(dot);
    setTimeout(() => dot.remove(), 600);
  };
  const hideCursor = () => {
    const host = document.getElementById(CURSOR_ID);
    if (host && cursorRoot) cursorRoot.querySelector('.c').classList.remove('on');
    return 'hidden';
  };
  const click = (ref) => {
    const el = find(ref);
    const at = center(el);
    ripple(at.clientX, at.clientY);
    for (const type of ['pointerover', 'pointerdown', 'mousedown', 'pointerup', 'mouseup']) {
      el.dispatchEvent(new (type.startsWith('pointer') ? PointerEvent : MouseEvent)(type, at));
    }
    if (typeof el.focus === 'function') el.focus();
    el.click();
    return '已点击 ' + describe(el);
  };
  const setValue = (el, value) => {
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
    setter.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const submitFrom = (el) => {
    if (el.form) { el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit(); return; }
    for (const type of ['keydown', 'keypress', 'keyup']) el.dispatchEvent(new KeyboardEvent(type, { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true }));
  };
  const type = (ref, text, submit) => {
    const el = find(ref);
    center(el);
    el.focus();
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') setValue(el, text);
    else if (el.isContentEditable) {
      el.textContent = '';
      document.execCommand('insertText', false, text);
      el.dispatchEvent(new Event('input', { bubbles: true }));
    } else throw new Error(ref + ' 不是输入框');
    if (submit) submitFrom(el);
    return '已输入到 ' + ref + (submit ? '，并提交' : '');
  };
  const select = (ref, value) => {
    const el = find(ref);
    if (el.tagName !== 'SELECT') throw new Error(ref + ' 不是下拉框');
    const option = Array.from(el.options).find((o) => o.value === value || o.text.trim() === value);
    if (!option) throw new Error('下拉框里没有「' + value + '」');
    el.value = option.value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return '已选择「' + option.text.trim() + '」';
  };
  const press = (key) => {
    const el = document.activeElement || document.body;
    const init = { key, code: key.length === 1 ? 'Key' + key.toUpperCase() : key, bubbles: true, cancelable: true };
    for (const t of ['keydown', 'keypress', 'keyup']) el.dispatchEvent(new KeyboardEvent(t, init));
    if (key === 'Enter' && el.form) el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit();
    return '已按下 ' + key;
  };
  const scrollTo = (ref) => { center(find(ref)); return '已滚动到 ' + ref; };
  return { snapshot, click, type, select, press, scrollTo, place, point, hideCursor };
})()
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_tells_the_agent_when_the_page_waits_for_the_user() {
        use browser_pane::BrowserPermission;
        let ask = BrowserPermission::new("downloads", "https://httpbin.org/html");
        let note = permission_note(Some(&ask), None);
        assert!(note.contains("httpbin.org 想要下载多个文件"), "{note}");
        assert!(note.contains("请告诉用户"), "{note}");
        // 在等答复优先于「已阻止」。
        assert_eq!(permission_note(Some(&ask), Some(&ask)), note);
        let blocked = permission_note(None, Some(&ask));
        assert!(blocked.contains("已阻止") && blocked.contains("刷新"), "{blocked}");
        assert!(note.ends_with("\n\n") && blocked.ends_with("\n\n"));
        assert_eq!(permission_note(None, None), "");
    }

    #[test]
    fn decode_unwraps_both_layers() {
        let page =
            serde_json::to_string(&json!({ "ok": true, "value": "hi" }).to_string()).unwrap();
        assert_eq!(decode(&page).unwrap(), json!("hi"));
        let failed =
            serde_json::to_string(&json!({ "ok": false, "error": "找不到元素 e9" }).to_string())
                .unwrap();
        assert_eq!(decode(&failed).unwrap_err(), "找不到元素 e9");
        assert!(decode("not json").is_err());
    }

    #[test]
    fn element_refs_must_be_plain() {
        assert_eq!(ref_arg(&json!({ "ref": "e12" })).unwrap(), "e12");
        assert!(ref_arg(&json!({ "ref": "e1\"]; alert(1); //" })).is_err());
        assert!(ref_arg(&json!({})).is_err());
    }

    /// kimi 的 mcp.json：开着就加 meowo 一项、关了就摘掉，别的服务器原样保留；
    /// 用户手写坏了的文件不碰。
    #[test]
    fn kimi_mcp_entry_is_merged_and_removed_without_touching_others() {
        let dir = std::env::temp_dir().join(format!("meowo-kimi-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        std::fs::write(
            &path,
            r#"{"mcpServers":{"other":{"command":"x"}},"keep":1}"#,
        )
        .unwrap();
        let read = || serde_json::from_slice::<Value>(&std::fs::read(&path).unwrap()).unwrap();
        sync_mcp_file(&dir.join("mcp.json"), true, Some("C:/m/meowo-reporter.exe"));
        let root = read();
        assert_eq!(root["mcpServers"]["meowo"]["args"], json!(["mcp"]));
        assert_eq!(root["mcpServers"]["other"]["command"], "x");
        assert_eq!(root["keep"], 1);
        sync_mcp_file(
            &dir.join("mcp.json"),
            false,
            Some("C:/m/meowo-reporter.exe"),
        );
        let root = read();
        assert!(root["mcpServers"].get("meowo").is_none());
        assert_eq!(root["mcpServers"]["other"]["command"], "x");
        std::fs::write(&path, "{broken").unwrap();
        sync_mcp_file(&dir.join("mcp.json"), true, Some("C:/m/meowo-reporter.exe"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn evaluate_is_not_auto_allowed() {
        assert!(!AUTO_ALLOWED.contains(&"browser_evaluate"));
        assert!(AUTO_ALLOWED.contains(&"browser_click"));
    }

    #[test]
    fn strings_are_escaped_for_the_page() {
        assert_eq!(js_str("a\"b\n"), "\"a\\\"b\\n\"");
    }
}
