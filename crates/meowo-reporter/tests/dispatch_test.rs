use meowo_reporter::dispatch::owner_repo_from_url_pub;
use meowo_reporter::hook::HookEvent;
use std::io::Write as _;

#[test]
fn owner_repo_parsing() {
    assert_eq!(
        owner_repo_from_url_pub("https://github.com/larrygogo/autopilot.git").as_deref(),
        Some("larrygogo/autopilot")
    );
    assert_eq!(
        owner_repo_from_url_pub("git@github.com:larrygogo/autopilot.git").as_deref(),
        Some("larrygogo/autopilot")
    );
    assert_eq!(
        owner_repo_from_url_pub("https://gitlab.com/grp/sub/repo.git").as_deref(),
        Some("sub/repo")
    );
}

#[test]
fn parse_user_prompt_event() {
    let json = r#"{
        "hook_event_name": "UserPromptSubmit",
        "session_id": "abc",
        "cwd": "/home/me/proj",
        "prompt": "写个登录"
    }"#;
    let ev = HookEvent::parse(json).expect("parse");
    assert_eq!(ev.session_id, "abc");
    assert_eq!(ev.cwd.as_deref(), Some("/home/me/proj"));
    assert_eq!(ev.hook_event_name, "UserPromptSubmit");
    assert_eq!(ev.prompt_text().as_deref(), Some("写个登录"));
}

#[test]
fn parse_kimi_user_prompt_array_form() {
    // kimi-code 的 prompt 是内容块数组（非字符串）；旧的 Option<String> 会解析失败，现应规整成文本。
    let json = r#"{
        "hook_event_name": "UserPromptSubmit",
        "session_id": "k",
        "cwd": "/p",
        "prompt": [{"type":"text","text":"实现"},{"type":"text","text":"登录"}]
    }"#;
    let ev = HookEvent::parse(json).expect("kimi 数组形式应能解析");
    assert_eq!(ev.prompt_text().as_deref(), Some("实现登录"));
}

#[test]
fn parse_posttooluse_todowrite() {
    let json = r#"{
        "hook_event_name": "PostToolUse",
        "session_id": "abc",
        "cwd": "/p",
        "tool_name": "TodoWrite",
        "tool_input": { "todos": [
            {"content":"a","status":"completed"},
            {"content":"b","status":"in_progress"}
        ]}
    }"#;
    let ev = HookEvent::parse(json).expect("parse");
    assert_eq!(ev.tool_name.as_deref(), Some("TodoWrite"));
    let todos = ev.todo_items();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[1].content, "b");
}

#[test]
fn parse_tolerates_unknown_fields() {
    let json = r#"{"hook_event_name":"Stop","session_id":"z","extra":123}"#;
    let ev = HookEvent::parse(json).expect("parse");
    assert_eq!(ev.hook_event_name, "Stop");
}

use meowo_reporter::dispatch::dispatch;
use meowo_store::{ProviderKey, Store};

fn ev(json: &str) -> HookEvent { HookEvent::parse(json).unwrap() }

/// 测试默认走 claude provider；provider 行为单独在 kimi_session_tagged_with_provider 覆盖。
fn disp(store: &Store, ev: &HookEvent, now_ms: i64) -> Result<(), meowo_store::StoreError> {
    dispatch(store, ev, now_ms, ProviderKey::Claude)
}

fn write_transcript(name: &str, body: &[u8]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(body).unwrap();
    p
}

#[test]
fn session_start_then_prompt_then_todos_flow() {
    let store = Store::open_in_memory().unwrap();

    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/home/me/proj"}"#), 100).unwrap();
    let projects = store.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "proj");

    disp(&store, &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"实现登录"}"#), 200).unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"TodoWrite","tool_input":{"todos":[{"content":"a","status":"in_progress"}]}}"#), 300).unwrap();

    let sid = store.find_session_id_pub("s1").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    let t = store.get_task(tid).unwrap();
    assert_eq!(t.title, "实现登录");
    assert_eq!(t.column, "doing");
    assert_eq!(store.list_todos(tid).unwrap().len(), 1);
}

