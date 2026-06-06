//! 从 Claude Code transcript 检测「致命卡死错误」并与标题解析共用一次文件读取。
use serde::Serialize;

/// 检测到的回合错误：短中文标签 + 原始文案 + 去重指纹（出错 assistant 消息的 uuid）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnError {
    pub label: String,
    pub raw: String,
    pub fingerprint: String,
}

/// 单次扫 transcript 的产物：标题与错误。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptInfo {
    pub title: Option<String>,
    pub error: Option<TurnError>,
}

/// 把 assistant 正文归类为「卡死错误」短标签；非卡死返回 None。
/// 刻意排除 529/500/ECONNRESET 等临时错误（多数自愈，标红会误报）。
pub fn classify_error(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.contains("could not be parsed (retry also failed)") {
        return Some("工具调用解析失败");
    }
    if t.starts_with("Please run /login") || t.contains("API Error: 403") {
        return Some("需要重新登录");
    }
    if t.starts_with("Failed to authenticate") || t.contains("API Error: 401") {
        return Some("认证失败");
    }
    None
}

/// 单次遍历 transcript：同时解析标题（custom-title 优先于 ai-title）与
/// 「最后一条带 text 的 assistant 正文」，对该正文做卡死归类。读不到/空 → 全 None。
pub fn analyze_transcript(path: &str) -> TranscriptInfo {
    let Ok(content) = std::fs::read_to_string(path) else {
        return TranscriptInfo::default();
    };
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    let mut last_text: Option<(String, String)> = None; // (正文, uuid)

    for line in content.lines() {
        let has_title = line.contains("-title");
        let has_assistant = line.contains("\"assistant\"");
        if !has_title && !has_assistant {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("custom-title") => {
                if let Some(s) = v.get("customTitle").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        custom = Some(s.to_string());
                    }
                }
            }
            Some("ai-title") => {
                if let Some(s) = v.get("aiTitle").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        ai = Some(s.to_string());
                    }
                }
            }
            Some("assistant") => {
                // 取该 assistant 消息 content 数组里第一个 text 块；无 text 块则跳过（如纯 tool_use）。
                let text = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|arr| {
                        arr.iter().find_map(|x| {
                            if x.get("type").and_then(|t| t.as_str()) == Some("text") {
                                x.get("text").and_then(|t| t.as_str()).map(|s| s.to_string())
                            } else {
                                None
                            }
                        })
                    });
                if let Some(text) = text {
                    let uuid = v.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string();
                    last_text = Some((text, uuid));
                }
            }
            _ => {}
        }
    }

    let error = last_text.and_then(|(text, uuid)| {
        classify_error(&text).map(|label| TurnError {
            label: label.to_string(),
            raw: text,
            fingerprint: uuid,
        })
    });
    TranscriptInfo { title: custom.or(ai), error }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_matches_stuck_errors() {
        assert_eq!(
            classify_error("The model's tool call could not be parsed (retry also failed)."),
            Some("工具调用解析失败")
        );
        assert_eq!(
            classify_error("Please run /login · API Error: 403 Request not allowed"),
            Some("需要重新登录")
        );
        assert_eq!(classify_error("API Error: 403 Request not allowed"), Some("需要重新登录"));
        assert_eq!(
            classify_error("Failed to authenticate. API Error: 401 Invalid authentication credentials"),
            Some("认证失败")
        );
        assert_eq!(classify_error("API Error: 401 Invalid authentication credentials"), Some("认证失败"));
    }

    #[test]
    fn classify_ignores_transient_and_normal() {
        assert_eq!(classify_error("API Error: 529 Overloaded. This is a server-side issue"), None);
        assert_eq!(classify_error("API Error: 500 status code (no body)"), None);
        assert_eq!(classify_error("Unable to connect to API (ECONNRESET)"), None);
        assert_eq!(classify_error("这是一段正常的助手回答。"), None);
    }

    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("cc_analyze_{}_{}.jsonl", std::process::id(), name));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn analyze_detects_parse_abort_and_title() {
        let content = concat!(
            r#"{"type":"ai-title","aiTitle":"做某功能"}"#, "\n",
            r#"{"type":"assistant","uuid":"u-err-1","message":{"role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"text","text":"The model's tool call could not be parsed (retry also failed)."}]}}"#, "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":1000}"#, "\n",
        );
        let p = write_tmp("parse", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.title.as_deref(), Some("做某功能"));
        let e = info.error.expect("应检测到错误");
        assert_eq!(e.label, "工具调用解析失败");
        assert_eq!(e.fingerprint, "u-err-1");
    }

    #[test]
    fn analyze_no_error_on_normal_ending() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"已完成，结果如下。"}]}}"#, "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":500}"#, "\n",
        );
        let p = write_tmp("normal", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.error, None);
    }

    #[test]
    fn analyze_recovered_after_error_not_flagged() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u-err","message":{"role":"assistant","content":[{"type":"text","text":"The model's tool call could not be parsed (retry also failed)."}]}}"#, "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":100}"#, "\n",
            r#"{"type":"user","message":{"role":"user","content":"继续"}}"#, "\n",
            r#"{"type":"assistant","uuid":"u-ok","message":{"role":"assistant","content":[{"type":"text","text":"好的，已经修好了。"}]}}"#, "\n",
            r#"{"type":"system","subtype":"turn_duration","durationMs":200}"#, "\n",
        );
        let p = write_tmp("recover", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.error, None);
    }

    #[test]
    fn analyze_skips_tooluse_only_assistant() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u-err","message":{"role":"assistant","content":[{"type":"text","text":"Please run /login · API Error: 403 Request not allowed"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"u-tool","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{}}]}}"#, "\n",
        );
        let p = write_tmp("toolonly", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.error.map(|e| e.label), Some("需要重新登录".to_string()));
    }

    #[test]
    fn analyze_missing_file_is_empty() {
        let info = analyze_transcript("C:/no/such/file-xyz.jsonl");
        assert_eq!(info, TranscriptInfo::default());
    }
}
