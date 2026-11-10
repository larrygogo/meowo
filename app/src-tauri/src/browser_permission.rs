//! 面板浏览器里网页的权限请求（下载多个文件、摄像头、麦克风、位置、通知……），Windows 实装。
//!
//! 为什么自己接 WebView2 的 `PermissionRequested`：不接时 WebView2 弹自己的原生权限框，它不归
//! 子 webview 的显隐管，页签切走、面板收起后还浮在 Meowo 上（2026-11-10 用户实拍「httpbin.org
//! 想要 下载多个文件」，切页面也还在）。tauri 2.12 的 `on_permission_request` 是同步回调，拿不到
//! 发起请求的网址，也不能先挂起、等用户在面板里选，所以这里用 `GetDeferral` 挂起请求，由面板里的
//! 「允许 / 阻止」答复（leader 在 2026-11-10 选定方案 A）。所有权限种类同一个毛病，一并接管。
//!
//! COM 对象不是 Send：挂起的请求只存在主线程的 thread_local 里，读写都经 `run_on_main_thread`
//! 回到主线程（WebView2 的事件本来就在主线程触发）。
//!
//! macOS 的 WKWebView 没有这类原生框（不拦多次下载），这里是空实现。

#[cfg(not(target_os = "windows"))]
use tauri::Url;

#[cfg(target_os = "windows")]
pub(crate) use imp::{answer, drop_stale, forget, install};

