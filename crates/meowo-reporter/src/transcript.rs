//! 会话标题相关函数的重导出。实现在 meowo-store::title，dispatch 现经 TranscriptSpec::resolve_title
//! 解析标题。本模块仅重导出 reconstruct_transcript_path 与 title_from_transcript 供测试使用。
pub use meowo_store::title::{reconstruct_transcript_path, title_from_transcript};
