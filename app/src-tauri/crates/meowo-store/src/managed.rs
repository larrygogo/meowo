pub use crate::managed_chat::ManagedChatWindow;
pub use crate::managed_recovery::{
    ManagedNativeRecoveryChunk, ManagedNativeRecoveryCursor, ManagedNativeRecoveryPage,
};
use crate::{store::ImmediateTx, Store, StoreError};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

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

const SESSION_COLUMNS: &str = "session_id,provider,cwd,profile_id,native_session_id,phase,adopted,runtime_owner,runtime_pid,last_error,created_at,updated_at";
const DELIVERY_COLUMNS: &str = "session_id,delivery_id,ordinal,text,attachments_json,gate_ref_json,state,native_message_id,last_error,accepted_at,consumed_at,completed_at,created_at,updated_at,origin";
pub const MANAGED_PAGE_LIMIT: usize = 200;
pub(crate) const MAX_JSON_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedSession {
    pub session_id: i64,
    pub provider: String,
    pub cwd: String,
    pub profile_id: Option<String>,
    pub native_session_id: Option<String>,
    pub phase: String,
    pub adopted: bool,
    pub runtime_owner: Option<String>,
    pub runtime_pid: Option<i64>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedDelivery {
    pub session_id: i64,
    pub delivery_id: String,
    pub ordinal: i64,
    pub text: String,
    pub origin: String,
    pub attachments_json: String,
    pub gate_ref_json: Option<String>,
    pub state: String,
    pub native_message_id: Option<String>,
    pub last_error: Option<String>,
    pub accepted_at: Option<i64>,
    pub consumed_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedEvent {
    pub session_id: i64,
    pub sequence: i64,
    pub payload_json: String,
    pub created_at: i64,
}

pub(crate) fn invalid(code: &str) -> StoreError {
    StoreError::InvalidInput(format!("conversation/{code}"))
}

pub fn validate_delivery_id(value: &str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid("delivery_id_invalid"));
    }
    Ok(())
}

fn nonempty(value: &str, max: usize) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(invalid("invalid_input"));
    }
    Ok(())
}

pub(crate) fn canonical_json(value: &str) -> Result<String, StoreError> {
    if value.len() > MAX_JSON_BYTES {
        return Err(invalid("payload_too_large"));
    }
    let json: serde_json::Value =
        serde_json::from_str(value).map_err(|_| invalid("invalid_json"))?;
    serde_json::to_string(&json).map_err(|_| invalid("invalid_json"))
}

