use cc_reporter::hook::HookEvent;

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
