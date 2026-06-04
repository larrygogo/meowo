use cc_store::{Project, Session, SessionStatus, Store, Task, TaskColumn, Todo, TodoInput, TodoStatus};

#[test]
fn open_in_memory_creates_tables() {
    let store = Store::open_in_memory().expect("open");
    let count: i64 = store
        .raw_table_count()
        .expect("count tables");
    assert_eq!(count, 5);
}

// == Task 4 ==
#[test]
fn upsert_project_is_idempotent_by_root() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.upsert_project_by_root("/home/me/proj", "proj", 1000).unwrap();
    let id2 = store.upsert_project_by_root("/home/me/proj", "proj", 2000).unwrap();
    assert_eq!(id1, id2);

    let projects: Vec<Project> = store.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "proj");
    assert_eq!(projects[0].updated_at, 2000);
}

// == Task 5 ==
#[test]
fn start_session_creates_session_and_placeholder_task() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-abc", 200).unwrap();
    assert!(sid > 0 && tid > 0);

    let (sid2, tid2) = store.start_session(pid, "cc-abc", 300).unwrap();
    assert_eq!(sid, sid2);
    assert_eq!(tid, tid2);

    let task: Task = store.get_task(tid).unwrap();
    assert_eq!(task.title, "(未命名会话)");
    assert_eq!(task.column, "todo");
    assert_eq!(task.session_id, Some(sid));
}

// == Task 6 ==
#[test]
fn first_prompt_sets_title_then_later_prompts_only_update_activity() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-1", 200).unwrap();

    store.on_user_prompt(sid, "实现登录功能并写测试", 300).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "实现登录功能并写测试");
    assert_eq!(t.current_activity.as_deref(), Some("实现登录功能并写测试"));

    store.on_user_prompt(sid, "再加个登出按钮", 400).unwrap();
    let t2 = store.get_task(tid).unwrap();
    assert_eq!(t2.title, "实现登录功能并写测试");
    assert_eq!(t2.current_activity.as_deref(), Some("再加个登出按钮"));
}

#[test]
fn long_prompt_title_is_truncated_to_60_chars() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-2", 200).unwrap();
    let long = "字".repeat(80);
    store.on_user_prompt(sid, &long, 300).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title.chars().count(), 60);
}

// == Task 7 ==
#[test]
fn sync_todos_replaces_list_and_derives_column() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-1", 200).unwrap();

    store.sync_todos(sid, &[
        TodoInput { content: "解析".into(), status: TodoStatus::Completed },
        TodoInput { content: "建图".into(), status: TodoStatus::InProgress },
        TodoInput { content: "测试".into(), status: TodoStatus::Pending },
    ], 300).unwrap();

    let todos: Vec<Todo> = store.list_todos(tid).unwrap();
    assert_eq!(todos.len(), 3);
    assert_eq!(todos[0].content, "解析");
    assert_eq!(store.get_task(tid).unwrap().column, "doing");

    store.sync_todos(sid, &[
        TodoInput { content: "解析".into(), status: TodoStatus::Completed },
        TodoInput { content: "建图".into(), status: TodoStatus::Completed },
    ], 400).unwrap();
    assert_eq!(store.list_todos(tid).unwrap().len(), 2);
    assert_eq!(store.get_task(tid).unwrap().column, "done");
}

#[test]
fn sync_todos_does_not_override_locked_column() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-1", 200).unwrap();
    store.set_task_column(tid, TaskColumn::Done, true, 250).unwrap();

    store.sync_todos(sid, &[
        TodoInput { content: "x".into(), status: TodoStatus::InProgress },
    ], 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().column, "done");
}

// == Task 8 ==
#[test]
fn stop_sets_waiting_and_end_sets_ended() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _tid) = store.start_session(pid, "cc-1", 200).unwrap();

    store.set_session_status(sid, SessionStatus::Waiting, 300).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "waiting");

    store.end_session(sid, 400).unwrap();
    let s: Session = store.get_session(sid).unwrap();
    assert_eq!(s.status, "ended");
    assert_eq!(s.ended_at, Some(400));
}

