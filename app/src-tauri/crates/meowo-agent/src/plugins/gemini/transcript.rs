//! Gemini 0.65 本机 ChatRecordingService / createJsonlRecordAccumulator 取证：同 id 是覆盖，
//! $patch/$rewindTo 会修改已有历史。先归并成稳定快照，避免把原生日志的更新误当新消息。

use crate::transcript::{
    preview_text, TranscriptEvent, TranscriptInfo, TranscriptParser, TranscriptSpec, TurnError,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant, SystemTime},
};

pub(super) static TRANSCRIPT: GeminiTranscript = GeminiTranscript;
pub(super) struct GeminiTranscript;

#[derive(Default)]
struct Journal {
    metadata: Value,
    messages: Vec<Value>,
    positions: HashMap<String, usize>,
}

impl Journal {
    fn upsert(&mut self, value: Value) {
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            return;
        };
        if let Some(&index) = self.positions.get(id) {
            self.messages[index] = value;
        } else {
            self.positions.insert(id.to_string(), self.messages.len());
            self.messages.push(value);
        }
    }

    fn reindex(&mut self) {
        self.positions = self
            .messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                message
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|id| (id.to_string(), index))
            })
            .collect();
    }

    fn metadata(&mut self, value: &Value) {
        if !self.metadata.is_object() {
            self.metadata = json!({});
        }
        if let Some(fields) = value.as_object() {
            for (key, value) in fields {
                if key != "messages" {
                    self.metadata[key] = value.clone();
                }
            }
        }
        if let Some(messages) = value.get("messages").and_then(Value::as_array) {
            self.messages.clear();
            self.positions.clear();
            for message in messages {
                self.upsert(message.clone());
            }
        }
    }

    fn patch(&mut self, patch: &Value) {
        let Some(index) = patch
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| self.positions.get(id))
            .copied()
        else {
            return;
        };
        let message = &mut self.messages[index];
        if let Some(content) = patch.get("content") {
            message["content"] = content.clone();
        }
        if let (Some(updates), Some(calls)) = (
            patch.get("toolCalls").and_then(Value::as_array),
            message.get_mut("toolCalls").and_then(Value::as_array_mut),
        ) {
            for update in updates {
                if let Some(call) = calls.iter_mut().find(|call| call["id"] == update["id"]) {
                    if let Some(result) = update.get("result") {
                        call["result"] = result.clone();
                    }
                }
            }
        }
    }

    fn fold(&mut self, value: Value) {
        if let Some(id) = value.get("$rewindTo").and_then(Value::as_str) {
            let end = self
                .messages
                .iter()
                .position(|message| message["id"] == id)
                .unwrap_or(0);
            self.messages.truncate(end);
            self.reindex();
        } else if let Some(patch) = value.get("$patch") {
            if patch.get("id").is_some() {
                self.patch(patch);
            }
            if let Some(updates) = patch.get("updates").and_then(Value::as_array) {
                for update in updates {
                    self.patch(update);
                }
            }
            if let Some(ids) = patch.get("removeIds").and_then(Value::as_array) {
                self.messages
                    .retain(|message| !ids.contains(&message["id"]));
            }
            if let Some(ids) = patch.get("orderIds").and_then(Value::as_array) {
                let mut suffix = Vec::new();
                for id in ids {
                    if let Some(index) = self
                        .messages
                        .iter()
                        .position(|message| &message["id"] == id)
                    {
                        suffix.push(self.messages.remove(index));
                    }
                }
                self.messages.extend(suffix);
            }
            self.reindex();
        } else if let Some(update) = value.get("$set") {
            self.metadata(update);
        } else if value.get("id").and_then(Value::as_str).is_some()
            && value.get("type").and_then(Value::as_str).is_some()
        {
            self.upsert(value);
        } else {
            self.metadata(&value);
        }
    }

    fn snapshot(&self) -> String {
        // lastUpdated 每个消息都变，不能让它迫使所有正常追加都重建快照。
        let mut text =
            json!({"gemini_metadata": {"summary": self.metadata.get("summary")}}).to_string();
        text.push('\n');
        for message in &self.messages {
            text.push_str(&message.to_string());
            text.push('\n');
        }
        text
    }
}

