//! 插件只编解码原生会话协议，所有进程、网络与持久化由宿主执行，避免再次出现两套归属地基。

use crate::variant::Installation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) mod acp;
pub(crate) mod task_write;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversationTransport {
    JsonLines,
    /// 宿主用随机密码和本机保留端口启动独立服务，args里的{meowo_port}仅替换为该端口。
    HttpServer {
        health_path: String,
        events_path: String,
        username: String,
        password_env: String,
    },
}

#[derive(Debug, Clone)]
pub struct ConversationSpec {
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub transport: ConversationTransport,
    pub ready_timeout_ms: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ConversationInit {
    pub cwd: String,
    pub native_session_id: Option<String>,
    pub model: Option<String>,
    pub approval_mode: Option<String>,
    pub mcp_servers: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationAttachment {
    pub path: String,
    pub mime_type: Option<String>,
    pub data_base64: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ConversationInput {
    Prompt {
        request_id: String,
        text: String,
        attachments: Vec<ConversationAttachment>,
    },
    Cancel,
    Resolve {
        interaction_id: String,
        option_id: Option<String>,
        answers: Value,
    },
    SetModel {
        model: String,
    },
    SetMode {
        dimension: String,
        mode: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationChoice {
    pub id: String,
    pub label: String,
    /// 原生allow_once/allow_always/reject_once/reject_always或未知种类；未知不能当默认允许。
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationQuestion {
    pub id: String,
    pub title: String,
    pub text: String,
    pub multiple: bool,
    pub options: Vec<ConversationChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationToolStatus {
    Started,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationEvent {
    Bound {
        native_session_id: String,
    },
    Ready,
    Accepted {
        request_id: String,
        native_message_id: Option<String>,
    },
    /// 只有原生user消息或明确turn-start事实能作为消费证据，stdio写完和HTTP202都不算。
    Consumed {
        request_id: String,
        native_message_id: Option<String>,
    },
    UserMessage {
        request_id: String,
        text: String,
        native_message_id: Option<String>,
    },
    Output {
        request_id: Option<String>,
        segment_id: String,
        text: String,
        reasoning: bool,
    },
    Tool {
        request_id: Option<String>,
        tool_id: String,
        name: String,
        input: Value,
        output: Option<String>,
        status: ConversationToolStatus,
    },
    /// 插件只在已验证root的成功写工具事实后给出规范调用；宿主不从界面文字猜测认领。
    TaskWrite {
        request_id: String,
        tool_id: String,
        call: Value,
    },
    Interaction {
        interaction_id: String,
        title: String,
        detail: String,
        choices: Vec<ConversationChoice>,
        questions: Vec<ConversationQuestion>,
    },
    InteractionResolved {
        interaction_id: String,
    },
    Completed {
        request_id: String,
        stop_reason: String,
    },
    Failed {
        request_id: Option<String>,
        reason: String,
    },
    /// 原生协议失败的受限投影：只保留固定分类，供隔离探针定位链路，绝不保存原始错误文本。
    NativeDiagnostic {
        provider: String,
        transport: String,
        signal: String,
        category: String,
        error_shape: String,
        status: Option<u16>,
    },
    History {
        messages: Vec<ConversationHistoryMessage>,
    },
    Metadata {
        model: Option<String>,
        mode: Option<String>,
    },
    Catalog {
        models: Vec<ConversationChoice>,
        modes: Vec<ConversationChoice>,
        mode_dimension: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationHistoryMessage {
    pub native_message_id: Option<String>,
    pub segment_id: Option<String>,
    pub request_id: Option<String>,
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub enum ConversationAction {
    Send(Value),
    /// 响应经receive({"_meowo_http": {"tag":...,"status":...,"body":...}})回到同一个codec。
    Http {
        tag: String,
        method: String,
        path: String,
        body: Option<Value>,
    },
    Emit(ConversationEvent),
}

pub trait ConversationCodec: Send {
    /// 宿主在claim发送状态前预检；失败不允许改变active/pending，否则未写协议也会污染下一次投递。
    fn validate_prompt(
        &self,
        _text: &str,
        attachments: &[ConversationAttachment],
    ) -> Result<(), String> {
        if attachments.is_empty() {
            Ok(())
        } else {
            Err("conversation/attachment_unsupported".into())
        }
    }
    fn initialize(&mut self) -> Result<Vec<ConversationAction>, String>;
    fn input(&mut self, input: ConversationInput) -> Result<Vec<ConversationAction>, String>;
    fn receive(&mut self, message: Value) -> Result<Vec<ConversationAction>, String>;
}

pub trait ConversationCap: Sync {
    fn spec(
        &self,
        installation: &Installation,
        version: Option<&str>,
    ) -> Result<ConversationSpec, String>;
    fn codec(&self, init: ConversationInit) -> Box<dyn ConversationCodec>;
}
