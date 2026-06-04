use crate::error::StoreError;
use crate::migrations::SCHEMA;
use crate::models::{Project, Session, SessionStatus, Task, TaskColumn, Todo, TodoInput, TodoStatus};
use rusqlite::Connection;
use std::path::Path;

pub struct Store {
    pub(crate) conn: Connection,
}

impl Store {
    /// 打开（或新建）数据库，开启 WAL，执行建表。
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Store, StoreError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 3000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn);
        Ok(Store { conn })
    }

    /// 仅用于测试：内存库。
    pub fn open_in_memory() -> Result<Store, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn);
        Ok(Store { conn })
    }

    /// 幂等迁移：给已存在的库补列。重复执行安全（列已存在的错误忽略）。
    fn migrate(conn: &rusqlite::Connection) {
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN pid INTEGER", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN cwd TEXT", []);
        let _ = conn.execute("ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0", []);
    }

    /// 测试辅助：统计用户表数量。
    pub fn raw_table_count(&self) -> Result<i64, StoreError> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    // == Task 4: upsert_project_by_root + list_projects ==

    /// 按 root_path upsert 项目，返回 project id。已存在则更新 updated_at。
    pub fn upsert_project_by_root(
        &self,
        root_path: &str,
        name: &str,
        now_ms: i64,
    ) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO projects (root_path, name, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(root_path) DO UPDATE SET updated_at = ?3, name = ?2",
            rusqlite::params![root_path, name, now_ms],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM projects WHERE root_path = ?1",
            [root_path],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, root_path, name, created_at, updated_at FROM projects ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                root_path: r.get(1)?,
                name: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // == Task 5: start_session ==

    /// 开始一个会话；若 cc_session_id 已存在则幂等返回既有 (session_id, task_id)。
    /// 新会话会同时建一张占位任务卡。
    pub fn start_session(
        &self,
        project_id: i64,
        cc_session_id: &str,
        now_ms: i64,
    ) -> Result<(i64, i64), StoreError> {
        // 会话幂等：已存在则复活为 running（resume/--continue 场景），清掉 ended_at。
        self.conn.execute(
            "INSERT INTO sessions (project_id, cc_session_id, status, started_at, last_event_at)
             VALUES (?1, ?2, 'running', ?3, ?3)
             ON CONFLICT(cc_session_id) DO UPDATE SET
                 status = 'running',
                 last_event_at = excluded.last_event_at,
                 ended_at = NULL",
            rusqlite::params![project_id, cc_session_id, now_ms],
        )?;
        let sid = self
            .find_session_id(cc_session_id)?
            .ok_or(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;

        // 占位任务幂等：靠 tasks(session_id) 唯一索引 + INSERT OR IGNORE 防并发重复建卡。
        self.conn.execute(
            "INSERT OR IGNORE INTO tasks
                (project_id, session_id, title, column_name, column_locked, created_at, updated_at)
             VALUES (?1, ?2, '(未命名会话)', 'todo', 0, ?3, ?3)",
            rusqlite::params![project_id, sid, now_ms],
        )?;
        let tid = self.task_id_of_session(sid)?;
        Ok((sid, tid))
    }

    pub fn find_session_id_pub(&self, cc_session_id: &str) -> Result<Option<i64>, StoreError> {
        self.find_session_id(cc_session_id)
    }

    pub fn task_id_of_session_pub(&self, session_id: i64) -> Result<i64, StoreError> {
        self.task_id_of_session(session_id)
    }

    pub fn set_current_activity(
        &self,
        session_id: i64,
        activity: &str,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let tid = self.task_id_of_session(session_id)?;
        self.conn.execute(
            "UPDATE tasks SET current_activity = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![activity, now_ms, tid],
        )?;
        self.touch_session(session_id, now_ms)?;
        Ok(())
    }

    /// 直接设置会话任务标题（来自 CC ai-title/custom-title），覆盖占位/旧标题。空标题忽略。
    pub fn set_session_title(&self, session_id: i64, title: &str, now_ms: i64) -> Result<(), StoreError> {
        let t = truncate_chars(title.trim(), 80);
        if t.is_empty() {
            return Ok(());
        }
        let tid = self.task_id_of_session(session_id)?;
        self.conn.execute(
            "UPDATE tasks SET title = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![t, now_ms, tid],
        )?;
        Ok(())
    }

    pub(crate) fn find_session_id(&self, cc_session_id: &str) -> Result<Option<i64>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM sessions WHERE cc_session_id = ?1")?;
        let mut rows = stmt.query([cc_session_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn task_id_of_session(&self, session_id: i64) -> Result<i64, StoreError> {
        let id: i64 = self.conn.query_row(
            "SELECT id FROM tasks WHERE session_id = ?1 ORDER BY id LIMIT 1",
            [session_id],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn get_task(&self, task_id: i64) -> Result<Task, StoreError> {
        let task = self.conn.query_row(
            "SELECT id, project_id, session_id, title, column_name, column_locked, current_activity, created_at, updated_at
             FROM tasks WHERE id = ?1",
            [task_id],
            |r| {
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
            },
        )?;
        Ok(task)
    }

    // == Task 6: on_user_prompt + touch_session ==

    /// 收到用户 prompt：占位标题则替换为截断后的 prompt；当前动作总是更新为该 prompt。
    pub fn on_user_prompt(
        &self,
        session_id: i64,
        prompt: &str,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let tid = self.task_id_of_session(session_id)?;
        let cleaned = truncate_chars(&sanitize_prompt(prompt), 60);
        if !cleaned.is_empty() {
            let title: String = self.conn.query_row(
                "SELECT title FROM tasks WHERE id = ?1",
                [tid],
                |r| r.get(0),
            )?;
            if title == "(未命名会话)" {
                self.conn.execute(
                    "UPDATE tasks SET title = ?1, current_activity = ?2, updated_at = ?3 WHERE id = ?4",
                    rusqlite::params![cleaned, cleaned, now_ms, tid],
                )?;
            } else {
                self.conn.execute(
                    "UPDATE tasks SET current_activity = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![cleaned, now_ms, tid],
                )?;
            }
        }
        self.touch_session(session_id, now_ms)?;
        Ok(())
    }

    /// 更新会话 last_event_at；若处于 waiting/stale 恢复为 running。
    pub fn touch_session(&self, session_id: i64, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions
             SET last_event_at = ?1,
                 status = CASE WHEN status IN ('waiting','stale') THEN 'running' ELSE status END
             WHERE id = ?2",
            rusqlite::params![now_ms, session_id],
        )?;
        Ok(())
    }

    // == Task 7: sync_todos + set_task_column + list_todos ==

    /// 用新列表整体替换某会话任务的 todos；未锁定时按 todo 推导列。
    pub fn sync_todos(
        &self,
        session_id: i64,
        todos: &[TodoInput],
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let tid = self.task_id_of_session(session_id)?;
        let locked: bool = self.conn.query_row(
            "SELECT column_locked FROM tasks WHERE id = ?1",
            [tid],
            |r| Ok(r.get::<_, i64>(0)? != 0),
        )?;

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM todos WHERE task_id = ?1", [tid])?;
        for (i, t) in todos.iter().enumerate() {
            tx.execute(
                "INSERT INTO todos (task_id, content, status, order_idx) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tid, t.content, t.status.as_str(), i as i64],
            )?;
        }
        if !locked {
            let col = derive_column(todos);
            tx.execute(
                "UPDATE tasks SET column_name = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![col.as_str(), now_ms, tid],
            )?;
        } else {
            tx.execute(
                "UPDATE tasks SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now_ms, tid],
            )?;
        }
        // touch_session 等价逻辑（事务内）：刷新 last_event_at，waiting/stale 复活为 running
        tx.execute(
            "UPDATE sessions
             SET last_event_at = ?1,
                 status = CASE WHEN status IN ('waiting','stale') THEN 'running' ELSE status END
             WHERE id = ?2",
            rusqlite::params![now_ms, session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_todos(&self, task_id: i64) -> Result<Vec<Todo>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, content, status, order_idx FROM todos WHERE task_id = ?1 ORDER BY order_idx",
        )?;
        let rows = stmt.query_map([task_id], |r| {
            Ok(Todo {
                id: r.get(0)?,
                task_id: r.get(1)?,
                content: r.get(2)?,
                status: r.get(3)?,
                order_idx: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 设置任务列；locked=true 表示手动覆盖，之后自动推导不再生效。
    pub fn set_task_column(
        &self,
        task_id: i64,
        column: TaskColumn,
        locked: bool,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE tasks SET column_name = ?1, column_locked = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![column.as_str(), locked as i64, now_ms, task_id],
        )?;
        Ok(())
    }

    // == Task 8: 会话状态变更与 stale 标记 ==

    /// 手动设置会话状态（如 waiting / stale），同时更新 last_event_at。
    pub fn set_session_status(
        &self,
        session_id: i64,
        status: SessionStatus,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET status = ?1, last_event_at = ?2 WHERE id = ?3",
            rusqlite::params![status.as_str(), now_ms, session_id],
        )?;
        Ok(())
    }

    /// 结束会话：状态设为 ended，记录 ended_at。
    pub fn end_session(&self, session_id: i64, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET status = 'ended', ended_at = ?1, last_event_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, session_id],
        )?;
        Ok(())
    }

    /// 导入一条历史会话：以 ended 状态写入，started_at=ended_at=last_event_at=mtime。
    /// 用 ON CONFLICT(cc_session_id) DO NOTHING 保证绝不覆盖已存在的真实会话。
    /// 返回 true 表示新插入；false 表示 cc_session_id 已存在被跳过。
    pub fn import_session(
        &self,
        cc_session_id: &str,
        project_id: i64,
        title: &str,
        cwd: Option<&str>,
        last_event_at: i64,
    ) -> Result<bool, StoreError> {
        let n = self.conn.execute(
            "INSERT INTO sessions
                (project_id, cc_session_id, status, started_at, last_event_at, ended_at, cwd)
             VALUES (?1, ?2, 'ended', ?3, ?3, ?3, ?4)
             ON CONFLICT(cc_session_id) DO NOTHING",
            rusqlite::params![project_id, cc_session_id, last_event_at, cwd],
        )?;
        if n == 0 {
            return Ok(false); // 已存在，绝不覆盖
        }
        let sid = self
            .find_session_id(cc_session_id)?
            .ok_or(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
        let mut t = truncate_chars(title.trim(), 80);
        if t.is_empty() {
            t = "(未命名会话)".to_string();
        }
        // 历史已结束会话的任务卡固定放 done 列，不导入 todo。
        self.conn.execute(
            "INSERT OR IGNORE INTO tasks
                (project_id, session_id, title, column_name, column_locked, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'done', 0, ?4, ?4)",
            rusqlite::params![project_id, sid, t, last_event_at],
        )?;
        Ok(true)
    }

    pub fn get_session(&self, session_id: i64) -> Result<Session, StoreError> {
        let s = self.conn.query_row(
            "SELECT id, project_id, cc_session_id, status, started_at, last_event_at, ended_at
             FROM sessions WHERE id = ?1",
            [session_id],
            |r| {
                Ok(Session {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    cc_session_id: r.get(2)?,
                    status: r.get(3)?,
                    started_at: r.get(4)?,
                    last_event_at: r.get(5)?,
                    ended_at: r.get(6)?,
                })
            },
        )?;
        Ok(s)
    }

    /// 记录会话启动时的 cwd（用于重建 transcript 路径取标题）。
    pub fn set_session_cwd(&self, session_id: i64, cwd: &str, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET cwd = ?1, last_event_at = ?2 WHERE id = ?3",
            rusqlite::params![cwd, now_ms, session_id],
        )?;
        Ok(())
    }

    /// 取会话存的 cwd。
    pub fn session_cwd(&self, session_id: i64) -> Result<Option<String>, StoreError> {
        let r = self.conn.query_row(
            "SELECT cwd FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get::<_, Option<String>>(0),
        )?;
        Ok(r)
    }

    /// 记录会话所属进程 PID（来自 reporter 在 SessionStart 抓取的 claude.exe 父进程）。
    pub fn set_session_pid(&self, session_id: i64, pid: i64, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET pid = ?1, last_event_at = ?2 WHERE id = ?3",
            rusqlite::params![pid, now_ms, session_id],
        )?;
        Ok(())
    }

    /// 取所有 live(running/waiting/stale) 会话的 (id, pid, last_event_at)，供 app 做存活清理。
    pub fn live_session_liveness(&self) -> Result<Vec<(i64, Option<i64>, i64)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, pid, last_event_at FROM sessions WHERE status IN ('running','waiting','stale')",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 手动归档/取消归档某会话。不更新 last_event_at，避免排序乱跳。
    pub fn set_session_archived(&self, session_id: i64, archived: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET archived = ?1 WHERE id = ?2",
            rusqlite::params![archived as i64, session_id],
        )?;
        Ok(())
    }
}

/// 按字符（非字节）截断，避免切坏多字节中文。
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// 移除形如 `[Image #N]`（以及任意 `[Image ...]`）的占位标记。
fn strip_image_markers(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("[Image") {
        if let Some(rel_end) = result[start..].find(']') {
            result.replace_range(start..start + rel_end + 1, "");
        } else {
            break; // 没有闭合 ] 就停，避免死循环
        }
    }
    result
}

/// 清洗 prompt：剔除图片标记 + 折叠空白 + 去首尾空白。
fn sanitize_prompt(s: &str) -> String {
    strip_image_markers(s)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 无 todo -> todo；有 in_progress 或部分完成 -> doing；全 completed -> done。
fn derive_column(todos: &[TodoInput]) -> TaskColumn {
    if todos.is_empty() {
        return TaskColumn::Todo;
    }
    if todos.iter().all(|t| t.status == TodoStatus::Completed) {
        return TaskColumn::Done;
    }
    if todos.iter().any(|t| t.status != TodoStatus::Pending) {
        return TaskColumn::Doing;
    }
    TaskColumn::Todo
}
