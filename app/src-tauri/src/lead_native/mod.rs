//! 关卡答复归**原生 TUI 会话**时，让同一个会话自动开新回合（任务 .lead/20261113-lead-tui）。
//!
//! 按插件声明的机制分发（[`meowo_agent::lead_install::NativeContinue`]），不按 provider 身份：
//! - `InSessionWaiter`：TUI 里的后台等待进程自己读答复、退出、唤醒 CLI。在线时宿主不推送；
//!   TUI 已退出时按插件给的离线方式续写（[`waiter`]）。
//! - `ServerQueue`：TUI 连在宿主管理的服务上，宿主经服务投递（[`server`]）。
//!
//! 约束：不走 ACP（leader：Provider 会把 ACP 流量另算），不往 PTY 里打字（覆盖草稿、误答审批）。

// 临时：三路实现填入前 NativeTarget 字段与 Delivered/WaiterWillWake 尚未使用；实现完成后删除这行。
#![allow(dead_code)]
pub(crate) mod codex_app_server;
pub(crate) mod opencode_http;
pub(crate) mod server;
pub(crate) mod waiter;
mod ws;

use meowo_agent::lead_install::{NativeContinue, ServerProtocol};
use meowo_protocol::ipc::LeadDeliveryDto;
use std::path::Path;

/// 一次原生续跑的上下文。`native_session` 是 CLI 自己的会话 id（.owner 里那个）。
pub(crate) struct NativeTarget<'a> {
    pub db: &'a Path,
    pub session_id: i64,
    pub provider: &'a str,
    pub native_session: &'a str,
    pub cwd: Option<&'a str>,
    pub task: &'a Path,
    pub gate_id: &'a str,
    /// 送给模型的答复正文（reporter::lead::answer_message）。
    pub message: &'a str,
    /// 面板「重试」：跳过自动重试的退避。
    pub force: bool,
}

/// 结果：已送出 / 交给会话内等待器（宿主无需动作）/ 这家做不到（面板提示手动继续）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeOutcome {
    Delivered,
    WaiterWillWake,
    Unsupported,
}

pub(crate) fn mechanism(provider: &str) -> Option<NativeContinue> {
    meowo_agent::by_id(provider)?.lead()?.native_continue()
}

pub(crate) fn deliver(target: &NativeTarget<'_>, app: Option<&tauri::AppHandle>) -> Result<NativeOutcome, String> {
    match mechanism(target.provider) {
        None => Ok(NativeOutcome::Unsupported),
        Some(NativeContinue::InSessionWaiter { offline }) => waiter::deliver(target, offline, app),
        Some(NativeContinue::ServerQueue(protocol)) => server::deliver(target, protocol, app),
    }
}

/// opencode_http 内部用的送达状态（state 取 i18n leadDeliveryState 的键），由 [`delivery_status`] 转成面板 DTO。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeStatus {
    pub state: &'static str,
    pub reason: Option<String>,
    pub can_retry: bool,
}

/// 原生会话关卡答复的真实送达状态，**唯一入口**：按机制分发，一律返回面板要的 DTO。
/// None = 该机制没有宿主侧记录（调用方沿用原有面板提示）。
pub(crate) fn delivery_status(
    db: &Path,
    provider: &str,
    session_id: i64,
    cwd: String,
    task: &Path,
    gate_id: &str,
    native: &str,
) -> Option<LeadDeliveryDto> {
    match mechanism(provider)? {
        NativeContinue::ServerQueue(ServerProtocol::CodexAppServer) => Some(
            codex_app_server::status(db, session_id, cwd, task, gate_id, native),
        ),
        NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp) => {
            let status = opencode_http::status(db, task, gate_id, native);
            Some(LeadDeliveryDto {
                session_id,
                cwd,
                delivery_id: String::new(),
                can_retry: status.can_retry,
                state: status.state.into(),
                reason: status.reason,
            })
        }
        _ => None,
    }
}

/// 面板「继续送达」：清掉失败记录的冷却，紧接着的 deliver 会立即重投；已送出的不受影响。
pub(crate) fn reset_failed(target: &NativeTarget<'_>) {
    if mechanism(target.provider) == Some(NativeContinue::ServerQueue(ServerProtocol::CodexAppServer)) {
        codex_app_server::clear_failure(target.db, target.task, target.gate_id, target.native_session);
    }
}
