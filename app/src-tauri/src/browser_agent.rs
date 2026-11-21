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

use serde_json::{Value, json};
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
const TAKEOVER_ERROR: &str =
    "browser/user_takeover：用户已接管面板浏览器，本次自动化已停止。等用户点「交还」后再试。";

fn control_is_current(
    agent_paused: bool,
    current_generation: u64,
    expected_generation: u64,
) -> bool {
    !agent_paused && current_generation == expected_generation
}

fn ensure_agent_control(panes: &BrowserPanes, id: u32, generation: u64) -> Result<(), String> {
    let map = panes.panes.lock().map_err(|e| e.to_string())?;
    let info = map.get(&id).ok_or("browser_pane/not_found")?;
    if !control_is_current(info.agent_paused, info.agent_generation, generation) {
        Err(TAKEOVER_ERROR.into())
    } else {
        Ok(())
    }
}

/// 每会话 agent 正在用的那个浏览器页签（会话 id → 页签编号）。
fn agent_panes() -> &'static Mutex<std::collections::HashMap<i64, u32>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<i64, u32>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// 首次并发 browser_open 的「选中现有页签 → 没有则创建 → remember」必须整体互斥，
/// 否则两个调用会各建一个页签，再各拿不同的页签锁并行操作。
fn session_selection_locks() -> &'static Mutex<std::collections::HashMap<i64, Arc<Mutex<()>>>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<i64, Arc<Mutex<()>>>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn session_selection_lock(session_id: i64) -> Arc<Mutex<()>> {
    session_selection_locks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(session_id)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// broker 收到 `BrokerRequest::Browser` 后调这里。返回给 agent 的文字；出错返回原因。
/// 返回给 agent 的内容：`{"text": …}`，截图另带 `{"image": base64, "mime": …}`。
pub(crate) fn handle(session_id: i64, tool: &str, args: &Value) -> Result<Value, String> {
    // 开关关掉后，已经在跑的会话手上还挂着这组工具（启动参数/配置文件是开的时候给的），
    // 每次调用都要再看一眼，不然「关掉」管不住正在跑的 agent。
    if !crate::settings::load_settings().browser_agent {
        return Err(
            "用户在 Meowo 设置里关掉了「允许 agent 操作面板浏览器」，这组工具现在不能用。".into(),
        );
    }
    let app = APP.get().ok_or("Meowo 还没启动完")?;
    let panes = app.state::<BrowserPanes>();
    if tool == "browser_close" {
        return close_agent_pane(app, &panes, session_id).map(|text| json!({ "text": text }));
    }
    let id = pane_for(app, &panes, session_id, tool == "browser_open")?;
    // 同一页签一次只跑一个动作：两个工具并行时，旧动作不能在用户接管后越过新动作继续写页面。
    let (operation, generation) = panes
        .panes
        .lock()
        .map_err(|e| e.to_string())?
        .get(&id)
        .map(|info| (info.operation.clone(), info.agent_generation))
        .ok_or("browser_pane/not_found")?;
    let _operation = operation.lock().map_err(|e| e.to_string())?;
    ensure_agent_control(&panes, id, generation)?;
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
        run(app, id, tool, args, generation).map(|text| json!({ "text": text }))
    };
    // 等待、截图或脚本执行期间用户可能刚接管：已开始的 JS 没法撤回，但结果不能冒充本轮仍归 Agent。
    let result = match ensure_agent_control(&panes, id, generation) {
        Ok(()) => result,
        Err(error) => Err(error),
    };
    schedule_idle(app, id);
    result
}

