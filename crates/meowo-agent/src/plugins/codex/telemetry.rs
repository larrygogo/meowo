//! codex（OpenAI Codex CLI）会话解析。codex 的 hooks 与 claude 同款：Stop hook 直带
//! `last_assistant_message`（故最近 AI 正文走 hook payload，不在此读），标题靠首条 prompt 命名
//! （rollout 首条 user 文本被 AGENTS.md/指令包裹，不适合当标题）。唯一需从会话文件补的是【模型】
//! ——Stop hook 不携带模型，需读 rollout 的 `turn_context.model`。
//!
//! rollout：`{CODEX_HOME 或 ~/.codex}/sessions/<YYYY>/<MM>/<DD>/rollout-<ISO>-<session_uuid>.jsonl`，
//! 每行一个事件 `{type, payload}`。首行 `type=session_meta`；其后 `type=turn_context` 的
//! `payload.model` 即模型（如 "gpt-5.5"），通常在文件靠前（首回合）。

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::transcript::ChatItem;
use crate::transcript::{ChatOnlyParser, TranscriptEvent, TranscriptParser, TranscriptSpec};

fn chat_id(prefix: &str, line: &str) -> String {
    let hash = line.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("codex-{prefix}-{hash:016x}")
}

fn content_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .or_else(|| part.get("input_text"))
                    .or_else(|| part.get("output_text"))
                    .and_then(|text| text.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

fn parse_transcript_events(line: &str) -> Vec<TranscriptEvent> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    let timestamp = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let payload = value.get("payload").unwrap_or(&value);
    let outer = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let kind = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match (outer, kind) {
        ("event_msg", "user_message") => {
            let text = payload.get("message").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::UserMessage {
                    id: chat_id("user", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("event_msg", "agent_message") => {
            let text = payload.get("message").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::AssistantMessage {
                    id: chat_id("assistant", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("event_msg", "agent_reasoning") => {
            let text = payload
                .get("text")
                .or_else(|| payload.get("message"))
                .map(content_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::Reasoning {
                    id: chat_id("reasoning", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("response_item", "reasoning") => {
            let text = payload
                .get("summary")
                .or_else(|| payload.get("content"))
                .or_else(|| payload.get("text"))
                .map(content_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::Reasoning {
                    id: chat_id("reasoning", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let summary = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .map(content_text)
                .unwrap_or_default();
            vec![TranscriptEvent::ToolCall {
                id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| chat_id("tool", line)),
                timestamp,
                name,
                summary,
                // codex 当前没有子任务：工具集只有 exec/wait，`task_started` 是回合级事件
                // （payload 带 turn_id / model_context_window）而非委派。
                subagent: None,
            }]
        }
        ("response_item", "function_call_output")
        | ("response_item", "custom_tool_call_output") => {
            vec![TranscriptEvent::ToolResult {
                // codex 没有子任务概念（见 mod.rs 的 subagents 说明）。
                subagent: None,
                id: chat_id("result", line),
                timestamp,
                tool_call_id: payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                text: payload.get("output").map(content_text).unwrap_or_default(),
                is_error: payload
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            }]
        }
        ("event_msg", "context_compacted") => vec![TranscriptEvent::Metadata {
            id: chat_id("compact", line),
            timestamp,
            kind: "compact".into(),
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn parse_chat_items(line: &str) -> Vec<ChatItem> {
    parse_transcript_events(line)
        .into_iter()
        .map(ChatItem::from)
        .collect()
}

pub struct CodexTranscript;
pub static CODEX_TRANSCRIPT: CodexTranscript = CodexTranscript;

impl TranscriptSpec for CodexTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::new(ChatOnlyParser)
    }

    fn resolve_transcript_path(
        &self,
        transcript_path: Option<&str>,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<PathBuf> {
        transcript_path
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| find_rollout(session_id))
    }

    fn resolve_title(
        &self,
        _transcript_path: Option<&str>,
        _cwd: Option<&str>,
        _session_id: &str,
    ) -> Option<String> {
        None
    }

    fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
        parse_transcript_events(line)
    }

    fn agent_modes_from_line(&self, line: &str) -> Vec<crate::AgentMode> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return Vec::new();
        };
        let outer = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&value);
        let mut modes = Vec::new();
        if outer == "turn_context" {
            if let Some(mode) = payload
                .pointer("/collaboration_mode/mode")
                .and_then(|v| v.as_str())
            {
                modes.push(crate::AgentMode::new("collaboration", mode));
            }
            if let Some(approval) = payload.get("approval_policy").and_then(|v| v.as_str()) {
                modes.push(crate::AgentMode::new("approval", approval));
            }
            if let Some(sandbox) = payload
                .pointer("/sandbox_policy/type")
                .and_then(|v| v.as_str())
            {
                modes.push(crate::AgentMode::new("sandbox", sandbox));
            }
        } else if outer == "event_msg"
            && payload.get("type").and_then(|v| v.as_str()) == Some("task_started")
        {
            if let Some(mode) = payload
                .get("collaboration_mode_kind")
                .and_then(|v| v.as_str())
            {
                modes.push(crate::AgentMode::new("collaboration", mode));
            }
        }
        modes
    }

    fn supports_chat(&self) -> bool {
        true
    }

    fn supports_analysis(&self) -> bool {
        false
    }
}

/// codex 在本机的实况（数据目录/hooks 规格/凭据/启动 argv）。变体表见
/// `meowo_agent::plugins::codex`：数据目录 `CODEX_HOME` 优先，否则 `~/.codex`。
/// 检测/接线/状态/账号/会话读取全部经此一处解析路径。
pub fn codex_install() -> Option<crate::Installation> {
    crate::registry::installation(crate::id::CODEX)
}

/// codex 数据根。`codex_install()` 的便捷取值。
pub fn codex_home() -> Option<PathBuf> {
    codex_install().map(|i| i.data_dir)
}

/// codex 的启动前缀 argv（不含 `resume <id>`）：bun 全局 exe ／ `node <codex.js>` ／ 独立安装 exe。
/// 都没有则 None（调用方回退裸名 codex）。优先级与理由见变体表的 `LAUNCH`。
pub fn codex_launch_prefix() -> Option<Vec<String>> {
    codex_install()?.launch
}

/// 在 `~/.codex/sessions` 下按 session_id 找 rollout 文件（文件名内嵌 uuid，以 `<uuid>.jsonl` 结尾）。
/// 递归 walk 年/月/日（限深，避免误入无关深目录）。仅作 transcript_path 缺失时的兜底。
fn find_rollout(session_id: &str) -> Option<PathBuf> {
    let sessions = codex_home()?.join("sessions");
    let suffix = format!("{session_id}.jsonl");
    walk_find(&sessions, &suffix, 6)
}

fn walk_find(dir: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            return Some(p);
        }
    }
    for d in subdirs {
        if let Some(f) = walk_find(&d, suffix, depth - 1) {
            return Some(f);
        }
    }
    None
}

/// 读文件前 max_lines 行为一个 String（模型在文件靠前，无需读全量）。
fn read_head_lines(path: &Path, max_lines: usize) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut out = String::new();
    for line in BufReader::new(f)
        .lines()
        .take(max_lines)
        .map_while(Result::ok)
    {
        out.push_str(&line);
        out.push('\n');
    }
    Some(out)
}

/// 纯解析：从 rollout 文本取第一条 `turn_context` 的 `payload.model`。便于单测，不碰文件系统。
pub fn parse_model(content: &str) -> Option<String> {
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("turn_context") {
            if let Some(m) = v
                .get("payload")
                .and_then(|p| p.get("model"))
                .and_then(|m| m.as_str())
                .filter(|s| !s.is_empty())
            {
                return Some(m.to_string());
            }
        }
    }
    None
}

/// 取某 codex 会话的模型展示名：优先用 hook 给的 transcript_path，否则按 session_id 在 sessions 下找。
/// 读 rollout 前若干行解析 `turn_context.model`。定位/解析失败返回 None（卡片模型留空，不阻断）。
pub fn read_model(transcript_path: Option<&str>, session_id: &str) -> Option<String> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    // turn_context 在首回合、文件靠前；读前 200 行足够，避免长会话全量读。
    let head = read_head_lines(&path, 200)?;
    parse_model(&head)
}

/// 从 rollout 文本取**最后一条 info 非 null** 的 token_count 的 (input_tokens, model_context_window)。
/// codex 会话开头的 token_count `info` 为 null（只有 rate_limits），跳过。used 取 last_token_usage.input_tokens
/// （最近一次请求的 context 输入量，已含 cached_input_tokens）。
pub fn parse_context(content: &str) -> Option<(i64, i64)> {
    let mut last: Option<(i64, i64)> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = v.get("payload");
        if payload.and_then(|p| p.get("type")).and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.and_then(|p| p.get("info")).filter(|i| !i.is_null()) else {
            continue;
        };
        let used = info
            .get("last_token_usage")
            .and_then(|l| l.get("input_tokens"))
            .and_then(|x| x.as_i64());
        let window = info.get("model_context_window").and_then(|x| x.as_i64());
        if let (Some(u), Some(w)) = (used, window) {
            last = Some((u, w));
        }
    }
    last
}

/// 读文件尾部最多 max_bytes 字节为 lossy UTF-8（首个半截行交给 parse_context 跳过）。
/// `pub(super)`：account 的 token_count 尾部扫描复用同一份有界读——rollout 可达数十 MB，
/// 绝不能整个读进内存。
pub(super) fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(max_bytes)))
        .ok()?;
    let mut buf = Vec::new();
    f.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// codex 会话最近上下文占用：定位 rollout（hook 的 transcript_path 优先，否则按 id 找），
/// 尾部读取最后一条 token_count。定位/解析失败返回 None。
pub fn read_context(
    transcript_path: Option<&str>,
    session_id: &str,
) -> Option<crate::caps::ContextUsage> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    const TAIL_BYTES: u64 = 256 * 1024;
    let text = read_tail(&path, TAIL_BYTES)?;
    let (used, window) = parse_context(&text)?;
    if window <= 0 {
        return None;
    }
    let pct = (used * 100 / window).clamp(0, 100);
    Some(crate::caps::ContextUsage {
        used_pct: pct,
        window,
    })
}

// ═══ 能力槽 ═══

pub struct CodexTelemetry;
pub static TELEMETRY: CodexTelemetry = CodexTelemetry;

impl crate::caps::TelemetryCap for CodexTelemetry {
    /// codex 的 Stop hook 直带 AI 正文（同 claude）；模型 Stop 不带，从 rollout 的 turn_context 补。
    fn stop_outputs(&self, ctx: &crate::caps::HookContext) -> crate::caps::StopOutputs {
        crate::caps::StopOutputs {
            last_ai: ctx.last_assistant_message.map(str::to_string),
            model: read_model(ctx.transcript_path, ctx.session_id),
        }
    }

    fn read_context(&self, ctx: &crate::caps::HookContext) -> Option<crate::caps::ContextUsage> {
        read_context(ctx.transcript_path, ctx.session_id)
    }

    fn transcript(&self) -> Option<&'static dyn TranscriptSpec> {
        Some(&CODEX_TRANSCRIPT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_collaboration_approval_and_sandbox_dimensions() {
        let modes = CODEX_TRANSCRIPT.agent_modes_from_line(
            r#"{"type":"turn_context","payload":{"approval_policy":"on-request","sandbox_policy":{"type":"workspace-write"},"collaboration_mode":{"mode":"plan"}}}"#,
        );
        assert_eq!(
            modes,
            vec![
                crate::AgentMode::new("collaboration", "plan"),
                crate::AgentMode::new("approval", "on-request"),
                crate::AgentMode::new("sandbox", "workspace-write"),
            ]
        );
        assert_eq!(
            CODEX_TRANSCRIPT.agent_modes_from_line(
                r#"{"type":"event_msg","payload":{"type":"task_started","collaboration_mode_kind":"default"}}"#,
            ),
            vec![crate::AgentMode::new("collaboration", "default")]
        );
    }

    #[test]
    fn parse_context_takes_last_nonnull_token_count() {
        let rollout = r#"
{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":7.0}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":6766,"cached_input_tokens":4480},"model_context_window":258400}}}
"#;
        // 跳过 info=null 那条；取最后一条 info 非 null 的：input_tokens=6766, window=258400。
        assert_eq!(parse_context(rollout), Some((6766, 258400)));
    }

    #[test]
    fn parse_context_none_when_no_token_count() {
        assert_eq!(
            parse_context(r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#),
            None
        );
    }

    #[test]
    fn parse_model_takes_first_turn_context() {
        let rollout = r#"
{"type":"session_meta","payload":{"id":"x","cwd":"/p","model_provider":"openai"}}
{"type":"turn_context","payload":{"model":"gpt-5.5","cwd":"/p","effort":"medium"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}
{"type":"turn_context","payload":{"model":"gpt-5.3-codex"}}
"#;
        assert_eq!(parse_model(rollout).as_deref(), Some("gpt-5.5"));
    }

    #[test]
    fn parse_model_none_when_absent() {
        let rollout = r#"{"type":"session_meta","payload":{"id":"x"}}
{"type":"turn_context","payload":{"cwd":"/p"}}"#;
        assert_eq!(parse_model(rollout), None);
        assert_eq!(parse_model(""), None);
    }

    #[test]
    fn chat_parser_uses_clean_events_and_tool_records_without_duplicates() {
        let user = r#"{"timestamp":"t1","type":"event_msg","payload":{"type":"user_message","message":"修复测试"}}"#;
        let assistant = r#"{"timestamp":"t2","type":"event_msg","payload":{"type":"agent_message","message":"开始处理"}}"#;
        let tool = r#"{"timestamp":"t3","type":"response_item","payload":{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"cargo test\"}","call_id":"c1"}}"#;
        let reasoning = r#"{"timestamp":"t2","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"先运行测试"}]}}"#;
        assert!(
            matches!(&parse_chat_items(user)[0], ChatItem::UserText { text, .. } if text == "修复测试")
        );
        assert!(
            matches!(&parse_chat_items(assistant)[0], ChatItem::AssistantText { text, .. } if text == "开始处理")
        );
        assert!(
            matches!(&parse_chat_items(tool)[0], ChatItem::ToolUse { name, summary, .. } if name == "shell_command" && summary.contains("cargo test"))
        );
        assert!(matches!(
            &parse_chat_items(reasoning)[0],
            ChatItem::Reasoning { text, .. } if text == "先运行测试"
        ));
        // 原始 response_item user message 常含指令包，与 event_msg.user_message 重复，必须跳过。
        assert!(parse_chat_items(
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#
        )
        .is_empty());
    }
}
