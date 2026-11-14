//! opencode 的遥测：结构化对话、Stop 正文/模型、待办，三样都从它的会话库读。
//!
//! opencode 没有会话日志文件，对话页读的是 [`store::materialize`] 导出的 JSONL 缓存——行结构由
//! 我们自己定（见 store.rs），这里只负责把三种行（`user` / `part` / `error`）映射成领域事件。
//! 上下文占用不提供：库里有 token 数，却没有模型的窗口大小，算不出百分比。

use serde_json::Value;

#[cfg(test)]
use crate::transcript::ChatItem;
use crate::{
    caps::{HookContext, StopOutputs, TelemetryCap, TodoSnapshot},
    transcript::{
        CompactionDetail, ToolDetail, ToolFacts, ToolKind, TranscriptEvent, TranscriptInfo,
        TranscriptParser, TranscriptSpec, TurnError,
    },
};

use super::store;

pub static TELEMETRY: OpencodeTelemetry = OpencodeTelemetry;
pub static TRANSCRIPT: OpencodeTranscript = OpencodeTranscript;

pub struct OpencodeTelemetry;

impl TelemetryCap for OpencodeTelemetry {
    /// 桥接插件构造的 Stop 负载不带正文（`session.idle` 事件本身只有 sessionID），从库里读。
    fn stop_outputs(&self, ctx: &HookContext) -> StopOutputs {
        let (last_ai, model) = store::last_reply(ctx.session_id).unwrap_or_default();
        StopOutputs {
            last_ai,
            model,
            effort: None,
        }
    }

    fn read_todos(&self, ctx: &HookContext) -> Option<Vec<TodoSnapshot>> {
        Some(
            store::todos(ctx.session_id)?
                .into_iter()
                .map(|(content, status)| TodoSnapshot { content, status })
                .collect(),
        )
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

pub struct OpencodeTranscript;

/// opencode 的看板分析解析器：只产**会话级出错**。
///
/// 回合出错在时间线里是一张红色错误卡，可侧栏状态、灵动岛与通知读的是分析口径——此前
/// 这里是空解析器，对话里报错、侧栏却写着「等待输入」，也不弹出错通知（走查 A8）。
/// 判据与时间线共用 [`turn_error`]：导出行里的 `error` 记录（用户中断不算）。之后只要
/// 又有新的用户消息或助手内容，就说明已经越过那次出错，清掉。
#[derive(Default)]
struct OpencodeParser {
    error: Option<TurnError>,
}

impl TranscriptParser for OpencodeParser {
    fn fold_line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        match value.get("kind").and_then(Value::as_str) {
            Some("error") => {
                // 用户中断（MessageAbortedError）不是出错，也不清掉此前的出错。
                if let Some(error) = value.get("error").and_then(turn_error) {
                    self.error = Some(error);
                }
            }
            Some("user" | "part") => self.error = None,
            _ => {}
        }
    }

    fn to_info(&self) -> TranscriptInfo {
        TranscriptInfo {
            error: self.error.clone(),
            ..TranscriptInfo::default()
        }
    }
}

/// 看板轮询的导出节流（见 [`crate::transcript::PolledPaths`]）：「定位」对 opencode 意味着开库、
/// 比指纹、必要时增量导出。出错徽标容忍几秒延迟，5 秒内直接用上次的路径。
static POLLED_EXPORTS: crate::transcript::PolledPaths =
    crate::transcript::PolledPaths::new(std::time::Duration::from_secs(5));

impl TranscriptSpec for OpencodeTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::new(OpencodeParser::default())
    }

    fn resolve_transcript_path_polled(
        &self,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        POLLED_EXPORTS.get_or_resolve(session_id, || store::materialize(session_id))
    }

    /// 定位＝导出：每次调用把库里新定稿的内容追加进缓存（库没动时只比一次 mtime）。
    fn resolve_transcript_path(
        &self,
        _transcript_path: Option<&str>,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<std::path::PathBuf> {
        store::materialize(session_id)
    }

    /// opencode 自己用小模型起的标题（库里 session.title）。2026-11 起采用（用户定：各家都用
    /// agent 自己起的标题，同 Claude 的 ai-title）；还没起好时是「New session - 时间」，不采。
    fn resolve_title(&self, _: Option<&str>, _: Option<&str>, session_id: &str) -> Option<String> {
        store::session_title(session_id)
    }

    fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
        parse_line(line)
    }

    fn supports_chat(&self) -> bool {
        true
    }

    /// 长会话首开只解析尾部一屏。导出的 JSONL 逐行独立（parse_line 只看本行），没有撤回、
    /// 分叉记录，也不产出模式与计划，窗口前的内容不影响结果。
    fn tail_read(&self) -> Option<crate::transcript::TailRead> {
        Some(crate::transcript::TailRead {
            mode_markers: &[],
            plan_markers: &[],
        })
    }

    /// 只为会话级出错（见 [`OpencodeParser`]）。看板后台分析会对每张卡反复定位 transcript，
    /// 对 opencode 那意味着反复导出，所以轮询路径做了节流（resolve_transcript_path_polled）。
    fn supports_analysis(&self) -> bool {
        true
    }
}

