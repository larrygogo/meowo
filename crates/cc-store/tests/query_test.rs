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
fn overview_keeps_per_project_counts_separate() {
    // 合并为全局聚合查询后，最易出错的是跨项目串数据——本测试锁住按项目分组的正确性。
    let store = Store::open_in_memory().unwrap();
    let p1 = store.upsert_project_by_root("/p1", "p1", 100).unwrap();
    let p2 = store.upsert_project_by_root("/p2", "p2", 100).unwrap();

    // p1：一个活跃会话（doing），最近活动 250
    let (s1, _) = store.start_session(p1, "s1", 200).unwrap();
    store.on_user_prompt(s1, "p1 任务", 210).unwrap();
    store.sync_todos(s1, &[TodoInput { content: "a".into(), status: TodoStatus::InProgress }], 250).unwrap();

    // p2：两个会话，其一已结束（done），最近活动 500
    let (s2, _) = store.start_session(p2, "s2", 300).unwrap();
    store.on_user_prompt(s2, "p2 任务一", 310).unwrap();
    let (s3, _) = store.start_session(p2, "s3", 400).unwrap();
    store.on_user_prompt(s3, "p2 任务二", 410).unwrap();
    store.sync_todos(s3, &[TodoInput { content: "b".into(), status: TodoStatus::Completed }], 500).unwrap();
    store.end_session(s3, 500).unwrap();

    let ov = store.overview().unwrap();
    assert_eq!(ov.len(), 2);
    // 按 last_activity_at 倒序：p2(500) 在前，p1(250) 在后
    assert_eq!(ov[0].project.name, "p2");
    assert_eq!(ov[0].active_sessions, 1); // s2 running，s3 ended
    assert_eq!(ov[0].doing_count, 0);
    assert_eq!(ov[0].done_count, 1);
    assert_eq!(ov[0].todo_count, 1); // p2 任务一（有 prompt 无 todo → todo 列）
    assert_eq!(ov[0].last_activity_at, 500);

    assert_eq!(ov[1].project.name, "p1");
    assert_eq!(ov[1].active_sessions, 1);
    assert_eq!(ov[1].doing_count, 1);
    assert_eq!(ov[1].done_count, 0);
    assert_eq!(ov[1].todo_count, 0);
    assert_eq!(ov[1].last_activity_at, 250);
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

// ===== Task 1: live_sessions =====

use cc_store::SessionStatus;

#[test]
fn live_sessions_includes_ended_sessions() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();

    let (s1, _) = store.start_session(pid, "r", 100).unwrap();
    store.on_user_prompt(s1, "活的", 110).unwrap();
    let (s2, _) = store.start_session(pid, "w", 200).unwrap();
    store.set_session_status(s2, SessionStatus::Waiting, 210).unwrap();
    let (s3, _) = store.start_session(pid, "st", 300).unwrap();
    store.set_session_status(s3, SessionStatus::Stale, 310).unwrap();
    let (s4, _) = store.start_session(pid, "e", 400).unwrap();
    store.end_session(s4, 410).unwrap();

    let live = store.live_sessions().unwrap();
    // 四个都在（ended 也保留）
    assert_eq!(live.len(), 4);
    let statuses: Vec<&str> = live.iter().map(|l| l.session.status.as_str()).collect();
    assert!(statuses.contains(&"running"));
    assert!(statuses.contains(&"waiting"));
    assert!(statuses.contains(&"stale"));
    assert!(statuses.contains(&"ended"));
}

#[test]
fn live_session_carries_project_name_title_and_progress() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "proj", 100).unwrap();
    let (s1, _t1) = store.start_session(pid, "r", 100).unwrap();
    store.on_user_prompt(s1, "实现登录", 110).unwrap();
    store.sync_todos(s1, &[
        cc_store::TodoInput { content: "a".into(), status: cc_store::TodoStatus::Completed },
        cc_store::TodoInput { content: "b".into(), status: cc_store::TodoStatus::InProgress },
    ], 120).unwrap();

    let live = store.live_sessions().unwrap();
    assert_eq!(live.len(), 1);
    let l = &live[0];
    assert_eq!(l.project_name, "proj");
    assert_eq!(l.task_title, "实现登录");
    assert_eq!(l.column, "doing");
    assert_eq!(l.todo_total, 2);
    assert_eq!(l.todo_done, 1);
    assert_eq!(l.todos.len(), 2);
    assert_eq!(l.todos[0].content, "a");
}

#[test]
fn session_note_upsert_delete_and_surfaces_in_live() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (s1, _) = store.start_session(pid, "sess-a", 100).unwrap();
    store.on_user_prompt(s1, "标题", 110).unwrap();

    // 初始无便签
    assert_eq!(store.live_sessions().unwrap()[0].note, None);

    // 写入便签 → live_sessions 带出（前后空白被 trim）
    store.set_session_note("sess-a", "  记得 review  ", 120).unwrap();
    assert_eq!(store.live_sessions().unwrap()[0].note.as_deref(), Some("记得 review"));

    // upsert 覆盖旧便签
    store.set_session_note("sess-a", "改主意了", 130).unwrap();
    assert_eq!(store.live_sessions().unwrap()[0].note.as_deref(), Some("改主意了"));

    // 清空（trim 后为空）→ 删除该行，回到 None
    store.set_session_note("sess-a", "   ", 140).unwrap();
    assert_eq!(store.live_sessions().unwrap()[0].note, None);
}

// ===== Task 2: 过滤未命名空卡 =====

#[test]
fn project_tasks_hides_unnamed_empty_placeholder() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (s1, _) = store.start_session(pid, "s1", 100).unwrap();
    store.on_user_prompt(s1, "真任务", 110).unwrap();
    // s2 从没发 prompt、无 todo -> 未命名空卡，应被隐藏
    let (_s2, _) = store.start_session(pid, "s2", 200).unwrap();

    let cards = store.project_tasks(pid).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].task.title, "真任务");
}

#[test]
fn overview_counts_exclude_unnamed_empty_placeholder() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    // 真任务（有 prompt，无 todo -> todo 列）
    let (s1, _) = store.start_session(pid, "s1", 100).unwrap();
    store.on_user_prompt(s1, "真任务", 110).unwrap();
    // 未命名空卡（应不计入）
    let (_s2, _) = store.start_session(pid, "s2", 200).unwrap();

    let o = &store.overview().unwrap()[0];
    assert_eq!(o.todo_count, 1); // 只数真任务，不数未命名空卡
}
