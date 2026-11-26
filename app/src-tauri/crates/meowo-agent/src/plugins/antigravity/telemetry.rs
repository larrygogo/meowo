//! agy 的遥测：结构化对话、Stop 正文/模型、上下文占用，都从它的对话库读。
//!
//! 对话页读的是 [`store::materialize`] 导出的 JSONL 缓存，行结构由我们自己定（见 store.rs），
//! 这里只负责把几种行（`user` / `assistant` / `tool` / `subagent` / `error` / `compact` /
//! `system`）映射成领域事件。
//!
//! 上下文占用直接可算：每次模型调用的 `context_window_metadata` 同时给了已用量和窗口大小
//! （样本 22958 / 512000），不像 opencode 只有 token 数。

use serde_json::Value;

use crate::{
    caps::{HookContext, StopOutputs, TelemetryCap},
    transcript::{
        CompactionDetail, ToolDetail, ToolFacts, ToolKind, TranscriptEvent, TranscriptInfo,
        TranscriptParser, TranscriptSpec, TurnError,
    },
};

use super::store;

pub static TELEMETRY: AntigravityTelemetry = AntigravityTelemetry;
pub static TRANSCRIPT: AntigravityTranscript = AntigravityTranscript;

pub struct AntigravityTelemetry;

impl TelemetryCap for AntigravityTelemetry {
    /// Stop 负载里没有回复正文（真机探针：finalModelOutput / modelOutput 都不输出，
    /// probe-hooks-1.md 第 8 条），从库里读。
    fn stop_outputs(&self, ctx: &HookContext) -> StopOutputs {
        let (last_ai, model) = store::last_reply(ctx.session_id).unwrap_or_default();
        StopOutputs {
            last_ai,
            model,
            effort: None,
        }
    }

    fn resolves_transcript_title(&self) -> bool {
        true
    }

    fn transcript_title_generated(&self) -> bool {
        true
    }

    fn transcript(&self) -> Option<&'static dyn TranscriptSpec> {
        Some(&TRANSCRIPT)
    }
}

pub struct AntigravityTranscript;

/// 看板分析解析器：会话级出错与上下文占用。出错之后只要又有新的用户消息或回复，就说明已经
/// 越过那次出错，清掉（与 opencode 同一判据）。
#[derive(Default)]
struct AntigravityParser {
    error: Option<TurnError>,
    context_tokens: Option<u64>,
    context_pct: Option<u8>,
    preview: Option<String>,
}

impl TranscriptParser for AntigravityParser {
    fn fold_line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        match value.get("kind").and_then(Value::as_str) {
            Some("error") => {
                if let Some(error) = turn_error(&value) {
                    self.error = Some(error);
                }
            }
            Some("user") => self.error = None,
            Some("assistant") => {
                self.error = None;
                let tokens = value.get("context_tokens").and_then(Value::as_u64);
                if let Some(tokens) = tokens {
                    self.context_tokens = Some(tokens);
                    self.context_pct = value
                        .get("context_window")
                        .and_then(Value::as_u64)
                        .filter(|w| *w > 0)
                        .map(|w| (tokens.saturating_mul(100) / w).min(100) as u8);
                }
                let text = value.get("text").and_then(Value::as_str).unwrap_or_default();
                if !text.trim().is_empty() {
                    self.preview = crate::transcript::preview_text(text);
                }
            }
            _ => {}
        }
    }

    fn to_info(&self) -> TranscriptInfo {
        TranscriptInfo {
            error: self.error.clone(),
            context_tokens: self.context_tokens,
            context_pct: self.context_pct,
            preview: self.preview.clone(),
            ..TranscriptInfo::default()
        }
    }
}

/// 看板轮询的导出节流（见 [`crate::transcript::PolledPaths`]）：「定位」对 agy 意味着开库、比指纹、
/// 必要时增量导出。出错徽标与上下文占用容忍几秒延迟。
static POLLED_EXPORTS: crate::transcript::PolledPaths =
    crate::transcript::PolledPaths::new(std::time::Duration::from_secs(5));

