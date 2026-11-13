use super::*;
use crate::transcript::{read_chat_delta, read_chat_delta_paged, ChatItem};

#[test]
fn independent_review_long_prefix_in_place_rewrite_discards_old_history() {
    let root = temp("independent-long-rewrite");
    let source = root.join("session.jsonl");
    let common = format!(
        "{HEADER}\n{}\n",
        json!({"id":"common","type":"user","content":"x".repeat(2000)})
    );
    fs::write(
        &source,
        format!("{common}{{\"id\":\"removed\",\"type\":\"user\",\"content\":\"OLD_HISTORY\"}}\n"),
    )
    .unwrap();
    let _first = materialize_in(&source, &root.join("cache")).unwrap();
    fs::write(
        &source,
        format!(
            "{common}{}\n",
            json!({"id":"new","type":"user","content":"NEW_HISTORY".repeat(100)})
        ),
    )
    .unwrap();
    let next = materialize_in(&source, &root.join("cache")).unwrap();
    let raw = fs::read_to_string(next).unwrap();
    assert!(!raw.contains("OLD_HISTORY"), "重写后不应保留已删除消息");
    assert!(raw.contains("NEW_HISTORY"), "重写后必须读取新增消息");
    fs::remove_dir_all(root).unwrap();
}

// 样例字段逐项对照本机 0.65 ChatRecordingService（bundle 287175-287300），不包含用户内容。
const HEADER: &str = r#"{"sessionId":"abcd-1234","kind":"main","summary":"示例任务"}"#;
const USER: &str = r#"{"id":"u1","timestamp":"2026-11-12T09:00:00Z","type":"user","content":[{"text":"检查文件"}]}"#;
const CALL: &str = r#"{"id":"a1","timestamp":"2026-11-12T09:00:01Z","type":"gemini","content":"正在检查","thoughts":[{"subject":"检查","description":"读取指定文件"}],"model":"gemini-3.8-flash","toolCalls":[{"id":"c1","name":"read_file","args":{"file_path":"a.txt"}}]}"#;
const RESULT: &str = r#"{"id":"a1","timestamp":"2026-11-12T09:00:01Z","type":"gemini","content":"正在检查","model":"gemini-3.8-flash","tokens":{"input":100,"output":10,"cached":80,"thoughts":5,"tool":1,"total":116},"toolCalls":[{"id":"c1","name":"read_file","args":{"file_path":"a.txt"},"status":"success","result":[{"functionResponse":{"id":"c1","name":"read_file","response":{"output":"hello"}}}]}]}"#;

fn temp(name: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "meowo-gemini-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn append(path: &Path, text: &str) {
    fs::OpenOptions::new()
        .append(true)
        .open(path)
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}

