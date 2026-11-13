use crate::{
    managed::{canonical_json, invalid, ManagedEvent, MANAGED_PAGE_LIMIT},
    store::ImmediateTx,
    Store, StoreError,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedChatWindow {
    pub items_json: Vec<String>,
    pub offset: i64,
    pub earliest: i64,
    pub has_more: bool,
}

fn chat_shape(item: &Value) -> Result<(&str, &str), StoreError> {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .ok_or_else(|| invalid("invalid_chat_item"))?;
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("invalid_chat_item"))?;
    let text = || item.get("text").is_some_and(Value::is_string);
    let valid = match kind {
        "user_text" | "assistant_text" | "assistant_delta" | "reasoning" | "reasoning_delta"
        | "prompt_queued" => text(),
        "turn_error" => text() && item.get("label").is_some_and(Value::is_string),
        "tool_use" => {
            item.get("name").is_some_and(Value::is_string)
                && item.get("summary").is_some_and(Value::is_string)
        }
        "tool_result" => text() && item.get("is_error").is_some_and(Value::is_boolean),
        "meta" => item.get("kind").is_some_and(Value::is_string),
        _ => false,
    };
    if !valid
        || item
            .get("timestamp")
            .is_some_and(|v| !v.is_null() && !v.is_string())
    {
        return Err(invalid("invalid_chat_item"));
    }
    Ok((id, kind))
}

fn fact_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedEvent> {
    Ok(ManagedEvent {
        session_id: row.get(0)?,
        sequence: row.get(1)?,
        payload_json: row.get(2)?,
        created_at: row.get(3)?,
    })
}

