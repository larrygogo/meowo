//! 1.19.3 隔离 /doc 与无模型 server 探针：messageID 单独不幂等，part id 也必须稳定。

use crate::conversation::*;
use crate::variant::Installation;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) static CAP: Cap = Cap;
pub(super) struct Cap;
impl ConversationCap for Cap {
    fn spec(
        &self,
        installation: &Installation,
        version: Option<&str>,
    ) -> Result<ConversationSpec, String> {
        if installation.variant_tag != "stable" || version.map(str::trim) != Some("1.19.3") {
            return Err("conversation/version_unverified".into());
        }
        Ok(ConversationSpec {
            args: vec![
                "serve".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                "{meowo_port}".into(),
            ],
            environment: vec![
                ("MEOWO_PROTOCOL_MANAGED".into(), "1".into()),
                ("OPENCODE_SERVER_USERNAME".into(), "opencode".into()),
                ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into()),
            ],
            transport: ConversationTransport::HttpServer {
                health_path: "/global/health".into(),
                events_path: "/event".into(),
                username: "opencode".into(),
                password_env: "OPENCODE_SERVER_PASSWORD".into(),
            },
            ready_timeout_ms: 30_000,
        })
    }
    fn codec(&self, init: ConversationInit) -> Box<dyn ConversationCodec> {
        Box::new(Codec::new(init))
    }
}

