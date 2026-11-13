//! ACP v1 的纯状态机；不声明客户端 fs/terminal，使 CLI 使用本地实现，宿主不获得任意反向执行面。
//! 协议：https://agentclientprotocol.com/protocol/v1/initialization /session-setup /tool-calls。
//! Kimi 2.5.3/Gemini 0.65.0 隔离握手确认 loadSession；Gemini 本机
//! gemini-GLQ4MVLB.js:16313/16330 确认 fs 能力 false 时回退 CLI 自己的文件实现。

use super::*;
use serde_json::json;
use std::collections::HashMap;

enum Pending {
    Initialize,
    Bind,
    Configure {
        model: bool,
        value: String,
        startup: bool,
    },
    Prompt(String),
}
struct Permission {
    rpc_id: Value,
    options: Vec<ConversationChoice>,
    form: Option<Vec<ConversationQuestion>>,
}
struct ActivePrompt {
    request_id: String,
    consumed: bool,
}
#[derive(Clone)]
struct ToolFact {
    name: String,
    input: Value,
    kind: Option<String>,
    raw_output: Value,
    started: bool,
    write_emitted: bool,
}

pub(crate) struct AcpCodec {
    init: ConversationInit,
    agent_name: &'static str,
    version: (u32, u32),
    next_id: u64,
    pending: HashMap<u64, Pending>,
    permissions: HashMap<String, Permission>,
    root: Option<String>,
    initialized: bool,
    binding: bool,
    ready: bool,
    closed: bool,
    load_session: bool,
    image: bool,
    audio: bool,
    embedded: bool,
    active: Option<ActivePrompt>,
    history: Vec<ConversationHistoryMessage>,
    replay_request_id: Option<String>,
    tools: HashMap<String, ToolFact>,
    startup_config: Vec<(bool, String)>,
    output_segment: Option<(bool, String)>,
    next_segment: u64,
}

