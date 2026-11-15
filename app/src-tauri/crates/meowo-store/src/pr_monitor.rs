use crate::{Store, StoreError};
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrMonitor {
    pub key: String,
    pub cwd: String,
    pub session_id: i64,
    pub slug: String,
    pub number: u32,
    pub enabled: bool,
    pub seeded: bool,
    pub checks_state: Option<String>,
    pub mergeable: Option<String>,
    pub head_oid: Option<String>,
    pub known_comments: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrMonitorEvent {
    pub id: String,
    pub monitor_key: String,
    pub kind: String,
    pub summary: String,
    pub created_at: i64,
    pub wake_state: String,
}

pub struct PrMonitorInput<'a> {
    pub key: &'a str,
    pub cwd: &'a str,
    pub session_id: i64,
    pub slug: &'a str,
    pub number: u32,
    pub enabled: bool,
}

pub struct PrPollInput<'a> {
    pub key: &'a str,
    pub checks: &'a str,
    pub mergeable: &'a str,
    pub head: &'a str,
    pub known: &'a str,
    pub error: Option<&'a str>,
}

impl Store {
    pub fn upsert_pr_monitor(&self, input: PrMonitorInput<'_>, now_ms: i64) -> Result<(), StoreError> {
        let PrMonitorInput { key, cwd, session_id, slug, number, enabled } = input;
        self.conn.execute(
            "INSERT INTO pr_monitors(key, cwd, session_id, slug, number, enabled, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(key) DO UPDATE SET cwd = excluded.cwd, session_id = excluded.session_id, enabled = excluded.enabled, last_error = NULL, updated_at = excluded.updated_at",
            rusqlite::params![key, cwd, session_id, slug, number, enabled, now_ms],
        )?;
        Ok(())
    }

    fn monitor_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PrMonitor> {
        Ok(PrMonitor {
            key: row.get(0)?, cwd: row.get(1)?, session_id: row.get(2)?, slug: row.get(3)?, number: row.get(4)?,
            enabled: row.get::<_, i64>(5)? != 0, seeded: row.get::<_, i64>(6)? != 0,
            checks_state: row.get(7)?, mergeable: row.get(8)?, head_oid: row.get(9)?, known_comments: row.get(10)?, last_error: row.get(11)?,
        })
    }

    pub fn set_pr_monitor_enabled(&self, key: &str, session_id: i64, enabled: bool, now_ms: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE pr_monitors SET enabled = ?3, last_error = NULL, updated_at = ?4 WHERE key = ?1 AND session_id = ?2", rusqlite::params![key, session_id, enabled, now_ms])? == 1)
    }

    pub fn pr_monitor(&self, key: &str) -> Result<Option<PrMonitor>, StoreError> {
        self.conn.query_row("SELECT key, cwd, session_id, slug, number, enabled, seeded, checks_state, mergeable, head_oid, known_comments, last_error FROM pr_monitors WHERE key = ?1", [key], Self::monitor_row).optional().map_err(Into::into)
    }

    pub fn pr_monitor_for(&self, cwd: &str, number: u32) -> Result<Option<PrMonitor>, StoreError> {
        self.conn.query_row("SELECT key, cwd, session_id, slug, number, enabled, seeded, checks_state, mergeable, head_oid, known_comments, last_error FROM pr_monitors WHERE cwd = ?1 AND number = ?2", rusqlite::params![cwd, number], Self::monitor_row).optional().map_err(Into::into)
    }

    pub fn enabled_pr_monitors(&self) -> Result<Vec<PrMonitor>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT key, cwd, session_id, slug, number, enabled, seeded, checks_state, mergeable, head_oid, known_comments, last_error FROM pr_monitors WHERE enabled = 1 ORDER BY key")?;
        let rows = stmt.query_map([], Self::monitor_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn save_pr_poll(&self, input: PrPollInput<'_>, now_ms: i64) -> Result<(), StoreError> {
        let PrPollInput { key, checks, mergeable, head, known, error } = input;
        self.conn.execute("UPDATE pr_monitors SET seeded = 1, checks_state = ?2, mergeable = ?3, head_oid = ?4, known_comments = ?5, last_error = ?6, updated_at = ?7 WHERE key = ?1", rusqlite::params![key, checks, mergeable, head, known, error, now_ms])?;
        Ok(())
    }

    pub fn pr_monitor_error(&self, key: &str, error: &str, now_ms: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE pr_monitors SET last_error = ?2, updated_at = ?3 WHERE key = ?1", rusqlite::params![key, error, now_ms])?;
        Ok(())
    }

    pub fn add_pr_event(&self, monitor_key: &str, id: &str, kind: &str, summary: &str, now_ms: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("INSERT OR IGNORE INTO pr_monitor_events(id, monitor_key, kind, summary, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", rusqlite::params![id, monitor_key, kind, summary, now_ms])? == 1)
    }

    pub fn pr_events(&self, monitor_key: &str) -> Result<Vec<PrMonitorEvent>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id, monitor_key, kind, summary, created_at, wake_state FROM pr_monitor_events WHERE monitor_key = ?1 ORDER BY created_at DESC, id DESC LIMIT 200")?;
        let rows = stmt.query_map([monitor_key], |row| Ok(PrMonitorEvent { id: row.get(0)?, monitor_key: row.get(1)?, kind: row.get(2)?, summary: row.get(3)?, created_at: row.get(4)?, wake_state: row.get(5)? }))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn pr_event_for_session(&self, session_id: i64, monitor_key: &str, id: &str) -> Result<Option<PrMonitorEvent>, StoreError> {
        self.conn.query_row("SELECT e.id, e.monitor_key, e.kind, e.summary, e.created_at, e.wake_state FROM pr_monitor_events e JOIN pr_monitors m ON m.key = e.monitor_key WHERE m.session_id = ?1 AND e.monitor_key = ?2 AND e.id = ?3", rusqlite::params![session_id, monitor_key, id], |row| Ok(PrMonitorEvent { id: row.get(0)?, monitor_key: row.get(1)?, kind: row.get(2)?, summary: row.get(3)?, created_at: row.get(4)?, wake_state: row.get(5)? })).optional().map_err(Into::into)
    }

