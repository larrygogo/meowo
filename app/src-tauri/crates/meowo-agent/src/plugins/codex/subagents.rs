//! codex 的子代理（collaboration 工具）→ 与 claude、kimi 同一个子任务块。
//!
//! 记录里有什么（2026-11-14，本机 98 份 rollout：spawn_agent 82 次、SubAgentActivity 595 条、
//! 带 `thread_spawn` 的子线程 rollout 21 份；例：会话 01a1cea4 第 631 到 745 行）：
//! - 派出：`function_call spawn_agent`，实参 `task_name` 可读，任务正文 `message` 是加密串。
//! - 开始：`item_completed/SubAgentActivity kind=started`，`item.id` 就是 spawn_agent 的 call_id，
//!   带子线程 id（`agent_thread_id`）与路径（`agent_path`，如 `/root/lead_design_one`）。
//! - 结局：`kind=completed`（子线程跑完一轮）、`kind=interrupted`（父线程调了 interrupt_agent）。
//!   它们只带子线程 id 与路径，不带 spawn 的 call_id，所以按路径归回原委派（前端的 task_id 路由）。
//! - 过程：子线程有自己的 rollout（文件名以子线程 id 结尾，`session_meta.source.subagent.thread_spawn`），
//!   格式与主线程相同，展开时按需读。
//!
//! 记录里没有的，块上就不显示，不编：
//! - 没有「失败」这一档（SubAgentActivity 只有 started / interacted / completed / interrupted）。
//! - 子代理交回的结论在父线程里是加密的（`agent_message` 的 `encrypted_content`），读不到；
//!   可读的那份在子线程 rollout 里，展开即见。
//! - `kind=interacted`（父线程给它发了消息）之后它有没有再开工，记录不说，所以不改状态：
//!   跑完过一次、又被叫起来干活的子代理，块上仍是上一次的结局。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::transcript::{
    SubagentOutcome, SubagentRef, SubagentSpec, SubagentStream, ToolFacts, TranscriptEvent,
};

pub(super) struct CodexSubagents;

/// spawn_agent 的实参 → 子任务块的展示信息。只有任务名可读。
pub(super) fn spawn_ref(arg: &Value) -> SubagentRef {
    let text = |key: &str| arg.get(key).and_then(Value::as_str).map(str::to_string);
    SubagentRef {
        description: text("task_name").unwrap_or_default(),
        agent_type: text("agent_type"),
        count: 1,
    }
}

/// `item_completed/SubAgentActivity` → 挂在原委派上的结局回执。
pub(super) fn activity_events(item: &Value, timestamp: Option<String>) -> Vec<TranscriptEvent> {
    let text = |key: &str| item.get(key).and_then(Value::as_str);
    let (Some(id), Some(path)) = (text("id"), text("agent_path")) else {
        return Vec::new();
    };
    let outcome = |running: u32, completed: u32, interrupted: bool| SubagentOutcome {
        running,
        completed,
        failed: 0,
        background_running: false,
        task_id: Some(path.to_string()),
        interrupted,
    };
    let (call_id, outcome) = match text("kind") {
        // started 的 id 就是 spawn_agent 的 call_id：这条回执把「路径」登记到那次委派名下。
        Some("started") => (id.to_string(), outcome(1, 0, false)),
        // 之后的结局只带路径；回执 id 另取，免得顶掉别的调用自己的回执
        // （interrupted 的 id 是 interrupt_agent 那次调用的 call_id）。
        Some("completed") => (format!("{id}:subagent"), outcome(0, 1, false)),
        Some("interrupted") => (format!("{id}:subagent"), outcome(0, 0, true)),
        _ => return Vec::new(),
    };
    vec![TranscriptEvent::ToolResult {
        id: format!("codex-subagent-{call_id}"),
        timestamp,
        tool_call_id: Some(call_id),
        text: String::new(),
        is_error: false,
        subagent: Some(outcome),
        patch: None,
        facts: ToolFacts::default(),
    }]
}

