//! 会话标题解析。实现已统一到 cc-store::title（reporter 写入、cc-app 读取共享），
//! 这里仅重导出供 dispatch 继续用 `crate::transcript::resolve_title`。
pub use cc_store::title::{reconstruct_transcript_path, resolve_title, title_from_transcript};
