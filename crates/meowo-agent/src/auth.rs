//! 鉴权参数描述。**只描述、不执行**：HTTP 由 meowo-app 发（ureq 不进本 crate），
//! Keychain 读取也在 app 侧（要跑 `security` 命令）。
//!
//! 变体层的意义在此最直接：同一 agent 的新旧版可能有不同的 OAuth `client_id` / 刷新端点 /
//! 凭据位置，此前它们是 account 模块里的一把常量，换个版本就整体失效。

/// 凭据存放位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    /// 相对 `data_dir` 的 JSON 文件（`/` 分隔，由 `Installation` 分段拼接）。
    File(&'static str),
    /// macOS 登录 Keychain 的通用密码；其它平台回退到 `file`（Claude Code 就是这样）。
    KeychainOrFile { service: &'static str, account: &'static str, file: &'static str },
}

impl CredentialSource {
    /// 文件回退路径（相对 `data_dir`）。Keychain 命中时调用方不用它。
    pub fn file_rel(self) -> &'static str {
        match self {
            Self::File(rel) => rel,
            Self::KeychainOrFile { file, .. } => file,
        }
    }
}

/// OAuth 刷新参数。无刷新需求的 agent（如 codex 的 auth.json 由 CLI 自己维护）置 None。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OAuthRefresh {
    /// token 端点（`grant_type=refresh_token` POST 到这里）。
    pub token_url: &'static str,
    /// OAuth client_id。刷新返回 `invalid_client` 即此值与该变体不符。
    pub client_id: &'static str,
}

/// 一个变体的凭据布局与刷新方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthScheme {
    pub credentials: CredentialSource,
    pub refresh: Option<OAuthRefresh>,
    /// 用量等 API 的默认 base_url（配置文件里的 base_url 优先，此为兜底）。空串 = 无。
    pub default_base_url: &'static str,
}