impl TranscriptSpec for AntigravityTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::new(AntigravityParser::default())
    }

    fn resolve_transcript_path_polled(
        &self,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        POLLED_EXPORTS.get_or_resolve(session_id, || store::materialize(session_id))
    }

    /// 定位 = 导出：每次调用把库里新定稿的内容追加进缓存（库没动时只比一次 mtime）。hook 负载里的
    /// transcriptPath 指向 agy 自己的 transcript_full.jsonl，那份缺工具名、长内容截断，不用。
    fn resolve_transcript_path(
        &self,
        _transcript_path: Option<&str>,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        store::materialize(session_id)
    }

    /// agy 自己起的标题（conversation_summaries.db；还没起好是空串，不采）。
    fn resolve_title(&self, _: Option<&str>, _: Option<&str>, session_id: &str) -> Option<String> {
        store::conversation_title(session_id)
    }

    fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
        parse_line(line)
    }

    fn supports_chat(&self) -> bool {
        true
    }

    /// 长会话首开只解析尾部一屏：导出行逐行独立（parse_line 只看本行），不产出模式与计划。
    fn tail_read(&self) -> Option<crate::transcript::TailRead> {
        Some(crate::transcript::TailRead {
            mode_markers: &[],
            plan_markers: &[],
        })
    }

    fn supports_analysis(&self) -> bool {
        true
    }
}

/// epoch 毫秒 → ISO-8601 UTC（`ChatItem.timestamp` 与 claude 同形）。
fn iso_from_ms(ms: i64) -> String {
    let secs = crate::codec::unix_to_iso8601(ms.div_euclid(1000));
    let millis = ms.rem_euclid(1000);
    format!("{}.{millis:03}Z", secs.trim_end_matches('Z'))
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn parse_line(line: &str) -> Vec<TranscriptEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let Some(idx) = value.get("idx").and_then(Value::as_i64) else {
        return Vec::new();
    };
    let id = format!("step-{idx}");
    let timestamp = value.get("ts").and_then(Value::as_i64).map(iso_from_ms);
    match value.get("kind").and_then(Value::as_str) {
        Some("user") => user_events(&id, timestamp, &value),
        Some("assistant") => assistant_events(&id, timestamp, &value),
        Some("tool") => tool_events(timestamp, &value),
        Some("subagent") => subagent_events(&id, timestamp, &value),
        Some("error") => turn_error(&value)
            .map(|error| TranscriptEvent::TurnError {
                id,
                timestamp,
                label: error.label,
                text: error.raw,
            })
            .into_iter()
            .collect(),
        Some("compact") => vec![TranscriptEvent::Metadata {
            id,
            timestamp,
            kind: "compact".into(),
            compaction: Some(CompactionDetail::default()),
        }],
        _ => Vec::new(),
    }
}

fn user_events(id: &str, timestamp: Option<String>, value: &Value) -> Vec<TranscriptEvent> {
    let mut lines = vec![str_of(value, "text").to_string()];
    for (index, image) in value
        .get("images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        if let Some(path) = persist_image(id, index, image) {
            lines.push(format!("[Image: source: {}]", path.display()));
        }
    }
    let text = lines.join("\n").trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }
    vec![TranscriptEvent::UserMessage {
        id: id.to_string(),
        timestamp,
        text,
        // 运行中发出、排进 agy 自己队列的消息（is_queued_message）。
        interjected: value.get("queued").and_then(Value::as_bool).unwrap_or(false),
        turn_id: None,
    }]
}