impl AcpCodec {
    pub(crate) fn new(
        init: ConversationInit,
        agent_name: &'static str,
        version: (u32, u32),
    ) -> Self {
        Self {
            root: init.native_session_id.clone(),
            init,
            agent_name,
            version,
            next_id: 0,
            pending: HashMap::new(),
            permissions: HashMap::new(),
            initialized: false,
            binding: false,
            ready: false,
            closed: false,
            load_session: false,
            image: false,
            audio: false,
            embedded: false,
            active: None,
            history: Vec::new(),
            replay_request_id: None,
            tools: HashMap::new(),
            startup_config: Vec::new(),
            output_segment: None,
            next_segment: 0,
        }
    }
    fn request(&mut self, method: &str, params: Value, pending: Pending) -> ConversationAction {
        self.next_id += 1;
        self.pending.insert(self.next_id, pending);
        ConversationAction::Send(
            json!({"jsonrpc":"2.0","id":self.next_id,"method":method,"params":params}),
        )
    }
    fn emit(event: ConversationEvent) -> ConversationAction {
        ConversationAction::Emit(event)
    }
    fn rpc_error(id: Value, code: i64, message: &str) -> ConversationAction {
        ConversationAction::Send(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
        )
    }
    fn matches_root(&self, params: &Value) -> bool {
        self.root
            .as_deref()
            .is_some_and(|id| params.get("sessionId").and_then(Value::as_str) == Some(id))
    }
    fn require_ready(&self) -> Result<&str, String> {
        if self.closed {
            return Err("conversation/closed".into());
        }
        if self.active.is_some() {
            return Err("conversation/busy".into());
        }
        if !self.ready {
            return Err("conversation/not_ready".into());
        }
        self.root
            .as_deref()
            .ok_or_else(|| "conversation/session_unbound".into())
    }
    fn consume(&mut self, actions: &mut Vec<ConversationAction>) {
        // 同一 root、当前 prompt 的原生输出/工具/审批或成功响应证明引擎开始执行；写 stdin 不算。
        // load 回放发生在 active=None/binding=true 时，绝不能成为新消息的消费凭据。
        if !self.binding {
            if let Some(active) = &mut self.active {
                if !active.consumed {
                    active.consumed = true;
                    actions.push(Self::emit(ConversationEvent::Consumed {
                        request_id: active.request_id.clone(),
                        native_message_id: None,
                    }));
                }
            }
        }
    }
    fn configure(&mut self, model: bool, value: String, startup: bool) -> ConversationAction {
        let key = if model { "modelId" } else { "modeId" };
        let mut params = json!({"sessionId":self.root});
        params[key] = json!(value);
        self.request(
            if model {
                "session/set_model"
            } else {
                "session/set_mode"
            },
            params,
            Pending::Configure {
                model,
                value,
                startup,
            },
        )
    }
    fn finish_startup(&mut self) -> Vec<ConversationAction> {
        if self.closed {
            return Vec::new();
        }
        if let Some((model, value)) = self.startup_config.pop() {
            return vec![self.configure(model, value, true)];
        }
        self.ready = true;
        vec![Self::emit(ConversationEvent::Ready)]
    }
    fn metadata(result: &Value) -> Option<ConversationEvent> {
        let model = result
            .pointer("/models/currentModelId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mode = result
            .pointer("/modes/currentModeId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        (model.is_some() || mode.is_some()).then_some(ConversationEvent::Metadata { model, mode })
    }
    fn response(&mut self, message: &Value) -> Result<Vec<ConversationAction>, String> {
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return Err("conversation/response_invalid".into());
        };
        let Some(pending) = self.pending.remove(&id) else {
            return Err("conversation/response_unknown".into());
        };
        if let Some(error) = message.get("error") {
            let code = error.get("code").and_then(Value::as_i64);
            let reason = match code {
                Some(-32000) => "conversation/auth_required",
                Some(-32601) => "conversation/method_unsupported",
                Some(-32602) => "conversation/params_invalid",
                // 其余错误按 message/data 归类（只在内存比对，不存原文）；认不出才是笼统错误。
                _ => super::classify_native_fault(&format!(
                    "{} {}",
                    error.get("message").and_then(Value::as_str).unwrap_or(""),
                    error.get("data").map(Value::to_string).unwrap_or_default()
                ))
                .unwrap_or("conversation/native_error"),
            };
            let request_id = if let Pending::Prompt(request_id) = pending {
                self.active = None;
                self.ready = true;
                Some(request_id)
            } else {
                self.ready = false;
                None
            };
            self.binding = false;
            let mut actions = self.cancel_permissions();
            actions.push(Self::emit(ConversationEvent::Failed {
                request_id,
                reason: reason.into(),
            }));
            return Ok(actions);
        }
        let result = message
            .get("result")
            .filter(|result| result.is_object())
            .ok_or("conversation/response_invalid")?;
        match pending {
            Pending::Initialize => {
                if result.get("protocolVersion").and_then(Value::as_u64) != Some(1) {
                    return Err("conversation/protocol_unsupported".into());
                }
                if result.pointer("/agentInfo/name").and_then(Value::as_str)
                    != Some(self.agent_name)
                    || !crate::lead_install::version_matches(
                        result.pointer("/agentInfo/version").and_then(Value::as_str),
                        self.version,
                    )
                {
                    return Err("conversation/version_unsupported".into());
                }
                self.initialized = true;
                self.load_session = result
                    .pointer("/agentCapabilities/loadSession")
                    .and_then(Value::as_bool)
                    == Some(true);
                self.image = result
                    .pointer("/agentCapabilities/promptCapabilities/image")
                    .and_then(Value::as_bool)
                    == Some(true);
                self.audio = result
                    .pointer("/agentCapabilities/promptCapabilities/audio")
                    .and_then(Value::as_bool)
                    == Some(true);
                self.embedded = result
                    .pointer("/agentCapabilities/promptCapabilities/embeddedContext")
                    .and_then(Value::as_bool)
                    == Some(true);
                if self.root.is_some() && !self.load_session {
                    return Err("conversation/resume_unsupported".into());
                }
                let mut params = json!({"cwd":self.init.cwd,"mcpServers":self.init.mcp_servers});
                let method = if let Some(root) = &self.root {
                    params["sessionId"] = json!(root);
                    "session/load"
                } else {
                    "session/new"
                };
                self.binding = true;
                Ok(vec![self.request(method, params, Pending::Bind)])
            }
            Pending::Bind => {
                self.finish_history_user();
                if let Some(expected) = &self.root {
                    if result
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .is_some_and(|id| id != expected)
                    {
                        return Err("conversation/session_mismatch".into());
                    }
                } else {
                    let id = result
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .filter(|id| {
                            !id.is_empty() && id.len() <= 2048 && !id.chars().any(char::is_control)
                        })
                        .ok_or("conversation/session_unbound")?;
                    self.root = Some(id.into());
                }
                self.binding = false;
                let mut actions = vec![Self::emit(ConversationEvent::Bound {
                    native_session_id: self.root.clone().ok_or("conversation/session_unbound")?,
                })];
                if !self.history.is_empty() {
                    actions.push(Self::emit(ConversationEvent::History {
                        messages: std::mem::take(&mut self.history),
                    }));
                }
                if let Some(event) = Self::metadata(result) {
                    actions.push(Self::emit(event));
                }
                if let Some(event) = catalog(result) {
                    actions.push(Self::emit(event));
                }
                if let Some(mode) = self.init.approval_mode.take() {
                    self.startup_config.push((false, mode));
                }
                if let Some(model) = self.init.model.take() {
                    self.startup_config.push((true, model));
                }
                actions.extend(self.finish_startup());
                Ok(actions)
            }
            Pending::Configure {
                model,
                value,
                startup,
            } => {
                let event = ConversationEvent::Metadata {
                    model: if model { Some(value.clone()) } else { None },
                    mode: if model { None } else { Some(value) },
                };
                let mut actions = vec![Self::emit(event)];
                if startup {
                    actions.extend(self.finish_startup());
                } else {
                    self.ready = true;
                    actions.push(Self::emit(ConversationEvent::Ready));
                }
                Ok(actions)
            }
            Pending::Prompt(request_id) => {
                let stop_reason = result
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .ok_or("conversation/response_invalid")?
                    .to_owned();
                if ![
                    "end_turn",
                    "max_tokens",
                    "max_turn_requests",
                    "refusal",
                    "cancelled",
                ]
                .contains(&stop_reason.as_str())
                {
                    return Err("conversation/stop_reason_unknown".into());
                }
                // 本机 Kimi onTurnEnded 将非认证 API 失败也映射成 end_turn；空成功回包不足以证明执行。
                // 不猜错误文案、不自动重发，让宿主保留消息并明确提示无法确认。
                if self.agent_name == "Kimi Code CLI"
                    && stop_reason != "cancelled"
                    && self.active.as_ref().is_some_and(|active| !active.consumed)
                {
                    self.active = None;
                    self.ready = !self.closed;
                    let mut actions = vec![Self::emit(ConversationEvent::Failed {
                        request_id: Some(request_id),
                        reason: "conversation/no_execution_evidence".into(),
                    })];
                    actions.extend(self.cancel_permissions());
                    if !self.closed {
                        actions.push(Self::emit(ConversationEvent::Ready));
                    }
                    return Ok(actions);
                }
                let mut actions = Vec::new();
                if stop_reason != "cancelled" {
                    self.consume(&mut actions);
                }
                self.active = None;
                self.ready = !self.closed;
                self.tools.clear();
                actions.extend(self.cancel_permissions());
                actions.push(Self::emit(ConversationEvent::Completed {
                    request_id,
                    stop_reason,
                }));
                if !self.closed {
                    actions.push(Self::emit(ConversationEvent::Ready));
                }
                Ok(actions)
            }
        }
    }
    fn cancel_permissions(&mut self) -> Vec<ConversationAction> {
        let mut actions = Vec::new();
        for (interaction_id, pending) in self.permissions.drain() {
            let result = if pending.form.is_some() {
                json!({"action":"cancel"})
            } else {
                json!({"outcome":{"outcome":"cancelled"}})
            };
            actions.push(ConversationAction::Send(
                json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":result}),
            ));
            actions.push(Self::emit(ConversationEvent::InteractionResolved {
                interaction_id,
            }));
        }
        actions
    }
    fn permission(&mut self, id: Value, params: &Value) -> Result<Vec<ConversationAction>, String> {
        if !self.matches_root(params) || self.binding || self.active.is_none() {
            return Ok(vec![Self::rpc_error(
                id,
                -32602,
                "Root session has no active prompt",
            )]);
        }
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .ok_or("conversation/permission_invalid")?;
        let mut choices = Vec::new();
        for option in options {
            let choice = ConversationChoice {
                id: option
                    .get("optionId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or("conversation/permission_invalid")?
                    .into(),
                label: option
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("conversation/permission_invalid")?
                    .into(),
                kind: option
                    .get("kind")
                    .and_then(Value::as_str)
                    .ok_or("conversation/permission_invalid")?
                    .into(),
            };
            if !["allow_once", "allow_always", "reject_once", "reject_always"]
                .contains(&choice.kind.as_str())
                || choices
                    .iter()
                    .any(|old: &ConversationChoice| old.id == choice.id)
            {
                return Ok(vec![Self::rpc_error(
                    id,
                    -32602,
                    "Unsupported permission options",
                )]);
            }
            choices.push(choice);
        }
        if choices.is_empty() {
            return Ok(vec![Self::rpc_error(id, -32602, "No permission options")]);
        }
        if self
            .permissions
            .values()
            .any(|permission| permission.rpc_id == id)
        {
            return Ok(vec![Self::rpc_error(id, -32600, "Duplicate request id")]);
        }
        self.next_id += 1;
        let interaction_id = format!("acp-permission-{}", self.next_id);
        self.permissions.insert(
            interaction_id.clone(),
            Permission {
                rpc_id: id,
                options: choices.clone(),
                form: None,
            },
        );
        let mut actions = Vec::new();
        self.consume(&mut actions);
        let tool = params.get("toolCall").unwrap_or(&Value::Null);
        actions.push(Self::emit(ConversationEvent::Interaction {
            interaction_id,
            title: tool
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Tool permission")
                .into(),
            detail: content_text(tool.get("content").unwrap_or(&Value::Null)),
            choices,
            questions: Vec::new(),
        }));
        Ok(actions)
    }
    fn elicitation(
        &mut self,
        id: Value,
        params: &Value,
    ) -> Result<Vec<ConversationAction>, String> {
        if self.agent_name != "Kimi Code CLI"
            || !self.matches_root(params)
            || self.binding
            || self.active.is_none()
            || params.get("mode").and_then(Value::as_str) != Some("form")
        {
            return Ok(vec![Self::rpc_error(
                id,
                -32602,
                "Unsupported form or session",
            )]);
        }
        let schema = params
            .get("requestedSchema")
            .ok_or("conversation/question_unsupported")?;
        let questions = form_questions(schema)?;
        if self
            .permissions
            .values()
            .any(|pending| pending.rpc_id == id)
        {
            return Ok(vec![Self::rpc_error(id, -32600, "Duplicate request id")]);
        }
        let choices = [
            ("accept", "Submit"),
            ("decline", "Decline"),
            ("cancel", "Cancel"),
        ]
        .into_iter()
        .map(|(id, label)| ConversationChoice {
            id: id.into(),
            label: label.into(),
            kind: id.into(),
        })
        .collect::<Vec<_>>();
        self.next_id += 1;
        let interaction_id = format!("acp-form-{}", self.next_id);
        self.permissions.insert(
            interaction_id.clone(),
            Permission {
                rpc_id: id,
                options: choices.clone(),
                form: Some(questions.clone()),
            },
        );
        let mut actions = Vec::new();
        self.consume(&mut actions);
        actions.push(Self::emit(ConversationEvent::Interaction {
            interaction_id,
            title: "AskUserQuestion".into(),
            detail: params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            choices,
            questions,
        }));
        Ok(actions)
    }
    fn update(&mut self, params: &Value) -> Result<Vec<ConversationAction>, String> {
        if !self.matches_root(params) {
            return Ok(Vec::new());
        }
        let update = params.get("update").ok_or("conversation/event_invalid")?;
        let kind = update
            .get("sessionUpdate")
            .and_then(Value::as_str)
            .ok_or("conversation/event_invalid")?;
        let request_id = self.active.as_ref().map(|active| active.request_id.clone());
        if self.binding {
            if kind != "user_message_chunk" {
                self.finish_history_user();
            }
            if matches!(
                kind,
                "user_message_chunk" | "agent_message_chunk" | "agent_thought_chunk"
            ) {
                let text = update
                    .pointer("/content/text")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let role = if kind == "user_message_chunk" {
                    "user"
                } else if kind == "agent_thought_chunk" {
                    "reasoning"
                } else {
                    "assistant"
                };
                if let Some(last) = self.history.last_mut().filter(|last| last.role == role) {
                    last.text.push_str(text);
                } else {
                    self.history.push(ConversationHistoryMessage {
                        native_message_id: None,
                        segment_id: None,
                        request_id: if role == "user" {
                            None
                        } else {
                            self.replay_request_id.clone()
                        },
                        role: role.into(),
                        text: text.into(),
                    });
                }
            } else if matches!(kind, "tool_call" | "tool_call_update") {
                // load保留工具证据供独立恢复记录；绝不把历史工具当当前成功写事实或猜正文分段ID。
                self.history.push(ConversationHistoryMessage {
                    native_message_id: None,
                    segment_id: None,
                    request_id: self.replay_request_id.clone(),
                    role: "tool".into(),
                    text: update.to_string(),
                });
            }
            return Ok(Vec::new());
        }
        let mut actions = Vec::new();
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                // ready 时的延迟通知没有当前 delivery，不能混进下一次发送。
                if self.active.is_none() {
                    return Ok(actions);
                }
                let Some(text) = update.pointer("/content/text").and_then(Value::as_str) else {
                    return Ok(actions);
                };
                let reasoning = kind == "agent_thought_chunk";
                if self
                    .output_segment
                    .as_ref()
                    .is_none_or(|(previous, _)| *previous != reasoning)
                {
                    self.next_segment += 1;
                    self.output_segment = Some((
                        reasoning,
                        format!(
                            "acp-{}-segment-{}",
                            request_id.as_deref().ok_or("conversation/event_invalid")?,
                            self.next_segment
                        ),
                    ));
                }
                let segment_id = self
                    .output_segment
                    .as_ref()
                    .map(|(_, id)| id.clone())
                    .ok_or("conversation/event_invalid")?;
                self.consume(&mut actions);
                actions.push(Self::emit(ConversationEvent::Output {
                    request_id,
                    segment_id,
                    text: text.into(),
                    reasoning,
                }));
            }
            "user_message_chunk" => {
                if self.active.is_some() {
                    self.consume(&mut actions);
                }
            }
            "tool_call" | "tool_call_update" => {
                self.output_segment = None;
                if self.active.is_none() {
                    return Ok(actions);
                }
                let tool_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .ok_or("conversation/event_invalid")?
                    .to_owned();
                let previous = self.tools.get(&tool_id);
                let name = update
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| previous.map(|fact| fact.name.clone()))
                    .unwrap_or_else(|| "Tool".into());
                let input = update
                    .get("rawInput")
                    .cloned()
                    .or_else(|| previous.map(|fact| fact.input.clone()))
                    .unwrap_or(Value::Null);
                let status = match update.get("status").and_then(Value::as_str) {
                    Some("pending") => ConversationToolStatus::Started,
                    Some("in_progress") => ConversationToolStatus::Running,
                    Some("completed") => ConversationToolStatus::Completed,
                    Some("failed") => ConversationToolStatus::Failed,
                    None if kind == "tool_call" => ConversationToolStatus::Started,
                    None => ConversationToolStatus::Running,
                    _ => return Err("conversation/tool_status_unknown".into()),
                };
                let mut fact = ToolFact {
                    name: name.clone(),
                    input: input.clone(),
                    kind: update
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or_else(|| previous.and_then(|fact| fact.kind.clone())),
                    raw_output: update
                        .get("rawOutput")
                        .cloned()
                        .or_else(|| previous.map(|fact| fact.raw_output.clone()))
                        .unwrap_or(Value::Null),
                    started: kind == "tool_call" || previous.is_some_and(|fact| fact.started),
                    write_emitted: previous.is_some_and(|fact| fact.write_emitted),
                };
                let task_write = if status == ConversationToolStatus::Completed
                    && fact.started
                    && !fact.write_emitted
                {
                    task_write_call(self.agent_name, &fact, update)
                } else {
                    None
                };
                if task_write.is_some() {
                    fact.write_emitted = true;
                }
                self.tools.insert(tool_id.clone(), fact);
                self.consume(&mut actions);
                let text = content_text(update.get("content").unwrap_or(&Value::Null));
                let output = update
                    .get("rawOutput")
                    .map(|output| {
                        output
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| output.to_string())
                    })
                    .or_else(|| (!text.is_empty()).then_some(text));
                actions.push(Self::emit(ConversationEvent::Tool {
                    request_id: request_id.clone(),
                    tool_id: tool_id.clone(),
                    name,
                    input,
                    output,
                    status,
                }));
                if let (Some(request_id), Some(call)) = (request_id, task_write) {
                    actions.push(Self::emit(ConversationEvent::TaskWrite {
                        request_id,
                        tool_id,
                        call,
                    }));
                }
            }
            "current_mode_update" => {
                actions.push(Self::emit(ConversationEvent::Metadata {
                    model: None,
                    mode: update
                        .get("currentModeId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                }));
            }
            "config_option_update" => {
                let result = json!({"configOptions":update.get("configOptions").cloned().unwrap_or(Value::Null)});
                if let Some(event) = catalog(&result) {
                    actions.push(Self::emit(event));
                }
            }
            // 原生扩展通知允许忽略；反向 RPC 仍必须明确 method-not-found，不会自动批准。
            _ => {}
        }
        Ok(actions)
    }
}

impl AcpCodec {
    fn finish_history_user(&mut self) {
        let Some(last) = self
            .history
            .last_mut()
            .filter(|message| message.role == "user")
        else {
            return;
        };
        if last.request_id.is_some() {
            return;
        }
        let (text, request_id) = strip_delivery_marker(&last.text);
        let text = text.to_owned();
        let request_id = request_id.map(str::to_owned);
        self.replay_request_id = request_id.clone();
        if request_id.is_some() {
            last.text = text;
            last.request_id = request_id;
        }
    }
}

const DELIVERY_MARKER: &str = "\n\n<!-- meowo:delivery=";
fn valid_delivery_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 160
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
fn delivery_marker(id: &str) -> String {
    format!("{DELIVERY_MARKER}{id} -->")
}
fn strip_delivery_marker(text: &str) -> (&str, Option<&str>) {
    let Some(body) = text.strip_suffix(" -->") else {
        return (text, None);
    };
    let Some((original, id)) = body.rsplit_once(DELIVERY_MARKER) else {
        return (text, None);
    };
    if valid_delivery_id(id) {
        (original, Some(id))
    } else {
        (text, None)
    }
}

impl ConversationCodec for AcpCodec {
    fn validate_prompt(
        &self,
        _text: &str,
        attachments: &[ConversationAttachment],
    ) -> Result<(), String> {
        self.require_ready()?;
        for attachment in attachments {
            let mime = attachment
                .mime_type
                .as_deref()
                .ok_or("conversation/attachment_unsupported")?;
            if attachment.data_base64.is_none()
                || !(mime.starts_with("image/") && self.image
                    || mime.starts_with("audio/") && self.audio)
            {
                return Err("conversation/attachment_unsupported".into());
            }
        }
        Ok(())
    }
    fn initialize(&mut self) -> Result<Vec<ConversationAction>, String> {
        if self.initialized || !self.pending.is_empty() || self.closed {
            return Err("conversation/protocol_state".into());
        }
        let mut capabilities =
            json!({"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false});
        // 本机 Kimi 源码表明多题/多选需要原生 form；Gemini 未取证，不能从 SDK 支持推断引擎能力。
        if self.agent_name == "Kimi Code CLI" {
            capabilities["elicitation"] = json!({"form":true});
        }
        Ok(vec![self.request("initialize",json!({"protocolVersion":1,"clientInfo":{"name":"meowo","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":capabilities}),Pending::Initialize)])
    }
    fn input(&mut self, input: ConversationInput) -> Result<Vec<ConversationAction>, String> {
        let shutting_down = matches!(&input, ConversationInput::Shutdown);
        match input {
            ConversationInput::Prompt {
                request_id,
                text,
                attachments,
            } => {
                let root = self.require_ready()?.to_owned();
                if !valid_delivery_id(&request_id) {
                    return Err("conversation/delivery_id_invalid".into());
                }
                if text.is_empty() && attachments.is_empty() {
                    return Err("conversation/message_invalid".into());
                }
                let mut prompt = vec![json!({"type":"text","text":text})];
                for attachment in attachments {
                    if let (Some(mime), Some(data)) =
                        (&attachment.mime_type, &attachment.data_base64)
                    {
                        let kind = if mime.starts_with("image/") && self.image {
                            "image"
                        } else if mime.starts_with("audio/") && self.audio {
                            "audio"
                        } else {
                            return Err("conversation/attachment_unsupported".into());
                        };
                        prompt.push(json!({"type":kind,"mimeType":mime,"data":data}));
                    } else {
                        return Err("conversation/attachment_unsupported".into());
                    }
                }
                // 最后单独追加文本块，附件之后仍保持完整suffix，GUI原文不带内部tag。
                prompt.push(json!({"type":"text","text":delivery_marker(&request_id)}));
                self.ready = false;
                self.tools.clear();
                self.output_segment = None;
                self.next_segment = 0;
                self.active = Some(ActivePrompt {
                    request_id: request_id.clone(),
                    consumed: false,
                });
                // ACP session/prompt 没有独立接收回包，Accepted 表示已交给本连接，不把它冒充 Consumed。
                Ok(vec![self.request(
                    "session/prompt",
                    json!({"sessionId":root,"prompt":prompt}),
                    Pending::Prompt(request_id),
                )])
            }
            ConversationInput::Cancel | ConversationInput::Shutdown => {
                if shutting_down {
                    self.closed = true;
                    self.ready = false;
                }
                let mut actions = self.cancel_permissions();
                if self.active.is_some() {
                    actions.push(ConversationAction::Send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.root}})));
                }
                Ok(std::mem::take(&mut actions))
            }
            ConversationInput::Resolve {
                interaction_id,
                option_id,
                answers,
            } => {
                let pending = self
                    .permissions
                    .get(&interaction_id)
                    .ok_or("conversation/interaction_stale")?;
                if let Some(option) = &option_id {
                    if !pending.options.iter().any(|choice| &choice.id == option) {
                        return Err("conversation/option_invalid".into());
                    }
                }
                if let Some(questions) = &pending.form {
                    if option_id.as_deref() == Some("accept") {
                        validate_answers(questions, &answers)?;
                    }
                } else if !answers.is_null() && answers != json!({}) {
                    return Err("conversation/question_unsupported".into());
                }
                let pending = self
                    .permissions
                    .remove(&interaction_id)
                    .ok_or("conversation/interaction_stale")?;
                let result = if pending.form.is_some() {
                    let action = option_id.as_deref().unwrap_or("cancel");
                    if action == "accept" {
                        json!({"action":action,"content":answers})
                    } else {
                        json!({"action":action})
                    }
                } else {
                    let outcome = if let Some(option_id) = option_id {
                        json!({"outcome":"selected","optionId":option_id})
                    } else {
                        json!({"outcome":"cancelled"})
                    };
                    json!({"outcome":outcome})
                };
                Ok(vec![
                    ConversationAction::Send(
                        json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":result}),
                    ),
                    Self::emit(ConversationEvent::InteractionResolved { interaction_id }),
                ])
            }
            ConversationInput::SetModel { model } => {
                self.require_ready()?;
                self.ready = false;
                Ok(vec![self.configure(true, model, false)])
            }
            ConversationInput::SetMode { dimension, mode } => {
                if dimension != "mode" {
                    return Err("conversation/mode_dimension_unsupported".into());
                }
                self.require_ready()?;
                self.ready = false;
                Ok(vec![self.configure(false, mode, false)])
            }
        }
    }
    fn receive(&mut self, message: Value) -> Result<Vec<ConversationAction>, String> {
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err("conversation/frame_invalid".into());
        }
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            let params = message.get("params").unwrap_or(&Value::Null);
            if let Some(id) = message.get("id") {
                if !id.is_string() && !id.is_number() {
                    return Ok(vec![Self::rpc_error(
                        Value::Null,
                        -32600,
                        "Invalid request id",
                    )]);
                }
                let result = match method {
                    "session/request_permission" => self.permission(id.clone(), params),
                    "elicitation/create" => self.elicitation(id.clone(), params),
                    _ => Ok(vec![Self::rpc_error(
                        id.clone(),
                        -32601,
                        "Method not supported by this client",
                    )]),
                };
                return Ok(result.unwrap_or_else(|_| {
                    vec![Self::rpc_error(
                        id.clone(),
                        -32602,
                        "Unsupported request parameters",
                    )]
                }));
            }
            if method == "session/update" {
                self.update(params)
            } else {
                Ok(Vec::new())
            }
        } else {
            self.response(&message)
        }
    }
}

