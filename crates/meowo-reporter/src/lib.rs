pub mod agent;
pub mod codex;
pub mod dispatch;
pub mod hook;
pub mod import;
pub mod kimi;
pub mod proc;
pub mod statusline;
pub mod tabtitle;
pub mod transcript;

use std::path::PathBuf;

/// 库路径：环境变量 MEOWO_DB 优先，否则 ~/.meowo/board.db。
pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("MEOWO_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".meowo").join("board.db")
}