/// 粘贴的截图在库里是 base64（ImageData.base64_data）。落盘后补 `[Image: source: …]` 行，与
/// claude/kimi 同形，前端缩略图链路直接接住。按 step + 序号命名做幂等。
fn persist_image(id: &str, index: usize, image: &Value) -> Option<std::path::PathBuf> {
    use crate::fsutil::PASTE_MAX_BYTES;
    let data = image.get("data")?.as_str()?;
    let ext = match image.get("mime").and_then(Value::as_str)? {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    if data.len() > PASTE_MAX_BYTES / 3 * 4 + 4 {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .ok()?;
    crate::fsutil::persist_paste_bytes(&format!("antigravity-{id}-{index}"), ext, &bytes)
}

fn assistant_events(id: &str, timestamp: Option<String>, value: &Value) -> Vec<TranscriptEvent> {
    let mut events = Vec::new();
    let thinking = str_of(value, "thinking");
    if !thinking.trim().is_empty() {
        events.push(TranscriptEvent::Reasoning {
            id: format!("{id}:thinking"),
            timestamp: timestamp.clone(),
            text: thinking.trim().to_string(),
        });
    }
    let text = str_of(value, "text");
    if !text.trim().is_empty() {
        events.push(TranscriptEvent::AssistantMessage {
            id: id.to_string(),
            timestamp,
            text: text.to_string(),
        });
    }
    events
}

fn tool_events(timestamp: Option<String>, value: &Value) -> Vec<TranscriptEvent> {
    let name = str_of(value, "name");
    let name = if name.is_empty() { "tool" } else { name };
    let call_id = str_of(value, "id").to_string();
    let args = value.get("args").filter(|v| !v.is_null());
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty());
    let image_call = (name == SEND_IMAGE)
        .then(|| crate::images::send_image_call(args.unwrap_or(&Value::Null)));
    let (summary, detail) = match image_call {
        Some((_, summary, detail)) => (summary, detail),
        None => (tool_summary(name, args, title), tool_detail(name, args)),
    };
    let call = TranscriptEvent::ToolCall {
        id: call_id.clone(),
        timestamp: timestamp.clone(),
        kind: tool_kind(name),
        summary,
        detail,
        name: crate::images::canonical_meowo_tool(name, MEOWO_PREFIX),
        subagent: None,
        plan: None,
    };
    let (text, is_error, interrupted) = match str_of(value, "status") {
        "error" => {
            let error = value
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or_default();
            // 用户在审批面板上拒绝：short_error 以 "permission check failed" 开头、带
            // "user denied permission"（research-transcript.md §3 审批一节的真机样本）。这是
            // 用户的选择，不是工具失败，按被打断显示，不标红。
            if error.contains("user denied permission") {
                (String::new(), false, true)
            } else {
                let output = str_of(value, "output");
                let text = if error.is_empty() { output } else { error };
                (text.to_string(), true, false)
            }
        }
        "interrupted" => (String::new(), false, true),
        _ => (str_of(value, "output").to_string(), false, false),
    };
    let ended = value
        .get("end")
        .and_then(Value::as_i64)
        .map(iso_from_ms)
        .or(timestamp);
    vec![
        call,
        TranscriptEvent::ToolResult {
            id: format!("{call_id}:result"),
            timestamp: ended,
            tool_call_id: Some(call_id),
            text,
            is_error,
            subagent: None,
            patch: None,
            facts: ToolFacts {
                interrupted,
                // agy 自己界面的终端块里的退出码（StepRenderInfo.ContentBlock.terminal.exit_code）。
                exit_code: value
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .map(|code| code.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
                ..ToolFacts::default()
            },
        },
    ]
}

fn subagent_events(id: &str, timestamp: Option<String>, value: &Value) -> Vec<TranscriptEvent> {
    let name = str_of(value, "name");
    let prompt = str_of(value, "prompt");
    let summary: String = if name.is_empty() { prompt } else { name }
        .chars()
        .take(800)
        .collect();
    vec![
        TranscriptEvent::ToolCall {
            id: id.to_string(),
            timestamp: timestamp.clone(),
            kind: ToolKind::Agent,
            summary,
            detail: None,
            name: "invoke_subagent".into(),
            subagent: None,
            plan: None,
        },
        TranscriptEvent::ToolResult {
            id: format!("{id}:result"),
            timestamp,
            tool_call_id: Some(id.to_string()),
            text: String::new(),
            is_error: str_of(value, "status") == "error",
            subagent: None,
            patch: None,
            facts: ToolFacts {
                interrupted: str_of(value, "status") == "interrupted",
                ..ToolFacts::default()
            },
        },
    ]
}

/// agy 内置工具 → 语义类别。工具名取自真机库（run_command / read_file / write_file /
/// ask_question）与 agy 内置的 agent 说明（`tools: [read_file, edit_file, write_file,
/// run_command, manage_task, search_web]`，agy-strings 107110 行）。
fn tool_kind(name: &str) -> ToolKind {
    match name {
        "run_command" | "send_command_input" | "command_status" => ToolKind::Run,
        "read_file" | "view_file" | "list_dir" => ToolKind::Read,
        "write_file" | "write_to_file" => ToolKind::Write,
        "edit_file" | "multi_edit" | "replace_file_content" | "multi_replace_file_content" => {
            ToolKind::Edit
        }
        "grep_search" | "find_by_name" | "glob" => ToolKind::Search,
        "search_web" | "read_url_content" => ToolKind::Web,
        "invoke_subagent" => ToolKind::Agent,
        "manage_task" => ToolKind::Todo,
        SEND_IMAGE => ToolKind::Image,
        _ => ToolKind::Other,
    }
}

/// Meowo 发图工具在 agy 里的全名：MCP 工具急加载时注册成 `mcp_<服务器>_<工具>`
/// （agy 文档 mcp.md「Tool loading」）。
const SEND_IMAGE: &str = "mcp_meowo_send_image";
/// Meowo 的 MCP 工具在 agy 里的前缀；对话页上换成统一的 `mcp__meowo__<工具>`。
const MEOWO_PREFIX: &str = "mcp_meowo_";

/// 参数里第一个存在的字符串值。agy 的工具参数是 PascalCase（`CommandLine`、`Path`）。
fn first_str(args: Option<&Value>, keys: &[&str]) -> Option<String> {
    let args = args?;
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

/// 摘要：agy 让模型给每次调用写一句 `toolSummary`（实测 run_command 的参数里就有，如
/// "Echo hello-probe"），但那是模型的转述；命令行和路径更能说明在干什么，优先取它们，
/// 认不得的工具退回 agy 界面自己的标题，再退回 toolSummary 与整包参数。
fn tool_summary(name: &str, args: Option<&Value>, title: Option<&str>) -> String {
    let keys: &[&str] = match name {
        "run_command" => &["CommandLine"],
        "read_file" | "view_file" | "write_file" | "write_to_file" | "edit_file" | "multi_edit" => {
            &["Path", "TargetFile", "AbsolutePath"]
        }
        "list_dir" => &["DirectoryPath", "Path"],
        "grep_search" | "find_by_name" | "glob" => &["Query", "Pattern", "SearchPath"],
        "search_web" => &["Query", "query"],
        "read_url_content" => &["Url"],
        "invoke_subagent" => &["Prompt"],
        _ => &[],
    };
    let text = first_str(args, keys)
        .or_else(|| title.map(str::to_string))
        .or_else(|| first_str(args, &["toolSummary"]))
        .or_else(|| {
            args.filter(|v| v.as_object().is_some_and(|map| !map.is_empty()))
                .map(Value::to_string)
        })
        .unwrap_or_default();
    let mut s: String = text.chars().take(800).collect();
    if text.chars().count() > 800 {
        s.push('…');
    }
    s
}

/// 详情正文上限（字符），与 claude 侧同值：前端只做预览，靠 truncated 标注「已截断」。
const DETAIL_TEXT_MAX_CHARS: usize = 16_000;

fn clip_detail(text: &str) -> (String, bool) {
    if text.chars().count() > DETAIL_TEXT_MAX_CHARS {
        (text.chars().take(DETAIL_TEXT_MAX_CHARS).collect(), true)
    } else {
        (text.to_string(), false)
    }
}

/// 结构化详情。参数名取自真机库（run_command：CommandLine/toolSummary；read_file：Path/StartLine/
/// EndLine；write_file：Path/Content）。编辑工具的参数名没有真机样本，认不出就不给详情。
fn tool_detail(name: &str, args: Option<&Value>) -> Option<ToolDetail> {
    let num = |key: &str| {
        args?
            .get(key)
            .and_then(Value::as_u64)
            .map(|v| v.min(u32::MAX as u64) as u32)
    };
    match name {
        "run_command" => Some(ToolDetail::Command {
            command: first_str(args, &["CommandLine"])?,
            description: first_str(args, &["toolSummary"]),
        }),
        "read_file" | "view_file" => {
            let start = num("StartLine");
            let end = num("EndLine");
            Some(ToolDetail::Read {
                path: first_str(args, &["Path", "AbsolutePath"])?,
                offset: start,
                limit: start.zip(end).map(|(s, e)| e.saturating_sub(s) + 1),
            })
        }
        "write_file" | "write_to_file" => {
            let full = first_str(args, &["Content", "CodeContent"])?;
            let lines = full.lines().count() as u32;
            let (content, truncated) = clip_detail(&full);
            Some(ToolDetail::Write {
                path: first_str(args, &["Path", "TargetFile"])?,
                content,
                lines,
                truncated,
            })
        }
        "edit_file" | "replace_file_content" => {
            let (old, old_cut) = clip_detail(&first_str(args, &["TargetContent", "OldString"])?);
            let (new, new_cut) =
                clip_detail(&first_str(args, &["ReplacementContent", "NewString"])?);
            Some(ToolDetail::Edit {
                path: first_str(args, &["Path", "TargetFile"])?,
                old,
                new,
                replace_all: args
                    .and_then(|a| a.get("AllowMultiple"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                truncated: old_cut || new_cut,
            })
        }
        "grep_search" | "find_by_name" => Some(ToolDetail::Search {
            pattern: first_str(args, &["Query", "Pattern"])?,
            path: first_str(args, &["SearchPath", "SearchDirectory"]),
            glob: first_str(args, &["Includes"]),
        }),
        _ => None,
    }
}

/// 导出行里的回合错误 → 标签与原文。时间线的错误卡与会话级「出错」状态共用。
fn turn_error(value: &Value) -> Option<TurnError> {
    let text = str_of(value, "text").trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    // 标签必须是前端 errorLabels 表里已有的 sentinel（i18n/zh.ts），不另造新词。
    let label = if lower.contains("not logged in") || lower.contains("unauthenticated") {
        "需要重新登录"
    } else {
        "回合出错"
    };
    Some(TurnError {
        label: label.into(),
        raw: text.to_string(),
        fingerprint: format!("antigravity:{label}"),
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 时间线的错误卡与会话级「出错」读同一行、给同一个标签；之后又有回复就清掉。
    #[test]
    fn error_line_feeds_both_the_timeline_and_the_session_state() {
        let error = r#"{"idx":3,"kind":"error","ts":1,"text":"You are not logged into Antigravity."}"#;
        let label = parse_line(error)
            .into_iter()
            .find_map(|e| match e {
                TranscriptEvent::TurnError { label, .. } => Some(label),
                _ => None,
            })
            .unwrap();
        assert_eq!(label, "需要重新登录");
        let mut parser = AntigravityParser::default();
        parser.fold_line(error);
        assert_eq!(parser.to_info().error.unwrap().label, label);
        parser.fold_line(r#"{"idx":4,"kind":"assistant","ts":2,"text":"ok","context_tokens":1000,"context_window":4000}"#);
        let info = parser.to_info();
        assert!(info.error.is_none());
        assert_eq!(info.context_pct, Some(25));
    }

    /// 审批面板上被拒绝的调用是用户的选择，不标红；真的失败标红并带上错误原文。
    #[test]
    fn denied_tool_is_interrupted_and_failed_tool_is_an_error() {
        let denied = r#"{"idx":2,"kind":"tool","id":"c1","name":"write_file","status":"error","error":"permission check failed for write_file(a.txt): user denied permission","output":""}"#;
        let failed = r#"{"idx":3,"kind":"tool","id":"c2","name":"read_file","status":"error","error":"file does not exist","output":""}"#;
        let result = |line: &str| {
            parse_line(line).into_iter().find_map(|e| match e {
                TranscriptEvent::ToolResult { is_error, facts, text, .. } => Some((is_error, facts.interrupted, text)),
                _ => None,
            })
        };
        assert_eq!(result(denied), Some((false, true, String::new())));
        assert_eq!(result(failed), Some((true, false, "file does not exist".to_string())));
    }
}
