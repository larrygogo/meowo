/// 从 CC transcript JSONL 取会话标题：最后一条 custom-title 优先，否则最后一条 ai-title。
/// 读不到/无标题返回 None。只解析含 "-title" 的行，避免全量 JSON 解析开销。
pub fn title_from_transcript(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    for line in content.lines() {
        if !line.contains("-title") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
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
            _ => {}
        }
    }
    custom.or(ai)
}