enum Pending {
    Bind,
    History,
    Prompt(String),
    Abort,
    Resolve(String),
    Models,
    Agents,
    SetModel(String),
    SetAgent(String),
}
struct Active {
    request: String,
    native: String,
    consumed: bool,
    failed: bool,
}
struct Interaction {
    question: bool,
    v2: bool,
    params: Value,
}
struct Codec {
    init: ConversationInit,
    root: Option<String>,
    initialized: bool,
    ready: bool,
    sequence: u64,
    pending: HashMap<String, Pending>,
    active: Option<Active>,
    seen: HashSet<String>,
    interactions: HashMap<String, Interaction>,
    roles: HashMap<String, String>,
    parts: HashMap<String, (String, bool)>,
    event_ids: VecDeque<String>,
    stream_bytes: usize,
    models: Vec<ConversationChoice>,
    modes: Vec<ConversationChoice>,
    agent_mode: Option<String>,
    assistant_parents: HashMap<String, String>,
    running_tools: HashSet<String>,
    session_parents: HashMap<String, String>,
}
fn emit(event: ConversationEvent) -> ConversationAction {
    ConversationAction::Emit(event)
}
fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").into()
}
fn safe_id(id: &str, prefix: &str) -> bool {
    id.starts_with(prefix)
        && id.len() <= 1024
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn diagnostic_text(value: &Value, text: &mut String, depth: usize) {
    // OpenCode 的错误包装层会随 provider 改版而增加；只在内存中走有限层级，
    // 既能识别嵌套的鉴权/配额错误，也不会把原文带入持久化事件。
    if depth > 4 || text.len() >= 4096 {
        return;
    }
    match value {
        Value::String(value) => {
            let remaining = 4096usize.saturating_sub(text.len());
            text.extend(
                value
                    .chars()
                    .take(remaining.min(512))
                    .flat_map(char::to_lowercase),
            );
            text.push(' ');
        }
        Value::Array(values) => {
            for value in values.iter().take(32) {
                diagnostic_text(value, text, depth + 1);
            }
        }
        Value::Object(values) => {
            for value in values.values().take(32) {
                diagnostic_text(value, text, depth + 1);
            }
        }
        _ => {}
    }
}
fn native_error_category(error: &Value) -> String {
    let mut text = String::new();
    diagnostic_text(error, &mut text, 0);
    if ["unauthorized", "forbidden", "authentication", "credential"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "auth"
    } else if ["rate limit", "too many request", "quota"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "rate_limited"
    } else if ["timeout", "timed out", "deadline"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "timeout"
    } else if ["network", "connection", "dns", "econn"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "network"
    } else if ["context", "token limit", "max tokens"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "context_limit"
    } else if ["model", "provider"]
        .iter()
        .any(|needle| text.contains(needle))
    {
        "model_or_provider"
    } else {
        "unknown"
    }
    .into()
}
fn native_diagnostic(signal: &str, error: &Value) -> ConversationAction {
    let status = error
        .get("status")
        .or_else(|| error.get("statusCode"))
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| (100..=599).contains(value));
    let error_shape = match error {
        Value::Null => "missing",
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        _ => "scalar",
    };
    emit(ConversationEvent::NativeDiagnostic {
        provider: "opencode".into(),
        transport: "sse".into(),
        signal: signal.into(),
        category: native_error_category(error),
        error_shape: error_shape.into(),
        status,
    })
}
fn choice(id: &str, kind: &str) -> ConversationChoice {
    ConversationChoice {
        id: id.into(),
        label: id.into(),
        kind: kind.into(),
    }
}
fn message_id(request: &str) -> String {
    let hex: String = request
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("msg_meowo_{hex}")
}
fn request_id(native: &str) -> Option<String> {
    let hex = native.strip_prefix("msg_meowo_")?;
    if hex.len() > 512 || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect();
    String::from_utf8(bytes?)
        .ok()
        .filter(|s| !s.is_empty() && message_id(s) == native)
}
fn percent(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

impl Codec {
    fn new(init: ConversationInit) -> Self {
        Self {
            init,
            root: None,
            initialized: false,
            ready: false,
            sequence: 0,
            pending: HashMap::new(),
            active: None,
            seen: HashSet::new(),
            interactions: HashMap::new(),
            roles: HashMap::new(),
            parts: HashMap::new(),
            event_ids: VecDeque::new(),
            stream_bytes: 0,
            models: vec![],
            modes: vec![],
            agent_mode: None,
            assistant_parents: HashMap::new(),
            running_tools: HashSet::new(),
            session_parents: HashMap::new(),
        }
    }
    fn http(
        &mut self,
        method: &str,
        path: String,
        body: Option<Value>,
        pending: Pending,
    ) -> ConversationAction {
        self.sequence += 1;
        let tag = format!("meowo-{}", self.sequence);
        self.pending.insert(tag.clone(), pending);
        ConversationAction::Http {
            tag,
            method: method.into(),
            path: format!(
                "{path}{}directory={}",
                if path.contains('?') { "&" } else { "?" },
                percent(&self.init.cwd)
            ),
            body,
        }
    }
    fn active_id(&self) -> Option<String> {
        self.active.as_ref().map(|a| a.request.clone())
    }
    fn owns_child(&self, id: &str) -> bool {
        let Some(root) = self.root.as_deref() else {
            return false;
        };
        let mut current = id;
        for _ in 0..128 {
            let Some(parent) = self.session_parents.get(current) else {
                return false;
            };
            if parent == root {
                return true;
            }
            if parent == current {
                return false;
            }
            current = parent;
        }
        false
    }
    fn task_writes(&self, part: &Value) -> Vec<ConversationAction> {
        let state = &part["state"];
        if state["status"] != "completed" {
            return vec![];
        }
        let mid = text(part, "messageID");
        let Some(active) = self
            .active
            .as_ref()
            .filter(|a| self.assistant_parents.get(&mid) == Some(&a.native))
        else {
            return vec![];
        };
        let tool_id = text(part, "callID");
        if tool_id.is_empty() {
            return vec![];
        }
        let paths = match part["tool"].as_str() {
            Some("write" | "edit") => state
                .pointer("/input/filePath")
                .and_then(Value::as_str)
                .and_then(crate::conversation::task_write::literal_lead_path)
                .map(|p| vec![p])
                .unwrap_or_default(),
            Some("bash") if state.pointer("/metadata/exit").and_then(Value::as_i64) == Some(0) => {
                crate::conversation::task_write::targets(&text(&state["input"], "command"))
            }
            _ => vec![],
        };
        paths
            .into_iter()
            .map(|path| {
                emit(ConversationEvent::TaskWrite {
                    request_id: active.request.clone(),
                    tool_id: tool_id.clone(),
                    call: json!({"name":"Write","input":{"file_path":path}}),
                })
            })
            .collect()
    }
    fn catalog(&self) -> ConversationAction {
        emit(ConversationEvent::Catalog {
            models: self.models.clone(),
            modes: self.modes.clone(),
            mode_dimension: Some("agent".into()),
        })
    }
    fn consume(&mut self, native: &str) -> Vec<ConversationAction> {
        let Some(active) = self
            .active
            .as_mut()
            .filter(|a| a.native == native && !a.consumed)
        else {
            return vec![];
        };
        active.consumed = true;
        self.seen.insert(active.request.clone());
        vec![emit(ConversationEvent::Consumed {
            request_id: active.request.clone(),
            native_message_id: Some(native.into()),
        })]
    }
    fn history(&mut self, body: &Value) -> Result<Vec<ConversationAction>, String> {
        let messages = body.as_array().ok_or("conversation/history_invalid")?;
        if messages.len() > 4096 {
            return Err("conversation/history_limit".into());
        }
        let mut history = vec![];
        let mut consumed = vec![];
        for message in messages {
            let info = &message["info"];
            if info.get("sessionID").and_then(Value::as_str) != self.root.as_deref() {
                return Err("conversation/root_mismatch".into());
            }
            let native = text(info, "id");
            let role = text(info, "role");
            let client = if role == "user" {
                request_id(&native)
            } else {
                None
            };
            if role == "user" {
                if let Some(request) = client.clone() {
                    self.seen.insert(request.clone());
                    consumed.push(emit(ConversationEvent::Consumed {
                        request_id: request,
                        native_message_id: Some(native.clone()),
                    }));
                }
            }
            for part in message
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|p| ["text", "reasoning"].contains(&p["type"].as_str().unwrap_or("")))
            {
                if history.len() >= 4096 {
                    return Err("conversation/history_limit".into());
                }
                let segment = text(part, "id");
                history.push(ConversationHistoryMessage {
                    native_message_id: Some(native.clone()),
                    segment_id: (!segment.is_empty()).then_some(segment),
                    request_id: client.clone(),
                    role: if part["type"] == "reasoning" {
                        "reasoning".into()
                    } else {
                        role.clone()
                    },
                    text: text(part, "text"),
                });
            }
        }
        self.ready = true;
        let mut out = vec![emit(ConversationEvent::History { messages: history })];
        out.extend(consumed);
        out.push(emit(ConversationEvent::Ready));
        Ok(out)
    }
    fn interaction(&mut self, kind: &str, p: &Value) -> Result<Vec<ConversationAction>, String> {
        if self.interactions.len() >= 32 {
            return Err("conversation/interaction_limit".into());
        }
        let id = text(p, "id");
        let question = kind.starts_with("question");
        if !safe_id(&id, if question { "que" } else { "per" }) {
            return Err("conversation/interaction_invalid".into());
        }
        let choices = if question {
            vec![choice("reject", "reject_once")]
        } else {
            vec![
                choice("once", "allow_once"),
                choice("always", "allow_always"),
                choice("reject", "reject_once"),
            ]
        };
        let questions = p
            .get("questions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, q)| ConversationQuestion {
                id: i.to_string(),
                title: text(q, "header"),
                text: text(q, "question"),
                multiple: q["multiple"].as_bool().unwrap_or(false),
                options: q
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|o| choice(&text(o, "label"), "answer"))
                    .collect(),
            })
            .collect();
        self.interactions.insert(
            id.clone(),
            Interaction {
                question,
                v2: kind.contains(".v2."),
                params: p.clone(),
            },
        );
        Ok(vec![emit(ConversationEvent::Interaction {
            interaction_id: id,
            title: if question {
                "question".into()
            } else {
                p.get("permission")
                    .or_else(|| p.get("action"))
                    .and_then(Value::as_str)
                    .unwrap_or("permission")
                    .into()
            },
            detail: p.to_string(),
            choices,
            questions,
        })])
    }
    fn stream(
        &mut self,
        part: &Value,
        delta: Option<&str>,
    ) -> Result<Vec<ConversationAction>, String> {
        let mid = text(part, "messageID");
        if self.roles.get(&mid).map(String::as_str) != Some("assistant") {
            return Ok(vec![]);
        }
        let id = text(part, "id");
        let kind = text(part, "type");
        let reasoning = kind == "reasoning";
        if !["text", "reasoning"].contains(&kind.as_str()) {
            return Ok(vec![]);
        }
        if self.parts.len() >= 2048 && !self.parts.contains_key(&id) {
            return Err("conversation/stream_limit".into());
        }
        let old = self
            .parts
            .entry(id.clone())
            .or_insert_with(|| (String::new(), reasoning));
        let append = if let Some(delta) = delta {
            delta.to_owned()
        } else {
            let full = text(part, "text");
            full.strip_prefix(&old.0)
                .ok_or("conversation/stream_rewrite")?
                .to_owned()
        };
        if self.stream_bytes + append.len() > 8 * 1024 * 1024 {
            return Err("conversation/stream_limit".into());
        }
        self.stream_bytes += append.len();
        old.0.push_str(&append);
        if append.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![emit(ConversationEvent::Output {
            segment_id: id,
            request_id: self.active_id(),
            text: append,
            reasoning,
        })])
    }
}

