//! 已停用的「Meowo 托管」会话（PR #172 曾让 Meowo 直接驱动 codex app-server / ACP / opencode HTTP
//! 跑对话）只剩只读的遗留结构。leader 2026-11-13 定的原则：对话一律依托官方 CLI，Meowo 只做上层
//! 整合，托管路线整块删除。
//!
//! 表和数据一律保留（删数据须用户另行同意）：
//! - 用过托管会话的库里有真实数据；新库也照建这些表，结构与 v21–v25 一致，免得同一 user_version
//!   下有的库有表、有的库没有（同 v12/v13 死结构的处理）。
//! - 宿主只读两件事：这个会话是不是托管会话、有没有绑定到 CLI 自己的会话 id（有就能用官方 CLI 恢复）。
use crate::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS managed_conversations (
 session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
 provider TEXT NOT NULL, cwd TEXT NOT NULL, profile_id TEXT,
 native_session_id TEXT, phase TEXT NOT NULL,
 adopted INTEGER NOT NULL DEFAULT 0, runtime_owner TEXT, runtime_pid INTEGER,
 last_error TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_managed_native
 ON managed_conversations(provider, COALESCE(profile_id,''), native_session_id)
 WHERE native_session_id IS NOT NULL;
CREATE TABLE IF NOT EXISTS managed_deliveries (
 delivery_id TEXT PRIMARY KEY, session_id INTEGER NOT NULL REFERENCES managed_conversations(session_id) ON DELETE CASCADE,
 ordinal INTEGER NOT NULL, text TEXT NOT NULL, attachments_json TEXT NOT NULL, gate_ref_json TEXT,
 origin TEXT NOT NULL DEFAULT 'user',
 state TEXT NOT NULL, native_message_id TEXT, last_error TEXT,
 accepted_at INTEGER, consumed_at INTEGER, completed_at INTEGER,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 UNIQUE(session_id, ordinal)
);
CREATE UNIQUE INDEX IF NOT EXISTS ux_managed_native_message
 ON managed_deliveries(session_id, native_message_id) WHERE native_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_managed_queued ON managed_deliveries(session_id,state,ordinal);
CREATE INDEX IF NOT EXISTS ix_managed_pending_heads ON managed_deliveries(session_id,ordinal) WHERE state NOT IN ('completed','cancelled');
CREATE INDEX IF NOT EXISTS ix_managed_latest_user ON managed_deliveries(session_id,ordinal DESC) WHERE gate_ref_json IS NULL;
CREATE TABLE IF NOT EXISTS managed_events (
 session_id INTEGER NOT NULL REFERENCES managed_conversations(session_id) ON DELETE CASCADE,
 sequence INTEGER NOT NULL, payload_json TEXT NOT NULL, created_at INTEGER NOT NULL,
 PRIMARY KEY(session_id,sequence)
);
CREATE TABLE IF NOT EXISTS managed_chat_items (
 session_id INTEGER NOT NULL REFERENCES managed_conversations(session_id) ON DELETE CASCADE,
 ordinal INTEGER NOT NULL, item_id TEXT NOT NULL, kind TEXT NOT NULL,
 payload_json TEXT NOT NULL, history INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,ordinal)
);
CREATE INDEX IF NOT EXISTS ix_managed_chat_item ON managed_chat_items(session_id,item_id,ordinal);
CREATE TABLE IF NOT EXISTS managed_facts (
 session_id INTEGER NOT NULL REFERENCES managed_conversations(session_id) ON DELETE CASCADE,
 kind TEXT NOT NULL, fact_key TEXT NOT NULL, sequence INTEGER NOT NULL,
 payload_json TEXT NOT NULL, created_at INTEGER NOT NULL, resolved INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(session_id,kind,fact_key)
);
CREATE TABLE IF NOT EXISTS managed_delivery_dispositions (
 session_id INTEGER NOT NULL REFERENCES managed_conversations(session_id) ON DELETE CASCADE,
 delivery_id TEXT NOT NULL, action TEXT NOT NULL, replacement_id TEXT, created_at INTEGER NOT NULL,
 PRIMARY KEY(session_id,delivery_id)
);
CREATE TABLE IF NOT EXISTS managed_native_recoveries (
 session_id INTEGER NOT NULL, sequence INTEGER NOT NULL, unresolved INTEGER NOT NULL DEFAULT 1,
 PRIMARY KEY(session_id,sequence),
 FOREIGN KEY(session_id,sequence) REFERENCES managed_events(session_id,sequence) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS ix_managed_native_recovery_page ON managed_native_recoveries(session_id,unresolved,sequence DESC);
CREATE TABLE IF NOT EXISTS managed_native_recovery_messages (
 session_id INTEGER NOT NULL,sequence INTEGER NOT NULL,message_index INTEGER NOT NULL,
 role TEXT NOT NULL,text TEXT NOT NULL,
 PRIMARY KEY(session_id,sequence,message_index),
 FOREIGN KEY(session_id,sequence) REFERENCES managed_native_recoveries(session_id,sequence) ON DELETE CASCADE
);
"#;

/// 已停用的「Meowo 托管」会话的只读视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredManaged {
    pub provider: String,
    /// CLI 自己的会话 id；绑定成功时它也写进了 sessions.cc_session_id，所以原生恢复路径直接可用。
    pub native_session_id: Option<String>,
}

