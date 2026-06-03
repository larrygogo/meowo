pub mod dispatch;
pub mod hook;

use std::path::PathBuf;

/// 库路径：环境变量 CC_KANBAN_DB 优先，否则 ~/.cc-kanban/board.db。
pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("CC_KANBAN_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cc-kanban").join("board.db")
}