fn session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedSession> {
    Ok(ManagedSession {
        session_id: row.get(0)?,
        provider: row.get(1)?,
        cwd: row.get(2)?,
        profile_id: row.get(3)?,
        native_session_id: row.get(4)?,
        phase: row.get(5)?,
        adopted: row.get(6)?,
        runtime_owner: row.get(7)?,
        runtime_pid: row.get(8)?,
        last_error: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn delivery_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedDelivery> {
    Ok(ManagedDelivery {
        session_id: row.get(0)?,
        delivery_id: row.get(1)?,
        ordinal: row.get(2)?,
        text: row.get(3)?,
        attachments_json: row.get(4)?,
        gate_ref_json: row.get(5)?,
        state: row.get(6)?,
        native_message_id: row.get(7)?,
        last_error: row.get(8)?,
        accepted_at: row.get(9)?,
        consumed_at: row.get(10)?,
        completed_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        origin: row.get(14)?,
    })
}

fn valid_phase(from: &str, to: &str) -> bool {
    const PHASES: &[&str] = &[
        "starting", "ready", "running", "blocked", "stopping", "stopped", "failed",
    ];
    PHASES.contains(&from)
        && PHASES.contains(&to)
        && (from == to
            || match from {
                "starting" => matches!(
                    to,
                    "ready" | "running" | "blocked" | "stopping" | "stopped" | "failed"
                ),
                "ready" | "running" | "blocked" => matches!(
                    to,
                    "ready" | "running" | "blocked" | "stopping" | "stopped" | "failed"
                ),
                "stopping" => matches!(to, "stopped" | "failed"),
                "stopped" => to == "starting",
                "failed" => matches!(to, "starting" | "stopping" | "stopped"),
                _ => false,
            })
}

fn valid_delivery(from: &str, to: &str) -> bool {
    if from == to
        && [
            "queued",
            "sending",
            "accepted",
            "consumed",
            "completed",
            "uncertain",
            "failed",
            "cancelled",
        ]
        .contains(&from)
    {
        return true;
    }
    match from {
        "queued" => matches!(to, "sending" | "failed" | "cancelled"),
        "sending" => matches!(
            to,
            "accepted" | "consumed" | "completed" | "uncertain" | "failed" | "cancelled"
        ),
        "accepted" => matches!(
            to,
            "consumed" | "completed" | "uncertain" | "failed" | "cancelled"
        ),
        "consumed" => matches!(to, "completed" | "uncertain" | "failed" | "cancelled"),
        // 原生历史对账可消除不确定性，但它绝不能自动回到发送队列。
        "uncertain" => matches!(to, "consumed" | "completed" | "failed" | "cancelled"),
        _ => false,
    }
}

impl Store {
    pub fn managed_delivery_replacement(
        &self,
        session_id: i64,
        delivery_id: &str,
    ) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT replacement_id FROM managed_delivery_dispositions WHERE session_id=?1 AND delivery_id=?2 AND action='retry'",params![session_id,delivery_id],|row|row.get(0)).optional()?.flatten())
    }
    /// 人类处置与替代入队同事务；重试必须换新 id，旧回执保留且重复点击不复制第二条消息。
    pub fn managed_delivery_action(
        &self,
        session_id: i64,
        delivery_id: &str,
        action: &str,
        replacement_id: Option<&str>,
        now: i64,
    ) -> Result<ManagedDelivery, StoreError> {
        if !matches!(action, "dismiss" | "retry")
            || (action == "dismiss" && replacement_id.is_some())
        {
            return Err(invalid("delivery_action_invalid"));
        }
        let replacement = if action == "retry" {
            let value = replacement_id.ok_or_else(|| invalid("replacement_required"))?;
            validate_delivery_id(value)?;
            if value == delivery_id {
                return Err(invalid("replacement_required"));
            }
            Some(value)
        } else {
            None
        };
        let tx = ImmediateTx::begin(&self.conn)?;
        let state = self
            .managed_state(session_id)?
            .ok_or_else(|| invalid("session_missing"))?;
        let original = self
            .managed_delivery(session_id, delivery_id)?
            .ok_or_else(|| invalid("delivery_missing"))?;
        let previous: Option<(String, Option<String>)> = self.conn.query_row("SELECT action,replacement_id FROM managed_delivery_dispositions WHERE session_id=?1 AND delivery_id=?2",params![session_id,delivery_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((previous_action, previous_replacement)) = previous {
            if previous_action != action || previous_replacement.as_deref() != replacement {
                return Err(invalid("delivery_action_conflict"));
            }
            let result = self
                .managed_delivery(session_id, replacement.unwrap_or(delivery_id))?
                .ok_or_else(|| invalid("delivery_missing"))?;
            tx.commit()?;
            return Ok(result);
        }
        if !matches!(
            original.state.as_str(),
            "queued" | "failed" | "uncertain" | "consumed"
        ) {
            return Err(invalid("delivery_action_invalid"));
        }
        if matches!(original.state.as_str(), "uncertain" | "consumed")
            && state.runtime_owner.is_some()
        {
            return Err(invalid("delivery_engine_live"));
        }
        if let Some(replacement) = replacement {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM managed_deliveries WHERE delivery_id=?1)",
                [replacement],
                |r| r.get(0),
            )?;
            if exists {
                return Err(invalid("delivery_id_conflict"));
            }
            self.conn.execute("INSERT INTO managed_deliveries(delivery_id,session_id,ordinal,text,attachments_json,gate_ref_json,state,created_at,updated_at,origin) SELECT ?1,?2,COALESCE(MAX(ordinal),0)+1,?3,?4,?5,'queued',?6,?6,?7 FROM managed_deliveries WHERE session_id=?2",params![replacement,session_id,original.text,original.attachments_json,original.gate_ref_json,now,original.origin])?;
        }
        self.conn.execute("UPDATE managed_deliveries SET state='cancelled',updated_at=MAX(updated_at,?1) WHERE session_id=?2 AND delivery_id=?3",params![now,session_id,delivery_id])?;
        self.conn.execute("INSERT INTO managed_delivery_dispositions(session_id,delivery_id,action,replacement_id,created_at) VALUES(?1,?2,?3,?4,?5)",params![session_id,delivery_id,action,replacement,now])?;
        let result = self
            .managed_delivery(session_id, replacement.unwrap_or(delivery_id))?
            .ok_or_else(|| invalid("delivery_missing"))?;
        tx.commit()?;
        Ok(result)
    }

    pub fn managed_create(
        &self,
        provider: &str,
        cwd: &str,
        profile_id: Option<&str>,
        now: i64,
    ) -> Result<i64, StoreError> {
        nonempty(provider, 64)?;
        nonempty(cwd, 32768)?;
        if let Some(profile) = profile_id {
            nonempty(profile, 1024)?;
        }
        let tx = ImmediateTx::begin(&self.conn)?;
        let name = cwd
            .rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or(cwd);
        let project = self.upsert_project_by_root(cwd, name, now)?;
        // SQLite 生成随机 UUID，避免未绑定的原生 ID 与其他会话相撞；会话主键从此保持不变。
        let uuid: String = self
            .conn
            .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?;
        let temporary = format!(
            "meowo-managed-{}-{}-{}-{}-{}",
            &uuid[..8],
            &uuid[8..12],
            &uuid[12..16],
            &uuid[16..20],
            &uuid[20..]
        );
        self.conn.execute("INSERT INTO sessions(project_id,cc_session_id,status,started_at,last_event_at,cwd,provider,profile) VALUES(?1,?2,'waiting',?3,?3,?4,?5,?6)",params![project,temporary,now,cwd,provider,profile_id])?;
        let id = self.conn.last_insert_rowid();
        self.conn.execute("INSERT INTO tasks(project_id,session_id,title,column_name,column_locked,created_at,updated_at) VALUES(?1,?2,'(未命名会话)','todo',0,?3,?3)",params![project,id,now])?;
        self.conn.execute("INSERT INTO managed_conversations(session_id,provider,cwd,profile_id,phase,created_at,updated_at) VALUES(?1,?2,?3,?4,'starting',?5,?5)",params![id,provider,cwd,profile_id,now])?;
        tx.commit()?;
        Ok(id)
    }

    /// 仅登记明确移交，不启动或终止原进程；宿主必须先确认原 writer 已退出。
    pub fn managed_adopt(&self, existing_id: i64, now: i64) -> Result<ManagedSession, StoreError> {
        let tx = ImmediateTx::begin(&self.conn)?;
        if let Some(existing) = self.managed_state(existing_id)? {
            tx.commit()?;
            return Ok(existing);
        }
        let (provider,cwd,profile,native): (String,Option<String>,Option<String>,String) = self.conn.query_row(
            "SELECT s.provider,COALESCE(NULLIF(s.cwd,''),p.root_path),s.profile,s.cc_session_id FROM sessions s JOIN projects p ON p.id=s.project_id WHERE s.id=?1 AND s.superseded_by IS NULL",[existing_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or_else(|| invalid("session_missing"))?;
        let cwd = cwd
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| invalid("cwd_missing"))?;
        nonempty(&native, 4096)?;
        self.conn.execute("INSERT INTO managed_conversations(session_id,provider,cwd,profile_id,native_session_id,phase,adopted,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'stopped',1,?6,?6)",params![existing_id,provider,cwd,profile,native,now])?;
        let state = self
            .managed_state(existing_id)?
            .ok_or_else(|| invalid("session_missing"))?;
        tx.commit()?;
        Ok(state)
    }

    pub fn managed_state(&self, session_id: i64) -> Result<Option<ManagedSession>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM managed_conversations WHERE session_id=?1"),
                [session_id],
                session_row,
            )
            .optional()?)
    }

    pub fn managed_states(&self) -> Result<Vec<ManagedSession>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM managed_conversations ORDER BY session_id"
        ))?;
        let rows = stmt
            .query_map([], session_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn managed_bind(
        &self,
        session_id: i64,
        native_id: &str,
        now: i64,
    ) -> Result<ManagedSession, StoreError> {
        nonempty(native_id, 4096)?;
        if native_id.starts_with("meowo-managed-") {
            return Err(invalid("invalid_native_id"));
        }
        let tx = ImmediateTx::begin(&self.conn)?;
        let state = self
            .managed_state(session_id)?
            .ok_or_else(|| invalid("session_missing"))?;
        if let Some(bound) = &state.native_session_id {
            if bound != native_id {
                return Err(invalid("binding_conflict"));
            }
            tx.commit()?;
            return Ok(state);
        }
        let collision: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE cc_session_id=?1 AND id<>?2)",
            params![native_id, session_id],
            |r| r.get(0),
        )?;
        if collision {
            return Err(invalid("native_bound_elsewhere"));
        }
        let old: String = self.conn.query_row(
            "SELECT cc_session_id FROM sessions WHERE id=?1",
            [session_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "UPDATE sessions SET cc_session_id=?1,last_event_at=MAX(last_event_at,?2) WHERE id=?3",
            params![native_id, now, session_id],
        )?;
        self.conn.execute(
            "UPDATE session_context SET cc_session_id=?1 WHERE cc_session_id=?2",
            params![native_id, old],
        )?;
        self.conn.execute("UPDATE managed_conversations SET native_session_id=?1,updated_at=MAX(updated_at,?2) WHERE session_id=?3 AND native_session_id IS NULL",params![native_id,now,session_id])?;
        let result = self
            .managed_state(session_id)?
            .ok_or_else(|| invalid("session_missing"))?;
        tx.commit()?;
        Ok(result)
    }

    pub fn managed_set_phase(
        &self,
        session_id: i64,
        expected: &str,
        next: &str,
        error: Option<&str>,
        now: i64,
    ) -> Result<bool, StoreError> {
        if !valid_phase(expected, next) {
            return Err(invalid("invalid_phase_transition"));
        }
        let tx = ImmediateTx::begin(&self.conn)?;
        let changed = self.conn.execute("UPDATE managed_conversations SET phase=?1,last_error=?2,updated_at=?3 WHERE session_id=?4 AND phase=?5 AND updated_at<=?3",params![next,error,now,session_id,expected])? > 0;
        if changed {
            self.conn.execute("UPDATE sessions SET status=CASE WHEN ?1='stopped' THEN 'ended' WHEN ?1='running' THEN 'running' ELSE 'waiting' END,last_event_at=MAX(last_event_at,?2),ended_at=CASE WHEN ?1='stopped' THEN ?2 ELSE NULL END,pid=CASE WHEN ?1='stopped' THEN NULL ELSE pid END WHERE id=?3",params![next,now,session_id])?;
        }
        tx.commit()?;
        Ok(changed)
    }

    pub fn managed_acquire_runtime(
        &self,
        session_id: i64,
        owner: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        nonempty(owner, 1024)?;
        Ok(self.conn.execute("UPDATE managed_conversations SET runtime_owner=?1,runtime_pid=NULL,updated_at=MAX(updated_at,?2) WHERE session_id=?3 AND runtime_owner IS NULL",params![owner,now,session_id])? > 0)
    }

    pub fn managed_set_runtime_pid(
        &self,
        session_id: i64,
        owner: &str,
        pid: i64,
        now: i64,
    ) -> Result<bool, StoreError> {
        if pid <= 0 {
            return Err(invalid("invalid_pid"));
        }
        Ok(self.conn.execute("UPDATE managed_conversations SET runtime_pid=?1,updated_at=MAX(updated_at,?2) WHERE session_id=?3 AND runtime_owner=?4",params![pid,now,session_id,owner])? > 0)
    }

    /// 不能靠租约超时清 slot：进程暂停或网络变慢不等于原 writer 已退出。
    pub fn managed_release_runtime(
        &self,
        session_id: i64,
        owner: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE managed_conversations SET runtime_owner=NULL,runtime_pid=NULL,updated_at=MAX(updated_at,?1) WHERE session_id=?2 AND runtime_owner=?3",params![now,session_id,owner])? > 0)
    }

    pub fn managed_enqueue(
        &self,
        session_id: i64,
        delivery_id: &str,
        text: &str,
        attachments_json: &str,
        gate_ref_json: Option<&str>,
        now: i64,
    ) -> Result<ManagedDelivery, StoreError> {
        self.managed_enqueue_origin(
            session_id,
            delivery_id,
            text,
            attachments_json,
            gate_ref_json,
            now,
            "user",
        )
    }

    pub fn managed_enqueue_lead(
        &self,
        session_id: i64,
        delivery_id: &str,
        text: &str,
        attachments_json: &str,
        gate_ref_json: Option<&str>,
        now: i64,
    ) -> Result<ManagedDelivery, StoreError> {
        self.managed_enqueue_origin(
            session_id,
            delivery_id,
            text,
            attachments_json,
            gate_ref_json,
            now,
            "lead",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn managed_enqueue_origin(
        &self,
        session_id: i64,
        delivery_id: &str,
        text: &str,
        attachments_json: &str,
        gate_ref_json: Option<&str>,
        now: i64,
        origin: &str,
    ) -> Result<ManagedDelivery, StoreError> {
        validate_delivery_id(delivery_id)?;
        if text.len() > MAX_JSON_BYTES {
            return Err(invalid("payload_too_large"));
        }
        let attachments = canonical_json(attachments_json)?;
        if !serde_json::from_str::<serde_json::Value>(&attachments).is_ok_and(|v| v.is_array()) {
            return Err(invalid("invalid_attachments"));
        }
        let gate = gate_ref_json.map(canonical_json).transpose()?;
        let tx = ImmediateTx::begin(&self.conn)?;
        if self.managed_state(session_id)?.is_none() {
            return Err(invalid("session_missing"));
        }
        let existing = self
            .conn
            .query_row(
                &format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE delivery_id=?1"),
                [delivery_id],
                delivery_row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.session_id != session_id
                || existing.origin != origin
                || existing.text != text
                || existing.attachments_json != attachments
                || existing.gate_ref_json != gate
            {
                return Err(invalid("delivery_id_conflict"));
            }
            tx.commit()?;
            return Ok(existing);
        }
        self.conn.execute("INSERT INTO managed_deliveries(session_id,delivery_id,ordinal,text,attachments_json,gate_ref_json,state,created_at,updated_at,origin) SELECT ?1,?2,COALESCE(MAX(ordinal),0)+1,?3,?4,?5,'queued',?6,?6,?7 FROM managed_deliveries WHERE session_id=?1",params![session_id,delivery_id,text,attachments,gate,now,origin])?;
        let result = self
            .managed_delivery(session_id, delivery_id)?
            .ok_or_else(|| invalid("delivery_missing"))?;
        tx.commit()?;
        Ok(result)
    }

    pub fn managed_delivery(
        &self,
        session_id: i64,
        delivery_id: &str,
    ) -> Result<Option<ManagedDelivery>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 AND delivery_id=?2"),params![session_id,delivery_id],delivery_row).optional()?)
    }

    pub fn managed_delivery_by_native(
        &self,
        session_id: i64,
        native_id: &str,
    ) -> Result<Option<ManagedDelivery>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 AND native_message_id=?2"),params![session_id,native_id],delivery_row).optional()?)
    }

    pub fn managed_latest_user_delivery(
        &self,
        session_id: i64,
    ) -> Result<Option<ManagedDelivery>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 AND origin='user' AND gate_ref_json IS NULL ORDER BY ordinal DESC LIMIT 1"),[session_id],delivery_row).optional()?)
    }

    pub fn managed_deliveries(&self, session_id: i64) -> Result<Vec<ManagedDelivery>, StoreError> {
        let mut stmt = self.conn.prepare(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 ORDER BY ordinal DESC LIMIT ?2"))?;
        let rows = stmt
            .query_map(params![session_id, MANAGED_PAGE_LIMIT as i64], delivery_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn managed_pending_deliveries(
        &self,
        session_id: i64,
        limit: usize,
    ) -> Result<Vec<ManagedDelivery>, StoreError> {
        let mut stmt=self.conn.prepare(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 AND state NOT IN ('completed','cancelled') ORDER BY ordinal LIMIT ?2"))?;
        let rows = stmt
            .query_map(params![session_id, limit.min(40) as i64], delivery_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn managed_queued_count(&self, session_id: i64) -> Result<usize, StoreError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM managed_deliveries WHERE session_id=?1 AND state='queued'",
            [session_id],
            |r| r.get(0),
        )?;
        usize::try_from(count).map_err(|_| invalid("count_overflow"))
    }

    pub fn managed_next_queued(
        &self,
        session_id: i64,
    ) -> Result<Option<ManagedDelivery>, StoreError> {
        Ok(self.conn.query_row(&format!("SELECT {DELIVERY_COLUMNS} FROM managed_deliveries WHERE session_id=?1 AND state='queued' AND NOT EXISTS(SELECT 1 FROM managed_deliveries WHERE session_id=?1 AND state IN('sending','accepted','consumed','uncertain')) ORDER BY ordinal LIMIT 1"),[session_id],delivery_row).optional()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn managed_set_delivery_state(
        &self,
        session_id: i64,
        delivery_id: &str,
        expected: &str,
        next: &str,
        native_message_id: Option<&str>,
        error: Option<&str>,
        now: i64,
    ) -> Result<bool, StoreError> {
        if !valid_delivery(expected, next) {
            return Err(invalid("invalid_delivery_transition"));
        }
        if let Some(native) = native_message_id {
            nonempty(native, 4096)?;
        }
        let tx = ImmediateTx::begin(&self.conn)?;
        if next == "sending" {
            let occupied: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM managed_deliveries WHERE session_id=?1 AND delivery_id<>?2 AND state IN('sending','accepted','consumed','uncertain'))",params![session_id,delivery_id],|r|r.get(0))?;
            if occupied {
                tx.commit()?;
                return Ok(false);
            }
        }
        let changed = self.conn.execute("UPDATE managed_deliveries SET state=?1,native_message_id=COALESCE(?2,native_message_id),last_error=?3,updated_at=?4,accepted_at=CASE WHEN ?1='accepted' THEN COALESCE(accepted_at,?4) ELSE accepted_at END,consumed_at=CASE WHEN ?1='consumed' THEN COALESCE(consumed_at,?4) ELSE consumed_at END,completed_at=CASE WHEN ?1='completed' THEN COALESCE(completed_at,?4) ELSE completed_at END WHERE session_id=?5 AND delivery_id=?6 AND state=?7 AND updated_at<=?4 AND (native_message_id IS NULL OR ?2 IS NULL OR native_message_id=?2)",params![next,native_message_id,error,now,session_id,delivery_id,expected])? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn managed_append_event(
        &self,
        session_id: i64,
        payload_json: &str,
        now: i64,
    ) -> Result<i64, StoreError> {
        self.managed_record_event(session_id, payload_json, None, now)
    }

    pub fn managed_events_since(
        &self,
        session_id: i64,
        after: i64,
        limit: usize,
    ) -> Result<Vec<ManagedEvent>, StoreError> {
        // 大History原文已单独持久化/分页，普通事件轮询只发reset标记，不能一次IPC搬整条16MiB快照。
        let mut stmt = self.conn.prepare("SELECT e.session_id,e.sequence,CASE WHEN EXISTS(SELECT 1 FROM managed_native_recoveries r WHERE r.session_id=e.session_id AND r.sequence=e.sequence) THEN '{\"kind\":\"history\",\"messages\":[]}' ELSE e.payload_json END,e.created_at FROM managed_events e WHERE e.session_id=?1 AND e.sequence>?2 ORDER BY e.sequence LIMIT ?3")?;
        let rows = stmt
            .query_map(
                params![
                    session_id,
                    after.max(0),
                    limit.min(MANAGED_PAGE_LIMIT) as i64
                ],
                |r| {
                    Ok(ManagedEvent {
                        session_id: r.get(0)?,
                        sequence: r.get(1)?,
                        payload_json: r.get(2)?,
                        created_at: r.get(3)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 宿主已证明该 owner 的 engine 退出后才可调用；未知 PID 的遗留 slot 不得自动抢占。
    pub fn managed_recover_owner(
        &self,
        session_id: i64,
        expected_owner: &str,
        now: i64,
    ) -> Result<bool, StoreError> {
        let tx = ImmediateTx::begin(&self.conn)?;
        if !self.managed_release_runtime(session_id, expected_owner, now)? {
            tx.commit()?;
            return Ok(false);
        }
        self.recover_managed_session(session_id, now)?;
        tx.commit()?;
        Ok(true)
    }

    pub fn managed_recover_interrupted(&self, now: i64) -> Result<usize, StoreError> {
        let tx = ImmediateTx::begin(&self.conn)?;
        let ids = {
            let mut stmt = self.conn.prepare("SELECT session_id FROM managed_conversations WHERE runtime_owner IS NULL AND (phase NOT IN('stopped','failed') OR EXISTS(SELECT 1 FROM managed_deliveries d WHERE d.session_id=managed_conversations.session_id AND d.state IN('sending','accepted','consumed')))")?;
            let values = stmt
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            values
        };
        for id in &ids {
            self.recover_managed_session(*id, now)?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    fn recover_managed_session(&self, session_id: i64, now: i64) -> Result<(), StoreError> {
        // 收据时间保留：已消费后崩溃也不能伪装成从未消费并自动执行第二次。
        self.conn.execute("UPDATE managed_deliveries SET state='uncertain',last_error='conversation/recovery_uncertain',updated_at=MAX(updated_at,?1) WHERE session_id=?2 AND state IN('sending','accepted','consumed')",params![now,session_id])?;
        self.conn.execute("UPDATE managed_conversations SET phase='failed',last_error='conversation/recovery_required',updated_at=MAX(updated_at,?1) WHERE session_id=?2",params![now,session_id])?;
        self.conn.execute("UPDATE sessions SET status='ended',pid=NULL,ended_at=?1,pending_review=NULL WHERE id=?2",params![now,session_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn create(store: &Store) -> i64 {
        store
            .managed_create("codex", "C:/isolated", Some("test-profile"), 100)
            .unwrap()
    }

    fn enqueue(store: &Store, id: i64, delivery: &str) -> ManagedDelivery {
        store
            .managed_enqueue(id, delivery, "test only", "[]", None, 100)
            .unwrap()
    }

    struct Database(std::path::PathBuf);
    impl Database {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(
                std::env::temp_dir()
                    .join(format!("meowo-managed-{}-{nonce}.db", std::process::id())),
            )
        }
    }
    impl Drop for Database {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
            }
        }
    }

    #[test]
    fn native_bind_preserves_main_key_and_rejects_other_or_child_sessions() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        let old = store.get_session(id).unwrap().cc_session_id;
        let task = store.task_id_of_session_pub(id).unwrap();
        assert!(old.starts_with("meowo-managed-"));
        let bound = store.managed_bind(id, "native-root", 110).unwrap();
        assert_eq!(bound.native_session_id.as_deref(), Some("native-root"));
        assert_eq!(store.find_session_id_pub(&old).unwrap(), None);
        assert_eq!(store.find_session_id_pub("native-root").unwrap(), Some(id));
        assert_eq!(store.task_id_of_session_pub(id).unwrap(), task);
        assert_eq!(store.managed_bind(id, "native-root", 120).unwrap(), bound);
        assert!(store.managed_bind(id, "child-root", 120).is_err());
        let other = create(&store);
        assert!(store.managed_bind(other, "native-root", 120).is_err());
        assert!(store
            .managed_state(other)
            .unwrap()
            .unwrap()
            .native_session_id
            .is_none());
    }

    #[test]
    fn explicit_adoption_only_registers_the_existing_session() {
        let store = Store::open_in_memory().unwrap();
        let project = store.upsert_project_by_root("C:/old", "old", 1).unwrap();
        let (id, task) = store.start_session(project, "existing-native", 1).unwrap();
        store.set_session_provider(id, "gemini").unwrap();
        let before = store.get_session(id).unwrap();
        let adopted = store.managed_adopt(id, 10).unwrap();
        assert!(adopted.adopted);
        assert_eq!(adopted.cwd, "C:/old");
        assert_eq!(adopted.phase, "stopped");
        assert_eq!(
            adopted.native_session_id.as_deref(),
            Some("existing-native")
        );
        assert_eq!(store.get_session(id).unwrap().status, before.status);
        assert_eq!(store.task_id_of_session_pub(id).unwrap(), task);
        assert_eq!(store.managed_adopt(id, 20).unwrap(), adopted);
    }

    #[test]
    fn stable_delivery_id_is_payload_and_owner_scoped_and_does_not_reset_receipts() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        let original = store
            .managed_enqueue(
                id,
                "stable",
                "answer",
                "[ {\"path\":\"x\"} ]",
                Some("{\"gate\":\"g\"}"),
                100,
            )
            .unwrap();
        assert_eq!(
            store
                .managed_enqueue(
                    id,
                    "stable",
                    "answer",
                    "[{\"path\":\"x\"}]",
                    Some("{\"gate\": \"g\"}"),
                    101
                )
                .unwrap(),
            original
        );
        assert!(store
            .managed_set_delivery_state(id, "stable", "queued", "sending", None, None, 101)
            .unwrap());
        assert!(store
            .managed_set_delivery_state(
                id,
                "stable",
                "sending",
                "accepted",
                Some("native-message"),
                None,
                102
            )
            .unwrap());
        let replay = store
            .managed_enqueue(
                id,
                "stable",
                "answer",
                "[{\"path\":\"x\"}]",
                Some("{\"gate\":\"g\"}"),
                200,
            )
            .unwrap();
        assert_eq!(replay.state, "accepted");
        assert_eq!(replay.accepted_at, Some(102));
        assert_eq!(replay.consumed_at, None);
        assert!(store
            .managed_enqueue(id, "stable", "changed", "[]", None, 200)
            .is_err());
        let other = create(&store);
        assert!(store
            .managed_enqueue(
                other,
                "stable",
                "answer",
                "[{\"path\":\"x\"}]",
                Some("{\"gate\":\"g\"}"),
                200
            )
            .is_err());
        assert!(store.managed_delivery(other, "stable").unwrap().is_none());
    }

    #[test]
    fn stale_cas_cannot_reverse_phase_and_stopping_cannot_become_ready() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        assert!(store
            .managed_set_phase(id, "starting", "ready", None, 110)
            .unwrap());
        assert!(!store
            .managed_set_phase(id, "starting", "running", None, 120)
            .unwrap());
        assert!(!store
            .managed_set_phase(id, "ready", "running", None, 109)
            .unwrap());
        assert!(store
            .managed_set_phase(id, "ready", "running", None, 120)
            .unwrap());
        assert!(store
            .managed_set_phase(id, "running", "stopping", None, 130)
            .unwrap());
        assert!(store
            .managed_set_phase(id, "stopping", "ready", None, 140)
            .is_err());
        assert!(store
            .managed_set_phase(id, "stopping", "stopped", None, 140)
            .unwrap());
        assert_eq!(store.get_session(id).unwrap().status, "ended");
    }

    #[test]
    fn delivery_claim_is_serial_and_native_receipt_cannot_move_to_another_message() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        enqueue(&store, id, "first");
        enqueue(&store, id, "second");
        assert!(store
            .managed_set_delivery_state(id, "first", "queued", "sending", None, None, 110)
            .unwrap());
        assert!(!store
            .managed_set_delivery_state(id, "second", "queued", "sending", None, None, 110)
            .unwrap());
        assert!(!store
            .managed_set_delivery_state(id, "first", "queued", "sending", None, None, 110)
            .unwrap());
        assert!(store
            .managed_set_delivery_state(
                id,
                "first",
                "sending",
                "accepted",
                Some("native-one"),
                None,
                120
            )
            .unwrap());
        assert!(!store
            .managed_set_delivery_state(
                id,
                "first",
                "accepted",
                "consumed",
                Some("wrong-native"),
                None,
                130
            )
            .unwrap());
        assert!(store
            .managed_set_delivery_state(
                id,
                "first",
                "accepted",
                "consumed",
                Some("native-one"),
                None,
                130
            )
            .unwrap());
        assert!(store
            .managed_set_delivery_state(id, "first", "consumed", "completed", None, None, 140)
            .unwrap());
        assert_eq!(
            store.managed_next_queued(id).unwrap().unwrap().delivery_id,
            "second"
        );
    }

    #[test]
    fn crash_marks_inflight_uncertain_keeps_consumption_and_never_requeues() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        enqueue(&store, id, "first");
        enqueue(&store, id, "later");
        store
            .managed_set_delivery_state(id, "first", "queued", "sending", None, None, 110)
            .unwrap();
        store
            .managed_set_delivery_state(id, "first", "sending", "consumed", Some("m"), None, 120)
            .unwrap();
        assert_eq!(store.managed_recover_interrupted(130).unwrap(), 1);
        let receipt = store.managed_delivery(id, "first").unwrap().unwrap();
        assert_eq!(receipt.state, "uncertain");
        assert_eq!(receipt.consumed_at, Some(120));
        assert_eq!(
            store.managed_delivery(id, "later").unwrap().unwrap().state,
            "queued"
        );
        assert!(store.managed_next_queued(id).unwrap().is_none());
        assert!(store
            .managed_set_delivery_state(id, "first", "uncertain", "queued", None, None, 140)
            .is_err());
        assert_eq!(store.managed_recover_interrupted(140).unwrap(), 0);
    }

    #[test]
    fn owner_slot_survives_other_host_recovery_and_requires_exact_release_token() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        assert!(store.managed_acquire_runtime(id, "owner-a", 101).unwrap());
        assert!(!store.managed_acquire_runtime(id, "owner-b", 102).unwrap());
        assert!(!store.managed_acquire_runtime(id, "owner-a", 102).unwrap());
        assert!(store
            .managed_set_runtime_pid(id, "owner-a", 12345, 103)
            .unwrap());
        assert!(!store
            .managed_set_runtime_pid(id, "owner-b", 54321, 103)
            .unwrap());
        assert_eq!(store.managed_recover_interrupted(200).unwrap(), 0);
        assert!(!store.managed_recover_owner(id, "owner-b", 201).unwrap());
        assert_eq!(
            store.managed_state(id).unwrap().unwrap().runtime_pid,
            Some(12345)
        );
        assert!(store.managed_recover_owner(id, "owner-a", 202).unwrap());
        assert_eq!(store.managed_state(id).unwrap().unwrap().phase, "failed");
        assert!(store.managed_acquire_runtime(id, "owner-b", 203).unwrap());
    }

    #[test]
    fn ui_delivery_window_does_not_drop_older_queued_work() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        for index in 0..250 {
            enqueue(&store, id, &format!("d-{index}"));
        }
        let page = store.managed_deliveries(id).unwrap();
        assert_eq!(page.len(), MANAGED_PAGE_LIMIT);
        assert_eq!(page[0].delivery_id, "d-249");
        assert_eq!(store.managed_queued_count(id).unwrap(), 250);
        assert_eq!(
            store.managed_next_queued(id).unwrap().unwrap().delivery_id,
            "d-0"
        );
    }

    #[test]
    fn event_pagination_is_bounded_monotonic_and_session_scoped() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        let other = create(&store);
        for seq in 1..=207 {
            assert_eq!(
                store
                    .managed_append_event(id, "{\"kind\":\"output\"}", 100)
                    .unwrap(),
                seq
            );
        }
        assert_eq!(
            store
                .managed_append_event(other, "{\"kind\":\"private\"}", 100)
                .unwrap(),
            1
        );
        let first = store.managed_events_since(id, 0, usize::MAX).unwrap();
        assert_eq!(first.len(), 200);
        assert_eq!(first[0].sequence, 1);
        let tail = store.managed_events_since(id, 200, 200).unwrap();
        assert_eq!(tail.len(), 7);
        assert!(tail.iter().all(|e| e.session_id == id));
        assert!(store.managed_events_since(id, 0, 0).unwrap().is_empty());
        assert!(store
            .managed_events_since(id, 0, 200)
            .unwrap()
            .iter()
            .all(|e| !e.payload_json.contains("private")));
        assert!(store.managed_append_event(id, "invalid", 100).is_err());
        assert!(store
            .managed_append_event(id, &" ".repeat(MAX_JSON_BYTES + 1), 100)
            .is_err());
        assert_eq!(store.managed_append_event(id, "{}", 100).unwrap(), 208);
    }

    #[test]
    fn two_connections_cannot_acquire_the_same_runtime_or_duplicate_event_sequence() {
        let db = Database::new();
        let store = Store::open(&db.0).unwrap();
        let id = create(&store);
        let barrier = Arc::new(Barrier::new(2));
        let jobs: Vec<_> = (0..2)
            .map(|index| {
                let path = db.0.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let store = Store::open(path).unwrap();
                    barrier.wait();
                    let won = store
                        .managed_acquire_runtime(id, &format!("engine-{index}"), 110)
                        .unwrap();
                    for _ in 0..20 {
                        store.managed_append_event(id, "{}", 110).unwrap();
                    }
                    won
                })
            })
            .collect();
        assert_eq!(
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        let events = store.managed_events_since(id, 0, 200).unwrap();
        assert_eq!(
            events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            (1..=40).collect::<Vec<_>>()
        );
    }

    #[test]
    fn v20_database_migrates_without_changing_ordinary_sessions() {
        let db = Database::new();
        let id = {
            let store = Store::open(&db.0).unwrap();
            let project = store
                .upsert_project_by_root("C:/legacy", "legacy", 1)
                .unwrap();
            let (id, _) = store.start_session(project, "ordinary", 1).unwrap();
            store.conn.execute_batch("DROP TABLE managed_events; DROP TABLE managed_deliveries; DROP TABLE managed_conversations; PRAGMA user_version=20;").unwrap();
            id
        };
        let store = Store::open(&db.0).unwrap();
        assert_eq!(store.user_version().unwrap(), Store::CURRENT_USER_VERSION);
        assert_eq!(store.get_session(id).unwrap().cc_session_id, "ordinary");
        assert!(store.managed_state(id).unwrap().is_none());
        assert!(store.managed_create("codex", "C:/new", None, 100).is_ok());
    }

    #[test]
    fn concurrent_human_retry_has_one_durable_replacement_and_keeps_native_receipts() {
        let db = Database::new();
        let store = Store::open(&db.0).unwrap();
        let id = create(&store);
        store
            .managed_enqueue(id, "old", "original", "[]", None, 100)
            .unwrap();
        store
            .managed_set_delivery_state(id, "old", "queued", "sending", None, None, 101)
            .unwrap();
        store
            .managed_set_delivery_state(
                id,
                "old",
                "sending",
                "accepted",
                Some("native-old"),
                None,
                102,
            )
            .unwrap();
        store
            .managed_set_delivery_state(id, "old", "accepted", "uncertain", None, None, 103)
            .unwrap();
        store
            .managed_acquire_runtime(id, "live-engine", 103)
            .unwrap();
        assert!(store
            .managed_delivery_action(id, "old", "retry", Some("new"), 104)
            .is_err());
        assert!(store.managed_delivery(id, "new").unwrap().is_none());
        store
            .managed_release_runtime(id, "live-engine", 104)
            .unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let jobs: Vec<_> = (0..2)
            .map(|_| {
                let path = db.0.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let store = Store::open(path).unwrap();
                    barrier.wait();
                    store
                        .managed_delivery_action(id, "old", "retry", Some("new"), 105)
                        .unwrap()
                })
            })
            .collect();
        for job in jobs {
            assert_eq!(job.join().unwrap().delivery_id, "new");
        }
        assert_eq!(store.managed_queued_count(id).unwrap(), 1);
        let old = store.managed_delivery(id, "old").unwrap().unwrap();
        assert_eq!(old.state, "cancelled");
        assert_eq!(old.native_message_id.as_deref(), Some("native-old"));
        assert_eq!(old.accepted_at, Some(102));
        assert!(store
            .managed_delivery_action(id, "old", "retry", Some("another"), 106)
            .is_err());
        assert!(store
            .managed_delivery_action(id, "old", "dismiss", None, 106)
            .is_err());
    }

    #[test]
    fn v22_upgrade_adds_the_disposition_table_without_requeueing_uncertain() {
        let db = Database::new();
        let id = {
            let store = Store::open(&db.0).unwrap();
            let id = create(&store);
            store
                .managed_enqueue(id, "old", "original", "[]", None, 100)
                .unwrap();
            store
                .managed_set_delivery_state(id, "old", "queued", "sending", None, None, 101)
                .unwrap();
            store
                .managed_set_delivery_state(id, "old", "sending", "uncertain", None, None, 102)
                .unwrap();
            store
                .conn
                .execute_batch("DROP TABLE managed_delivery_dispositions; PRAGMA user_version=22;")
                .unwrap();
            id
        };
        let store = Store::open(&db.0).unwrap();
        assert_eq!(store.user_version().unwrap(), Store::CURRENT_USER_VERSION);
        assert_eq!(
            store.managed_delivery(id, "old").unwrap().unwrap().state,
            "uncertain"
        );
        assert_eq!(
            store
                .managed_delivery_action(id, "old", "dismiss", None, 103)
                .unwrap()
                .state,
            "cancelled"
        );
    }

    #[test]
    fn latest_human_delivery_is_not_hidden_by_more_than_a_ui_page_of_gate_deliveries() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        store
            .managed_enqueue(id, "human-stop", "pause here", "[]", None, 100)
            .unwrap();
        for index in 0..205 {
            store
                .managed_enqueue(
                    id,
                    &format!("gate-{index}"),
                    "gate continuation",
                    "[]",
                    Some("{\"taskDir\":\"C:/task\",\"gateId\":\"g\"}"),
                    101 + index,
                )
                .unwrap();
        }
        assert!(store
            .managed_deliveries(id)
            .unwrap()
            .iter()
            .all(|item| item.gate_ref_json.is_some()));
        assert_eq!(
            store
                .managed_latest_user_delivery(id)
                .unwrap()
                .unwrap()
                .delivery_id,
            "human-stop"
        );
        assert!(store
            .managed_latest_user_delivery(id + 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn internal_origin_cannot_clear_human_pause_or_reuse_a_human_delivery_id() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        store
            .managed_enqueue(id, "human", "pause", "[]", None, 100)
            .unwrap();
        let lead = store
            .managed_enqueue_lead(id, "auto", "continue internally", "[]", None, 101)
            .unwrap();
        assert_eq!(lead.origin, "lead");
        assert_eq!(
            store
                .managed_latest_user_delivery(id)
                .unwrap()
                .unwrap()
                .delivery_id,
            "human"
        );
        assert!(store
            .managed_enqueue_lead(id, "human", "pause", "[]", None, 102)
            .is_err());
        assert!(store
            .managed_enqueue(id, "auto", "continue internally", "[]", None, 102)
            .is_err());
        let retry = store
            .managed_delivery_action(id, "auto", "retry", Some("auto-new"), 103)
            .unwrap();
        assert_eq!(retry.origin, "lead");
        assert_eq!(
            store
                .managed_latest_user_delivery(id)
                .unwrap()
                .unwrap()
                .delivery_id,
            "human"
        );
        assert_eq!(
            store
                .managed_enqueue_lead(id, "auto", "continue internally", "[]", None, 104)
                .unwrap()
                .state,
            "cancelled"
        );
    }

    #[test]
    fn v23_upgrade_preserves_old_deliveries_as_user_origin() {
        let db = Database::new();
        let id = {
            let store = Store::open(&db.0).unwrap();
            let id = create(&store);
            store
                .managed_enqueue(id, "human", "old payload", "[]", None, 100)
                .unwrap();
            store.conn.execute_batch("DROP INDEX ix_managed_human_origin; ALTER TABLE managed_deliveries DROP COLUMN origin; PRAGMA user_version=23;").unwrap();
            id
        };
        let store = Store::open(&db.0).unwrap();
        assert_eq!(store.user_version().unwrap(), Store::CURRENT_USER_VERSION);
        assert_eq!(
            store.managed_delivery(id, "human").unwrap().unwrap().origin,
            "user"
        );
        assert_eq!(
            store
                .managed_latest_user_delivery(id)
                .unwrap()
                .unwrap()
                .delivery_id,
            "human"
        );
    }

    #[test]
    fn invalid_wire_marker_ids_are_rejected_before_any_enqueue_or_retry_side_effect() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        for invalid in [
            "",
            "with space",
            "with:separator",
            "中文",
            "x\n",
            &"x".repeat(161),
        ] {
            assert!(store
                .managed_enqueue(id, invalid, "body", "[]", None, 100)
                .is_err());
        }
        assert_eq!(store.managed_queued_count(id).unwrap(), 0);
        store
            .managed_enqueue(id, "old-valid_id", "body", "[]", None, 100)
            .unwrap();
        assert!(store
            .managed_delivery_action(
                id,
                "old-valid_id",
                "retry",
                Some("invalid:replacement"),
                101
            )
            .is_err());
        assert_eq!(
            store
                .managed_delivery(id, "old-valid_id")
                .unwrap()
                .unwrap()
                .state,
            "queued"
        );
        assert!(validate_delivery_id(&"x".repeat(160)).is_ok());
    }

    #[test]
    fn bounded_pending_window_keeps_the_uncertain_head_visible_before_hundreds_of_queued_messages()
    {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        store
            .managed_enqueue(id, "old-head", "first", "[]", None, 100)
            .unwrap();
        store
            .managed_set_delivery_state(id, "old-head", "queued", "sending", None, None, 101)
            .unwrap();
        store
            .managed_set_delivery_state(id, "old-head", "sending", "uncertain", None, None, 102)
            .unwrap();
        for index in 0..250 {
            store
                .managed_enqueue(id, &format!("q-{index}"), "later", "[]", None, 103 + index)
                .unwrap();
        }
        assert!(store
            .managed_deliveries(id)
            .unwrap()
            .iter()
            .all(|delivery| delivery.delivery_id != "old-head"));
        let pending = store.managed_pending_deliveries(id, 40).unwrap();
        assert_eq!(pending.len(), 40);
        assert_eq!(pending[0].delivery_id, "old-head");
        assert!(pending
            .windows(2)
            .all(|pair| pair[0].ordinal < pair[1].ordinal));
    }
}
