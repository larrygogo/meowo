//! `ServerQueue`：codex app-server（A 路）/ opencode HTTP（B 路）。
use super::{NativeOutcome, NativeTarget};
use meowo_agent::lead_install::ServerProtocol;

pub(crate) fn deliver(
    target: &NativeTarget<'_>,
    protocol: ServerProtocol,
    app: Option<&tauri::AppHandle>,
) -> Result<NativeOutcome, String> {
    match protocol {
        ServerProtocol::CodexAppServer => super::codex_app_server::deliver(target, app),
        ServerProtocol::OpencodeHttp => super::opencode_http::deliver(target, app),
    }
}