/// epoch 毫秒 → ISO-8601 UTC（`ChatItem.timestamp` 与 claude 同形）。
fn iso_from_ms(ms: i64) -> String {
    let secs = crate::codec::unix_to_iso8601(ms.div_euclid(1000));
    let millis = ms.rem_euclid(1000);
    // unix_to_iso8601 输出以 `Z` 结尾，把毫秒插在它前面。
    format!("{}.{millis:03}Z", secs.trim_end_matches('Z'))
}

fn parse_line(line: &str) -> Vec<TranscriptEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let msg = value.get("msg").and_then(Value::as_str).unwrap_or_default();
    let key = value.get("key").and_then(Value::as_str).unwrap_or_default();
    let timestamp = value.get("time").and_then(Value::as_i64).map(iso_from_ms);
    match value.get("kind").and_then(Value::as_str) {
        Some("user") => user_events(msg, timestamp, value.get("parts")),
        Some("part") => {
            // 压缩产出的摘要消息（`summary: true`）是喂给模型的上下文，不是对用户说的话。
            let summary = value
                .get("summary")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match value.get("part") {
                Some(part) => part_events(key, part, timestamp, summary),
                None => Vec::new(),
            }
        }
        Some("error") => error_events(msg, timestamp, value.get("error")),
        _ => Vec::new(),
    }
}

fn user_events(
    msg: &str,
    timestamp: Option<String>,
    parts: Option<&Value>,
) -> Vec<TranscriptEvent> {
    let parts = parts
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    // /compact 落成一条带 `compaction` part 的用户消息：画成分隔条，不当用户的话。
    if let Some(part) = parts
        .iter()
        .find(|p| p.get("type").and_then(Value::as_str) == Some("compaction"))
    {
        // part 自带 `auto`（上下文将满时自动压 / 用户 /compact），分隔条据此区分说法。
        let trigger = part
            .get("auto")
            .and_then(Value::as_bool)
            .map(|auto| if auto { "auto" } else { "manual" }.to_string());
        return vec![TranscriptEvent::Metadata {
            id: format!("{msg}:compact"),
            timestamp,
            kind: "compact".into(),
            compaction: trigger.map(|trigger| CompactionDetail {
                trigger: Some(trigger),
                ..CompactionDetail::default()
            }),
        }];
    }
    let mut lines: Vec<String> = parts
        .iter()
        .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
        // synthetic 是 opencode 替用户注入的（@ 文件的正文、工具结果回灌），用户没打过这些字。
        .filter(|p| !flag(p, "synthetic") && !flag(p, "ignored"))
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    for (index, part) in parts.iter().enumerate() {
        if let Some(path) = persist_image(msg, index, part) {
            lines.push(format!("[Image: source: {}]", path.display()));
        }
    }
    let text = lines.join("\n");
    if text.trim().is_empty() {
        return Vec::new();
    }
    vec![TranscriptEvent::UserMessage {
        id: msg.to_string(),
        timestamp,
        text,
        interjected: false,
        turn_id: None,
    }]
}

