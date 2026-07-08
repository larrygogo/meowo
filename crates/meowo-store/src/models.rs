use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Waiting,
    Ended,
    Stale,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Running => "running",
            SessionStatus::Waiting => "waiting",
            SessionStatus::Ended => "ended",
            SessionStatus::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskColumn {
    Todo,
    Doing,
    Done,
}

impl TaskColumn {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskColumn::Todo => "todo",
            TaskColumn::Doing => "doing",
            TaskColumn::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl TodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TodoStatus::Pending => "pending",
            TodoStatus::InProgress => "in_progress",
            TodoStatus::Completed => "completed",
        }
    }
    /// 从 TodoWrite 的 status 字符串映射；无副作用、未知值降级为 Pending，
    /// 故用中缀方法而非 fallible 的 std FromStr。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> TodoStatus {
        match s {
            "in_progress" => TodoStatus::InProgress,
            "completed" => TodoStatus::Completed,
            _ => TodoStatus::Pending,
        }
    }
}

/// 待审批子态:回合中途等用户介入的三种情形。NULL 态在 store 层用 Option 表达,枚举不含 None 变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PendingReview {
    Approval,
    Question,
    Plan,
}

impl PendingReview {
    pub fn as_str(self) -> &'static str {
        match self {
            PendingReview::Approval => "approval",
            PendingReview::Question => "question",
            PendingReview::Plan => "plan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub root_path: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub project_id: i64,
    pub cc_session_id: String,
    pub status: String,
    pub started_at: i64,
    pub last_event_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub session_id: Option<i64>,
    pub title: String,
    pub column: String,
    pub column_locked: bool,
    pub current_activity: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub task_id: i64,
    pub content: String,
    pub status: String,
    pub order_idx: i64,
}

/// 上报器同步 todo 时的输入项。
#[derive(Debug, Clone, PartialEq)]
pub struct TodoInput {
    pub content: String,
    pub status: TodoStatus,
}

/// 默认 agent provider 名，与 sessions.provider 列的 SQL DEFAULT 'claude'
/// （migrations.rs 建表 + store.rs ALTER）必须保持一致。此常量改动时下方测试会变红、
/// 提醒同步 SQL 字面量，但若只改 SQL 而不改此常量则无法被发现（单向绊线）。
pub const DEFAULT_PROVIDER: &str = "claude";

/// agent 提供方（CLI）。与 sessions.provider 列、前端 ProviderConfig key 对齐。
/// 仿 SessionStatus/TodoStatus：as_str + 无副作用 from_str（未知/空降级默认），
/// 作为全项目「provider 名」的单一强类型，取代散落的裸 &str 比较与 unwrap_or("claude")。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKey {
    Claude,
    Kimi,
    Codex,
}

impl ProviderKey {
    /// 全部已知 provider。新增 variant 必在此登记；meowo-reporter 的 enum↔registry
    /// 配对测试据此校验每个 key 都有对应 Agent 实现。
    pub const ALL: &'static [ProviderKey] = &[ProviderKey::Claude, ProviderKey::Kimi, ProviderKey::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKey::Claude => "claude",
            ProviderKey::Kimi => "kimi",
            ProviderKey::Codex => "codex",
        }
    }

    /// 无副作用解析：未知 → 默认（Claude）。仿 TodoStatus::from_str。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> ProviderKey {
        match s {
            "kimi" => ProviderKey::Kimi,
            "codex" => ProviderKey::Codex,
            _ => ProviderKey::Claude,
        }
    }

    /// 唯一归一点：替代散落的 unwrap_or("claude") / != "claude"。None/未知 → 默认。
    pub fn parse(s: Option<&str>) -> ProviderKey {
        match s {
            Some(v) => ProviderKey::from_str(v),
            None => ProviderKey::Claude,
        }
    }

    /// 是否为默认 provider（claude）。DB 把 NULL/缺省视作 claude，故默认 provider 不写库。
    pub fn is_default(self) -> bool {
        matches!(self, ProviderKey::Claude)
    }
}

#[cfg(test)]
mod provider_key_tests {
    use super::*;

    #[test]
    fn as_str_roundtrips_known_keys() {
        assert_eq!(ProviderKey::Claude.as_str(), "claude");
        assert_eq!(ProviderKey::Kimi.as_str(), "kimi");
        assert_eq!(ProviderKey::Codex.as_str(), "codex");
    }

    #[test]
    fn from_str_falls_back_to_claude_on_unknown() {
        assert_eq!(ProviderKey::from_str("kimi"), ProviderKey::Kimi);
        assert_eq!(ProviderKey::from_str("codex"), ProviderKey::Codex);
        assert_eq!(ProviderKey::from_str("claude"), ProviderKey::Claude);
        assert_eq!(ProviderKey::from_str("nonsense"), ProviderKey::Claude);
        assert_eq!(ProviderKey::from_str(""), ProviderKey::Claude);
    }

    #[test]
    fn parse_normalizes_none_and_unknown_to_default() {
        // 唯一归一点：替代散落的 unwrap_or("claude")。
        assert_eq!(ProviderKey::parse(None), ProviderKey::Claude);
        assert_eq!(ProviderKey::parse(Some("kimi")), ProviderKey::Kimi);
        assert_eq!(ProviderKey::parse(Some("zzz")), ProviderKey::Claude);
    }

    #[test]
    fn is_default_only_for_claude() {
        assert!(ProviderKey::Claude.is_default());
        assert!(!ProviderKey::Kimi.is_default());
        assert!(!ProviderKey::Codex.is_default());
    }

    #[test]
    fn default_const_matches_claude_variant_and_schema() {
        // 单向绊线：改 DEFAULT_PROVIDER 常量时此断言变红，提醒同步 migrations.rs 的建表
        // 默认值与 store.rs 的 ALTER 语句。但此测试**不会**检测「只改 SQL 字面量而不改常量」
        // 的情况，需在 migrations.rs / store.rs 的 SQL 行旁记下关联注释自行守护。
        assert_eq!(DEFAULT_PROVIDER, "claude");
        assert_eq!(ProviderKey::Claude.as_str(), DEFAULT_PROVIDER);
    }

    #[test]
    fn all_lists_every_variant_once() {
        assert_eq!(ProviderKey::ALL.len(), 3);
        for v in [ProviderKey::Claude, ProviderKey::Kimi, ProviderKey::Codex] {
            assert!(ProviderKey::ALL.contains(&v));
        }
    }
}
