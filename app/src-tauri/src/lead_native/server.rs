//! `ServerQueue`：codex app-server（A 路）/ opencode HTTP（B 路）。
use super::{NativeOutcome, NativeTarget};
use meowo_agent::lead_install::ServerProtocol;

pub(crate) fn deliver(
    _target: &NativeTarget<'_>,
    protocol: ServerProtocol,
    _app: Option<&tauri::AppHandle>,
) -> Result<NativeOutcome, String> {
    match protocol {
        ServerProtocol::CodexAppServer => Err("lead/native_continue_unimplemented".into()),
        ServerProtocol::OpencodeHttp => Err("lead/native_continue_unimplemented".into()),
    }
}
