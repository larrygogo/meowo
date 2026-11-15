//! 对话页按回合读取与恢复 Git 快照；危险写操作只收会话/回合，路径由快照元数据反查。

use meowo_protocol::ipc::{CheckpointRepoDto, CheckpointRestoreDto, CheckpointRestoreResultDto, CheckpointTurnDto, GitFileDiffDto};
use meowo_store::{RestoreCheckpoint, Store, TurnCheckpoint};
use std::collections::BTreeMap;
use std::path::Path;

fn store(state: &tauri::State<'_, crate::AppState>) -> Result<Store, String> {
    crate::open_store(&state.db_path).map_err(|e| e.to_string())
}

fn repo(row: &TurnCheckpoint, detail: bool) -> CheckpointRepoDto {
    let files = if detail && row.end_ref.is_some() && row.error.is_none() {
        meowo_reporter::checkpoint::changed_files(row)
            .map(|files| files.into_iter().map(|file| meowo_protocol::ipc::GitChangedFileDto {
                path: file.path,
                status: file.status,
                additions: file.additions,
                deletions: file.deletions,
                index_status: None,
                worktree_status: None,
                untracked_files: None,
            }).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    CheckpointRepoDto {
        root: row.repo_root.clone(),
        name: Path::new(&row.repo_root).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| row.repo_root.clone()),
        start_ref: row.start_ref.clone(),
        end_ref: row.end_ref.clone(),
        files,
        error: row.error.clone(),
    }
}

fn grouped(rows: Vec<TurnCheckpoint>, detail: bool) -> Vec<CheckpointTurnDto> {
    let mut turns: BTreeMap<i64, Vec<CheckpointRepoDto>> = BTreeMap::new();
    for row in rows {
        turns.entry(row.turn).or_default().push(repo(&row, detail));
    }
    turns.into_iter().rev().map(|(turn, repos)| CheckpointTurnDto { turn, repos }).collect()
}

#[tauri::command]
pub(crate) fn checkpoint_turns(state: tauri::State<'_, crate::AppState>, session_id: i64) -> Result<Vec<CheckpointTurnDto>, String> {
    Ok(grouped(store(&state)?.turn_checkpoints(session_id).map_err(|e| e.to_string())?, false))
}

#[tauri::command]
pub(crate) async fn checkpoint_turn_detail(state: tauri::State<'_, crate::AppState>, session_id: i64, turn: i64) -> Result<Option<CheckpointTurnDto>, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db).map_err(|e| e.to_string())?;
        let rows = store.turn_checkpoint_repos(session_id, turn).map_err(|e| e.to_string())?;
        Ok(grouped(rows, true).into_iter().next())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn checkpoint_file_diff(state: tauri::State<'_, crate::AppState>, session_id: i64, turn: i64, repo_root: String, path: String) -> Result<GitFileDiffDto, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db).map_err(|e| e.to_string())?;
        let rows = store.turn_checkpoint_repos(session_id, turn).map_err(|e| e.to_string())?;
        let row = rows.iter().find(|row| row.repo_root == repo_root).ok_or("checkpoint/repo_not_in_turn")?;
        let patch = meowo_reporter::checkpoint::file_diff(row, &path)?;
        let status = meowo_reporter::checkpoint::changed_files(row)?.into_iter().find(|file| file.path == path).map(|file| file.status).unwrap_or_else(|| "M".into());
        Ok(GitFileDiffDto { path, status, diff: patch.diff, truncated: patch.truncated })
    }).await.map_err(|e| e.to_string())?
}

fn ensure_restorable(store: &Store, session_id: i64) -> Result<(), String> {
    let session = store.get_session(session_id).map_err(|e| e.to_string())?;
    if session.status == "running" {
        return Err("checkpoint/session_running".into());
    }
    Ok(())
}

fn restore_targets(store: &Store, session_id: i64, targets: Vec<(String, String)>, turn: i64) -> Result<CheckpointRestoreDto, String> {
    ensure_restorable(store, session_id)?;
    if targets.is_empty() {
        return Err("checkpoint/not_ready".into());
    }
    let now = crate::now_ms();
    let mut snapshots = Vec::new();
    let mut results = Vec::new();
    for (index, (root, _)) in targets.iter().enumerate() {
        let path = Path::new(root);
        let actual = meowo_reporter::checkpoint::repo_root(path)?.filter(|actual| crate::git::same_root(&actual.to_string_lossy(), root));
        if actual.is_none() {
            results.push(CheckpointRestoreResultDto { root: root.clone(), restored: false, error: Some("checkpoint/repo_changed".into()) });
            continue;
        }
        match meowo_reporter::checkpoint::capture(path, session_id, turn, &format!("restore-{now}-{index}")) {
            Ok(snapshot_ref) => snapshots.push(RestoreCheckpoint { repo_root: root.clone(), snapshot_ref }),
            Err(error) => results.push(CheckpointRestoreResultDto { root: root.clone(), restored: false, error: Some(error) }),
        }
    }
    if snapshots.is_empty() {
        return Ok(CheckpointRestoreDto { undo_id: 0, results });
    }
    let undo_id = store.create_checkpoint_restore(session_id, &snapshots, now).map_err(|e| e.to_string())?;
    for (root, target) in targets {
        let Some(_) = snapshots.iter().find(|item| item.repo_root == root) else {
            continue;
        };
        let result = meowo_reporter::checkpoint::restore(Path::new(&root), &target);
        results.push(CheckpointRestoreResultDto { root: root.clone(), restored: result.is_ok(), error: result.err() });
    }
    Ok(CheckpointRestoreDto { undo_id, results })
}

