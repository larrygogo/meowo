//! 关卡答复归**原生 TUI 会话**时，让同一个会话自动开新回合（任务 .lead/20261113-lead-tui）。
//!
//! 按插件声明的机制分发（[`meowo_agent::lead_install::NativeContinue`]），不按 provider 身份：
//! - `InSessionWaiter`：TUI 里的后台等待进程自己读答复、退出、唤醒 CLI，宿主不推送（[`waiter`]）。
//! - `ServerQueue`：TUI 连在宿主管理的服务上，宿主经服务投递（[`server`]）。
//!
//! TUI 不在时一律只保存答复、不起后台进程（leader 2026-11-13「不需要离线」），面板提示打开会话。
//!
//! 约束：不走 ACP（leader：Provider 会把 ACP 流量另算），不往 PTY 里打字（覆盖草稿、误答审批）。

pub(crate) mod codex_app_server;
pub(crate) mod opencode_http;
#[cfg(test)]
mod probe;
#[cfg(test)]
pub(crate) mod probe_util;
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

/// 结果：已送出 / 交给会话内等待器（宿主无需动作）/ 会话不在、答复已保存等打开后送达 /
/// 这家做不到（面板提示手动继续）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeOutcome {
    Delivered,
    WaiterWillWake,
    Saved,
    Unsupported,
}

/// 该 provider 此刻能用的续跑机制：插件声明 + 按本机 CLI 版本过滤（做空 M6：续跑只在查证过的
/// 版本上启用，低于它走手动提示）。版本探测有缓存（probe_cli_version）。
///
/// 只有声明了版本门（[`meowo_agent::lead_install::LeadCap::native_continue_floor`]）的机制才探测
/// 版本（做空 N2），且只在作答投递与面板状态里调用。开终端的启动路径不调用它，改用
/// [`launches_server`]：每个会话启动都会经过 opencode_http::launch_plan 与
/// codex_app_server::prepare_launch，直接调 mechanism() 会让 kimi/gemini 每次开终端都同步跑一遍
/// `<cli> --version`（缓存过期时最长 5 秒），没装 Lead 也一样（做空 R2）。
pub(crate) fn mechanism(provider: &str) -> Option<NativeContinue> {
    mechanism_with(provider, probe_version)
}

fn probe_version(_plugin: &'static dyn meowo_agent::AgentPlugin) -> Option<Option<String>> {
    #[cfg(not(test))]
    let version = Some(crate::probe_cli_version(_plugin));
    // 单测默认不探测真实 CLI（CI 上没装）：按已查证版本处理；版本门由 test_version 覆盖专门测。
    #[cfg(test)]
    let version = test_version::get();
    version
}

/// 开终端的启动路径用：这次启动要不要为 `protocol` 起服务端口。先看插件**声明**的机制（不探测
/// 版本），不是这个协议的 provider（claude、kimi/gemini 的会话内等待器）直接放行、零代价；再看本会话
/// 账号装没装 Lead（`enabled`）；两者都满足才走带版本门的 [`mechanism`]（目前 ServerQueue 机制都
/// 没有版本门，不会真的探测）。
pub(crate) fn launches_server(
    provider: &str,
    protocol: ServerProtocol,
    enabled: impl FnOnce() -> bool,
) -> bool {
    launches_server_with(provider, protocol, enabled, probe_version)
}

