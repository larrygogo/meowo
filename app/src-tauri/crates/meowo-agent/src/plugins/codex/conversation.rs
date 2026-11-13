//! 0.167.2 本机 generate-json-schema 与隔离 app-server 握手约束；IO 留在宿主。

use crate::conversation::*;
use crate::variant::Installation;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

type FileChangeKey = (String, String, String);
const FILE_CHANGE_ITEM_BYTES: usize = 128 * 1024;
const FILE_CHANGE_CACHE_BYTES: usize = 512 * 1024;
const FILE_CHANGE_CACHE_ITEMS: usize = 32;

struct FileChangeSnapshot {
    changes: Option<Value>,
    bytes: usize,
    unavailable: &'static str,
}

pub(super) static CAP: Cap = Cap;
pub(super) struct Cap;

impl ConversationCap for Cap {
    fn spec(
        &self,
        installation: &Installation,
        version: Option<&str>,
    ) -> Result<ConversationSpec, String> {
        // 下限 = app-server 协议实测版本；更新的版本放行，理由见 lead_install::version_matches。
        if installation.variant_tag != "stable"
            || !crate::lead_install::version_at_least(version, (0, 167, 2))
        {
            return Err("conversation/version_unverified".into());
        }
        Ok(ConversationSpec {
            args: vec!["app-server".into(), "--stdio".into()],
            environment: vec![("MEOWO_PROTOCOL_MANAGED".into(), "1".into())],
            transport: ConversationTransport::JsonLines,
            ready_timeout_ms: 30_000,
        })
    }
    fn codec(&self, init: ConversationInit) -> Box<dyn ConversationCodec> {
        Box::new(Codec::new(init))
    }
}

enum Pending {
    Initialize,
    Bind,
    Prompt(String),
    Interrupt,
    Models,
}
struct Interaction {
    id: Value,
    method: String,
    params: Value,
    choices: Vec<ConversationChoice>,
}
struct Active {
    request: String,
    turn: Option<String>,
    consumed: bool,
}
struct Codec {
    init: ConversationInit,
    root: Option<String>,
    sequence: u64,
    pending: HashMap<String, Pending>,
    interactions: BTreeMap<String, Interaction>,
    active: Option<Active>,
    seen: HashMap<String, String>,
    initialized: bool,
    ready: bool,
    stream_sequence: u64,
    stream_reasoning: Option<bool>,
    models: Vec<ConversationChoice>,
    model_pages: usize,
    thread_parents: HashMap<String, String>,
    running_tools: HashSet<String>,
    running_children: HashSet<String>,
    file_changes: HashMap<FileChangeKey, FileChangeSnapshot>,
    file_change_order: VecDeque<FileChangeKey>,
    file_change_bytes: usize,
}

fn emit(event: ConversationEvent) -> ConversationAction {
    ConversationAction::Emit(event)
}
fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}
fn choice(id: &str, kind: &str) -> ConversationChoice {
    ConversationChoice {
        id: id.into(),
        label: id.into(),
        kind: kind.into(),
    }
}
fn decision_kind(value: &Value, file_change: bool) -> Result<&'static str, String> {
    if let Some(value) = value.as_str() {
        return match value {
            "accept" => Ok("allow_once"),
            "acceptForSession" => Ok("allow_always"),
            "decline" | "cancel" => Ok("reject_once"),
            _ => Err("conversation/approval_decision_unsupported".into()),
        };
    }
    if !file_change && value.as_object().is_some_and(|o| o.len() == 1) {
        if let Some(inner) = value
            .get("acceptWithExecpolicyAmendment")
            .and_then(Value::as_object)
        {
            if inner.len() == 1
                && inner
                    .get("execpolicy_amendment")
                    .and_then(Value::as_array)
                    .is_some_and(|a| {
                        !a.is_empty() && a.len() <= 64 && a.iter().all(Value::is_string)
                    })
            {
                return Ok("allow_always");
            }
        }
        if let Some(inner) = value
            .get("applyNetworkPolicyAmendment")
            .and_then(Value::as_object)
            .filter(|o| o.len() == 1)
        {
            if let Some(policy) = inner
                .get("network_policy_amendment")
                .and_then(Value::as_object)
                .filter(|o| o.len() == 2)
            {
                if policy
                    .get("host")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
                {
                    return match policy.get("action").and_then(Value::as_str) {
                        Some("allow") => Ok("allow_always"),
                        Some("deny") => Ok("reject_always"),
                        _ => Err("conversation/approval_decision_unsupported".into()),
                    };
                }
            }
        }
    }
    Err("conversation/approval_decision_unsupported".into())
}
fn decisions(params: &Value) -> Result<Vec<Value>, String> {
    if let Some(value) = params.get("availableDecisions").filter(|v| !v.is_null()) {
        return value
            .as_array()
            .filter(|a| !a.is_empty() && a.len() <= 32)
            .cloned()
            .ok_or_else(|| "conversation/approval_decision_unsupported".into());
    }
    Ok(if params["kind"] == "humanCommand" {
        vec![json!("accept"), json!("decline"), json!("cancel")]
    } else {
        vec![
            json!("accept"),
            json!("acceptForSession"),
            json!("decline"),
            json!("cancel"),
        ]
    })
}
fn decision_id(value: &Value, index: usize) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("native-decision-{index}"))
}
fn input_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