#[cfg(target_os = "windows")]
mod imp {
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};

    use tauri::{Manager, Url};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Deferral, ICoreWebView2PermissionRequestedEventArgs,
        ICoreWebView2PermissionRequestedEventArgs3,
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ, COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
        COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, COREWEBVIEW2_PERMISSION_STATE,
        COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_PERMISSION_STATE_DEFAULT,
        COREWEBVIEW2_PERMISSION_STATE_DENY,
    };
    use webview2_com::{take_pwstr, PermissionRequestedEventHandler};
    use windows::core::{Interface, PWSTR};

    use crate::browser_pane::{update, BrowserPanes, BrowserPermission};

    struct Pending {
        prompt: BrowserPermission,
        args: ICoreWebView2PermissionRequestedEventArgs,
        deferral: ICoreWebView2Deferral,
    }

    thread_local! {
        /// 每个页签挂起的请求，先来先答。面板只显示排在最前的一条。
        static PENDING: RefCell<HashMap<u32, VecDeque<Pending>>> = RefCell::new(HashMap::new());
    }

    /// WebView2 的权限种类 → 前端文案键。不认识的归 other，文案说「使用额外权限」。
    pub(super) fn kind_name(kind: COREWEBVIEW2_PERMISSION_KIND) -> &'static str {
        match kind {
            COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS => "downloads",
            COREWEBVIEW2_PERMISSION_KIND_CAMERA => "camera",
            COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => "microphone",
            COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => "location",
            COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => "notifications",
            COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => "clipboard",
            COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE => "files",
            _ => "other",
        }
    }

    /// 建好页签后调用：挂上权限请求处理器。
    pub(crate) fn install(app: &tauri::AppHandle, webview: &tauri::Webview, id: u32) {
        let app = app.clone();
        let _ = webview.with_webview(move |platform| {
            let result = (|| -> windows::core::Result<()> {
                // SAFETY: with_webview 的闭包在主线程执行，controller 是活的 COM 指针。
                let core = unsafe { platform.controller().CoreWebView2()? };
                let handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut state = COREWEBVIEW2_PERMISSION_STATE::default();
                    unsafe { args.State(&mut state)? };
                    // 先注册的处理器已经定了（wry 对剪贴板读取自动放行），不再问用户。
                    if state != COREWEBVIEW2_PERMISSION_STATE_DEFAULT {
                        return Ok(());
                    }
                    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                    unsafe { args.PermissionKind(&mut kind)? };
                    let mut uri = PWSTR::null();
                    unsafe { args.Uri(&mut uri)? };
                    let uri = take_pwstr(uri);
                    // 挂起之后 WebView2 不弹原生框，等 Complete。
                    let deferral = unsafe { args.GetDeferral()? };
                    // 答复只管这一次，不写进 WebView2 配置：默认会记住，误点一次「阻止」这个网站
                    // 就永远不能再连续下载（摄像头等同理），而 Meowo 里没有撤销的地方。实测（同版本
                    // wry 0.57.1 测试程序）：不记住时同一页面只问一次，换页面或重开再问，同 Chrome。
                    // 旧版 WebView2 运行时没有这个接口，失败就按默认来。
                    if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                        let _ = unsafe { args3.SetSavesInProfile(false) };
                    }
                    let prompt = BrowserPermission::new(kind_name(kind), &uri);
                    PENDING.with(|pending| {
                        pending
                            .borrow_mut()
                            .entry(id)
                            .or_default()
                            .push_back(Pending {
                                prompt,
                                args,
                                deferral,
                            });
                    });
                    publish(&app, id);
                    Ok(())
                }));
                let mut token = 0i64;
                unsafe { core.add_PermissionRequested(&handler, &mut token) }
            })();
            if let Err(error) = result {
                eprintln!("面板浏览器接管权限请求失败: {error}");
            }
        });
    }

    /// 把排在最前的请求同步给前端（没有了就清掉提示）。在主线程上调。
    fn publish(app: &tauri::AppHandle, id: u32) {
        let front = PENDING.with(|pending| {
            pending
                .borrow()
                .get(&id)
                .and_then(|queue| queue.front())
                .map(|entry| entry.prompt.clone())
        });
        let panes = app.state::<BrowserPanes>().panes.clone();
        update(app, &panes, id, |info| info.permission = front);
    }

    fn settle(entry: Pending, allow: bool) {
        let state = if allow {
            COREWEBVIEW2_PERMISSION_STATE_ALLOW
        } else {
            COREWEBVIEW2_PERMISSION_STATE_DENY
        };
        // 页签已经关了、页面已经走了时这两步会失败，请求本来也作废了，忽略即可。
        unsafe {
            let _ = entry.args.SetState(state);
            let _ = entry.deferral.Complete();
        }
    }

    /// 面板里点了「允许 / 阻止」：答复排在最前的那条。
    pub(crate) fn answer(app: &tauri::AppHandle, id: u32, allow: bool) -> Result<(), String> {
        let handle = app.clone();
        app.run_on_main_thread(move || {
            let next = PENDING.with(|pending| {
                pending
                    .borrow_mut()
                    .get_mut(&id)
                    .and_then(|queue| queue.pop_front())
            });
            if let Some(entry) = next {
                let prompt = entry.prompt.clone();
                settle(entry, allow);
                // 阻止后同一页面不会再问，被拦的下载连事件都没有：留一条「已阻止」提示。
                let panes = handle.state::<BrowserPanes>().panes.clone();
                update(&handle, &panes, id, |info| {
                    info.blocked = if allow { None } else { Some(prompt) };
                });
            }
            publish(&handle, id);
        })
        .map_err(|e| e.to_string())
    }

    /// 页面跳到了别的网站：旧网站的请求按阻止处理，提示随之消失。同站跳转（点附件链接
    /// 下载也会起一次导航）不动，否则刚弹出的「下载多个文件」会被自己的导航冲掉。
    pub(crate) fn drop_stale(app: &tauri::AppHandle, id: u32, url: Url) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let stale: VecDeque<Pending> = PENDING.with(|pending| {
                let mut map = pending.borrow_mut();
                let Some(queue) = map.get_mut(&id) else {
                    return VecDeque::new();
                };
                let (stale, keep): (VecDeque<Pending>, VecDeque<Pending>) =
                    queue.drain(..).partition(|entry| entry.prompt.stale_for(&url));
                *queue = keep;
                stale
            });
            if stale.is_empty() {
                return;
            }
            for entry in stale {
                settle(entry, false);
            }
            publish(&handle, id);
        });
    }

    /// 页签关掉：挂起的请求一律按阻止处理并丢掉，不留 COM 引用。
    pub(crate) fn forget(app: &tauri::AppHandle, id: u32) {
        let _ = app.run_on_main_thread(move || {
            let queue = PENDING.with(|pending| pending.borrow_mut().remove(&id));
            for entry in queue.into_iter().flatten() {
                settle(entry, false);
            }
        });
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::imp::kind_name;
    use webview2_com::Microsoft::Web::WebView2::Win32::*;

    #[test]
    fn webview2_permission_kinds_map_to_prompt_keys() {
        // 前端 browserPermissionKinds 的键与这里一一对应，不认识的一律 other。
        let cases = [
            (COREWEBVIEW2_PERMISSION_KIND_MULTIPLE_AUTOMATIC_DOWNLOADS, "downloads"),
            (COREWEBVIEW2_PERMISSION_KIND_CAMERA, "camera"),
            (COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, "microphone"),
            (COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION, "location"),
            (COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, "notifications"),
            (COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ, "clipboard"),
            (COREWEBVIEW2_PERMISSION_KIND_FILE_READ_WRITE, "files"),
            (COREWEBVIEW2_PERMISSION_KIND_WINDOW_MANAGEMENT, "other"),
            (COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION, "other"),
        ];
        for (kind, name) in cases {
            assert_eq!(kind_name(kind), name);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn install(_app: &tauri::AppHandle, _webview: &tauri::Webview, _id: u32) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn answer(_app: &tauri::AppHandle, _id: u32, _allow: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn drop_stale(_app: &tauri::AppHandle, _id: u32, _url: Url) {}

#[cfg(not(target_os = "windows"))]
pub(crate) fn forget(_app: &tauri::AppHandle, _id: u32) {}
