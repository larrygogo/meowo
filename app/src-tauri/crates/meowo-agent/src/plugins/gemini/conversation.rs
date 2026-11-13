//! 本机隔离 initialize 已验证 ACP v1 与 loadSession；未验证版本不能静默降级到 PTY。
//! 取证：target/lead-provider-probes/acp-handshake，2026-11-12（无认证、无模型）。
use crate::conversation::{
    ConversationCap, ConversationCodec, ConversationInit, ConversationSpec, ConversationTransport,
};
use crate::variant::Installation;
pub(super) static CAP: GeminiConversation = GeminiConversation;
pub(super) struct GeminiConversation;
impl ConversationCap for GeminiConversation {
    fn spec(
        &self,
        installation: &Installation,
        version: Option<&str>,
    ) -> Result<ConversationSpec, String> {
        if installation.variant_tag != "stable" {
            return Err("conversation/variant_unsupported".into());
        }
        if !crate::lead_install::version_matches(version, (0, 65)) {
            return Err("conversation/version_unsupported".into());
        }
        Ok(ConversationSpec {
            args: ["--acp"].iter().map(|s| s.to_string()).collect(),
            environment: vec![("MEOWO_PROTOCOL_MANAGED".into(), "1".into())],
            transport: ConversationTransport::JsonLines,
            ready_timeout_ms: 30_000,
        })
    }
    fn codec(&self, init: ConversationInit) -> Box<dyn ConversationCodec> {
        Box::new(crate::conversation::acp::AcpCodec::new(
            init,
            "gemini-cli",
            (0, 65),
        ))
    }
}