impl ConversationCodec for Codec {
    fn validate_prompt(
        &self,
        _text: &str,
        attachments: &[ConversationAttachment],
    ) -> Result<(), String> {
        if !self.ready || self.active.is_some() || self.root.is_none() {
            return Err("conversation/not_ready".into());
        }
        for attachment in attachments {
            if !attachment
                .mime_type
                .as_deref()
                .is_some_and(|mime| mime.starts_with("image/"))
                || attachment.data_base64.is_none()
            {
                return Err("conversation/attachment_unsupported".into());
            }
        }
        Ok(())
    }
    fn initialize(&mut self) -> Result<Vec<ConversationAction>, String> {
        if self.initialized {
            return Err("conversation/already_initialized".into());
        }
        if self.init.approval_mode.is_some() {
            return Err("conversation/mode_unsupported".into());
        }
        self.initialized = true;
        if let Some(root) = &self.init.native_session_id {
            if !safe_id(root, "ses") {
                return Err("conversation/session_invalid".into());
            }
            Ok(vec![self.http(
                "GET",
                format!("/session/{root}"),
                None,
                Pending::Bind,
            )])
        } else {
            Ok(vec![self.http(
                "POST",
                "/session".into(),
                Some(json!({})),
                Pending::Bind,
            )])
        }
    }
    fn input(&mut self, input: ConversationInput) -> Result<Vec<ConversationAction>, String> {
        match input {
            ConversationInput::Prompt {
                request_id,
                text,
                attachments,
            } => {
                if !self.ready {
                    return Err("conversation/not_ready".into());
                }
                if self.seen.contains(&request_id) {
                    return Ok(vec![emit(ConversationEvent::Consumed {
                        native_message_id: Some(message_id(&request_id)),
                        request_id,
                    })]);
                }
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                if request_id.is_empty() || request_id.len() > 256 || self.seen.len() >= 4096 {
                    return Err("conversation/request_limit".into());
                }
                let root = self.root.clone().ok_or("conversation/not_ready")?;
                let native = message_id(&request_id);
                let suffix = native.trim_start_matches("msg_");
                let mut parts =
                    vec![json!({"id":format!("prt_{suffix}_0"),"type":"text","text":text})];
                for (index, a) in attachments.into_iter().enumerate() {
                    let mime = a.mime_type.ok_or("conversation/attachment_mime_missing")?;
                    let url = if let Some(data) = a.data_base64 {
                        format!("data:{mime};base64,{data}")
                    } else {
                        // URI 仅编码本地文件引用，宿主仍负责附件落盘与路径访问策略。
                        let path = a.path.replace('\\', "/");
                        if path.starts_with('/') {
                            format!(
                                "file://{}",
                                path.split('/').map(percent).collect::<Vec<_>>().join("/")
                            )
                        } else if path.as_bytes().get(1) == Some(&b':') {
                            format!(
                                "file:///{}:{}",
                                &path[..1],
                                path[2..]
                                    .split('/')
                                    .map(percent)
                                    .collect::<Vec<_>>()
                                    .join("/")
                            )
                        } else {
                            return Err("conversation/attachment_path_invalid".into());
                        }
                    };
                    parts.push(json!({"id":format!("prt_{suffix}_{}",index+1),"type":"file","mime":mime,"url":url}));
                }
                let mut body = json!({"messageID":native,"parts":parts});
                if let Some(agent) = &self.agent_mode {
                    body["agent"] = json!(agent);
                }
                if let Some(model) = &self.init.model {
                    let (provider, model) = model
                        .split_once('/')
                        .filter(|(p, m)| !p.is_empty() && !m.is_empty())
                        .ok_or("conversation/model_invalid")?;
                    body["model"] = json!({"providerID":provider,"modelID":model});
                }
                self.parts.clear();
                self.roles.clear();
                self.assistant_parents.clear();
                self.stream_bytes = 0;
                self.active = Some(Active {
                    request: request_id.clone(),
                    native,
                    consumed: false,
                    failed: false,
                });
                Ok(vec![self.http(
                    "POST",
                    format!("/session/{root}/prompt_async"),
                    Some(body),
                    Pending::Prompt(request_id),
                )])
            }
            ConversationInput::Cancel | ConversationInput::Shutdown => {
                if self.active.is_none() {
                    return Ok(vec![]);
                }
                Ok(vec![self.http(
                    "POST",
                    format!(
                        "/session/{}/abort",
                        self.root.as_ref().ok_or("conversation/not_ready")?
                    ),
                    None,
                    Pending::Abort,
                )])
            }
            ConversationInput::Resolve {
                interaction_id,
                option_id,
                answers,
            } => {
                let item = self
                    .interactions
                    .get(&interaction_id)
                    .ok_or("conversation/interaction_missing")?;
                let root = item
                    .params
                    .get("sessionID")
                    .and_then(Value::as_str)
                    .ok_or("conversation/interaction_invalid")?;
                let base = if item.v2 {
                    format!(
                        "/api/session/{root}/{}",
                        if item.question {
                            "question"
                        } else {
                            "permission"
                        }
                    )
                } else {
                    format!(
                        "/{}",
                        if item.question {
                            "question"
                        } else {
                            "permission"
                        }
                    )
                };
                let (path, body) = if item.question {
                    if option_id.as_deref() == Some("reject") {
                        (format!("{base}/{interaction_id}/reject"), None)
                    } else {
                        let qs = item.params["questions"]
                            .as_array()
                            .ok_or("conversation/answers_invalid")?;
                        let answers = if let Some(a) = answers.as_array() {
                            a.clone()
                        } else {
                            let map = answers.as_object().ok_or("conversation/answers_invalid")?;
                            (0..qs.len())
                                .map(|i| {
                                    map.get(&i.to_string())
                                        .cloned()
                                        .ok_or("conversation/answers_invalid")
                                })
                                .collect::<Result<Vec<_>, _>>()?
                        };
                        if answers.len() != qs.len()
                            || answers.iter().any(|a| {
                                !a.as_array().is_some_and(|v| {
                                    !v.is_empty() && v.iter().all(Value::is_string)
                                })
                            })
                        {
                            return Err("conversation/answers_invalid".into());
                        }
                        (
                            format!("{base}/{interaction_id}/reply"),
                            Some(json!({"answers":answers})),
                        )
                    }
                } else {
                    let reply = option_id
                        .filter(|s| ["once", "always", "reject"].contains(&s.as_str()))
                        .ok_or("conversation/option_invalid")?;
                    (
                        format!("{base}/{interaction_id}/reply"),
                        Some(json!({"reply":reply})),
                    )
                };
                Ok(vec![self.http(
                    "POST",
                    path,
                    body,
                    Pending::Resolve(interaction_id),
                )])
            }
            ConversationInput::SetModel { model } => {
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                if !model
                    .split_once('/')
                    .is_some_and(|(p, m)| !p.is_empty() && !m.is_empty())
                {
                    return Err("conversation/model_invalid".into());
                }
                let (provider, id) = model.split_once('/').ok_or("conversation/model_invalid")?;
                let body = json!({"model":{"providerID":provider,"modelID":id}});
                let root = self.root.as_ref().ok_or("conversation/not_ready")?;
                Ok(vec![self.http(
                    "POST",
                    format!("/api/session/{root}/model"),
                    Some(body),
                    Pending::SetModel(model),
                )])
            }
            ConversationInput::SetMode { dimension, mode } => {
                if dimension != "agent" {
                    return Err("conversation/mode_dimension_unsupported".into());
                }
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                if !self.modes.iter().any(|m| m.id == mode) {
                    return Err("conversation/mode_unsupported".into());
                }
                let root = self.root.as_ref().ok_or("conversation/not_ready")?;
                Ok(vec![self.http(
                    "POST",
                    format!("/api/session/{root}/agent"),
                    Some(json!({"agent":mode})),
                    Pending::SetAgent(mode),
                )])
            }
        }
    }
    fn receive(&mut self, message: Value) -> Result<Vec<ConversationAction>, String> {
        if let Some(response) = message.get("_meowo_http") {
            let tag = text(response, "tag");
            let Some(pending) = self.pending.remove(&tag) else {
                return Ok(vec![]);
            };
            if !response["status"]
                .as_u64()
                .is_some_and(|s| (200..300).contains(&s))
            {
                if matches!(pending, Pending::Models | Pending::Agents) {
                    return Ok(vec![self.catalog()]);
                }
                let request_id = match &pending {
                    Pending::Prompt(id) => Some(id.clone()),
                    _ => self.active_id(),
                };
                if let Pending::Prompt(id) = &pending {
                    if self.active.as_ref().is_some_and(|a| a.request == *id) {
                        self.active = None;
                    }
                }
                return Ok(vec![emit(ConversationEvent::Failed {
                    request_id,
                    reason: "conversation/native_http_error".into(),
                })]);
            }
            let body = &response["body"];
            return match pending {
                Pending::Bind => {
                    let root = text(body, "id");
                    if !safe_id(&root, "ses")
                        || body.get("parentID").is_some_and(|v| !v.is_null())
                        || self
                            .init
                            .native_session_id
                            .as_ref()
                            .is_some_and(|s| s != &root)
                    {
                        return Err("conversation/root_mismatch".into());
                    }
                    self.root = Some(root.clone());
                    Ok(vec![
                        emit(ConversationEvent::Bound {
                            native_session_id: root.clone(),
                        }),
                        self.http(
                            "GET",
                            format!("/session/{root}/message?limit=4097"),
                            None,
                            Pending::History,
                        ),
                    ])
                }
                Pending::History => {
                    let mut out = self.history(body)?;
                    out.push(self.http("GET", "/provider".into(), None, Pending::Models));
                    out.push(self.http("GET", "/agent".into(), None, Pending::Agents));
                    Ok(out)
                }
                Pending::Models => {
                    let connected = body
                        .get("connected")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    self.models.clear();
                    for provider in body
                        .get("all")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let pid = text(provider, "id");
                        if !connected.iter().any(|id| id.as_str() == Some(pid.as_str())) {
                            continue;
                        }
                        for (id, model) in provider
                            .get("models")
                            .and_then(Value::as_object)
                            .into_iter()
                            .flatten()
                        {
                            if self.models.len() >= 1024 {
                                return Err("conversation/catalog_limit".into());
                            }
                            self.models.push(ConversationChoice {
                                id: format!("{pid}/{id}"),
                                label: format!(
                                    "{pid}/{}",
                                    model.get("name").and_then(Value::as_str).unwrap_or(id)
                                ),
                                kind: "model".into(),
                            });
                        }
                    }
                    Ok(vec![self.catalog()])
                }
                Pending::Agents => {
                    self.modes = body
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|a| {
                            a["hidden"] != true
                                && ["primary", "all"].contains(&a["mode"].as_str().unwrap_or(""))
                        })
                        .take(128)
                        .filter_map(|a| {
                            let id = a.get("name")?.as_str()?;
                            Some(ConversationChoice {
                                id: id.into(),
                                label: id.into(),
                                kind: "agent".into(),
                            })
                        })
                        .collect();
                    Ok(vec![self.catalog()])
                }
                Pending::SetModel(model) => {
                    self.init.model = Some(model.clone());
                    Ok(vec![emit(ConversationEvent::Metadata {
                        model: Some(model),
                        mode: self.agent_mode.clone(),
                    })])
                }
                Pending::SetAgent(mode) => {
                    self.agent_mode = Some(mode.clone());
                    Ok(vec![emit(ConversationEvent::Metadata {
                        model: self.init.model.clone(),
                        mode: Some(mode),
                    })])
                }
                Pending::Prompt(id) => {
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|a| a.request == id && !a.consumed)
                    {
                        Ok(vec![emit(ConversationEvent::Accepted {
                            request_id: id.clone(),
                            native_message_id: Some(message_id(&id)),
                        })])
                    } else {
                        Ok(vec![])
                    }
                }
                Pending::Abort => Ok(vec![]),
                Pending::Resolve(id) => {
                    self.interactions.remove(&id);
                    Ok(vec![emit(ConversationEvent::InteractionResolved {
                        interaction_id: id,
                    })])
                }
            };
        }
        let message = message
            .get("payload")
            .filter(|v| v.is_object())
            .unwrap_or(&message);
        let kind = text(message, "type");
        let p = &message["properties"];
        if kind == "session.created" {
            let info = &p["info"];
            let id = text(info, "id");
            if let Some(parent) = info
                .get("parentID")
                .and_then(Value::as_str)
                .filter(|p| safe_id(p, "ses") && *p != id)
            {
                if !safe_id(&id, "ses") {
                    return Err("conversation/session_invalid".into());
                }
                if self.session_parents.len() >= 128 && !self.session_parents.contains_key(&id) {
                    return Err("conversation/session_tree_limit".into());
                }
                // 1.19.3真实无模型SSE+GET均返回原生parentID；不读取工具args里的父会话声明。
                self.session_parents.insert(id, parent.into());
            }
            return Ok(vec![]);
        }
        if p.get("sessionID").and_then(Value::as_str) != self.root.as_deref() {
            if p.get("sessionID")
                .and_then(Value::as_str)
                .is_some_and(|id| self.owns_child(id))
            {
                if [
                    "permission.asked",
                    "permission.v2.asked",
                    "question.asked",
                    "question.v2.asked",
                ]
                .contains(&kind.as_str())
                {
                    let mut out = self.interaction(&kind, p)?;
                    for action in &mut out {
                        if let ConversationAction::Emit(ConversationEvent::Interaction {
                            title,
                            ..
                        }) = action
                        {
                            *title = format!("Subtask · {title}");
                        }
                    }
                    return Ok(out);
                }
                if [
                    "permission.replied",
                    "permission.v2.replied",
                    "question.replied",
                    "question.rejected",
                    "question.v2.replied",
                    "question.v2.rejected",
                ]
                .contains(&kind.as_str())
                {
                    let id = text(p, "requestID");
                    if self
                        .interactions
                        .get(&id)
                        .is_some_and(|i| i.params["sessionID"] == p["sessionID"])
                    {
                        self.interactions.remove(&id);
                        return Ok(vec![emit(ConversationEvent::InteractionResolved {
                            interaction_id: id,
                        })]);
                    }
                }
            }
            return Ok(vec![]);
        }
        let event = text(message, "id");
        if !event.is_empty() {
            if self.event_ids.contains(&event) {
                return Ok(vec![]);
            }
            self.event_ids.push_back(event);
            if self.event_ids.len() > 1024 {
                self.event_ids.pop_front();
            }
        }
        match kind.as_str() {
            "message.updated" => {
                let info = &p["info"];
                let id = text(info, "id");
                let role = text(info, "role");
                if self.roles.len() >= 4096 && !self.roles.contains_key(&id) {
                    return Err("conversation/stream_limit".into());
                }
                self.roles.insert(id.clone(), role.clone());
                if role == "assistant" {
                    if let Some(parent) = info.get("parentID").and_then(Value::as_str) {
                        self.assistant_parents.insert(id.clone(), parent.into());
                    }
                }
                if role == "user" {
                    return Ok(self.consume(&id));
                }
                if !info["error"].is_null() {
                    if let Some(active) = &mut self.active {
                        active.failed = true;
                    }
                    return Ok(vec![
                        native_diagnostic("message.updated.info.error", &info["error"]),
                        emit(ConversationEvent::Failed {
                            request_id: self.active_id(),
                            reason: "conversation/native_turn_failed".into(),
                        }),
                    ]);
                }
                Ok(vec![])
            }
            "session.next.prompt.admitted" => Ok(self.consume(&text(p, "messageID"))),
            "message.part.updated" => {
                let part = &p["part"];
                if part["type"] == "tool" {
                    let state = &part["state"];
                    let status = match state["status"].as_str() {
                        Some("pending") => ConversationToolStatus::Started,
                        Some("running") => ConversationToolStatus::Running,
                        Some("completed") => ConversationToolStatus::Completed,
                        Some("error") => ConversationToolStatus::Failed,
                        _ => return Err("conversation/tool_status_unknown".into()),
                    };
                    let call = text(part, "callID");
                    if ["completed", "error"].contains(&state["status"].as_str().unwrap_or("")) {
                        self.running_tools.remove(&call);
                    } else if self.active.as_ref().is_some_and(|a| {
                        self.assistant_parents.get(&text(part, "messageID")) == Some(&a.native)
                    }) {
                        self.running_tools.insert(call);
                    }
                    let mut out = vec![emit(ConversationEvent::Tool {
                        request_id: self.active_id(),
                        tool_id: text(part, "callID"),
                        name: text(part, "tool"),
                        input: state["input"].clone(),
                        output: state
                            .get("output")
                            .or_else(|| state.get("error"))
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        status,
                    })];
                    out.extend(self.task_writes(part));
                    return Ok(out);
                }
                self.stream(part, None)
            }
            "message.part.delta" => {
                if p["field"] != "text" {
                    return Ok(vec![]);
                }
                let id = text(p, "partID");
                let reasoning = self.parts.get(&id).is_some_and(|p| p.1);
                self.stream(&json!({"id":id,"messageID":p["messageID"],"type":if reasoning{"reasoning"}else{"text"}}),p["delta"].as_str())
            }
            "permission.asked" | "permission.v2.asked" | "question.asked" | "question.v2.asked" => {
                self.interaction(&kind, p)
            }
            "permission.replied"
            | "permission.v2.replied"
            | "question.replied"
            | "question.rejected"
            | "question.v2.replied"
            | "question.v2.rejected" => {
                let id = text(p, "requestID");
                if self
                    .interactions
                    .get(&id)
                    .is_some_and(|i| i.params["sessionID"] == p["sessionID"])
                {
                    self.interactions.remove(&id);
                    Ok(vec![emit(ConversationEvent::InteractionResolved {
                        interaction_id: id,
                    })])
                } else {
                    Ok(vec![])
                }
            }
            "session.error" => {
                if let Some(active) = &mut self.active {
                    active.failed = true;
                }
                let error = p
                    .get("error")
                    .or_else(|| p.get("info").and_then(|info| info.get("error")))
                    .unwrap_or(&Value::Null);
                Ok(vec![
                    native_diagnostic("session.error", error),
                    emit(ConversationEvent::Failed {
                        request_id: self.active_id(),
                        reason: "conversation/native_turn_failed".into(),
                    }),
                ])
            }
            "session.idle" | "session.status"
                if kind == "session.idle" || p["status"]["type"] == "idle" =>
            {
                // HTTP响应与SSE没有共同顺序；idle在user事实之前到达不能结束尚未确认接收的消息。
                if !self.active.as_ref().is_some_and(|a| a.consumed || a.failed) {
                    return Ok(vec![]);
                }
                let active = self.active.take().unwrap();
                // 主会话idle不证明子会话的审批已结束；保留到原生replied/rejected或该HTTP回执。
                Ok(vec![
                    emit(ConversationEvent::Completed {
                        request_id: active.request,
                        stop_reason: if active.failed {
                            "failed"
                        } else if !self.running_tools.is_empty() {
                            "awaiting_background"
                        } else {
                            "idle"
                        }
                        .into(),
                    }),
                    emit(ConversationEvent::Ready),
                ])
            }
            _ => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests;
