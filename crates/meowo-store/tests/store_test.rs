use meowo_store::{PendingReview, Project, Session, SessionStatus, Store, Task, TaskColumn, Todo, TodoInput, TodoStatus};

#[test]
fn open_in_memory_creates_tables() {
    let store = Store::open_in_memory().expect("open");
    let count: i64 = store
        .raw_table_count()
        .expect("count tables");
    // projects / sessions / tasks / todos / events / session_context / session_notes
    assert_eq!(count, 7);
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

#[test]
fn upsert_project_updates_name_on_conflict() {
    let store = Store::open_in_memory().unwrap();
    let id1 = store.upsert_project_by_root("/r", "old-name", 100).unwrap();
    let id2 = store.upsert_project_by_root("/r", "owner/repo", 200).unwrap();
    assert_eq!(id1, id2);
    assert_eq!(store.list_projects().unwrap()[0].name, "owner/repo");
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
fn first_prompt_sets_title_then_later_prompts_keep_title() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-1", 200).unwrap();

    store.on_user_prompt(sid, "实现登录功能并写测试", 300).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "实现登录功能并写测试");

    store.on_user_prompt(sid, "再加个登出按钮", 400).unwrap();
    let t2 = store.get_task(tid).unwrap();
    assert_eq!(t2.title, "实现登录功能并写测试");
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
fn empty_todos_resets_column_to_todo() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, tid) = store.start_session(pid, "cc-e", 200).unwrap();
    // 先 doing
    store.sync_todos(sid, &[meowo_store::TodoInput { content: "x".into(), status: meowo_store::TodoStatus::InProgress }], 300).unwrap();
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
        meowo_store::TodoInput { content: "a".into(), status: meowo_store::TodoStatus::Pending },
        meowo_store::TodoInput { content: "b".into(), status: meowo_store::TodoStatus::Pending },
    ], 300).unwrap();
    assert_eq!(store.get_task(tid).unwrap().column, "todo");
}

#[test]
fn touch_session_revives_waiting_to_running() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _tid) = store.start_session(pid, "cc-r", 200).unwrap();
    store.set_session_status(sid, meowo_store::SessionStatus::Waiting, 300).unwrap();
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

// == session cwd ==
#[test]
fn set_and_get_session_cwd() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    assert_eq!(store.session_cwd(sid).unwrap(), None);
    store.set_session_cwd(sid, "C:\\proj", 110).unwrap();
    assert_eq!(store.session_cwd(sid).unwrap().as_deref(), Some("C:\\proj"));
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

// == live_sessions pid + end_orphaned_idle ==

#[test]
fn live_sessions_carries_pid() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 1234, 110).unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].pid, Some(1234));
}

#[test]
fn set_pid_evicts_same_pid_from_other_sessions() {
    // /clear 等会在同一进程上开新会话：新会话认领 pid 后，旧会话的 pid 应被摘除，
    // 否则旧会话会因进程仍存活而一直误显示「已连接」。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (old, _) = store.start_session(pid, "old", 100).unwrap();
    store.set_session_pid(old, 7777, 110).unwrap();
    let (new, _) = store.start_session(pid, "new", 200).unwrap();
    store.set_session_pid(new, 7777, 210).unwrap(); // 同一进程认领新会话

    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let of = |cc: &str| live.iter().find(|s| s.session.cc_session_id == cc).unwrap();
    // 旧会话被收尾：pid 摘除 + 状态 ended → 不再误判已连接、状态也收尾。
    assert_eq!(of("old").pid, None);
    assert_eq!(of("old").session.status, "ended");
    // 新会话持有 pid，状态仍 live。
    assert_eq!(of("new").pid, Some(7777));
    assert_ne!(of("new").session.status, "ended");
}

#[test]
fn revive_for_resume_revives_ended_and_clears_pid() {
    // 看板 resume 一个已断开会话：应复活(脱离 ended)并清空 pid——旧进程已死，清 pid 让 reaper 不臆测收尾，
    // 卡片即刻显示已连接，新进程首个 hook 再认领 pid。覆盖 codex「session_start 要到首个 turn 才触发」场景。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 5555, 110).unwrap();
    store.end_session(sid, 200).unwrap(); // 断开
    assert!(store.revive_for_resume(sid, 300, None).unwrap()); // 真的复活了
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].session.cc_session_id, "s");
    assert_ne!(live[0].session.status, "ended"); // 已复活
    assert_eq!(live[0].pid, None); // pid 已清
}

