//! gemini 的遥测：只有一样——回合结束时的 AI 正文，且它就躺在 hook 负载里。
//!
//! Gemini 的 `AfterAgent`（＝规范的 `Stop`）负载带 `prompt_response` 字段，reporter 已把它 alias 到
//! `last_assistant_message`，故这里直接取用，不必去读会话文件。
//!
//! 其余能力一概降级：
//!
//! - **模型**：claude 的模型名走 statusLine，codex/kimi 从会话日志读；gemini 的 hook 负载不带，
//!   而为了一个模型名去解析它的会话文件不划算——卡片上不显示模型即可。
//! - **上下文占用**：同上，负载里没有。
//! - **transcript**：负载带 `transcript_path`，但标题走首条 prompt 已经够用（与 codex/kimi 一致），
//!   不为此引入一套解析器。
//! - **重命名回写**：Gemini 无对应写入面。

use crate::caps::{HookContext, StopOutputs, TelemetryCap};

pub static TELEMETRY: GeminiTelemetry = GeminiTelemetry;

pub struct GeminiTelemetry;

impl TelemetryCap for GeminiTelemetry {
    /// 正文取 hook 携带的那条；模型无来源，留空。
    fn stop_outputs(&self, ctx: &HookContext) -> StopOutputs {
        StopOutputs {
            last_ai: ctx.last_assistant_message.map(str::to_string),
            model: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// 标题走首条 prompt，不解析 transcript——与 codex/kimi 同，registry 的能力槽测试也依赖这点。
    #[test]
    fn does_not_resolve_titles_from_transcript() {
        assert!(TELEMETRY.transcript().is_none());
        assert!(!TELEMETRY.resolves_transcript_title());
        assert!(!TELEMETRY.write_rename("s", None, "t"));
    }
}
