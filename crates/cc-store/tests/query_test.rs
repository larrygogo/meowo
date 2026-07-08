use cc_store::{SessionStatus, Store, TodoInput, TodoStatus};

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

    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
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

    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
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
    assert_eq!(store.live_sessions(None, None, None, None, 1000).unwrap()[0].note, None);

    // 写入便签 → live_sessions 带出（前后空白被 trim）
    store.set_session_note("sess-a", "  记得 review  ", 120).unwrap();
    assert_eq!(store.live_sessions(None, None, None, None, 1000).unwrap()[0].note.as_deref(), Some("记得 review"));

    // upsert 覆盖旧便签
    store.set_session_note("sess-a", "改主意了", 130).unwrap();
    assert_eq!(store.live_sessions(None, None, None, None, 1000).unwrap()[0].note.as_deref(), Some("改主意了"));

    // 清空（trim 后为空）→ 删除该行，回到 None
    store.set_session_note("sess-a", "   ", 140).unwrap();
    assert_eq!(store.live_sessions(None, None, None, None, 1000).unwrap()[0].note, None);
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

#[test]
fn live_sessions_returns_new_columns_as_none_by_default() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    let (sid, _) = store.start_session(pid, "cc1", 100).unwrap();
    let _ = sid;

    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "cc1").unwrap();
    assert_eq!(s.pending_review, None);
    assert_eq!(s.last_ai_text, None);
    assert_eq!(s.last_user_text, None);
}

#[test]
fn live_sessions_paginates_without_ended_cap() {
    // 分页模式下不再对已结束会话做 100 条兜底截断；
    // 旧但仍在 running 的会话会按 last_event_at 排序，可能落到后续页，但全量查询时一定存在。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();
    // 一个很旧但仍 running 的会话（挂在后台终端里数天未发消息的合法长连接）。
    let (old_running, _) = store.start_session(pid, "old-running", 100).unwrap();
    store.set_session_pid(old_running, 4242, 110).unwrap();
    // 120 条更近活跃的已结束会话。
    for i in 0..120 {
        let (s, _) = store.start_session(pid, &format!("ended-{i}"), 1000 + i).unwrap();
        store.end_session(s, 2000 + i).unwrap();
    }

    // 第 0 页 100 条全是更近活跃的已结束会话，旧 running 不在其中。
    let first_page = store.live_sessions(Some("all"), None, None, None, 100).unwrap();
    assert_eq!(first_page.len(), 100);
    assert!(
        !first_page.iter().any(|s| s.session.cc_session_id == "old-running"),
        "旧 running 不应出现在全为已结束会话的首页"
    );

    // 用首页最后一条 cursor 取第二页：剩余 20 条已结束 + 1 条旧 running。
    let last = first_page.last().unwrap();
    let second_page = store
        .live_sessions(Some("all"), None, Some(last.session.last_event_at), Some(last.session.id), 100)
        .unwrap();
    assert_eq!(second_page.len(), 21);
    assert!(
        second_page.iter().any(|s| s.session.cc_session_id == "old-running"),
        "cursor 分页应把旧 running 带到第二页"
    );

    // 全量拉取时旧 running 仍存在，且已结束会话不再受 100 条限制。
    let all = store.live_sessions(None, None, None, None, 1000).unwrap();
    assert!(
        all.iter().any(|s| s.session.cc_session_id == "old-running"),
        "连接中的旧会话在全量结果中必须存在"
    );
    let ended = all.iter().filter(|s| s.session.status == "ended").count();
    assert_eq!(ended, 120);
}

#[test]
fn live_sessions_cursor_tie_respects_filter() {
    // 回归（审查发现）：游标条件 `(last_event_at < ts) OR (last_event_at = ts AND id < id)`
    // 若不整体加括号，与 filter 条件用 AND 拼接时，SQL 的 AND 优先级高于 OR，会让第二个
    // OR 分支绕过 filter（如 archived=0），把不该出现的行混进分页结果。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();

    // a 与 b 的 last_event_at 相同（游标边界 tie）；a 先建 id 更小，且被归档，b 不归档。
    let (a, _) = store.start_session(pid, "a1", 100).unwrap();
    let (b, _) = store.start_session(pid, "b1", 100).unwrap();
    store.set_session_archived(a, true, 200).unwrap();

    // 以 b 的 (last_event_at, id) 为游标：a 的 last_event_at 与游标相等且 id 更小，
    // 命中游标条件的第二分支；但 a 已归档，filter="all" 应把它排除，不能因游标 tie 而绕过。
    let page = store.live_sessions(Some("all"), None, Some(100), Some(b), 100).unwrap();
    assert!(
        page.iter().all(|l| l.session.id != a),
        "归档会话不应因游标边界 tie 绕过 filter 混入分页结果"
    );
}

