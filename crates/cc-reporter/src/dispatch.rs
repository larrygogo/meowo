use cc_store::{SessionStatus, Store, StoreError};
use crate::hook::HookEvent;
use std::path::Path;

/// 把一个 hook 事件落到库。未知/缺字段一律降级为「无操作」，绝不报错冒泡。
pub fn dispatch(store: &Store, ev: &HookEvent, now_ms: i64) -> Result<(), StoreError> {
    match ev.hook_event_name.as_str() {
        "SessionStart" => {
            let Some(cwd) = ev.cwd.as_deref() else { return Ok(()) };
            if ev.session_id.is_empty() { return Ok(()); }
            let (root, name) = project_root_and_name(cwd);
            let pid = store.upsert_project_by_root(&root, &name, now_ms)?;
            store.start_session(pid, &ev.session_id, now_ms)?;
        }
        "UserPromptSubmit" => {
            if let Some(sid) = lookup_session(store, ev)? {
                if let Some(prompt) = ev.prompt.as_deref() {
                    store.on_user_prompt(sid, prompt, now_ms)?;
                }
            }
        }
        "PostToolUse" => {
            if let Some(sid) = lookup_session(store, ev)? {
                match ev.tool_name.as_deref() {
                    Some("TodoWrite") => {
                        store.sync_todos(sid, &ev.todo_items(), now_ms)?;
                    }
                    Some("Bash") => {
                        if let Some(cmd) = ev.bash_command() {
                            store.set_current_activity(sid, &format!("› {cmd}"), now_ms)?;
                        }
                    }
                    _ => { store.touch_session(sid, now_ms)?; }
                }
            }
        }
        "Stop" => {
            if let Some(sid) = lookup_session(store, ev)? {
                store.set_session_status(sid, SessionStatus::Waiting, now_ms)?;
            }
        }
        "SessionEnd" => {
            if let Some(sid) = lookup_session(store, ev)? {
                store.end_session(sid, now_ms)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn lookup_session(store: &Store, ev: &HookEvent) -> Result<Option<i64>, StoreError> {
    if ev.session_id.is_empty() {
        return Ok(None);
    }
    store.find_session_id_pub(&ev.session_id)
}

/// cwd 的 git 根（向上找 .git）作为项目 root；无 git 则用 cwd 本身。name = 末段目录名。
fn project_root_and_name(cwd: &str) -> (String, String) {
    let root = find_git_root(cwd).unwrap_or_else(|| cwd.to_string());
    let name = Path::new(&root)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&root)
        .to_string();
    (root, name)
}

fn find_git_root(start: &str) -> Option<String> {
    let mut dir = Path::new(start);
    loop {
        if dir.join(".git").exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        dir = dir.parent()?;
    }
}