/// 主线程 rollout 里这次委派的子线程：id、路径，以及记录里最后一个明确的状态。
fn spawned_thread(main_transcript: &Path, call_id: &str) -> Option<(String, String, Option<String>, Option<String>)> {
    let main = std::fs::read_to_string(main_transcript).ok()?;
    let activity = |line: &str| -> Option<(Value, Option<String>)> {
        if !line.contains("\"SubAgentActivity\"") {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let timestamp = value.get("timestamp").and_then(Value::as_str).map(str::to_string);
        let item = value.pointer("/payload/item")?.clone();
        (item.get("type").and_then(Value::as_str) == Some("SubAgentActivity")).then_some((item, timestamp))
    };
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
    let (thread, path) = main.lines().find_map(|line| {
        let (item, _) = activity(line)?;
        (field(&item, "kind").as_deref() == Some("started") && field(&item, "id").as_deref() == Some(call_id))
            .then(|| Some((field(&item, "agent_thread_id")?, field(&item, "agent_path")?)))
            .flatten()
    })?;
    let mut status = None;
    let mut finished_at = None;
    for line in main.lines() {
        let Some((item, timestamp)) = activity(line) else {
            continue;
        };
        if field(&item, "agent_thread_id").as_deref() != Some(thread.as_str()) {
            continue;
        }
        match field(&item, "kind").as_deref() {
            Some("started") => (status, finished_at) = (Some("running".to_string()), None),
            Some("completed") => (status, finished_at) = (Some("completed".to_string()), timestamp),
            // 被叫停没有对应的归一化状态（running / completed / failed 都不是）：留空，
            // 由主链回执上的「已中断」说话。
            Some("interrupted") => (status, finished_at) = (None, timestamp),
            _ => {}
        }
    }
    Some((thread, path, status, finished_at))
}

/// 子线程的 rollout：文件名以 `<子线程 id>.jsonl` 结尾。先看主线程同目录（同一天派出的都在这），
/// 再按会话 id 在 codex 的 sessions 目录里找。
fn thread_rollout(main_transcript: &Path, thread_id: &str) -> Option<PathBuf> {
    let suffix = format!("-{thread_id}.jsonl");
    let beside = main_transcript.parent().and_then(|dir| {
        std::fs::read_dir(dir).ok()?.flatten().map(|entry| entry.path()).find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
        })
    });
    beside.or_else(|| super::telemetry::find_rollout(thread_id))
}

impl SubagentSpec for CodexSubagents {
    fn detect_call(&self, tool_name: &str, input: Option<&Value>) -> Option<SubagentRef> {
        (tool_name == "spawn_agent").then(|| spawn_ref(input.unwrap_or(&Value::Null)))
    }

    fn locate_streams(&self, main_transcript: &Path, tool_use_id: &str) -> Vec<SubagentStream> {
        let Some((thread, path, status, finished_at)) = spawned_thread(main_transcript, tool_use_id) else {
            return Vec::new();
        };
        thread_rollout(main_transcript, &thread)
            .map(|rollout| SubagentStream {
                label: Some(path),
                status,
                finished_at,
                path: rollout,
            })
            .into_iter()
            .collect()
    }