#[derive(Default)]
struct Cached {
    stamp: Option<(u64, Option<SystemTime>, Option<SystemTime>)>,
    source_hash: Sha256,
    offset: u64,
    journal: Journal,
    snapshot: String,
    path: PathBuf,
}
static CACHE: LazyLock<Mutex<HashMap<PathBuf, Cached>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
type Locations = HashMap<(PathBuf, String), (Instant, Option<PathBuf>)>;
static LOCATIONS: LazyLock<Mutex<Locations>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn prefix_matches(file: &mut fs::File, length: u64, expected: &Sha256) -> Option<bool> {
    let mut remaining = length;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let capacity = remaining.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..capacity]).ok()?;
        if read == 0 {
            return Some(false);
        }
        hash.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Some(hash.finalize() == expected.clone().finalize())
}

fn materialize(source: &Path) -> Option<PathBuf> {
    let dir = crate::meowo_data_root()?.join("cache/gemini/v1");
    materialize_in(source, &dir)
}

fn materialize_in(source: &Path, dir: &Path) -> Option<PathBuf> {
    let metadata = fs::metadata(source).ok()?;
    let stamp = (
        metadata.len(),
        metadata.modified().ok(),
        metadata.created().ok(),
    );
    let mut cache = CACHE.lock().ok()?;
    // 只保留最近一批活动会话；磁盘快照不因此消失，已经打开的分页请求仍可读。
    if cache.len() >= 128 && !cache.contains_key(source) {
        cache.clear();
    }
    let entry = cache.entry(source.into()).or_default();
    if entry.stamp == Some(stamp) && entry.path.is_file() {
        return Some(entry.path.clone());
    }
    let legacy = source
        .extension()
        .is_some_and(|extension| extension == "json");
    let mut file = fs::File::open(source).ok()?;
    let reset = legacy
        || entry.stamp.is_none()
        || entry.stamp.is_some_and(|old| old.2 != stamp.2)
        || stamp.0 < entry.offset;
    // 独立审查复现：前 1024 字节相同的增长重写仍会丢消息。只在源 stamp 改变时流式核对
    // 整个旧 offset，不解析旧 JSON；完整前缀相同才是真追加，不变轮询仍只有 stat。
    if reset || !prefix_matches(&mut file, entry.offset, &entry.source_hash)? {
        entry.journal = Journal::default();
        entry.offset = 0;
        entry.source_hash = Sha256::new();
    }
    file.seek(SeekFrom::Start(entry.offset)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    if legacy {
        entry.journal.fold(serde_json::from_slice(&bytes).ok()?);
        entry.offset = stamp.0;
        entry.source_hash.update(&bytes);
    } else {
        let complete = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        for line in bytes[..complete].split(|byte| *byte == b'\n') {
            if let Ok(value) = serde_json::from_slice::<Value>(line) {
                entry.journal.fold(value);
            }
        }
        entry.offset += complete as u64;
        entry.source_hash.update(&bytes[..complete]);
    }
    let snapshot = entry.journal.snapshot();
    if snapshot != entry.snapshot || !entry.path.is_file() {
        if !entry.path.as_os_str().is_empty()
            && entry.path.is_file()
            && snapshot.starts_with(&entry.snapshot)
        {
            fs::OpenOptions::new()
                .append(true)
                .open(&entry.path)
                .ok()?
                .write_all(&snapshot.as_bytes()[entry.snapshot.len()..])
                .ok()?;
        } else {
            // 旧消息被改写时换路径，宿主会重置 offset。仅改文件长度无法可靠识别变长重写。
            let source_key = fingerprint(source.to_string_lossy().as_bytes());
            let next = dir.join(format!(
                "{source_key}-{}.jsonl",
                fingerprint(snapshot.as_bytes())
            ));
            crate::fsutil::write_atomic_secure(&next, &snapshot).ok()?;
            if !entry.path.as_os_str().is_empty() && entry.path != next {
                let _ = fs::remove_file(&entry.path);
            }
            entry.path = next;
        }
        entry.snapshot = snapshot;
    }
    entry.stamp = Some(stamp);
    Some(entry.path.clone())
}

fn matches_session(path: &Path, session: &str) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    // JSONL 的首行是 metadata；旧 JSON 也只读有界头部，不扫描其他会话的正文。
    let mut bytes = Vec::new();
    let _ = BufReader::new(file).take(64 * 1024).read_to_end(&mut bytes);
    let head = String::from_utf8_lossy(&bytes);
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        return serde_json::from_str::<Value>(head.lines().next().unwrap_or(""))
            .ok()
            .is_some_and(|value| value["sessionId"] == session && value["kind"] != "subagent");
    }
    // 旧 JSON 的 messages 可以很大；上游先写 metadata 再写 messages，读头部即可确认身份。
    struct Header<'a>(&'a mut (Option<String>, Option<String>, bool));
    impl<'de> serde::de::Visitor<'de> for Header<'_> {
        type Value = ();
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("Gemini session metadata")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut map: M,
        ) -> Result<Self::Value, M::Error> {
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "sessionId" => self.0 .0 = Some(map.next_value()?),
                    "kind" => self.0 .1 = Some(map.next_value()?),
                    "messages" => {
                        self.0 .2 = true;
                        return Err(serde::de::Error::custom("metadata complete"));
                    }
                    _ => {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
            }
            self.0 .2 = true;
            Ok(())
        }
    }
    let mut fields = (None, None, false);
    let _ = serde::Deserializer::deserialize_map(
        &mut serde_json::Deserializer::from_str(&head),
        Header(&mut fields),
    );
    fields.2 && fields.0.as_deref() == Some(session) && fields.1.as_deref() != Some("subagent")
}