#[test]
fn stop_then_end_updates_session_status() {
    let store = Store::open_in_memory().unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"s2","cwd":"/p"}"#), 100).unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"Stop","session_id":"s2"}"#), 200).unwrap();
    let sid = store.find_session_id_pub("s2").unwrap().unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "waiting");

    disp(&store, &ev(r#"{"hook_event_name":"SessionEnd","session_id":"s2"}"#), 300).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "ended");
}

#[test]
fn unknown_session_for_prompt_is_ignored_gracefully() {
    let store = Store::open_in_memory().unwrap();
    let r = disp(&store, &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"ghost","prompt":"x"}"#), 100);
    assert!(r.is_ok());
}

#[test]
fn posttooluse_bash_sets_current_activity() {
    let store = Store::open_in_memory().unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"b1","cwd":"/tmp/p"}"#), 100).unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"PostToolUse","session_id":"b1","tool_name":"Bash","tool_input":{"command":"cargo build"}}"#), 200).unwrap();
    let sid = store.find_session_id_pub("b1").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    assert_eq!(store.get_task(tid).unwrap().current_activity.as_deref(), Some("› cargo build"));
}

#[test]
fn stop_and_end_for_unknown_session_are_ignored() {
    let store = Store::open_in_memory().unwrap();
    assert!(disp(&store, &ev(r#"{"hook_event_name":"Stop","session_id":"nope"}"#), 100).is_ok());
    assert!(disp(&store, &ev(r#"{"hook_event_name":"SessionEnd","session_id":"nope"}"#), 100).is_ok());
}

#[test]
fn session_start_with_transcript_sets_ai_title() {
    let store = Store::open_in_memory().unwrap();
    let tp = write_transcript(
        "cc_disp_start.jsonl",
        b"{\"type\":\"ai-title\",\"aiTitle\":\"\xe5\x81\x9a\xe7\x9c\x8b\xe6\x9d\xbf\",\"sessionId\":\"s\"}\n",
    );
    let tps = tp.to_str().unwrap().replace('\\', "\\\\");
    let json = format!(
        r#"{{"hook_event_name":"SessionStart","session_id":"st1","cwd":"/tmp/x","transcript_path":"{tps}"}}"#
    );
    disp(&store, &ev(&json), 100).unwrap();
    let sid = store.find_session_id_pub("st1").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    assert_eq!(store.get_task(tid).unwrap().title, "做看板");
    let _ = std::fs::remove_file(tp);
}

/// Stop 事件无 cwd，但 SessionStart 时已存进库，应能用存的 cwd 重建路径并刷新标题。
#[test]
fn stop_refreshes_title_via_stored_cwd() {
    let store = Store::open_in_memory().unwrap();
    // 先 SessionStart（不带 transcript_path），把 cwd 存进库
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"st3","cwd":"/tmp/z"}"#), 100).unwrap();

    // 写 transcript（用真实路径直接传给下方 Stop）
    let tp = write_transcript(
        "cc_disp_stop.jsonl",
        b"{\"type\":\"ai-title\",\"aiTitle\":\"Stop\xe5\x88\xb7\xe6\x96\xb0\",\"sessionId\":\"s\"}\n",
    );
    let tps = tp.to_str().unwrap().replace('\\', "\\\\");

    // Stop 带 transcript_path 但不带 cwd——由 store 里的 cwd 兜底
    let json = format!(
        r#"{{"hook_event_name":"Stop","session_id":"st3","transcript_path":"{tps}"}}"#
    );
    disp(&store, &ev(&json), 200).unwrap();

    let sid = store.find_session_id_pub("st3").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    // 标题已从 transcript 刷新
    assert_eq!(store.get_task(tid).unwrap().title, "Stop刷新");
    // 状态应为 waiting
    assert_eq!(store.get_session(sid).unwrap().status, "waiting");
    let _ = std::fs::remove_file(tp);
}

#[test]
fn hookevent_parses_last_assistant_message_and_alias() {
    let a = ev(r#"{"hook_event_name":"Stop","session_id":"s","last_assistant_message":"结论更微妙"}"#);
    assert_eq!(a.last_assistant_message.as_deref(), Some("结论更微妙"));
    // 官方文档另称 assistant_message,alias 也要能接住。
    let b = ev(r#"{"hook_event_name":"Stop","session_id":"s","assistant_message":"另一种字段名"}"#);
    assert_eq!(b.last_assistant_message.as_deref(), Some("另一种字段名"));
}

/// UserPromptSubmit 不带 cwd，但 SessionStart 存了，apply_title 应用存的 cwd 重建路径。
#[test]
fn prompt_without_cwd_uses_stored_cwd_for_title() {
    let store = Store::open_in_memory().unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"st4","cwd":"/tmp/w"}"#), 100).unwrap();

    let tp = write_transcript(
        "cc_disp_nocwd.jsonl",
        b"{\"type\":\"custom-title\",\"customTitle\":\"NoCwd\xe5\x85\x9c\xe5\xba\x95\",\"sessionId\":\"s\"}\n",
    );
    let tps = tp.to_str().unwrap().replace('\\', "\\\\");

    // UserPromptSubmit 不带 cwd，但带 transcript_path
    let json = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"st4","prompt":"hello","transcript_path":"{tps}"}}"#
    );
    disp(&store, &ev(&json), 200).unwrap();

    let sid = store.find_session_id_pub("st4").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    assert_eq!(store.get_task(tid).unwrap().title, "NoCwd兜底");
    let _ = std::fs::remove_file(tp);
}

#[test]
fn user_prompt_with_transcript_overrides_prompt_title() {
    let store = Store::open_in_memory().unwrap();
    let tp = write_transcript(
        "cc_disp_prompt.jsonl",
        b"{\"type\":\"custom-title\",\"customTitle\":\"My Custom Title\",\"sessionId\":\"s\"}\n",
    );
    let tps = tp.to_str().unwrap().replace('\\', "\\\\");

    // 先 SessionStart（无 transcript）
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"st2","cwd":"/tmp/y"}"#), 100).unwrap();
    // 再 UserPromptSubmit（带 transcript）
    let json = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"st2","prompt":"首条prompt兜底","transcript_path":"{tps}"}}"#
    );
    disp(&store, &ev(&json), 200).unwrap();
    let sid = store.find_session_id_pub("st2").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    // transcript custom-title 覆盖了 prompt 兜底标题
    assert_eq!(store.get_task(tid).unwrap().title, "My Custom Title");
    let _ = std::fs::remove_file(tp);
}

