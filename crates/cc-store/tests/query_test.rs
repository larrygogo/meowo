use cc_store::{Store, TodoInput, TodoStatus};

#[test]
fn overview_aggregates_counts_and_active_sessions() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();

    let (s1, _t1) = store.start_session(pid, "s1", 200).unwrap();
    store.on_user_prompt(s1, "任务一", 210).unwrap();
    store.sync_todos(s1, &[TodoInput { content: "a".into(), status: TodoStatus::InProgress }], 220).unwrap();

    let (s2, _t2) = store.start_session(pid, "s2", 300).unwrap();
    store.on_user_prompt(s2, "任务二", 310).unwrap();
    store.sync_todos(s2, &[TodoInput { content: "b".into(), status: TodoStatus::Completed }], 320).unwrap();
    store.end_session(s2, 330).unwrap();

    let ov = store.overview().unwrap();
    assert_eq!(ov.len(), 1);
    let o = &ov[0];
    assert_eq!(o.project.name, "p");
    assert_eq!(o.active_sessions, 1);
    assert_eq!(o.doing_count, 1);
    assert_eq!(o.done_count, 1);
    assert_eq!(o.todo_count, 0);
    assert_eq!(o.last_activity_at, 330);
}

#[test]
fn overview_empty_when_no_projects() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.overview().unwrap().len(), 0);
}

#[test]
fn project_tasks_returns_cards_with_todos_and_session_status() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (s1, t1) = store.start_session(pid, "s1", 200).unwrap();
    store.on_user_prompt(s1, "卡一", 210).unwrap();
    store.sync_todos(s1, &[
        cc_store::TodoInput { content: "x".into(), status: cc_store::TodoStatus::InProgress },
        cc_store::TodoInput { content: "y".into(), status: cc_store::TodoStatus::Pending },
    ], 220).unwrap();

    let cards = store.project_tasks(pid).unwrap();
    assert_eq!(cards.len(), 1);
    let c = &cards[0];
    assert_eq!(c.task.id, t1);
    assert_eq!(c.task.title, "卡一");
    assert_eq!(c.task.column, "doing");
    assert_eq!(c.todos.len(), 2);
    assert_eq!(c.todos[0].content, "x");
    assert_eq!(c.session_status.as_deref(), Some("running"));
}

#[test]
fn project_tasks_empty_for_unknown_project() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.project_tasks(999).unwrap().len(), 0);
}
