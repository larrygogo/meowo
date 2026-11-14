//! 会话运行时交互的统一 GUI 边界。实时所有权仍归 broker 或 PTY，本 DTO 只描述事实。

use serde::{Deserialize, Serialize};

use crate::broker::ApprovalRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRequestKind {
    Question,
    Approval,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRequestStatus {
    Pending,
    Resolved,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeResponseCapability {
    BrokerLive,
    TerminalLive,
    NotResumable { reason: String },
}

/// 终端卡片仍使用插件识别出的原始按键；统一契约只描述事实，不把按键改写成 broker 决策。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct TerminalInteractionOptionDto {
    pub label: String,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub selected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub focused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub position: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct TerminalInteractionDto {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub options: Option<Vec<TerminalInteractionOptionDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRequestDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub request_id: String,
    pub provider: String,
    pub kind: RuntimeRequestKind,
    pub status: RuntimeRequestStatus,
    pub tool_name: String,
    pub description: Option<String>,
    pub input: String,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub permission_suggestions: Vec<serde_json::Value>,
    pub response_capability: RuntimeResponseCapability,
    /// 仅终端屏幕请求携带；broker 请求省略，旧载荷缺少该字段时仍可反序列化。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub terminal: Option<TerminalInteractionDto>,
}

impl RuntimeRequestDto {
    fn broker(request: ApprovalRequest, kind: RuntimeRequestKind, broker_live: bool) -> Self {
        Self {
            session_id: request.session_id,
            request_id: request.request_id,
            provider: request.provider,
            kind,
            status: RuntimeRequestStatus::Pending,
            tool_name: request.tool_name,
            description: request.description,
            input: request.input,
            permission_suggestions: request.permission_suggestions,
            response_capability: if broker_live {
                RuntimeResponseCapability::BrokerLive
            } else {
                RuntimeResponseCapability::NotResumable {
                    reason: "broker_released".into(),
                }
            },
            terminal: None,
        }
    }

    pub fn broker_question(request: ApprovalRequest, broker_live: bool) -> Self {
        Self::broker(request, RuntimeRequestKind::Question, broker_live)
    }

    pub fn broker_approval(request: ApprovalRequest, broker_live: bool) -> Self {
        Self::broker(request, RuntimeRequestKind::Approval, broker_live)
    }

    pub fn with_terminal_state(
        mut self,
        status: RuntimeRequestStatus,
        reason: impl Into<String>,
    ) -> Self {
        self.status = status;
        self.response_capability = RuntimeResponseCapability::NotResumable {
            reason: reason.into(),
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            session_id: 7,
            request_id: "question-1".into(),
            provider: "claude".into(),
            tool_name: "AskUserQuestion".into(),
            description: None,
            input: r#"{"questions":[]}"#.into(),
            permission_suggestions: vec![],
            pre_tool_use: true,
        }
    }

    #[test]
    fn broker_question_exposes_live_response_capability() {
        let value =
            serde_json::to_value(RuntimeRequestDto::broker_question(request(), true)).unwrap();
        assert_eq!(value["kind"], "question");
        assert_eq!(value["status"], "pending");
        assert_eq!(
            value["responseCapability"],
            serde_json::json!({ "type": "broker_live" })
        );
        assert!(value.get("permissionSuggestions").is_some());
    }

    #[test]
    fn released_broker_question_remains_visible_but_not_resumable() {
        let value =
            serde_json::to_value(RuntimeRequestDto::broker_question(request(), false)).unwrap();
        assert_eq!(value["status"], "pending");
        assert_eq!(
            value["responseCapability"],
            serde_json::json!({ "type": "not_resumable", "reason": "broker_released" })
        );
    }

    #[test]
    fn broker_approval_keeps_its_kind_and_permission_options() {
        let mut approval = request();
        approval.tool_name = "Bash".into();
        approval.input = r#"{"command":"cargo test"}"#.into();
        approval.permission_suggestions = vec![serde_json::json!({"destination":"session"})];
        let value =
            serde_json::to_value(RuntimeRequestDto::broker_approval(approval, true)).unwrap();
        assert_eq!(value["kind"], "approval");
        assert_eq!(
            value["responseCapability"],
            serde_json::json!({ "type": "broker_live" })
        );
        assert_eq!(value["permissionSuggestions"][0]["destination"], "session");
    }

    #[test]
    fn terminal_state_is_explicit_and_not_resumable() {
        let dto = RuntimeRequestDto::broker_question(request(), false)
            .with_terminal_state(RuntimeRequestStatus::Resolved, "answered");
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["status"], "resolved");
        assert_eq!(
            value["responseCapability"],
            serde_json::json!({ "type": "not_resumable", "reason": "answered" })
        );
    }

    #[test]
    fn terminal_prompt_keeps_native_options_and_response_capability() {
        let dto = RuntimeRequestDto {
            kind: RuntimeRequestKind::Prompt,
            response_capability: RuntimeResponseCapability::TerminalLive,
            terminal: Some(TerminalInteractionDto {
                text: "Choose a directory".into(),
                options: Some(vec![TerminalInteractionOptionDto {
                    label: "Yes".into(),
                    input: "\r".into(),
                    description: None,
                    selected: None,
                    focused: Some(true),
                    position: Some(1),
                    kind: Some("choice".into()),
                }]),
                details: None,
            }),
            ..RuntimeRequestDto::broker_question(request(), true)
        };
        let value = serde_json::to_value(dto).unwrap();
        assert_eq!(value["kind"], "prompt");
        assert_eq!(
            value["responseCapability"],
            serde_json::json!({"type":"terminal_live"})
        );
        assert_eq!(value["terminal"]["options"][0]["input"], "\r");
    }

    #[test]
    fn legacy_broker_payload_does_not_require_terminal_payload() {
        let mut value =
            serde_json::to_value(RuntimeRequestDto::broker_approval(request(), true)).unwrap();
        value.as_object_mut().unwrap().remove("terminal");
        let dto: RuntimeRequestDto = serde_json::from_value(value).unwrap();
        assert_eq!(dto.terminal, None);
        assert_eq!(
            dto.response_capability,
            RuntimeResponseCapability::BrokerLive
        );
    }
}