// == Task 6: PermissionRequest / PreToolUse 置 pending_review ==
#[test]
fn permission_and_pretooluse_set_pending_review() {
    let store = Store::open_in_memory().unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"p1","cwd":"/p"}"#), 100).unwrap();

    let kind = |cc: &str| {
        store.live_sessions(None, None, None, None, 1000).unwrap().into_iter()
            .find(|l| l.session.cc_session_id == cc).unwrap().pending_review
    };

    // PermissionRequest:无 tool_name/普通工具 → approval。
    disp(&store, &ev(r#"{"hook_event_name":"PermissionRequest","session_id":"p1","tool_name":"Bash"}"#), 200).unwrap();
    assert_eq!(kind("p1").as_deref(), Some("approval"));
    // PermissionRequest:ExitPlanMode → plan。
    disp(&store, &ev(r#"{"hook_event_name":"PermissionRequest","session_id":"p1","tool_name":"ExitPlanMode"}"#), 210).unwrap();
    assert_eq!(kind("p1").as_deref(), Some("plan"));
    // PreToolUse:AskUserQuestion → question。
    disp(&store, &ev(r#"{"hook_event_name":"PreToolUse","session_id":"p1","tool_name":"AskUserQuestion"}"#), 220).unwrap();
    assert_eq!(kind("p1").as_deref(), Some("question"));
    // PreToolUse:其它工具 → 无操作(保持上一个 question)。
    disp(&store, &ev(r#"{"hook_event_name":"PreToolUse","session_id":"p1","tool_name":"Read"}"#), 230).unwrap();
    assert_eq!(kind("p1").as_deref(), Some("question"));
}

// == Task 5: Stop 落 last_ai_text、UserPromptSubmit 落 last_user_text ==
#[test]
fn stop_sets_last_ai_text_and_prompt_sets_last_user_text() {
    let store = Store::open_in_memory().unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"m1","cwd":"/p"}"#), 100).unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"m1","prompt":"切到这个任务"}"#), 200).unwrap();
    disp(&store, &ev(r#"{"hook_event_name":"Stop","session_id":"m1","last_assistant_message":"调研完成,结论更微妙"}"#), 300).unwrap();

    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = live.iter().find(|l| l.session.cc_session_id == "m1").unwrap();
    assert_eq!(s.last_user_text.as_deref(), Some("切到这个任务"));
    assert_eq!(s.last_ai_text.as_deref(), Some("调研完成,结论更微妙"));
}

#[test]
fn pending_review_cleared_by_next_event() {
    for (i, clear_ev) in [
        r#"{"hook_event_name":"PostToolUse","session_id":"c1","tool_name":"Read"}"#,
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"c1","prompt":"继续"}"#,
        r#"{"hook_event_name":"Stop","session_id":"c1"}"#,
        r#"{"hook_event_name":"SessionEnd","session_id":"c1"}"#,
    ].iter().enumerate() {
        let store = Store::open_in_memory().unwrap();
        disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"c1","cwd":"/p"}"#), 100).unwrap();
        disp(&store, &ev(r#"{"hook_event_name":"PermissionRequest","session_id":"c1","tool_name":"Bash"}"#), 200).unwrap();
        // 置位后确认非空。
        let pending = store.live_sessions(None, None, None, None, 1000).unwrap().into_iter()
            .find(|l| l.session.cc_session_id == "c1").unwrap().pending_review;
        assert_eq!(pending.as_deref(), Some("approval"), "case {i} 置位前提");
        // 下一个事件清除。
        disp(&store, &ev(clear_ev), 300).unwrap();
        let pending = store.live_sessions(None, None, None, None, 1000).unwrap().into_iter()
            .find(|l| l.session.cc_session_id == "c1").unwrap().pending_review;
        assert_eq!(pending, None, "case {i} 应被清除");
    }
}

#[test]
fn provider_defaults_claude_and_kimi_is_tagged() {
    let store = Store::open_in_memory().unwrap();
    // 默认 provider（不带 --provider）→ claude。
    disp(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"cl1","cwd":"/p"}"#), 100).unwrap();
    // kimi provider 显式标记。
    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"km1","cwd":"/p"}"#), 110, ProviderKey::Kimi).unwrap();
    let live = store.live_sessions(None, None, None, None, 1000).unwrap();
    let prov = |sid: &str| {
        live.iter().find(|l| l.session.cc_session_id == sid).unwrap().provider.clone()
    };
    assert_eq!(prov("cl1"), "claude");
    assert_eq!(prov("km1"), "kimi");
}

#[test]
fn activity_event_revives_mis_reaped_ended_session() {
    // 会话被误清成 ended（如 app 的 reap 一度不认 kimi pid）后，任一活动事件都应复活，不只 UserPromptSubmit。
    let store = Store::open_in_memory().unwrap();
    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"rv1","cwd":"/p"}"#), 100, ProviderKey::Kimi).unwrap();
    let sid = store.find_session_id_pub("rv1").unwrap().unwrap();
    store.end_session(sid, 150).unwrap(); // 模拟被误 reap
    dispatch(&store, &ev(r#"{"hook_event_name":"PostToolUse","session_id":"rv1","cwd":"/p","tool_name":"Read"}"#), 200, ProviderKey::Kimi).unwrap();
    let s = store.live_sessions(None, None, None, None, 1000).unwrap().into_iter().find(|l| l.session.cc_session_id == "rv1").unwrap();
    assert_eq!(s.session.status, "running");
    assert_eq!(s.session.ended_at, None); // ended_at 被清，状态自洽
}

#[test]
fn lazy_creates_session_on_prompt_when_session_start_missing() {
    // 模拟「hooks 中途装上」：没有 SessionStart，直接来 UserPromptSubmit（带 cwd）→ 应就地建会话。
    let store = Store::open_in_memory().unwrap();
    dispatch(
        &store,
        &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"mid1","cwd":"/p","prompt":"中途接入"}"#),
        100,
        ProviderKey::Kimi,
    )
    .unwrap();
    let l = store.live_sessions(None, None, None, None, 1000).unwrap();
    let s = l.iter().find(|l| l.session.cc_session_id == "mid1").expect("应懒创建出会话");
    assert_eq!(s.provider, "kimi");
    assert_eq!(s.task_title, "中途接入");
}