impl Codec {
    fn new(init: ConversationInit) -> Self {
        Self {
            init,
            root: None,
            sequence: 0,
            pending: HashMap::new(),
            interactions: BTreeMap::new(),
            active: None,
            seen: HashMap::new(),
            initialized: false,
            ready: false,
            stream_sequence: 0,
            stream_reasoning: None,
            models: vec![],
            model_pages: 0,
            thread_parents: HashMap::new(),
            running_tools: HashSet::new(),
            running_children: HashSet::new(),
            file_changes: HashMap::new(),
            file_change_order: VecDeque::new(),
            file_change_bytes: 0,
        }
    }
    fn rpc(&mut self, method: &str, params: Value, pending: Pending) -> ConversationAction {
        self.sequence += 1;
        let id = format!("meowo-{}", self.sequence);
        self.pending.insert(id.clone(), pending);
        ConversationAction::Send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
    }
    fn mode(mode: &str) -> Result<(), String> {
        ["on-request", "untrusted", "never"]
            .contains(&mode)
            .then_some(())
            .ok_or_else(|| "conversation/mode_unsupported".into())
    }
    fn request_id(&self) -> Option<String> {
        self.active.as_ref().map(|a| a.request.clone())
    }
    fn file_change_key(params: &Value, item_id: &str) -> Option<FileChangeKey> {
        let thread = params.get("threadId")?.as_str()?;
        let turn = params.get("turnId")?.as_str()?;
        [thread, turn, item_id]
            .iter()
            .all(|id| !id.is_empty() && id.len() <= 256)
            .then(|| (thread.into(), turn.into(), item_id.into()))
    }
    fn cache_file_change(&mut self, params: &Value, item_id: &str, changes: &Value) {
        // 0.167.2 真机审批只带 itemId/grantRoot；必须关联此前原生 item，不能据路径猜 diff。
        let Some(key) = Self::file_change_key(params, item_id) else {
            return;
        };
        if let Some(old) = self.file_changes.remove(&key) {
            self.file_change_bytes -= old.bytes;
        }
        self.file_change_order.retain(|old| old != &key);
        let valid = changes
            .as_array()
            .filter(|items| !items.is_empty() && items.len() <= 128)
            .is_some_and(|items| {
                items.iter().all(|change| {
                    change
                        .get("path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| !p.is_empty() && p.len() <= 8192)
                        && change.get("diff").is_some_and(Value::is_string)
                        && matches!(
                            change.pointer("/kind/type").and_then(Value::as_str),
                            Some("add" | "delete" | "update")
                        )
                        && change.pointer("/kind/move_path").is_none_or(|p| {
                            p.is_null()
                                || p.as_str().is_some_and(|s| !s.is_empty() && s.len() <= 8192)
                        })
                })
            });
        let bytes = serde_json::to_vec(changes).map_or(usize::MAX, |v| v.len());
        let snapshot = if !valid {
            FileChangeSnapshot {
                changes: None,
                bytes: 0,
                unavailable: "conversation/approval_details_invalid",
            }
        } else if bytes > FILE_CHANGE_ITEM_BYTES {
            FileChangeSnapshot {
                changes: None,
                bytes: 0,
                unavailable: "conversation/approval_details_too_large",
            }
        } else {
            FileChangeSnapshot {
                changes: Some(changes.clone()),
                bytes,
                unavailable: "",
            }
        };
        while self.file_changes.len() >= FILE_CHANGE_CACHE_ITEMS
            || self.file_change_bytes + snapshot.bytes > FILE_CHANGE_CACHE_BYTES
        {
            let Some(old) = self.file_change_order.pop_front() else {
                break;
            };
            if let Some(old) = self.file_changes.remove(&old) {
                self.file_change_bytes -= old.bytes;
            }
        }
        self.file_change_bytes += snapshot.bytes;
        self.file_change_order.push_back(key.clone());
        self.file_changes.insert(key, snapshot);
    }
    fn file_change_detail(&self, params: &Value) -> (String, bool) {
        let key = Self::file_change_key(params, &text(params, "itemId"));
        let snapshot = key.as_ref().and_then(|key| self.file_changes.get(key));
        if let Some(changes) = snapshot.and_then(|item| item.changes.as_ref()) {
            return (
                json!({"request":params,"changes":changes}).to_string(),
                true,
            );
        }
        (json!({"request":params,"reviewUnavailable":snapshot.map_or("conversation/approval_details_missing",|item|item.unavailable),"review":"Review unavailable. Approval is disabled; only a native cancel decision, if advertised, can be sent."}).to_string(),false)
    }
    fn task_writes(&self, p: &Value, item: &Value) -> Vec<ConversationAction> {
        let Some(active) = self.active.as_ref().filter(|a| {
            a.turn.as_deref() == p.get("turnId").and_then(Value::as_str) && a.turn.is_some()
        }) else {
            return vec![];
        };
        if item["status"] != "completed" {
            return vec![];
        }
        let tool_id = text(item, "id");
        if tool_id.is_empty() {
            return vec![];
        }
        let paths = match item["type"].as_str() {
            Some("fileChange") => item
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|change| change.get("path").and_then(Value::as_str))
                .filter_map(crate::conversation::task_write::literal_lead_path)
                .take(32)
                .collect(),
            Some("commandExecution") if item["exitCode"] == 0 => {
                crate::conversation::task_write::targets(&text(item, "command"))
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
    fn owns_child(&self, id: &str) -> bool {
        let Some(root) = self.root.as_deref() else {
            return false;
        };
        let mut cursor = id;
        for _ in 0..128 {
            let Some(parent) = self.thread_parents.get(cursor) else {
                return false;
            };
            if parent == root {
                return true;
            }
            if parent == cursor {
                return false;
            }
            cursor = parent;
        }
        false
    }
    fn consumed(&mut self, native: Option<String>) -> Vec<ConversationAction> {
        let Some(active) = &mut self.active else {
            return vec![];
        };
        if let Some(id) = &native {
            self.seen.insert(active.request.clone(), id.clone());
        } else {
            self.seen.entry(active.request.clone()).or_default();
        }
        if active.consumed && native.is_none() {
            return vec![];
        }
        active.consumed = true;
        vec![emit(ConversationEvent::Consumed {
            request_id: active.request.clone(),
            native_message_id: native,
        })]
    }
    fn history(&mut self, thread: &Value) -> Result<Vec<ConversationAction>, String> {
        let mut history = Vec::new();
        let mut events = Vec::new();
        for turn in thread
            .get("turns")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if turn
                .get("itemsView")
                .and_then(Value::as_str)
                .is_some_and(|s| s != "full")
            {
                return Err("conversation/history_incomplete".into());
            }
            for item in turn
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if history.len() >= 4096 {
                    return Err("conversation/history_limit".into());
                }
                let kind = text(item, "type");
                let native = text(item, "id");
                if kind == "userMessage" {
                    let client = text(item, "clientId");
                    if !client.is_empty() {
                        self.seen.insert(client.clone(), native.clone());
                        events.push(emit(ConversationEvent::Consumed {
                            request_id: client.clone(),
                            native_message_id: Some(native.clone()),
                        }));
                    }
                    history.push(ConversationHistoryMessage {
                        native_message_id: Some(native.clone()),
                        segment_id: Some(native),
                        request_id: (!client.is_empty()).then_some(client),
                        role: "user".into(),
                        text: input_text(&item["content"]),
                    });
                } else if kind == "agentMessage" {
                    history.push(ConversationHistoryMessage {
                        native_message_id: Some(native.clone()),
                        segment_id: Some(native),
                        request_id: None,
                        role: "assistant".into(),
                        text: text(item, "text"),
                    });
                } else if kind == "reasoning" {
                    let values = item
                        .get("summary")
                        .and_then(Value::as_array)
                        .filter(|a| !a.is_empty())
                        .or_else(|| item.get("content").and_then(Value::as_array));
                    let content = values
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n");
                    history.push(ConversationHistoryMessage {
                        native_message_id: Some(native.clone()),
                        segment_id: Some(native),
                        request_id: None,
                        role: "reasoning".into(),
                        text: content,
                    });
                }
            }
        }
        events.insert(0, emit(ConversationEvent::History { messages: history }));
        Ok(events)
    }
    fn bind(&mut self, result: &Value) -> Result<Vec<ConversationAction>, String> {
        self.ready = false;
        let thread = result.get("thread").ok_or("conversation/bind_invalid")?;
        let root = text(thread, "id");
        if root.is_empty()
            || thread.get("parentThreadId").is_some_and(|v| !v.is_null())
            || self
                .init
                .native_session_id
                .as_ref()
                .is_some_and(|id| id != &root)
        {
            return Err("conversation/root_mismatch".into());
        }
        // 已持久paginated副本实测：excludeTurns=false仍返回完整turns；只有未首发的空thread无法读历史。
        if !thread.get("turns").is_some_and(Value::is_array) {
            return Err("conversation/history_invalid".into());
        }
        self.root = Some(root.clone());
        let mut out = vec![emit(ConversationEvent::Bound {
            native_session_id: root,
        })];
        out.extend(self.history(thread)?);
        out.push(emit(ConversationEvent::Metadata {
            model: result
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
            mode: result
                .get("approvalPolicy")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }));
        // 恢复仍在运行的 turn 不能当 ready，也不能把一条新消息拼进旧 turn。
        if let Some(turn) = thread
            .get("turns")
            .and_then(Value::as_array)
            .and_then(|a| a.last())
            .filter(|t| t["status"] == "inProgress")
        {
            let request = turn
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|i| i.get("clientId").and_then(Value::as_str))
                .map(str::to_owned)
                .ok_or("conversation/active_turn_unreconciled")?;
            self.active = Some(Active {
                request,
                turn: Some(text(turn, "id")),
                consumed: true,
            });
        } else {
            self.ready = true;
            out.push(emit(ConversationEvent::Ready));
        }
        Ok(out)
    }
    fn interaction(
        &mut self,
        id: Value,
        method: &str,
        params: &Value,
    ) -> Result<Vec<ConversationAction>, String> {
        if self.interactions.len() >= 32 && !self.interactions.contains_key(&id.to_string()) {
            return Err("conversation/interaction_limit".into());
        }
        let mut questions = Vec::new();
        let mut choices = match method {
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let mut choices = vec![];
                for (index, value) in decisions(params)?.iter().enumerate() {
                    let kind = decision_kind(value, method == "item/fileChange/requestApproval")?;
                    if params["kind"] == "humanCommand" && kind.ends_with("always") {
                        return Err("conversation/approval_decision_unsupported".into());
                    }
                    let id = decision_id(value, index);
                    let label = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    choices.push(ConversationChoice {
                        id,
                        label,
                        kind: kind.into(),
                    });
                }
                choices
            }
            "item/permissions/requestApproval" => vec![
                choice("accept", "allow_once"),
                choice("decline", "reject_once"),
            ],
            "item/tool/requestUserInput" => {
                for q in params
                    .get("questions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    questions.push(ConversationQuestion {
                        id: text(q, "id"),
                        title: text(q, "header"),
                        text: text(q, "question"),
                        multiple: false,
                        options: q
                            .get("options")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|o| choice(&text(o, "label"), "answer"))
                            .collect(),
                    });
                }
                vec![]
            }
            "mcpServer/elicitation/request" => vec![
                choice("accept", "allow_once"),
                choice("decline", "reject_once"),
                choice("cancel", "reject_once"),
            ],
            _ => {
                return Ok(vec![
                    ConversationAction::Send(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"conversation/request_unsupported"}}),
                    ),
                    emit(ConversationEvent::Failed {
                        request_id: self.request_id(),
                        reason: "conversation/request_unsupported".into(),
                    }),
                ])
            }
        };
        let detail = if method == "item/fileChange/requestApproval" {
            let (detail, reviewable) = self.file_change_detail(params);
            if !reviewable {
                choices.retain(|choice| choice.id == "cancel");
            }
            detail
        } else {
            params.to_string()
        };
        let interaction_id = id.to_string();
        self.interactions.insert(
            interaction_id.clone(),
            Interaction {
                id,
                method: method.into(),
                params: params.clone(),
                choices: choices.clone(),
            },
        );
        Ok(vec![emit(ConversationEvent::Interaction {
            interaction_id,
            title: method.into(),
            detail,
            choices,
            questions,
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
        if let Some(mode) = &self.init.approval_mode {
            Self::mode(mode)?;
        }
        self.initialized = true;
        Ok(vec![self.rpc("initialize",json!({"clientInfo":{"name":"meowo","version":"1.0"},"capabilities":{"experimentalApi":true,"requiredHumanApproval":true}}),Pending::Initialize)])
    }
    fn input(&mut self, input: ConversationInput) -> Result<Vec<ConversationAction>, String> {
        match input {
            ConversationInput::Prompt {
                request_id,
                text,
                attachments,
            } => {
                let root = self.root.clone().ok_or("conversation/not_ready")?;
                if !self.ready {
                    return Err("conversation/not_ready".into());
                }
                if let Some(native) = self.seen.get(&request_id) {
                    return Ok(vec![emit(ConversationEvent::Consumed {
                        request_id,
                        native_message_id: (!native.is_empty()).then(|| native.clone()),
                    })]);
                }
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                if request_id.is_empty() || request_id.len() > 256 || self.seen.len() >= 4096 {
                    return Err("conversation/request_limit".into());
                }
                let mut content = vec![json!({"type":"text","text":text,"text_elements":[]})];
                for a in attachments {
                    let mime = a.mime_type.as_deref().unwrap_or("");
                    if !mime.starts_with("image/") {
                        return Err("conversation/attachment_unsupported".into());
                    }
                    if let Some(data) = a.data_base64 {
                        content.push(
                            json!({"type":"image","url":format!("data:{mime};base64,{data}")}),
                        );
                    } else {
                        content.push(json!({"type":"localImage","path":a.path}));
                    }
                }
                let mut params =
                    json!({"threadId":root,"clientUserMessageId":request_id,"input":content});
                if let Some(model) = &self.init.model {
                    params["model"] = json!(model);
                }
                if let Some(mode) = &self.init.approval_mode {
                    params["approvalPolicy"] = json!(mode);
                }
                self.active = Some(Active {
                    request: request_id.clone(),
                    turn: None,
                    consumed: false,
                });
                self.ready = false;
                self.stream_sequence += 1;
                self.stream_reasoning = None;
                Ok(vec![self.rpc(
                    "turn/start",
                    params,
                    Pending::Prompt(request_id),
                )])
            }
            ConversationInput::Cancel | ConversationInput::Shutdown => {
                let Some(active) = &self.active else {
                    return Ok(vec![]);
                };
                let turn = active
                    .turn
                    .clone()
                    .ok_or("conversation/turn_identity_pending")?;
                Ok(vec![self.rpc(
                    "turn/interrupt",
                    json!({"threadId":self.root,"turnId":turn}),
                    Pending::Interrupt,
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
                let response = if item.method == "item/tool/requestUserInput" {
                    let map = answers.as_object().ok_or("conversation/answers_invalid")?;
                    let mut out = serde_json::Map::new();
                    for q in item.params["questions"]
                        .as_array()
                        .ok_or("conversation/answers_invalid")?
                    {
                        let id = text(q, "id");
                        let answer = map.get(&id).ok_or("conversation/answers_invalid")?;
                        let values = answer
                            .as_array()
                            .filter(|a| !a.is_empty() && a.iter().all(Value::is_string))
                            .ok_or("conversation/answers_invalid")?;
                        out.insert(id, json!({"answers":values}));
                    }
                    json!({"answers":out})
                } else {
                    let option = option_id.as_deref().ok_or("conversation/option_missing")?;
                    if !item.choices.iter().any(|c| c.id == option) {
                        return Err("conversation/option_invalid".into());
                    }
                    if item.method == "item/permissions/requestApproval" {
                        json!({"permissions":if option=="accept" {item.params["permissions"].clone()}else{json!({})},"scope":"turn"})
                    } else if item.method == "mcpServer/elicitation/request" {
                        json!({"action":option,"content":if option=="accept" {answers}else{Value::Null}})
                    } else {
                        let values = decisions(&item.params)?;
                        let decision = values
                            .iter()
                            .enumerate()
                            .find(|(index, value)| decision_id(value, *index) == option)
                            .map(|(_, value)| value)
                            .ok_or("conversation/option_invalid")?;
                        decision_kind(decision, item.method == "item/fileChange/requestApproval")?;
                        // 规则型决定必须原样回原RPC，不能重建成更宽的session或其他授权。
                        json!({"decision":decision})
                    }
                };
                let id = item.id.clone();
                self.interactions.remove(&interaction_id);
                Ok(vec![
                    ConversationAction::Send(json!({"jsonrpc":"2.0","id":id,"result":response})),
                    emit(ConversationEvent::InteractionResolved { interaction_id }),
                ])
            }
            ConversationInput::SetModel { model } => {
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                self.init.model = Some(model.clone());
                Ok(vec![emit(ConversationEvent::Metadata {
                    model: Some(model),
                    mode: self.init.approval_mode.clone(),
                })])
            }
            ConversationInput::SetMode { dimension, mode } => {
                if dimension != "approval" {
                    return Err("conversation/mode_dimension_unsupported".into());
                }
                Self::mode(&mode)?;
                if self.active.is_some() {
                    return Err("conversation/turn_busy".into());
                }
                self.init.approval_mode = Some(mode.clone());
                Ok(vec![emit(ConversationEvent::Metadata {
                    model: self.init.model.clone(),
                    mode: Some(mode),
                })])
            }
        }
    }
    fn receive(&mut self, message: Value) -> Result<Vec<ConversationAction>, String> {
        if message.get("method").is_none() {
            let Some(id) = message.get("id").and_then(Value::as_str) else {
                return Ok(vec![]);
            };
            let Some(pending) = self.pending.remove(id) else {
                return Ok(vec![]);
            };
            if message.get("error").is_some() {
                if matches!(pending, Pending::Models) {
                    return Ok(vec![emit(ConversationEvent::Catalog {
                        models: vec![],
                        modes: vec![],
                        mode_dimension: None,
                    })]);
                }
                let request_id = match pending {
                    Pending::Prompt(id) => {
                        if self.active.as_ref().is_some_and(|a| a.request == id) {
                            self.active = None;
                            self.ready = true;
                        }
                        Some(id)
                    }
                    _ => self.request_id(),
                };
                return Ok(vec![emit(ConversationEvent::Failed {
                    request_id,
                    reason: "conversation/native_rpc_error".into(),
                })]);
            }
            let result = &message["result"];
            return match pending {
                Pending::Initialize => {
                    let mut p = json!({"cwd":self.init.cwd});
                    if let Some(model) = &self.init.model {
                        p["model"] = json!(model)
                    }
                    if let Some(mode) = &self.init.approval_mode {
                        p["approvalPolicy"] = json!(mode)
                    }
                    let method = if let Some(id) = &self.init.native_session_id {
                        p["threadId"] = json!(id);
                        p["excludeTurns"] = json!(false);
                        "thread/resume"
                    } else {
                        "thread/start"
                    };
                    Ok(vec![
                        ConversationAction::Send(
                            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
                        ),
                        self.rpc(method, p, Pending::Bind),
                    ])
                }
                Pending::Bind => {
                    let mut out = self.bind(result)?;
                    out.push(self.rpc(
                        "model/list",
                        json!({"limit":100,"includeHidden":false}),
                        Pending::Models,
                    ));
                    Ok(out)
                }
                Pending::Models => {
                    self.model_pages += 1;
                    if self.model_pages > 16 {
                        return Err("conversation/catalog_limit".into());
                    }
                    for model in result
                        .get("data")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if model["hidden"] == true {
                            continue;
                        }
                        let id = model
                            .get("model")
                            .or_else(|| model.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !id.is_empty() && !self.models.iter().any(|m| m.id == id) {
                            self.models.push(ConversationChoice {
                                id: id.into(),
                                label: model
                                    .get("displayName")
                                    .and_then(Value::as_str)
                                    .unwrap_or(id)
                                    .into(),
                                kind: "model".into(),
                            });
                        }
                    }
                    if self.models.len() > 1024 {
                        return Err("conversation/catalog_limit".into());
                    }
                    if let Some(cursor) = result.get("nextCursor").and_then(Value::as_str) {
                        return Ok(vec![self.rpc(
                            "model/list",
                            json!({"cursor":cursor,"limit":100,"includeHidden":false}),
                            Pending::Models,
                        )]);
                    }
                    Ok(vec![emit(ConversationEvent::Catalog {
                        models: self.models.clone(),
                        modes: vec![],
                        mode_dimension: None,
                    })])
                }
                Pending::Prompt(id) => {
                    let turn = text(&result["turn"], "id");
                    if turn.is_empty() {
                        return Err("conversation/turn_identity_missing".into());
                    }
                    if let Some(active) = self.active.as_mut().filter(|a| a.request == id) {
                        active.turn = Some(turn);
                        if active.consumed {
                            Ok(vec![])
                        } else {
                            Ok(vec![emit(ConversationEvent::Accepted {
                                request_id: id,
                                native_message_id: None,
                            })])
                        }
                    } else {
                        Ok(vec![])
                    }
                }
                Pending::Interrupt => Ok(vec![]),
            };
        }
        let method = text(&message, "method");
        let p = &message["params"];
        let root_matches = self
            .root
            .as_ref()
            .is_some_and(|id| p.get("threadId").and_then(Value::as_str) == Some(id.as_str()));
        if method == "thread/started" && message.get("id").is_none() {
            let thread = &p["thread"];
            let child = text(thread, "id");
            if let Some(parent) = thread
                .get("parentThreadId")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty() && *p != child)
            {
                if self.thread_parents.len() >= 128 && !self.thread_parents.contains_key(&child) {
                    return Err("conversation/thread_tree_limit".into());
                }
                // 只登记原生通知的父链；是否属于已绑定root在每次审批时重新核对，不能由工具args认领。
                self.thread_parents.insert(child, parent.into());
            }
            return Ok(vec![]);
        }
        if method == "serverRequest/resolved" && message.get("id").is_none() {
            let thread = p.get("threadId").and_then(Value::as_str).unwrap_or("");
            if root_matches || self.owns_child(thread) {
                let key = p.get("requestId").map(Value::to_string).unwrap_or_default();
                if self.interactions.get(&key).is_some_and(|i| {
                    i.params.get("threadId").and_then(Value::as_str) == Some(thread)
                }) {
                    self.interactions.remove(&key);
                    return Ok(vec![emit(ConversationEvent::InteractionResolved {
                        interaction_id: key,
                    })]);
                }
            }
            return Ok(vec![]);
        }
        let mut review_updates = vec![];
        if message.get("id").is_none()
            && (root_matches
                || p.get("threadId")
                    .and_then(Value::as_str)
                    .is_some_and(|thread| self.owns_child(thread)))
        {
            let update = if method == "item/started" && p["item"]["type"] == "fileChange" {
                Some((text(&p["item"], "id"), &p["item"]["changes"]))
            } else if method == "item/fileChange/patchUpdated" {
                Some((text(p, "itemId"), &p["changes"]))
            } else {
                None
            };
            if let Some((item_id, changes)) = update {
                self.cache_file_change(p, &item_id, changes);
                let key = Self::file_change_key(p, &item_id);
                let pending: Vec<_> = self
                    .interactions
                    .values()
                    .filter(|item| {
                        item.method == "item/fileChange/requestApproval"
                            && key.is_some()
                            && Self::file_change_key(&item.params, &text(&item.params, "itemId"))
                                == key
                    })
                    .map(|item| (item.id.clone(), item.method.clone(), item.params.clone()))
                    .collect();
                for (id, method, params) in pending {
                    let mut updates = self.interaction(id, &method, &params)?;
                    if !root_matches {
                        for update in &mut updates {
                            if let ConversationAction::Emit(ConversationEvent::Interaction {
                                title,
                                ..
                            }) = update
                            {
                                *title = format!("Subtask · {title}");
                            }
                        }
                    }
                    review_updates.extend(updates);
                }
                if method == "item/fileChange/patchUpdated" {
                    return Ok(review_updates);
                }
            }
        }
        if !root_matches {
            if let Some(thread) = p
                .get("threadId")
                .and_then(Value::as_str)
                .filter(|id| self.owns_child(id))
            {
                if method == "turn/started" {
                    self.running_children.insert(thread.into());
                } else if method == "turn/completed" {
                    self.running_children.remove(thread);
                }
            }
            if message.get("id").is_none() {
                return Ok(review_updates);
            }
            if let (Some(id), Some(thread)) =
                (message.get("id"), p.get("threadId").and_then(Value::as_str))
            {
                if self.owns_child(thread) {
                    let mut out = self.interaction(id.clone(), &method, p)?;
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
            }
            // 子线程通知不能覆盖 root；子线程审批也不能由 root 的用户卡片误答。
            return Ok(message.get("id").map(|id| ConversationAction::Send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"conversation/root_mismatch"}}))).into_iter().collect());
        }
        if let Some(id) = message.get("id") {
            return self.interaction(id.clone(), &method, p);
        }
        if let Some(turn) = p
            .get("turnId")
            .or_else(|| p.pointer("/turn/id"))
            .and_then(Value::as_str)
        {
            if self
                .active
                .as_ref()
                .and_then(|a| a.turn.as_deref())
                .is_some_and(|id| id != turn)
            {
                return Ok(vec![]);
            }
        }
        match method.as_str() {
            "turn/started" => {
                if let Some(active) = &mut self.active {
                    active.turn = Some(text(&p["turn"], "id"));
                }
                Ok(self.consumed(None))
            }
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let kind = text(item, "type");
                if kind == "userMessage" {
                    if self.active.as_ref().is_some_and(|a| {
                        item.get("clientId").and_then(Value::as_str) == Some(a.request.as_str())
                    }) {
                        return Ok(self.consumed(Some(text(item, "id"))));
                    }
                    return Ok(vec![]);
                }
                if [
                    "commandExecution",
                    "fileChange",
                    "mcpToolCall",
                    "dynamicToolCall",
                    "webSearch",
                ]
                .contains(&kind.as_str())
                {
                    let tool_id = text(item, "id");
                    if item["status"] == "inProgress" && !tool_id.is_empty() {
                        self.running_tools.insert(tool_id.clone());
                    } else if method == "item/completed" {
                        self.running_tools.remove(&tool_id);
                    }
                    self.stream_sequence += 1;
                    self.stream_reasoning = None;
                    let status = if method == "item/started" {
                        ConversationToolStatus::Started
                    } else if item["status"] == "failed" {
                        ConversationToolStatus::Failed
                    } else {
                        ConversationToolStatus::Completed
                    };
                    let mut out = vec![emit(ConversationEvent::Tool {
                        request_id: self.request_id(),
                        tool_id: text(item, "id"),
                        name: kind,
                        input: item.clone(),
                        output: item
                            .get("aggregatedOutput")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        status,
                    })];
                    out.extend(review_updates);
                    if method == "item/completed" {
                        out.extend(self.task_writes(p, item));
                    }
                    return Ok(out);
                }
                Ok(vec![])
            }
            "item/agentMessage/delta"
            | "item/reasoning/textDelta"
            | "item/reasoning/summaryTextDelta" => {
                let reasoning = method != "item/agentMessage/delta";
                let item = text(p, "itemId");
                let segment_id = if item.is_empty() {
                    if self.stream_reasoning != Some(reasoning) {
                        self.stream_sequence += 1;
                        self.stream_reasoning = Some(reasoning);
                    }
                    format!("fallback-{}", self.stream_sequence)
                } else {
                    item
                };
                Ok(vec![emit(ConversationEvent::Output {
                    segment_id,
                    request_id: self.request_id(),
                    text: text(p, "delta"),
                    reasoning,
                })])
            }
            "turn/completed" => {
                let Some(active) = self.active.take() else {
                    return Ok(vec![]);
                };
                let status = text(&p["turn"], "status");
                // 父turn结束不等于子任务审批结束；只认原生serverRequest/resolved或该请求的显式回复。
                self.ready = true;
                let mut out = vec![];
                if status == "failed" {
                    out.push(emit(ConversationEvent::Failed {
                        request_id: Some(active.request.clone()),
                        reason: "conversation/native_turn_failed".into(),
                    }));
                }
                out.push(emit(ConversationEvent::Completed {
                    request_id: active.request,
                    stop_reason: if status == "completed"
                        && (!self.running_tools.is_empty()
                            || self.running_children.iter().any(|id| self.owns_child(id)))
                    {
                        "awaiting_background".into()
                    } else {
                        status
                    },
                }));
                out.push(emit(ConversationEvent::Ready));
                Ok(out)
            }
            "error" => Ok(vec![emit(ConversationEvent::Failed {
                request_id: self.request_id(),
                reason: "conversation/native_error".into(),
            })]),
            _ => Ok(vec![]),
        }
    }
}

#[cfg(test)]
mod tests;
