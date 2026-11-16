use crate::{Store, StoreError};
use rusqlite::OptionalExtension;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub id: i64,
    pub session_id: i64,
    pub draft_json: String,
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedPrompt> {
    Ok(QueuedPrompt {
        id: row.get(0)?,
        session_id: row.get(1)?,
        draft_json: row.get(2)?,
        state: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

const SELECT: &str = "SELECT id, session_id, draft_json, state, created_at, updated_at FROM prompt_queue";

impl Store {
    pub fn enqueue_prompt(&self, session_id: i64, draft_json: &str, now_ms: i64) -> Result<QueuedPrompt, StoreError> {
        self.conn.execute(
            "INSERT INTO prompt_queue(session_id, draft_json, state, created_at, updated_at) VALUES (?1, ?2, 'held', ?3, ?3)",
            rusqlite::params![session_id, draft_json, now_ms],
        )?;
        self.queued_prompt(session_id, self.conn.last_insert_rowid())?
            .ok_or_else(|| StoreError::InvalidInput("queue/not_found".into()))
    }

    pub fn queued_prompt(&self, session_id: i64, id: i64) -> Result<Option<QueuedPrompt>, StoreError> {
        Ok(self
            .conn
            .query_row(&format!("{SELECT} WHERE session_id = ?1 AND id = ?2"), [session_id, id], from_row)
            .optional()?)
    }

    pub fn orphaned_prompts(&self) -> Result<Vec<QueuedPrompt>, StoreError> {
        let mut stmt = self.conn.prepare(&format!("{SELECT} WHERE state = 'orphaned' ORDER BY id"))?;
        let rows = stmt.query_map([], from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn take_orphaned_prompt(&self, session_id: i64, id: i64) -> Result<Option<QueuedPrompt>, StoreError> {
        self.conn.query_row(
            "DELETE FROM prompt_queue WHERE session_id = ?1 AND id = ?2 AND state = 'orphaned' RETURNING id, session_id, draft_json, state, created_at, updated_at",
            [session_id, id],
            from_row,
        ).optional().map_err(Into::into)
    }

    pub fn queued_prompts(&self, session_id: i64) -> Result<Vec<QueuedPrompt>, StoreError> {
        let mut stmt = self.conn.prepare(&format!(
            "{SELECT} WHERE session_id = ?1 AND state IN ('held', 'delivering', 'failed') ORDER BY COALESCE(position, id), id"
        ))?;
        let rows = stmt.query_map([session_id], from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 拖拽排序：`ids` 是用户排好的新顺序，只能是这个会话里仍在等待（held）的消息。
    ///
    /// 不给整列重新编号，只把这几条原有的位置值按新顺序重新发给它们：没参与排序的（正在投递、
    /// 投递失败）留在原位，新入队的 id 比所有已有位置都大，照样排在最后。
    /// 有一条已经不是 held（另一端刚领走、取消了）就整批不动、返回 false，界面据此刷新列表。
    pub fn reorder_queued_prompts(&self, session_id: i64, ids: &[i64], now_ms: i64) -> Result<bool, StoreError> {
        if ids.is_empty() {
            return Ok(true);
        }
        // 读位置和改位置在同一个写事务里：两端同时拖时，后到的一方读到的是前一方排好的结果。
        let tx = crate::store::ImmediateTx::begin(&self.conn)?;
        let mut slots = Vec::with_capacity(ids.len());
        for id in ids {
            let slot: Option<i64> = self
                .conn
                .query_row(
                    "SELECT COALESCE(position, id) FROM prompt_queue WHERE session_id = ?1 AND id = ?2 AND state = 'held'",
                    [session_id, *id],
                    |row| row.get(0),
                )
                .optional()?;
            match slot {
                Some(slot) if !slots.contains(&slot) => slots.push(slot),
                _ => return Ok(false),
            }
        }
        slots.sort_unstable();
        for (id, slot) in ids.iter().zip(slots) {
            self.conn.execute(
                "UPDATE prompt_queue SET position = ?3, updated_at = ?4 WHERE session_id = ?1 AND id = ?2",
                rusqlite::params![session_id, id, slot, now_ms],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn edit_queued_prompt(&self, session_id: i64, id: i64, draft_json: &str, now_ms: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE prompt_queue SET draft_json = ?3, updated_at = ?4 WHERE session_id = ?1 AND id = ?2 AND state = 'held'",
            rusqlite::params![session_id, id, draft_json, now_ms],
        )?;
        Ok(changed == 1)
    }

    /// held/failed 可从列表清理；delivering 必须先结算，不能把可能在写的消息直接说成已取消。
    pub fn cancel_queued_prompt(&self, session_id: i64, id: i64, now_ms: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE prompt_queue SET state = 'cancelled', updated_at = ?3 WHERE session_id = ?1 AND id = ?2 AND state IN ('held', 'failed')",
            rusqlite::params![session_id, id, now_ms],
        )?;
        Ok(changed == 1)
    }

    /// 两个客户端可同时看到 held；只有完成这次状态迁移的一方可以写 CLI。
    pub fn claim_queued_prompt(&self, session_id: i64, id: i64, now_ms: i64) -> Result<Option<QueuedPrompt>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "UPDATE prompt_queue SET state = 'delivering', updated_at = ?3 WHERE session_id = ?1 AND id = ?2 AND state = 'held' RETURNING id, session_id, draft_json, state, created_at, updated_at",
                rusqlite::params![session_id, id, now_ms],
                from_row,
            )
            .optional()?)
    }

    pub fn finish_prompt_delivery(&self, session_id: i64, id: i64, delivered: bool, now_ms: i64) -> Result<bool, StoreError> {
        let state = if delivered { "delivered" } else { "failed" };
        let changed = self.conn.execute(
            "UPDATE prompt_queue SET state = ?3, updated_at = ?4 WHERE session_id = ?1 AND id = ?2 AND state = 'delivering'",
            rusqlite::params![session_id, id, state, now_ms],
        )?;
        Ok(changed == 1)
    }

    pub fn retry_queued_prompt(&self, session_id: i64, id: i64, now_ms: i64) -> Result<bool, StoreError> {
        let changed = self.conn.execute(
            "UPDATE prompt_queue SET state = 'held', updated_at = ?3 WHERE session_id = ?1 AND id = ?2 AND state = 'failed'",
            rusqlite::params![session_id, id, now_ms],
        )?;
        Ok(changed == 1)
    }

    /// 进程在 delivering 中消失时不能自动重写 CLI；转为失败，由用户确认重试。
    pub fn recover_prompt_deliveries(&self) -> Result<usize, StoreError> {
        let delivering = self.conn.execute("UPDATE prompt_queue SET state = 'failed' WHERE state = 'delivering'", [])?;
        // 临时负 id 跨重启会复用；启动时留下的 held 只能孤立保存，不能随下一次认领投进新会话。
        let orphaned = self.conn.execute("UPDATE prompt_queue SET state = 'orphaned' WHERE session_id < 0 AND state IN ('held', 'failed', 'delivering')", [])?;
        Ok(delivering + orphaned)
    }

    pub fn move_queued_prompts(&self, from: i64, to: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE prompt_queue SET session_id = ?2 WHERE session_id = ?1 AND state <> 'orphaned'", [from, to])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_moves_held_prompts_and_keeps_new_ones_last() {
        let store = Store::open_in_memory().unwrap();
        let a = store.enqueue_prompt(7, "a", 100).unwrap();
        let b = store.enqueue_prompt(7, "b", 101).unwrap();
        let c = store.enqueue_prompt(7, "c", 102).unwrap();
        let order = |store: &Store| store.queued_prompts(7).unwrap().into_iter().map(|item| item.draft_json).collect::<Vec<_>>();
        assert!(store.reorder_queued_prompts(7, &[c.id, a.id, b.id], 103).unwrap());
        assert_eq!(order(&store), ["c", "a", "b"]);
        // 新入队的排在最后，不会插进拖过的几条中间。
        store.enqueue_prompt(7, "d", 104).unwrap();
        assert_eq!(order(&store), ["c", "a", "b", "d"]);
        // 只挪其中两条：其余留在原位。
        assert!(store.reorder_queued_prompts(7, &[b.id, c.id], 105).unwrap());
        assert_eq!(order(&store), ["b", "a", "c", "d"]);
    }

    #[test]
    fn reorder_refuses_prompts_that_are_no_longer_held() {
        let store = Store::open_in_memory().unwrap();
        let a = store.enqueue_prompt(7, "a", 100).unwrap();
        let b = store.enqueue_prompt(7, "b", 101).unwrap();
        let other = store.enqueue_prompt(8, "other", 101).unwrap();
        store.claim_queued_prompt(7, a.id, 102).unwrap();
        // 另一端刚领走 a：整批不动。
        assert!(!store.reorder_queued_prompts(7, &[b.id, a.id], 103).unwrap());
        // 别的会话的消息、重复的 id 都不行。
        assert!(!store.reorder_queued_prompts(7, &[b.id, other.id], 103).unwrap());
        assert!(!store.reorder_queued_prompts(7, &[b.id, b.id], 103).unwrap());
        let order: Vec<_> = store.queued_prompts(7).unwrap().into_iter().map(|item| item.draft_json).collect();
        assert_eq!(order, ["a", "b"]);
    }

    #[test]
    fn edit_cancel_and_claim_respect_ownership() {
        let store = Store::open_in_memory().unwrap();
        let first = store.enqueue_prompt(7, "first", 100).unwrap();
        let second = store.enqueue_prompt(7, "second", 101).unwrap();
        assert!(store.edit_queued_prompt(7, first.id, "edited", 102).unwrap());
        assert_eq!(store.queued_prompt(7, first.id).unwrap().unwrap().draft_json, "edited");
        assert_eq!(store.claim_queued_prompt(7, first.id, 103).unwrap().unwrap().state, "delivering");
        assert!(!store.edit_queued_prompt(7, first.id, "too late", 104).unwrap());
        assert!(!store.cancel_queued_prompt(7, first.id, 104).unwrap());
        assert!(store.cancel_queued_prompt(7, second.id, 104).unwrap());
        assert!(store.claim_queued_prompt(7, second.id, 105).unwrap().is_none());
        assert!(store.finish_prompt_delivery(7, first.id, true, 106).unwrap());
        assert!(store.queued_prompts(7).unwrap().is_empty());
    }

    #[test]
    fn failed_delivery_keeps_draft_and_requires_explicit_retry() {
        let store = Store::open_in_memory().unwrap();
        let item = store.enqueue_prompt(9, "draft", 100).unwrap();
        store.claim_queued_prompt(9, item.id, 101).unwrap();
        assert!(store.finish_prompt_delivery(9, item.id, false, 102).unwrap());
        assert_eq!(store.queued_prompts(9).unwrap()[0].state, "failed");
        assert!(store.retry_queued_prompt(9, item.id, 103).unwrap());
        assert!(store.claim_queued_prompt(9, item.id, 104).unwrap().is_some());
        store.recover_prompt_deliveries().unwrap();
        assert_eq!(store.queued_prompts(9).unwrap()[0].state, "failed");
    }

    #[test]
    fn failed_delivery_can_be_dismissed_without_retrying() {
        let store = Store::open_in_memory().unwrap();
        let item = store.enqueue_prompt(10, "draft", 100).unwrap();
        store.claim_queued_prompt(10, item.id, 101).unwrap();
        store.finish_prompt_delivery(10, item.id, false, 102).unwrap();
        assert!(store.cancel_queued_prompt(10, item.id, 103).unwrap());
        assert!(store.queued_prompts(10).unwrap().is_empty());
        assert!(!store.retry_queued_prompt(10, item.id, 104).unwrap());
    }

    #[test]
    fn claim_and_cancel_cannot_both_win() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-queue-{}-{unique}", std::process::id()));
        let db = dir.join("board.db");
        std::fs::create_dir_all(&dir).unwrap();
        let first = Store::open(&db).unwrap();
        let second = Store::open(&db).unwrap();
        let item = first.enqueue_prompt(11, "draft", 100).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let (cancel, claim) = std::thread::scope(|scope| {
            let left = barrier.clone();
            let right = barrier.clone();
            let cancel_thread = scope.spawn(move || {
                left.wait();
                first.cancel_queued_prompt(11, item.id, 200).unwrap()
            });
            let claim_thread = scope.spawn(move || {
                right.wait();
                second.claim_queued_prompt(11, item.id, 200).unwrap().is_some()
            });
            barrier.wait();
            (cancel_thread.join().unwrap(), claim_thread.join().unwrap())
        });
        assert_ne!(cancel, claim);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn reopen_restores_held_and_failed_without_replaying_cancelled() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("meowo-queue-reopen-{}-{unique}", std::process::id()));
        let db = dir.join("board.db");
        std::fs::create_dir_all(&dir).unwrap();
        {
            let store = Store::open(&db).unwrap();
            let held = store.enqueue_prompt(21, "before", 100).unwrap();
            store.edit_queued_prompt(21, held.id, "after", 101).unwrap();
            let delivering = store.enqueue_prompt(21, "in flight", 102).unwrap();
            store.claim_queued_prompt(21, delivering.id, 103).unwrap();
            let cancelled = store.enqueue_prompt(21, "never", 104).unwrap();
            store.cancel_queued_prompt(21, cancelled.id, 105).unwrap();
        }
        let store = Store::open(&db).unwrap();
        assert_eq!(store.recover_prompt_deliveries().unwrap(), 1);
        let items = store.queued_prompts(21).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].draft_json, "after");
        assert_eq!(items[0].state, "held");
        assert_eq!(items[1].state, "failed");
        assert!(store.claim_queued_prompt(21, 3, 200).unwrap().is_none());
        drop(store);
        let _ = std::fs::remove_file(db);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn temporary_session_queue_moves_on_claim() {
        let store = Store::open_in_memory().unwrap();
        let item = store.enqueue_prompt(-1, "draft", 100).unwrap();
        store.move_queued_prompts(-1, 42).unwrap();
        assert!(store.queued_prompts(-1).unwrap().is_empty());
        assert_eq!(store.queued_prompt(42, item.id).unwrap().unwrap().session_id, 42);
    }
}


#[cfg(test)]
mod restart_tests {
    use super::*;

    #[test]
    fn stale_negative_queue_never_moves_into_a_reused_session_id() {
        let store = Store::open_in_memory().unwrap();
        let item = store.enqueue_prompt(-1, "old launch", 1).unwrap();
        store.recover_prompt_deliveries().unwrap();
        assert_eq!(store.queued_prompt(-1, item.id).unwrap().unwrap().state, "orphaned");
        store.move_queued_prompts(-1, 99).unwrap();
        assert!(store.queued_prompt(99, item.id).unwrap().is_none());
        assert!(store.claim_queued_prompt(-1, item.id, 2).unwrap().is_none());
        let failed = store.enqueue_prompt(-1, "failed before restart", 3).unwrap();
        store.claim_queued_prompt(-1, failed.id, 4).unwrap();
        store.finish_prompt_delivery(-1, failed.id, false, 5).unwrap();
        store.recover_prompt_deliveries().unwrap();
        store.move_queued_prompts(-1, 100).unwrap();
        assert!(store.queued_prompt(100, failed.id).unwrap().is_none());
        assert_eq!(store.orphaned_prompts().unwrap().len(), 2);
        let restored = store.take_orphaned_prompt(-1, failed.id).unwrap().unwrap();
        assert_eq!(restored.draft_json, "failed before restart");
        assert_eq!(store.orphaned_prompts().unwrap().len(), 1);
    }
}