    pub fn mark_pr_event_wake(&self, session_id: i64, monitor_key: &str, id: &str, expected: &str, state: &str) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE pr_monitor_events SET wake_state = ?5 WHERE monitor_key = ?2 AND id = ?3 AND wake_state = ?4 AND monitor_key IN (SELECT key FROM pr_monitors WHERE session_id = ?1)", rusqlite::params![session_id, monitor_key, id, expected, state])? == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_and_wake_state_survive_reopen_without_duplicate_events() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-pr-monitor-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("board.db");
        {
            let store = Store::open(&db).unwrap();
            store.upsert_pr_monitor(PrMonitorInput { key: "repo:1", cwd: "C:/repo", session_id: 7, slug: "a/b", number: 1, enabled: true }, 100).unwrap();
            assert!(store.add_pr_event("repo:1", "comment:1", "comment", "new comment", 101).unwrap());
            assert!(!store.add_pr_event("repo:1", "comment:1", "comment", "new comment", 102).unwrap());
            assert!(store.mark_pr_event_wake(7, "repo:1", "comment:1", "pending", "queued").unwrap());
        }
        let store = Store::open(&db).unwrap();
        assert_eq!(store.enabled_pr_monitors().unwrap().len(), 1);
        assert_eq!(store.pr_events("repo:1").unwrap()[0].wake_state, "queued");
        assert!(!store.mark_pr_event_wake(7, "repo:1", "comment:1", "pending", "delivered").unwrap());
        store.upsert_pr_monitor(PrMonitorInput { key: "repo:1", cwd: "C:/repo", session_id: 7, slug: "a/b", number: 1, enabled: false }, 200).unwrap();
        assert!(store.enabled_pr_monitors().unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_dir(dir);
    }
}


#[cfg(test)]
mod isolation_tests {
    use super::*;

    #[test]
    fn same_head_event_is_independent_per_pr_and_monitor_follows_new_worktree() {
        let store = Store::open_in_memory().unwrap();
        for (key, cwd) in [("a/one#1", "C:/a"), ("b/two#2", "C:/b")] {
            store.upsert_pr_monitor(PrMonitorInput { key, cwd, session_id: 7, slug: "a/b", number: 1, enabled: true }, 1).unwrap();
            store.add_pr_event(key, "checks:fail:same-sha", "checks_failed", "CI failed", 2).unwrap();
        }
        assert_eq!(store.pr_events("a/one#1").unwrap().len(), 1);
        assert_eq!(store.pr_events("b/two#2").unwrap().len(), 1);
        assert!(store.mark_pr_event_wake(7, "a/one#1", "checks:fail:same-sha", "pending", "queued").unwrap());
        assert_eq!(store.pr_events("b/two#2").unwrap()[0].wake_state, "pending");
        assert_eq!(store.pr_event_for_session(7, "b/two#2", "checks:fail:same-sha").unwrap().unwrap().wake_state, "pending");
        store.upsert_pr_monitor(PrMonitorInput { key: "a/one#1", cwd: "C:/new-worktree", session_id: 8, slug: "a/one", number: 1, enabled: true }, 3).unwrap();
        assert_eq!(store.pr_monitor("a/one#1").unwrap().unwrap().cwd, "C:/new-worktree");
        assert!(!store.set_pr_monitor_enabled("a/one#1", 7, false, 4).unwrap());
        assert!(store.pr_monitor("a/one#1").unwrap().unwrap().enabled);
        assert!(store.set_pr_monitor_enabled("a/one#1", 8, false, 5).unwrap());
        assert!(!store.pr_monitor("a/one#1").unwrap().unwrap().enabled);
    }
}


#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn v29_global_event_ids_migrate_to_per_monitor_keys() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-pr-v29-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("board.db");
        let store = Store::open(&db).unwrap();
        store.upsert_pr_monitor(PrMonitorInput { key: "a/b#1", cwd: "C:/a", session_id: 7, slug: "a/b", number: 1, enabled: true }, 1).unwrap();
        drop(store);
        {
            let conn = rusqlite::Connection::open(&db).unwrap();
            conn.execute_batch(
                "DROP TABLE pr_monitor_events;
                 CREATE TABLE pr_monitor_events (
                    id TEXT PRIMARY KEY,
                    monitor_key TEXT NOT NULL REFERENCES pr_monitors(key) ON DELETE CASCADE,
                    kind TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    wake_state TEXT NOT NULL DEFAULT 'pending'
                 );
                 INSERT INTO pr_monitor_events VALUES ('same-head', 'a/b#1', 'checks_failed', 'CI failed', 2, 'pending');
                 PRAGMA user_version = 29;",
            ).unwrap();
        }
        let store = Store::open(&db).unwrap();
        assert_eq!(store.user_version().unwrap(), Store::CURRENT_USER_VERSION);
        assert_eq!(store.pr_events("a/b#1").unwrap()[0].id, "same-head");
        drop(store);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_dir(dir);
    }
}