#[test]
fn live_sessions_counts_matches_tabs() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();

    // running ×2
    let (r1, _) = store.start_session(pid, "r1", 100).unwrap();
    store.set_session_status(r1, SessionStatus::Running, 110).unwrap();
    let (r2, _) = store.start_session(pid, "r2", 200).unwrap();
    store.set_session_status(r2, SessionStatus::Running, 210).unwrap();

    // waiting ×1（status=waiting）
    let (w1, _) = store.start_session(pid, "w1", 300).unwrap();
    store.set_session_status(w1, SessionStatus::Waiting, 310).unwrap();

    // pending_review ×1：status 仍是 running，但应被算进 waiting
    let (p1, _) = store.start_session(pid, "p1", 320).unwrap();
    store.set_session_status(p1, SessionStatus::Running, 330).unwrap();
    store.set_pending_review(p1, cc_store::PendingReview::Question, 340).unwrap();

    // ended ×3，其中 1 条归档
    let mut archived_id = None;
    for i in 0..3 {
        let (s, _) = store.start_session(pid, &format!("ended-{i}"), 400 + i).unwrap();
        store.end_session(s, 500 + i).unwrap();
        if i == 0 {
            archived_id = Some(s);
        }
    }
    store.set_session_archived(archived_id.unwrap(), true, 600).unwrap();

    let c = store.live_sessions_counts().unwrap();
    assert_eq!(c.total, 7);
    assert_eq!(c.running, 2);
    assert_eq!(c.waiting, 2, "waiting 应包含 status=waiting 与 pending_review");
    assert_eq!(c.archived, 1);
}

#[test]
fn live_sessions_cursor_loads_all_non_archived() {
    // 验证 cursor 分页不会漏掉任何非归档会话。
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();

    // 250 条非归档会话，last_event_at 从 1000 递增到 3499
    for i in 0..250 {
        let (s, _) = store.start_session(pid, &format!("s-{i}"), 1000 + i).unwrap();
        store.end_session(s, 2000 + i).unwrap();
    }
    // 50 条归档会话，last_event_at 更晚（4500..），确保不会被误算进 all
    for i in 0..50 {
        let (s, _) = store.start_session(pid, &format!("arch-{i}"), 4500 + i).unwrap();
        store.end_session(s, 4600 + i).unwrap();
        store.set_session_archived(s, true, 4700 + i).unwrap();
    }

    let counts = store.live_sessions_counts().unwrap();
    assert_eq!(counts.total, 300);
    assert_eq!(counts.archived, 50);

    let mut loaded: Vec<i64> = Vec::new();
    let mut cursor: Option<(i64, i64)> = None;
    loop {
        let page = match cursor {
            Some((ts, id)) => store.live_sessions(Some("all"), None, Some(ts), Some(id), 80).unwrap(),
            None => store.live_sessions(Some("all"), None, None, None, 80).unwrap(),
        };
        if page.is_empty() {
            break;
        }
        loaded.extend(page.iter().map(|s| s.session.id));
        let last = page.last().unwrap();
        cursor = Some((last.session.last_event_at, last.session.id));
        if page.len() < 80 {
            break;
        }
    }

    assert_eq!(loaded.len(), 250, "应加载全部 250 条非归档会话");
    let loaded_set: std::collections::HashSet<_> = loaded.iter().copied().collect();
    assert_eq!(loaded_set.len(), 250, "不应有重复 session");
}

#[test]
fn live_sessions_waiting_includes_pending_review() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 100).unwrap();

    let (running, _) = store.start_session(pid, "running", 100).unwrap();
    store.set_session_status(running, SessionStatus::Running, 110).unwrap();

    let (waiting, _) = store.start_session(pid, "waiting", 200).unwrap();
    store.set_session_status(waiting, SessionStatus::Waiting, 210).unwrap();

    let (pending, _) = store.start_session(pid, "pending", 300).unwrap();
    store.set_session_status(pending, SessionStatus::Running, 310).unwrap();
    store.set_pending_review(pending, cc_store::PendingReview::Question, 320).unwrap();

    let waiting_page = store.live_sessions(Some("waiting"), None, None, None, 100).unwrap();
    let ids: std::collections::HashSet<i64> = waiting_page.iter().map(|l| l.session.id).collect();
    assert!(ids.contains(&waiting), "status=waiting 应在 waiting 分页");
    assert!(ids.contains(&pending), "pending_review 应在 waiting 分页");
    assert!(!ids.contains(&running), "纯 running 不应在 waiting 分页");
}