#[test]
fn compatibility_gemini_replaces_same_id_and_supports_incremental_tool_results() {
    let root = temp("journal");
    let source = root.join("session.jsonl");
    fs::write(&source, format!("{HEADER}\n{USER}\n{CALL}\n")).unwrap();
    let first = materialize_in(&source, &root.join("cache")).unwrap();
    let delta = read_chat_delta(&TRANSCRIPT, &first, 0, None);
    assert_eq!(
        delta
            .items
            .iter()
            .filter(|item| matches!(item, ChatItem::ToolUse { .. }))
            .count(),
        1
    );
    append(&source, &format!("{RESULT}\n"));
    let second = materialize_in(&source, &root.join("cache")).unwrap();
    assert_ne!(first, second, "覆盖旧消息时必须切换路径，让宿主重置 offset");
    let items = read_chat_delta(&TRANSCRIPT, &second, 0, None).items;
    assert_eq!(
        items
            .iter()
            .filter(|item| matches!(item, ChatItem::ToolUse { .. }))
            .count(),
        1
    );
    assert!(items.iter().any(|item| matches!(item, ChatItem::ToolResult { tool_use_id: Some(id), text, .. } if id == "gemini-call-c1" && text.contains("hello"))));
    let offset = fs::metadata(&second).unwrap().len();
    append(
        &source,
        "{\"id\":\"a2\",\"type\":\"gemini\",\"content\":\"完成\"}\n",
    );
    let stable = materialize_in(&source, &root.join("cache")).unwrap();
    assert_eq!(stable, second);
    let more = read_chat_delta(&TRANSCRIPT, &stable, offset, None);
    assert!(more
        .items
        .iter()
        .any(|item| matches!(item, ChatItem::AssistantText { text, .. } if text == "完成")));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn compatibility_gemini_patch_rewind_and_set_follow_upstream_journal_rules() {
    let mut journal = Journal::default();
    for line in [HEADER, USER, CALL, RESULT] {
        journal.fold(serde_json::from_str(line).unwrap());
    }
    journal.fold(json!({"$patch":{"updates":[{"id":"a1","content":"已更改","toolCalls":[{"id":"c1","result":[{"functionResponse":{"id":"c1","response":{"output":"new"}}}]}]}]}}));
    assert_eq!(journal.messages.len(), 2);
    assert_eq!(journal.messages[1]["content"], "已更改");
    assert_eq!(
        journal.messages[1]["toolCalls"][0]["result"][0]["functionResponse"]["response"]["output"],
        "new"
    );
    journal.fold(json!({"$patch":{"orderIds":["a1","u1"]}}));
    assert_eq!(journal.messages[0]["id"], "a1");
    journal.fold(json!({"$rewindTo":"u1"}));
    assert_eq!(journal.messages.len(), 1);
    journal.fold(json!({"$set":{"messages":[{"id":"fresh","type":"user","content":"新轮"}],"summary":"新标题"}}));
    assert_eq!(journal.messages.len(), 1);
    assert_eq!(journal.messages[0]["id"], "fresh");
    journal.fold(json!({"$patch":{"removeIds":["fresh"]}}));
    assert!(journal.messages.is_empty());
    journal.fold(json!({"$rewindTo":"missing"}));
    assert!(journal.messages.is_empty());
}

#[test]
fn compatibility_gemini_analysis_title_tokens_error_and_recovery() {
    let mut parser = GeminiParser::default();
    parser.fold_line(r#"{"gemini_metadata":{"summary":"示例任务"}}"#);
    parser.fold_line(RESULT);
    let info = parser.to_info();
    assert_eq!(info.title.as_deref(), Some("示例任务"));
    assert_eq!(info.context_tokens, Some(116));
    assert_eq!(info.context_pct, None);
    assert_eq!(info.preview.as_deref(), Some("正在检查"));
    parser.fold_line(r#"{"id":"e1","type":"error","content":"API unavailable"}"#);
    assert!(parser.to_info().error.is_some());
    parser.fold_line(USER);
    assert!(parser.to_info().error.is_none());
    parser.fold_line(r#"{"id":"e2","type":"error","content":"API unavailable"}"#);
    parser.fold_line(RESULT);
    assert!(parser.to_info().error.is_none());
    assert!(
        matches!(parse_line(USER).first(), Some(TranscriptEvent::UserMessage { text, .. }) if text == "检查文件")
    );
}

#[test]
fn compatibility_gemini_locator_checks_full_id_and_prefers_jsonl() {
    let root = temp("locate");
    let chats = root.join("tmp/project/chats");
    fs::create_dir_all(&chats).unwrap();
    fs::write(
        chats.join("session-old.json"),
        format!("{{\"sessionId\":\"abcd-1234\",\"messages\":[{USER}]}}"),
    )
    .unwrap();
    let current = chats.join("session-current.jsonl");
    fs::write(&current, format!("{HEADER}\n{USER}\n")).unwrap();
    fs::write(
        chats.join("session-wrong.jsonl"),
        "{\"sessionId\":\"abcd-9999\"}\n",
    )
    .unwrap();
    fs::write(
        chats.join("session-child.jsonl"),
        "{\"sessionId\":\"child-1234\",\"kind\":\"subagent\"}\n",
    )
    .unwrap();
    assert!(find_in(&root, "child-1234").is_none());
    assert_eq!(find_in(&root, "abcd-1234"), Some(current));
    assert!(find_in(&root, "abcd").is_none());
    let large = chats.join("session-large.json");
    fs::write(&large, format!("{{\"sessionId\":\"large-1234\",\"messages\":[{{\"id\":\"u\",\"type\":\"user\",\"content\":\"{}\"}}]}}", "a".repeat(80_000))).unwrap();
    assert!(matches_session(&large, "large-1234"));
    let legacy = materialize_in(&chats.join("session-old.json"), &root.join("cache")).unwrap();
    assert_eq!(
        read_chat_delta(&TRANSCRIPT, &legacy, 0, None).items.len(),
        1
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn compatibility_gemini_partial_lines_and_pagination_use_shared_reader() {
    let root = temp("pages");
    let source = root.join("session.jsonl");
    let mut journal = format!("{HEADER}\n");
    for index in 0..8 {
        journal.push_str(
            &json!({"id":format!("u{index}"),"type":"user","content":format!("question-{index}")})
                .to_string(),
        );
        journal.push('\n');
    }
    journal.push_str("{\"id\":\"a9\",\"type\":\"gemini\",\"content\":\"partial");
    fs::write(&source, journal).unwrap();
    let first = materialize_in(&source, &root.join("cache")).unwrap();
    let page = read_chat_delta_paged(&TRANSCRIPT, &first, 0, None, 3);
    assert_eq!(page.items.len(), 3);
    assert!(page.start > 0);
    let before = crate::transcript::read_chat_window(&TRANSCRIPT, &first, page.start, 3).unwrap();
    assert_eq!(before.items.len(), 3);
    let offset = fs::metadata(&first).unwrap().len();
    append(&source, "\"}\n");
    let same = materialize_in(&source, &root.join("cache")).unwrap();
    assert_eq!(same, first);
    let more = read_chat_delta(&TRANSCRIPT, &same, offset, None);
    assert!(
        matches!(more.items.first(), Some(ChatItem::AssistantText { text, .. }) if text == "partial")
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn compatibility_gemini_source_rewrite_discards_old_messages() {
    let root = temp("rewrite");
    let source = root.join("session.jsonl");
    fs::write(&source, format!("{HEADER}\n{USER}\n")).unwrap();
    let old = materialize_in(&source, &root.join("cache")).unwrap();
    fs::write(
        &source,
        format!(
            "{HEADER}\n{{\"id\":\"fresh\",\"type\":\"user\",\"content\":\"{}\"}}\n",
            "new history ".repeat(80)
        ),
    )
    .unwrap();
    let next = materialize_in(&source, &root.join("cache")).unwrap();
    assert_ne!(next, old);
    let items = read_chat_delta(&TRANSCRIPT, &next, 0, None).items;
    assert_eq!(items.len(), 1);
    assert!(
        matches!(&items[0], ChatItem::UserText { text, .. } if text.starts_with("new history"))
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn compatibility_gemini_stop_reads_model_without_writing_a_cache() {
    let root = temp("model");
    let source = root.join("session.jsonl");
    fs::write(
        &source,
        format!(
            "{HEADER}\n{{\"id\":\"large\",\"type\":\"user\",\"content\":\"{}\"}}\n{RESULT}\n",
            "中".repeat(700_000)
        ),
    )
    .unwrap();
    let path = source.to_str().unwrap();
    let ctx = crate::caps::HookContext {
        session_id: "abcd-1234",
        transcript_path: Some(path),
        last_assistant_message: None,
    };
    assert_eq!(read_model(&ctx).as_deref(), Some("gemini-3.8-flash"));
    assert!(!root.join("cache").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "显式运行的本地性能探针，不启动 CLI 或模型"]
fn compatibility_gemini_materialization_benchmark() {
    for count in [100, 10_000] {
        let root = temp("bench");
        let source = root.join("session.jsonl");
        let mut raw = format!("{HEADER}\n");
        for index in 0..count {
            raw.push_str(&json!({"id":format!("m{index}"),"type":if index % 2 == 0 { "user" } else { "gemini" },"content":format!("真实字段形态的样例消息 {index}"),"timestamp":"2026-11-12T09:00:00Z","model":"gemini-3.8-flash","tokens":{"input":100,"output":10,"cached":80,"total":110}}).to_string());
            raw.push('\n');
        }
        fs::write(&source, &raw).unwrap();
        let clock = Instant::now();
        let mut uncached = Journal::default();
        for line in fs::read_to_string(&source).unwrap().lines() {
            uncached.fold(serde_json::from_str(line).unwrap());
        }
        let _ = uncached.snapshot();
        let uncached_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let clock = Instant::now();
        let path = materialize_in(&source, &root.join("cache")).unwrap();
        let first_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let clock = Instant::now();
        for _ in 0..100 {
            assert_eq!(
                materialize_in(&source, &root.join("cache")).as_ref(),
                Some(&path)
            );
        }
        let poll_us = clock.elapsed().as_secs_f64() * 10_000.0;
        let clock = Instant::now();
        let _ = read_chat_delta_paged(&TRANSCRIPT, &path, 0, None, 200);
        let read_page_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let old_len = fs::metadata(&path).unwrap().len();
        append(
            &source,
            "{\"id\":\"append\",\"type\":\"user\",\"content\":\"new message\"}\n",
        );
        let clock = Instant::now();
        let appended = materialize_in(&source, &root.join("cache")).unwrap();
        let append_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let append_bytes = fs::metadata(&appended).unwrap().len() - old_len;
        append(
            &source,
            "{\"$patch\":{\"updates\":[{\"id\":\"m1\",\"content\":\"patched\"}]}}\n",
        );
        let clock = Instant::now();
        let patched = materialize_in(&source, &root.join("cache")).unwrap();
        let patch_ms = clock.elapsed().as_secs_f64() * 1000.0;
        println!("gemini records={count} source_bytes={} uncached_ms={uncached_ms:.3} first_ms={first_ms:.3} unchanged_poll_us={poll_us:.3} first_page_ms={read_page_ms:.3} append_ms={append_ms:.3} append_written_bytes={append_bytes} patch_ms={patch_ms:.3} patch_new_path={}", raw.len(), patched != appended);
        let _ = fs::remove_dir_all(root);
    }
}
