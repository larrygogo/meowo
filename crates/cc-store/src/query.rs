use crate::error::StoreError;
use crate::models::{Project, Task, Todo};
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
                    "SELECT count(*) FROM tasks WHERE project_id = ?1 AND column_name = ?2",
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
}

impl Store {
    /// 某项目的所有任务卡，按 updated_at 倒序。
    pub fn project_tasks(&self, project_id: i64) -> Result<Vec<TaskCard>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, session_id, title, column_name, column_locked, current_activity, created_at, updated_at
             FROM tasks WHERE project_id = ?1 ORDER BY updated_at DESC, id DESC",
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
}
