use crate::error::StoreError;
use crate::models::{Project, Session, Task, Todo};
use crate::store::Store;
use serde::Serialize;
use std::collections::HashMap;

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
    /// 上下文已用百分比（来自 Claude Code statusline，准确）；无 statusline 数据为 None。
    pub context_pct: Option<i64>,
    /// 上下文窗口大小（200000 或 1000000）；无 statusline 数据为 None。
    pub context_window: Option<i64>,
    /// 用户给会话挂的便签（手写备忘）；无便签为 None。
    pub note: Option<String>,
}

impl Store {
    /// 批量取多个 task 的 todos，按 task_id 分组——替代逐 task 调 `list_todos` 的 N+1。
    /// 按固定块大小分批拼 `IN (...)`：单条 `IN` 的占位符数不能超过 SQLite 绑定参数上限
    /// （旧版默认 999），单项目任务很多时一次性塞进去会 `too many SQL variables`。
    /// task_id 唯一（一个 task 只属一个会话），不跨块，故每个分组内仍按 order_idx 有序。
    fn todos_by_task(&self, task_ids: &[i64]) -> Result<HashMap<i64, Vec<Todo>>, StoreError> {
        const CHUNK: usize = 900;
        let mut map: HashMap<i64, Vec<Todo>> = HashMap::new();
        for chunk in task_ids.chunks(CHUNK) {
            let placeholders = vec!["?"; chunk.len()].join(",");
            let sql = format!(
                "SELECT id, task_id, content, status, order_idx FROM todos
                 WHERE task_id IN ({placeholders}) ORDER BY task_id, order_idx"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk), |r| {
                Ok(Todo {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    content: r.get(2)?,
                    status: r.get(3)?,
                    order_idx: r.get(4)?,
                })
            })?;
            for row in rows {
                let todo = row?;
                map.entry(todo.task_id).or_default().push(todo);
            }
        }
        Ok(map)
    }

    /// 所有项目的总览聚合，按 last_activity_at 倒序。
    pub fn overview(&self) -> Result<Vec<ProjectOverview>, StoreError> {
        let projects = self.list_projects()?;
        if projects.is_empty() {
            return Ok(Vec::new());
        }
        // 活跃会话数：一次按项目分组取回（替代逐项目 count）。
        let mut active: HashMap<i64, i64> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT project_id, count(*) FROM sessions
                 WHERE status IN ('running','waiting') GROUP BY project_id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (pid, n) = row?;
                active.insert(pid, n);
            }
        }
        // 各列任务数（排除未命名空卡）：一次按 (项目, 列) 分组取回。
        let mut cols: HashMap<(i64, String), i64> = HashMap::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT project_id, column_name, count(*) FROM tasks
                 WHERE (title <> '(未命名会话)' OR EXISTS (SELECT 1 FROM todos WHERE todos.task_id = tasks.id))
                 GROUP BY project_id, column_name",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?;
            for row in rows {
                let (pid, col, n) = row?;
                cols.insert((pid, col), n);
            }
        }
        // 最近活动时间：一次按项目取 MAX(last_event_at)；无会话的项目回退 project.updated_at。
        let mut last_evt: HashMap<i64, i64> = HashMap::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT project_id, MAX(last_event_at) FROM sessions GROUP BY project_id")?;
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
            })?;
            for row in rows {
                let (pid, m) = row?;
                if let Some(v) = m {
                    last_evt.insert(pid, v);
                }
            }
        }
        let col_count = |pid: i64, col: &str| cols.get(&(pid, col.to_string())).copied().unwrap_or(0);
        let mut out = Vec::with_capacity(projects.len());
        for project in projects {
            let pid = project.id;
            let last_activity_at = last_evt.get(&pid).copied().unwrap_or(project.updated_at);
            out.push(ProjectOverview {
                active_sessions: active.get(&pid).copied().unwrap_or(0),
                todo_count: col_count(pid, "todo"),
                doing_count: col_count(pid, "doing"),
                done_count: col_count(pid, "done"),
                last_activity_at,
                project,
            });
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.last_activity_at));
        Ok(out)
    }

    /// 某项目的所有任务卡，按 updated_at 倒序。
    pub fn project_tasks(&self, project_id: i64) -> Result<Vec<TaskCard>, StoreError> {
        // session_status 用 LEFT JOIN 一次取回（替代逐任务 query_row）。
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.project_id, t.session_id, t.title, t.column_name, t.column_locked,
                    t.current_activity, t.created_at, t.updated_at, s.status
             FROM tasks t
             LEFT JOIN sessions s ON s.id = t.session_id
             WHERE t.project_id = ?1
               AND (t.title <> '(未命名会话)' OR EXISTS (SELECT 1 FROM todos WHERE todos.task_id = t.id))
             ORDER BY t.updated_at DESC, t.id DESC",
        )?;
        let rows = stmt
            .query_map([project_id], |r| {
                let task = Task {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    session_id: r.get(2)?,
                    title: r.get(3)?,
                    column: r.get(4)?,
                    column_locked: r.get::<_, i64>(5)? != 0,
                    current_activity: r.get(6)?,
                    created_at: r.get(7)?,
                    updated_at: r.get(8)?,
                };
                let session_status: Option<String> = r.get(9)?;
                Ok((task, session_status))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // todos 批量取回，按 task 分组（替代逐任务 list_todos 的 N+1）。
        let task_ids: Vec<i64> = rows.iter().map(|(t, _)| t.id).collect();
        let mut todos_map = self.todos_by_task(&task_ids)?;
        let mut out = Vec::with_capacity(rows.len());
        for (task, session_status) in rows {
            let todos = todos_map.remove(&task.id).unwrap_or_default();
            out.push(TaskCard { task, todos, session_status });
        }
        Ok(out)
    }

    /// 活跃区：所有会话（含已结束），附项目名、任务标题、进度。
    /// 按 last_event_at 倒序（最近活跃在前），最多返回 100 条（cc-app 会再过滤截断）。
    pub fn live_sessions(&self) -> Result<Vec<LiveSession>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.project_id, s.cc_session_id, s.status, s.started_at, s.last_event_at, s.ended_at,
                    p.name, t.id, t.title, t.current_activity, t.column_name, s.pid, s.archived, s.cwd, s.archived_at,
                    sc.used_pct, sc.window_size, sn.note
             FROM sessions s
             JOIN projects p ON p.id = s.project_id
             LEFT JOIN tasks t ON t.session_id = s.id
             LEFT JOIN session_context sc ON sc.cc_session_id = s.cc_session_id
             LEFT JOIN session_notes sn ON sn.cc_session_id = s.cc_session_id
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
                let context_pct: Option<i64> = r.get(16)?;
                let context_window: Option<i64> = r.get(17)?;
                let note: Option<String> = r.get(18)?;
                Ok((session, project_name, task_id, task_title, current_activity, column, pid, archived, cwd, archived_at, context_pct, context_window, note))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        // todos 批量取回（替代逐会话 list_todos 的 N+1）。
        let task_ids: Vec<i64> = rows.iter().filter_map(|r| r.2).collect();
        let mut todos_map = self.todos_by_task(&task_ids)?;
        let mut out = Vec::with_capacity(rows.len());
        for (session, project_name, task_id, task_title, current_activity, column, pid, archived, cwd, archived_at, context_pct, context_window, note) in rows {
            let todos = task_id
                .and_then(|tid| todos_map.remove(&tid))
                .unwrap_or_default();
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
                context_pct,
                context_window,
                note,
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

