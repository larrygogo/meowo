//! claude 的 transcript：`~/.claude/projects/<encode(cwd)>/<session_id>.jsonl` 的路径布局，
//! 与该 JSONL 的增量解析（标题 / 卡死错误 / 上下文占用 / 正文预览）。
//!
//! 这些代码此前住在 `meowo-store`（`analyze.rs` + `title.rs` + `transcript_spec.rs`），于是
//! 「读一个 JSONL 文件」平白拖着 rusqlite 依赖，claude 专属的路径布局也伪装成了通用的 store API。
//! 通用部分（`TranscriptInfo` / trait / `TranscriptCache`）见 `crate::transcript`。

use crate::transcript::{TranscriptInfo, TranscriptParser, TranscriptSpec, TurnError};

/// 上下文窗口基准（标准 200k）。1M-context 变体无法从 transcript 的 model 字段可靠识别，
/// 故统一按 200k 估算并封顶 100%；后续若需精确可按 model 调整。
const CONTEXT_WINDOW: u64 = 200_000;

// ═══ 解析：JSONL 逐行 fold ═══

/// 把 assistant 正文清洗成卡片预览：合并所有空白为单空格、按**字符**截断到 ~180。
/// 单次遍历完成「折叠空白 + 计数截断」，命中上限即提前返回——大消息不再整条 collapse/分配。
pub(crate) fn preview_text(s: &str) -> Option<String> {
    const MAX: usize = 180;
    let mut out = String::new();
    let mut count = 0usize; // out 中的字符数
    let mut pending_space = false; // 词间是否有待补的单空格（行首/行尾不补）
    for ch in s.chars() {
        if ch.is_whitespace() {
            if count > 0 {
                pending_space = true;
            }
            continue;
        }
        // 写入该非空白字符（连同可能的前导空格）前先判断是否会超限。
        let need = if pending_space { 2 } else { 1 };
        if count + need > MAX {
            out.push('…');
            return Some(out);
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        out.push(ch);
        count += 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 把 assistant 正文归类为「卡死错误」短标签；非卡死返回 None。
/// 刻意排除 529/500/ECONNRESET 等临时错误（多数自愈，标红会误报）。
/// 真实卡死错误都是独立短文案；长正文（如讨论/引用错误日志的正常回答）不判错，避免误报。
pub(crate) fn classify_error(text: &str) -> Option<&'static str> {
    let t = text.trim();
    if t.chars().count() > 200 {
        return None;
    }
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

/// 增量解析的累积状态：标题（custom/ai 分开存，custom 优先）、最近一条 assistant 正文、
/// 最近一条 usage。逐行 fold，故对「只追加」的 transcript 可跨多次调用累积，无需重头扫。
#[derive(Default, Clone)]
struct ParseState {
    custom: Option<String>,
    ai: Option<String>,
    last_text: Option<(String, String)>, // (正文, uuid)
    last_usage: Option<u64>,             // 最近一条 assistant 的上下文已用 token
}

impl ParseState {
    /// 折叠一行 JSONL：只关心 title / assistant 行，其它快速跳过（不解析）。
    fn fold_line(&mut self, line: &str) {
        let has_title = line.contains("-title");
        let has_assistant = line.contains("\"assistant\"");
        if !has_title && !has_assistant {
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("custom-title") => {
                if let Some(s) = v.get("customTitle").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        self.custom = Some(s.to_string());
                    }
                }
            }
            Some("ai-title") => {
                if let Some(s) = v.get("aiTitle").and_then(|x| x.as_str()) {
                    if !s.trim().is_empty() {
                        self.ai = Some(s.to_string());
                    }
                }
            }
            Some("assistant") => {
                // 上下文已用量：每条 assistant（含纯 tool_use）都带 usage，取最新一条。
                if let Some(u) = v.get("message").and_then(|m| m.get("usage")) {
                    let g = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    let used = g("input_tokens")
                        + g("cache_creation_input_tokens")
                        + g("cache_read_input_tokens")
                        + g("output_tokens");
                    if used > 0 {
                        self.last_usage = Some(used);
                    }
                }
                // 取该 assistant 消息 content 数组里所有 text 块，空格拼接（对齐 moshi）；无 text 块则 None（如纯 tool_use）。
                let text = v
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                    .and_then(|arr| {
                        let joined = arr
                            .iter()
                            .filter(|x| x.get("type").and_then(|t| t.as_str()) == Some("text"))
                            .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join(" ");
                        if joined.is_empty() { None } else { Some(joined) }
                    });
                if let Some(text) = text {
                    let uuid = v.get("uuid").and_then(|u| u.as_str()).unwrap_or("").to_string();
                    self.last_text = Some((text, uuid));
                }
            }
            _ => {}
        }
    }

    /// 从累积状态产出 TranscriptInfo。
    fn to_info(&self) -> TranscriptInfo {
        let error = self.last_text.as_ref().and_then(|(text, uuid)| {
            classify_error(text).map(|label| TurnError {
                label: label.to_string(),
                raw: text.clone(),
                fingerprint: uuid.clone(),
            })
        });
        let context_pct = self
            .last_usage
            .map(|u| ((u as f64 / CONTEXT_WINDOW as f64) * 100.0).round().min(100.0) as u8);
        TranscriptInfo {
            title: self.custom.clone().or_else(|| self.ai.clone()),
            error,
            context_tokens: self.last_usage,
            context_pct,
            preview: self.last_text.as_ref().and_then(|(t, _)| preview_text(t)),
        }
    }
}

/// 单次遍历 transcript（全量）：解析标题（custom-title 优先于 ai-title）、最后一条 assistant
/// 正文（卡死归类）与上下文已用量。读不到/空 → 全 None。热路径请用 [`crate::TranscriptCache`]。
pub fn analyze_transcript(path: &str) -> TranscriptInfo {
    let Ok(content) = std::fs::read_to_string(path) else {
        return TranscriptInfo::default();
    };
    let mut st = ParseState::default();
    for line in content.lines() {
        st.fold_line(line);
    }
    st.to_info()
}

/// ClaudeParser：把私有的 ParseState 包成 TranscriptParser trait 对象（逐字节等价，仅转发）。
struct ClaudeParser(ParseState);

impl TranscriptParser for ClaudeParser {
    fn fold_line(&mut self, line: &str) {
        self.0.fold_line(line);
    }
    fn to_info(&self) -> TranscriptInfo {
        self.0.to_info()
    }
}

// ═══ 路径布局：~/.claude/projects/<encode(cwd)>/<session_id>.jsonl ═══

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

/// projects 目录。刻意直查 `~/.claude/projects` 而不走变体表的 data_dir：`CLAUDE_CONFIG_DIR`
/// 只搬走 settings/credentials，transcript 仍落在 home 下的 `.claude`（与 Claude Code 行为一致）。
fn projects_dir() -> Option<std::path::PathBuf> {
    Some(crate::home_dir()?.join(".claude").join("projects"))
}

/// 根据 cwd + session_id 重建 transcript 路径：
/// ~/.claude/projects/<encode(cwd)>/<session_id>.jsonl。
pub fn reconstruct_transcript_path(cwd: &str, session_id: &str) -> Option<std::path::PathBuf> {
    Some(projects_dir()?.join(encode_cwd(cwd)).join(format!("{session_id}.jsonl")))
}

/// 不依赖 cwd，直接在 ~/.claude/projects/*/ 下按 `<session_id>.jsonl` 找 transcript。
/// transcript 文件名即 session_id（全局唯一），对 cwd 缺失/编码不一致都免疫。
pub fn find_transcript_by_session(session_id: &str) -> Option<std::path::PathBuf> {
    let projects = projects_dir()?;
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

/// 解析 transcript 文件路径，依次尝试：1) hook 给的 path；2) cwd+session_id 重建；
/// 3) 按 session_id 全局查找。供「同时要标题+错误」的调用方先拿路径再 analyze。
/// 注意：与 resolve_title 不同，本函数只做路径定位，不保证文件内含有标题；
/// 第一个候选文件存在即返回，不会因「文件无标题」继续回落。
fn resolve_path(transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<std::path::PathBuf> {
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

// ═══ TranscriptSpec 实现 ═══

/// Claude Code 的 transcript 规格。
pub struct ClaudeTranscript;

/// 全局唯一 claude transcript 规格实例，供插件的 transcript 能力槽以 &'static 返回。
pub static CLAUDE_TRANSCRIPT: ClaudeTranscript = ClaudeTranscript;

impl TranscriptSpec for ClaudeTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::new(ClaudeParser(ParseState::default()))
    }

    fn resolve_transcript_path(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<std::path::PathBuf> {
        resolve_path(transcript_path, cwd, session_id)
    }

    /// 解析会话标题，依次尝试：
    /// 1) hook 给的 transcript_path；2) cwd+session_id 重建路径；3) 按 session_id 全局查找。
    fn resolve_title(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<String> {
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

    /// 已知 cwd（DB 记录）不再盲信：先校验其对应目录下确有该会话的 transcript。DB 的 cwd 可能
    /// 失真——会话早于 hook 接线、SessionStart 丢失、项目目录事后被移动/重命名——盲信会让
    /// `claude --resume` 在错误目录下启动、报「No conversation found」，且只能靠用户在 Claude Code
    /// 里手动 resume 一次（SessionStart hook 重写 cwd）才自愈。校验不过则按 session_id 全局反查
    /// transcript、从其内容读出权威 cwd；全局也找不到（transcript 已被 Claude Code 按
    /// cleanupPeriodDays 清理）时回退 DB cwd。
    fn resolve_cwd(&self, cwd: Option<&str>, session_id: &str) -> Option<String> {
        let known = crate::transcript::default_resolve_cwd(cwd);
        if let Some(c) = &known {
            if reconstruct_transcript_path(c, session_id).is_some_and(|p| p.exists()) {
                return known;
            }
        }
        find_transcript_by_session(session_id)
            .and_then(|p| cwd_from_transcript(p.to_str()?))
            .or(known)
    }
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

    #[test]
    fn classify_ignores_long_text_quoting_error() {
        // 正常长回答里引用错误文案（如调试 API 的会话）不应被判为卡死。
        let long = format!("{}先看日志里的 API Error: 403 Request not allowed，这是因为……", "分析：".repeat(100));
        assert_eq!(classify_error(&long), None);
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
    fn preview_text_collapses_and_truncates() {
        assert_eq!(preview_text("  hi\n\n  there  "), Some("hi there".to_string()));
        assert_eq!(preview_text("   \n\t  "), None);
        let long: String = "あ".repeat(200);
        let p = preview_text(&long).unwrap();
        // 按字符截断到 180 + 省略号；多字节字符不会被截半。
        assert_eq!(p.chars().count(), 181);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn analyze_concatenates_multiple_text_blocks_in_one_assistant() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"先说开场白"},{"type":"tool_use","id":"t","name":"Bash","input":{}},{"type":"text","text":"再说结论"}]}}"#, "\n",
        );
        let p = write_tmp("concat", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.preview.as_deref(), Some("先说开场白 再说结论"));
    }

    #[test]
    fn analyze_exposes_last_assistant_preview() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"first turn"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"u2","message":{"role":"assistant","content":[{"type":"text","text":"  need your\n  confirmation  "}]}}"#, "\n",
        );
        let p = write_tmp("preview", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.preview.as_deref(), Some("need your confirmation"));
    }

    #[test]
    fn analyze_missing_file_is_empty() {
        let info = analyze_transcript("C:/no/such/file-xyz.jsonl");
        assert_eq!(info, TranscriptInfo::default());
    }

    #[test]
    fn analyze_extracts_latest_context_usage() {
        // 两条 assistant：取最新一条的 usage。50000+50000+0+10000 = 110000 → 55%。
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":10,"cache_creation_input_tokens":1000,"cache_read_input_tokens":2000,"output_tokens":500},"content":[{"type":"text","text":"早些的回合"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"u2","message":{"role":"assistant","usage":{"input_tokens":50000,"cache_creation_input_tokens":50000,"cache_read_input_tokens":0,"output_tokens":10000},"content":[{"type":"tool_use","name":"Bash","input":{}}]}}"#, "\n",
        );
        let p = write_tmp("usage", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.context_tokens, Some(110_000));
        assert_eq!(info.context_pct, Some(55));
    }

    #[test]
    fn analyze_context_pct_caps_at_100() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":300000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"超长上下文"}]}}"#, "\n",
        );
        let p = write_tmp("usage_cap", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.context_pct, Some(100));
    }

    #[test]
    fn analyze_no_usage_is_none() {
        let content = concat!(
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","content":[{"type":"text","text":"没有 usage 字段"}]}}"#, "\n",
        );
        let p = write_tmp("usage_none", content);
        let info = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(info.context_tokens, None);
        assert_eq!(info.context_pct, None);
    }

    /// 增量解析器逐行 fold 的结果须与 analyze_transcript 全量解析逐字段一致。
    #[test]
    fn claude_parser_matches_full_scan() {
        let content = concat!(
            r#"{"type":"ai-title","aiTitle":"标题X"}"#, "\n",
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":40000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"hi there"}]}}"#, "\n",
        );
        let mut parser = CLAUDE_TRANSCRIPT.new_parser();
        for line in content.lines() {
            parser.fold_line(line);
        }
        let p = write_tmp("parser_full", content);
        let full = analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(parser.to_info(), full);
        assert_eq!(parser.to_info().title.as_deref(), Some("标题X"));
        assert_eq!(parser.to_info().context_tokens, Some(40000));
    }

    #[test]
    fn resolve_title_reads_custom_title() {
        let p = write_tmp("resolve_title", "{\"type\":\"custom-title\",\"customTitle\":\"我的标题\"}\n");
        let path = p.to_str().unwrap();
        let via_spec = CLAUDE_TRANSCRIPT.resolve_title(Some(path), None, "sid");
        let via_fn = title_from_transcript(path);
        std::fs::remove_file(&p).ok();
        assert_eq!(via_spec, via_fn);
        assert_eq!(via_spec.as_deref(), Some("我的标题"));
    }

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
        let content = concat!(
            "{\"type\":\"leafUuid\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"permissionMode\",\"sessionId\":\"s\"}\n",
            "{\"type\":\"user\",\"cwd\":\"C:\\\\Users\\\\me\\\\proj\",\"sessionId\":\"s\"}\n",
        );
        let path = write_tmp("cwd_test", content);
        let got = cwd_from_transcript(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        assert_eq!(got.as_deref(), Some(r"C:\Users\me\proj"));
    }

    #[test]
    fn resolve_cwd_prefers_known() {
        // 已知 cwd 校验不过（其下无 transcript）且全局也找不到 → 回退已知 cwd（已清理场景）。
        assert_eq!(CLAUDE_TRANSCRIPT.resolve_cwd(Some(r"C:\a\b"), "anyid").as_deref(), Some(r"C:\a\b"));
        assert_eq!(CLAUDE_TRANSCRIPT.resolve_cwd(Some("  "), "no-such-session-id-xxx"), None);
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
        let corrected = CLAUDE_TRANSCRIPT.resolve_cwd(Some(r"C:\stale\gone"), &sid);
        let verified_ok = CLAUDE_TRANSCRIPT.resolve_cwd(Some(r"C:\real\proj"), &sid); // 校验通过 → 原样返回，不做全局扫描
        match old_home {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(corrected.as_deref(), Some(r"C:\real\proj"));
        assert_eq!(verified_ok.as_deref(), Some(r"C:\real\proj"));
    }

    /// 非 claude agent 走默认实现：直接采信 DB cwd，不去翻 ~/.claude/projects。
    #[test]
    fn default_resolve_cwd_trusts_db_value() {
        use crate::transcript::default_resolve_cwd;
        assert_eq!(default_resolve_cwd(Some("/x/y")).as_deref(), Some("/x/y"));
        assert_eq!(default_resolve_cwd(Some("   ")), None);
        assert_eq!(default_resolve_cwd(None), None);
    }
}
