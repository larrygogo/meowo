//! hook 上报器。agent 的身份、能力与会话解析全部住在 `meowo_agent` 插件注册表——本 crate
//! 只负责「把 hook 事件落库」这件事，不再持有第二张 agent 注册表。

pub mod dispatch;
pub mod hook;
pub mod import;
pub mod proc;
pub mod statusline;
pub mod tabtitle;

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

#[cfg(test)]
mod tests {
    /// 默认 agent 的 id 必须与 DB schema 的 `DEFAULT 'claude'` 字面量一致——否则老会话
    /// （provider 列为 NULL）会被解析成另一个 agent。本 crate 同时依赖 `meowo-agent` 与
    /// `meowo-store`，是唯一能做这个配对断言的地方。
    #[test]
    fn default_agent_id_matches_db_default_provider() {
        assert_eq!(
            meowo_agent::DEFAULT_ID.as_str(),
            meowo_store::DEFAULT_PROVIDER
        );
    }

    /// 未知 provider 串不得被冒名成默认 agent——否则一个本版本尚不认识的 agent，其会话会被按
    /// claude 去 resume（拉起错误的 CLI）、读 transcript（读错文件）。空/缺省才走默认。
    #[test]
    fn unknown_provider_resolves_to_none_not_default() {
        assert_eq!(
            meowo_agent::resolve(None).map(|p| p.id()),
            Some(meowo_agent::DEFAULT_ID)
        );
        assert_eq!(
            meowo_agent::resolve(Some("")).map(|p| p.id()),
            Some(meowo_agent::DEFAULT_ID)
        );
        // 反例得挑一个**永远**不会被注册的串。这里原本写的是 "gemini"——它后来真的成了注册过的
        // agent，于是这条断言差点在无人察觉的情况下失去意义（幸而它当场变红）。
        assert!(meowo_agent::resolve(Some("not-an-agent")).is_none());
    }
}
