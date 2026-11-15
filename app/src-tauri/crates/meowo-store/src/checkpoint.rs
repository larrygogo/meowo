use crate::{Store, StoreError};
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCheckpoint {
    pub session_id: i64,
    pub turn: i64,
    pub repo_root: String,
    pub start_ref: String,
    pub end_ref: Option<String>,
    pub error: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreCheckpoint {
    pub repo_root: String,
    pub snapshot_ref: String,
}

const SELECT: &str = "SELECT session_id, turn, repo_root, start_ref, end_ref, error, updated_at FROM turn_checkpoints";

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TurnCheckpoint> {
    Ok(TurnCheckpoint {
        session_id: row.get(0)?,
        turn: row.get(1)?,
        repo_root: row.get(2)?,
        start_ref: row.get(3)?,
        end_ref: row.get(4)?,
        error: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

impl Store {
    pub fn checkpoint_start(
        &self,
        session_id: i64,
        turn: i64,
        repo_root: &str,
        start_ref: &str,
        now_ms: i64,
    ) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "INSERT INTO turn_checkpoints(session_id, turn, repo_root, start_ref, updated_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(session_id, turn, repo_root) DO NOTHING",
            rusqlite::params![session_id, turn, repo_root, start_ref, now_ms],
        )?;
        Ok(changed == 1)
    }

    pub fn checkpoint_finish(
        &self,
        session_id: i64,
        turn: i64,
        repo_root: &str,
        end_ref: &str,
        now_ms: i64,
    ) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE turn_checkpoints SET end_ref = ?4, error = NULL, updated_at = ?5 WHERE session_id = ?1 AND turn = ?2 AND repo_root = ?3",
            rusqlite::params![session_id, turn, repo_root, end_ref, now_ms],
        )?;
        Ok(changed == 1)
    }

    pub fn checkpoint_error(
        &self,
        session_id: i64,
        turn: i64,
        repo_root: &str,
        error: &str,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO turn_checkpoints(session_id, turn, repo_root, start_ref, error, updated_at) VALUES (?1, ?2, ?3, '', ?4, ?5) ON CONFLICT(session_id, turn, repo_root) DO UPDATE SET error = excluded.error, updated_at = excluded.updated_at",
            rusqlite::params![session_id, turn, repo_root, error, now_ms],
        )?;
        Ok(())
    }

    pub fn turn_checkpoints(&self, session_id: i64) -> Result<Vec<TurnCheckpoint>, StoreError> {
        let mut stmt = self.conn.prepare(&format!("{SELECT} WHERE session_id = ?1 ORDER BY turn DESC, repo_root"))?;
        let rows = stmt.query_map([session_id], from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn turn_checkpoint_repos(&self, session_id: i64, turn: i64) -> Result<Vec<TurnCheckpoint>, StoreError> {
        let mut stmt = self.conn.prepare(&format!("{SELECT} WHERE session_id = ?1 AND turn = ?2 ORDER BY repo_root"))?;
        let rows = stmt.query_map([session_id, turn], from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn create_checkpoint_restore(&self, session_id: i64, snapshots: &[RestoreCheckpoint], now_ms: i64) -> Result<i64, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("INSERT INTO checkpoint_restores(session_id, created_at) VALUES (?1, ?2)", [session_id, now_ms])?;
        let id = tx.last_insert_rowid();
        for item in snapshots {
            tx.execute("INSERT INTO checkpoint_restore_repos(restore_id, repo_root, snapshot_ref) VALUES (?1, ?2, ?3)", rusqlite::params![id, item.repo_root, item.snapshot_ref])?;
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn checkpoint_restore(&self, session_id: i64, id: i64) -> Result<Option<Vec<RestoreCheckpoint>>, StoreError> {
        let exists = self.conn.query_row("SELECT 1 FROM checkpoint_restores WHERE id = ?1 AND session_id = ?2", [id, session_id], |_| Ok(())).optional()?;
        if exists.is_none() {
            return Ok(None);
        }
        let mut stmt = self.conn.prepare("SELECT repo_root, snapshot_ref FROM checkpoint_restore_repos WHERE restore_id = ?1 ORDER BY repo_root")?;
        let rows = stmt.query_map([id], |row| Ok(RestoreCheckpoint { repo_root: row.get(0)?, snapshot_ref: row.get(1)? }))?;
        Ok(Some(rows.collect::<Result<Vec<_>, _>>()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_metadata_survives_reopen_and_start_is_not_rewritten() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-checkpoint-store-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("board.db");
        {
            let store = Store::open(&db).unwrap();
            assert!(store.checkpoint_start(7, 1, "C:/repo", "refs/meowo/start", 100).unwrap());
            assert!(!store.checkpoint_start(7, 1, "C:/repo", "refs/meowo/other", 101).unwrap());
            store.checkpoint_finish(7, 1, "C:/repo", "refs/meowo/end", 102).unwrap();
            store.create_checkpoint_restore(7, &[RestoreCheckpoint { repo_root: "C:/repo".into(), snapshot_ref: "refs/meowo/end".into() }], 103).unwrap();
        }
        let store = Store::open(&db).unwrap();
        let rows = store.turn_checkpoints(7).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].start_ref, "refs/meowo/start");
        assert_eq!(rows[0].end_ref.as_deref(), Some("refs/meowo/end"));
        assert_eq!(store.checkpoint_restore(7, 1).unwrap().unwrap()[0].snapshot_ref, "refs/meowo/end");
        drop(store);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_dir(dir);
    }
}