fn content_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block.pointer("/content/text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn task_write_call(agent: &str, fact: &ToolFact, update: &Value) -> Option<Value> {
    if fact.kind.as_deref() != Some("edit")
        || fact.input.get("background").and_then(Value::as_bool) == Some(true)
        || fact.input.get("run_in_background").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let path = if agent == "Kimi Code CLI" {
        // Kimi本机events-map：kind=edit仅由Write/Edit生成，completed来自isError=false，rawInput保留args。
        if update.get("rawOutput").is_none() || fact.raw_output.get("error").is_some() {
            return None;
        }
        fact.input.get("path")?.as_str()?
    } else if agent == "gemini-cli" {
        // Gemini本机toToolCallContent先检查ToolResult.error；执行后completed的diff才证明写入。
        // 不能复用执行前的permission diff，亦不能从displayTitle/模型call id猜函数名或shell exit0。
        update
            .get("content")?
            .as_array()?
            .iter()
            .find_map(|block| {
                if block.get("type").and_then(Value::as_str) != Some("diff")
                    || !block.get("newText").is_some_and(Value::is_string)
                {
                    return None;
                }
                block.get("path").and_then(Value::as_str)
            })?
    } else {
        return None;
    };
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    Some(json!({"name":"Write","input":{"file_path":path}}))
}

