//! agy 对话库（每个对话一个 SQLite）的只读访问，以及把它物化成 JSONL 缓存。
//!
//! agy 的对话存在 `<数据目录>/conversations/<对话 id>.db`，`steps` 表一行一个 step，正文是
//! protobuf（见 proto.rs）。宿主的对话页管线（增量偏移、翻页、mtime 失效）建立在「只追加的
//! JSONL 文件」上，所以照 opencode 的做法在插件里单向导出成同形文件，宿主不另开读库路径。
//!
//! 只追加的前提是**只导出已定稿的 step**：回复行先以 GENERATING 插入、生成完才写正文；工具行
//! RUNNING → WAITING（等审批）→ DONE/ERROR，都是原地更新（research-transcript.md §4 的只读轮询
//! 时序）。按 idx 升序扫，遇到第一个未定稿的就停，后面的等下一轮。例外：之后又出现了用户消息，
//! 说明那一行再也不会定稿（回合被打断），照导出、标成被打断，免得整份缓存永远卡在那里。
//!
//! 已导出的 idx 在库里消失（/rewind 截断）或变短时，整份重导到**新一代文件名**，宿主按「路径
//! 变了」重置前端，不会把旧偏移套到新内容上。
//!
//! 库是 WAL 模式。只读连接在 agy 运行时可以正常读，一致快照约 15ms，没有遇到 SQLITE_BUSY；每次
//! 读完就关，不长期持有读事务（否则挡住 agy 的检查点，WAL 一直涨）。

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

use super::proto::{self, status, step_type, Step};

/// 缓存格式版本进目录名：行结构一变，旧缓存整体作废，不必写迁移。
const CACHE_REL: &str = "cache/antigravity/v1";

/// 工具输出的落盘上限（字符）。命令和读文件的输出动辄几十 KB，对话页只做预览。
const TOOL_OUTPUT_MAX_CHARS: usize = 16_000;

/// agy 的数据目录。agy 没有覆盖数据目录的环境变量（1.5.17 二进制里 `GEMINI_CLI_HOME` 零命中），
/// 固定在家目录下。
pub(super) fn data_dir() -> Option<PathBuf> {
    Some(crate::join_rel(&crate::home_dir()?, super::DATA_DIR_REL))
}

fn db_path(session_id: &str) -> Option<PathBuf> {
    let safe = safe_name(session_id)?;
    // 对话 id 就是库文件名（= trajectory_meta.cascade_id）。
    let path = data_dir()?.join("conversations").join(format!("{safe}.db"));
    path.is_file().then_some(path)
}

fn open(db: &Path) -> Option<Connection> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(Duration::from_millis(500)).ok()?;
    Some(conn)
}

/// 库的变化指纹：主库与 WAL 的 mtime。agy 写入先落 WAL，只看主库会漏掉整段新内容。
fn db_stamp(db: &Path) -> (Option<SystemTime>, Option<SystemTime>) {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    (mtime(db), mtime(Path::new(&wal)))
}

/// 文件名安全化：对话 id 是 uuid，直接拼进路径前过滤掉分隔符与点（点是代号分隔符）。
fn safe_name(session_id: &str) -> Option<String> {
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(96)
        .collect();
    (!safe.is_empty() && safe.len() == session_id.len()).then_some(safe)
}

struct Exported {
    stamp: (Option<SystemTime>, Option<SystemTime>),
    path: PathBuf,
}