impl Store {
    pub fn managed_resolve_native_recovery(
        &self,
        session_id: i64,
        sequence: i64,
    ) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE managed_native_recoveries SET unresolved=0 WHERE session_id=?1 AND sequence=?2 AND unresolved=1",params![session_id,sequence])?>0)
    }

    pub fn managed_chat_item(
        &self,
        session_id: i64,
        item_id: &str,
    ) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT payload_json FROM managed_chat_items WHERE session_id=?1 AND item_id=?2 ORDER BY ordinal DESC LIMIT 1",params![session_id,item_id],|r|r.get(0)).optional()?)
    }

    /// raw 与投影同事务；若 ChatItem 不合法，事件序号也不能被提前消费。
    pub fn managed_record_event(
        &self,
        session_id: i64,
        event_payload_json: &str,
        chat_item_json: Option<&str>,
        now: i64,
    ) -> Result<i64, StoreError> {
        let payload = crate::managed_recovery::canonical_event(event_payload_json)?;
        let event: Value = serde_json::from_str(&payload).map_err(|_| invalid("invalid_json"))?;
        let item = chat_item_json
            .map(canonical_json)
            .transpose()?
            .map(|value| {
                serde_json::from_str::<Value>(&value).map_err(|_| invalid("invalid_chat_item"))
            })
            .transpose()?;
        if let Some(item) = &item {
            chat_shape(item)?;
        }
        let tx = ImmediateTx::begin(&self.conn)?;
        if self.managed_state(session_id)?.is_none() {
            return Err(invalid("session_missing"));
        }
        let latest: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM managed_events WHERE session_id=?1",
            [session_id],
            |r| r.get(0),
        )?;
        let sequence = latest
            .checked_add(1)
            .ok_or_else(|| invalid("sequence_exhausted"))?;
        self.conn.execute("INSERT INTO managed_events(session_id,sequence,payload_json,created_at) VALUES(?1,?2,?3,?4)",params![session_id,sequence,payload,now])?;
        if event.get("kind").and_then(Value::as_str) == Some("history")
            && event.get("reconciled").and_then(Value::as_bool) != Some(true)
            && event.get("messages").is_some_and(Value::is_array)
        {
            self.conn.execute("INSERT INTO managed_native_recoveries(session_id,sequence,unresolved) VALUES(?1,?2,1)",params![session_id,sequence])?;
            let messages = event["messages"]
                .as_array()
                .ok_or_else(|| invalid("invalid_history"))?;
            if messages.len() > 16384 {
                return Err(invalid("history_messages_limit"));
            }
            for (index, message) in messages.iter().enumerate() {
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .filter(|role| role.len() <= 128)
                    .ok_or_else(|| invalid("invalid_history"))?;
                let text = message
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("invalid_history"))?;
                self.conn.execute("INSERT INTO managed_native_recovery_messages(session_id,sequence,message_index,role,text) VALUES(?1,?2,?3,?4,?5)",params![session_id,sequence,index as i64,role,text])?;
            }
        }
        if let Some(item) = item {
            self.project_managed_chat(
                session_id,
                sequence,
                item,
                event.get("kind").and_then(Value::as_str) == Some("history")
                    && event.get("reconciled").and_then(Value::as_bool) != Some(true),
            )?;
        }
        self.snapshot_managed_fact(session_id, sequence, &event, &payload, now)?;
        tx.commit()?;
        Ok(sequence)
    }

    fn project_managed_chat(
        &self,
        session_id: i64,
        sequence: i64,
        mut item: Value,
        history: bool,
    ) -> Result<(), StoreError> {
        let (id, kind) = chat_shape(&item)?;
        let (id, kind) = (id.to_string(), kind.to_string());
        // 入队回执不是真正的聊天正文；只在 raw 日志推进投递反馈，不能挤占消息分页窗口。
        if kind == "prompt_queued" {
            return Ok(());
        }
        let existing:Option<(i64,String)> = self.conn.query_row("SELECT ordinal,kind FROM managed_chat_items WHERE session_id=?1 AND item_id=?2 ORDER BY ordinal DESC LIMIT 1",params![session_id,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if history && existing.is_some() {
            return Ok(());
        }
        let target = match kind.as_str() {
            "assistant_delta" => Some("assistant_text"),
            "reasoning_delta" => Some("reasoning"),
            _ => None,
        };
        if let Some(target) = target {
            item["type"] = Value::String(target.into());
            let last:Option<(i64,String,String,String)> = self.conn.query_row("SELECT ordinal,item_id,kind,payload_json FROM managed_chat_items WHERE session_id=?1 ORDER BY ordinal DESC LIMIT 1",[session_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            if let Some((ordinal, last_id, last_kind, payload)) =
                last.filter(|(_, last_id, last_kind, _)| last_id == &id && last_kind == target)
            {
                let mut previous: Value =
                    serde_json::from_str(&payload).map_err(|_| invalid("invalid_chat_item"))?;
                let previous_text = previous
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("invalid_chat_item"))?;
                let delta = item
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("invalid_chat_item"))?;
                previous["text"] = Value::String(format!("{previous_text}{delta}"));
                self.conn.execute("UPDATE managed_chat_items SET payload_json=?1,history=0 WHERE session_id=?2 AND ordinal=?3 AND item_id=?4 AND kind=?5",params![previous.to_string(),session_id,ordinal,last_id,last_kind])?;
                return Ok(());
            }
            // 同 id 在工具之后再次出现也不能回头追加，保留独立段；宿主应给不同段分配不同稳定 id。
            self.conn.execute("INSERT INTO managed_chat_items(session_id,ordinal,item_id,kind,payload_json,history) VALUES(?1,?2,?3,?4,?5,?6)",params![session_id,sequence,id,target,item.to_string(),history])?;
        } else if let Some((ordinal, old_kind)) = existing {
            if old_kind != kind {
                return Err(invalid("chat_id_conflict"));
            }
            self.conn.execute("UPDATE managed_chat_items SET payload_json=?1,history=0 WHERE session_id=?2 AND ordinal=?3",params![item.to_string(),session_id,ordinal])?;
        } else {
            self.conn.execute("INSERT INTO managed_chat_items(session_id,ordinal,item_id,kind,payload_json,history) VALUES(?1,?2,?3,?4,?5,?6)",params![session_id,sequence,id,kind,item.to_string(),history])?;
        }
        Ok(())
    }

    fn snapshot_managed_fact(
        &self,
        session_id: i64,
        sequence: i64,
        event: &Value,
        payload: &str,
        now: i64,
    ) -> Result<(), StoreError> {
        match event.get("kind").and_then(Value::as_str) {
            Some(kind @ ("metadata" | "catalog" | "lead_claimed")) => {
                self.conn.execute("INSERT INTO managed_facts(session_id,kind,fact_key,sequence,payload_json,created_at,resolved) VALUES(?1,?2,'',?3,?4,?5,0) ON CONFLICT(session_id,kind,fact_key) DO UPDATE SET sequence=excluded.sequence,payload_json=excluded.payload_json,created_at=excluded.created_at",params![session_id,kind,sequence,payload,now])?;
            }
            Some(kind @ ("interaction" | "interaction_resolved")) => {
                let id = event
                    .get("interaction_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && s.len() <= 4096)
                    .ok_or_else(|| invalid("invalid_interaction"))?;
                let resolved = kind == "interaction_resolved";
                // 保留 resolve 墓碑；同一请求的迟到重放不能把已经处理的审批重新变成待批。
                self.conn.execute("INSERT INTO managed_facts(session_id,kind,fact_key,sequence,payload_json,created_at,resolved) VALUES(?1,'interaction',?2,?3,?4,?5,?6) ON CONFLICT(session_id,kind,fact_key) DO UPDATE SET sequence=excluded.sequence,payload_json=excluded.payload_json,created_at=excluded.created_at,resolved=excluded.resolved WHERE managed_facts.resolved=0",params![session_id,id,sequence,payload,now,resolved])?;
            }
            _ => {}
        }
        Ok(())
    }

    pub fn managed_latest_fact(
        &self,
        session_id: i64,
        kind: &str,
    ) -> Result<Option<ManagedEvent>, StoreError> {
        Ok(self.conn.query_row("SELECT session_id,sequence,payload_json,created_at FROM managed_facts WHERE session_id=?1 AND kind=?2 AND fact_key=''",params![session_id,kind],fact_row).optional()?)
    }

    /// 返回全部尚未解决的独立原生请求；UI 在引擎 failed/stopped 时必须另行禁止作答。
    pub fn managed_pending_interactions(
        &self,
        session_id: i64,
    ) -> Result<Vec<ManagedEvent>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT session_id,sequence,payload_json,created_at FROM managed_facts WHERE session_id=?1 AND kind='interaction' AND resolved=0 ORDER BY sequence")?;
        let rows = stmt
            .query_map([session_id], fact_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn managed_chat_window(
        &self,
        session_id: i64,
        before: Option<i64>,
        limit: usize,
    ) -> Result<ManagedChatWindow, StoreError> {
        self.managed_chat_snapshot(session_id, before, limit, || {})
    }

    fn managed_chat_snapshot(
        &self,
        session_id: i64,
        before: Option<i64>,
        limit: usize,
        after_snapshot: impl FnOnce(),
    ) -> Result<ManagedChatWindow, StoreError> {
        if limit == 0 {
            return Err(invalid("invalid_limit"));
        }
        let tx = self.conn.unchecked_transaction()?;
        // 第一条读固定 WAL 快照，offset 与完整文本必须一起取，不能夹进另一连接刚提交的半轮状态。
        let offset: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sequence),0) FROM managed_events WHERE session_id=?1",
            [session_id],
            |r| r.get(0),
        )?;
        after_snapshot();
        let mut rows = {
            let mut stmt = tx.prepare("SELECT ordinal,payload_json FROM managed_chat_items WHERE session_id=?1 AND ordinal<?2 ORDER BY ordinal DESC LIMIT ?3")?;
            let rows = stmt
                .query_map(
                    params![
                        session_id,
                        before.unwrap_or(i64::MAX),
                        limit.min(MANAGED_PAGE_LIMIT) as i64
                    ],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        rows.reverse();
        let earliest = rows.first().map_or(0, |r| r.0);
        let has_more = earliest>0 && tx.query_row("SELECT EXISTS(SELECT 1 FROM managed_chat_items WHERE session_id=?1 AND ordinal<?2)",params![session_id,earliest],|r|r.get::<_,bool>(0))?;
        tx.commit()?;
        Ok(ManagedChatWindow {
            items_json: rows.into_iter().map(|r| r.1).collect(),
            offset,
            earliest,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::Duration;

    fn create(store: &Store) -> i64 {
        store
            .managed_create("codex", "C:/projection", None, 1)
            .unwrap()
    }
    fn item(kind: &str, id: &str, text: &str) -> Value {
        json!({"type":kind,"id":id,"timestamp":null,"text":text})
    }
    fn record(store: &Store, id: i64, item: &Value) -> i64 {
        store
            .managed_record_event(id, "{\"kind\":\"output\"}", Some(&item.to_string()), 100)
            .unwrap()
    }
    fn values(window: &ManagedChatWindow) -> Vec<Value> {
        window
            .items_json
            .iter()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }

    #[test]
    fn deltas_merge_only_the_same_tail_kind_and_segment_id() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        record(&store, id, &item("user_text", "u", "go"));
        record(&store, id, &item("assistant_delta", "a", "first"));
        record(&store, id, &item("assistant_delta", "a", " part"));
        record(&store, id, &item("reasoning", "r", "why"));
        record(&store, id, &item("reasoning_delta", "r", "?"));
        record(
            &store,
            id,
            &json!({"type":"tool_use","id":"t","timestamp":null,"name":"Read","summary":"file"}),
        );
        record(&store, id, &item("assistant_delta", "a", "after tool"));
        record(&store, id, &item("assistant_delta", "b", "another segment"));
        record(&store, id, &item("prompt_queued", "ack", "not a message"));
        let window = store.managed_chat_window(id, None, 50).unwrap();
        let rows = values(&window);
        assert_eq!(window.offset, 9);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[1], item("assistant_text", "a", "first part"));
        assert_eq!(rows[2], item("reasoning", "r", "why?"));
        assert_eq!(rows[4], item("assistant_text", "a", "after tool"));
        assert_eq!(rows[5], item("assistant_text", "b", "another segment"));
    }

    #[test]
    fn paging_counts_complete_messages_not_stream_chunks() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        for index in 0..55 {
            record(
                &store,
                id,
                &item(
                    "assistant_delta",
                    &format!("a-{index}"),
                    &format!("message {index}"),
                ),
            );
            record(
                &store,
                id,
                &item("assistant_delta", &format!("a-{index}"), " complete"),
            );
        }
        let tail = store.managed_chat_window(id, None, 40).unwrap();
        assert_eq!(tail.items_json.len(), 40);
        assert_eq!(tail.offset, 110);
        assert_eq!(tail.earliest, 31);
        assert!(tail.has_more);
        assert_eq!(values(&tail)[0]["text"], "message 15 complete");
        let before = store
            .managed_chat_window(id, Some(tail.earliest), 40)
            .unwrap();
        assert_eq!(before.items_json.len(), 15);
        assert_eq!(before.earliest, 1);
        assert!(!before.has_more);
        assert_eq!(before.offset, 110);
        assert_eq!(
            store
                .managed_chat_window(id, None, usize::MAX)
                .unwrap()
                .items_json
                .len(),
            55
        );
        assert!(store.managed_chat_window(id, None, 0).is_err());
    }

    #[test]
    fn stable_items_update_without_moving_the_ordinal_or_duplicate_history() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        let mut tool = json!({"type":"tool_result","id":"result","timestamp":null,"text":"running","is_error":false});
        record(&store, id, &tool);
        tool["text"] = json!("done");
        record(&store, id, &tool);
        let window = store.managed_chat_window(id, None, 40).unwrap();
        assert_eq!(window.items_json.len(), 1);
        assert_eq!(window.earliest, 1);
        assert_eq!(window.offset, 2);
        assert_eq!(values(&window)[0]["text"], "done");
        let conflicting = item("assistant_text", "result", "wrong type");
        assert!(store
            .managed_record_event(id, "{}", Some(&conflicting.to_string()), 100)
            .is_err());
        assert_eq!(store.managed_chat_window(id, None, 40).unwrap(), window);
    }

    #[test]
    fn late_history_cannot_overwrite_stream_and_repeated_import_is_deduplicated() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        record(&store, id, &item("assistant_delta", "live", "new stream"));
        let stale = item("assistant_text", "live", "old history");
        store
            .managed_record_event(id, "{\"kind\":\"history\"}", Some(&stale.to_string()), 100)
            .unwrap();
        let imported = item("user_text", "history-u", "imported");
        for _ in 0..2 {
            store
                .managed_record_event(
                    id,
                    "{\"kind\":\"history\"}",
                    Some(&imported.to_string()),
                    100,
                )
                .unwrap();
        }
        let rows = values(&store.managed_chat_window(id, None, 40).unwrap());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["text"], "new stream");
        assert_eq!(rows[1], imported);
        let other = create(&store);
        record(
            &store,
            other,
            &item("user_text", "history-u", "other session"),
        );
        assert_eq!(
            values(&store.managed_chat_window(id, None, 40).unwrap())[1],
            imported
        );
    }

    #[test]
    fn malformed_projection_rolls_back_raw_sequence_and_fact_snapshot_together() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        record(&store, id, &item("assistant_delta", "a", "keep"));
        let before = store.managed_chat_window(id, None, 40).unwrap();
        assert!(store
            .managed_record_event(
                id,
                "{\"kind\":\"metadata\",\"model\":\"bad\"}",
                Some("{\"id\":\"bad\",\"type\":\"assistant_delta\",\"text\":3}"),
                100
            )
            .is_err());
        assert!(store
            .managed_record_event(
                id,
                "{\"kind\":\"interaction\"}",
                Some(&item("user_text", "new", "must rollback").to_string()),
                100
            )
            .is_err());
        assert_eq!(store.managed_chat_window(id, None, 40).unwrap(), before);
        assert!(store.managed_latest_fact(id, "metadata").unwrap().is_none());
        assert_eq!(store.managed_append_event(id, "{}", 100).unwrap(), 2);
    }

    #[test]
    fn snapshot_and_event_incremental_replay_produce_the_same_complete_text() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        for index in 0..500 {
            let text = format!("{index}🐱");
            let event = json!({"kind":"output","segment_id":"segment","text":text});
            store
                .managed_record_event(
                    id,
                    &event.to_string(),
                    Some(&item("assistant_delta", "segment", &text).to_string()),
                    100,
                )
                .unwrap();
        }
        let mut after = 0;
        let mut replay = String::new();
        loop {
            let batch = store.managed_events_since(id, after, 200).unwrap();
            if batch.is_empty() {
                break;
            }
            for event in batch {
                after = event.sequence;
                let payload: Value = serde_json::from_str(&event.payload_json).unwrap();
                replay.push_str(payload["text"].as_str().unwrap());
            }
        }
        let window = store.managed_chat_window(id, None, 40).unwrap();
        assert_eq!(window.offset, after);
        assert_eq!(window.items_json.len(), 1);
        assert_eq!(values(&window)[0]["text"].as_str(), Some(replay.as_str()));
    }

    #[test]
    fn small_facts_and_parallel_interactions_are_indexed_and_session_scoped() {
        let store = Store::open_in_memory().unwrap();
        let id = create(&store);
        let other = create(&store);
        store
            .managed_append_event(id, "{\"kind\":\"metadata\",\"model\":\"m\"}", 100)
            .unwrap();
        store
            .managed_append_event(id, "{\"kind\":\"catalog\",\"models\":[]}", 100)
            .unwrap();
        for _ in 0..250 {
            store
                .managed_append_event(id, "{\"kind\":\"output\",\"text\":\"no fact write\"}", 100)
                .unwrap();
        }
        for request in ["a", "b"] {
            store
                .managed_append_event(
                    id,
                    &json!({"kind":"interaction","interaction_id":request}).to_string(),
                    100,
                )
                .unwrap();
        }
        store
            .managed_append_event(
                id,
                "{\"kind\":\"interaction_resolved\",\"interaction_id\":\"a\"}",
                100,
            )
            .unwrap();
        // 先 resolve 后补到的迟到事件不得复活请求。
        store
            .managed_append_event(
                id,
                "{\"kind\":\"interaction_resolved\",\"interaction_id\":\"c\"}",
                100,
            )
            .unwrap();
        store
            .managed_append_event(
                id,
                "{\"kind\":\"interaction\",\"interaction_id\":\"c\"}",
                100,
            )
            .unwrap();
        store
            .managed_append_event(
                other,
                "{\"kind\":\"interaction\",\"interaction_id\":\"a\"}",
                100,
            )
            .unwrap();
        let pending = store.managed_pending_interactions(id).unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending[0].payload_json.contains("\"b\""));
        assert_eq!(store.managed_pending_interactions(other).unwrap().len(), 1);
        assert_eq!(
            store
                .managed_latest_fact(id, "metadata")
                .unwrap()
                .unwrap()
                .sequence,
            1
        );
        assert_eq!(
            store
                .managed_latest_fact(id, "catalog")
                .unwrap()
                .unwrap()
                .sequence,
            2
        );
        assert!(store.managed_latest_fact(id, "output").unwrap().is_none());
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM managed_facts WHERE session_id=?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);
    }

    struct Database(std::path::PathBuf);
    impl Database {
        fn new() -> Self {
            let time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "meowo-chat-snapshot-{}-{time}.db",
                std::process::id()
            )))
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
    fn concurrent_commit_between_offset_and_window_reads_does_not_tear_the_snapshot() {
        let db = Database::new();
        let store = Store::open(&db.0).unwrap();
        let id = create(&store);
        record(&store, id, &item("assistant_text", "a", "first"));
        let (ready_send, ready_recv) = mpsc::channel();
        let (go_send, go_recv) = mpsc::channel();
        let (done_send, done_recv) = mpsc::channel();
        let path = db.0.clone();
        let writer = std::thread::spawn(move || {
            let writer = Store::open(path).unwrap();
            ready_send.send(()).unwrap();
            go_recv.recv().unwrap();
            record(&writer, id, &item("assistant_text", "a", "second"));
            done_send.send(()).unwrap();
        });
        ready_recv.recv_timeout(Duration::from_secs(5)).unwrap();
        let old = store
            .managed_chat_snapshot(id, None, 40, || {
                go_send.send(()).unwrap();
                done_recv.recv_timeout(Duration::from_secs(5)).unwrap();
            })
            .unwrap();
        writer.join().unwrap();
        assert_eq!(old.offset, 1);
        assert_eq!(values(&old)[0]["text"], "first");
        let new = store.managed_chat_window(id, None, 40).unwrap();
        assert_eq!(new.offset, 2);
        assert_eq!(values(&new)[0]["text"], "second");
    }

    #[test]
    fn v21_upgrade_preserves_raw_log_without_inventing_projection_from_native_events() {
        let db = Database::new();
        let id = {
            let store = Store::open(&db.0).unwrap();
            let id = create(&store);
            store
                .managed_append_event(id, "{\"kind\":\"output\",\"text\":\"old\"}", 100)
                .unwrap();
            store.conn.execute_batch("DROP TABLE managed_chat_items; DROP TABLE managed_facts; PRAGMA user_version=21;").unwrap();
            id
        };
        let store = Store::open(&db.0).unwrap();
        assert_eq!(store.user_version().unwrap(), Store::CURRENT_USER_VERSION);
        assert_eq!(store.managed_events_since(id, 0, 200).unwrap().len(), 1);
        let window = store.managed_chat_window(id, None, 40).unwrap();
        assert!(window.items_json.is_empty());
        assert_eq!(window.offset, 1);
        record(&store, id, &item("assistant_delta", "new", "projected"));
        assert_eq!(
            values(&store.managed_chat_window(id, None, 40).unwrap())[0]["text"],
            "projected"
        );
    }
}
