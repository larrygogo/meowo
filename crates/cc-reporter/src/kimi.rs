//! kimi-code 会话记录解析。kimi 的 Stop hook **不带** AI 正文（只有 session_id/cwd），
//! 故需从会话的 `agents/main/wire.jsonl` 里读最近一条 AI 文本。
//!
//! wire.jsonl 结构（kimi-code 0.19.2 实测）：每行一个事件，AI 正文在
//! `type="context.append_loop_event"` 且 `event.type="content.part"` 且 `event.part.type="text"`
//! 的 `event.part.text` 里（`part.type="think"` 是思考过程，跳过）。用户输入则是 `type="turn.prompt"`
//! 或 `type="context.append_message"` 且 `message.role="user"`——遇到即清空缓冲，使最终缓冲恰为
//! 「最后一条用户输入之后的 AI 文本」。

use std::path::{Path, PathBuf};

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

/// kimi 可执行的绝对路径（~/.kimi-code/bin/kimi[.exe]）；找不到回退裸名 "kimi"（依赖 PATH）。
/// resume 用：cc-app 拉起的终端 PATH 未必含 kimi（或 kimi 是 shim/别名），故优先用绝对路径，
/// 避免 wt/powershell「系统找不到指定的文件」。
pub fn kimi_exe() -> String {
    let bin = if cfg!(windows) { "kimi.exe" } else { "kimi" };
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = kimi_share_dir() {
        cands.push(d.join("bin").join(bin));
    }
    // KIMI_SHARE_DIR 可能改了数据目录，但 bin 通常仍在 ~/.kimi-code/bin，单列一条兜底。
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        cands.push(PathBuf::from(home).join(".kimi-code").join("bin").join(bin));
    }
    cands
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "kimi".to_string())
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
    obj.insert("title".to_string(), serde_json::Value::String(title.to_string()));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
