//! 从 Claude Code transcript 检测「致命卡死错误」并与标题解析共用一次文件读取。
use serde::Serialize;

/// 上下文窗口基准（标准 200k）。1M-context 变体无法从 transcript 的 model 字段可靠识别，
/// 故统一按 200k 估算并封顶 100%；后续若需精确可按 model 调整。
const CONTEXT_WINDOW: u64 = 200_000;

/// 检测到的回合错误：短中文标签 + 原始文案 + 去重指纹（出错 assistant 消息的 uuid）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnError {
    pub label: String,
    pub raw: String,
    pub fingerprint: String,
}

/// 单次扫 transcript 的产物：标题、错误与上下文已用量。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptInfo {
    pub title: Option<String>,
    pub error: Option<TurnError>,
    /// 最近一条带 usage 的 assistant 回合的「上下文已用 token 数」
    /// = input + cache_creation + cache_read + output。无 usage 时为 None。
    pub context_tokens: Option<u64>,
    /// 上下文已用百分比（相对 200k 标准窗口，封顶 100）。
    pub context_pct: Option<u8>,
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
        }
    }
}

/// 单次遍历 transcript（全量）：解析标题（custom-title 优先于 ai-title）、最后一条 assistant
/// 正文（卡死归类）与上下文已用量。读不到/空 → 全 None。热路径请用 [`TranscriptCache`]。
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

/// 单条缓存：已解析到的字节偏移 + 上次解析时的 mtime + 累积状态 + 最近使用刻度（淘汰用）。
struct CacheEntry {
    offset: u64,
    mtime: Option<std::time::SystemTime>,
    state: ParseState,
    last_used: u64,
}

/// transcript 增量解析缓存：transcript 是只追加的 JSONL，没必要每轮把整文件重读重解析
/// （几十 MB → 数百 ms，多个会话叠加可达数秒，每 ~300ms 一次会打满 CPU、拖慢整窗）。
/// 这里按文件路径缓存「已解析到的字节偏移 + 累积状态」，每轮只读+解析新追加的完整行，
/// 把每次刷新从 O(整文件) 降到 O(新增字节) ≈ 接近 0。
#[derive(Default)]
pub struct TranscriptCache {
    entries: std::collections::HashMap<String, CacheEntry>,
    tick: u64, // 单调递增的访问刻度，供 LRU 淘汰
}

/// 缓存条目上限：超出时淘汰最久未访问的条目，防长期运行无界增长。
const MAX_CACHE_ENTRIES: usize = 256;

impl TranscriptCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 增量解析 path：只处理上次偏移之后新追加的「完整行」（末尾未结束的半行留到下次）。
    /// 失效检测用 len + mtime 双重校验：len < 偏移（截断）或 len == 偏移但 mtime 变了
    /// （等长重写）→ 从头重解析。打开/读失败 → 返回当前累积结果。
    pub fn analyze(&mut self, path: &str) -> TranscriptInfo {
        use std::io::{Read, Seek, SeekFrom};
        self.tick += 1;
        // 容量上限：插入新 key 前先淘汰最久未访问的条目。
        if !self.entries.contains_key(path) && self.entries.len() >= MAX_CACHE_ENTRIES {
            if let Some(k) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&k);
            }
        }
        let tick = self.tick;
        let entry = self.entries.entry(path.to_string()).or_insert_with(|| CacheEntry {
            offset: 0,
            mtime: None,
            state: ParseState::default(),
            last_used: tick,
        });
        entry.last_used = tick;

        let Ok(mut f) = std::fs::File::open(path) else {
            return entry.state.to_info();
        };
        let (len, mtime) = match f.metadata() {
            Ok(m) => (m.len(), m.modified().ok()),
            Err(_) => (0, None),
        };
        if len < entry.offset || (len == entry.offset && mtime != entry.mtime) {
            // 被截断，或等长但 mtime 变了（同长度重写）→ 重头解析。
            entry.offset = 0;
            entry.state = ParseState::default();
        }
        if len == entry.offset {
            entry.mtime = mtime;
            return entry.state.to_info(); // 无新增，直接复用
        }
        if f.seek(SeekFrom::Start(entry.offset)).is_err() {
            return entry.state.to_info();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return entry.state.to_info();
        }
        // 只吃到最后一个换行为止，保证按完整行解析；其后半行（writer 可能正写一半）留到下次。
        if let Some(nl) = buf.iter().rposition(|&b| b == b'\n') {
            entry.offset += (nl + 1) as u64;
            let chunk = String::from_utf8_lossy(&buf[..=nl]);
            for line in chunk.lines() {
                entry.state.fold_line(line);
            }
        }
        entry.mtime = mtime;
        entry.state.to_info()
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
    fn cache_incremental_matches_full_and_picks_up_appends() {
        use std::io::Write;
        let p = write_tmp(
            "cache_inc",
            concat!(
                r#"{"type":"ai-title","aiTitle":"标题A"}"#, "\n",
                r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"hi"}]}}"#, "\n",
            ),
        );
        let mut cache = TranscriptCache::new();
        let i1 = cache.analyze(p.to_str().unwrap());
        assert_eq!(i1.title.as_deref(), Some("标题A"));
        assert_eq!(i1.context_tokens, Some(1000));

        // 追加新一轮（带更大 usage + 自定义标题），增量解析应读到。
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(
            f,
            r#"{{"type":"custom-title","customTitle":"标题B"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","uuid":"u2","message":{{"role":"assistant","usage":{{"input_tokens":40000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}},"content":[{{"type":"tool_use","name":"Bash","input":{{}}}}]}}}}"#
        )
        .unwrap();
        drop(f);

        let i2 = cache.analyze(p.to_str().unwrap());
        // 与全量解析结果一致
        let full = analyze_transcript(p.to_str().unwrap());
        assert_eq!(i2.title.as_deref(), Some("标题B")); // custom 覆盖 ai
        assert_eq!(i2.context_tokens, Some(40000));
        assert_eq!(i2.title, full.title);
        assert_eq!(i2.context_tokens, full.context_tokens);

        // 再次调用、无新增 → 结果稳定。
        let i3 = cache.analyze(p.to_str().unwrap());
        assert_eq!(i3.context_tokens, Some(40000));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn cache_detects_same_length_rewrite_by_mtime() {
        // 等长重写：len 不变但 mtime 变了 → 应从头重解析，而不是沿用旧状态。
        let line_a = r#"{"type":"ai-title","aiTitle":"AAAA"}"#;
        let line_b = r#"{"type":"ai-title","aiTitle":"BBBB"}"#;
        assert_eq!(line_a.len(), line_b.len());
        let p = write_tmp("cache_rewrite", &format!("{line_a}\n"));
        let mut cache = TranscriptCache::new();
        assert_eq!(cache.analyze(p.to_str().unwrap()).title.as_deref(), Some("AAAA"));

        // 等长重写，循环到 mtime 确认变化为止（兼容粗粒度文件系统，NTFS/APFS 首轮即过）。
        let mtime0 = std::fs::metadata(&p).unwrap().modified().unwrap();
        for _ in 0..120 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            std::fs::write(&p, format!("{line_b}\n")).unwrap();
            if std::fs::metadata(&p).unwrap().modified().unwrap() != mtime0 {
                break;
            }
        }
        assert_ne!(std::fs::metadata(&p).unwrap().modified().unwrap(), mtime0, "mtime 未变化，无法验证缓存失效");
        assert_eq!(cache.analyze(p.to_str().unwrap()).title.as_deref(), Some("BBBB"));
        std::fs::remove_file(&p).ok();
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
}