#[test]
fn revive_for_resume_noop_on_connected_session() {
    // hook 已认领 pid 的活跃会话(非 ended 且 pid 非空、未验证到死 pid)不命中 →
    // pid 原样保留且返回 false，避免误清活跃会话/误触发失败回滚。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 6666, 110).unwrap();
    assert!(!store.revive_for_resume(sid, 300, None).unwrap());
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live[0].pid, Some(6666));
}

#[test]
fn revive_for_resume_refreshes_pidless_running_session() {
    // 宽限过期后用户再次点 resume：会话 status 仍 running、pid 空(从未被 hook 认领) → 应刷新 last_event_at
    // 重启 app 侧乐观连接宽限，而不是因「非 ended」被跳过。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap(); // 默认 running、pid 空、last_event_at=100
    assert!(store.revive_for_resume(sid, 500, None).unwrap());
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live[0].pid, None);
    assert_eq!(live[0].session.last_event_at, 500); // 已刷新 → 宽限重启
}

#[test]
fn revive_for_resume_forces_when_pid_confirmed_dead() {
    // 进程刚死、reaper(5s 周期)尚未收尾的窗口内点 resume：status 仍 running 且 pid 非空，
    // 常规守卫不命中；调用方校验到该 pid 进程确已死亡后以 dead_pid=Some(旧 pid) 强制复活，
    // 否则本次 resume 静默 0 行更新、随后被 reaper 收尾成 ended，卡片长期显示未连接。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 5555, 110).unwrap(); // running + pid 非空(进程实际已死)
    assert!(store.revive_for_resume(sid, 300, Some(5555)).unwrap());
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live[0].pid, None); // 旧死 pid 已清，reaper 不再臆测收尾
    assert_eq!(live[0].session.last_event_at, 300); // 宽限期重启
    assert_ne!(live[0].session.status, "ended");
}

#[test]
fn revive_for_resume_stale_dead_pid_does_not_clear_new_live_pid() {
    // TOCTOU 守卫：调用方快照校验旧 pid(5555) 已死之后、UPDATE 之前，新进程 hook 认领了
    // 新的存活 pid(7777)——dead_pid=Some(5555) 与行内当前 pid 不等，守卫必须不命中，
    // 绝不能把刚认领的活 pid 清掉(否则 120s 宽限过期后活会话被 end_orphaned_idle 误收尾)。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    store.set_session_pid(sid, 7777, 200).unwrap(); // 新进程已认领新活 pid
    assert!(!store.revive_for_resume(sid, 300, Some(5555)).unwrap()); // 持旧快照的迟到 UPDATE
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert_eq!(live[0].pid, Some(7777)); // 新活 pid 原样保留
    assert_eq!(live[0].session.last_event_at, 200); // 宽限未被重启
}

#[test]
fn end_orphaned_idle_only_reaps_pidless_stale_sessions() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    // s1：无 pid 且空闲超阈值 → 应被收尾。
    let (s1, _) = store.start_session(pid, "orphan-idle", 1000).unwrap();
    // s2：无 pid 但最近有事件（未超阈值）→ 保留。
    let (s2, _) = store.start_session(pid, "orphan-fresh", 1000).unwrap();
    store.touch_session(s2, 9000).unwrap();
    // s3：带 pid 且空闲很久（claude 在等用户输入）→ 绝不能误杀。
    let (s3, _) = store.start_session(pid, "connected-idle", 1000).unwrap();
    store.set_session_pid(s3, 1234, 1000).unwrap();

    // now=10000, idle阈值=2000。
    let n = store.end_orphaned_idle(2000, 10000).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.get_session(s1).unwrap().status, "ended");
    assert_eq!(store.get_session(s2).unwrap().status, "running");
    assert_ne!(store.get_session(s3).unwrap().status, "ended"); // 带 pid 不受空闲超时影响
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

// == archived ==
#[test]
fn archive_flag_roundtrip_in_live_sessions() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "s", 100).unwrap();
    assert!(!store.live_sessions(None, None, None, None, 1000).unwrap()[0].archived);
    assert!(store.live_sessions(None, None, None, None, 1000).unwrap()[0].archived_at.is_none());
    store.set_session_archived(sid, true, 1234).unwrap();
    let s = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert!(s[0].archived);
    assert_eq!(s[0].archived_at, Some(1234)); // 归档记录时间戳
    store.set_session_archived(sid, false, 5678).unwrap();
    let s2 = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert!(!s2[0].archived);
    assert!(s2[0].archived_at.is_none()); // 取消归档清空时间戳
}

