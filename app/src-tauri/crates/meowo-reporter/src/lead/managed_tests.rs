use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn parallel_workers_in_one_host_keep_every_task_registration() {
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("lead-sessions.json");
    let barrier = Arc::new(Barrier::new(24));
    let workers: Vec<_> = (0..24)
        .map(|index| {
            let cwd = root.path().to_path_buf();
            let registry = registry.clone();
            let barrier = barrier.clone();
            let task = root.path().join(format!(".lead/task-{index}"));
            std::fs::create_dir_all(&task).unwrap();
            std::fs::write(task.join("progress.md"), "phase: implement").unwrap();
            std::thread::spawn(move || {
                let sid = format!("root-{index}");
                let call = json!({"name":"Write","input":{"file_path":task.join("progress.md")}});
                barrier.wait();
                assert!(managed_claim(
                    &cwd.to_string_lossy(),
                    &sid,
                    &call,
                    &registry
                ));
                (sid, task)
            })
        })
        .collect();
    let claimed: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let map = load_map(&registry);
    assert_eq!(map.len(), 24);
    for (sid, task) in claimed {
        assert_eq!(owner_of(&task), sid);
        assert_eq!(task_of_session(&sid, &registry), Some(task));
    }
}

#[test]
fn registered_worktree_context_cannot_fall_back_to_another_active_task() {
    let root = tempfile::tempdir().unwrap();
    let owned = root.path().join("foreign-project/.lead/owned");
    let cwd = root.path().join("current-project");
    let other = cwd.join(".lead/other");
    for task in [&owned, &other] {
        std::fs::create_dir_all(task).unwrap();
        std::fs::write(task.join("progress.md"), "phase: implement").unwrap();
        std::fs::write(task.join("acceptance.json"), "[]").unwrap();
    }
    std::fs::write(owned.join("SPEC.md"), "registered own worktree").unwrap();
    std::fs::write(other.join("SPEC.md"), "someone else's task").unwrap();
    std::fs::write(cwd.join(".lead/active"), "other").unwrap();
    let registry = root.path().join("lead-sessions.json");
    assert!(managed_claim(
        &cwd.to_string_lossy(),
        "root-one",
        &json!({"name":"Write","input":{"file_path":owned.join("progress.md")}}),
        &registry
    ));
    let context = managed_context(&cwd.to_string_lossy(), "root-one", &registry).unwrap();
    assert!(context.contains("registered own worktree"));
    assert!(!context.contains("someone else's task"));
}
