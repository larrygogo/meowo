use cc_store::Store;

#[test]
fn open_in_memory_creates_tables() {
    let store = Store::open_in_memory().expect("open");
    let count: i64 = store
        .raw_table_count()
        .expect("count tables");
    assert_eq!(count, 5);
}

// == Task 4 ==
use cc_store::{Project, Task};

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
