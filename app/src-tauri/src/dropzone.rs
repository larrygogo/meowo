//! Windows 上对话窗文件拖入的兜底注册（前端零改动，仍收 `tauri://drag-*`）。
//!
//! 为什么需要：wry 0.57 的 Windows 拖放只在 webview 创建时 `EnumChildWindows` 一次，
//! 给当时存在的子窗口各注册一个 `IDropTarget`（webview2/drag_drop.rs）。实测（wry#1639
//! 2026-10-18 评论）页面真正绘制在 **msedgewebview2 browser 进程**的
//! `Chrome_RenderWidgetHostHWND` 上，这些窗口在 `build()` 返回后才 reparent 进本窗口树；
//! 而 OLE 的拖放命中测试只在**同一进程内**向上找已注册的祖先——wry 注册在宿主进程
//! `Chrome_WidgetWin_0` 上的目标从页面永远够不到，拖入直接被系统拒绝（连 enter 都没有，
//! 粘贴不受影响因为不走 OLE）。WebView2 运行时自动更新改变挂载时序后，创建时那一次
//! 枚举就再也赶不上，表现为「以前能拖，现在只能粘贴」。官方修复 wry#1638 迟迟未合并，
//! 且 show 时重扫仍是事后检查、照样可能丢竞态，所以在应用层按事件驱动补齐。
//!
//! 做法（同 wry#1639 评论给出的方案，参考其 MIT 实现）：
//! - 对 WebView2 browser 进程挂 `EVENT_OBJECT_CREATE` / `EVENT_OBJECT_PARENTCHANGE`
//!   的 WinEvent 钩子（先挂钩再全树走一遍，先到的事件驱动补扫，谁都不必等谁）；
//! - 凡根部属于本窗口的子窗口（含 browser 进程的），都注册**我们自己的** IDropTarget，
//!   顶替 wry / Chromium 的——全窗只有这一个实现，事件名与 payload 与 Tauri 原样一致；
//! - browser 进程崩溃换 pid 时经 `ProcessFailed` 重挂钩；窗口销毁时摘钩并注销。
//!
//! 已知残留（参考实现实测无害）：Chromium 的设置落地时可能收回 `Chrome_WidgetWin_1`
//! 上的目标且不发声，但铺满页面的 render window 上的目标它从不动；GPU 进程的
//! `Intermediate D3D Window` 是 `WS_EX_LAYERED | WS_EX_TRANSPARENT`，命中测试会穿过它。