    fn parse_stream_line(&self, line: &str) -> Vec<TranscriptEvent> {
        super::telemetry::parse_transcript_events(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::ChatItem;

    // 本机 rollout 01a1cea4 的 633、745、4010 行（时间戳与 id 原样，其余字段未删）。
    const STARTED: &str = r#"{"timestamp":"2026-11-12T08:59:07.391Z","type":"event_msg","payload":{"type":"item_completed","thread_id":"01a1cea4-2a7b-7981-8f01-d75ef5130a74","turn_id":"01a1cef9-2d73-7e61-888c-a21e6ac116f0","item":{"type":"SubAgentActivity","model":"gpt-6.1-sol","reasoning_effort":"medium","id":"call_f415988e6e7e4fc78b4498daa7cde2e4","kind":"started","agent_thread_id":"01a1cefb-7030-74f2-ba1e-7c74e9e51051","agent_path":"/root/lead_design_one"},"started_at_ms":1794473947391,"completed_at_ms":1794473947391}}"#;
    const COMPLETED: &str = r#"{"timestamp":"2026-11-12T09:03:37.804Z","type":"event_msg","payload":{"type":"item_completed","thread_id":"01a1cea4-2a7b-7981-8f01-d75ef5130a74","turn_id":"01a1cef9-2d73-7e61-888c-a21e6ac116f0","item":{"type":"SubAgentActivity","model":null,"reasoning_effort":null,"id":"subagent-completed-01a1cefb-70ff-7573-8598-3fa94748f8d0","kind":"completed","agent_thread_id":"01a1cefb-7030-74f2-ba1e-7c74e9e51051","agent_path":"/root/lead_design_one"},"started_at_ms":1794474217804,"completed_at_ms":1794474217804}}"#;
    const INTERACTED: &str = r#"{"timestamp":"2026-11-12T09:02:52.633Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"SubAgentActivity","model":null,"reasoning_effort":null,"id":"call_b821282853bb4600818415065f0b55fd","kind":"interacted","agent_thread_id":"01a1cefb-7030-74f2-ba1e-7c74e9e51051","agent_path":"/root/lead_design_one"}}}"#;
    const INTERRUPTED: &str = r#"{"timestamp":"2026-11-12T12:26:47.993Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"SubAgentActivity","model":null,"reasoning_effort":null,"id":"call_1d7538dee7c54a75900dd11bccee288d","kind":"interrupted","agent_thread_id":"01a1cefb-7030-74f2-ba1e-7c74e9e51051","agent_path":"/root/lead_design_one"}}}"#;

    fn items(line: &str) -> Vec<ChatItem> {
        super::super::telemetry::parse_transcript_events(line)
            .into_iter()
            .map(ChatItem::from)
            .collect()
    }

    #[test]
    fn activity_records_become_receipts_routed_by_agent_path() {
        // 开始：挂在 spawn_agent 那次调用上，登记路径。
        assert!(matches!(
            items(STARTED).as_slice(),
            [ChatItem::ToolResult { tool_use_id: Some(call), subagent: Some(outcome), .. }]
                if call == "call_f415988e6e7e4fc78b4498daa7cde2e4"
                    && outcome.running == 1
                    && outcome.task_id.as_deref() == Some("/root/lead_design_one")
        ));
        // 跑完：只带路径，前端按它归回原委派。
        assert!(matches!(
            items(COMPLETED).as_slice(),
            [ChatItem::ToolResult { tool_use_id: Some(call), subagent: Some(outcome), timestamp: Some(_), .. }]
                if call != "call_f415988e6e7e4fc78b4498daa7cde2e4"
                    && outcome.completed == 1 && outcome.running == 0
                    && outcome.task_id.as_deref() == Some("/root/lead_design_one")
        ));
        // 被叫停：三个计数都是 0，单标已中断；回执 id 不占 interrupt_agent 那次调用自己的。
        assert!(matches!(
            items(INTERRUPTED).as_slice(),
            [ChatItem::ToolResult { tool_use_id: Some(call), subagent: Some(outcome), .. }]
                if call == "call_1d7538dee7c54a75900dd11bccee288d:subagent"
                    && outcome.interrupted && outcome.running + outcome.completed + outcome.failed == 0
        ));
        // 发消息之后它开没开工，记录不说：不产出状态。
        assert!(items(INTERACTED).is_empty());
    }

    #[test]
    fn spawn_call_is_a_delegation_with_the_task_name() {
        let call = r#"{"timestamp":"2026-11-12T08:59:07.000Z","type":"response_item","payload":{"type":"function_call","name":"spawn_agent","namespace":"collaboration","arguments":"{\"task_name\":\"lead_design_one\",\"fork_turns\":\"none\",\"message\":\"gAAAAABq9X_Y\"}","call_id":"call_f415988e6e7e4fc78b4498daa7cde2e4"}}"#;
        assert!(matches!(
            items(call).as_slice(),
            [ChatItem::ToolUse { id, subagent: Some(subagent), .. }]
                if id == "call_f415988e6e7e4fc78b4498daa7cde2e4"
                    && subagent.description == "lead_design_one" && subagent.count == 1
        ));
        // 协作类的其它工具不是委派。
        let wait = r#"{"type":"response_item","payload":{"type":"function_call","name":"wait_agent","arguments":"{\"timeout_ms\":10000}","call_id":"c2"}}"#;
        assert!(matches!(items(wait).as_slice(), [ChatItem::ToolUse { subagent: None, .. }]));
    }

    /// 展开时读的是子线程自己的 rollout：按 started 记录里的子线程 id 找文件。
    #[test]
    fn locates_the_child_thread_rollout_and_its_last_known_state() {
        let dir = std::env::temp_dir().join(format!("meowo_codex_subagents_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("rollout-2026-11-12T15-23-47-01a1cea4-2a7b-7981-8f01-d75ef5130a74.jsonl");
        let child = dir.join("rollout-2026-11-12T16-59-07-01a1cefb-7030-74f2-ba1e-7c74e9e51051.jsonl");
        std::fs::write(&child, r#"{"timestamp":"2026-11-12T08:59:20.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"m1","content":[{"type":"text","text":"方案一写好了"}]}}}"#).unwrap();
        let call = "call_f415988e6e7e4fc78b4498daa7cde2e4";

        std::fs::write(&main, format!("{STARTED}\n")).unwrap();
        let streams = CodexSubagents.locate_streams(&main, call);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].path, child);
        assert_eq!(streams[0].label.as_deref(), Some("/root/lead_design_one"));
        assert_eq!(streams[0].status.as_deref(), Some("running"));

        std::fs::write(&main, format!("{STARTED}\n{INTERACTED}\n{COMPLETED}\n")).unwrap();
        let streams = CodexSubagents.locate_streams(&main, call);
        assert_eq!(streams[0].status.as_deref(), Some("completed"));
        assert_eq!(streams[0].finished_at.as_deref(), Some("2026-11-12T09:03:37.804Z"));
        let runs = crate::transcript::read_subagent_chat(&super::super::telemetry::CodexTranscript, &main, call);
        assert!(matches!(
            runs.as_slice(),
            [run] if run.status.as_deref() == Some("completed")
                && matches!(run.items.as_slice(), [ChatItem::AssistantText { text, .. }] if text == "方案一写好了")
        ));

        std::fs::write(&main, format!("{STARTED}\n{COMPLETED}\n{INTERRUPTED}\n")).unwrap();
        assert_eq!(CodexSubagents.locate_streams(&main, call)[0].status, None);
        // 不是委派的调用没有侧车。
        assert!(CodexSubagents.locate_streams(&main, "call_other").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