/// 本进程内已导出的指纹：库没动就直接返回缓存路径，不开库。对话页 650ms 轮询，这是热路径。
static EXPORTS: LazyLock<Mutex<HashMap<String, Exported>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 把对话导出（或增量追加）到 JSONL 缓存，返回缓存文件路径。库不存在返回 None。
pub(super) fn materialize(session_id: &str) -> Option<PathBuf> {
    let db = db_path(session_id)?;
    let safe = safe_name(session_id)?;
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
    let conn = open(&db)?;
    let dir = crate::meowo_data_root()?.join(CACHE_REL);
    std::fs::create_dir_all(&dir).ok()?;
    // dev 与安装版共享 ~/.meowo（AGENTS.md「多实例」），两边同时开着同一个对话会并发追加同一份
    // 文件；文件锁把「读游标 → 查库 → 追加」整段串行化。
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{safe}.lock")))
        .ok()?;
    lock.lock().ok()?;
    let path = export_locked(&conn, &dir, &safe)?;
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

fn export_locked(conn: &Connection, dir: &Path, safe: &str) -> Option<PathBuf> {
    let current = latest_generation(dir, safe);
    if let Some((gen, path)) = &current {
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let cursor = last_cursor(path);
        let resumable = match cursor {
            // 游标那一行还在：原地续写。不在了（/rewind 截掉了尾部）：作废重导。
            Some(idx) => step_exists(conn, idx),
            // 空文件可以原地续写；非空却读不出游标 = 文件坏了，重导。
            None => len == 0,
        };
        if resumable {
            let lines = collect_lines(conn, cursor).ok()?;
            append_lines(path, &lines).ok()?;
            return Some(path.clone());
        }
        let next = dir.join(format!("{safe}.{}.jsonl", gen + 1));
        let lines = collect_lines(conn, None).ok()?;
        std::fs::write(&next, lines.concat()).ok()?;
        let _ = std::fs::remove_file(path);
        return Some(next);
    }
    let path = dir.join(format!("{safe}.0.jsonl"));
    let lines = collect_lines(conn, None).ok()?;
    std::fs::write(&path, lines.concat()).ok()?;
    Some(path)
}

/// 找该对话最新一代缓存 `<safe>.<gen>.jsonl`。
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

/// 读文件最后一行的游标（step idx）。行可能很长（工具输出），从尾部按倍增窗口往回找换行。
fn last_cursor(path: &Path) -> Option<i64> {
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

fn cursor_of(line: &[u8]) -> Option<i64> {
    serde_json::from_slice::<Value>(line).ok()?.get("idx")?.as_i64()
}

fn step_exists(conn: &Connection, idx: i64) -> bool {
    conn.query_row("SELECT 1 FROM steps WHERE idx = ?1", [idx], |_| Ok(()))
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

/// 游标之后的 step（按 idx 升序），解码失败的行跳过（视同不存在）。
fn load_steps(conn: &Connection, after: Option<i64>) -> rusqlite::Result<Vec<(i64, Step)>> {
    let mut stmt =
        conn.prepare("SELECT idx, step_payload FROM steps WHERE idx > ?1 ORDER BY idx")?;
    let rows = stmt
        .query_map([after.unwrap_or(-1)], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })?
        .filter_map(Result::ok)
        .filter_map(|(idx, blob)| Some((idx, proto::decode::<Step>(&blob?)?)))
        .collect();
    Ok(rows)
}

/// 每次模型调用的模型名与上下文占用，按它产出的 step idx 索引。
#[derive(Debug, Clone, Default, PartialEq)]
struct GenInfo {
    model: Option<String>,
    context_tokens: Option<u64>,
    context_window: Option<u64>,
}

/// 读 `gen_metadata`，覆盖 idx ≥ `from` 的 step。从最新一行往回读，读到一行产出的 step 全在
/// `from` 之前就停：每回合最后一次调用那一行带约 115KB 的系统提示，增量导出不能每轮全表解码。
fn load_gen_infos(conn: &Connection, from: i64) -> HashMap<i64, GenInfo> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT data FROM gen_metadata ORDER BY idx DESC") else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, Option<Vec<u8>>>(0)) else {
        return out;
    };
    for blob in rows.filter_map(Result::ok).flatten() {
        let Some(meta) = proto::decode::<proto::GeneratorMetadata>(&blob) else {
            continue;
        };
        if meta
            .step_indices
            .iter()
            .all(|idx| i64::from(*idx) < from)
            && !meta.step_indices.is_empty()
        {
            break;
        }
        let Some(model) = meta.chat_model else {
            continue;
        };
        let window = model
            .chat_start_metadata
            .and_then(|start| start.context_window_metadata);
        let info = GenInfo {
            model: Some(model.model_display_name)
                .filter(|s| !s.is_empty())
                .or(Some(model.response_model).filter(|s| !s.is_empty())),
            context_tokens: window
                .as_ref()
                .map(|w| w.estimated_tokens_used)
                .filter(|v| *v > 0)
                .map(|v| v as u64),
            context_window: window
                .and_then(|w| w.max_context_tokens)
                .filter(|v| *v > 0)
                .map(|v| v as u64),
        };
        for idx in meta.step_indices {
            out.entry(i64::from(idx)).or_insert_with(|| info.clone());
        }
    }
    out
}

