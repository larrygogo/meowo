use crate::{
    managed::{canonical_json, invalid, MAX_JSON_BYTES},
    Store, StoreError,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

const MAX_HISTORY_BYTES: usize = 16 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024 - 4096;
const TEXT_CHUNK_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNativeRecoveryCursor {
    pub sequence: i64,
    pub message_index: usize,
    pub text_byte_offset: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNativeRecoveryChunk {
    pub sequence: i64,
    pub created_at: i64,
    pub message_index: usize,
    pub role: String,
    pub text: String,
    pub text_byte_offset: usize,
    pub complete: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNativeRecoveryPage {
    pub chunks: Vec<ManagedNativeRecoveryChunk>,
    pub next_cursor: Option<ManagedNativeRecoveryCursor>,
}

pub(crate) fn canonical_event(raw: &str) -> Result<String, StoreError> {
    if raw.len() <= MAX_JSON_BYTES {
        return canonical_json(raw);
    }
    if raw.len() > MAX_HISTORY_BYTES {
        return Err(invalid("payload_too_large"));
    }
    let value: Value = serde_json::from_str(raw).map_err(|_| invalid("invalid_json"))?;
    if value.get("kind").and_then(Value::as_str) != Some("history")
        || !value.get("messages").is_some_and(Value::is_array)
    {
        return Err(invalid("payload_too_large"));
    }
    serde_json::to_string(&value).map_err(|_| invalid("invalid_json"))
}

fn next_after(
    tx: &rusqlite::Transaction<'_>,
    session: i64,
    sequence: i64,
    index: usize,
) -> Result<Option<ManagedNativeRecoveryCursor>, StoreError> {
    Ok(tx.query_row("SELECT m.sequence,m.message_index FROM managed_native_recovery_messages m JOIN managed_native_recoveries r ON r.session_id=m.session_id AND r.sequence=m.sequence WHERE m.session_id=?1 AND r.unresolved=1 AND (m.sequence<?2 OR (m.sequence=?2 AND m.message_index>?3)) ORDER BY m.sequence DESC,m.message_index LIMIT 1",params![session,sequence,index as i64],|row|Ok(ManagedNativeRecoveryCursor{sequence:row.get(0)?,message_index:row.get::<_,i64>(1)? as usize,text_byte_offset:0})).optional()?)
}

impl Store {
    /// 原文按 UTF-8 边界分块；游标只定位已保存的原生事实，不按文字相似度归并现场消息。
    pub fn managed_native_recovery_page(
        &self,
        session: i64,
        cursor: Option<&ManagedNativeRecoveryCursor>,
        limit: usize,
    ) -> Result<ManagedNativeRecoveryPage, StoreError> {
        if limit == 0 {
            return Err(invalid("invalid_limit"));
        }
        if cursor.is_some_and(|cursor| {
            cursor.sequence <= 0
                || cursor.message_index > 16384
                || cursor.text_byte_offset > MAX_HISTORY_BYTES
        }) {
            return Err(invalid("invalid_cursor"));
        }
        let limit = limit.min(200);
        let tx = self.conn.unchecked_transaction()?;
        let mut chunks = Vec::new();
        let mut used = 0;
        let mut next_cursor = None;
        {
            let mut stmt=tx.prepare("SELECT m.sequence,e.created_at,m.message_index,m.role,m.text FROM managed_native_recovery_messages m JOIN managed_native_recoveries r ON r.session_id=m.session_id AND r.sequence=m.sequence JOIN managed_events e ON e.session_id=m.session_id AND e.sequence=m.sequence WHERE m.session_id=?1 AND r.unresolved=1 AND (m.sequence<?2 OR (m.sequence=?2 AND m.message_index>=?3)) ORDER BY m.sequence DESC,m.message_index LIMIT ?4")?;
            let mut rows = stmt.query(params![
                session,
                cursor.map_or(i64::MAX, |cursor| cursor.sequence),
                cursor.map_or(0, |cursor| cursor.message_index) as i64,
                (limit + 1) as i64
            ])?;
            'messages: while let Some(row) = rows.next()? {
                let sequence: i64 = row.get(0)?;
                let created_at: i64 = row.get(1)?;
                let message_index = row.get::<_, i64>(2)? as usize;
                let role: String = row.get(3)?;
                let text: String = row.get(4)?;
                let mut offset = cursor
                    .filter(|cursor| {
                        cursor.sequence == sequence && cursor.message_index == message_index
                    })
                    .map_or(0, |cursor| cursor.text_byte_offset);
                if offset > text.len() || !text.is_char_boundary(offset) {
                    return Err(invalid("invalid_cursor"));
                }
                if offset == text.len() && !text.is_empty() {
                    continue;
                }
                loop {
                    let mut end = (offset + TEXT_CHUNK_BYTES).min(text.len());
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    let chunk = ManagedNativeRecoveryChunk {
                        sequence,
                        created_at,
                        message_index,
                        role: role.clone(),
                        text: text[offset..end].into(),
                        text_byte_offset: offset,
                        complete: end == text.len(),
                    };
                    let size = serde_json::to_vec(&chunk)
                        .map_err(|_| invalid("invalid_history"))?
                        .len()
                        + 1;
                    if used + size > MAX_PAGE_BYTES {
                        next_cursor = Some(ManagedNativeRecoveryCursor {
                            sequence,
                            message_index,
                            text_byte_offset: offset,
                        });
                        break 'messages;
                    }
                    used += size;
                    chunks.push(chunk);
                    offset = end;
                    if chunks.len() == limit {
                        next_cursor = if offset < text.len() {
                            Some(ManagedNativeRecoveryCursor {
                                sequence,
                                message_index,
                                text_byte_offset: offset,
                            })
                        } else {
                            next_after(&tx, session, sequence, message_index)?
                        };
                        break 'messages;
                    }
                    if offset == text.len() {
                        break;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(ManagedNativeRecoveryPage {
            chunks,
            next_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(store: &Store) -> i64 {
        store
            .managed_create("gemini", "C:/isolated-recovery", None, 1)
            .unwrap()
    }
    fn history(text: &str) -> String {
        json!({"kind":"history","messages":[{"role":"assistant","text":text,"native_message_id":null,"segment_id":null,"request_id":null}]}).to_string()
    }

    #[test]
    fn large_utf8_native_text_pages_losslessly_under_the_ipc_budget_without_hiding_older_snapshots()
    {
        let store = Store::open_in_memory().unwrap();
        let session = session(&store);
        let old = store
            .managed_record_event(
                session,
                &history("older unknown tail before compact"),
                None,
                1,
            )
            .unwrap();
        // 大于单次IPC预算，中文字节与emoji刻意跨128KiB边界；原文不得截断或按相似度覆盖旧snapshot。
        let original = "中文🙂\n".repeat(600_000);
        let raw = history(&original);
        assert!(raw.len() > 4 * 1024 * 1024 && raw.len() < MAX_HISTORY_BYTES);
        let sequence = store.managed_record_event(session, &raw, None, 2).unwrap();
        let saved: String = store
            .conn
            .query_row(
                "SELECT payload_json FROM managed_events WHERE session_id=?1 AND sequence=?2",
                params![session, sequence],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&saved).unwrap()["messages"][0]["text"],
            original
        );
        assert!(store
            .managed_events_since(session, 0, 200)
            .unwrap()
            .iter()
            .all(|event| event.payload_json.len() < 100));
        let mut cursor = None;
        let mut reconstructed = String::new();
        let mut older = String::new();
        let mut pages = 0;
        loop {
            let page = store
                .managed_native_recovery_page(session, cursor.as_ref(), 50)
                .unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() < 4 * 1024 * 1024);
            for chunk in &page.chunks {
                if chunk.sequence == sequence {
                    assert_eq!(chunk.text_byte_offset, reconstructed.len());
                    reconstructed.push_str(&chunk.text);
                } else {
                    assert_eq!(chunk.sequence, old);
                    assert_eq!(chunk.text_byte_offset, older.len());
                    older.push_str(&chunk.text);
                }
            }
            pages += 1;
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            assert!(pages < 100);
        }
        assert!(pages > 1);
        assert_eq!(reconstructed, original);
        assert_eq!(older, "older unknown tail before compact");
        assert!(store
            .managed_chat_window(session, None, 30)
            .unwrap()
            .items_json
            .is_empty());
        assert!(store
            .managed_native_recovery_page(
                session + 1,
                Some(&ManagedNativeRecoveryCursor {
                    sequence,
                    message_index: 0,
                    text_byte_offset: 0
                }),
                50
            )
            .unwrap()
            .chunks
            .is_empty());
        assert!(store
            .managed_native_recovery_page(
                session,
                Some(&ManagedNativeRecoveryCursor {
                    sequence,
                    message_index: 0,
                    text_byte_offset: 1
                }),
                50
            )
            .is_err());
        store
            .managed_resolve_native_recovery(session, sequence)
            .unwrap();
        let remaining = store
            .managed_native_recovery_page(session, None, 50)
            .unwrap();
        assert!(remaining.chunks.iter().all(|chunk| chunk.sequence == old));
    }

    #[test]
    fn two_megabyte_history_is_special_cased_but_ordinary_events_keep_their_limit() {
        let store = Store::open_in_memory().unwrap();
        let session = session(&store);
        let text = "中🙂".repeat(300_000);
        assert!(text.len() > 2 * 1024 * 1024);
        assert!(store
            .managed_record_event(session, &history(&text), None, 1)
            .is_ok());
        assert!(store
            .managed_record_event(
                session,
                &json!({"kind":"output","text":text}).to_string(),
                None,
                2
            )
            .is_err());
        assert!(store
            .managed_record_event(session, &history(&"x".repeat(MAX_HISTORY_BYTES)), None, 3)
            .is_err());
        assert_eq!(
            store.managed_events_since(session, 0, 200).unwrap().len(),
            1
        );
    }
}