fn find_in(root: &Path, session: &str) -> Option<PathBuf> {
    let projects = fs::read_dir(root.join("tmp")).ok()?;
    for project in projects.flatten() {
        let Ok(files) = fs::read_dir(project.path().join("chats")) else {
            continue;
        };
        let mut candidates: Vec<_> = files
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("session-"))
            })
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "jsonl" || ext == "json")
            })
            .collect();
        candidates.sort_by_key(|path| path.extension().is_none_or(|ext| ext != "jsonl"));
        if let Some(path) = candidates
            .into_iter()
            .find(|path| matches_session(path, session))
        {
            return Some(path);
        }
    }
    None
}

fn source_path(explicit: Option<&str>, session: &str) -> Option<PathBuf> {
    if session.is_empty()
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    if let Some(path) = explicit
        .map(PathBuf::from)
        .filter(|path| matches_session(path, session))
    {
        return Some(path);
    }
    let root = crate::by_id("gemini")?.resolve()?.data_dir;
    let key = (root.clone(), session.to_string());
    let mut locations = LOCATIONS.lock().ok()?;
    if let Some((_, Some(path))) = locations
        .get(&key)
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(2))
    {
        if path.is_file() {
            return Some(path.clone());
        }
    }
    let found = find_in(&root, session);
    if locations.len() >= 128 {
        locations.clear();
    }
    locations.insert(key, (Instant::now(), found.clone()));
    found
}

impl TranscriptSpec for GeminiTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::<GeminiParser>::default()
    }
    fn resolve_transcript_path(
        &self,
        transcript_path: Option<&str>,
        _: Option<&str>,
        session_id: &str,
    ) -> Option<PathBuf> {
        materialize(&source_path(transcript_path, session_id)?)
    }
    fn resolve_title(
        &self,
        transcript_path: Option<&str>,
        cwd: Option<&str>,
        session_id: &str,
    ) -> Option<String> {
        let path = self.resolve_transcript_path(transcript_path, cwd, session_id)?;
        let mut first = String::new();
        BufReader::new(fs::File::open(path).ok()?)
            .read_line(&mut first)
            .ok()?;
        let value: Value = serde_json::from_str(&first).ok()?;
        value
            .pointer("/gemini_metadata/summary")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
    }
    fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
        parse_line(line)
    }
    fn supports_chat(&self) -> bool {
        true
    }
    fn tail_read(&self) -> Option<crate::transcript::TailRead> {
        Some(crate::transcript::TailRead {
            mode_markers: &[],
            plan_markers: &[],
        })
    }
}

