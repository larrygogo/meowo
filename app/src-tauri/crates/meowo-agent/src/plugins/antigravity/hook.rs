//! agy hook 负载 → Meowo 规范形状（claude 同款 snake_case）。
//!
//! 依据是真机探针（agy 1.5.18，2026-11-26，任务目录 agents/probe-hooks-1.md）：
//! - 负载是 camelCase：`hookEventName`、`conversationId`、`transcriptPath`、`lastUserInput`。
//! - `cwd`、`workspacePaths`、`toolCall` 的值是 **JSON 编码后的字符串**（cwd 实拍为
//!   `"\"C:\\\\Users\\\\…\""`），要再解一层。
//! - 没有「用户提交」事件：`PreInvocation` 在每次模型调用前触发，`invocationNum` 每个用户回合从 0
//!   开始（第二轮又是 0），所以 `invocationNum == 0` 那一次才是用户提交，正文在 `lastUserInput`。
//! - `SessionStart` 不在启动时触发，而在第一条消息提交时（负载已带 lastUserInput）。
//! - hooks.json 在 `~/.gemini/config/`，Antigravity App 与 IDE 也读它：那边的会话同样会走进来，
//!   数据目录是 `~/.gemini/antigravity/` 而不是 `antigravity-cli/`，按 `artifactDirectoryPath` /
//!   `transcriptPath` 所在目录分辨，不是 CLI 的一律不管。

use serde_json::{json, Map, Value};

use crate::HookPayload;

/// CLI 数据目录的目录名（`~/.gemini/antigravity-cli`）；App 是 `antigravity`。
const CLI_DIR_NAME: &str = "antigravity-cli";

pub(super) fn normalize(raw: &str) -> HookPayload {
    let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(raw) else {
        // 不是 agy 的负载形状（比如测试直接喂规范形状）：原样交给 reporter。
        return HookPayload::Keep;
    };
    let Some(event) = payload.get("hookEventName").and_then(Value::as_str) else {
        return HookPayload::Keep;
    };
    if !from_cli(&payload) {
        return HookPayload::Ignore;
    }
    // 子代理是单独的对话（research-transcript.md §5），它的事件若当成独立会话落库，看板会凭空多出
    // 卡片。负载里带父对话 id 的一律不管；探针里主对话的负载没有这个字段。
    if str_field(&payload, "parentConversationId").is_some_and(|p| !p.is_empty()) {
        return HookPayload::Ignore;
    }
    let canonical = match event {
        "SessionStart" => "SessionStart",
        "PreInvocation" => {
            if payload.get("invocationNum").and_then(Value::as_i64) != Some(0) {
                return HookPayload::Ignore;
            }
            "UserPromptSubmit"
        }
        "PreToolUse" => "PreToolUse",
        "PostToolUse" => "PostToolUse",
        "Stop" => "Stop",
        _ => return HookPayload::Ignore,
    };
    let Some(session_id) = str_field(&payload, "conversationId").filter(|s| !s.is_empty()) else {
        return HookPayload::Ignore;
    };
    let mut out = Map::new();
    out.insert("hook_event_name".into(), json!(canonical));
    out.insert("session_id".into(), json!(session_id));
    let cwd = decoded(&payload, "cwd")
        .and_then(|v| v.as_str().map(str::to_string))
        .or_else(|| {
            decoded(&payload, "workspacePaths")?
                .as_array()?
                .first()?
                .as_str()
                .map(str::to_string)
        });
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        out.insert("cwd".into(), json!(cwd));
    }
    if let Some(path) = str_field(&payload, "transcriptPath") {
        out.insert("transcript_path".into(), json!(path));
    }
    if canonical == "UserPromptSubmit" {
        if let Some(prompt) = str_field(&payload, "lastUserInput") {
            out.insert("prompt".into(), json!(prompt));
        }
    }
    if let Some(call) = decoded(&payload, "toolCall") {
        if let Some(name) = call.get("name").and_then(Value::as_str) {
            out.insert("tool_name".into(), json!(name));
        }
        if let Some(args) = call.get("args") {
            out.insert("tool_input".into(), args.clone());
        }
    }
    if canonical == "PostToolUse" {
        if let Some(error) = str_field(&payload, "error").filter(|e| !e.is_empty()) {
            out.insert("tool_response".into(), json!({"error": error}));
        }
    }
    HookPayload::Rewrite(Value::Object(out).to_string())
}

fn str_field<'a>(payload: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(Value::as_str)
}

/// 字段值是 JSON 编码后的字符串时解开一层；本来就是结构化值的原样返回（防 agy 哪天改成直接输出）。
fn decoded(payload: &Map<String, Value>, key: &str) -> Option<Value> {
    match payload.get(key)? {
        Value::String(text) => match serde_json::from_str::<Value>(text) {
            Ok(value) => Some(value),
            // 不是 JSON（裸路径）：当普通字符串。
            Err(_) => Some(Value::String(text.clone())),
        },
        other => Some(other.clone()),
    }
}

