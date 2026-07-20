//! Rust 后端与 TypeScript 前端之间的 Tauri IPC DTO。

use serde::{Deserialize, Serialize};

/// Provider 日志经插件解析后交给聊天归一化层的稳定消息单元。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatItem {
    UserText {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    AssistantText {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    AssistantDelta {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    Reasoning {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ReasoningDelta {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ToolUse {
        id: String,
        timestamp: Option<String>,
        name: String,
        summary: String,
    },
    ToolResult {
        id: String,
        timestamp: Option<String>,
        tool_use_id: Option<String>,
        text: String,
        is_error: bool,
    },
    Meta {
        id: String,
        timestamp: Option<String>,
        kind: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum PendingReviewKind {
    Approval,
    Question,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum LoginOutcome {
    Success,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LoginDoneEvent {
    pub operation_id: String,
    pub provider: String,
    pub outcome: LoginOutcome,
}

impl PendingReviewKind {
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "approval" => Some(Self::Approval),
            "question" => Some(Self::Question),
            "plan" => Some(Self::Plan),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct AgentModeDto {
    pub dimension: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub title: String,
    pub status: String,
    pub provider: String,
    pub cwd: Option<String>,
    pub supported: bool,
    pub items: Vec<ChatItem>,
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub reset: bool,
    pub pending_review: Option<PendingReviewKind>,
    pub model: Option<String>,
    pub agent_modes: Vec<AgentModeDto>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub context_pct: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub context_window: Option<i64>,
    pub current_activity: Option<String>,
    pub has_more: bool,
    /// hook 驱动的最近往来（UserPromptSubmit / Stop 落库），与 transcript 解析无关。
    /// items 为空（transcript 未落盘/未定位）或该 agent 不提供结构化 transcript 时，
    /// 前端用它们渲染临时时间线——「会话已在工作」不该显示成一片空白。
    pub last_user_text: Option<String>,
    pub last_ai_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub request_id: String,
    pub provider: String,
    pub tool_name: String,
    pub description: Option<String>,
    pub input: String,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub permission_suggestions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PtyOutputEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    /// 自 PTY 启动以来，本帧首字节的绝对偏移。
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PtyExitEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub code: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../app/src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedTerminalSnapshotDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub active: bool,
    pub data: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub start_offset: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_offset: u64,
    pub exited: bool,
    pub exit_code: Option<u32>,
}

impl From<crate::broker::ApprovalRequest> for PendingApprovalDto {
    fn from(request: crate::broker::ApprovalRequest) -> Self {
        Self {
            session_id: request.session_id,
            request_id: request.request_id,
            provider: request.provider,
            tool_name: request.tool_name,
            description: request.description,
            input: request.input,
            permission_suggestions: request.permission_suggestions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_snapshot_uses_the_frontend_camel_case_contract() {
        let value = serde_json::to_value(ManagedTerminalSnapshotDto {
            session_id: 7,
            active: true,
            data: "QUJD".into(),
            start_offset: 10,
            end_offset: 13,
            exited: false,
            exit_code: None,
        })
        .unwrap();
        assert_eq!(value["sessionId"], 7);
        assert_eq!(value["startOffset"], 10);
        assert_eq!(value["endOffset"], 13);
        assert!(value.get("start_offset").is_none());
    }

    /// GUI 边界必须走 DTO 的理由，钉成测试：原始 `broker::ApprovalRequest` 在
    /// `permission_suggestions` 为空时把字段整个 skip 掉（reporter 线路的减负），而前端类型
    /// （ts-rs 从 DTO 生成）承诺该字段**恒在**——app 曾直接把 ApprovalRequest emit 给前端，
    /// codex 的审批（从不带 suggestions）一弹就让 ChatWindow 在 `.map` 上崩掉。
    #[test]
    fn dto_always_carries_permission_suggestions_even_when_empty() {
        let request = crate::broker::ApprovalRequest {
            session_id: 7,
            request_id: "req-1".into(),
            provider: "codex".into(),
            tool_name: "Bash".into(),
            description: None,
            input: "{}".into(),
            permission_suggestions: vec![],
        };
        // 原始线路结构：空列表 → 字段消失（这正是不能拿它喂前端的原因）。
        let raw = serde_json::to_value(&request).unwrap();
        assert!(raw.get("permissionSuggestions").is_none());

        // DTO：字段恒在，空时是 `[]` 而不是缺席。
        let dto_value = serde_json::to_value(PendingApprovalDto::from(request)).unwrap();
        assert_eq!(dto_value["permissionSuggestions"], serde_json::json!([]));
        assert_eq!(dto_value["sessionId"], 7);
        assert_eq!(dto_value["requestId"], "req-1");
    }

    #[test]
    fn chat_contract_keeps_tagged_items_and_rejects_unknown_review_kinds() {
        let item = ChatItem::ToolResult {
            id: "result-1".into(),
            timestamp: None,
            tool_use_id: Some("tool-1".into()),
            text: "ok".into(),
            is_error: false,
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["type"], "tool_result");
        assert_eq!(value["tool_use_id"], "tool-1");
        assert_eq!(PendingReviewKind::from_stored("question"), Some(PendingReviewKind::Question));
        assert_eq!(PendingReviewKind::from_stored("future"), None);
    }
}
