//! `InSessionWaiter`：kimi / gemini（C 路实现）。
use super::{NativeOutcome, NativeTarget};
use meowo_agent::lead_install::OfflineResume;

pub(crate) fn deliver(
    _target: &NativeTarget<'_>,
    _offline: OfflineResume,
    _app: Option<&tauri::AppHandle>,
) -> Result<NativeOutcome, String> {
    Err("lead/native_continue_unimplemented".into())
}
