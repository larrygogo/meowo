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
pub(crate) mod server;
pub(crate) mod waiter;

use meowo_agent::lead_install::NativeContinue;
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
