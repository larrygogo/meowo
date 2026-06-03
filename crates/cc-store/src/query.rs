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

    /// 活跃区：status 为 running/waiting/stale 的会话，附项目名、任务标题、进度。
    /// 按 last_event_at 倒序（最近活跃在前）。
    pub fn live_sessions(&self) -> Result<Vec<LiveSession>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.project_id, s.cc_session_id, s.status, s.started_at, s.last_event_at, s.ended_at,
                    p.name, t.id, t.title, t.current_activity, t.column_name
             FROM sessions s
             JOIN projects p ON p.id = s.project_id
             LEFT JOIN tasks t ON t.session_id = s.id
             WHERE s.status IN ('running','waiting','stale')
             ORDER BY s.last_event_at DESC",
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
                Ok((session, project_name, task_id, task_title, current_activity, column))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut out = Vec::with_capacity(rows.len());
        for (session, project_name, task_id, task_title, current_activity, column) in rows {
            let (todo_done, todo_total) = match task_id {
                Some(tid) => {
                    let total: i64 = self.conn.query_row(
                        "SELECT count(*) FROM todos WHERE task_id = ?1",
                        [tid],
                        |r| r.get(0),
                    )?;
                    let done: i64 = self.conn.query_row(
                        "SELECT count(*) FROM todos WHERE task_id = ?1 AND status = 'completed'",
                        [tid],
                        |r| r.get(0),
                    )?;
                    (done, total)
                }
                None => (0, 0),
            };
            out.push(LiveSession {
                session,
                project_name,
                task_title: task_title.unwrap_or_default(),
                current_activity,
                column: column.unwrap_or_else(|| "todo".to_string()),
                todo_done,
                todo_total,
            });
        }
        Ok(out)
    }
}

