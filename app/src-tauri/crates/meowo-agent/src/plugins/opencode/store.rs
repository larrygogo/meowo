//! opencode 会话库（SQLite）的只读访问，以及把它物化成 JSONL 缓存。
//!
//! 为什么要物化：opencode 1.x 不写任何会话日志文件，对话全在 `<数据目录>/opencode/opencode.db`
//! 的 `message` / `part` 两张表里（实测 1.19.3，`opencode db path` 打印的就是它）。而宿主的对话页
//! 整条管线（增量偏移、翻页、mtime 失效、reset）都建立在「一个只追加的 JSONL 文件」上。与其为
//! 一家 agent 在宿主里另开一条 SQLite 读取路径，不如在插件里把库**单向**导出成同形的文件——
//! 宿主一行不改，「加 agent 只动 plugins/」的纪律也守住了。
//!
//! 只追加的前提是**只导出已定稿的 part**：opencode 的 part 是原地更新的（文本流式增长、工具
//! pending→running→completed），写进文件后就改不了了。定稿判据见 [`part_settled`]；遇到第一个
//! 未定稿的 part 即停，后面的等下一轮。消息被撤回（`/undo` 删掉尾部消息）时游标指向的消息不复
//! 存在，此时整份重导到**新一代文件名**——宿主按「路径变了」重置前端，不会把旧偏移套到新内容上。

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

/// opencode 在数据目录下拼的子目录名与库文件名（`opencode db path` 实测）。
const DB_SUBDIR: &str = "opencode";
const DB_FILE: &str = "opencode.db";

/// 缓存格式版本进目录名：行结构一变，旧缓存整体作废，不必写迁移。
const CACHE_REL: &str = "cache/opencode/v1";

/// 工具输出的落盘上限（字符）。bash/read 的输出动辄几十 KB，对话页只做预览。
const TOOL_OUTPUT_MAX_CHARS: usize = 16_000;

/// 可能存放会话库的数据目录：显式 `XDG_DATA_HOME`（reporter 由 profile 里的 opencode 派生时
/// 自带）→ 默认 `~/.local/share` → 全部受管 profile 的 `data/`（与 `PROFILE` 声明的
/// `("XDG_DATA_HOME", "data")` 同源）。meowo-app 进程没有 XDG_DATA_HOME，漏了 profile 兜底，
/// 多账号下的会话会静默读不到（codex 的 `managed_codex_homes` 是同一条纪律）。
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        dirs.push(PathBuf::from(xdg));
    }
    if let Some(home) = crate::home_dir() {
        dirs.push(crate::join_rel(&home, ".local/share"));
    }
    for profile in crate::managed_profile_dirs(crate::id::OPENCODE.as_str()) {
        dirs.push(profile.join("data"));
    }
    dirs.dedup();
    dirs.into_iter()
        .map(|dir| dir.join(DB_SUBDIR).join(DB_FILE))
        .collect()
}

fn open(db: &Path) -> Option<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    // opencode 运行中持续写 WAL；读者偶遇检查点时等一下，别直接失败成「对话空白」。
    conn.busy_timeout(Duration::from_millis(500)).ok()?;
    Some(conn)
}