fn catalog(result: &Value) -> Option<ConversationEvent> {
    let mut models = result
        .pointer("/models/availableModels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            Some(ConversationChoice {
                id: model.get("modelId")?.as_str()?.into(),
                label: model.get("name")?.as_str()?.into(),
                kind: "model".into(),
            })
        })
        .collect::<Vec<_>>();
    let mut modes = result
        .pointer("/modes/availableModes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            Some(ConversationChoice {
                id: mode.get("id")?.as_str()?.into(),
                label: mode.get("name")?.as_str()?.into(),
                kind: "mode".into(),
            })
        })
        .collect::<Vec<_>>();
    for config in result
        .get("configOptions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = config.get("id").and_then(Value::as_str) else {
            continue;
        };
        let target = match id {
            "model" => &mut models,
            "mode" => &mut modes,
            _ => continue,
        };
        // Kimi 的原生 select 清单使用 value/name；不从 CLI 的 slash 菜单或旧静态预设补齐。
        if let Some(options) = config.get("options").and_then(Value::as_array) {
            let choices = options
                .iter()
                .filter_map(|option| {
                    Some(ConversationChoice {
                        id: option.get("value")?.as_str()?.into(),
                        label: option.get("name")?.as_str()?.into(),
                        kind: id.into(),
                    })
                })
                .collect::<Vec<_>>();
            if !choices.is_empty() {
                *target = choices;
            }
        }
    }
    if models.is_empty() && modes.is_empty() {
        None
    } else {
        let mode_dimension = (!modes.is_empty()).then(|| "mode".into());
        Some(ConversationEvent::Catalog {
            models,
            modes,
            mode_dimension,
        })
    }
}

