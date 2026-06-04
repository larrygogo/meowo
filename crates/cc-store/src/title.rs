//! 从 Claude Code transcript JSONL 解析会话标题。
//! reporter 写入时用、cc-app 读取展示时也用，统一放在 store 里共享。

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

/// 把 cwd 编码成 Claude Code 在 ~/.claude/projects 下的子目录名：
/// 把 `\ / : . 空格` 都换成 `-`（与 CC 的编码一致）。
fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if matches!(c, '\\' | '/' | ':' | '.' | ' ') { '-' } else { c })
        .collect()
}

/// 根据 cwd + session_id 重建 transcript 路径：
/// ~/.claude/projects/<encode(cwd)>/<session_id>.jsonl。
pub fn reconstruct_transcript_path(cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(
        std::path::Path::new(&home)
            .join(".claude")
            .join("projects")
            .join(encode_cwd(cwd))
            .join(format!("{session_id}.jsonl")),
    )
}

/// 不依赖 cwd，直接在 ~/.claude/projects/*/ 下按 `<session_id>.jsonl` 找 transcript。
/// transcript 文件名即 session_id（全局唯一），对 cwd 缺失/编码不一致都免疫。
pub fn find_transcript_by_session(session_id: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    let projects = std::path::Path::new(&home).join(".claude").join("projects");
    let file = format!("{session_id}.jsonl");
    for entry in std::fs::read_dir(&projects).ok()?.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let candidate = entry.path().join(&file);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 解析会话标题，依次尝试：
/// 1) hook 给的 transcript_path；2) cwd+session_id 重建路径；3) 按 session_id 全局查找。
pub fn resolve_title(
    transcript_path: Option<&str>,
    cwd: Option<&str>,
    session_id: &str,
) -> Option<String> {
    if let Some(p) = transcript_path {
        if std::path::Path::new(p).exists() {
            if let Some(t) = title_from_transcript(p) {
                return Some(t);
            }
        }
    }
    if let Some(cwd) = cwd {
        if let Some(p) = reconstruct_transcript_path(cwd, session_id) {
            if let Some(t) = p.to_str().and_then(title_from_transcript) {
                return Some(t);
            }
        }
    }
    // 兜底：cwd 缺失（旧会话）或编码不一致时，按 session_id 直接找文件。
    let p = find_transcript_by_session(session_id)?;
    title_from_transcript(p.to_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_cwd_windows_path() {
        assert_eq!(encode_cwd(r"C:\Users\me\proj"), "C--Users-me-proj");
    }

    #[test]
    fn encode_cwd_unix_path() {
        assert_eq!(encode_cwd("/tmp/x y"), "-tmp-x-y");
    }
}