/// session_id → 所在库。session id 全局唯一，命中一次即可记住，免得每轮都去几个库里探。
static DB_OF: LazyLock<Mutex<HashMap<String, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 打开该会话所在的库。
pub(super) fn open_for_session(session_id: &str) -> Option<Connection> {
    if session_id.is_empty() {
        return None;
    }
    let cached = DB_OF.lock().ok().and_then(|m| m.get(session_id).cloned());
    if let Some(db) = cached.filter(|db| db.is_file()) {
        if let Some(conn) = open(&db) {
            return Some(conn);
        }
    }
    for db in data_dirs().into_iter().filter(|db| db.is_file()) {
        let Some(conn) = open(&db) else { continue };
        let found = conn
            .query_row("SELECT 1 FROM session WHERE id = ?1", [session_id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some();
        if found {
            if let Ok(mut m) = DB_OF.lock() {
                m.insert(session_id.to_string(), db);
            }
            return Some(conn);
        }
    }
    None
}

fn db_path_of(session_id: &str) -> Option<PathBuf> {
    DB_OF.lock().ok().and_then(|m| m.get(session_id).cloned())
}

/// 库的变化指纹：主库与 WAL 的 mtime。opencode 写入先落 WAL，只看主库会漏掉整段新内容。
fn db_stamp(db: &Path) -> (Option<SystemTime>, Option<SystemTime>) {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    (mtime(db), mtime(Path::new(&wal)))
}

/// 已导出到哪一条：(消息 id, 消息内键)。消息内键：用户消息整条一行用 `""`，assistant 的
/// part 用 part id（`prt_…`），回合错误用 `~error`——三者按字典序恰好就是写出顺序。
/// opencode 的 message/part id 都是按时间升序生成的，所以 (msg, key) 的字典序就是时间序。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Cursor {
    msg: String,
    key: String,
}

const USER_KEY: &str = "";
const ERROR_KEY: &str = "~error";

struct Exported {
    stamp: (Option<SystemTime>, Option<SystemTime>),
    path: PathBuf,
}

/// 本进程内已导出的指纹：库没动就直接返回缓存路径，不开库。对话页 650ms 轮询，这是热路径。
static EXPORTS: LazyLock<Mutex<HashMap<String, Exported>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 把会话导出（或增量追加）到 JSONL 缓存，返回缓存文件路径。库里找不到该会话返回 None。
pub(super) fn materialize(session_id: &str) -> Option<PathBuf> {
    let safe = safe_name(session_id)?;
    let conn = open_for_session(session_id)?;
    let db = db_path_of(session_id)?;
    // 指纹取在查询之前：查询期间的新写入会让下一轮指纹不同，不会被漏掉。
    let stamp = db_stamp(&db);
    if let Ok(exports) = EXPORTS.lock() {
        if let Some(hit) = exports
            .get(session_id)
            .filter(|e| e.stamp == stamp && e.path.is_file())
        {
            return Some(hit.path.clone());
        }
    }
    let dir = crate::meowo_data_root()?.join(CACHE_REL);
    std::fs::create_dir_all(&dir).ok()?;
    // dev 与安装版共享 ~/.meowo（见 AGENTS.md「多实例」），两边同时开着同一个对话就会并发
    // 追加同一份文件；文件锁把「读游标 → 查库 → 追加」整段串行化。
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{safe}.lock")))
        .ok()?;
    lock.lock().ok()?;
    let path = export_locked(&conn, session_id, &dir, &safe)?;
    if let Ok(mut exports) = EXPORTS.lock() {
        exports.insert(
            session_id.to_string(),
            Exported {
                stamp,
                path: path.clone(),
            },
        );
    }
    Some(path)
}

fn export_locked(conn: &Connection, session_id: &str, dir: &Path, safe: &str) -> Option<PathBuf> {
    let current = latest_generation(dir, safe);
    if let Some((gen, path)) = &current {
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let cursor = last_cursor(path);
        let resumable = match &cursor {
            Some(cursor) => message_exists(conn, &cursor.msg),
            // 空文件（还没导出过任何定稿内容）可以原地续写；非空却读不出游标＝文件坏了，重导。
            None => len == 0,
        };
        if resumable {
            let lines = collect_lines(conn, session_id, cursor.as_ref()).ok()?;
            append_lines(path, &lines).ok()?;
            return Some(path.clone());
        }
        let next = dir.join(format!("{safe}.{}.jsonl", gen + 1));
        let lines = collect_lines(conn, session_id, None).ok()?;
        write_fresh(&next, &lines).ok()?;
        let _ = std::fs::remove_file(path);
        return Some(next);
    }
    let path = dir.join(format!("{safe}.0.jsonl"));
    let lines = collect_lines(conn, session_id, None).ok()?;
    write_fresh(&path, &lines).ok()?;
    Some(path)
}

/// 文件名安全化：session id 直接拼进路径，过滤掉分隔符与点（点是代号分隔符）。
fn safe_name(session_id: &str) -> Option<String> {
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(96)
        .collect();
    (!safe.is_empty()).then_some(safe)
}

/// 找该会话最新一代缓存 `<safe>.<gen>.jsonl`。
fn latest_generation(dir: &Path, safe: &str) -> Option<(u64, PathBuf)> {
    let prefix = format!("{safe}.");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let gen = name
                .strip_prefix(&prefix)?
                .strip_suffix(".jsonl")?
                .parse::<u64>()
                .ok()?;
            Some((gen, entry.path()))
        })
        .max_by_key(|(gen, _)| *gen)
}

