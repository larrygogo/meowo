//! 会话标题相关函数的重导出。实现已随 transcript 抽象迁入 `meowo_agent::plugins::claude`，
//! dispatch 经 `TranscriptSpec::resolve_title` 解析标题。本模块仅为测试保留旧路径。
pub use meowo_agent::plugins::claude::transcript::{reconstruct_transcript_path, title_from_transcript};
