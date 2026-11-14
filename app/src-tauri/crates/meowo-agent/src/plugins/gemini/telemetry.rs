//! AfterAgent 正文沿用 hook；0.65 原生 JSONL 由 transcript 模块归并更新后供统一聊天/分析读取。

use crate::caps::{HookContext, StopOutputs, TelemetryCap};

pub static TELEMETRY: GeminiTelemetry = GeminiTelemetry;

pub struct GeminiTelemetry;

impl TelemetryCap for GeminiTelemetry {
    /// hook 正文优先；模型取原生日志，避免只因 hook 没携带就把可读能力隐藏。
    fn stop_outputs(&self, ctx: &HookContext) -> StopOutputs {
        let model = super::transcript::read_model(ctx);
        StopOutputs {
            last_ai: ctx.last_assistant_message.map(str::to_string),
            model,
            effort: None,
        }
    }

    fn transcript(&self) -> Option<&'static dyn crate::transcript::TranscriptSpec> {
        Some(&super::transcript::TRANSCRIPT)
    }

    fn resolves_transcript_title(&self) -> bool {
        true
    }

    fn transcript_title_generated(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_gemini_structured_history_is_available() {
        assert!(TELEMETRY
            .transcript()
            .is_some_and(|spec| spec.supports_chat()));
    }

    #[test]
    fn stop_outputs_take_ai_text_from_hook_payload() {
        let ctx = HookContext {
            session_id: "s",
            transcript_path: None,
            last_assistant_message: Some("好了"),
        };
        let out = TELEMETRY.stop_outputs(&ctx);
        assert_eq!(out.last_ai.as_deref(), Some("好了"));
        assert_eq!(out.model, None, "gemini 的 hook 不带模型名");

        // 负载没带正文（如工具回合）→ 不落库，而不是落一个空串。
        assert_eq!(
            TELEMETRY.stop_outputs(&HookContext::default()).last_ai,
            None
        );
    }

    /// 读取能力不能暗中开放未经验证的原生日志写回。
    #[test]
    fn transcript_support_does_not_enable_unverified_rename_writes() {
        assert!(TELEMETRY.transcript().is_some());
        assert!(TELEMETRY.resolves_transcript_title());
        assert!(!TELEMETRY.write_rename("s", None, "t"));
    }
}
