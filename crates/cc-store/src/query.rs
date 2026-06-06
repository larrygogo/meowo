use crate::error::StoreError;
use crate::models::{Project, Session, Task, Todo};
use crate::store::Store;
use serde::Serialize;

/// 总览里每个项目一行的聚合。
#[derive(Debug, Clone, Serialize)]
pub struct ProjectOverview {
    pub project: Project,
    pub active_sessions: i64,
    pub todo_count: i64,
    pub doing_count: i64,
    pub done_count: i64,
    pub last_activity_at: i64,
}

/// 项目看板里一张任务卡：任务 + 子清单 + 关联会话状态。
#[derive(Debug, Clone, Serialize)]
pub struct TaskCard {
    pub task: Task,
    pub todos: Vec<Todo>,
    pub session_status: Option<String>,
}

/// 当前活跃区的一张会话卡。
#[derive(Debug, Clone, Serialize)]
pub struct LiveSession {
    pub session: Session,
    pub project_name: String,
    pub task_title: String,
    pub current_activity: Option<String>,
    pub column: String,
    pub todo_done: i64,
    pub todo_total: i64,
    pub todos: Vec<Todo>,
    pub pid: Option<i64>,
    pub archived: bool,
    /// 归档时间戳（ms）；未归档为 None。用于「归档超过 N 天自动隐藏」。
    pub archived_at: Option<i64>,
    /// 会话工作目录，cc-app 用它重建 transcript 路径以实时解析 AI 标题。
    pub cwd: Option<String>,
}

