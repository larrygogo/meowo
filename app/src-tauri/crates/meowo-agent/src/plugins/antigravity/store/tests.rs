use super::proto::*;
use super::test_support::*;
use super::*;

fn ts(seconds: i64) -> Option<Timestamp> {
    Some(Timestamp { seconds, nanos: 0 })
}

fn meta(seconds: i64) -> Option<StepMetadata> {
    Some(StepMetadata {
        created_at: ts(seconds),
        ..Default::default()
    })
}

fn user(text: &str) -> Step {
    Step {
        r#type: step_type::USER_INPUT,
        status: status::DONE,
        metadata: meta(100),
        user_input: Some(UserInput {
            user_response: text.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn reply(text: &str, state: i32, calls: &[(&str, &str)]) -> Step {
    Step {
        r#type: step_type::PLANNER_RESPONSE,
        status: state,
        metadata: meta(101),
        planner_response: Some(PlannerResponse {
            modified_response: text.into(),
            thinking: "plan it".into(),
            tool_calls: calls
                .iter()
                .map(|(id, name)| ChatToolCall {
                    id: (*id).into(),
                    name: (*name).into(),
                    arguments_json: String::new(),
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn tool(id: &str, name: &str, args: &str, state: i32, output: &str) -> Step {
    Step {
        r#type: step_type::GENERIC,
        status: state,
        metadata: Some(StepMetadata {
            created_at: ts(102),
            completed_at: ts(103),
            tool_call: Some(ChatToolCall {
                id: id.into(),
                name: name.into(),
                arguments_json: args.into(),
            }),
            ..Default::default()
        }),
        generic: Some(Generic {
            result: Some(GenericResult {
                result: output.into(),
                step_render_info: Some(RenderInfo {
                    title_prefix: "Ran".into(),
                    title_parts: vec![TitlePart {
                        text: "echo hi".into(),
                    }],
                    content_blocks: vec![ContentBlock {
                        terminal: Some(TerminalBlock {
                            command: "echo hi".into(),
                            exit_code: Some(0),
                            output: "hi".into(),
                        }),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn kinds(lines: &[Value]) -> Vec<&str> {
    lines.iter().map(|l| l["kind"].as_str().unwrap()).collect()
}

#[test]
fn exports_only_the_settled_prefix() {
    let conn = memory_db();
    put(&conn, 0, &user("hi"));
    put(&conn, 1, &reply("", status::DONE, &[("c1", "run_command")]));
    put(&conn, 2, &tool("c1", "run_command", r#"{"CommandLine":"echo hi"}"#, status::WAITING, ""));
    put(&conn, 3, &reply("", status::GENERATING, &[]));
    let lines = lines(&conn, None);
    // 工具在等审批：停在它前面，后面的生成中回复也不导出。
    assert_eq!(kinds(&lines), ["user", "assistant"]);
    assert_eq!(lines[1]["idx"], 1);
}

#[test]
fn a_later_user_message_settles_an_abandoned_step() {
    let conn = memory_db();
    put(&conn, 0, &user("hi"));
    put(&conn, 1, &tool("c1", "run_command", "{}", status::RUNNING, ""));
    put(&conn, 2, &user("never mind"));
    let lines = lines(&conn, None);
    assert_eq!(kinds(&lines), ["user", "tool", "user"]);
    assert_eq!(lines[1]["status"], "interrupted");
}

#[test]
fn tool_line_keeps_name_args_title_exit_code_and_strips_time_header() {
    let conn = memory_db();
    put(
        &conn,
        0,
        &tool(
            "c1",
            "run_command",
            r#"{"CommandLine":"echo hi","WaitMsBeforeAsync":5000}"#,
            status::DONE,
            "Created At: 2026-11-26T15:12:35+08:00\nCompleted At: 2026-11-26T15:12:54+08:00\nCommand exited with code 0.\n",
        ),
    );
    let line = &lines(&conn, None)[0];
    assert_eq!(line["name"], "run_command");
    assert_eq!(line["id"], "c1");
    assert_eq!(line["args"]["WaitMsBeforeAsync"], 5000, "参数取原始 JSON，数字不被字符串化");
    assert_eq!(line["title"], "Ran echo hi");
    assert_eq!(line["exit_code"], 0);
    assert_eq!(line["output"], "Command exited with code 0.\n");
}

#[test]
fn model_and_context_come_from_gen_metadata() {
    let conn = memory_db();
    put(&conn, 0, &user("hi"));
    put(&conn, 1, &reply("hello", status::DONE, &[]));
    put_gen(
        &conn,
        0,
        &GeneratorMetadata {
            chat_model: Some(ChatModel {
                model_display_name: "Gemini 4 Neon (High)".into(),
                response_model: "gemini-4-neon".into(),
                chat_start_metadata: Some(ChatStart {
                    context_window_metadata: Some(ContextWindow {
                        estimated_tokens_used: 22958,
                        max_context_tokens: Some(512000),
                    }),
                }),
                ..Default::default()
            }),
            step_indices: vec![1],
        },
    );
    let line = &lines(&conn, None)[1];
    assert_eq!(line["model"], "Gemini 4 Neon (High)");
    assert_eq!(line["context_tokens"], 22958);
    assert_eq!(line["context_window"], 512000);
}

#[test]
fn hidden_system_messages_still_advance_the_cursor() {
    let conn = memory_db();
    put(
        &conn,
        0,
        &Step {
            r#type: step_type::SYSTEM_MESSAGE,
            status: status::DONE,
            system_message: Some(SystemMessage {
                message: "work efficiently".into(),
                render_info: Some(RenderInfo {
                    hidden: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    assert_eq!(kinds(&lines(&conn, None)), ["skip"]);
}

#[test]
fn export_appends_then_regenerates_after_rewind() {
    let dir = std::env::temp_dir().join(format!("meowo-agy-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let conn = memory_db();
    put(&conn, 0, &user("one"));
    put(&conn, 1, &reply("first", status::DONE, &[]));
    let first = export(&conn, &dir);
    put(&conn, 2, &user("two"));
    let same = export(&conn, &dir);
    assert_eq!(first, same, "新内容追加在同一代文件里");
    assert_eq!(std::fs::read_to_string(&same).unwrap().lines().count(), 3);
    // /rewind 截掉了游标所在的行：作废重导到下一代。
    conn.execute("DELETE FROM steps WHERE idx >= 1", []).unwrap();
    let next = export(&conn, &dir);
    assert_ne!(next, same);
    assert!(!same.exists());
    assert_eq!(std::fs::read_to_string(&next).unwrap().lines().count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn strip_result_header_handles_repeated_and_missing_headers() {
    assert_eq!(strip_result_header("Created At: a\nCompleted At: b\nCreated At: a\nCompleted At: b\nbody"), "body");
    assert_eq!(strip_result_header("plain"), "plain");
    assert_eq!(strip_result_header("Created At: a\nCompleted At: b"), "");
}

/// 本机真库回归（不进仓库：真库里有账号与工作区路径）。`MEOWO_AGY_DB=<conversations/x.db>` 指定，
/// 打印导出的行种类，并检查每个工具行都有工具名、每个回复行都有模型名。
#[test]
#[ignore]
fn real_conversation_db_exports() {
    let path = std::env::var("MEOWO_AGY_DB").expect("MEOWO_AGY_DB");
    let conn = open(Path::new(&path)).expect("open");
    let lines = lines(&conn, None);
    for line in &lines {
        eprintln!("{}", serde_json::to_string(line).unwrap().chars().take(200).collect::<String>());
    }
    // `MEOWO_AGY_OUT=<文件>`：把完整导出写出来，脱敏后做回放样本（tests/fixtures/chat-replay）。
    if let Ok(out) = std::env::var("MEOWO_AGY_OUT") {
        let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(out, body).unwrap();
    }
    assert!(!lines.is_empty());
    for line in lines.iter().filter(|l| l["kind"] == "tool") {
        assert_ne!(line["name"], "tool", "工具行缺名字: {line}");
    }
    for line in lines.iter().filter(|l| l["kind"] == "assistant") {
        assert!(line["model"].is_string(), "回复行缺模型: {line}");
    }
    for line in &lines {
        let events = super::super::telemetry::parse_line(&serde_json::to_string(line).unwrap());
        assert!(line["kind"] == "skip" || line["kind"] == "system" || !events.is_empty() || line["text"] == "", "行没产出事件: {line}");
    }
}