#[test]
fn mark_stale_flags_idle_running_sessions() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid_old, _) = store.start_session(pid, "old", 1000).unwrap();
    let (sid_new, _) = store.start_session(pid, "new", 1000).unwrap();
    store.touch_session(sid_new, 9000).unwrap();

    let n = store.mark_stale(2000, 10000).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.get_session(sid_old).unwrap().status, "stale");
    assert_eq!(store.get_session(sid_new).unwrap().status, "running");
}

#[test]
fn empty_todos_resets_column_to_todo() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-e", 200).unwrap();
    // 先 doing
    store.sync_todos(sid, &[cc_store::TodoInput { content: "x".into(), status: cc_store::TodoStatus::InProgress }], 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().column, "doing");
    // 清空 -> 回 todo
    store.sync_todos(sid, &[], 400).unwrap();
    assert_eq!(store.get_task(tid).unwrap().column, "todo");
    assert_eq!(store.list_todos(tid).unwrap().len(), 0);
}

#[test]
fn all_pending_todos_is_todo_column() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-p", 200).unwrap();
    store.sync_todos(sid, &[
        cc_store::TodoInput { content: "a".into(), status: cc_store::TodoStatus::Pending },
        cc_store::TodoInput { content: "b".into(), status: cc_store::TodoStatus::Pending },
    ], 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().column, "todo");
}

#[test]
fn touch_session_revives_waiting_to_running() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _tid) = store.start_session(pid, "cc-r", 200).unwrap();
    store.set_session_status(sid, cc_store::SessionStatus::Waiting, 300).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "waiting");
    store.touch_session(sid, 400).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "running");
}

#[test]
fn set_current_activity_updates_task() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-a", 200).unwrap();
    store.set_current_activity(sid, "› cargo test", 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().current_activity.as_deref(), Some("› cargo test"));
}

#[test]
fn prompt_with_image_marker_is_cleaned_for_title() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-img", 200).unwrap();
    store.on_user_prompt(sid, "[Image #4] 把路径放在最前面", 300).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "把路径放在最前面");
    assert_eq!(t.current_activity.as_deref(), Some("把路径放在最前面"));
}

#[test]
fn multiple_image_markers_and_whitespace_collapsed() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-img2", 200).unwrap();
    store.on_user_prompt(sid, "[Image #1]  改这个   [Image #2] 和那个 ", 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().title, "改这个 和那个");
}

#[test]
fn image_only_prompt_keeps_placeholder_title() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-img3", 200).unwrap();
    store.on_user_prompt(sid, "[Image #1]", 300).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "(未命名会话)");
    assert_eq!(t.current_activity, None);
}

// == set_session_title ==
#[test]
fn set_session_title_overrides_placeholder_and_prompt_title() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "s", 100).unwrap();
    store.on_user_prompt(sid, "继续", 110).unwrap(); // 首条填充词当了标题
    assert_eq!(store.get_task(tid).unwrap().title, "继续");
    store.set_session_title(sid, "Claude Code 看板", 120).unwrap();
    assert_eq!(store.get_task(tid).unwrap().title, "Claude Code 看板");
}

// == PID 存活检测 ==
#[test]
fn set_pid_and_liveness_query() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 4242, 110).unwrap();
    let live = store.live_session_liveness().unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].0, sid);
    assert_eq!(live[0].1, Some(4242));
}

#[test]
fn ended_session_not_in_liveness() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s2", 100).unwrap();
    store.set_session_pid(sid, 9999, 110).unwrap();
    store.end_session(sid, 200).unwrap();
    let live = store.live_session_liveness().unwrap();
    assert!(live.is_empty());
}

// == 审计修复测试 ==

#[test]
fn session_start_revives_ended_session() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _t) = store.start_session(pid, "s", 100).unwrap();
    store.end_session(sid, 200).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "ended");
    // resume：同 session_id 再次 SessionStart 应复活为 running 且清空 ended_at
    let (sid2, _t2) = store.start_session(pid, "s", 300).unwrap();
    assert_eq!(sid2, sid);
    let s = store.get_session(sid).unwrap();
    assert_eq!(s.status, "running");
    assert_eq!(s.ended_at, None);
}

#[test]
fn mark_stale_also_flags_idle_waiting() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "w", 1000).unwrap();
    store.set_session_status(sid, cc_store::SessionStatus::Waiting, 1000).unwrap();
    let n = store.mark_stale(2000, 10000).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.get_session(sid).unwrap().status, "stale");
}