/// 读文件最后一行的游标。行可能很长（用户消息里内嵌的图片 base64），从尾部按倍增窗口往回找换行。
fn last_cursor(path: &Path) -> Option<Cursor> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len == 0 {
        return None;
    }
    let mut window: u64 = 64 * 1024;
    loop {
        let start = len.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buf = Vec::with_capacity((len - start) as usize);
        file.read_to_end(&mut buf).ok()?;
        let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
        match body.iter().rposition(|b| *b == b'\n') {
            Some(i) => return cursor_of(&body[i + 1..]),
            None if start == 0 => return cursor_of(body),
            None => window = window.saturating_mul(4),
        }
    }
}

fn cursor_of(line: &[u8]) -> Option<Cursor> {
    let value: Value = serde_json::from_slice(line).ok()?;
    Some(Cursor {
        msg: value.get("msg")?.as_str()?.to_string(),
        key: value.get("key")?.as_str()?.to_string(),
    })
}

fn message_exists(conn: &Connection, id: &str) -> bool {
    conn.query_row("SELECT 1 FROM message WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .ok()
        .flatten()
        .is_some()
}

fn append_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new().append(true).open(path)?;
    // 一次 write_all：读者按「最后一个完整换行」截断，半行不会被解析。
    file.write_all(lines.concat().as_bytes())
}

fn write_fresh(path: &Path, lines: &[String]) -> std::io::Result<()> {
    std::fs::write(path, lines.concat())
}

/// 一条消息：(id, 消息正文, 按 id 升序的 (part id, part 正文))。
type MessageRow = (String, Value, Vec<(String, Value)>);

