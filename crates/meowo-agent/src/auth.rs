//! 鉴权参数描述。**只描述、不执行**：HTTP 由 meowo-app 发（ureq 不进本 crate）。
//!
//! 变体层的意义在此最直接：同一 agent 的新旧版可能有不同的 OAuth `client_id` / 刷新端点 /
//! 凭据文件位置，此前它们是 account 模块里的三个常量，换个版本就整体失效。

/// 一个变体的 OAuth 刷新与凭据布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthScheme {
    /// 凭据 JSON 相对 `data_dir` 的路径（`/` 分隔，跨平台由 `Installation` 分段拼接）。
    pub credentials_rel: &'static str,
    /// OAuth token 端点（`grant_type=refresh_token` POST 到这里）。
    pub token_url: &'static str,
    /// OAuth client_id。刷新返回 `invalid_client` 即此值与该变体不符。
    pub client_id: &'static str,
    /// 用量等 API 的默认 base_url（config.toml 里的 base_url 优先，此为兜底）。
    pub default_base_url: &'static str,
}
