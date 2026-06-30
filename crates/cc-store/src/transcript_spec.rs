//! provider 无关的 transcript 抽象。把「定位 transcript + 标题解析 + 增量分析」收成 trait：
//! claude 由 ClaudeTranscript 实现（委托现有 title/analyze，逐字节等价），codex/kimi 暂无实现
//! （Agent::transcript 返回 None，与现状一致——它们的标题/预览/模型走各自别的路径）。
use crate::analyze::TranscriptInfo;
use std::path::PathBuf;

/// 增量解析单元：逐行 fold、按需产出 TranscriptInfo（对应 analyze 的 ParseState）。
/// Send：TranscriptCache 经 Arc<Mutex<>> 在 Tauri 主线程与后台轮询线程间共享。
pub trait TranscriptParser: Send {
    fn fold_line(&mut self, line: &str);
    fn to_info(&self) -> TranscriptInfo;
}

/// 某 provider 的 transcript 规格：定位文件 + 解析标题 + 产出增量解析器。
/// Sync：以 &'static dyn 共享。
pub trait TranscriptSpec: Sync {
    /// 新建一个该 provider 的增量解析器（供 TranscriptCache 在新建/重置条目时调用）。
    fn new_parser(&self) -> Box<dyn TranscriptParser>;
    /// 定位 transcript 文件（hook 路径 → cwd+id 重建 → 全局查找）。
    fn resolve_transcript_path(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<PathBuf>;
    /// 解析会话标题（读不到/无标题返回 None）。
    fn resolve_title(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<String>;
}

/// Claude Code 的 transcript 规格：委托 crate::title / crate::analyze 的现有实现，逐字节等价。
pub struct ClaudeTranscript;

impl TranscriptSpec for ClaudeTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        crate::analyze::claude_new_parser()
    }
    fn resolve_transcript_path(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<PathBuf> {
        crate::title::resolve_transcript_path(transcript_path, cwd, session_id)
    }
    fn resolve_title(&self, transcript_path: Option<&str>, cwd: Option<&str>, session_id: &str) -> Option<String> {
        crate::title::resolve_title(transcript_path, cwd, session_id)
    }
}

/// 全局唯一 claude transcript 规格实例，供 Agent::transcript() 以 &'static 返回。
pub static CLAUDE_TRANSCRIPT: ClaudeTranscript = ClaudeTranscript;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn claude_parser_matches_parsestate_full_scan() {
        // ClaudeParser 逐行 fold 的结果须与 analyze_transcript 全量解析逐字段一致。
        let content = concat!(
            r#"{"type":"ai-title","aiTitle":"标题X"}"#, "\n",
            r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":40000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"hi there"}]}}"#, "\n",
        );
        let mut parser = crate::analyze::claude_new_parser();
        for line in content.lines() {
            parser.fold_line(line);
        }
        let p = std::env::temp_dir().join(format!("cc_ts_{}.jsonl", std::process::id()));
        std::fs::write(&p, content).unwrap();
        let full = crate::analyze::analyze_transcript(p.to_str().unwrap());
        std::fs::remove_file(&p).ok();
        assert_eq!(parser.to_info(), full);
        assert_eq!(parser.to_info().title.as_deref(), Some("标题X"));
        assert_eq!(parser.to_info().context_tokens, Some(40000));
    }

    #[test]
    fn claude_transcript_resolve_title_delegates() {
        // ClaudeTranscript.resolve_title 须与 title::resolve_title 对同一文件得到相同结果。
        let p = std::env::temp_dir().join(format!("cc_ts_title_{}.jsonl", std::process::id()));
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"我的标题"}}"#).unwrap();
        drop(f);
        let path = p.to_str().unwrap();
        let via_spec = CLAUDE_TRANSCRIPT.resolve_title(Some(path), None, "sid");
        let via_fn = crate::title::resolve_title(Some(path), None, "sid");
        std::fs::remove_file(&p).ok();
        assert_eq!(via_spec, via_fn);
        assert_eq!(via_spec.as_deref(), Some("我的标题"));
    }
}
