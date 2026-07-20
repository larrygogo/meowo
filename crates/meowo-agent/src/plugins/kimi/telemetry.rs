//! kimi-code 会话记录解析。kimi 的 Stop hook **不带** AI 正文（只有 session_id/cwd），
//! 故需从会话的 `agents/main/wire.jsonl` 里读最近一条 AI 文本。
//!
//! wire.jsonl 结构（kimi-code 0.19.2 实测）：每行一个事件，AI 正文在
//! `type="context.append_loop_event"` 且 `event.type="content.part"` 且 `event.part.type="text"`
//! 的 `event.part.text` 里（`part.type="think"` 是思考过程：聊天窗口展示，但不计入最终 AI 正文）。
//! 用户输入则是 `type="turn.prompt"`
//! 或 `type="context.append_message"` 且 `message.role="user"`——遇到即清空缓冲，使最终缓冲恰为
//! 「最后一条用户输入之后的 AI 文本」。

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::transcript::ChatItem;
use crate::transcript::{ChatOnlyParser, TranscriptEvent, TranscriptParser, TranscriptSpec};

fn chat_id(prefix: &str, line: &str) -> String {
    let hash = line.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("kimi-{prefix}-{hash:016x}")
}

fn value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
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
    match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "turn.prompt" => {
            let text = value
                .get("input")
                .or_else(|| value.get("prompt"))
                .or_else(|| value.get("message"))
                .map(value_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::UserMessage {
                    id: chat_id("user", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        "context.append_message"
            if value
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(|role| role.as_str())
                == Some("user") =>
        {
            let Some(message) = value.get("message") else {
                return Vec::new();
            };
            // Kimi 也用 role=user 承载 system reminder、后台任务通知等内部注入。只排除明确
            // 标成非 user 的 origin；旧版没有 origin 的纯用户消息仍需兼容。
            if message
                .get("origin")
                .and_then(|origin| origin.get("kind"))
                .and_then(|kind| kind.as_str())
                .is_some_and(|kind| kind != "user")
            {
                return Vec::new();
            }
            let text = message
                .get("content")
                .or_else(|| message.get("text"))
                .map(value_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::UserMessage {
                    id: chat_id("user", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        "context.append_loop_event" => {
            let Some(event) = value.get("event") else {
                return Vec::new();
            };
            let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if matches!(event_type, "tool.result" | "tool_result") {
                return vec![TranscriptEvent::ToolResult {
                    id: chat_id("result", line),
                    timestamp,
                    tool_call_id: event
                        .get("callId")
                        .or_else(|| event.get("call_id"))
                        .or_else(|| event.get("toolCallId"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    text: event
                        .get("output")
                        .or_else(|| event.pointer("/result/output"))
                        .or_else(|| event.get("result"))
                        .map(value_text)
                        .unwrap_or_default(),
                    is_error: event
                        .get("isError")
                        .or_else(|| event.get("is_error"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                }];
            }
            let part = if event_type == "content.part" {
                event.get("part")
            } else if matches!(event_type, "tool.call" | "tool_call") {
                Some(event)
            } else {
                None
            };
            let Some(part) = part else { return Vec::new() };
            match part
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or(event_type)
            {
                "text" => part
                    .get("text")
                    .and_then(|v| v.as_str())
                    .filter(|text| !text.is_empty())
                    .map(|text| TranscriptEvent::AssistantChunk {
                        id: chat_id("assistant", line),
                        timestamp,
                        text: text.to_string(),
                    })
                    .into_iter()
                    .collect(),
                "think" | "thinking" => part
                    .get("think")
                    .or_else(|| part.get("thinking"))
                    .or_else(|| part.get("text"))
                    .and_then(|v| v.as_str())
                    .filter(|text| !text.is_empty())
                    .map(|text| TranscriptEvent::ReasoningChunk {
                        id: chat_id("reasoning", line),
                        timestamp,
                        text: text.to_string(),
                    })
                    .into_iter()
                    .collect(),
                "tool.call" | "tool_call" | "tool" => vec![TranscriptEvent::ToolCall {
                    id: part
                        .get("id")
                        .or_else(|| part.get("callId"))
                        .or_else(|| part.get("toolCallId"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| chat_id("tool", line)),
                    timestamp,
                    name: part
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool")
                        .to_string(),
                    summary: part
                        .get("input")
                        .or_else(|| part.get("arguments"))
                        .or_else(|| part.get("args"))
                        .map(value_text)
                        .unwrap_or_default(),
                }],
                _ => Vec::new(),
            }
        }
        "context.compact" | "context.compacted" => vec![TranscriptEvent::Metadata {
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

pub struct KimiTranscript;
pub static KIMI_TRANSCRIPT: KimiTranscript = KimiTranscript;

impl TranscriptSpec for KimiTranscript {
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
            .or_else(|| {
                session_dir(session_id)
                    .map(|dir| dir.join("agents").join("main").join("wire.jsonl"))
                    .filter(|path| path.exists())
            })
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
        match value.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "permission.set_mode" => value
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|mode| vec![crate::AgentMode::new("permission", mode)])
                .unwrap_or_default(),
            "plan_mode.enter" => vec![crate::AgentMode::new("work", "plan")],
            "plan_mode.exit" => vec![crate::AgentMode::new("work", "default")],
            _ => Vec::new(),
        }
    }

    fn default_agent_modes(&self) -> Vec<crate::AgentMode> {
        vec![
            crate::AgentMode::new("work", "default"),
            crate::AgentMode::new("permission", "manual"),
        ]
    }

    fn supports_chat(&self) -> bool {
        true
    }

    fn supports_analysis(&self) -> bool {
        false
    }
}

/// kimi 在本机的实况（走哪个变体、数据目录/配置/凭据/可执行在哪）。变体表见
/// `meowo_agent::plugins::kimi`：新版 Node「Kimi Code」`~/.kimi-code` 优先，旧 Python 版
/// `kimi-cli` 的 `~/.kimi` 兼容（两者 hook 格式一致）；都没装则给出新版的默认落点。
/// 检测/接线/状态/账号凭据/会话读取全部经此一处解析路径。
pub fn kimi_install() -> Option<crate::Installation> {
    crate::registry::installation(crate::id::KIMI)
}

/// kimi 数据根。`kimi_install()` 的便捷取值——调用方只关心目录时用它。
pub fn kimi_share_dir() -> Option<PathBuf> {
    kimi_install().map(|i| i.data_dir)
}

/// kimi 的启动 argv（单元素：可执行绝对路径；找不到回退裸名 "kimi" 走 PATH）。
/// resume/launch 用：meowo-app 拉起的终端 PATH 未必含 kimi（或 kimi 是 shim/别名），故优先绝对路径，
/// 避免 wt/powershell「系统找不到指定的文件」。
pub fn kimi_launch_argv() -> Vec<String> {
    kimi_install()
        .map(|i| i.launch_argv())
        .unwrap_or_else(|| vec!["kimi".to_string()])
}

/// kimi 可执行是否真实落在某个已知位置（区别于 `kimi_launch_argv` 找不到时回退裸名）。
pub fn kimi_installed() -> bool {
    kimi_install().is_some_and(|i| i.is_launchable())
}

/// 从 `session_index.jsonl` 查 session_id 对应的会话目录（kimi 的目录名带哈希，靠此索引而非自己算）。
fn session_dir(session_id: &str) -> Option<PathBuf> {
    let idx = kimi_share_dir()?.join("session_index.jsonl");
    let content = std::fs::read_to_string(idx).ok()?;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("sessionId").and_then(|s| s.as_str()) == Some(session_id) {
            return v
                .get("sessionDir")
                .and_then(|s| s.as_str())
                .map(PathBuf::from);
        }
    }
    None
}

/// 从 wire.jsonl 提取的会话摘要。
#[derive(Debug, Default, PartialEq)]
pub struct WireSummary {
    /// 最近一条 AI 正文（最后一条用户输入之后拼接的 text 片段）。
    pub last_ai: Option<String>,
    /// 模型展示名（由模型 alias 映射，见 model_display）。
    pub model: Option<String>,
}

/// 模型 alias → 展示名。alias 形如 "kimi-code/kimi-for-coding"；取末段，已知模型映射成 kimi
/// 自己显示的名字，未知则用末段本身（不致空白）。
fn model_display(alias: &str) -> String {
    let last = alias.rsplit('/').next().unwrap_or(alias);
    match last {
        "kimi-for-coding" => "K2.7 Code".to_string(),
        other => other.to_string(),
    }
}

/// 纯解析：从 wire.jsonl 文本取最近 AI 文本（拼接最后一回合的 text 片段、跳过 think）+ 模型展示名。
/// 便于单测，不碰文件系统。
pub fn parse_wire(content: &str) -> WireSummary {
    let mut buf = String::new();
    let mut model: Option<String> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            // 新用户回合开始 → 清空，使最终缓冲只剩最后一回合的 AI 文本。
            "turn.prompt" => buf.clear(),
            "context.append_message" => {
                let role = v
                    .get("message")
                    .and_then(|m| m.get("role"))
                    .and_then(|r| r.as_str());
                if role == Some("user") {
                    buf.clear();
                }
            }
            "config.update" => {
                if let Some(a) = v.get("modelAlias").and_then(|m| m.as_str()) {
                    model = Some(model_display(a));
                }
            }
            "context.append_loop_event" => {
                let part = v
                    .get("event")
                    .filter(|e| e.get("type").and_then(|t| t.as_str()) == Some("content.part"));
                let part = part.and_then(|e| e.get("part"));
                if part.and_then(|p| p.get("type")).and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = part.and_then(|p| p.get("text")).and_then(|t| t.as_str()) {
                        buf.push_str(t);
                    }
                }
            }
            _ => {}
        }
    }
    let s = buf.trim();
    WireSummary {
        last_ai: (!s.is_empty()).then(|| s.to_string()),
        model,
    }
}

/// 小文件全量读的上限；超过则改头/尾有界读，避免长会话下每个 Stop 都全量读+解析（近 O(n²)）。
const FULL_READ_CAP: u64 = 512 * 1024;
/// 大文件时：头部读这么多取模型（config.update 在文件靠前），尾部读这么多取最近 AI 正文。
const HEAD_BYTES: u64 = 16 * 1024;
const TAIL_BYTES: u64 = 256 * 1024;

/// 读文件 [offset, offset+len) 字节为 lossy UTF-8（边界处的半截行交给 parse_wire 跳过）。
fn read_range(path: &Path, offset: u64, len: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = Vec::with_capacity(len.min(TAIL_BYTES) as usize);
    f.take(len).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 读某 kimi 会话的 wire.jsonl 并解析（定位失败/读失败返回 None）。
/// wire.jsonl 是只增的会话日志，长会话可达数 MB；模型在头部、最近 AI 正文在尾部，故大文件分别
/// 头/尾有界读，small 文件仍一次全读。set_last_ai_text 本就截断 200 字，尾部窗口足够。
pub fn read_summary(session_id: &str) -> Option<WireSummary> {
    let wire = session_dir(session_id)?
        .join("agents")
        .join("main")
        .join("wire.jsonl");
    let size = std::fs::metadata(&wire).ok()?.len();
    if size <= FULL_READ_CAP {
        return Some(parse_wire(&std::fs::read_to_string(&wire).ok()?));
    }
    let head = read_range(&wire, 0, HEAD_BYTES).unwrap_or_default();
    let tail = read_range(&wire, size.saturating_sub(TAIL_BYTES), TAIL_BYTES).unwrap_or_default();
    Some(WireSummary {
        model: parse_wire(&head).model,
        last_ai: parse_wire(&tail).last_ai,
    })
}

/// 从 wire.jsonl 文本取**最后一条** usage.record 的 (used_input_tokens, model_alias)。
/// used = inputOther + inputCacheRead + inputCacheCreation（≈ 该回合请求发送时的 context 输入量，
/// 每次请求都把整个 context 作为 input 发送）；output 不计（本轮新生成，尚未进 context）。
pub fn parse_context(content: &str) -> Option<(i64, String)> {
    let mut last: Option<(i64, String)> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("usage.record") {
            continue;
        }
        let Some(u) = v.get("usage") else { continue };
        let field = |k: &str| u.get(k).and_then(|x| x.as_i64()).unwrap_or(0);
        let used = field("inputOther") + field("inputCacheRead") + field("inputCacheCreation");
        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        last = Some((used, model));
    }
    last
}

/// 读 config.toml 里 `[models."<alias>"]` 的 `max_context_size`；找不到回退 262144。
/// 逐行启发式解析（不引 toml 依赖，同 account/kimi.rs 既有范式）。
pub fn context_window(model_alias: &str) -> i64 {
    const FALLBACK: i64 = 262_144;
    let Some(inst) = kimi_install() else {
        return FALLBACK;
    };
    let Ok(content) = std::fs::read_to_string(inst.config_path()) else {
        return FALLBACK;
    };
    let want = format!("[models.\"{model_alias}\"]");
    let mut in_section = false;
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == want;
            continue;
        }
        if !in_section || t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix("max_context_size") {
            if let Some(after) = rest.trim_start().strip_prefix('=') {
                if let Ok(n) = after.trim().parse::<i64>() {
                    if n > 0 {
                        return n;
                    }
                }
            }
        }
    }
    FALLBACK
}

/// 读某 kimi 会话最近的上下文占用：wire.jsonl 尾部取最后一条 usage.record，used/window 算百分比。
/// 定位/读/解析失败返回 None。大文件尾部有界读（与 read_summary 同款）。
pub fn read_context(session_id: &str) -> Option<crate::caps::ContextUsage> {
    let wire = session_dir(session_id)?
        .join("agents")
        .join("main")
        .join("wire.jsonl");
    let size = std::fs::metadata(&wire).ok()?.len();
    let text = if size <= FULL_READ_CAP {
        std::fs::read_to_string(&wire).ok()?
    } else {
        read_range(&wire, size.saturating_sub(TAIL_BYTES), TAIL_BYTES)?
    };
    let (used, model) = parse_context(&text)?;
    let window = context_window(&model);
    if window <= 0 {
        return None;
    }
    let pct = (used * 100 / window).clamp(0, 100);
    Some(crate::caps::ContextUsage {
        used_pct: pct,
        window,
    })
}

/// 把某 kimi 会话改成自定义标题：改写 session `state.json` 的 `title` + `isCustomTitle=true`
/// （后者阻止 kimi 之后用 AI 标题覆盖，与 claude 的 custom-title 同义），使 kimi 自身会话列表与
/// `kimi -r` 列表也显示新名。其余字段原样保留。临时文件 + rename 原子写，避免与运行中的 kimi
/// 并发写 state.json 撕裂。定位/读/解析/写失败返回 false。
pub fn set_custom_title(session_id: &str, title: &str) -> bool {
    let Some(dir) = session_dir(session_id) else {
        return false;
    };
    let path = dir.join("state.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let Some(obj) = v.as_object_mut() else {
        return false;
    };
    obj.insert(
        "title".to_string(),
        serde_json::Value::String(title.to_string()),
    );
    obj.insert("isCustomTitle".to_string(), serde_json::Value::Bool(true));
    let Ok(s) = serde_json::to_string(&v) else {
        return false;
    };
    let tmp = path.with_extension("json.cckb-tmp");
    if std::fs::write(&tmp, s).is_err() {
        return false;
    }
    std::fs::rename(&tmp, &path).is_ok()
}

// ═══ 能力槽 ═══

pub struct KimiTelemetry;
pub static TELEMETRY: KimiTelemetry = KimiTelemetry;

impl crate::caps::TelemetryCap for KimiTelemetry {
    /// kimi 的 Stop hook 不带正文/模型 → 从 wire.jsonl 一次读出两者（避免双读）。
    fn stop_outputs(&self, ctx: &crate::caps::HookContext) -> crate::caps::StopOutputs {
        match read_summary(ctx.session_id) {
            Some(s) => crate::caps::StopOutputs {
                last_ai: s.last_ai,
                model: s.model,
            },
            None => crate::caps::StopOutputs::default(),
        }
    }

    fn read_context(&self, ctx: &crate::caps::HookContext) -> Option<crate::caps::ContextUsage> {
        read_context(ctx.session_id)
    }

    fn write_rename(&self, session_id: &str, _cwd: Option<&str>, title: &str) -> bool {
        set_custom_title(session_id, title)
    }

    fn transcript(&self) -> Option<&'static dyn TranscriptSpec> {
        Some(&KIMI_TRANSCRIPT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_independent_permission_and_work_modes() {
        assert_eq!(
            KIMI_TRANSCRIPT.agent_modes_from_line(
                r#"{"type":"permission.set_mode","mode":"yolo"}"#,
            ),
            vec![crate::AgentMode::new("permission", "yolo")]
        );
        assert_eq!(
            KIMI_TRANSCRIPT.agent_modes_from_line(r#"{"type":"plan_mode.enter"}"#),
            vec![crate::AgentMode::new("work", "plan")]
        );
        assert_eq!(
            KIMI_TRANSCRIPT.agent_modes_from_line(r#"{"type":"plan_mode.exit"}"#),
            vec![crate::AgentMode::new("work", "default")]
        );
    }

    #[test]
    fn parse_context_takes_last_usage_record_and_sums_inputs() {
        let wire = r#"
{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":100,"output":5,"inputCacheRead":200,"inputCacheCreation":0}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"hi"}}}
{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":727,"output":815,"inputCacheRead":20480,"inputCacheCreation":13}}
"#;
        // 取最后一条：727 + 20480 + 13 = 21220；output 不计。
        assert_eq!(
            parse_context(wire),
            Some((21220, "kimi-code/kimi-for-coding".to_string()))
        );
    }

    #[test]
    fn parse_context_none_when_no_usage_record() {
        let wire = r#"{"type":"turn.prompt","input":"hi"}"#;
        assert_eq!(parse_context(wire), None);
    }

    #[test]
    fn extracts_last_ai_text_and_model_skipping_think_and_prior_turns() {
        // 两个回合；应只取第二回合的 text，跳过 think、忽略第一回合；模型由 alias 映射。
        let wire = r#"
{"type":"config.update","modelAlias":"kimi-code/kimi-for-coding"}
{"type":"turn.prompt","input":"hi"}
{"type":"context.append_message","message":{"role":"user","content":"hi"}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"想一下"}}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"Hi! "}}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"How can I help?"}}}
{"type":"turn.prompt","input":"再见"}
{"type":"context.append_message","message":{"role":"user","content":"再见"}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"再见！"}}}
"#;
        let s = parse_wire(wire);
        assert_eq!(s.last_ai.as_deref(), Some("再见！"));
        assert_eq!(s.model.as_deref(), Some("K2.7 Code"));
    }

    #[test]
    fn none_when_no_text_parts() {
        let wire = r#"{"type":"turn.prompt","input":"hi"}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"only thinking"}}}"#;
        assert_eq!(parse_wire(wire).last_ai, None);
    }

    #[test]
    fn chat_parser_emits_user_and_mergeable_assistant_deltas() {
        let user = r#"{"type":"turn.prompt","input":"继续"}"#;
        let first = r#"{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"正在"}}}"#;
        let second = r#"{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"处理"}}}"#;
        let thinking = r#"{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"先分析"}}}"#;
        assert!(
            matches!(&parse_chat_items(user)[0], ChatItem::UserText { text, .. } if text == "继续")
        );
        assert!(
            matches!(&parse_chat_items(first)[0], ChatItem::AssistantDelta { text, .. } if text == "正在")
        );
        assert!(
            matches!(&parse_chat_items(second)[0], ChatItem::AssistantDelta { text, .. } if text == "处理")
        );
        assert!(matches!(
            &parse_chat_items(thinking)[0],
            ChatItem::ReasoningDelta { text, .. } if text == "先分析"
        ));
    }

    #[test]
    fn chat_parser_emits_context_append_message_user_content() {
        let string_content = r#"{"type":"context.append_message","timestamp":"2026-07-18T01:02:03Z","message":{"role":"user","content":"从这里继续"}}"#;
        let array_content = r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"第一段"},{"type":"text","text":"第二段"}]}}"#;
        let assistant = r#"{"type":"context.append_message","message":{"role":"assistant","content":"不要重复解析"}}"#;
        let injected = r#"{"type":"context.append_message","message":{"role":"user","content":"hidden reminder","origin":{"kind":"injection"}}}"#;

        assert!(matches!(
            &parse_chat_items(string_content)[0],
            ChatItem::UserText { timestamp: Some(timestamp), text, .. }
                if timestamp == "2026-07-18T01:02:03Z" && text == "从这里继续"
        ));
        assert!(matches!(
            &parse_chat_items(array_content)[0],
            ChatItem::UserText { text, .. } if text == "第一段第二段"
        ));
        assert!(parse_chat_items(assistant).is_empty());
        assert!(parse_chat_items(injected).is_empty());
    }

    #[test]
    fn chat_parser_matches_real_kimi_tool_fields() {
        let call = r#"{"type":"context.append_loop_event","event":{"type":"tool.call","toolCallId":"tool-1","name":"Bash","args":{"command":"cargo test"}}}"#;
        let result = r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"tool-1","result":{"output":"all green"}}}"#;
        assert!(matches!(
            &parse_chat_items(call)[0],
            ChatItem::ToolUse { id, summary, .. }
                if id == "tool-1" && summary.contains("cargo test")
        ));
        assert!(matches!(
            &parse_chat_items(result)[0],
            ChatItem::ToolResult { tool_use_id: Some(id), text, .. }
                if id == "tool-1" && text == "all green"
        ));
    }

    #[test]
    fn install_resolves_config_path_under_share_dir() {
        // 目录优先级本身由 meowo-agent 的变体表单测覆盖（不碰真实 home）；这里只守住薄封装的一致性：
        // config.toml 必须落在 share_dir 下，argv 非空且指向 kimi。
        let inst = kimi_install().expect("resolve 应总能给出实况或默认落点");
        assert_eq!(
            inst.config_path(),
            kimi_share_dir().unwrap().join("config.toml")
        );
        let argv = kimi_launch_argv();
        assert_eq!(argv.len(), 1);
        assert!(argv[0].to_ascii_lowercase().contains("kimi"));
    }
}
