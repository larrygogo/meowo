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