fn form_questions(schema: &Value) -> Result<Vec<ConversationQuestion>, String> {
    let invalid = || "conversation/question_unsupported".to_string();
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(invalid());
    }
    if schema.as_object().is_none_or(|object| {
        object
            .keys()
            .any(|key| !matches!(key.as_str(), "type" | "properties" | "required"))
    }) {
        return Err(invalid());
    }
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    // 当前表单模型没有可选字段/数字范围等约束；未知 schema 明确拒绝，不能静默丢约束。
    if properties.is_empty()
        || properties.len() > 32
        || properties.len() != required.len()
        || properties.keys().any(|key| !required.contains(&json!(key)))
    {
        return Err(invalid());
    }
    let mut questions = Vec::new();
    for (id, field) in properties {
        if field.as_object().is_none_or(|field| {
            field.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "type" | "title" | "description" | "oneOf" | "items" | "minItems"
                )
            })
        }) {
            return Err(invalid());
        }
        let multiple = field.get("type").and_then(Value::as_str) == Some("array");
        if (multiple && field.get("oneOf").is_some())
            || (!multiple && (field.get("items").is_some() || field.get("minItems").is_some()))
        {
            return Err(invalid());
        }
        let options = if multiple {
            if field.get("minItems").and_then(Value::as_u64) != Some(1) {
                return Err(invalid());
            }
            if field
                .get("items")
                .and_then(Value::as_object)
                .is_none_or(|items| items.len() != 1 || !items.contains_key("anyOf"))
            {
                return Err(invalid());
            }
            field
                .pointer("/items/anyOf")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?
        } else {
            if field.get("type").and_then(Value::as_str) != Some("string") {
                return Err(invalid());
            }
            field
                .get("oneOf")
                .and_then(Value::as_array)
                .ok_or_else(invalid)?
        };
        if options.is_empty() || options.len() > 128 {
            return Err(invalid());
        }
        let mut choices = Vec::new();
        for option in options {
            if option.as_object().is_none_or(|option| {
                option
                    .keys()
                    .any(|key| !matches!(key.as_str(), "const" | "title" | "description"))
            }) {
                return Err(invalid());
            }
            let value = option
                .get("const")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            if choices
                .iter()
                .any(|old: &ConversationChoice| old.id == value)
            {
                return Err(invalid());
            }
            choices.push(ConversationChoice {
                id: value.into(),
                label: option
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(value)
                    .into(),
                kind: "option".into(),
            });
        }
        questions.push(ConversationQuestion {
            id: id.clone(),
            title: field
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .into(),
            text: field
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
            multiple,
            options: choices,
        });
    }
    Ok(questions)
}

fn validate_answers(questions: &[ConversationQuestion], answers: &Value) -> Result<(), String> {
    let invalid = || "conversation/answer_invalid".to_string();
    let answers = answers.as_object().ok_or_else(invalid)?;
    if answers.len() != questions.len() {
        return Err(invalid());
    }
    for question in questions {
        let answer = answers.get(&question.id).ok_or_else(invalid)?;
        let values = if question.multiple {
            answer
                .as_array()
                .ok_or_else(invalid)?
                .iter()
                .map(|value| value.as_str().ok_or_else(invalid))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![answer.as_str().ok_or_else(invalid)?]
        };
        if values.is_empty()
            || values
                .iter()
                .any(|value| !question.options.iter().any(|option| &option.id == value))
        {
            return Err(invalid());
        }
        let unique = values.iter().collect::<std::collections::HashSet<_>>();
        if unique.len() != values.len() {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