/// 事件来自 CLI，而不是共用 hooks.json 的 App / IDE。两个路径字段都没有时不能判断，按 CLI 处理：
/// 这时漏收比错收代价大（CLI 会话整段失联）。
fn from_cli(payload: &Map<String, Value>) -> bool {
    let paths: Vec<&str> = ["artifactDirectoryPath", "transcriptPath"]
        .iter()
        .filter_map(|key| str_field(payload, key))
        .filter(|p| !p.is_empty())
        .collect();
    paths.is_empty()
        || paths.iter().any(|path| {
            path.split(['/', '\\'])
                .any(|part| part.eq_ignore_ascii_case(CLI_DIR_NAME))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 探针实拍的负载形状（路径换成占位）。
    fn sample(event: &str, extra: Value) -> String {
        let mut base = json!({
            "artifactDirectoryPath": "C:\\Users\\u\\.gemini\\antigravity-cli\\brain\\conv-1",
            "conversationId": "conv-1",
            "cwd": "\"C:\\\\work\\\\repo\"",
            "hookEventName": event,
            "lastUserInput": "say ok",
            "modelName": "gemini-4-neon-high",
            "transcriptPath": "C:\\Users\\u\\.gemini\\antigravity-cli\\brain\\conv-1\\.system_generated\\logs\\transcript_full.jsonl",
            "workspacePaths": "[\"C:\\\\work\\\\repo\"]",
        });
        for (k, v) in extra.as_object().unwrap() {
            base[k] = v.clone();
        }
        base.to_string()
    }

    fn rewritten(raw: &str) -> Value {
        match normalize(raw) {
            HookPayload::Rewrite(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("应改写，实际 {other:?}"),
        }
    }

    #[test]
    fn session_start_decodes_json_string_cwd() {
        let out = rewritten(&sample("SessionStart", json!({})));
        assert_eq!(out["hook_event_name"], "SessionStart");
        assert_eq!(out["session_id"], "conv-1");
        assert_eq!(out["cwd"], "C:\\work\\repo");
    }

    #[test]
    fn first_invocation_of_a_turn_is_the_user_prompt() {
        let out = rewritten(&sample("PreInvocation", json!({"invocationNum": 0, "initialNumSteps": 7})));
        assert_eq!(out["hook_event_name"], "UserPromptSubmit");
        assert_eq!(out["prompt"], "say ok");
        // 同一回合里后续的模型调用不是新的提交。
        assert_eq!(
            normalize(&sample("PreInvocation", json!({"invocationNum": 2}))),
            HookPayload::Ignore
        );
        assert_eq!(normalize(&sample("PostInvocation", json!({"invocationNum": 0}))), HookPayload::Ignore);
    }

    #[test]
    fn tool_call_string_becomes_tool_name_and_input() {
        let call = "{\"args\": {\"CommandLine\": \"echo hi\", \"WaitMsBeforeAsync\": 5000}, \"id\": \"call_1\", \"name\": \"run_command\"}";
        let out = rewritten(&sample("PostToolUse", json!({"toolCall": call, "stepIdx": 2, "error": ""})));
        assert_eq!(out["hook_event_name"], "PostToolUse");
        assert_eq!(out["tool_name"], "run_command");
        assert_eq!(out["tool_input"]["CommandLine"], "echo hi");
        assert!(out.get("tool_response").is_none(), "空 error 不算失败");
    }

    #[test]
    fn stop_maps_to_stop() {
        let out = rewritten(&sample(
            "Stop",
            json!({"terminationReason": "NO_TOOL_CALL", "fullyIdle": true, "continuationActive": false}),
        ));
        assert_eq!(out["hook_event_name"], "Stop");
    }

    /// App / IDE 的会话也走同一份 hooks.json（数据目录是 antigravity 而不是 antigravity-cli）。
    #[test]
    fn app_and_ide_sessions_are_ignored() {
        let raw = sample(
            "SessionStart",
            json!({
                "artifactDirectoryPath": "C:\\Users\\u\\.gemini\\antigravity\\brain\\conv-9",
                "transcriptPath": "C:\\Users\\u\\.gemini\\antigravity\\brain\\conv-9\\.system_generated\\logs\\transcript_full.jsonl",
            }),
        );
        assert_eq!(normalize(&raw), HookPayload::Ignore);
    }

    #[test]
    fn subagent_conversations_are_ignored() {
        let raw = sample("Stop", json!({"parentConversationId": "conv-0"}));
        assert_eq!(normalize(&raw), HookPayload::Ignore);
    }

    #[test]
    fn non_agy_payloads_pass_through() {
        assert_eq!(normalize("{\"hook_event_name\":\"Stop\",\"session_id\":\"x\"}"), HookPayload::Keep);
        assert_eq!(normalize("not json"), HookPayload::Keep);
    }
}
