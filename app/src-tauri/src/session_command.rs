//! Session mutation commands and their shared input validation.

use tauri::State;

/// Safe for agent resume arguments and provider-owned session paths.
pub(crate) fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[tauri::command]
pub(crate) fn rename_session(
    app: tauri::AppHandle,
    state: State<super::AppState>,
    cwd: Option<String>,
    session_id: String,
    title: String,
    provider: Option<String>,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    let title: String = title.trim().chars().take(80).collect();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }

    // Provider persistence is best-effort; the local database remains the UI source of truth.
    if let Some(telemetry) =
        meowo_agent::resolve(provider.as_deref()).and_then(|agent| agent.telemetry())
    {
        let _ = telemetry.write_rename(&session_id, cwd.as_deref(), &title);
    }
    if let Ok(store) = super::open_store(&state.db_path) {
        if let Ok(Some(id)) = store.find_session_id_pub(&session_id) {
            let _ = store.set_session_title(id, &title, super::now_ms());
        }
    }
    super::watch::emit_board_changed(&app, "rename");
    Ok(())
}

#[tauri::command]
pub(crate) fn set_archived(
    app: tauri::AppHandle,
    state: State<super::AppState>,
    session_id: i64,
    archived: bool,
) -> Result<(), String> {
    super::open_store(&state.db_path)?
        .set_session_archived(session_id, archived, super::now_ms())
        .map_err(|error| error.to_string())?;
    super::watch::emit_board_changed(&app, "set_archived");
    Ok(())
}

#[tauri::command]
pub(crate) fn set_session_note(
    app: tauri::AppHandle,
    state: State<super::AppState>,
    session_id: String,
    note: String,
) -> Result<(), String> {
    if !is_safe_id(&session_id) {
        return Err("无效 session_id".into());
    }
    let note: String = note.chars().take(500).collect();
    super::open_store(&state.db_path)?
        .set_session_note(&session_id, &note, super::now_ms())
        .map_err(|error| error.to_string())?;
    super::watch::emit_board_changed(&app, "note");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_safe_id;

    #[test]
    fn session_ids_accept_provider_shapes_and_reject_shell_or_path_syntax() {
        assert!(is_safe_id("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(is_safe_id("session_a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        for invalid in [
            "",
            "../../etc/passwd",
            "a/b",
            "a.b",
            "abc; calc",
            "trailing ",
        ] {
            assert!(!is_safe_id(invalid), "unexpectedly accepted {invalid:?}");
        }
        assert!(!is_safe_id(&"a".repeat(129)));
    }
}