pub(super) fn read_model(ctx: &crate::caps::HookContext<'_>) -> Option<String> {
    let source = source_path(ctx.transcript_path, ctx.session_id)?;
    let mut file = fs::File::open(source).ok()?;
    let size = file.metadata().ok()?.len();
    let start = size.saturating_sub(1024 * 1024);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let tail = String::from_utf8_lossy(&bytes);
    // Stop hook 子进程只读有界尾部，不与 GUI 争写归并缓存；超长未完整记录宁可不给模型名。
    let tail = if start > 0 {
        tail.split_once('\n').map_or("", |(_, tail)| tail)
    } else {
        &tail
    };
    tail.lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["type"] == "gemini")
        .find_map(|message| {
            message
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn content_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn parse_line(line: &str) -> Vec<TranscriptEvent> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let text = content_text(
        value
            .get("displayContent")
            .filter(|value| !value.is_null())
            .or_else(|| value.get("content")),
    );
    let mut events = Vec::new();
    match value.get("type").and_then(Value::as_str) {
        Some("user") if !text.trim().is_empty() => events.push(TranscriptEvent::UserMessage {
            id: format!("gemini-user-{id}"),
            timestamp,
            text,
            interjected: false,
        }),
        Some("error") if !text.trim().is_empty() => events.push(TranscriptEvent::TurnError {
            id: format!("gemini-error-{id}"),
            timestamp,
            label: "API 请求出错".into(),
            text,
        }),
        Some("gemini") => {
            if let Some(thoughts) = value.get("thoughts").and_then(Value::as_array) {
                for (index, thought) in thoughts.iter().enumerate() {
                    let text = [thought.get("subject"), thought.get("description")]
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !text.is_empty() {
                        events.push(TranscriptEvent::Reasoning {
                            id: format!("gemini-thought-{id}-{index}"),
                            timestamp: timestamp.clone(),
                            text,
                        });
                    }
                }
            }
            if !text.trim().is_empty() {
                events.push(TranscriptEvent::AssistantMessage {
                    id: format!("gemini-assistant-{id}"),
                    timestamp: timestamp.clone(),
                    text,
                });
            }
            if let Some(calls) = value.get("toolCalls").and_then(Value::as_array) {
                for call in calls {
                    let Some(call_id) = call.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let name = call.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let arguments = call.get("args").map(Value::to_string).unwrap_or_default();
                    events.push(TranscriptEvent::ToolCall {
                        id: format!("gemini-call-{call_id}"),
                        timestamp: timestamp.clone(),
                        name: name.into(),
                        summary: arguments.chars().take(300).collect(),
                        subagent: None,
                        detail: None,
                        plan: None,
                    });
                    if let Some(result) = call.get("result").filter(|value| !value.is_null()) {
                        let text = if let Some(parts) = result.as_array() {
                            parts
                                .iter()
                                .filter_map(|part| {
                                    part.get("functionResponse")
                                        .and_then(|response| response.get("response"))
                                })
                                .map(Value::to_string)
                                .collect::<Vec<_>>()
                                .join("\n")
                        } else {
                            result.to_string()
                        };
                        let is_error = call["status"] == "error"
                            || result.as_array().is_some_and(|parts| {
                                parts.iter().any(|part| {
                                    part.pointer("/functionResponse/response/error").is_some()
                                })
                            });
                        events.push(TranscriptEvent::ToolResult {
                            id: format!("gemini-result-{call_id}"),
                            timestamp: timestamp.clone(),
                            tool_call_id: Some(format!("gemini-call-{call_id}")),
                            text: text.chars().take(16_000).collect(),
                            is_error,
                            subagent: None,
                            patch: None,
                        });
                    }
                }
            }
        }
        _ => {}
    }
    events
}

#[derive(Default)]
struct GeminiParser {
    info: TranscriptInfo,
}
impl TranscriptParser for GeminiParser {
    fn fold_line(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(title) = value
            .pointer("/gemini_metadata/summary")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
        {
            self.info.title = Some(title.into());
        }
        match value.get("type").and_then(Value::as_str) {
            Some("user" | "gemini") => self.info.error = None,
            Some("error") => {
                let raw = content_text(value.get("content"));
                if !raw.is_empty() {
                    self.info.error = Some(TurnError {
                        label: "API 请求出错".into(),
                        raw,
                        fingerprint: "gemini:api".into(),
                    });
                }
            }
            _ => {}
        }
        if value["type"] == "gemini" {
            let text = content_text(value.get("content"));
            if let Some(preview) = preview_text(&text) {
                self.info.preview = Some(preview);
            }
            if let Some(tokens) = value.get("tokens").filter(|value| value.is_object()) {
                // cached 已包含在 input，不能再次累加；未知模型窗口不伪造百分比。
                self.info.context_tokens =
                    tokens.get("total").and_then(Value::as_u64).or_else(|| {
                        Some(
                            ["input", "output", "thoughts", "tool"]
                                .into_iter()
                                .filter_map(|key| tokens.get(key).and_then(Value::as_u64))
                                .sum(),
                        )
                    });
            }
        }
    }
    fn to_info(&self) -> TranscriptInfo {
        self.info.clone()
    }
}

#[cfg(test)]
mod tests;
