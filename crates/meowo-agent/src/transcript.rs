//! Transcript 抽象（provider 无关）：数据类型 + 解析器 trait + 增量缓存。
//!
//! 「怎么定位 transcript、怎么解析它」是 agent 的能力，故 trait 住在插件层而非 DB 层——
//! 此前这套代码寄生在 `meowo-store` 里，让「读一个 JSONL 文件」平白拖上了 rusqlite 依赖，
//! 也让 claude 专属的 `~/.claude/projects` 路径布局伪装成了通用的 store API。
//!
//! 具体格式由各 agent 在 `plugins/<id>/transcript.rs` 实现（目前只有 claude；codex/kimi 的
//! 标题走首条 prompt、预览/模型走 Stop hook，不读 transcript）。

use serde::Serialize;
use std::path::PathBuf;

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

/// 增量解析单元：逐行 fold、按需产出 TranscriptInfo。
/// Send：TranscriptCache 经 Arc<Mutex<>> 在 Tauri 主线程与后台轮询线程间共享。
pub trait TranscriptParser: Send {
    fn fold_line(&mut self, line: &str);
    fn to_info(&self) -> TranscriptInfo;
}

/// 某 agent 的 transcript 规格：定位文件 + 解析标题 + 产出增量解析器。
/// Sync：以 &'static dyn 共享。
pub trait TranscriptSpec: Sync {
    /// 新建一个该 agent 的增量解析器（供 TranscriptCache 在新建/重置条目时调用）。
    fn new_parser(&self) -> Box<dyn TranscriptParser>;
    /// 定位 transcript 文件（hook 路径 → cwd+id 重建 → 全局查找）。
    fn resolve_transcript_path(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<PathBuf>;
    /// 解析会话标题（读不到/无标题返回 None）。
    fn resolve_title(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<String>;

    /// 解析会话的真实工作目录——resume 必须在正确的项目目录下运行才找得到会话。
    ///
    /// 默认实现原样返回 DB 记录的 cwd。能从 transcript 内容读出权威 cwd 的 agent（claude）覆写它，
    /// 以纠正失真的 DB 记录（会话早于 hook 接线、SessionStart 丢失、项目目录事后被移动）。
    ///
    /// 此前这是 `meowo_store::title::resolve_cwd`：一个读 `~/.claude/projects` 的 claude 专属函数，
    /// 却被 app 当通用 API 对所有 agent 调用——非 claude 会话靠「全局找不到就回退 DB cwd」的巧合
    /// 拿到正确结果。现在这个回退就是默认实现本身。
    fn resolve_cwd(&self, cwd: Option<&str>, _session_id: &str) -> Option<String> {
        default_resolve_cwd(cwd)
    }
}

/// 无 transcript 规格的 agent（codex/kimi）以及 `TranscriptSpec::resolve_cwd` 默认实现共用：
/// 直接采信 DB 记录的 cwd，空白视作没有。
pub fn default_resolve_cwd(cwd: Option<&str>) -> Option<String> {
    cwd.filter(|c| !c.trim().is_empty()).map(str::to_string)
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

/// read_transcript_delta 的结果：analyze 与 analyze_shared 共用的文件 IO 段。
enum DeltaOutcome {
    /// 打开/metadata/seek/读取失败：沿用已累积状态（不要用 len=0 当真实长度误判截断）。
    Unreadable,
    /// 无新增字节：仅需刷新 mtime。
    NoChange(Option<std::time::SystemTime>),
    /// 读到了新增（或需从头重读）的字节。
    Data { reset: bool, buf: Vec<u8>, mtime: Option<std::time::SystemTime> },
}

/// 从 offset/prev_mtime 快照出发读取 transcript 的增量字节。纯文件 IO、不触碰缓存，
/// 供 analyze（持锁调用）与 analyze_shared（锁外调用）共用。
/// 失效检测：len < offset（截断）或 len == offset 但 mtime 变了（等长重写）→ reset 从头读。
fn read_transcript_delta(
    path: &str,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
) -> DeltaOutcome {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return DeltaOutcome::Unreadable;
    };
    let (len, mtime) = match f.metadata() {
        Ok(m) => (m.len(), m.modified().ok()),
        Err(_) => return DeltaOutcome::Unreadable,
    };
    let reset = len < offset || (len == offset && mtime != prev_mtime);
    if !reset && len == offset {
        return DeltaOutcome::NoChange(mtime);
    }
    let base = if reset { 0 } else { offset };
    if f.seek(SeekFrom::Start(base)).is_err() {
        return DeltaOutcome::Unreadable;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return DeltaOutcome::Unreadable;
    }
    DeltaOutcome::Data { reset, buf, mtime }
}

impl TranscriptCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 增量解析 path：只处理上次偏移之后新追加的「完整行」（末尾未结束的半行留到下次）。
    /// 失效检测用 len + mtime 双重校验：len < 偏移（截断）或 len == 偏移但 mtime 变了
    /// （等长重写）→ 从头重解析。打开/读失败 → 返回当前累积结果。
    /// `spec` 决定新建/重置条目时用哪种 agent 的解析器。
    /// 与 analyze_shared 共用 snapshot/read_transcript_delta/commit 三段（单一事实源），
    /// 差别仅在本方法独占 &mut self、无并发窗口。
    pub fn analyze(&mut self, spec: &dyn TranscriptSpec, path: &str) -> TranscriptInfo {
        let (offset, prev_mtime) = self.snapshot(spec, path);
        match read_transcript_delta(path, offset, prev_mtime) {
            DeltaOutcome::Unreadable => self.current_info(path),
            DeltaOutcome::NoChange(mtime) => self.touch_mtime(path, mtime),
            DeltaOutcome::Data { reset, buf, mtime } => {
                self.commit(spec, path, offset, reset, &buf, mtime)
            }
        }
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
        let lock = || cache.lock().unwrap_or_else(|e| e.into_inner());
        // 短临界区 1：确保条目存在，取（已解析偏移, 上次 mtime）快照。
        let (offset, prev_mtime) = lock().snapshot(spec, path);
        // 锁外做全部文件 IO。失败时与 analyze 同语义：返回当前累积结果。
        match read_transcript_delta(path, offset, prev_mtime) {
            DeltaOutcome::Unreadable => lock().current_info(path),
            DeltaOutcome::NoChange(mtime) => lock().touch_mtime(path, mtime),
            // 短临界区 2：偏移仍与快照一致才提交；其它线程已推进则弃用本次读取、复用其结果。
            DeltaOutcome::Data { reset, buf, mtime } => {
                lock().commit(spec, path, offset, reset, &buf, mtime)
            }
        }
    }

    /// analyze / analyze_shared 临界区 1：确保条目存在（含 LRU 淘汰），返回（偏移, mtime）快照。不做文件 IO。
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

    /// analyze / analyze_shared 临界区 2：把读到的字节合并进缓存。仅当条目偏移仍等于快照偏移
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
    // 缓存的增量/失效语义要用一个真实解析器才测得出来（断言标题随追加变化），故借 claude 的 spec。
    use crate::plugins::claude::transcript::{analyze_transcript, ClaudeTranscript};

    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("meowo_cache_{}_{}.jsonl", std::process::id(), name));
        std::fs::write(&p, content).unwrap();
        p
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
        let i1 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
        assert_eq!(i1.title.as_deref(), Some("标题A"));
        assert_eq!(i1.context_tokens, Some(1000));