/// 读会话的全部消息（按 id 升序 = 时间序）及其 part。
fn load_messages(
    conn: &Connection,
    session_id: &str,
    from_msg: &str,
) -> rusqlite::Result<Vec<MessageRow>> {
    let mut msgs = conn.prepare(
        "SELECT id, data FROM message WHERE session_id = ?1 AND id >= ?2 ORDER BY id",
    )?;
    let mut parts = conn.prepare("SELECT id, data FROM part WHERE message_id = ?1 ORDER BY id")?;
    let rows = msgs
        .query_map([session_id, from_msg], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, data) in rows {
        let msg: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
        let msg_parts = parts
            .query_map([&id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .filter_map(Result::ok)
            .map(|(pid, data)| (pid, serde_json::from_str(&data).unwrap_or(Value::Null)))
            .collect();
        out.push((id, msg, msg_parts));
    }
    Ok(out)
}

/// 对话页会消费的 part 类型。step-start/step-finish/snapshot/patch 只是 opencode 内部的回合
/// 记账，既不导出，也不参与「遇到未定稿即停」——否则一个永远没有 finish 的 step 会卡住后面一切。
fn exported_part(part: &Value) -> bool {
    matches!(
        part.get("type").and_then(Value::as_str),
        Some("text" | "reasoning" | "tool")
    )
}

/// part 自身是否已定稿：文本/思考看 `time.end`，工具看终态。
fn part_settled(part: &Value) -> bool {
    match part.get("type").and_then(Value::as_str) {
        Some("tool") => matches!(
            part.pointer("/state/status").and_then(Value::as_str),
            Some("completed" | "error")
        ),
        _ => part.pointer("/time/end").is_some(),
    }
}

/// 从游标之后收集「已定稿前缀」，每条一行（含换行）。
fn collect_lines(
    conn: &Connection,
    session_id: &str,
    after: Option<&Cursor>,
) -> rusqlite::Result<Vec<String>> {
    let from = after.map(|c| c.msg.as_str()).unwrap_or("");
    let messages = load_messages(conn, session_id, from)?;
    let total = messages.len();
    let mut lines = Vec::new();
    let mut push = |line: Value| lines.push(format!("{line}\n"));
    let skip = |key: &Cursor| after.is_some_and(|after| key <= after);
    for (index, (msg_id, msg, parts)) in messages.into_iter().enumerate() {
        let created = msg.pointer("/time/created").and_then(Value::as_i64);
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        // 消息整体定稿：有了 completed 时刻，或后面已经有新消息——后者兜住进程被杀、中断后
        // 再也不会补完的消息（实测：「继续」之前那条 assistant 消息永远停在 running 的 bash 上），
        // 不然它会永远卡住其后全部内容的导出。
        let has_later = index + 1 < total;
        let msg_done = msg.pointer("/time/completed").is_some() || has_later;
        if role == "user" {
            let key = Cursor {
                msg: msg_id.clone(),
                key: USER_KEY.into(),
            };
            if skip(&key) {
                continue;
            }
            // part 是紧跟着消息行写入的：一条 part 都还没有时等下一轮，免得把空消息定成稿。
            if parts.is_empty() && !has_later {
                break;
            }
            let parts: Vec<Value> = parts.into_iter().map(|(_, p)| slim_user_part(p)).collect();
            push(json!({"kind": "user", "msg": msg_id, "key": key.key, "time": created, "parts": parts}));
            continue;
        }
        let summary = msg.get("summary").and_then(Value::as_bool).unwrap_or(false);
        for (part_id, part) in parts {
            if !exported_part(&part) {
                continue;
            }
            let key = Cursor {
                msg: msg_id.clone(),
                key: part_id,
            };
            if skip(&key) {
                continue;
            }
            if !msg_done && !part_settled(&part) {
                return Ok(lines);
            }
            let time = part
                .pointer("/time/start")
                .or_else(|| part.pointer("/state/time/start"))
                .and_then(Value::as_i64)
                .or(created);
            push(json!({
                "kind": "part",
                "msg": msg_id,
                "key": key.key,
                "time": time,
                "summary": summary,
                "part": slim_part(part),
            }));
        }
        if !msg_done {
            return Ok(lines);
        }
        if let Some(error) = msg.get("error").filter(|e| !e.is_null()) {
            let key = Cursor {
                msg: msg_id.clone(),
                key: ERROR_KEY.into(),
            };
            if !skip(&key) {
                let time = msg.pointer("/time/completed").and_then(Value::as_i64).or(created);
                push(json!({"kind": "error", "msg": msg_id, "key": key.key, "time": time, "error": error}));
            }
        }
    }
    Ok(lines)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        let mut s: String = text.chars().take(max).collect();
        s.push('…');
        s
    } else {
        text.to_string()
    }
}

/// 工具 part 只留对话页用得到的字段：`state.metadata` 里有整份 diff/预览，体积远大于正文。
fn slim_part(part: Value) -> Value {
    if part.get("type").and_then(Value::as_str) != Some("tool") {
        return part;
    }
    let state = part.get("state");
    let field = |key: &str| state.and_then(|s| s.get(key)).cloned().unwrap_or(Value::Null);
    let output = state
        .and_then(|s| s.get("output"))
        .and_then(Value::as_str)
        .map(|s| Value::String(clip(s, TOOL_OUTPUT_MAX_CHARS)))
        .unwrap_or(Value::Null);
    json!({
        "type": "tool",
        "tool": part.get("tool"),
        "callID": part.get("callID"),
        "state": {
            "status": field("status"),
            "title": field("title"),
            "input": field("input"),
            "output": output,
            "error": field("error"),
        },
    })
}

/// 用户消息的 file part 只有内嵌图片（粘贴截图）值得留正文；其余附件的 url 可能是整份
/// 文件的 data URL，去掉以免缓存膨胀。
fn slim_user_part(mut part: Value) -> Value {
    if part.get("type").and_then(Value::as_str) == Some("file") {
        let keep = part
            .get("mime")
            .and_then(Value::as_str)
            .is_some_and(|m| m.starts_with("image/"))
            && part
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(|u| u.starts_with("data:"));
        if !keep {
            if let Some(obj) = part.as_object_mut() {
                obj.remove("url");
            }
        }
    }
    part
}

/// Stop 时的最近 AI 正文与模型：最后一条用户消息之后、最新一条带正文的 assistant 消息。
pub(super) fn last_reply(session_id: &str) -> Option<(Option<String>, Option<String>)> {
    let conn = open_for_session(session_id)?;
    let mut stmt = conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY id DESC LIMIT 64")
        .ok()?;
    let rows: Vec<(String, Value)> = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?
        .filter_map(Result::ok)
        .map(|(id, data)| (id, serde_json::from_str(&data).unwrap_or(Value::Null)))
        .collect();
    let mut model = None;
    for (id, msg) in rows {
        if msg.get("role").and_then(Value::as_str) != Some("assistant") {
            break;
        }
        if model.is_none() {
            model = msg.get("modelID").and_then(Value::as_str).map(str::to_string);
        }
        if msg.get("summary").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let text = message_text(&conn, &id);
        if !text.is_empty() {
            return Some((Some(text), model));
        }
    }
    Some((None, model))
}

fn message_text(conn: &Connection, msg_id: &str) -> String {
    let Ok(mut stmt) = conn.prepare("SELECT data FROM part WHERE message_id = ?1 ORDER BY id") else {
        return String::new();
    };
    let Ok(rows) = stmt.query_map([msg_id], |row| row.get::<_, String>(0)) else {
        return String::new();
    };
    rows.filter_map(Result::ok)
        .filter_map(|data| serde_json::from_str::<Value>(&data).ok())
        .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
        .filter(|p| !p.get("synthetic").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|p| p.get("text").and_then(Value::as_str).map(str::trim).map(str::to_string))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 当前待办快照（opencode 的 todowrite 工具整表覆写 `todo` 表）。会话不在任何库里返回 None。
pub(super) fn todos(session_id: &str) -> Option<Vec<(String, String)>> {
    let conn = open_for_session(session_id)?;
    let mut stmt = conn
        .prepare("SELECT content, status FROM todo WHERE session_id = ?1 ORDER BY position")
        .ok()?;
    let rows = stmt
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?
        .filter_map(Result::ok)
        .collect();
    Some(rows)
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    /// 建一个与 opencode 1.19.3 同构（只含用到的列）的内存库。
    pub fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE session (id TEXT PRIMARY KEY);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, data TEXT);
             INSERT INTO session VALUES ('ses_1');",
        )
        .unwrap();
        conn
    }

    pub fn msg(conn: &Connection, id: &str, data: Value) {
        conn.execute(
            "INSERT OR REPLACE INTO message VALUES (?1, 'ses_1', ?2)",
            rusqlite::params![id, data.to_string()],
        )
        .unwrap();
    }

    pub fn part(conn: &Connection, id: &str, msg: &str, data: Value) {
        conn.execute(
            "INSERT OR REPLACE INTO part VALUES (?1, ?2, 'ses_1', ?3)",
            rusqlite::params![id, msg, data.to_string()],
        )
        .unwrap();
    }

    pub fn lines(conn: &Connection, after: Option<(&str, &str)>) -> Vec<String> {
        let after = after.map(|(msg, key)| Cursor {
            msg: msg.into(),
            key: key.into(),
        });
        collect_lines(conn, "ses_1", after.as_ref()).unwrap()
    }

    pub fn cursor(line: &str) -> (String, String) {
        let c = cursor_of(line.trim_end().as_bytes()).unwrap();
        (c.msg, c.key)
    }

    pub fn export(conn: &Connection, dir: &Path) -> PathBuf {
        export_locked(conn, "ses_1", dir, "ses_1").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn seed_turn(conn: &Connection) {
        msg(conn, "msg_1", json!({"role": "user", "time": {"created": 1}}));
        part(conn, "prt_1", "msg_1", json!({"type": "text", "text": "看看云端 bot"}));
        msg(conn, "msg_2", json!({"role": "assistant", "time": {"created": 2}}));
        part(conn, "prt_2", "msg_2", json!({"type": "step-start"}));
        part(conn, "prt_3", "msg_2", json!({"type": "reasoning", "text": "先搜", "time": {"start": 3, "end": 4}}));
        part(conn, "prt_4", "msg_2", json!({"type": "tool", "tool": "bash", "callID": "c1",
            "state": {"status": "running", "input": {"command": "ls"}}}));
        part(conn, "prt_5", "msg_2", json!({"type": "text", "text": "好的", "time": {"start": 5}}));
    }

    #[test]
    fn exports_only_the_settled_prefix() {
        let conn = memory_db();
        seed_turn(&conn);
        let lines = lines(&conn, None);
        // 用户消息 + 已结束的思考；运行中的 bash 卡住其后的一切（step-start 不算）。
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(cursor(&lines[1]), ("msg_2".into(), "prt_3".into()));
    }

    #[test]
    fn later_message_settles_an_abandoned_one() {
        let conn = memory_db();
        seed_turn(&conn);
        // 中断后用户又发了一句：前一条 assistant 再也不会补完，必须整条放行。
        msg(&conn, "msg_3", json!({"role": "user", "time": {"created": 9}}));
        part(&conn, "prt_9", "msg_3", json!({"type": "text", "text": "继续"}));
        let all = lines(&conn, None);
        assert_eq!(all.len(), 5, "{all:?}");
        // 增量：从游标之后只拿新增。
        let tail = lines(&conn, Some(("msg_2", "prt_3")));
        assert_eq!(tail.len(), 3);
        assert_eq!(cursor(&tail[2]), ("msg_3".into(), USER_KEY.into()));
    }

    #[test]
    fn turn_error_follows_its_message_parts() {
        let conn = memory_db();
        msg(&conn, "msg_1", json!({"role": "assistant", "time": {"created": 1, "completed": 2},
            "error": {"name": "APIError", "data": {"message": "boom"}}}));
        part(&conn, "prt_1", "msg_1", json!({"type": "text", "text": "半句", "time": {"start": 1}}));
        let lines = lines(&conn, None);
        assert_eq!(lines.len(), 2);
        assert_eq!(cursor(&lines[1]), ("msg_1".into(), ERROR_KEY.into()));
    }

    #[test]
    fn slim_tool_part_drops_metadata_and_clips_output() {
        let big = "x".repeat(TOOL_OUTPUT_MAX_CHARS + 10);
        let slim = slim_part(json!({"type": "tool", "tool": "edit", "callID": "c",
            "state": {"status": "completed", "input": {}, "output": big, "metadata": {"diff": "…"}}}));
        assert!(slim.pointer("/state/metadata").is_none());
        let out = slim.pointer("/state/output").and_then(Value::as_str).unwrap();
        assert_eq!(out.chars().count(), TOOL_OUTPUT_MAX_CHARS + 1);
    }

    #[test]
    fn export_appends_then_regenerates_after_undo() {
        let dir = std::env::temp_dir().join(format!("meowo_oc_export_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = memory_db();
        seed_turn(&conn);
        let first = export(&conn, &dir);
        assert!(first.ends_with("ses_1.0.jsonl"));
        let len0 = std::fs::metadata(&first).unwrap().len();

        // 工具跑完、文本写完 → 原地追加，前缀字节不变（宿主的偏移因此仍然有效）。
        part(&conn, "prt_4", "msg_2", json!({"type": "tool", "tool": "bash", "callID": "c1",
            "state": {"status": "completed", "input": {"command": "ls"}, "output": "a"}}));
        part(&conn, "prt_5", "msg_2", json!({"type": "text", "text": "好的", "time": {"start": 5, "end": 6}}));
        let same = export(&conn, &dir);
        assert_eq!(same, first);
        let text = std::fs::read_to_string(&same).unwrap();
        assert!(text.len() as u64 > len0);
        assert_eq!(text.lines().count(), 4);

        // /undo 删掉尾部消息 → 游标失效，重导到下一代文件，旧代删除。
        conn.execute("DELETE FROM part WHERE message_id = 'msg_2'", []).unwrap();
        conn.execute("DELETE FROM message WHERE id = 'msg_2'", []).unwrap();
        let next = export(&conn, &dir);
        assert!(next.ends_with("ses_1.1.jsonl"), "{next:?}");
        assert!(!first.exists());
        assert_eq!(std::fs::read_to_string(&next).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_cursor_reads_lines_longer_than_the_first_window() {
        let dir = std::env::temp_dir().join(format!("meowo_oc_cursor_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.jsonl");
        let long = "y".repeat(300 * 1024);
        let body = format!(
            "{}\n{}\n",
            json!({"msg": "m1", "key": ""}),
            json!({"msg": "m2", "key": "", "pad": long})
        );
        std::fs::write(&path, body).unwrap();
        assert_eq!(
            last_cursor(&path),
            Some(Cursor {
                msg: "m2".into(),
                key: "".into()
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
