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
    /// 相对 **home** 的 JSON 文件——凭据不在 `data_dir` 底下。
    ///
    /// opencode 逼出了这一条：它把配置与数据分了家。插件必须落在**配置**目录
    /// （`~/.config/opencode`，也就是它的 `data_dir`），凭据却写在**数据**目录
    /// （`~/.local/share/opencode/auth.json`，实测 `opencode auth list` 的输出）。
    /// 另四家两者同在一处，从没暴露过「相对 data_dir」这个隐含假设。
    HomeFile(&'static str),
    /// macOS 登录 Keychain 的通用密码；其它平台回退到 `file`（Claude Code 就是这样）。
    KeychainOrFile { service: &'static str, account: &'static str, file: &'static str },
}

impl CredentialSource {
    /// 文件回退路径。**注意它是相对谁的**：`File` / `KeychainOrFile` 相对 `data_dir`，
    /// `HomeFile` 相对 home。解析一律走 [`crate::variant::Installation::credentials_path`]，
    /// 别在别处自行拼接——那正是会把 opencode 的凭据拼到错误目录下的地方。
    pub fn file_rel(self) -> &'static str {
        match self {
            Self::File(rel) | Self::HomeFile(rel) => rel,
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

/// 一个变体的凭据布局、刷新方式与登录入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthScheme {
    pub credentials: CredentialSource,
    pub refresh: Option<OAuthRefresh>,
    /// 用量等 API 的默认 base_url（配置文件里的 base_url 优先，此为兜底）。空串 = 无。
    pub default_base_url: &'static str,
    /// 拉起交互式登录的子命令，接在启动 argv 之后。各家并不同构（均为实测）：
    ///
    /// - `Some(&["auth", "login"])` —— claude（没有 `claude login`）、opencode。
    /// - `Some(&["login"])` —— codex / kimi。
    /// - `Some(&[])` —— **裸启动即登录**。gemini 压根没有登录子命令（0.50 的子命令只有
    ///   mcp / extensions / skills / hooks / gemma），首次运行 `gemini` 本身就会引导你选认证方式。
    ///   这个空切片与下面的 `None` 是两回事：它是「有入口，且入口就是启动它自己」。
    /// - `None` —— 无登录入口，[`crate::variant::Installation::login_argv`] 返回 None。
    ///
    /// 这里曾经是个裸切片，空切片即「无入口」——于是 gemini 这种「裸启动即登录」的 agent 无从表达，
    /// 只能被判成没有登录入口，前端却仍旧亮出登录按钮，点下去只得到一句「拉起登录失败」。
    pub login: Option<&'static [&'static str]>,
}