impl Store {
    /// 该会话若是已停用的「Meowo 托管」会话，返回它的 provider 与原生会话 id。
    pub fn retired_managed(&self, session_id: i64) -> Result<Option<RetiredManaged>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT provider,native_session_id FROM managed_conversations WHERE session_id=?1",
                [session_id],
                |r| {
                    Ok(RetiredManaged {
                        provider: r.get(0)?,
                        native_session_id: r
                            .get::<_, Option<String>>(1)?
                            .filter(|id| !id.trim().is_empty()),
                    })
                },
            )
            .optional()?)
    }

    /// 启动时把还挂着运行/等待的托管会话收尾为已结束：驱动它们的宿主 worker 已删除，不会再有
    /// 事件把它们推进到终态，不收尾就会在看板上永远显示「运行中」。只改 sessions 的状态列，
    /// managed_* 表与对话数据原样保留。返回收尾的条数。
    ///
    /// 已用「用官方 CLI 打开」恢复成原生会话的（独占 pid，pid_shared=0）不动：它们由原生
    /// 存活判定管，这里一刀切会把正在跑的 CLI 会话误标成已结束。
    pub fn end_retired_managed_sessions(&self, now: i64) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE sessions SET status='ended',pid=NULL,pid_shared=0,ended_at=COALESCE(ended_at,?1),pending_review=NULL \
             WHERE status<>'ended' AND (pid IS NULL OR pid_shared=1) \
             AND id IN (SELECT session_id FROM managed_conversations)",
            params![now],
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 旧版本写下的托管会话：表与数据保留、能读出原生 id，启动收尾只动状态列。
    #[test]
    fn retired_rows_stay_readable_and_are_ended_without_touching_data() {
        let store = Store::open_in_memory().unwrap();
        let project = store.upsert_project_by_root("C:/repo", "repo", 1).unwrap();
        let (bound, _) = store.start_session(project, "thread-1", 1).unwrap();
        let (unbound, _) = store.start_session(project, "meowo-managed-x", 1).unwrap();
        let (native, _) = store.start_session(project, "plain", 1).unwrap();
        for (id, native_id) in [(bound, Some("thread-1")), (unbound, None)] {
            store
                .conn
                .execute(
                    "INSERT INTO managed_conversations(session_id,provider,cwd,native_session_id,phase,created_at,updated_at) VALUES(?1,'codex','C:/repo',?2,'ready',1,1)",
                    params![id, native_id],
                )
                .unwrap();
        }
        store
            .conn
            .execute(
                "INSERT INTO managed_events(session_id,sequence,payload_json,created_at) VALUES(?1,1,'{}',1)",
                [bound],
            )
            .unwrap();
        assert_eq!(
            store.retired_managed(bound).unwrap(),
            Some(RetiredManaged {
                provider: "codex".into(),
                native_session_id: Some("thread-1".into())
            })
        );
        assert_eq!(store.retired_managed(unbound).unwrap().unwrap().native_session_id, None);
        assert_eq!(store.retired_managed(native).unwrap(), None);

        // 已恢复成原生 CLI 会话的（独占 pid）不收尾。
        let (resumed, _) = store.start_session(project, "thread-resumed", 1).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO managed_conversations(session_id,provider,cwd,native_session_id,phase,created_at,updated_at) VALUES(?1,'codex','C:/repo','thread-resumed','failed',1,1)",
                [resumed],
            )
            .unwrap();
        store.set_session_pid(resumed, 4242, 2).unwrap();

        assert_eq!(store.end_retired_managed_sessions(5).unwrap(), 2);
        assert_ne!(store.get_session(resumed).unwrap().status, "ended");
        assert_eq!(store.get_session(bound).unwrap().status, "ended");
        assert_ne!(store.get_session(native).unwrap().status, "ended");
        assert_eq!(store.end_retired_managed_sessions(6).unwrap(), 0);
        let events: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM managed_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(events, 1, "对话数据不删");
    }
}
