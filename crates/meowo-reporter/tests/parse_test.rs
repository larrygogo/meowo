use meowo_reporter::hook::HookEvent;

#[test]
fn empty_and_truncated_json_is_err() {
    assert!(HookEvent::parse("").is_err());
    assert!(HookEvent::parse("{").is_err());
    assert!(HookEvent::parse("{\"a\":").is_err());
}

#[test]
fn non_object_json_is_err() {
    assert!(HookEvent::parse("[]").is_err());
    assert!(HookEvent::parse("\"x\"").is_err());
    assert!(HookEvent::parse("42").is_err());
}

#[test]
fn missing_hook_event_name_is_err() {
    assert!(HookEvent::parse(r#"{"session_id":"a"}"#).is_err());
}

#[test]
fn null_tool_input_yields_empty_todos_and_no_bash() {
    let ev = HookEvent::parse(r#"{"hook_event_name":"PostToolUse","session_id":"a","tool_input":null}"#).unwrap();
    assert_eq!(ev.todo_items().len(), 0);
    assert_eq!(ev.bash_command(), None);
}

#[test]
fn todos_not_array_yields_empty() {
    let ev = HookEvent::parse(r#"{"hook_event_name":"PostToolUse","session_id":"a","tool_name":"TodoWrite","tool_input":{"todos":"oops"}}"#).unwrap();
    assert_eq!(ev.todo_items().len(), 0);
}

#[test]
fn todo_element_missing_content_is_skipped() {
    // 一条缺 content（必填）应被 filter_map 跳过；另一条合法保留
    let ev = HookEvent::parse(r#"{"hook_event_name":"PostToolUse","session_id":"a","tool_name":"TodoWrite","tool_input":{"todos":[{"status":"completed"},{"content":"ok","status":"in_progress"}]}}"#).unwrap();
    let items = ev.todo_items();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].content, "ok");
}

#[test]
fn bash_command_non_string_is_none() {
    let ev = HookEvent::parse(r#"{"hook_event_name":"PostToolUse","session_id":"a","tool_name":"Bash","tool_input":{"command":123}}"#).unwrap();
    assert_eq!(ev.bash_command(), None);
}
