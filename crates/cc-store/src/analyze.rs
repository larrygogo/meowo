//! 从 Claude Code transcript 检测「致命卡死错误」并与标题解析共用一次文件读取。
use serde::Serialize;
use crate::transcript_spec::{TranscriptParser, TranscriptSpec};

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
    /// 最近一条 assistant 正文的轻推预览（合并空白、截断）——供卡片 hover 速览，
    /// 不切终端就能判断该会话在问什么/说了什么。无正文回合（纯 tool_use）时为 None。
    pub preview: Option<String>,
}

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

/// ClaudeParser：把私有的 ParseState 包成 TranscriptParser trait 对象（逐字节等价，仅转发）。
pub struct ClaudeParser(ParseState);

impl TranscriptParser for ClaudeParser {
    fn fold_line(&mut self, line: &str) {
        self.0.fold_line(line);
    }
    fn to_info(&self) -> TranscriptInfo {
        self.0.to_info()
    }
}

/// 新建一个 claude 增量解析器（ClaudeTranscript::new_parser 委托此函数）。
pub fn claude_new_parser() -> Box<dyn TranscriptParser> {
    Box::new(ClaudeParser(ParseState::default()))
}

/// 单条缓存：已解析到的字节偏移 + 上次解析时的 mtime + 累积解析器 + 最近使用刻度（淘汰用）。
struct CacheEntry {
    offset: u64,
    mtime: Option<std::time::SystemTime>,
    parser: Box<dyn TranscriptParser>,
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
    /// `spec` 决定新建/重置条目时用哪种 provider 的解析器（claude 即 ClaudeParser）。
    pub fn analyze(&mut self, spec: &dyn TranscriptSpec, path: &str) -> TranscriptInfo {
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
            parser: spec.new_parser(),
            last_used: tick,
        });
        entry.last_used = tick;

        let Ok(mut f) = std::fs::File::open(path) else {
            return entry.parser.to_info();
        };
        let (len, mtime) = match f.metadata() {
            Ok(m) => (m.len(), m.modified().ok()),
            // metadata 偶发失败（I/O 抖动等）时**沿用已累积状态**，不要用 len=0 当真实长度——
            // 否则会被下面误判为「截断」而清空已解析的标题/错误，导致瞬时丢失（与 File::open 失败一致处理）。
            Err(_) => return entry.parser.to_info(),
        };
        if len < entry.offset || (len == entry.offset && mtime != entry.mtime) {
            // 被截断，或等长但 mtime 变了（同长度重写）→ 重头解析。
            entry.offset = 0;
            entry.parser = spec.new_parser();
        }
        if len == entry.offset {
            entry.mtime = mtime;
            return entry.parser.to_info(); // 无新增，直接复用
        }
        if f.seek(SeekFrom::Start(entry.offset)).is_err() {
            return entry.parser.to_info();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return entry.parser.to_info();
        }
        // 只吃到最后一个换行为止，保证按完整行解析；其后半行（writer 可能正写一半）留到下次。
        if let Some(nl) = buf.iter().rposition(|&b| b == b'\n') {
            entry.offset += (nl + 1) as u64;
            let chunk = String::from_utf8_lossy(&buf[..=nl]);
            for line in chunk.lines() {
                entry.parser.fold_line(line);
            }
        }
        entry.mtime = mtime;
        entry.parser.to_info()
    }

    /// 与 `analyze` 等价，但供多线程经 `Mutex` 共享缓存时调用：文件 IO（open/metadata/读新增字节）
    /// 全部在锁外进行，只有「取快照」与「提交结果」两个短临界区持锁——避免大 transcript 首读
    /// （数 MB、数百 ms）期间把其它调用方（如 get_live_sessions）一并阻塞在缓存锁上。
    /// 两个线程并发分析同一文件时可能重复读取，但提交前校验偏移快照，只有一方生效，状态不会错乱。
    pub fn analyze_shared(
        cache: &std::sync::Mutex<TranscriptCache>,
        spec: &dyn TranscriptSpec,
        path: &str,
    ) -> TranscriptInfo {
        use std::io::{Read, Seek, SeekFrom};
        let lock = || cache.lock().unwrap_or_else(|e| e.into_inner());
        // 短临界区 1：确保条目存在，取（已解析偏移, 上次 mtime）快照。
        let (offset, prev_mtime) = lock().snapshot(spec, path);

        // 锁外做全部文件 IO。失败时与 analyze 同语义：返回当前累积结果。
        let Ok(mut f) = std::fs::File::open(path) else {
            return lock().current_info(path);
        };
        let (len, mtime) = match f.metadata() {
            Ok(m) => (m.len(), m.modified().ok()),
            Err(_) => return lock().current_info(path),
        };
        // 截断，或等长但 mtime 变了（同长度重写）→ 从头重读；否则只读快照偏移之后的新增。
        let reset = len < offset || (len == offset && mtime != prev_mtime);
        if !reset && len == offset {
            return lock().touch_mtime(path, mtime); // 无新增
        }
        let base = if reset { 0 } else { offset };
        if f.seek(SeekFrom::Start(base)).is_err() {
            return lock().current_info(path);
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return lock().current_info(path);
        }
        // 短临界区 2：偏移仍与快照一致才提交；其它线程已推进则弃用本次读取、复用其结果。
        lock().commit(spec, path, offset, reset, &buf, mtime)
    }

    /// analyze_shared 临界区 1：确保条目存在（含 LRU 淘汰），返回（偏移, mtime）快照。不做文件 IO。
    fn snapshot(
        &mut self,
        spec: &dyn TranscriptSpec,
        path: &str,
    ) -> (u64, Option<std::time::SystemTime>) {
        self.tick += 1;
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
            parser: spec.new_parser(),
            last_used: tick,
        });
        entry.last_used = tick;
        (entry.offset, entry.mtime)
    }

    /// 当前累积结果；条目不存在（锁外窗口内被 LRU 淘汰）时返回空结果。
    fn current_info(&mut self, path: &str) -> TranscriptInfo {
        self.entries
            .get(path)
            .map(|e| e.parser.to_info())
            .unwrap_or_default()
    }

    /// 无新增时刷新 mtime 并返回累积结果。
    fn touch_mtime(&mut self, path: &str, mtime: Option<std::time::SystemTime>) -> TranscriptInfo {
        match self.entries.get_mut(path) {
            Some(e) => {
                e.mtime = mtime;
                e.parser.to_info()
            }
            None => TranscriptInfo::default(),
        }
    }

    /// analyze_shared 临界区 2：把锁外读到的字节合并进缓存。仅当条目偏移仍等于快照偏移
    /// （期间无其它线程推进）时生效；否则弃用本次读取，直接返回已有结果。
    fn commit(
        &mut self,
        spec: &dyn TranscriptSpec,
        path: &str,
        snap_offset: u64,
        reset: bool,
        buf: &[u8],
        mtime: Option<std::time::SystemTime>,
    ) -> TranscriptInfo {
        self.tick += 1;
        let tick = self.tick;
        // buf 是否从文件头读起（reset 或条目本就是新建的 0 偏移）——只有这种读取才能安全灌入全新条目。
        let from_zero = reset || snap_offset == 0;
        let entry = match self.entries.get_mut(path) {
            Some(e) => e,
            None => {
                // 条目在锁外窗口被 LRU 淘汰：从头读的可重建灌入；增量读的丢弃，下轮重来。
                if !from_zero {
                    return TranscriptInfo::default();
                }
                self.entries.insert(
                    path.to_string(),
                    CacheEntry { offset: 0, mtime: None, parser: spec.new_parser(), last_used: tick },
                );
                self.entries.get_mut(path).expect("刚插入的缓存条目必然存在")
            }
        };
        entry.last_used = tick;
        if entry.offset != snap_offset {
            return entry.parser.to_info();
        }
        if reset {
            entry.offset = 0;
            entry.parser = spec.new_parser();
        }
        if let Some(nl) = buf.iter().rposition(|&b| b == b'\n') {
            entry.offset += (nl + 1) as u64;
            let chunk = String::from_utf8_lossy(&buf[..=nl]);
            for line in chunk.lines() {
                entry.parser.fold_line(line);
            }
        }
        entry.mtime = mtime;
        entry.parser.to_info()
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
        let i1 = cache.analyze(&crate::transcript_spec::ClaudeTranscript, p.to_str().unwrap());
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

        let i2 = cache.analyze(&crate::transcript_spec::ClaudeTranscript, p.to_str().unwrap());
        // 与全量解析结果一致
        let full = analyze_transcript(p.to_str().unwrap());
        assert_eq!(i2.title.as_deref(), Some("标题B")); // custom 覆盖 ai
        assert_eq!(i2.context_tokens, Some(40000));
        assert_eq!(i2.title, full.title);
        assert_eq!(i2.context_tokens, full.context_tokens);

        // 再次调用、无新增 → 结果稳定。
        let i3 = cache.analyze(&crate::transcript_spec::ClaudeTranscript, p.to_str().unwrap());
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
        assert_eq!(cache.analyze(&crate::transcript_spec::ClaudeTranscript, p.to_str().unwrap()).title.as_deref(), Some("AAAA"));

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
        assert_eq!(cache.analyze(&crate::transcript_spec::ClaudeTranscript, p.to_str().unwrap()).title.as_deref(), Some("BBBB"));
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

    #[test]
    fn analyze_shared_matches_analyze_on_append_truncate_and_missing() {
        // 锁外 IO 版必须与 analyze 语义一致：首读、追加增量、截断重读、文件缺失四种路径。
        use std::io::Write;
        use std::sync::Mutex;
        let spec = &crate::transcript_spec::ClaudeTranscript;
        let p = write_tmp("cache_shared", concat!(r#"{"type":"ai-title","aiTitle":"标题A"}"#, "\n"));
        let path = p.to_str().unwrap();
        let cache = Mutex::new(TranscriptCache::new());

        // 首读
        let i1 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i1.title.as_deref(), Some("标题A"));

        // 追加 → 增量读到
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"标题B"}}"#).unwrap();
        drop(f);
        let i2 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i2.title.as_deref(), Some("标题B"));

        // 无新增 → 结果稳定
        let i3 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i3.title.as_deref(), Some("标题B"));

        // 截断成更短内容 → 从头重解析
        std::fs::write(&p, concat!(r#"{"type":"ai-title","aiTitle":"C"}"#, "\n")).unwrap();
        let i4 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i4.title.as_deref(), Some("C"));

        // 文件消失 → 沿用已累积结果（与 analyze 一致）
        std::fs::remove_file(&p).ok();
        let i5 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i5.title.as_deref(), Some("C"));
    }
}