#[test]
fn live_sessions_search_scoped_to_tab() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
    // running: 标题含 "login"
    let (r, _) = store.start_session(pid, "r", 100).unwrap();
    store.on_user_prompt(r, "实现 login 登录", 110).unwrap();
    store.set_session_status(r, SessionStatus::Running, 110).unwrap();
    // waiting: 标题含 "login"
    let (w, _) = store.start_session(pid, "w", 200).unwrap();
    store.on_user_prompt(w, "login 待回复", 210).unwrap();
    store.set_session_status(w, SessionStatus::Waiting, 210).unwrap();
    // running: 标题不含 "login"
    let (r2, _) = store.start_session(pid, "r2", 300).unwrap();
    store.on_user_prompt(r2, "别的任务", 310).unwrap();
    store.set_session_status(r2, SessionStatus::Running, 310).unwrap();

    // 全部 tab 搜 login：命中 r + w，不含 r2
    let all = store.live_sessions(Some("all"), Some("login"), None, None, 100).unwrap();
    assert_eq!(all.len(), 2);
    // running tab 搜 login：只命中 r（w 是 waiting，被 filter 排除）
    let run = store.live_sessions(Some("running"), Some("login"), None, None, 100).unwrap();
    assert_eq!(run.len(), 1);
    assert_eq!(run[0].session.cc_session_id, "r");
    // waiting tab 搜 login：只命中 w
    let wait = store.live_sessions(Some("waiting"), Some("login"), None, None, 100).unwrap();
    assert_eq!(wait.len(), 1);
    assert_eq!(wait[0].session.cc_session_id, "w");
}

#[test]
fn live_sessions_search_matches_cwd_and_escapes_wildcards() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
    let (a, _) = store.start_session(pid, "a", 100).unwrap();
    store.on_user_prompt(a, "无关标题", 110).unwrap();
    store.set_session_cwd(a, "C:/work/my_proj", 120).unwrap();
    let (b, _) = store.start_session(pid, "b", 200).unwrap();
    store.on_user_prompt(b, "无关", 210).unwrap();
    store.set_session_cwd(b, "C:/work/myXproj", 220).unwrap();

    // 搜 cwd 片段命中 a
    let hit = store.live_sessions(Some("all"), Some("my_proj"), None, None, 100).unwrap();
    // `_` 被转义为字面下划线：只命中 my_proj，不命中 myXproj
    assert!(hit.iter().any(|l| l.session.cc_session_id == "a"));
    assert!(!hit.iter().any(|l| l.session.cc_session_id == "b"), "`_` 应作字面量、不作通配");
}

#[test]
fn live_sessions_waiting_sorted_ascending_and_paginates() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
    // 3 条 waiting，last_event_at 递增：等最久(最小 last_event_at)应先出
    for i in 0..3 {
        let (s, _) = store.start_session(pid, &format!("w{i}"), 100 + i).unwrap();
        store.on_user_prompt(s, &format!("t{i}"), 100 + i).unwrap();
        store.set_session_status(s, SessionStatus::Waiting, 100 + i).unwrap();
    }
    let page = store.live_sessions(Some("waiting"), None, None, None, 2).unwrap();
    assert_eq!(page.len(), 2);
    // ASC：w0(最久) 在最前
    assert_eq!(page[0].session.cc_session_id, "w0");
    assert_eq!(page[1].session.cc_session_id, "w1");
    // 用末条 cursor 取下一页（ASC 游标方向）
    let last = page.last().unwrap();
    let next = store
        .live_sessions(Some("waiting"), None, Some(last.session.last_event_at), Some(last.session.id), 2)
        .unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].session.cc_session_id, "w2");
}

#[test]
fn live_sessions_search_none_is_backcompat() {
    let store = Store::open_in_memory().unwrap();
    let pid = store.upsert_project_by_root("/p", "p", 1).unwrap();
    let (s, _) = store.start_session(pid, "s", 100).unwrap();
    store.on_user_prompt(s, "任务", 110).unwrap();
    // search=None 与不搜一致：返回该会话
    let r = store.live_sessions(Some("all"), None, None, None, 100).unwrap();
    assert_eq!(r.len(), 1);
    // search=Some("") 空串按不搜处理
    let r2 = store.live_sessions(Some("all"), Some("  "), None, None, 100).unwrap();
    assert_eq!(r2.len(), 1);
}
