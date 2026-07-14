//! API 中转能力。插件显式声明才支持；宿主只负责密钥存储、HTTP 与进程启动。

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RelayOption {
    pub value: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RelaySuggestionGroup {
    /// 空串表示默认组；其它值对应协议 value。
    pub protocol: &'static str,
    pub models: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct RelayUi {
    pub protocols: &'static [RelayOption],
    pub auth_modes: &'static [RelayOption],
    pub default_protocol: &'static str,
    pub default_auth: &'static str,
    pub suggestions: &'static [RelaySuggestionGroup],
}

#[derive(Debug, Clone, Copy)]
pub struct RelayConfig<'a> {
    pub base_url: &'a str,
    pub model: &'a str,
    pub protocol: &'a str,
    pub auth: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayModelAuth {
    Bearer,
    ApiKey,
}

#[derive(Debug, Clone, Copy)]
pub struct RelayModelRequest {
    pub auth: RelayModelAuth,
    pub anthropic_version: bool,
}

pub trait RelayCap: Sync {
    fn ui(&self) -> RelayUi;

    /// 同一插件的旧变体可不支持中转（例如 legacy Kimi）。
    fn supports_variant(&self, _variant_tag: &str) -> bool {
        true
    }

    fn validate(&self, config: RelayConfig<'_>, variant_tag: &str) -> Result<(), String> {
        if !self.supports_variant(variant_tag) {
            return Err("当前安装版本不支持 API 中转".into());
        }
        let ui = self.ui();
        if !ui.protocols.is_empty()
            && !ui.protocols.iter().any(|option| option.value == config.protocol)
        {
            return Err("中转接口协议不受支持".into());
        }
        if !ui.auth_modes.iter().any(|option| option.value == config.auth) {
            return Err("中转凭据类型不受支持".into());
        }
        Ok(())
    }

    fn launch_env(&self, config: RelayConfig<'_>, key: &str) -> Vec<(String, String)>;
    fn augment_argv(
        &self,
        config: RelayConfig<'_>,
        has_secret: bool,
        argv: Vec<String>,
    ) -> Vec<String>;
    fn model_request(&self, config: RelayConfig<'_>) -> RelayModelRequest;
}
