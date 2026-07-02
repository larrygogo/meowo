//! 从 Claude Code transcript JSONL 解析会话标题。
//! reporter 写入时用、cc-app 读取展示时也用，统一放在 store 里共享。

/// 从 CC transcript JSONL 取会话标题：最后一条 custom-title 优先，否则最后一条 ai-title。
/// 读不到/无标题返回 None。只解析含 "-title" 的行，避免全量 JSON 解析开销。
pub fn title_from_transcript(path: &str) -> Option<String> {
    use std::io::BufRead;
    // 流式逐行读：transcript 可达数 MB，且 reporter 在每个 hook 事件都调用本函数，
    // 整体 read_to_string 会反复把整文件吃进内存——改 BufReader 降峰值内存（扫描复杂度不变）。
    let file = std::fs::File::open(path).ok()?;
    let mut custom: Option<String> = None;
    let mut ai: Option<String> = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { continue }; // 单行非 UTF-8 等只跳过，不放弃整文件
        if !line.contains("-title") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
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
/// 非 ASCII 字母数字的字符一律换成 `-`（与 CC 的 `[^a-zA-Z0-9] -> '-'` 规则一致，
/// 含下划线、中文、括号等）。
fn encode_cwd(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
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

/// 从 transcript JSONL 里读出会话工作目录(cwd)：取第一条带非空 "cwd" 字段的记录。
/// cwd 在文件靠前的消息记录里，故逐行读、命中即返回，避免把大文件整体读入。
pub fn cwd_from_transcript(path: &str) -> Option<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path).ok()?;
    for line in std::io::BufReader::new(file).lines() {
        // 单行读失败（如非 UTF-8 字节）只跳过该行，不放弃整个文件。
        let Ok(line) = line else { continue };
        if !line.contains("\"cwd\"") {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(c) = v.get("cwd").and_then(|x| x.as_str()) {
            if !c.trim().is_empty() {
                return Some(c.to_string());
            }
        }
    }
    None
}

/// 解析会话工作目录。用于「恢复会话」——claude --resume 必须在正确的项目目录下运行才找得到会话。
///
/// 已知 cwd（DB 记录）不再盲信：先校验其对应目录下确有该会话的 transcript。DB 的 cwd 可能
/// 失真——会话早于 hook 接线、SessionStart 丢失、项目目录事后被移动/重命名——盲信会让
/// claude --resume 在错误目录下启动、报「No conversation found」，且只能靠用户在 Claude Code
/// 里手动 resume 一次（SessionStart hook 重写 cwd）才自愈。校验不过则按 session_id 全局反查
/// transcript、从其内容读出权威 cwd；全局也找不到（kimi/codex 会话没有 claude transcript、
/// 或已被 Claude Code 按 cleanupPeriodDays 清理）时回退 DB cwd，维持非 claude 会话的原行为。
pub fn resolve_cwd(cwd: Option<&str>, session_id: &str) -> Option<String> {
    let known = cwd.filter(|c| !c.trim().is_empty()).map(str::to_string);
    if let Some(c) = &known {
        if reconstruct_transcript_path(c, session_id).is_some_and(|p| p.exists()) {
            return known;
        }
    }
    find_transcript_by_session(session_id)
        .and_then(|p| cwd_from_transcript(p.to_str()?))
        .or(known)
}

/// 解析 transcript 文件路径，依次尝试：1) hook 给的 path；2) cwd+session_id 重建；
/// 3) 按 session_id 全局查找。供「同时要标题+错误」的调用方先拿路径再 analyze。
/// 注意：与 resolve_title 不同，本函数只做路径定位，不保证文件内含有标题；
/// 第一个候选文件存在即返回，不会因「文件无标题」继续回落。
pub fn resolve_transcript_path(
    transcript_path: Option<&str>,
    cwd: Option<&str>,
    session_id: &str,
) -> Option<std::path::PathBuf> {
    if let Some(p) = transcript_path {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(cwd) = cwd {
        if let Some(p) = reconstruct_transcript_path(cwd, session_id) {
            if p.exists() {
                return Some(p);
            }
        }
    }
    find_transcript_by_session(session_id)
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

    #[test]
    fn encode_cwd_replaces_all_non_alphanumeric() {
        // CC 规则是 [^a-zA-Z0-9] 全替换：下划线、中文、括号都变 '-'。
        assert_eq!(encode_cwd(r"C:\a_b\my(中文)"), "C--a-b-my----");
    }

    #[test]
    fn cwd_from_transcript_skips_metadata_takes_message() {
        // 模拟真实 transcript：开头元数据无 cwd，消息记录才带 cwd。
        let dir = std::env::temp_dir();
        let path = dir.join(format!("cc_cwd_test_{}.jsonl", std::process::id()));
        let content = concat!(
            "{\"type\":\"leafUuid\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"permissionMode\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"user\",\"cwd\":\"C:\\\\Users\\\\me\\\\proj\",\"sessionId\":\"s\"}\n",
        );
        std::fs::write(&path, content).unwrap();
        let got = cwd_from_transcript(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert_eq!(got.as_deref(), Some(r"C:\Users\me\proj"));
    }

    #[test]
    fn resolve_cwd_prefers_known() {
        // 已知 cwd 校验不过（其下无 transcript）且全局也找不到 → 回退已知 cwd（kimi/codex/已清理场景）。
        assert_eq!(resolve_cwd(Some(r"C:\a\b"), "anyid").as_deref(), Some(r"C:\a\b"));
        assert_eq!(resolve_cwd(Some("  "), "no-such-session-id-xxx"), None); // 空 cwd 且找不到 transcript
    }

    #[test]
    fn resolve_cwd_corrects_stale_db_cwd_via_global_search() {
        // DB 记录的 cwd 已失真（其对应目录下没有该会话的 transcript）时，应按 session_id 全局反查
        // 并从 transcript 内容读出权威 cwd——否则 resume 会在错误目录下启动、报 No conversation found，
        // 用户只能去 Claude Code 手动 resume 一次（hook 重写 cwd）才能自愈。
        let sid = format!("resolve-cwd-stale-{}", std::process::id());
        let home = std::env::temp_dir().join(format!("cc_home_{}", std::process::id()));
        // encode_cwd(r"C:\real\proj") == "C--real-proj"
        let proj = home.join(".claude").join("projects").join("C--real-proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join(format!("{sid}.jsonl")),
            format!("{{\"type\":\"user\",\"cwd\":\"C:\\\\real\\\\proj\",\"sessionId\":\"{sid}\"}}\n"),
        )
        .unwrap();
        let old_home = std::env::var("USERPROFILE").ok();
        std::env::set_var("USERPROFILE", &home);
        let corrected = resolve_cwd(Some(r"C:\stale\gone"), &sid);
        let verified_ok = resolve_cwd(Some(r"C:\real\proj"), &sid); // 校验通过 → 原样返回，不做全局扫描
        match old_home {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(corrected.as_deref(), Some(r"C:\real\proj"));
        assert_eq!(verified_ok.as_deref(), Some(r"C:\real\proj"));
    }
}
