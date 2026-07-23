//! 确认对话框的**原生小窗**宿主:应用样式(webview 渲染,吃主题/字体)+ 原生窗口能力
//! (独立窗口,可拖拽、可拖出主窗边界)。两头要求只有「无边框小窗 + 应用 CSS」同时满足:
//! 系统 MessageBox 样式脱节(用户嫌丑),webview 内嵌模态又出不了窗口边界。
//!
//! 协议:请求方 invoke [`confirm_dialog`](等待结果)→ 建 `confirm-<id>` 小窗(前端按
//! label 前缀路由到 ConfirmWindow 视图)→ 小窗取 payload 渲染 → 用户点按钮 invoke
//! [`confirm_dialog_result`] → 结果经通道送回请求方。小窗被直接关闭(Alt-F4)按取消
//! 收场——确认宁可失败,绝不静默当同意。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, serde::Serialize)]
pub(crate) struct ConfirmPayload {
    title: String,
    message: String,
    danger: bool,
}

type ResultSender = tauri::async_runtime::Sender<bool>;

#[derive(Default)]
pub(crate) struct Confirms {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, (ConfirmPayload, Option<ResultSender>)>>,
}

impl Confirms {
    fn take_sender(&self, id: u64) -> Option<ResultSender> {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get_mut(&id)
            .and_then(|(_, sender)| sender.take())
    }

    fn remove(&self, id: u64) {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&id);
    }
}

/// 固定逻辑尺寸:高度给两行消息留量,更长的消息在卡片内滚动。
const WIDTH: f64 = 420.0;
const HEIGHT: f64 = 208.0;

#[tauri::command]
pub(crate) async fn confirm_dialog(
    app: AppHandle,
    window: tauri::Window,
    state: tauri::State<'_, super::AppState>,
    title: String,
    message: String,
    danger: Option<bool>,
) -> Result<bool, String> {
    let confirms = state.confirms.clone();
    let id = confirms.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = tauri::async_runtime::channel::<bool>(1);
    confirms
        .pending
        .lock()
        .map_err(|_| "确认状态锁已损坏")?
        .insert(
            id,
            (
                ConfirmPayload {
                    title,
                    message,
                    danger: danger.unwrap_or(false),
                },
                Some(tx),
            ),
        );
    let label = format!("confirm-{id}");
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // 首帧渲染完由前端 useShowWhenReady 显示,消除白框闪烁(与其余窗口同款)。
        .visible(false);
    // 居中于请求窗口(物理坐标折算逻辑坐标);任一项拿不到就交给系统默认落点。
    if let (Ok(pos), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.outer_size(),
        window.scale_factor(),
    ) {
        let x = (pos.x as f64 + (size.width as f64 - WIDTH * scale) / 2.0) / scale;
        let y = (pos.y as f64 + (size.height as f64 - HEIGHT * scale) / 2.0) / scale;
        builder = builder.position(x, y);
    }
    let confirm_window = builder.build().map_err(|error| {
        confirms.remove(id);
        error.to_string()
    })?;
    // 被直接关闭(Alt-F4/系统关闭)= 取消:sender 还在说明没人给过答案。
    {
        let confirms = confirms.clone();
        confirm_window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(sender) = confirms.take_sender(id) {
                    let _ = sender.try_send(false);
                }
            }
        });
    }
    let ok = rx.recv().await.unwrap_or(false);
    confirms.remove(id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }
    Ok(ok)
}

/// 小窗启动后取渲染内容。走命令而不是 URL 参数:标题/正文是任意用户语言文本,
/// 省掉转义,也不把内容留在窗口 URL 里。
#[tauri::command]
pub(crate) fn confirm_dialog_payload(
    state: tauri::State<'_, super::AppState>,
    id: u64,
) -> Result<ConfirmPayload, String> {
    state
        .confirms
        .pending
        .lock()
        .map_err(|_| "确认状态锁已损坏")?
        .get(&id)
        .map(|(payload, _)| payload.clone())
        .ok_or_else(|| "确认请求不存在".into())
}

/// 小窗按钮的裁决。纯内存转发,同步命令(见 CLAUDE.md 线程纪律)。
#[tauri::command]
pub(crate) fn confirm_dialog_result(state: tauri::State<'_, super::AppState>, id: u64, ok: bool) {
    if let Some(sender) = state.confirms.take_sender(id) {
        let _ = sender.try_send(ok);
    }
}