        // 追加新一轮（带更大 usage + 自定义标题），增量解析应读到。
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"标题B"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","uuid":"u2","message":{{"role":"assistant","usage":{{"input_tokens":40000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}},"content":[{{"type":"tool_use","name":"Bash","input":{{}}}}]}}}}"#
        )
        .unwrap();
        drop(f);

        let i2 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
        // 与全量解析结果一致
        let full = analyze_transcript(p.to_str().unwrap());
        assert_eq!(i2.title.as_deref(), Some("标题B")); // custom 覆盖 ai
        assert_eq!(i2.context_tokens, Some(40000));
        assert_eq!(i2.title, full.title);
        assert_eq!(i2.context_tokens, full.context_tokens);

        // 再次调用、无新增 → 结果稳定。
        let i3 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
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
        assert_eq!(cache.analyze(&ClaudeTranscript, p.to_str().unwrap()).title.as_deref(), Some("AAAA"));

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
        assert_eq!(cache.analyze(&ClaudeTranscript, p.to_str().unwrap()).title.as_deref(), Some("BBBB"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn analyze_shared_matches_analyze_on_append_truncate_and_missing() {
        // 锁外 IO 版必须与 analyze 语义一致：首读、追加增量、截断重读、文件缺失四种路径。
        use std::io::Write;
        use std::sync::Mutex;
        let spec = &ClaudeTranscript;
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