#[tauri::command]
pub(crate) async fn checkpoint_restore(state: tauri::State<'_, crate::AppState>, session_id: i64, turn: i64, phase: String) -> Result<CheckpointRestoreDto, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db).map_err(|e| e.to_string())?;
        let rows = store.turn_checkpoint_repos(session_id, turn).map_err(|e| e.to_string())?;
        let targets = rows.into_iter().filter_map(|row| {
            let reference = if phase == "start" { Some(row.start_ref) } else if phase == "end" { row.end_ref } else { None };
            reference.filter(|reference| !reference.is_empty()).map(|reference| (row.repo_root, reference))
        }).collect();
        restore_targets(&store, session_id, targets, turn)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn checkpoint_undo(state: tauri::State<'_, crate::AppState>, session_id: i64, id: i64) -> Result<CheckpointRestoreDto, String> {
    let db = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = crate::open_store(&db).map_err(|e| e.to_string())?;
        let rows = store.checkpoint_restore(session_id, id).map_err(|e| e.to_string())?.ok_or("checkpoint/restore_not_found")?;
        let targets = rows.into_iter().map(|row| (row.repo_root, row.snapshot_ref)).collect();
        restore_targets(&store, session_id, targets, 0)
    }).await.map_err(|e| e.to_string())?
}


#[cfg(test)]
mod tests {
    use super::*;
    use meowo_store::SessionStatus;

    fn git(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git").current_dir(repo).args(args).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    fn repo(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "user.email", "test@localhost"]);
        git(root, &["config", "core.autocrlf", "false"]);
        std::fs::write(root.join("a.txt"), "before\n").unwrap();
        git(root, &["add", "a.txt"]);
        git(root, &["commit", "-qm", "base"]);
    }

    fn session(store: &Store, status: SessionStatus) -> i64 {
        let project = store.upsert_project_by_root("C:/work", "work", 1).unwrap();
        let (sid, _) = store.start_session(project, "checkpoint-service", 1).unwrap();
        store.set_session_status(sid, status, 2).unwrap();
        sid
    }

    #[test]
    fn running_session_is_rejected_before_any_file_write() {
        let store = Store::open_in_memory().unwrap();
        let sid = session(&store, SessionStatus::Running);
        assert_eq!(ensure_restorable(&store, sid).unwrap_err(), "checkpoint/session_running");
    }

    #[test]
    fn multi_repo_partial_failure_is_reported_and_successful_part_can_undo() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let temp = std::env::temp_dir().join(format!("meowo-restore-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let a = temp.join("a");
        let b = temp.join("b");
        repo(&a);
        repo(&b);
        let store = Store::open_in_memory().unwrap();
        let sid = session(&store, SessionStatus::Waiting);
        for root in [&a, &b] {
            let root_text = root.to_string_lossy().replace('\\', "/");
            let start = meowo_reporter::checkpoint::capture(root, sid, 1, "start").unwrap();
            store.checkpoint_start(sid, 1, &root_text, &start, 10).unwrap();
            std::fs::write(root.join("a.txt"), "agent\n").unwrap();
            let end = meowo_reporter::checkpoint::capture(root, sid, 1, "end").unwrap();
            store.checkpoint_finish(sid, 1, &root_text, &end, 20).unwrap();
            std::fs::write(root.join("a.txt"), "later\n").unwrap();
        }
        std::fs::remove_dir_all(&b).unwrap();
        let targets = store.turn_checkpoint_repos(sid, 1).unwrap().into_iter().map(|row| (row.repo_root, row.start_ref)).collect();
        let result = restore_targets(&store, sid, targets, 1).unwrap();
        assert_eq!(result.results.iter().filter(|item| item.restored).count(), 1);
        assert_eq!(result.results.iter().find(|item| !item.restored).unwrap().error.as_deref(), Some("checkpoint/repo_changed"));
        assert_eq!(std::fs::read_to_string(a.join("a.txt")).unwrap(), "before\n");
        let undo = store.checkpoint_restore(sid, result.undo_id).unwrap().unwrap();
        let targets = undo.into_iter().map(|row| (row.repo_root, row.snapshot_ref)).collect();
        restore_targets(&store, sid, targets, 0).unwrap();
        assert_eq!(std::fs::read_to_string(a.join("a.txt")).unwrap(), "later\n");
    }
}
