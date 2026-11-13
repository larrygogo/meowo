//! 协议托管的桌面与手机共享数据，不依赖插件或数据库，宿主负责在边界转换。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedNativeRecoveryCursorDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub sequence: i64,
    pub message_index: usize,
    pub text_byte_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedNativeRecoveryChunkDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub sequence: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
    pub message_index: usize,
    pub role: String,
    pub text: String,
    pub text_byte_offset: usize,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedNativeRecoveryPageDto {
    pub chunks: Vec<ManagedNativeRecoveryChunkDto>,
    pub next_cursor: Option<ManagedNativeRecoveryCursorDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedChoiceDto {
    pub id: String,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedQuestionDto {
    pub id: String,
    pub title: String,
    pub text: String,
    pub multiple: bool,
    pub options: Vec<ManagedChoiceDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedInteractionDto {
    pub interaction_id: String,
    pub title: String,
    pub detail: String,
    pub choices: Vec<ManagedChoiceDto>,
    pub questions: Vec<ManagedQuestionDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedAttachmentDto {
    pub path: String,
    pub mime_type: Option<String>,
    pub data_base64: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagedGateRefDto {
    pub task_dir: String,
    pub gate_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedConversationDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub provider: String,
    pub cwd: String,
    pub profile_id: Option<String>,
    pub native_session_id: Option<String>,
    pub phase: String,
    pub adopted: bool,
    pub runtime_owner: Option<String>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub runtime_pid: Option<i64>,
    pub last_error: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedDeliveryDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub delivery_id: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub ordinal: i64,
    pub text: String,
    pub origin: String,
    pub attachments: Vec<ManagedAttachmentDto>,
    pub gate_ref: Option<ManagedGateRefDto>,
    pub state: String,
    pub native_message_id: Option<String>,
    pub last_error: Option<String>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub accepted_at: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub consumed_at: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub completed_at: Option<i64>,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedEventDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    #[cfg_attr(test, ts(type = "number"))]
    pub sequence: i64,
    /// 原生事实经宿主规范化后持久化，前端不得把未知事件当成默认允许。
    #[cfg_attr(test, ts(type = "unknown"))]
    pub payload: serde_json::Value,
    #[cfg_attr(test, ts(type = "number"))]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedEventPageDto {
    pub events: Vec<ManagedEventDto>,
    #[cfg_attr(test, ts(type = "number"))]
    pub next_sequence: i64,
    pub has_more: bool,
}