/// E2E 专用入口：真实走 broker 同一份 handle，但放进 blocking 线程，测试才能在等待中点接管。
/// 命令注册也用 cfg(feature="e2e") 门控，生产二进制没有这条能力。
#[cfg(feature = "e2e")]
#[tauri::command]
pub(crate) async fn browser_agent_e2e(
    session_id: i64,
    tool: String,
    args: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || handle(session_id, &tool, &args))
        .await
        .map_err(|e| e.to_string())?
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
    // 截图也存一份到 Meowo 图片目录（按内容哈希命名，和对话页从回执里存的是同一个文件），
    // 把路径告诉 agent：改界面前后各截一张，就能直接交给 compare_images。
    let saved = meowo_agent::images::store_bytes(meowo_agent::images::ImageKind::Tool, &bytes)
        .map(|path| format!("\n截图已存为本地文件：{}（可直接交给 compare_images 对比改前改后）", path.display()))
        .unwrap_or_default();
    Ok(json!({
        "text": format!("面板浏览器截图（{} KB）
    {title}{saved}", bytes.len() / 1024),
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
    let selection = session_selection_lock(session_id);
    let _selection = selection.lock().map_err(|e| e.to_string())?;
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

/// agent 关掉它正在用的页签。只关这一个：用户自己另开的页签不动；没在用的就不新建、
/// 也不去挑用户的页签来关。用户接管着的页签不让关（和其他工具一样被拒）。
fn close_agent_pane(
    app: &tauri::AppHandle,
    panes: &BrowserPanes,
    session_id: i64,
) -> Result<String, String> {
    let selection = session_selection_lock(session_id);
    let _selection = selection.lock().map_err(|e| e.to_string())?;
    let current = agent_panes()
        .lock()
        .map_err(|e| e.to_string())?
        .get(&session_id)
        .copied();
    let found = current.and_then(|id| {
        let map = panes.panes.lock().ok()?;
        let info = map.get(&id).filter(|info| info.owner == session_id)?;
        Some((id, info.operation.clone(), info.agent_paused))
    });
    let Some((id, operation, paused)) = found else {
        if let Ok(mut map) = agent_panes().lock() {
            map.remove(&session_id);
        }
        return Ok("没有在用的页签，不用关。".into());
    };
    if paused {
        return Err(TAKEOVER_ERROR.into());
    }
    // 等手上的动作做完再关，别在点击、等待的半路把页签拿掉。
    let _operation = operation.lock().map_err(|e| e.to_string())?;
    if let Ok(mut map) = panes.panes.lock() {
        map.remove(&id);
    }
    if let Ok(mut map) = agent_panes().lock() {
        map.remove(&session_id);
    }
    if let Ok(mut map) = last_used().lock() {
        map.remove(&id);
    }
    if let Ok(mut map) = cursor_positions().lock() {
        map.remove(&id);
    }
    crate::browser_permission::forget(app, id);
    if let Ok(webview) = browser_pane::webview(app, id) {
        webview.close().map_err(|e| e.to_string())?;
    }
    {
        use tauri::Emitter;
        let _ = app.emit_to(
            tauri::EventTarget::webview("chat"),
            "browser-pane-closed",
            json!({ "id": id, "owner": session_id }),
        );
    }
    Ok("已关闭页签。之后再用 browser_open 会新开一个。".into())
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
    browser_pane::set_agent_input_locked(app, id, true);
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
        let panes = app.state::<BrowserPanes>();
        let operation = panes
            .panes
            .lock()
            .ok()
            .and_then(|map| map.get(&id).map(|info| info.operation.clone()));
        let Some(operation) = operation else { return };
        // 长操作可能超过 6 秒；空闲清理与工具调用同锁，绝不能在做了一半时解开人工输入。
        let Ok(_operation) = operation.lock() else {
            return;
        };
        let idle = last_used()
            .lock()
            .ok()
            .and_then(|map| map.get(&id).copied())
            .is_none_or(|at| at.elapsed() >= ACTIVE_LINGER);
        if idle {
            browser_pane::update(&app, &panes.panes, id, |info| info.agent_active = false);
            browser_pane::set_agent_input_locked(&app, id, false);
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

fn run(
    app: &tauri::AppHandle,
    id: u32,
    tool: &str,
    args: &Value,
    generation: u64,
) -> Result<String, String> {
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
            wait_loaded(app, id, generation)?;
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
            wait_loaded(app, id, generation)?;
            snapshot(app, id)
        }
        "browser_reload" => {
            browser_pane::webview(app, id)?
                .reload()
                .map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_millis(400));
            wait_loaded(app, id, generation)?;
            snapshot(app, id)
        }
        "browser_snapshot" => snapshot(app, id),
        "browser_emulate" => {
            // 参数先校验完再动页签：不对就原样报错，模拟状态不变。
            let next = crate::browser_emulation::parse(args)?;
            let summary = match &next {
                Some(_) => format!(
                    "已设置：{}",
                    crate::browser_emulation::describe(next.as_ref())
                ),
                None => "已还原为面板实际大小和默认 UA".to_string(),
            };
            let panes = app.state::<BrowserPanes>();
            ensure_agent_control(&panes, id, generation)?;
            let check = || ensure_agent_control(&panes, id, generation);
            let reload = browser_pane::set_emulation(app, &panes, id, next, &check)?;
            if reload {
                // 刷新会丢掉用户刚接管后在页面上做的事：刷新前再确认一次控制权还在。
                check()?;
                browser_pane::webview(app, id)?
                    .reload()
                    .map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(400));
            } else {
                // 只改尺寸：给响应式布局一帧重排。
                std::thread::sleep(Duration::from_millis(300));
            }
            wait_loaded(app, id, generation)?;
            let reloaded = if reload {
                "，已刷新页面让服务端按新请求重新渲染"
            } else {
                ""
            };
            Ok(format!(
                "{summary}{reloaded}。\n\n{}",
                snapshot(app, id)?
            ))
        }
        "browser_click" => {
            let r = ref_arg(args)?;
            point(app, id, &r)?;
            act(app, id, &format!("H.click({})", js_str(&r)), generation)
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
                generation,
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
                generation,
            )
        }
        "browser_press" => {
            let key = str_arg("key").ok_or("缺少 key")?;
            act(app, id, &format!("H.press({})", js_str(&key)), generation)
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
            act(app, id, &expr, generation)
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
                None => loop {
                    ensure_agent_control(&app.state::<BrowserPanes>(), id, generation)?;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return snapshot(app, id);
                    }
                    std::thread::sleep(remaining.min(Duration::from_millis(100)));
                },
                Some(text) => loop {
                    ensure_agent_control(&app.state::<BrowserPanes>(), id, generation)?;
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
                ensure_agent_control(&app.state::<BrowserPanes>(), id, generation)?;
                let got = eval(
                    app,
                    id,
                    &format!(
                        "(() => {{ const box = window.__meowoEval || {{}}; const r = box['{slot}']; if (r) delete box['{slot}']; return r || null; }})()"
                    ),
                )?;
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
fn act(app: &tauri::AppHandle, id: u32, expr: &str, generation: u64) -> Result<String, String> {
    // point() 的鼠标动画和排队等待期间可能已经接管；真正写页面前再查一次。
    ensure_agent_control(&app.state::<BrowserPanes>(), id, generation)?;
    let value = eval(app, id, expr)?;
    let note = match value {
        Value::String(s) => s,
        other => other.to_string(),
    };
    std::thread::sleep(Duration::from_millis(500));
    wait_loaded(app, id, generation)?;
    Ok(format!("{note}\n\n{}", snapshot(app, id)?))
}

fn snapshot(app: &tauri::AppHandle, id: u32) -> Result<String, String> {
    let page = match eval(app, id, "H.snapshot()")? {
        Value::String(s) => s,
        other => other.to_string(),
    };
    let (waiting, blocked, emulation) = app
        .state::<BrowserPanes>()
        .panes
        .lock()
        .ok()
        .and_then(|map| {
            map.get(&id).map(|info| {
                (
                    info.permission.clone(),
                    info.blocked.clone(),
                    info.emulation.clone(),
                )
            })
        })
        .unwrap_or((None, None, None));
    // 视口写在页面内容前面：Agent 先知道看到的是哪一版布局，再决定要不要 browser_emulate。
    // 乘上 visualViewport.scale：手机模拟下内容超宽时页面会像真手机一样缩小装下，innerWidth
    // 量的是缩小后的可见区域（E2E 实测 390 宽的手机报成 1240），媒体查询看到的仍是 390。
    let inner = eval(
        app,
        id,
        "(() => { const s = (window.visualViewport && visualViewport.scale) || 1; return [Math.round(innerWidth * s), Math.round(innerHeight * s)]; })()",
    )
        .ok()
        .and_then(|v| {
            let pair = v.as_array()?;
            Some((
                pair.first()?.as_u64()? as u32,
                pair.get(1)?.as_u64()? as u32,
            ))
        })
        .unwrap_or((0, 0));
    Ok(format!(
        "{}{}{page}",
        permission_note(waiting.as_ref(), blocked.as_ref()),
        crate::browser_emulation::viewport_note(emulation.as_ref(), inner)
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
/// 长连接，照样能读能点。等待期间持续响应用户接管，不能卡满 20 秒才停。
fn wait_loaded(app: &tauri::AppHandle, id: u32, generation: u64) -> Result<(), String> {
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while Instant::now() < deadline {
        let panes = app.state::<BrowserPanes>();
        ensure_agent_control(&panes, id, generation)?;
        let loading = panes
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
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
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
    if let Ok(mut map) = session_selection_locks().lock() {
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
    if let Ok(mut map) = session_selection_locks().lock() {
        if let Some(lock) = map.remove(&from) {
            map.insert(to, lock);
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

/// 发图一类的工具（meowo-reporter mcp.rs 的发图、改前改后对照）：与浏览器开关无关，始终挂上并免逐次审批——
/// 它们只是把图片显示给用户，不改用户的任何东西。
const IMAGE_TOOLS: &[&str] = meowo_agent::images::IMAGE_TOOLS;

/// 浏览器开关关掉时让服务器只列发图工具（meowo-reporter mcp.rs 的 NO_BROWSER_FLAG）。
const NO_BROWSER_FLAG: &str = "--no-browser";

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
    "browser_close",
    "browser_screenshot",
    "browser_emulate",
];

/// 一律要审批的浏览器工具（能显式写「问」的 agent 都写上，不随它的默认放行走）。
const ALWAYS_ASK: &str = "browser_evaluate";

/// 服务器启动参数：浏览器开关关掉时只给发图工具。
fn server_args(browser: bool) -> Vec<&'static str> {
    if browser {
        vec!["mcp"]
    } else {
        vec!["mcp", NO_BROWSER_FLAG]
    }
}

/// 免审批的工具：发图始终在；浏览器工具只在开关开着时。
fn auto_allowed(browser: bool) -> Vec<&'static str> {
    let mut tools = IMAGE_TOOLS.to_vec();
    if browser {
        tools.extend_from_slice(AUTO_ALLOWED);
    }
    tools
}

#[derive(Clone, Copy)]
pub(crate) struct McpSessionEnv<'a> {
    pub endpoint: &'a str,
    pub token: &'a str,
    pub protocol: u16,
}

/// 托管会话启动时追加的参数（pty.rs 的 start_spawned，外部终端启动不走这里）。各 agent 怎么接
/// 由插件的 `mcp_attach()` 声明（meowo_agent::McpAttach）：
/// - ConfigFileArg：启动参数指向配置文件，同时放行发图工具和（开关开着时）AUTO_ALLOWED 里的
///   浏览器工具——和 Kimi Code 一样 agent 直接操作，用户随时能在页签上「接管」。
/// - TomlOverride：配置覆盖参数，值用 TOML 单引号字符串；发图工具单独设免审批。
/// - DataDirJson：没有启动参数可用，走数据目录里的配置文件（见 sync_mcp_files），这里不追加。
/// - InlineConfigEnv：走环境变量（见 launch_env），这里不追加。
///
/// 浏览器开关只决定给不给浏览器工具；发图工具始终挂上（它和面板浏览器无关）。
pub(crate) fn launch_args(provider: &str, session: Option<McpSessionEnv<'_>>) -> Vec<String> {
    let browser = crate::settings::load_settings().browser_agent;
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
            let Some(path) = config_file(plugin.id().as_str(), &reporter, browser) else {
                return Vec::new();
            };
            let mut args = vec![file_flag.to_string(), path];
            if let Some(flag) = allow_flag {
                args.push(flag.to_string());
                args.extend(
                    auto_allowed(browser)
                        .iter()
                        .map(|tool| format!("{allow_prefix}{SERVER_NAME}__{tool}")),
                );
            }
            args
        }
        Some(meowo_agent::McpAttach::TomlOverride { flag }) => {
            toml_mcp_args(flag, &reporter, session, browser)
        }
        _ => Vec::new(),
    }
}

/// 托管会话启动时追加的环境变量（McpAttach::InlineConfigEnv）。`env` 是已经要设的环境（中转
/// 可能也用同一个变量放配置）：同名变量把 Meowo 的 `mcp` / `permission` 合并进去，不覆盖中转。
pub(crate) fn launch_env(provider: &str, env: &[(String, String)]) -> Vec<(String, String)> {
    let Some(plugin) = meowo_agent::by_id(provider) else {
        return Vec::new();
    };
    let Some(meowo_agent::McpAttach::InlineConfigEnv { env: key }) = plugin.mcp_attach() else {
        return Vec::new();
    };
    let Some(reporter) = crate::setup::sibling_reporter() else {
        return Vec::new();
    };
    let browser = crate::settings::load_settings().browser_agent;
    let existing = env
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var(key).ok());
    inline_config(existing.as_deref(), &reporter, browser)
        .map(|value| vec![(key.to_string(), value)])
        .unwrap_or_default()
}

/// 合并出 opencode 形态的内联配置。已有配置不是 JSON 对象（用户手写坏了）时不碰，返回 None。
fn inline_config(existing: Option<&str>, reporter: &str, browser: bool) -> Option<String> {
    let mut root: Value = match existing.filter(|s| !s.trim().is_empty()) {
        Some(text) => serde_json::from_str(text).ok()?,
        None => json!({}),
    };
    let obj = root.as_object_mut()?;
    let mut command = vec![json!(reporter)];
    command.extend(server_args(browser).into_iter().map(Value::from));
    let mcp = obj.entry("mcp").or_insert_with(|| json!({}));
    mcp.as_object_mut()?.insert(
        SERVER_NAME.into(),
        json!({ "type": "local", "command": command, "enabled": true }),
    );
    let permission = obj.entry("permission").or_insert_with(|| json!({}));
    // permission 也可以是一个字符串（全局 allow/ask）：那是用户的意思，不改写成对象。
    if let Some(permission) = permission.as_object_mut() {
        for tool in IMAGE_TOOLS {
            permission.insert(format!("{SERVER_NAME}_{tool}"), json!("allow"));
        }
        permission.insert(format!("{SERVER_NAME}_{ALWAYS_ASK}"), json!("ask"));
    }
    serde_json::to_string(&root).ok()
}

/// Codex 0.167.2 同 daemon 双 thread 实测：隐式继承拿的是 daemon 的旧环境，显式 MCP env 才隔离。
/// 本次 scoped token 只进入直接 spawn 的 argv，不写配置文件、不经 shell，也不输出参数日志。
fn toml_mcp_args(
    flag: &str,
    reporter: &str,
    session: Option<McpSessionEnv<'_>>,
    browser: bool,
) -> Vec<String> {
    let Some(session) = session else {
        return Vec::new();
    };
    let mut env = toml_edit::InlineTable::new();
    env.insert("MEOWO_PTY_ENDPOINT", session.endpoint.into());
    env.insert("MEOWO_PTY_TOKEN", session.token.into());
    env.insert("MEOWO_PTY_PROTOCOL", session.protocol.to_string().into());
    let args = server_args(browser)
        .into_iter()
        .map(|a| format!("'{a}'"))
        .collect::<Vec<_>>()
        .join(",");
    let mut out = vec![
        flag.to_owned(),
        format!(
            "mcp_servers.{SERVER_NAME}.command={}",
            toml_edit::Value::from(reporter)
        ),
        flag.to_owned(),
        format!("mcp_servers.{SERVER_NAME}.args=[{args}]"),
        flag.to_owned(),
        format!("mcp_servers.{SERVER_NAME}.env={env}"),
    ];
    // 发图、对照免审批。
    for tool in IMAGE_TOOLS {
        out.push(flag.to_owned());
        out.push(format!("mcp_servers.{SERVER_NAME}.tools.{tool}.approval_mode='approve'"));
    }
    if browser {
        // codex 默认 auto 下没有只读注解的 browser_evaluate 本来就要审批；显式写上，防默认值变化。
        out.push(flag.to_owned());
        out.push(format!(
            "mcp_servers.{SERVER_NAME}.tools.{ALWAYS_ASK}.approval_mode='prompt'"
        ));
    }
    out
}

/// 数据目录配置文件里的服务器项。`expand_env` 见 McpAttach::DataDirJson。
fn server_entry(reporter: &str, browser: bool, expand_env: bool) -> Value {
    let mut entry = json!({ "type": "stdio", "command": reporter, "args": server_args(browser) });
    if expand_env {
        entry["env"] = json!({
            "MEOWO_PTY_ENDPOINT": "${MEOWO_PTY_ENDPOINT:-}",
            "MEOWO_PTY_TOKEN": "${MEOWO_PTY_TOKEN:-}",
            "MEOWO_PTY_PROTOCOL": "${MEOWO_PTY_PROTOCOL:-}",
        });
    }
    entry
}

/// `~/.meowo/mcp/<agent>-browser.json`，每次启动按当前 reporter 路径重写一遍。
fn config_file(agent: &str, reporter: &str, browser: bool) -> Option<String> {
    let dir = crate::setup::meowo_dir().join("mcp");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{agent}-browser.json"));
    let body = json!({ "mcpServers": { SERVER_NAME: server_entry(reporter, browser, false) } });
    // 原子写：每次启动都重写，而 agent CLI 启动时按 --mcp-config 读它；直接 write 先截断，
    // 恰在此刻拉起的会话会读到空/半截 JSON，整组浏览器工具缺席。
    meowo_agent::fsutil::write_atomic(&path, &serde_json::to_string_pretty(&body).ok()?).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// 没有启动参数可用、只读数据目录里 MCP 配置文件的 agent（McpAttach::DataDirJson，如 kimi 2.x、gemini）。
/// 有 reporter 就登记 `meowo` 这一项（浏览器开关只决定参数里带不带 `--no-browser`），并按插件
/// 声明写发图工具的免审批规则；文件里别的服务器、别的规则原样保留。
/// 这份文件 agent 的桌面版和外部终端里的 CLI 也会读——服务器在那些地方拿不到 Meowo 的
/// 会话令牌，工具列表是空的（见 meowo-reporter 的 mcp.rs），不会添乱。
/// 只在自动接线开着时调（dev 构建默认不碰全局配置，同 hooks）。
use meowo_agent::mcp_config::{sync_mcp_file, sync_permission_rule, sync_policy_file, McpFileSync};

pub(crate) fn sync_mcp_files() {
    let settings = crate::settings::load_settings();
    let reporter = crate::setup::sibling_reporter();
    for plugin in meowo_agent::all() {
        let Some(meowo_agent::McpAttach::DataDirJson {
            rel,
            expand_env,
            approval,
        }) = plugin.mcp_attach()
        else {
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
        let entry = reporter
            .as_deref()
            .map(|reporter| server_entry(reporter, settings.browser_agent, expand_env));
        let mut unreadable = None;
        for home in homes.into_iter().filter(|dir| dir.is_dir()) {
            let file = home.join(rel);
            if sync_mcp_file(&file, entry.clone()) == McpFileSync::Unparsable {
                // 用户的配置文件 Meowo 读不懂（gemini 的 settings.json 允许注释，serde_json 不认）：
                // 不动它，记一行运行日志，并在 Agent 描述符里给前端一个标记（设置页提示
                // 「没能接上发图工具」）。
                eprintln!(
                    "[images] {agent} 的配置文件解析失败，没有登记 Meowo 发图工具：{}",
                    file.display()
                );
                unreadable.get_or_insert_with(|| file.to_string_lossy().into_owned());
                continue;
            }
            match approval {
                Some(meowo_agent::McpApproval::PermissionRules { rel, prefix }) => {
                    sync_permission_rule(&home.join(rel), prefix, entry.is_some());
                }
                Some(meowo_agent::McpApproval::PolicyFile { rel }) => {
                    sync_policy_file(&home.join(rel), entry.is_some());
                }
                None => {}
            }
        }
        meowo_agent::mcp_config::set_unreadable(agent, unreadable);
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
    fn toml_mcp_binding_is_explicit_and_different_for_each_session() {
        let binding = |token| McpSessionEnv {
            endpoint: "127.0.0.1:1234",
            token,
            protocol: 2,
        };
        let first = toml_mcp_args(
            "-c",
            "C:/O'Neil/reporter.exe",
            Some(binding("first-'quoted")),
            true,
        );
        let second = toml_mcp_args("-c", "C:/O'Neil/reporter.exe", Some(binding("second")), true);
        let config = first
            .chunks_exact(2)
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let doc: toml_edit::DocumentMut = config.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"][SERVER_NAME]["command"].as_str(),
            Some("C:/O'Neil/reporter.exe")
        );
        assert_eq!(
            doc["mcp_servers"][SERVER_NAME]["env"]["MEOWO_PTY_TOKEN"].as_str(),
            Some("first-'quoted")
        );
        assert_eq!(
            doc["mcp_servers"][SERVER_NAME]["env"]["MEOWO_PTY_PROTOCOL"].as_str(),
            Some("2")
        );
        // 发图免审批、browser_evaluate 显式要审批（codex 0.169 逐工具 approval_mode）。
        let tools = &doc["mcp_servers"][SERVER_NAME]["tools"];
        assert_eq!(tools["send_image"]["approval_mode"].as_str(), Some("approve"));
        assert_eq!(tools["browser_evaluate"]["approval_mode"].as_str(), Some("prompt"));
        assert_eq!(
            doc["mcp_servers"][SERVER_NAME]["args"].as_array().unwrap().len(),
            1
        );
        let env_of = |args: &[String]| {
            args.iter()
                .find(|a| a.starts_with("mcp_servers.meowo.env="))
                .cloned()
        };
        assert_ne!(env_of(&first), env_of(&second));
        assert!(toml_mcp_args("-c", "reporter.exe", None, true).is_empty());
        // 浏览器开关关掉：服务器带 --no-browser，只剩发图的免审批。
        let off = toml_mcp_args("-c", "r.exe", Some(binding("t")), false);
        let off_doc: toml_edit::DocumentMut = off
            .chunks_exact(2)
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .parse()
            .unwrap();
        let args: Vec<&str> = off_doc["mcp_servers"][SERVER_NAME]["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(args, vec!["mcp", "--no-browser"]);
        assert!(off_doc["mcp_servers"][SERVER_NAME]["tools"].get("browser_evaluate").is_none());
    }

    /// claude 的免审批清单：发图始终在；浏览器开关关掉时只剩它。
    #[test]
    fn send_image_is_always_auto_allowed() {
        assert_eq!(auto_allowed(false), vec!["send_image", "compare_images"]);
        let on = auto_allowed(true);
        assert!(on.contains(&"send_image") && on.contains(&"browser_click"));
        assert!(!on.contains(&"browser_evaluate"));
    }

    /// opencode 的内联配置：与中转已有的 OPENCODE_CONFIG_CONTENT 合并，不冲掉它的 provider；
    /// send_image 放行、browser_evaluate 要问；用户写坏的配置不碰。
    #[test]
    fn opencode_inline_config_merges_with_relay() {
        let relay = r#"{"provider":{"relay":{"npm":"x"}},"model":"relay/m","permission":{"*":"ask"}}"#;
        let merged: Value =
            serde_json::from_str(&inline_config(Some(relay), "C:/m/meowo-reporter.exe", true).unwrap())
                .unwrap();
        assert_eq!(merged["model"], "relay/m");
        assert_eq!(merged["provider"]["relay"]["npm"], "x");
        assert_eq!(
            merged["mcp"]["meowo"],
            json!({ "type": "local", "command": ["C:/m/meowo-reporter.exe", "mcp"], "enabled": true })
        );
        assert_eq!(merged["permission"]["*"], "ask");
        assert_eq!(merged["permission"]["meowo_send_image"], "allow");
        assert_eq!(merged["permission"]["meowo_browser_evaluate"], "ask");
        // 后写的规则胜出：meowo_send_image 必须排在用户的 "*" 之后。
        let keys: Vec<&String> = merged["permission"].as_object().unwrap().keys().collect();
        assert_eq!(keys.first().map(|k| k.as_str()), Some("*"));
        let fresh: Value =
            serde_json::from_str(&inline_config(None, "r.exe", false).unwrap()).unwrap();
        assert_eq!(fresh["mcp"]["meowo"]["command"], json!(["r.exe", "mcp", "--no-browser"]));
        assert!(inline_config(Some("{broken"), "r.exe", true).is_none());
        // permission 是全局字符串时保持原样。
        let global: Value =
            serde_json::from_str(&inline_config(Some(r#"{"permission":"ask"}"#), "r.exe", true).unwrap())
                .unwrap();
        assert_eq!(global["permission"], "ask");
    }

    /// gemini 的 settings.json 服务器项显式引用 MEOWO_PTY_*（gemini 会删掉继承环境里名字含
    /// TOKEN 的变量）；令牌本身不落盘。
    #[test]
    fn gemini_entry_expands_env_instead_of_writing_token() {
        let entry = server_entry("C:/m/meowo-reporter.exe", true, true);
        assert_eq!(entry["env"]["MEOWO_PTY_TOKEN"], "${MEOWO_PTY_TOKEN:-}");
        assert_eq!(entry["args"], json!(["mcp"]));
        assert!(server_entry("r", true, false).get("env").is_none());
        let body = meowo_agent::mcp_config::policy_body();
        let doc: toml_edit::DocumentMut = body.parse().unwrap();
        let rule = &doc["rule"].as_array_of_tables().unwrap().get(0).unwrap();
        assert_eq!(rule["mcpName"].as_str(), Some("meowo"));
        assert_eq!(rule["toolName"].as_str(), Some("send_image"));
        assert_eq!(rule["decision"].as_str(), Some("allow"));
        // 只放行发图，没有服务器级 trust 或通配。
        assert!(!body.contains("browser") && !body.contains('*'));
    }

    /// 五家都有 MCP 接法，且宿主不按名字分支：每家插件声明的 McpAttach 都能落到上面某条路径。
    #[test]
    fn every_agent_declares_an_mcp_attach() {
        for plugin in meowo_agent::all() {
            assert!(plugin.mcp_attach().is_some(), "{} 没有声明 mcp_attach", plugin.id().as_str());
        }
    }

    /// kimi config.toml：加放行 send_image、compare_images 的规则，用户与 Lead 的规则原样保留；
    /// 重复同步不重复加；关掉时只摘自己的那条；文件不存在不新建。
    #[test]
    fn kimi_permission_rule_is_added_once_and_removed_cleanly() {
        let dir = std::env::temp_dir().join(format!("meowo-kimi-rule-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        sync_permission_rule(&path, "mcp__", true);
        assert!(!path.exists(), "没有 config.toml 时不凭空新建");
        let original = "default_model = 'keep'\n\n[[permission.rules]]\ndecision = 'deny'\npattern = 'Bash(rm *)'\n";
        std::fs::write(&path, original).unwrap();
        sync_permission_rule(&path, "mcp__", true);
        sync_permission_rule(&path, "mcp__", true);
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        let rules = doc["permission"]["rules"].as_array_of_tables().unwrap();
        assert_eq!(rules.len(), 3, "{text}");
        assert_eq!(rules.get(0).unwrap()["pattern"].as_str(), Some("Bash(rm *)"));
        assert_eq!(rules.get(1).unwrap()["pattern"].as_str(), Some("mcp__meowo__send_image"));
        assert_eq!(rules.get(2).unwrap()["pattern"].as_str(), Some("mcp__meowo__compare_images"));
        assert_eq!(rules.get(1).unwrap()["decision"].as_str(), Some("allow"));
        // 旧版本只放行了发图：升级后再同步要补上对照，且不留重复。
        let mut old_doc: toml_edit::DocumentMut = text.parse().unwrap();
        old_doc["permission"]["rules"].as_array_of_tables_mut().unwrap().remove(2);
        std::fs::write(&path, old_doc.to_string()).unwrap();
        sync_permission_rule(&path, "mcp__", true);
        let upgraded: toml_edit::DocumentMut = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(upgraded["permission"]["rules"].as_array_of_tables().unwrap().len(), 3);
        assert_eq!(doc["default_model"].as_str(), Some("keep"));
        sync_permission_rule(&path, "mcp__", false);
        let doc: toml_edit::DocumentMut = std::fs::read_to_string(&path).unwrap().parse().unwrap();
        assert_eq!(doc["permission"]["rules"].as_array_of_tables().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        assert!(
            blocked.contains("已阻止") && blocked.contains("刷新"),
            "{blocked}"
        );
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

    /// kimi 的 mcp.json：有 reporter 就加 meowo 一项、没有就摘掉，别的服务器原样保留；
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
        sync_mcp_file(&dir.join("mcp.json"), Some(server_entry("C:/m/meowo-reporter.exe", true, false)));
        let root = read();
        assert_eq!(root["mcpServers"]["meowo"]["args"], json!(["mcp"]));
        assert_eq!(root["mcpServers"]["other"]["command"], "x");
        assert_eq!(root["keep"], 1);
        sync_mcp_file(&dir.join("mcp.json"), None);
        let root = read();
        assert!(root["mcpServers"].get("meowo").is_none());
        assert_eq!(root["mcpServers"]["other"]["command"], "x");
        std::fs::write(&path, "{broken").unwrap();
        sync_mcp_file(&dir.join("mcp.json"), Some(server_entry("C:/m/meowo-reporter.exe", true, false)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 读取失败不等于「文件不存在」：权限/锁/路径是目录之类的错误下绝不写，
    /// 否则会用只剩 meowo 一项的 mcpServers 冲掉用户配置。这里用「目标路径是个目录」
    /// 构造非 NotFound 的读取错误（Windows 上是拒绝访问，Unix 上是 EISDIR）。
    #[test]
    fn mcp_file_read_error_other_than_not_found_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("meowo-mcp-readerr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let target = dir.join("mcp.json");
        std::fs::create_dir_all(&target).unwrap();
        sync_mcp_file(&target, Some(server_entry("C:/m/meowo-reporter.exe", true, false)));
        assert!(target.is_dir(), "目录被替换了");
        let siblings: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            siblings,
            vec![std::ffi::OsString::from("mcp.json")],
            "不该落任何临时/新文件"
        );
        assert_eq!(
            std::fs::read_dir(&target).unwrap().count(),
            0,
            "目录内容被改动"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// NotFound 才当空文件：要登记时新建出只含 meowo 一项的配置；要摘掉时不凭空造文件。
    #[test]
    fn mcp_file_missing_is_created_only_when_enabled() {
        let dir = std::env::temp_dir().join(format!("meowo-mcp-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        sync_mcp_file(&path, None);
        assert!(!path.exists());
        sync_mcp_file(&path, Some(server_entry("C:/m/meowo-reporter.exe", true, false)));
        let root: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(root["mcpServers"]["meowo"]["args"], json!(["mcp"]));
        // 新建的文件没有可备份的原件，不该凭空出一份 .cckb-bak。
        assert!(!dir.join("mcp.json.cckb-bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 覆盖用户已有的文件前留一份原件备份（只留第一次的），同 wiring 的「写前必备份」。
    #[test]
    fn mcp_file_overwrite_backs_up_original_once() {
        let dir = std::env::temp_dir().join(format!("meowo-mcp-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mcp.json");
        let original = r#"{"mcpServers":{"other":{"command":"x"}}}"#;
        std::fs::write(&path, original).unwrap();
        sync_mcp_file(&path, Some(server_entry("C:/m/meowo-reporter.exe", true, false)));
        sync_mcp_file(&path, None);
        assert_eq!(
            std::fs::read_to_string(dir.join("mcp.json.cckb-bak")).unwrap(),
            original
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn first_open_selection_lock_is_shared_per_session() {
        let first = session_selection_lock(i64::MAX);
        let same = session_selection_lock(i64::MAX);
        let other = session_selection_lock(i64::MAX - 1);
        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
        let held = first.lock().unwrap();
        assert!(same.try_lock().is_err());
        drop(held);
        assert!(same.try_lock().is_ok());
    }

    #[test]
    fn old_control_lease_stays_invalid_after_takeover_and_handback() {
        assert!(control_is_current(false, 7, 7));
        assert!(!control_is_current(true, 8, 7));
        // 快速「接管 → 交还」后 paused 又是 false，但代际已变，旧排队调用仍须停止。
        assert!(!control_is_current(false, 9, 7));
        assert!(control_is_current(false, 9, 9));
    }

    #[test]
    fn evaluate_is_not_auto_allowed() {
        assert!(!AUTO_ALLOWED.contains(&"browser_evaluate"));
        assert!(AUTO_ALLOWED.contains(&"browser_click"));
    }

    /// meowo-reporter 的 MCP 工具清单在可执行文件里（不在库里），这里读它的源码核对：
    /// 除 browser_evaluate 外每个工具都在免审批清单里，免审批清单也没有清单外的名字。
    /// 加工具只改一边时，claude 会对新工具逐次弹审批（或放行一个不存在的名字）。
    #[test]
    fn auto_allowed_matches_the_mcp_tool_list() {
        let source = include_str!("../crates/meowo-reporter/src/mcp.rs");
        let body = &source[source.find("fn browser_tools()").expect("mcp.rs 里找不到 browser_tools()")..];
        let body = &body[..body.find("#[cfg(test)]").unwrap_or(body.len())];
        let mut listed: Vec<&str> = Vec::new();
        for (at, _) in body.match_indices("\"browser_") {
            let name = &body[at + 1..];
            let name = &name[..name.find('"').unwrap()];
            if !listed.contains(&name) {
                listed.push(name);
            }
        }
        assert!(listed.contains(&"browser_emulate"), "{listed:?}");
        for name in &listed {
            assert_eq!(
                AUTO_ALLOWED.contains(name),
                *name != "browser_evaluate",
                "{name}"
            );
        }
        for name in AUTO_ALLOWED {
            assert!(
                listed.contains(name),
                "免审批清单里的 {name} 不在 MCP 工具清单中"
            );
        }
    }

    #[test]
    fn strings_are_escaped_for_the_page() {
        assert_eq!(js_str("a\"b\n"), "\"a\\\"b\\n\"");
    }
}