#[test]
fn import_session_inserts_ended_and_skips_existing() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1000).unwrap();

    let inserted = store
        .import_session("hist1", pid, "历史标题", Some("/p"), 5000)
        .unwrap();
    assert!(inserted);

    let sid = store.find_session_id_pub("hist1").unwrap().unwrap();
    let s = store.get_session(sid).unwrap();
    assert_eq!(s.status, "ended");
    assert_eq!(s.started_at, 5000);
    assert_eq!(s.last_event_at, 5000);
    assert_eq!(s.ended_at, Some(5000));
    assert_eq!(store.session_cwd(sid).unwrap(), Some("/p".to_string()));

    let tid = store.task_id_of_session_pub(sid).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "历史标题");
    assert_eq!(t.column, "done");

    let again = store
        .import_session("hist1", pid, "改标题", Some("/p"), 9000)
        .unwrap();
    assert!(!again);
    let s2 = store.get_session(sid).unwrap();
    assert_eq!(s2.last_event_at, 5000);
    let t2 = store.get_task(tid).unwrap();
    assert_eq!(t2.title, "历史标题");
}

#[test]
fn import_session_does_not_resurrect_real_session() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1000).unwrap();
    let (sid, _) = store.start_session(pid, "live1", 2000).unwrap();

    let inserted = store
        .import_session("live1", pid, "x", None, 8000)
        .unwrap();
    assert!(!inserted);
    assert_eq!(store.get_session(sid).unwrap().status, "running");
}

// == Task 3: last_ai_text / last_user_text ==
#[test]
fn last_ai_and_user_text_set_with_cleaning() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "cc1", 100).unwrap();

    // 折叠空白;不动 last_event_at(仍是建会话时的 100)。
    store.set_last_ai_text(sid, "  调研   完成。\n结论更微妙  ").unwrap();
    store.set_last_user_text(sid, "切到这个 [Image #1] 任务").unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "cc1").unwrap();
    assert_eq!(s.last_ai_text.as_deref(), Some("调研 完成。 结论更微妙"));
    assert_eq!(s.last_user_text.as_deref(), Some("切到这个 任务")); // [Image #1] 被 sanitize 剥除
    assert_eq!(s.session.last_event_at, 100);

    // 空串/全空白不覆盖旧值。
    store.set_last_ai_text(sid, "   ").unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "cc1").unwrap();
    assert_eq!(s.last_ai_text.as_deref(), Some("调研 完成。 结论更微妙"));
}

// == Task 2: PendingReview ==
#[test]
fn pending_review_set_and_clear() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "cc1", 100).unwrap();

    // set:写入子态并刷新 last_event_at。
    store.set_pending_review(sid, PendingReview::Approval, 500).unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "cc1").unwrap();
    assert_eq!(s.pending_review.as_deref(), Some("approval"));
    assert_eq!(s.session.last_event_at, 500);

    // clear:置 NULL,且不改 last_event_at。
    store.clear_pending_review(sid).unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "cc1").unwrap();
    assert_eq!(s.pending_review, None);
    assert_eq!(s.session.last_event_at, 500);
}

// == Task 5: on_user_prompt 不再写 current_activity ==
#[test]
fn on_user_prompt_no_longer_writes_current_activity() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "cc1", 100).unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();

    store.on_user_prompt(sid, "实现登录功能", 200).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "实现登录功能");          // 占位标题被首句替换(保留)
    assert_eq!(t.current_activity, None);          // 不再把 prompt 写进 current_activity
}

/// data_version 的两条性质是 db-watcher「只在真实写入时刷新看板」的根基（见 store::data_version）：
/// 1) 本连接自身的写入 / 纯读都不改自己的 data_version —— 故 watcher 的持久连接读版本永不自触发；
/// 2) 别的连接提交写入后，本连接再读 data_version 会变化 —— 故真实写入必被检出。
/// 必须用文件库（内存库连接互不共享），并在同进程内开两个独立连接。
#[test]
fn data_version_reflects_only_other_connection_writes() {
    let path = std::env::temp_dir().join(format!("meowo-dv-{}.db", std::process::id()));
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }

    let a = Store::open(&path).unwrap();
    let v0 = a.data_version().unwrap();

    // 本连接自身写入：不改自己的 data_version。
    let pid = a.upsert_project_by_root("/p", "p", 1).unwrap();
    assert_eq!(a.data_version().unwrap(), v0, "本连接自身写入不应改变自己的 data_version");

    // 纯读：不改 data_version（app 读库不该触发刷新的核心保证）。
    let _ = a.live_sessions(None, None, None, None, 10).unwrap();
    assert_eq!(a.data_version().unwrap(), v0, "纯读不应改变 data_version");

    // 别的连接提交写入：本连接再读即变化。
    let b = Store::open(&path).unwrap();
    b.start_session(pid, "s", 1).unwrap();
    assert_ne!(a.data_version().unwrap(), v0, "别的连接提交写入后 data_version 应变化");

    drop(a);
    drop(b);
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
