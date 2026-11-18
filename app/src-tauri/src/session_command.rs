//! 会话写命令与它们共享的输入校验。

use tauri::State;

/// 可安全用于 agent 的 resume 参数与 provider 自有的会话路径。
pub(crate) fn is_safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

// 三条写库命令都 async + spawn_blocking：同步命令跑在主线程，而 reporter 的 hook 进程
// 会并发写同一个库——撞上写锁时 busy_timeout（3s）就直接冻住消息泵；rename 还要写
// provider 的 telemetry 文件。
#[tauri::command]
pub(crate) async fn rename_session(
    app: tauri::AppHandle,
    state: State<'_, super::AppState>,
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
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Provider persistence is best-effort; the local database remains the UI source of truth.
        if let Some(telemetry) =
            meowo_agent::resolve(provider.as_deref()).and_then(|agent| agent.telemetry())
        {
            let _ = telemetry.write_rename(&session_id, cwd.as_deref(), &title);
        }
        if let Ok(store) = super::open_store(&db_path) {
            if let Ok(Some(id)) = store.find_session_id_pub(&session_id) {
                // 上锁：之后 agent 自己起的标题（codex/kimi/opencode）不再覆盖用户定的名字。
                let _ = store.rename_session_title(id, &title, super::now_ms());
            }
        }
        super::watch::emit_board_changed(&app, "rename");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 侧栏拖拽排序：把会话挪到 `above_id` 正下方（或没有 above 时 `below_id` 正上方）。
/// 桌面拖动、手机长按拖动都走这里（远程白名单已放行：只改顺序，不碰会话内容）。
#[tauri::command]
pub(crate) async fn move_session_rank(
    app: tauri::AppHandle,
    state: State<'_, super::AppState>,
    session_id: i64,
    above_id: Option<i64>,
    below_id: Option<i64>,
) -> Result<(), String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::open_store(&db_path)?
            .move_session_rank(session_id, above_id, below_id)
            .map_err(|error| error.to_string())?;
        super::watch::emit_board_changed(&app, "move_session_rank");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn set_archived(
    app: tauri::AppHandle,
    state: State<'_, super::AppState>,
    session_id: i64,
    archived: bool,
) -> Result<(), String> {
    let db_path = state.db_path.clone();
    let ptys = state.ptys.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = super::open_store(&db_path)?;
        apply_archived(&store, session_id, archived, super::now_ms())?;
        // archived = 「已完成」：标记完成同时结束会话（用户要求，理由见 end_session_for_done）。
        // 恢复进行中不自动拉起——那是点开会话时的「恢复」动作，带着用户的启动选项。
        if archived {
            if let Err(error) =
                super::managed_terminal::end_session_for_done(&ptys, &db_path, session_id)
            {
                eprintln!("标记完成后结束会话 {session_id} 失败: {error}");
            }
            // 会话关了，面板里它开的终端和浏览器也关（会话本来就没在跑时上面不会触发退出收尾）。
            crate::browser_agent::close_session_panels(&app, session_id);
        }
        super::watch::emit_board_changed(&app, "set_archived");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 改「已完成」标记的库操作（命令外壳只管线程与收尾，便于单测）。
///
/// - 标记完成时趁目录还在记下原项目（worktree 的主检出）：之后 worktree 被清理、`.git` 文件跟着没了，
///   目录已删的会话靠这一笔给「在原项目新开会话」（见 wtnew::origin::origin_for_missing）。
///   推不出（不是 worktree）就不记，不覆盖已有的记录。
/// - 目录已经删掉的会话不能放回「进行中」：放回去也续不上（agent 按目录找会话记录），只会多一张
///   点开就是「目录已删除」的卡。所有入口（侧栏、会话管理页、手机、批量）都走这里，一处拦住。
pub(crate) fn apply_archived(
    store: &meowo_store::Store,
    session_id: i64,
    archived: bool,
    now_ms: i64,
) -> Result<(), String> {
    let cwd = store.session_cwd(session_id).map_err(|e| e.to_string())?;
    if !archived && super::session_query::cwd_is_missing(cwd.as_deref()) {
        return Err("session/dir_missing".into());
    }
    store
        .set_session_archived(session_id, archived, now_ms)
        .map_err(|error| error.to_string())?;
    if archived {
        let origin = cwd
            .as_deref()
            .and_then(|c| crate::wtnew::origin::origin_root_of(std::path::Path::new(c)));
        if let Some(origin) = origin {
            let origin = origin.to_string_lossy().replace('\\', "/");
            if let Err(error) = store.set_session_origin_root(session_id, &origin) {
                eprintln!("记录会话 {session_id} 的原项目失败: {error}");
            }
        }
    }
    Ok(())
}

/// 对话页改选启动选项后的落库（合并单键写回）：模型/权限是**启动参数**，resume/接管
/// 时回放（`terminal::splice_stored_launch_args`）——不落库的话每次重启都回默认档
/// （用户实拍：会话里切到 1M 上下文档，重启后回落 200K）。
#[tauri::command]
pub(crate) async fn set_session_launch_selection(
    session_id: i64,
    option: String,
    choice: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = super::open_store(&super::db_path()).map_err(|e| e.to_string())?;
        let mut selections: std::collections::HashMap<String, String> = store
            .session_launch_args(session_id)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        selections.insert(option, choice);
        let json = serde_json::to_string(&selections).map_err(|e| e.to_string())?;
        store
            .set_session_launch_args(session_id, &json)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 对话窗权限下拉的回显：读会话存的启动选项（sessions.launch_args，新建/接管改选时落库，
/// 见 `terminal::splice_stored_launch_args`）。有存值时前端把「沿用原设置」直接亮成那一档——
/// 一句黑盒不如具体档位（用户实拍反馈）。读库/解析失败返回空 map：回显是增强，不值得报错。
#[tauri::command]
pub(crate) async fn session_launch_selections(
    session_id: i64,
) -> Result<std::collections::HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(super::open_store(&super::db_path())
            .ok()
            .and_then(|store| store.session_launch_args(session_id).ok().flatten())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{apply_archived, is_safe_id};

    /// 目录已删的会话不能放回进行中（修复前会照常放回去，点开只有一个注定失败的「恢复会话」）；
    /// 目录还在的照常放回。标记完成不受目录影响。
    #[test]
    fn unarchive_is_refused_once_the_folder_is_gone() {
        let base = std::env::temp_dir().join(format!(
            "meowo-unarchive-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let here = base.join("here");
        std::fs::create_dir_all(&here).unwrap();
        let gone = base.join("gone");
        let store = meowo_store::Store::open_in_memory().unwrap();
        let project = store.upsert_project_by_root("/p", "p", 1).unwrap();
        let (a, _) = store.start_session(project, "a", 1).unwrap();
        let (b, _) = store.start_session(project, "b", 2).unwrap();
        store.set_session_cwd(a, &gone.to_string_lossy(), 3).unwrap();
        store.set_session_cwd(b, &here.to_string_lossy(), 3).unwrap();
        apply_archived(&store, a, true, 4).expect("目录没了照样能标记完成");
        apply_archived(&store, b, true, 4).unwrap();
        assert_eq!(apply_archived(&store, a, false, 5), Err("session/dir_missing".into()));
        assert!(store.session_header(a).unwrap().archived, "拒绝时不动标记");
        apply_archived(&store, b, false, 5).expect("目录还在的照常放回进行中");
        assert!(!store.session_header(b).unwrap().archived);
        let _ = std::fs::remove_dir_all(&base);
    }

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