impl Store {
    /// 所有项目的总览聚合，按 last_activity_at 倒序。
    pub fn overview(&self) -> Result<Vec<ProjectOverview>, StoreError> {
        let projects = self.list_projects()?;
        let mut out = Vec::with_capacity(projects.len());
        for project in projects {
            let pid = project.id;
            let active_sessions: i64 = self.conn.query_row(
                "SELECT count(*) FROM sessions WHERE project_id = ?1 AND status IN ('running','waiting')",
                [pid],
                |r| r.get(0),
            )?;
            let col_count = |col: &str| -> Result<i64, StoreError> {
                let n: i64 = self.conn.query_row(
                    "SELECT count(*) FROM tasks WHERE project_id = ?1 AND column_name = ?2
                       AND (title <> '(未命名会话)' OR EXISTS (SELECT 1 FROM todos WHERE todos.task_id = tasks.id))",
                    rusqlite::params![pid, col],
                    |r| r.get(0),
                )?;
                Ok(n)
            };
            let todo_count = col_count("todo")?;
            let doing_count = col_count("doing")?;
            let done_count = col_count("done")?;
            let last_activity_at: i64 = self.conn.query_row(
                "SELECT COALESCE(MAX(last_event_at), ?2) FROM sessions WHERE project_id = ?1",
                rusqlite::params![pid, project.updated_at],
                |r| r.get(0),
            )?;
            out.push(ProjectOverview {
                project,
                active_sessions,
                todo_count,
                doing_count,
                done_count,
                last_activity_at,
            });
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.last_activity_at));
        Ok(out)
    }

    /// 某项目的所有任务卡，按 updated_at 倒序。
    pub fn project_tasks(&self, project_id: i64) -> Result<Vec<TaskCard>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, session_id, title, column_name, column_locked, current_activity, created_at, updated_at
             FROM tasks WHERE project_id = ?1
               AND (title <> '(未命名会话)' OR EXISTS (SELECT 1 FROM todos WHERE todos.task_id = tasks.id))
             ORDER BY updated_at DESC, id DESC",
        )?;
        let tasks = stmt
            .query_map([project_id], |r| {
                Ok(Task {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    session_id: r.get(2)?,
                    title: r.get(3)?,
                    column: r.get(4)?,
                    column_locked: r.get::<_, i64>(5)? != 0,
                    current_activity: r.get(6)?,
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<Task>, _>>()?;

        let mut out = Vec::with_capacity(tasks.len());
        for task in tasks {
            let todos = self.list_todos(task.id)?;
            let session_status = match task.session_id {
                Some(sid) => self
                    .conn
                    .query_row("SELECT status FROM sessions WHERE id = ?1", [sid], |r| r.get(0))
                    .ok(),
                None => None,
            };
            out.push(TaskCard { task, todos, session_status });
        }
        Ok(out)
    }

    /// 活跃区：所有会话（含已结束），附项目名、任务标题、进度。
    /// 按 last_event_at 倒序（最近活跃在前），最多返回 100 条（cc-app 会再过滤截断）。
    pub fn live_sessions(&self) -> Result<Vec<LiveSession>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.project_id, s.cc_session_id, s.status, s.started_at, s.last_event_at, s.ended_at,
                    p.name, t.id, t.title, t.current_activity, t.column_name, s.pid, s.archived, s.cwd, s.archived_at
             FROM sessions s
             JOIN projects p ON p.id = s.project_id
             LEFT JOIN tasks t ON t.session_id = s.id
             ORDER BY s.last_event_at DESC
             LIMIT 100",
        )?;
        let rows = stmt
            .query_map([], |r| {
                let session = Session {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    cc_session_id: r.get(2)?,
                    status: r.get(3)?,
                    started_at: r.get(4)?,
                    last_event_at: r.get(5)?,
                    ended_at: r.get(6)?,
                };
                let project_name: String = r.get(7)?;
                let task_id: Option<i64> = r.get(8)?;
                let task_title: Option<String> = r.get(9)?;
                let current_activity: Option<String> = r.get(10)?;
                let column: Option<String> = r.get(11)?;
                let pid: Option<i64> = r.get(12)?;
                let archived: i64 = r.get(13)?;
                let cwd: Option<String> = r.get(14)?;
                let archived_at: Option<i64> = r.get(15)?;
                Ok((session, project_name, task_id, task_title, current_activity, column, pid, archived, cwd, archived_at))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut out = Vec::with_capacity(rows.len());
        for (session, project_name, task_id, task_title, current_activity, column, pid, archived, cwd, archived_at) in rows {
            let todos = match task_id {
                Some(tid) => self.list_todos(tid)?,
                None => Vec::new(),
            };
            let todo_total = todos.len() as i64;
            let todo_done = todos.iter().filter(|t| t.status == "completed").count() as i64;
            out.push(LiveSession {
                session,
                project_name,
                task_title: task_title.unwrap_or_default(),
                current_activity,
                column: column.unwrap_or_else(|| "todo".to_string()),
                todo_done,
                todo_total,
                todos,
                pid,
                archived: archived != 0,
                archived_at,
                cwd,
            });
        }
        Ok(out)
    }

    /// 兜底收尾「没有 pid、且超过 idle_ms 无任何事件」的 live 会话。返回受影响数。
    ///
    /// 带 pid 的会话由进程存活校验处理（进程在就保留，哪怕空闲很久——那是 claude 在等用户输入，
    /// 仍是连接中，绝不能因空闲误杀）。但 pid 为空（reporter 没抓到 owner pid）的会话无法做存活校验，
    /// 若终端被直接关掉（SessionEnd 丢失），就会永远卡在 live。对这类会话退化为「空闲超时」清理：
    /// 真正活跃的会话每个事件都会刷新 last_event_at，到不了这个阈值。
    pub fn end_orphaned_idle(&self, idle_ms: i64, now_ms: i64) -> Result<usize, StoreError> {
        let n = self.conn.execute(
            "UPDATE sessions SET status='ended', ended_at=?1
             WHERE pid IS NULL AND status IN ('running','waiting','stale')
               AND (?1 - last_event_at) > ?2",
            rusqlite::params![now_ms, idle_ms],
        )?;
        Ok(n)
    }
}