#[cfg(windows)]
mod imp {
    use std::{
        cell::{Cell, RefCell},
        collections::{HashMap, HashSet},
        ffi::OsString,
        os::windows::ffi::OsStringExt,
        path::PathBuf,
        rc::Rc,
    };
    use tauri::{AppHandle, Emitter, EventTarget, Manager, WebviewWindow, WindowEvent};
    use webview2_com::ProcessFailedEventHandler;
    use windows::{
        core::{implement, w, Interface, BOOL},
        Win32::{
            Foundation::{HWND, LPARAM, POINT, POINTL},
            Graphics::Gdi::ScreenToClient,
            System::{
                Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL},
                Ole::{
                    IDropTarget, IDropTarget_Impl, RegisterDragDrop, RevokeDragDrop, CF_HDROP,
                    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
                },
                SystemServices::MODIFIERKEYS_FLAGS,
            },
            UI::{
                Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
                Shell::{DragFinish, DragQueryFileW, HDROP},
                WindowsAndMessaging::{
                    EnumChildWindows, GetAncestor, GetClassNameW, GetPropW, CHILDID_SELF,
                    EVENT_OBJECT_CREATE, EVENT_OBJECT_PARENTCHANGE, GA_ROOT, OBJID_WINDOW,
                    WINEVENT_OUTOFCONTEXT,
                },
            },
        },
    };

    /// 与 Tauri `DragDropEvent` 同名的四种事件；position 前端目前不读，仍按协议给。
    enum Event {
        Enter(Vec<PathBuf>, (i32, i32)),
        Over((i32, i32)),
        Drop(Vec<PathBuf>, (i32, i32)),
        Leave,
    }

    type Say = Rc<dyn Fn(Event)>;

    struct Armed {
        top: HWND,
        /// 坐标统一换算到 webview 容器（页面原点），与落在哪个子窗口无关。
        page: HWND,
        say: Say,
    }

    #[derive(Default)]
    struct State {
        armed: Option<Armed>,
        hooked_pid: Option<u32>,
        hooks: Vec<HWINEVENTHOOK>,
        /// 本模块注册的目标按窗口留存：注册只在有人持有时存活，COM 对象非 Send，
        /// 全部留在主线程的 thread_local 里。
        held: HashMap<isize, IDropTarget>,
        /// 注册失败的窗口只报一次，免得每次补扫都刷日志。
        refused: HashSet<isize>,
    }

    thread_local! {
        static STATE: RefCell<State> = RefCell::new(State::default());
        /// 跨进程 `RegisterDragDrop` 等待时会泵消息，钩子回调可能因此重入补扫：
        /// 记下「还要再走一遍」，由外层循环收尾，不递归。
        static WALKING: Cell<bool> = const { Cell::new(false) };
        static AGAIN: Cell<bool> = const { Cell::new(false) };
    }

    #[implement(IDropTarget)]
    struct Target {
        page: HWND,
        say: Say,
        effect: Cell<DROPEFFECT>,
        /// 非文件拖拽（如网页里拖文字）没发过 Enter，DragLeave/Drop 不得对应发声。
        valid: Cell<bool>,
    }

    /// 取出拖拽携带的文件路径；不是文件拖拽返回 None。
    unsafe fn paths_of(data: &IDataObject) -> Option<(Vec<PathBuf>, HDROP)> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let medium = data.GetData(&format).ok()?;
        let hdrop = HDROP(medium.u.hGlobal.0 as _);
        let mut paths = Vec::new();
        for i in 0..DragQueryFileW(hdrop, 0xFFFF_FFFF, None) {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            paths.push(OsString::from_wide(&buf[..len]).into());
        }
        Some((paths, hdrop))
    }

    impl Target {
        fn client(&self, pt: &POINTL) -> (i32, i32) {
            let mut p = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { ScreenToClient(self.page, &mut p) };
            (p.x, p.y)
        }
    }

    #[allow(non_snake_case)]
    impl IDropTarget_Impl for Target_Impl {
        fn DragEnter(
            &self,
            data: windows::core::Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            let Some(data) = data.as_ref() else { return Ok(()) };
            let Some((paths, _)) = (unsafe { paths_of(data) }) else {
                self.valid.set(false);
                self.effect.set(DROPEFFECT_NONE);
                unsafe { *effect = DROPEFFECT_NONE };
                return Ok(());
            };
            self.valid.set(true);
            (self.say)(Event::Enter(paths, self.client(pt)));
            self.effect.set(DROPEFFECT_COPY);
            unsafe { *effect = DROPEFFECT_COPY };
            Ok(())
        }

        fn DragOver(
            &self,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            if self.valid.get() {
                (self.say)(Event::Over(self.client(pt)));
            }
            unsafe { *effect = self.effect.get() };
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            if self.valid.replace(false) {
                (self.say)(Event::Leave);
            }
            Ok(())
        }

        fn Drop(
            &self,
            data: windows::core::Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            if !self.valid.replace(false) {
                unsafe { *effect = DROPEFFECT_NONE };
                return Ok(());
            }
            let Some(data) = data.as_ref() else { return Ok(()) };
            if let Some((paths, hdrop)) = unsafe { paths_of(data) } {
                (self.say)(Event::Drop(paths, self.client(pt)));
                unsafe { DragFinish(hdrop) };
            }
            unsafe { *effect = DROPEFFECT_COPY };
            Ok(())
        }
    }

    /// `top` 的全部后代窗口（任意深度；窗口树是系统级的，含其他进程的子窗口）。
    fn descendants(top: HWND) -> Vec<HWND> {
        unsafe extern "system" fn each(hwnd: HWND, found: LPARAM) -> BOOL {
            unsafe { (*(found.0 as *mut Vec<HWND>)).push(hwnd) };
            true.into()
        }
        let mut found: Vec<HWND> = Vec::new();
        let _ = unsafe { EnumChildWindows(Some(top), Some(each), LPARAM(&mut found as *mut _ as isize)) };
        found
    }

    fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
        String::from_utf16_lossy(&buf[..len])
    }

    /// 全树补扫，直到没有新的重入请求；本模块唯一的注册入口。
    fn settle() {
        if WALKING.replace(true) {
            AGAIN.set(true);
            return;
        }
        loop {
            AGAIN.set(false);
            STATE.with(|s| walk(&mut s.borrow_mut()));
            if !AGAIN.get() {
                break;
            }
        }
        WALKING.set(false);
    }

    fn walk(state: &mut State) {
        let Some(armed) = state.armed.as_ref() else { return };
        let (top, page, say) = (armed.top, armed.page, armed.say.clone());
        for hwnd in descendants(top) {
            let key = hwnd.0 as isize;
            // OleDropTargetInterface 属性存的就是注册时的指针，「是不是我们的」直接比指针。
            let current = unsafe { GetPropW(hwnd, w!("OleDropTargetInterface")) };
            if let Some(ours) = state.held.get(&key) {
                if current.0 == ours.as_raw() {
                    continue;
                }
            }
            // 别人的（wry 的、Chromium 自己的）先撤下，保证只有一个实现在应答。
            // 跨进程窗口上 RevokeDragDrop 可能报 RPC_E_WRONG_THREAD 但已生效，
            // 所以不看它的返回值，以下面 RegisterDragDrop 的结果为准。
            if !current.is_invalid() {
                let _ = unsafe { RevokeDragDrop(hwnd) };
            }
            let target: IDropTarget = Target {
                page,
                say: say.clone(),
                effect: Cell::new(DROPEFFECT_NONE),
                valid: Cell::new(false),
            }
            .into();
            // 跨进程注册：OLE 接受从别的进程注册、并在拖入时回调到这里——这是
            // wry#1639 评论的实测结论而非文档承诺。若 WebView2 更新改变了这一点，
            // 会在这里以窗口类名报警。
            match unsafe { RegisterDragDrop(hwnd, &target) } {
                Ok(()) => {
                    state.refused.remove(&key);
                    state.held.insert(key, target);
                }
                Err(e) => {
                    if state.refused.insert(key) {
                        eprintln!("[dropzone] 给 {} 注册拖放目标失败: {e}（拖到该区域会被拒绝）", class_of(hwnd));
                    }
                }
            }
        }
    }

    /// 非本窗口的事件只有两次比较 + 一次 GetAncestor 的开销。
    /// 钩的是整个 browser 进程，它替所有页面/弹窗建窗口，不只我们的。
    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        object: i32,
        child: i32,
        _thread: u32,
        _time: u32,
    ) {
        if object != OBJID_WINDOW.0 || child != CHILDID_SELF as i32 || hwnd.is_invalid() {
            return;
        }
        if WALKING.get() {
            AGAIN.set(true);
            return;
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let ours = STATE.with(|s| s.borrow().armed.as_ref().is_some_and(|a| a.top == root));
        if ours {
            settle();
        }
    }

    fn unhook(state: &mut State) {
        for hook in state.hooks.drain(..) {
            if !hook.is_invalid() {
                let _ = unsafe { UnhookWinEvent(hook) };
            }
        }
        state.hooked_pid = None;
    }

    /// 主线程。pid 没变就只补扫；变了先挂新钩再摘旧钩，窗口不错过任何事件。
    fn arm(pid: u32) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.hooked_pid == Some(pid) {
                return;
            }
            unhook(&mut s);
            let hook = |event| unsafe {
                SetWinEventHook(event, event, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT)
            };
            let hooks = vec![hook(EVENT_OBJECT_CREATE), hook(EVENT_OBJECT_PARENTCHANGE)];
            if hooks.iter().any(|h| h.is_invalid()) {
                eprintln!("[dropzone] 监视 browser 进程 {pid} 的新窗口失败");
            }
            s.hooks = hooks;
            s.hooked_pid = Some(pid);
        });
        settle();
    }

    fn disarm() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            unhook(&mut s);
            s.armed = None;
            // 仍立着的窗口显式注销，OLE 那边也放手；窗口已随销毁消失的，放掉引用即释放。
            for (key, _) in s.held.drain() {
                let _ = unsafe { RevokeDragDrop(HWND(key as _)) };
            }
        });
    }

    pub fn watch(window: &WebviewWindow) {
        // HWND 是指针不是 Send，先落成数字；webview 只能在 with_webview 闭包里碰。
        let Ok(top) = window.hwnd().map(|h| h.0 as isize) else { return };
        let label = window.label().to_string();
        let app: AppHandle = window.app_handle().clone();

        window.on_window_event(move |event| {
            if matches!(event, WindowEvent::Destroyed) {
                disarm();
            }
        });

        let result = window.with_webview(move |webview| {
            let controller = webview.controller();
            let mut page = HWND::default();
            let _ = unsafe { controller.ParentWindow(&mut page) };
            let Ok(core) = (unsafe { controller.CoreWebView2() }) else {
                eprintln!("[dropzone] {label}: 拿不到 WebView2，文件拖入不可用");
                return;
            };

            // 与 tauri 2.12 manager/window.rs 的 is_webview_window 分支同一路由：
            // labeled 同时覆盖 Window/WebviewWindow 监听目标，前端 getCurrentWindow()
            // .onDragDropEvent 照收，不用改一行前端。
            let to = EventTarget::labeled(label.clone());
            let say: Say = Rc::new(move |event| {
                let pos = |(x, y): (i32, i32)| serde_json::json!({ "x": x, "y": y });
                let _ = match event {
                    Event::Enter(paths, p) => app.emit_to(
                        to.clone(),
                        "tauri://drag-enter",
                        serde_json::json!({ "paths": paths, "position": pos(p) }),
                    ),
                    Event::Over(p) => app.emit_to(
                        to.clone(),
                        "tauri://drag-over",
                        serde_json::json!({ "position": pos(p) }),
                    ),
                    Event::Drop(paths, p) => app.emit_to(
                        to.clone(),
                        "tauri://drag-drop",
                        serde_json::json!({ "paths": paths, "position": pos(p) }),
                    ),
                    Event::Leave => app.emit_to(to.clone(), "tauri://drag-leave", ()),
                };
            });
            STATE.with(|s| {
                s.borrow_mut().armed = Some(Armed {
                    top: HWND(top as _),
                    page: if page.is_invalid() { HWND(top as _) } else { page },
                    say,
                });
            });

            let browser_pid = |core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2| {
                let mut pid = 0u32;
                unsafe { core.BrowserProcessId(&mut pid) }.ok().filter(|_| pid != 0).map(|_| pid)
            };
            match browser_pid(&core) {
                Some(pid) => arm(pid),
                None => eprintln!("[dropzone] {label}: 读不到 browser 进程号，文件拖入不可用"),
            }

            // renderer/GPU/browser 进程死了会被换成新进程，新 browser 的 pid 旧钩子听不到；
            // 每次失败都重问一遍，pid 没变时 arm 是 no-op。
            let mut token = 0i64;
            let handler = ProcessFailedEventHandler::create(Box::new(move |sender, _args| {
                if let Some(pid) = sender.as_ref().and_then(browser_pid) {
                    arm(pid);
                }
                Ok(())
            }));
            let _ = unsafe { core.add_ProcessFailed(&handler, &mut token) };
        });
        if let Err(e) = result {
            eprintln!("[dropzone] {}: 接触 webview 失败（{e}），文件拖入不可用", window.label());
        }
    }
}

/// 建窗后调用一次，保这扇窗的文件拖入随窗口存活。非 Windows 上 wry 的拖放是
/// webview 自身的实现（WKWebView 子类方法 / GTK 信号），没有后到窗口的问题，无需处理。
#[cfg(windows)]
pub fn watch(window: &tauri::WebviewWindow) {
    imp::watch(window);
}

#[cfg(not(windows))]
pub fn watch(_window: &tauri::WebviewWindow) {}