fn launches_server_with(
    provider: &str,
    protocol: ServerProtocol,
    enabled: impl FnOnce() -> bool,
    probe: impl FnOnce(&'static dyn meowo_agent::AgentPlugin) -> Option<Option<String>>,
) -> bool {
    let wanted = Some(NativeContinue::ServerQueue(protocol));
    let declared = meowo_agent::by_id(provider)
        .and_then(|plugin| plugin.lead())
        .and_then(|cap| cap.native_continue());
    declared == wanted && enabled() && mechanism_with(provider, probe) == wanted
}

/// `probe` 返回 None = 不看版本（按插件声明）；Some(None) = 读不出版本。
fn mechanism_with(
    provider: &str,
    probe: impl FnOnce(&'static dyn meowo_agent::AgentPlugin) -> Option<Option<String>>,
) -> Option<NativeContinue> {
    let plugin = meowo_agent::by_id(provider)?;
    let cap = plugin.lead()?;
    if cap.native_continue_floor().is_none() {
        return cap.native_continue();
    }
    match probe(plugin) {
        Some(version) => meowo_agent::lead_install::native_continue_for(cap, version.as_deref()),
        None => cap.native_continue(),
    }
}

#[cfg(test)]
pub(crate) mod test_version {
    use std::cell::RefCell;
    thread_local! {
        static VERSION: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
    }
    /// 本线程里 [`super::mechanism`] 看到的 CLI 版本（Some(None) = 读不出版本）；None 恢复默认。
    pub(crate) fn set(version: Option<Option<&str>>) {
        VERSION.with(|v| *v.borrow_mut() = version.map(|inner| inner.map(str::to_owned)));
    }
    pub(super) fn get() -> Option<Option<String>> {
        PROBES.with(|n| n.set(n.get() + 1));
        VERSION.with(|v| v.borrow().clone())
    }
    thread_local! {
        static PROBES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    /// 本线程里 [`super::mechanism`] 被要求看版本的次数（启动路径不该有）。
    pub(super) fn probes() -> u32 {
        PROBES.with(|n| n.get())
    }
    pub(super) fn reset_probes() {
        PROBES.with(|n| n.set(0));
    }
}

pub(crate) fn deliver(target: &NativeTarget<'_>, app: Option<&tauri::AppHandle>) -> Result<NativeOutcome, String> {
    match mechanism(target.provider) {
        None => Ok(NativeOutcome::Unsupported),
        Some(NativeContinue::InSessionWaiter { .. }) => waiter::deliver(target),
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
        // kimi/gemini：会话内等待器，如实说答复走到了哪一步（已收到 / 发送中 / 在会话里说一句 / 打开会话后说一句）。
        // TUI 不在时宿主不续写（leader 2026-11-13），没有可重试的宿主动作。
        NativeContinue::InSessionWaiter { .. } => {
            let store = crate::open_store(db).ok()?;
            let gate = meowo_reporter::lead::read_gate(task).filter(|gate| gate.id == gate_id)?;
            Some(LeadDeliveryDto {
                session_id,
                cwd,
                delivery_id: String::new(),
                can_retry: false,
                state: waiter::status(&store, session_id, task, &gate, provider, native),
                reason: None,
            })
        }
    }
}

/// 面板「继续送达」：清掉失败记录的冷却，紧接着的 deliver 会立即重投；已送出的不受影响。
pub(crate) fn reset_failed(target: &NativeTarget<'_>) {
    if mechanism(target.provider) == Some(NativeContinue::ServerQueue(ServerProtocol::CodexAppServer)) {
        codex_app_server::clear_failure(target.db, target.task, target.gate_id, target.native_session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 做空 N2：没有版本门的机制（服务投递）不探测 CLI 版本——它们的 mechanism() 在开终端的
    /// 启动路径上；有版本门的才探测，且探测结果决定是否续跑。
    #[test]
    fn only_version_gated_mechanisms_probe_the_cli() {
        for provider in ["codex", "opencode"] {
            assert!(
                matches!(
                    mechanism_with(provider, |_| panic!("{provider} 不该探测版本")),
                    Some(NativeContinue::ServerQueue(_))
                ),
                "{provider}"
            );
        }
        for (provider, old, ok) in [("kimi", "2.5.3", "2.6.0"), ("gemini", "0.67.0", "0.68.0")] {
            let mut probed = 0;
            let low = mechanism_with(provider, |_| {
                probed += 1;
                Some(Some(old.into()))
            });
            assert_eq!((low, probed), (None, 1), "{provider}");
            assert!(mechanism_with(provider, |_| Some(Some(ok.into()))).is_some());
            assert!(mechanism_with(provider, |_| Some(None)).is_none(), "读不出版本不续跑");
        }
        assert!(mechanism_with("claude", |_| panic!()).is_none());
    }

    /// 做空 R2：开终端的启动判定对任何 provider 都不探测版本、对非本协议的 provider 也不读 Lead 安装状态；
    /// 只有声明了该协议且装了 Lead 的 provider 才会起服务。
    #[test]
    fn launch_gate_never_probes_cli_version() {
        use ServerProtocol::{CodexAppServer, OpencodeHttp};
        for provider in ["claude", "codex", "kimi", "gemini", "opencode", "nope"] {
            for protocol in [CodexAppServer, OpencodeHttp] {
                let mut enabled_asked = false;
                let launches = launches_server_with(
                    provider,
                    protocol,
                    || {
                        enabled_asked = true;
                        true
                    },
                    |_| panic!("{provider} 启动路径不该探测版本"),
                );
                let ours = matches!(
                    (provider, protocol),
                    ("codex", CodexAppServer) | ("opencode", OpencodeHttp)
                );
                assert_eq!(launches, ours, "{provider} {protocol:?}");
                assert_eq!(enabled_asked, ours, "{provider} {protocol:?}：别的协议不必读 Lead 安装状态");
            }
        }
        assert!(!launches_server_with("codex", CodexAppServer, || false, |_| panic!()));
    }

    /// 启动入口本身（不只判定函数）在 kimi/gemini 上不触发版本探测：mechanism() 的单测探针计数为 0。
    #[test]
    fn launch_entries_do_not_probe_for_waiter_providers() {
        test_version::reset_probes();
        for provider in ["kimi", "gemini", "claude"] {
            let argv = vec![provider.to_owned()];
            assert!(opencode_http::launch_plan(provider, &argv, &[]).is_none());
            let mut command = portable_pty::CommandBuilder::new(provider);
            assert!(codex_app_server::prepare_launch(1, provider, &[], &mut command).is_none());
        }
        assert_eq!(test_version::probes(), 0);
    }
}
