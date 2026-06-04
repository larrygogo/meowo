use cc_reporter::hook::HookEvent;
use std::io::Write as _;

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
    assert_eq!(ev.prompt.as_deref(), Some("写个登录"));
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

use cc_reporter::dispatch::dispatch;
use cc_store::Store;

fn ev(json: &str) -> HookEvent { HookEvent::parse(json).unwrap() }

fn write_transcript(name: &str, body: &[u8]) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(name);
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(body).unwrap();
    p
}

#[test]
fn session_start_then_prompt_then_todos_flow() {
    let store = Store::open_in_memory().unwrap();

    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/home/me/proj"}"#), 100).unwrap();
    let projects = store.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "proj");

    dispatch(&store, &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt":"实现登录"}"#), 200).unwrap();
    dispatch(&store, &ev(r#"{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"TodoWrite","tool_input":{"todos":[{"content":"a","status":"in_progress"}]}}"#), 300).unwrap();

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
    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"s2","cwd":"/p"}"#), 100).unwrap();
    dispatch(&store, &ev(r#"{"hook_event_name":"Stop","session_id":"s2"}"#), 200).unwrap();
    let sid = store.find_session_id_pub("s2").unwrap().unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "waiting");

    dispatch(&store, &ev(r#"{"hook_event_name":"SessionEnd","session_id":"s2"}"#), 300).unwrap();
    assert_eq!(store.get_session(sid).unwrap().status, "ended");
}

#[test]
fn unknown_session_for_prompt_is_ignored_gracefully() {
    let store = Store::open_in_memory().unwrap();
    let r = dispatch(&store, &ev(r#"{"hook_event_name":"UserPromptSubmit","session_id":"ghost","prompt":"x"}"#), 100);
    assert!(r.is_ok());
}

#[test]
fn posttooluse_bash_sets_current_activity() {
    let store = Store::open_in_memory().unwrap();
    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"b1","cwd":"/tmp/p"}"#), 100).unwrap();
    dispatch(&store, &ev(r#"{"hook_event_name":"PostToolUse","session_id":"b1","tool_name":"Bash","tool_input":{"command":"cargo build"}}"#), 200).unwrap();
    let sid = store.find_session_id_pub("b1").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    assert_eq!(store.get_task(tid).unwrap().current_activity.as_deref(), Some("› cargo build"));
}

#[test]
fn stop_and_end_for_unknown_session_are_ignored() {
    let store = Store::open_in_memory().unwrap();
    assert!(dispatch(&store, &ev(r#"{"hook_event_name":"Stop","session_id":"nope"}"#), 100).is_ok());
    assert!(dispatch(&store, &ev(r#"{"hook_event_name":"SessionEnd","session_id":"nope"}"#), 100).is_ok());
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
    dispatch(&store, &ev(&json), 100).unwrap();
    let sid = store.find_session_id_pub("st1").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    assert_eq!(store.get_task(tid).unwrap().title, "做看板");
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
    dispatch(&store, &ev(r#"{"hook_event_name":"SessionStart","session_id":"st2","cwd":"/tmp/y"}"#), 100).unwrap();
    // 再 UserPromptSubmit（带 transcript）
    let json = format!(
        r#"{{"hook_event_name":"UserPromptSubmit","session_id":"st2","prompt":"首条prompt兜底","transcript_path":"{tps}"}}"#
    );
    dispatch(&store, &ev(&json), 200).unwrap();
    let sid = store.find_session_id_pub("st2").unwrap().unwrap();
    let tid = store.task_id_of_session_pub(sid).unwrap();
    // transcript custom-title 覆盖了 prompt 兜底标题
    assert_eq!(store.get_task(tid).unwrap().title, "My Custom Title");
    let _ = std::fs::remove_file(tp);
}
