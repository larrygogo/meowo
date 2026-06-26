//! kimi-code 会话记录解析。kimi 的 Stop hook **不带** AI 正文（只有 session_id/cwd），
//! 故需从会话的 `agents/main/wire.jsonl` 里读最近一条 AI 文本。
//!
//! wire.jsonl 结构（kimi-code 0.19.2 实测）：每行一个事件，AI 正文在
//! `type="context.append_loop_event"` 且 `event.type="content.part"` 且 `event.part.type="text"`
//! 的 `event.part.text` 里（`part.type="think"` 是思考过程，跳过）。用户输入则是 `type="turn.prompt"`
//! 或 `type="context.append_message"` 且 `message.role="user"`——遇到即清空缓冲，使最终缓冲恰为
//! 「最后一条用户输入之后的 AI 文本」。

use std::path::PathBuf;

/// kimi 数据根：`KIMI_SHARE_DIR` 优先，否则 `~/.kimi-code`（迁移后的默认目录，非旧的 ~/.kimi）。
fn kimi_share_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("KIMI_SHARE_DIR") {
        if !d.is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(PathBuf::from(home).join(".kimi-code"))
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

/// 纯解析：从 wire.jsonl 文本取「最后一条用户输入之后」的 AI 文本（拼接各 text 片段，跳过 think）。
/// 便于单测，不碰文件系统。
pub fn last_ai_text_from_wire(content: &str) -> Option<String> {
    let mut buf = String::new();
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
            "context.append_loop_event" => {
                let part = v.get("event").filter(|e| {
                    e.get("type").and_then(|t| t.as_str()) == Some("content.part")
                });
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
    (!s.is_empty()).then(|| s.to_string())
}

/// 取某 kimi 会话最近一条 AI 正文（定位 wire.jsonl 后调纯解析）。任一步失败均返回 None。
pub fn last_ai_text(session_id: &str) -> Option<String> {
    let wire = session_dir(session_id)?
        .join("agents")
        .join("main")
        .join("wire.jsonl");
    let content = std::fs::read_to_string(wire).ok()?;
    last_ai_text_from_wire(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_last_ai_text_skipping_think_and_prior_turns() {
        // 两个回合；应只取第二回合的 text，跳过 think、忽略第一回合。
        let wire = r#"
{"type":"turn.prompt","input":"hi"}
{"type":"context.append_message","message":{"role":"user","content":"hi"}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"想一下"}}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"Hi! "}}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"How can I help?"}}}
{"type":"turn.prompt","input":"再见"}
{"type":"context.append_message","message":{"role":"user","content":"再见"}}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"text","text":"再见！"}}}
"#;
        assert_eq!(last_ai_text_from_wire(wire).as_deref(), Some("再见！"));
    }

    #[test]
    fn none_when_no_text_parts() {
        let wire = r#"{"type":"turn.prompt","input":"hi"}
{"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"only thinking"}}}"#;
        assert_eq!(last_ai_text_from_wire(wire), None);
    }
}