/// 游标之后、已定稿前缀内的 step → JSONL 行（每行以换行结尾）。
fn collect_lines(conn: &Connection, after: Option<i64>) -> rusqlite::Result<Vec<String>> {
    let steps = load_steps(conn, after)?;
    // 定稿前缀：遇到第一个未定稿的就停。之后若还有用户消息，那一行是被打断后遗留的，照导出。
    let last_user = steps
        .iter()
        .rposition(|(_, s)| s.r#type == step_type::USER_INPUT);
    let mut end = steps.len();
    for (i, (_, step)) in steps.iter().enumerate() {
        if !status::settled(step.status) && last_user.is_none_or(|u| u <= i) {
            end = i;
            break;
        }
    }
    let steps = &steps[..end];
    let from = steps.first().map_or(i64::MAX, |(idx, _)| *idx);
    let gens = if steps
        .iter()
        .any(|(_, s)| s.r#type == step_type::PLANNER_RESPONSE)
    {
        load_gen_infos(conn, from)
    } else {
        HashMap::new()
    };
    Ok(steps
        .iter()
        .filter_map(|(idx, step)| step_line(*idx, step, gens.get(idx)))
        .map(|line| format!("{line}\n"))
        .collect())
}

fn ts_of(step: &Step) -> Option<i64> {
    step.metadata
        .as_ref()?
        .created_at
        .as_ref()
        .map(proto::Timestamp::millis)
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// 工具结果文本开头固定两行 `Created At:` / `Completed At:`（并行调用时会重复一遍），是给模型
/// 看的时间戳，对话页不显示。
fn strip_result_header(text: &str) -> &str {
    let mut rest = text;
    loop {
        let trimmed = rest.trim_start_matches(['\r', '\n']);
        let Some(line_end) = trimmed.find('\n') else {
            return if trimmed.starts_with("Created At:") || trimmed.starts_with("Completed At:") {
                ""
            } else {
                trimmed
            };
        };
        let line = &trimmed[..line_end];
        if line.starts_with("Created At:") || line.starts_with("Completed At:") {
            rest = &trimmed[line_end + 1..];
        } else {
            return trimmed;
        }
    }
}

/// 一个 step → 一行。对话页不显示的（隐藏的系统提示、没有内容的回复）返回 None——但游标要
/// 前进，否则下一轮又从同一处开始扫，所以统一产出一行 `skip`。
fn step_line(idx: i64, step: &Step, gen: Option<&GenInfo>) -> Option<Value> {
    let ts = ts_of(step);
    let settled = status::settled(step.status);
    let line = match step.r#type {
        step_type::USER_INPUT => step.user_input.as_ref().map(|input| {
            let text = if input.user_response.trim().is_empty() {
                input
                    .items
                    .iter()
                    .map(|item| item.text.as_str())
                    .collect::<String>()
            } else {
                input.user_response.clone()
            };
            let images: Vec<Value> = input
                .images
                .iter()
                .filter(|img| !img.base64_data.is_empty())
                .map(|img| json!({"mime": img.mime_type, "data": img.base64_data}))
                .collect();
            json!({
                "idx": idx, "kind": "user", "ts": ts, "text": text,
                "queued": input.is_queued_message, "images": images,
            })
        }),
        step_type::PLANNER_RESPONSE => step.planner_response.as_ref().map(|resp| {
            let text = if resp.modified_response.is_empty() {
                &resp.response
            } else {
                &resp.modified_response
            };
            let calls: Vec<Value> = resp
                .tool_calls
                .iter()
                .map(|call| json!({"id": call.id, "name": call.name}))
                .collect();
            json!({
                "idx": idx, "kind": "assistant", "ts": ts,
                "text": text, "thinking": resp.thinking, "tool_calls": calls,
                "model": gen.and_then(|g| g.model.clone()),
                "context_tokens": gen.and_then(|g| g.context_tokens),
                "context_window": gen.and_then(|g| g.context_window),
            })
        }),
        step_type::GENERIC => Some(tool_line(idx, step, ts, settled)),
        step_type::ERROR_MESSAGE => step.error_message.as_ref().map(|msg| {
            let error = msg.error.as_ref();
            json!({"idx": idx, "kind": "error", "ts": ts, "text": error.map(error_text).unwrap_or_default()})
        }),
        step_type::CHECKPOINT => Some(json!({"idx": idx, "kind": "compact", "ts": ts})),
        step_type::INVOKE_SUBAGENT => step.invoke_subagent.as_ref().map(|sub| {
            json!({
                "idx": idx, "kind": "subagent", "ts": ts, "name": sub.subagent_name,
                "prompt": clip(&sub.prompt, TOOL_OUTPUT_MAX_CHARS), "conversation": sub.conversation_id,
                "status": status_word(step.status, settled),
            })
        }),
        step_type::SYSTEM_MESSAGE => step
            .system_message
            .as_ref()
            .filter(|msg| !msg.render_info.as_ref().is_some_and(|r| r.hidden))
            .filter(|msg| !msg.message.trim().is_empty())
            .map(|msg| json!({"idx": idx, "kind": "system", "ts": ts, "text": clip(&msg.message, TOOL_OUTPUT_MAX_CHARS)})),
        _ => None,
    };
    Some(line.unwrap_or_else(|| json!({"idx": idx, "kind": "skip"})))
}

fn error_text(error: &proto::ErrorDetails) -> String {
    [
        &error.user_error_message,
        &error.model_error_message,
        &error.short_error,
        &error.full_error,
    ]
    .into_iter()
    .find(|s| !s.trim().is_empty())
    .cloned()
    .unwrap_or_default()
}

/// 终态 → 行里的状态词。未定稿却被导出（后面又有用户消息）的算被打断。
fn status_word(code: i32, settled: bool) -> &'static str {
    match code {
        _ if !settled => "interrupted",
        status::DONE => "done",
        status::ERROR => "error",
        status::CANCELED | status::INTERRUPTED | status::CLEARED => "interrupted",
        _ => "done",
    }
}

fn tool_line(idx: i64, step: &Step, ts: Option<i64>, settled: bool) -> Value {
    let meta = step.metadata.as_ref();
    let call = meta.and_then(|m| m.tool_call.as_ref());
    let generic = step.generic.as_ref();
    let result = generic.and_then(|g| g.result.as_ref());
    let render = result.and_then(|r| r.step_render_info.as_ref());
    // 参数优先取原始 JSON（数字、布尔不失真）；缺席时退回字符串化的 map。
    let args = call
        .and_then(|c| serde_json::from_str::<Value>(&c.arguments_json).ok())
        .or_else(|| generic.map(|g| json!(g.args)))
        .unwrap_or(Value::Null);
    let terminal = render.and_then(|r| r.content_blocks.iter().find_map(|b| b.terminal.as_ref()));
    let title = render.map(|r| {
        let parts: String = r.title_parts.iter().map(|p| p.text.as_str()).collect();
        let body = if parts.trim().is_empty() { r.title.clone() } else { parts };
        [r.title_prefix.as_str(), body.trim()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    });
    let status = status_word(step.status, settled);
    let output = result.map(|r| strip_result_header(&r.result)).unwrap_or_default();
    json!({
        "idx": idx, "kind": "tool", "ts": ts,
        "end": meta.and_then(|m| m.completed_at.as_ref()).map(proto::Timestamp::millis),
        "id": call.map(|c| c.id.as_str()).filter(|s| !s.is_empty()).map_or_else(|| format!("step-{idx}"), str::to_string),
        "name": call.map(|c| c.name.as_str()).unwrap_or("tool"),
        "args": args,
        "status": status,
        "output": clip(output, TOOL_OUTPUT_MAX_CHARS),
        "error": step.error.as_ref().map(error_text).filter(|s| !s.is_empty()),
        "title": title.filter(|t| !t.is_empty()),
        "exit_code": terminal.and_then(|t| t.exit_code),
    })
}

/// 对话标题与会话列表信息都在 `conversation_summaries.db`（agy 自己生成；还没起好是空串）。
pub(super) fn conversation_title(session_id: &str) -> Option<String> {
    let path = data_dir()?.join("conversation_summaries.db");
    let conn = open(&path)?;
    let title: String = conn
        .query_row(
            "SELECT title FROM conversation_summaries WHERE conversation_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .ok()??;
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// Stop 时的最近 AI 正文与模型：最后一条用户消息之后最新一条带正文的回复。
pub(super) fn last_reply(session_id: &str) -> Option<(Option<String>, Option<String>)> {
    let conn = open(&db_path(session_id)?)?;
    let mut stmt = conn
        .prepare("SELECT idx, step_payload FROM steps ORDER BY idx DESC LIMIT 200")
        .ok()?;
    let rows: Vec<(i64, Step)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?))
        })
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|(idx, blob)| Some((idx, proto::decode::<Step>(&blob?)?)))
        .collect();
    let mut text = None;
    let mut reply_idx = None;
    for (idx, step) in &rows {
        if step.r#type == step_type::USER_INPUT {
            break;
        }
        if let Some(resp) = &step.planner_response {
            let body = if resp.modified_response.is_empty() {
                &resp.response
            } else {
                &resp.modified_response
            };
            if !body.trim().is_empty() {
                text = Some(body.trim().to_string());
                reply_idx = Some(*idx);
                break;
            }
        }
    }
    let model = reply_idx
        .or_else(|| rows.first().map(|(idx, _)| *idx))
        .and_then(|idx| load_gen_infos(&conn, idx).remove(&idx))
        .and_then(|info| info.model);
    Some((text, model))
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    use prost::Message;

    /// 建一个与 agy 1.5.17 同构（只含用到的列）的内存库。
    pub fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE steps (idx integer PRIMARY KEY, step_type integer NOT NULL DEFAULT 0,
                 status integer NOT NULL DEFAULT 0, step_payload blob);
             CREATE TABLE gen_metadata (idx integer PRIMARY KEY, data blob, size integer NOT NULL DEFAULT 0);",
        )
        .unwrap();
        conn
    }

    pub fn put(conn: &Connection, idx: i64, step: &Step) {
        conn.execute(
            "INSERT OR REPLACE INTO steps (idx, step_type, status, step_payload) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![idx, step.r#type, step.status, step.encode_to_vec()],
        )
        .unwrap();
    }

    pub fn put_gen(conn: &Connection, idx: i64, meta: &proto::GeneratorMetadata) {
        conn.execute(
            "INSERT OR REPLACE INTO gen_metadata (idx, data) VALUES (?1, ?2)",
            rusqlite::params![idx, meta.encode_to_vec()],
        )
        .unwrap();
    }

    pub fn lines(conn: &Connection, after: Option<i64>) -> Vec<Value> {
        collect_lines(conn, after)
            .unwrap()
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    pub fn export(conn: &Connection, dir: &Path) -> PathBuf {
        export_locked(conn, dir, "conv-1").unwrap()
    }
}

#[cfg(test)]
mod tests;