fn flag(part: &Value, key: &str) -> bool {
    part.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// 粘贴的截图在 opencode 里是 `file` part + `data:image/…;base64,` url。落盘后补
/// `[Image: source: …]` 行，与 claude/kimi 同形，前端缩略图链路直接接住。按消息 id + 序号命名
/// 做幂等（reset 重放同一行会落到同一个文件）。
fn persist_image(msg: &str, index: usize, part: &Value) -> Option<std::path::PathBuf> {
    use crate::fsutil::PASTE_MAX_BYTES;
    if part.get("type").and_then(Value::as_str) != Some("file") {
        return None;
    }
    let rest = part.get("url")?.as_str()?.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    let ext = match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let safe: String = msg
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(64)
        .collect();
    if safe.is_empty() || data.len() > PASTE_MAX_BYTES / 3 * 4 + 4 {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .ok()?;
    crate::fsutil::persist_paste_bytes(&format!("opencode-{safe}-{index}"), ext, &bytes)
}

/// `key` 是导出时写在行上的 part id（库里的 part 正文不带 id），直接当稳定 id 用。
fn part_events(
    key: &str,
    part: &Value,
    timestamp: Option<String>,
    summary: bool,
) -> Vec<TranscriptEvent> {
    let text = || part.get("text").and_then(Value::as_str).unwrap_or_default();
    match part.get("type").and_then(Value::as_str) {
        // 回合间常有只含 "\n" 的文本 part（实测：每次工具调用前都有一个），不出空气泡。
        Some("text") if !summary && !flag(part, "synthetic") && !text().trim().is_empty() => {
            vec![TranscriptEvent::AssistantMessage {
                id: key.to_string(),
                timestamp,
                text: text().to_string(),
            }]
        }
        Some("reasoning") if !text().trim().is_empty() => vec![TranscriptEvent::Reasoning {
            id: key.to_string(),
            timestamp,
            text: text().to_string(),
        }],
        Some("tool") => tool_events(key, part, timestamp),
        _ => Vec::new(),
    }
}

fn tool_events(key: &str, part: &Value, timestamp: Option<String>) -> Vec<TranscriptEvent> {
    let name = part
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let call_id = part
        .get("callID")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| key.to_string());
    let state = part.get("state");
    let input = state.and_then(|s| s.get("input"));
    let status = state
        .and_then(|s| s.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let title = state
        .and_then(|s| s.get("title"))
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty());
    let call = TranscriptEvent::ToolCall {
        id: call_id.clone(),
        timestamp: timestamp.clone(),
        kind: tool_kind(&name),
        summary: tool_summary(&name, input, title),
        detail: tool_detail(&name, input),
        name,
        subagent: None,
        plan: None,
    };
    let (text, is_error, interrupted) = match status {
        "completed" => (
            state
                .and_then(|s| s.get("output"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            false,
            false,
        ),
        "error" => {
            let error = state
                .and_then(|s| s.get("error"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            // 用户按 Esc 时，正在跑的工具以 error 收场，文案固定是这一句（本机 3 次中断都是）。
            // 它是被打断，不是失败：不标红，由前端显示「已中断」。
            if error == "Tool execution aborted" {
                (String::new(), false, true)
            } else {
                (error.to_string(), true, false)
            }
        }
        // 只有「所在消息被后续消息越过」时才会导出未终态的工具（见 store::collect_lines）：
        // 它再也不会有结果了。补一条回执并标成被打断，否则对话页会永远挂着「运行中」。
        // 「已中断」这个词由前端按界面语言给：此前这里把中文当回执正文下发，英文界面露中文。
        _ => (String::new(), false, true),
    };
    // 回执的时间取工具结束时刻（`state.time.end`）：此前调用与回执共用开始时刻，
    // 工具耗时永远算不出来。
    let ended = state
        .and_then(|s| s.pointer("/time/end"))
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
                ..ToolFacts::default()
            },
        },
    ]
}

/// opencode 内置工具 → 语义类别（工具名取自 1.19.x 实测落库的 `part.tool`）。
fn tool_kind(name: &str) -> ToolKind {
    match name {
        "bash" => ToolKind::Run,
        "read" => ToolKind::Read,
        "write" => ToolKind::Write,
        "edit" | "multiedit" | "patch" => ToolKind::Edit,
        "grep" | "glob" | "codesearch" => ToolKind::Search,
        "webfetch" | "websearch" => ToolKind::Web,
        "task" => ToolKind::Agent,
        "todowrite" | "todoread" => ToolKind::Todo,
        _ => ToolKind::Other,
    }
}

/// 摘要取各工具最能说明「在干什么」的那个入参（opencode 的入参是 camelCase），
/// 认不得的工具退回 opencode 自己写的 `state.title`，再退回整包入参。
fn tool_summary(name: &str, input: Option<&Value>, title: Option<&str>) -> String {
    let key = match name {
        "bash" => "command",
        "read" | "write" | "edit" => "filePath",
        "grep" | "glob" => "pattern",
        "list" => "path",
        "webfetch" => "url",
        "websearch" | "codesearch" => "query",
        "task" => "description",
        _ => "",
    };
    let picked = input
        .and_then(|v| v.get(key))
        .and_then(Value::as_str)
        .map(str::to_string);
    let text = picked
        .or_else(|| title.map(str::to_string))
        .or_else(|| input.filter(|v| !v.is_null()).map(Value::to_string))
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

/// 内置文件/终端/搜索工具的结构化详情（入参名取自 opencode 1.19.3 实测落库的 `state.input`）。
fn tool_detail(name: &str, input: Option<&Value>) -> Option<ToolDetail> {
    let input = input?;
    let text = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    let num = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_u64)
            .map(|v| v.min(u32::MAX as u64) as u32)
    };
    match name {
        "write" => {
            let full = text("content")?;
            let lines = full.lines().count() as u32;
            let (content, truncated) = clip_detail(&full);
            Some(ToolDetail::Write {
                path: text("filePath")?,
                content,
                lines,
                truncated,
            })
        }
        "edit" => {
            let (old, old_cut) = clip_detail(&text("oldString")?);
            let (new, new_cut) = clip_detail(&text("newString")?);
            Some(ToolDetail::Edit {
                path: text("filePath")?,
                old,
                new,
                replace_all: input
                    .get("replaceAll")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                truncated: old_cut || new_cut,
            })
        }
        "bash" => Some(ToolDetail::Command {
            command: text("command")?,
            description: text("description"),
        }),
        "read" => Some(ToolDetail::Read {
            path: text("filePath")?,
            offset: num("offset"),
            limit: num("limit"),
        }),
        "grep" | "glob" => Some(ToolDetail::Search {
            pattern: text("pattern")?,
            path: text("path"),
            glob: text("include"),
        }),
        _ => None,
    }
}

/// 回合错误。用户按 Esc 中断（`MessageAbortedError`）不是故障，不出错误气泡，
/// 画成与 claude 同款的中断分隔条。
fn error_events(
    msg: &str,
    timestamp: Option<String>,
    error: Option<&Value>,
) -> Vec<TranscriptEvent> {
    let Some(error) = error else {
        return Vec::new();
    };
    if error.get("name").and_then(Value::as_str) == Some("MessageAbortedError") {
        return vec![TranscriptEvent::Metadata {
            id: format!("{msg}:interrupted"),
            timestamp,
            kind: "interrupted".into(),
            compaction: None,
        }];
    }
    turn_error(error)
        .map(|error| TranscriptEvent::TurnError {
            id: format!("{msg}:error"),
            timestamp,
            label: error.label,
            text: error.raw,
        })
        .into_iter()
        .collect()
}

/// 导出行里的回合错误 → 标签与原文。时间线的错误卡与会话级「出错」状态共用。
/// 用户中断（`MessageAbortedError`）不是错误，返回 None。
fn turn_error(error: &Value) -> Option<TurnError> {
    let name = error
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if name == "MessageAbortedError" {
        return None;
    }
    let text = error
        .pointer("/data/message")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(name)
        .to_string();
    if text.trim().is_empty() {
        return None;
    }
    let label = if name == "ProviderAuthError" {
        "需要重新登录"
    } else {
        "回合出错"
    };
    Some(TurnError {
        label: label.into(),
        raw: text,
        fingerprint: format!("opencode:{label}"),
    })
}

#[cfg(test)]
fn parse_chat_items(line: &str) -> Vec<ChatItem> {
    parse_line(line).into_iter().map(ChatItem::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::opencode::store::test_support::*;
    use serde_json::json;

    #[test]
    fn epoch_ms_becomes_iso_with_millis() {
        assert_eq!(iso_from_ms(1_793_081_874_918), "2026-10-27T06:17:54.918Z");
    }

    #[test]
    fn user_line_joins_text_and_skips_synthetic_parts() {
        let line = json!({"kind": "user", "msg": "msg_1", "key": "", "time": 1, "parts": [
            {"type": "text", "text": "看看这个"},
            {"type": "text", "text": "<file content>", "synthetic": true},
        ]})
        .to_string();
        match parse_chat_items(&line).as_slice() {
            [ChatItem::UserText { id, text, .. }] => {
                assert_eq!(id, "msg_1");
                assert_eq!(text, "看看这个");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn compaction_user_message_becomes_a_divider() {
        let line = json!({"kind": "user", "msg": "msg_1", "key": "", "parts": [
            {"type": "compaction", "auto": false}]})
        .to_string();
        assert!(matches!(
            parse_chat_items(&line).as_slice(),
            [ChatItem::Meta { kind, compaction: Some(detail), .. }]
                if kind == "compact" && detail.trigger.as_deref() == Some("manual")
        ));
    }

    #[test]
    fn whitespace_text_and_summary_text_are_dropped() {
        let ws =
            json!({"kind": "part", "msg": "m", "key": "p", "part": {"type": "text", "text": "\n"}});
        assert!(parse_chat_items(&ws.to_string()).is_empty());
        let summary = json!({"kind": "part", "msg": "m", "key": "p", "summary": true,
            "part": {"type": "text", "text": "压缩摘要"}});
        assert!(parse_chat_items(&summary.to_string()).is_empty());
    }

    #[test]
    fn tool_part_yields_call_with_detail_and_result() {
        let line = json!({"kind": "part", "msg": "m", "key": "p", "time": 5, "part": {
            "type": "tool", "tool": "bash", "callID": "call_1",
            "state": {"status": "completed", "input": {"command": "git status", "description": "看状态"},
                      "output": "clean"}}})
        .to_string();
        match parse_chat_items(&line).as_slice() {
            [ChatItem::ToolUse {
                id,
                name,
                summary,
                detail,
                ..
            }, ChatItem::ToolResult {
                tool_use_id,
                text,
                is_error,
                ..
            }] => {
                assert_eq!(id, "call_1");
                assert_eq!(name, "bash");
                assert_eq!(summary, "git status");
                assert!(
                    matches!(detail, Some(ToolDetail::Command { command, .. }) if command == "git status")
                );
                assert_eq!(tool_use_id.as_deref(), Some("call_1"));
                assert_eq!(text, "clean");
                assert!(!is_error);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn abandoned_tool_gets_an_interrupted_receipt() {
        let line = json!({"kind": "part", "msg": "m", "key": "p", "part": {
            "type": "tool", "tool": "bash", "callID": "c", "state": {"status": "running", "input": {}}}})
        .to_string();
        // 被打断是终态，不是失败；正文为空，「已中断」由前端按界面语言给。
        assert!(matches!(
            parse_chat_items(&line).as_slice(),
            [_, ChatItem::ToolResult { is_error: false, interrupted: true, text, .. }] if text.is_empty()
        ));
    }

    #[test]
    fn errors_map_to_turn_error_and_abort_to_interrupted_divider() {
        let api = json!({"kind": "error", "msg": "m", "key": "~error",
            "error": {"name": "APIError", "data": {"message": "overloaded"}}});
        assert!(matches!(
            parse_chat_items(&api.to_string()).as_slice(),
            [ChatItem::TurnError { label, text, .. }] if label == "回合出错" && text == "overloaded"
        ));
        let aborted = json!({"kind": "error", "msg": "m", "key": "~error",
            "error": {"name": "MessageAbortedError", "data": {"message": "Aborted"}}});
        assert!(matches!(
            parse_chat_items(&aborted.to_string()).as_slice(),
            [ChatItem::Meta { kind, .. }] if kind == "interrupted"
        ));
    }

    /// 会话级「出错」与时间线的错误卡同源；用户中断不算出错；越过那次出错后清掉。
    #[test]
    fn analysis_parser_reports_turn_error_until_conversation_moves_on() {
        let api = json!({"kind": "error", "msg": "m", "key": "~error",
            "error": {"name": "APIError", "data": {"message": "overloaded"}}})
        .to_string();
        let mut parser = OpencodeParser::default();
        parser.fold_line(&api);
        let error = parser.to_info().error.expect("回合出错应置位");
        assert!(matches!(
            parse_chat_items(&api).as_slice(),
            [ChatItem::TurnError { label, text, .. }] if *label == error.label && *text == error.raw
        ));
        let aborted = json!({"kind": "error", "msg": "m2", "key": "~error",
            "error": {"name": "MessageAbortedError", "data": {"message": "Aborted"}}})
        .to_string();
        parser.fold_line(&aborted);
        assert!(parser.to_info().error.is_some(), "中断不清掉此前的出错");
        parser.fold_line(&json!({"kind": "user", "msg": "m3", "key": "", "parts": []}).to_string());
        assert!(parser.to_info().error.is_none(), "用户发了新消息即越过");

        let mut parser = OpencodeParser::default();
        parser.fold_line(&aborted);
        assert!(parser.to_info().error.is_none(), "用户中断不是出错");
    }

    /// 导出 → 解析的整条链路：缓存行喂回解析器得到完整的一轮对话。
    #[test]
    fn exported_lines_parse_into_a_conversation() {
        let conn = memory_db();
        msg(
            &conn,
            "msg_1",
            json!({"role": "user", "time": {"created": 1}}),
        );
        part(
            &conn,
            "prt_1",
            "msg_1",
            json!({"type": "text", "text": "hi"}),
        );
        msg(
            &conn,
            "msg_2",
            json!({"role": "assistant", "time": {"created": 2, "completed": 3}}),
        );
        part(
            &conn,
            "prt_2",
            "msg_2",
            json!({"type": "text", "text": "你好", "time": {"start": 2, "end": 3}}),
        );
        let items: Vec<ChatItem> = lines(&conn, None)
            .iter()
            .flat_map(|line| parse_chat_items(line.trim_end()))
            .collect();
        assert!(matches!(
            items.as_slice(),
            [ChatItem::UserText { text: u, .. }, ChatItem::AssistantText { text: a, .. }] if u == "hi" && a == "你好"
        ));
    }
}
