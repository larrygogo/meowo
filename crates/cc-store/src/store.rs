use crate::error::StoreError;
use crate::migrations::SCHEMA;
use crate::models::{PendingReview, Project, Session, SessionStatus, Task, TaskColumn, Todo, TodoInput, TodoStatus};
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
        // busy_timeout 必须先于 journal_mode：否则并发首次建库时 WAL 切换以 0 超时直接 BUSY。
        conn.pragma_update(None, "busy_timeout", 3000)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::init(&conn)?;
        Ok(Store { conn })
    }

    /// 仅用于测试：内存库。
    pub fn open_in_memory() -> Result<Store, StoreError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::init(&conn)?;
        Ok(Store { conn })
    }

    /// schema 版本：升 schema/加迁移时 +1。
    /// v2: 新增 session_notes 表（会话便签）。旧库 version<2 时 init 会重跑
    /// `CREATE TABLE IF NOT EXISTS` 把新表补上，再 bump。
    /// v3: sessions 加 pending_review / last_ai_text / last_user_text 三列。
    /// v4: session_context 加 model 列（statusline 的模型展示名）。
    /// v5: sessions 加 provider 列（agent 提供方：claude/kimi…）。
    const USER_VERSION: i64 = 5;

    /// 一次性建表 + 迁移 + 建索引，用 `PRAGMA user_version` 门控：已是最新版直接返回，
    /// 避免 statusline/hook 每次 open 都重跑 DDL 与注定失败的 ALTER（hot-path 浪费）。
    /// 迁移/建索引若遇非「列已存在」错误（BUSY/IO）则**不**bump 版本，下次 open 自动重试，
    /// 不再把瞬时错误永久吞掉。
    fn init(conn: &rusqlite::Connection) -> Result<(), StoreError> {
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if version >= Self::USER_VERSION {
            return Ok(());
        }
        conn.execute_batch(SCHEMA)?;
        // 给旧库补列（新库 SCHEMA 已含这些列 → ALTER 必报 duplicate，忽略即可）。
        const ALTERS: [&str; 9] = [
            "ALTER TABLE sessions ADD COLUMN pid INTEGER",
            "ALTER TABLE sessions ADD COLUMN cwd TEXT",
            "ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE sessions ADD COLUMN archived_at INTEGER",
            "ALTER TABLE sessions ADD COLUMN pending_review TEXT",
            "ALTER TABLE sessions ADD COLUMN last_ai_text TEXT",
            "ALTER TABLE sessions ADD COLUMN last_user_text TEXT",
            "ALTER TABLE session_context ADD COLUMN model TEXT",
            "ALTER TABLE sessions ADD COLUMN provider TEXT NOT NULL DEFAULT 'claude'",
        ];
        for sql in ALTERS {
            if let Err(e) = conn.execute(sql, []) {
                if !e.to_string().contains("duplicate column name") {
                    eprintln!("cc-store migrate 失败: {sql}: {e}");
                    return Ok(()); // 非「列已存在」（BUSY/IO）：不 bump，下次 open 重试
                }
            }
        }
        // 索引：加速按 project / task / pid 的查询与「驱逐旧会话」（小库无感，大库防全表扫）。
        const INDEXES: [&str; 4] = [
            "CREATE INDEX IF NOT EXISTS ix_sessions_project ON sessions(project_id)",
            "CREATE INDEX IF NOT EXISTS ix_sessions_pid ON sessions(pid)",
            "CREATE INDEX IF NOT EXISTS ix_tasks_project_col ON tasks(project_id, column_name)",
            "CREATE INDEX IF NOT EXISTS ix_todos_task ON todos(task_id)",
        ];
        for sql in INDEXES {
            if let Err(e) = conn.execute(sql, []) {
                eprintln!("cc-store 建索引失败: {sql}: {e}");
                return Ok(()); // 同上：不 bump，下次重试
            }
        }
        let _ = conn.pragma_update(None, "user_version", Self::USER_VERSION);
        Ok(())
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
             ON CONFLICT(root_path) DO UPDATE SET updated_at = MAX(updated_at, ?3), name = ?2",
            rusqlite::params![root_path, name, now_ms],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT id FROM projects WHERE root_path = ?1",
            [root_path],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// 写入/更新某会话的上下文用量与模型展示名（来自 Claude Code statusline）。
    pub fn set_session_context(
        &self,
        cc_session_id: &str,
        used_pct: Option<i64>,
        window_size: Option<i64>,
        model: Option<&str>,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO session_context (cc_session_id, used_pct, window_size, model, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(cc_session_id) DO UPDATE SET
                 used_pct = COALESCE(excluded.used_pct, used_pct),
                 window_size = COALESCE(excluded.window_size, window_size),
                 model = COALESCE(excluded.model, model),
                 updated_at = excluded.updated_at",
            rusqlite::params![cc_session_id, used_pct, window_size, model, now_ms],
        )?;
        Ok(())
    }

    /// 写入/清除某会话的便签：trim 后非空则 upsert，空则删除该行（便签清空即移除）。
    pub fn set_session_note(
        &self,
        cc_session_id: &str,
        note: &str,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            self.conn.execute(
                "DELETE FROM session_notes WHERE cc_session_id = ?1",
                [cc_session_id],
            )?;
        } else {
            self.conn.execute(
                "INSERT INTO session_notes (cc_session_id, note, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(cc_session_id) DO UPDATE SET note = excluded.note, updated_at = excluded.updated_at",
                rusqlite::params![cc_session_id, trimmed, now_ms],
            )?;
        }
        Ok(())
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
        // 事务保证「会话 + 占位任务」原子落库，避免中途失败留下无任务卡的半态会话。
        let tx = self.conn.unchecked_transaction()?;
        // 会话幂等：已存在则复活为 running（resume/--continue 场景），清掉 ended_at。
        tx.execute(
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
        tx.execute(
            "INSERT OR IGNORE INTO tasks
                (project_id, session_id, title, column_name, column_locked, created_at, updated_at)
             VALUES (?1, ?2, '(未命名会话)', 'todo', 0, ?3, ?3)",
            rusqlite::params![project_id, sid, now_ms],
        )?;
        let tid = self.task_id_of_session(sid)?;
        tx.commit()?;
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
        // 按字符截断，防超大 Bash 命令整条进库并随轮询全量下发。
        let activity = truncate_chars(activity, 200);
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

    /// 收到用户 prompt：仅当占位标题时替换为截断后的 prompt(不再写 current_activity，那已由 last_user_text 承担)。
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
                    "UPDATE tasks SET title = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![cleaned, now_ms, tid],
                )?;
            }
            // 非占位标题:不再把 prompt 写进 current_activity(改由 last_user_text 承担)。
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

    /// 设置待审批子态,同时刷新 last_event_at(让卡片排到最近活跃,并作为去重指纹)。
    pub fn set_pending_review(
        &self,
        session_id: i64,
        kind: PendingReview,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET pending_review = ?1, last_event_at = ?2 WHERE id = ?3",
            rusqlite::params![kind.as_str(), now_ms, session_id],
        )?;
        Ok(())
    }

    /// 清除待审批子态(置 NULL)。不动 last_event_at——由同回合的兄弟调用负责时间戳。
    pub fn clear_pending_review(&self, session_id: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET pending_review = NULL WHERE id = ?1",
            rusqlite::params![session_id],
        )?;
        Ok(())
    }

    /// 落最近一条 AI 正文:折叠空白 + 截断 200 字符;空/全空白不覆盖旧值。
    /// 不动 last_event_at——Stop 的兄弟 set_session_status 已刷新它。
    pub fn set_last_ai_text(&self, session_id: i64, text: &str) -> Result<(), StoreError> {
        let cleaned = truncate_chars(&sanitize_prompt(text), 200);
        if cleaned.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE sessions SET last_ai_text = ?1 WHERE id = ?2",
            rusqlite::params![cleaned, session_id],
        )?;
        Ok(())
    }

    /// 落最近一条用户消息:复用 sanitize_prompt(剥图片标记 + 折叠空白) + 截断 200;空不覆盖。
    /// 不动 last_event_at——UserPromptSubmit 的 on_user_prompt(touch_session) 已刷新它。
    pub fn set_last_user_text(&self, session_id: i64, text: &str) -> Result<(), StoreError> {
        let cleaned = truncate_chars(&sanitize_prompt(text), 200);
        if cleaned.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE sessions SET last_user_text = ?1 WHERE id = ?2",
            rusqlite::params![cleaned, session_id],
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
        // 事务保证「会话 + 任务卡」原子落库：DO NOTHING 的幂等判断使半态永不重试，必须避免。
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute(
            "INSERT INTO sessions
                (project_id, cc_session_id, status, started_at, last_event_at, ended_at, cwd)
             VALUES (?1, ?2, 'ended', ?3, ?3, ?3, ?4)
             ON CONFLICT(cc_session_id) DO NOTHING",
            rusqlite::params![project_id, cc_session_id, last_event_at, cwd],
        )?;
        if n == 0 {
            return Ok(false); // 已存在，绝不覆盖（事务随 drop 回滚，无写入）
        }
        let sid = self
            .find_session_id(cc_session_id)?
            .ok_or(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))?;
        let mut t = truncate_chars(title.trim(), 80);
        if t.is_empty() {
            t = "(未命名会话)".to_string();
        }
        // 历史已结束会话的任务卡固定放 done 列，不导入 todo。
        tx.execute(
            "INSERT OR IGNORE INTO tasks
                (project_id, session_id, title, column_name, column_locked, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'done', 0, ?4, ?4)",
            rusqlite::params![project_id, sid, t, last_event_at],
        )?;
        tx.commit()?;
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

    /// 设置会话所属 agent provider（claude/kimi…）。仅在 SessionStart 由 reporter 按 --provider 写一次；
    /// 不动 last_event_at（同回合的 set_session_cwd 等已刷新）。
    pub fn set_session_provider(&self, session_id: i64, provider: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE sessions SET provider = ?1 WHERE id = ?2",
            rusqlite::params![provider, session_id],
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
    ///
    /// 一个 claude 进程同一时刻只属于一个会话：故先把这个 pid 从**其它**会话上摘掉
    /// （它们已被 /clear、resume、或同进程开新会话取代），否则旧会话会因进程仍存活而一直
    /// 误显示「已连接」。pid 复用（旧进程退出、号被新 claude 占用）也由此一并纠正。
    pub fn set_session_pid(&self, session_id: i64, pid: i64, now_ms: i64) -> Result<(), StoreError> {
        // 事务保证「本会话认领 + 旧会话收尾」原子完成，避免交错留下两个会话同持一个 pid。
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE sessions SET pid = ?1, last_event_at = ?2 WHERE id = ?3",
            rusqlite::params![pid, now_ms, session_id],
        )?;
        // 被同一进程的新会话顶替的旧会话：直接收尾为 ended（pid 清空、记 ended_at），
        // 这样 /clear 一发生旧会话立刻从 live 列表消失，而不是只摘 pid 留个空壳。
        // 时间戳保护：只收尾 last_event_at 更旧的会话，迟到的旧会话 hook 无法反杀更活跃的新会话。
        tx.execute(
            "UPDATE sessions SET pid = NULL, status = 'ended', ended_at = ?2 \
             WHERE pid = ?1 AND id <> ?3 AND status <> 'ended' \
               AND last_event_at < (SELECT last_event_at FROM sessions WHERE id = ?3)",
            rusqlite::params![pid, now_ms, session_id],
        )?;
        tx.commit()?;
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
    /// 归档时记录 archived_at（用于「归档超过 N 天自动隐藏」）；取消归档清空。
    pub fn set_session_archived(
        &self,
        session_id: i64,
        archived: bool,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let archived_at: Option<i64> = if archived { Some(now_ms) } else { None };
        self.conn.execute(
            "UPDATE sessions SET archived = ?1, archived_at = ?2 WHERE id = ?3",
            rusqlite::params![archived as i64, archived_at, session_id],
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
