//! 端口：插件层需要、但自己不该实现的外部能力。由宿主（meowo-app）注入实现。
//!
//! 这是「账号 / 用量也住进 `plugins/<id>/`」的前提。插件要联网拉用量、要读 macOS 登录 Keychain，
//! 若直接依赖 `ureq` 与 `std::process::Command`，插件层就得背上 HTTP 栈与平台 `cfg`，且单测非得
//! 有真网络才能跑。改由宿主注入后，插件保持纯逻辑，测试注入假实现即可。
//!
//! **只有真正需要隔离的才做成端口。** 文件读写是纯 `std`、测试拿临时目录就能覆盖，不在此列——
//! 插件层本来就在读 transcript、探测可执行。

use std::time::Duration;

/// 请求体。
pub enum Body<'a> {
    /// 无请求体（GET）。
    Empty,
    /// `application/json`。
    Json(serde_json::Value),
    /// `application/x-www-form-urlencoded`（OAuth token 端点）。
    Form(&'a [(&'a str, &'a str)]),
}

pub struct HttpRequest<'a> {
    /// `"GET"` / `"POST"`。
    pub method: &'static str,
    pub url: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub body: Body<'a>,
    pub timeout: Duration,
}

#[derive(Debug)]
pub enum HttpError {
    /// 非 2xx，携带状态码与响应体。OAuth 错误体形如 `{"error":"invalid_grant"}`——只含错误码
    /// 不含 token，调用方可安全打印（截断防超长）。
    Status(u16, String),
    /// 网络 / 超时 / 读取失败。
    Transport(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Status(code, body) => {
                let snippet: String = body.chars().take(200).collect();
                write!(f, "HTTP {code}：{snippet}")
            }
            HttpError::Transport(e) => write!(f, "{e}"),
        }
    }
}

/// HTTP 客户端。返回响应体文本，由调用方自行解析（各家 API 的 schema 差异归各家插件）。
pub trait HttpPort: Sync {
    fn send(&self, req: &HttpRequest) -> Result<String, HttpError>;

    /// 流式下载到文件，返回写入的字节数。
    ///
    /// 与 [`send`](HttpPort::send) 分开是因为体量：agent 的二进制是 240–260 MB，读成 `String`
    /// 既撑爆内存也没法算增量进度。实现须边读边写，不得先全量缓冲。
    ///
    /// `on_progress` 每收到一块调用一次，参数是（已写字节, 总字节）；总字节未知时为 `None`。
    fn download(
        &self,
        url: &str,
        dest: &std::path::Path,
        timeout: Duration,
        on_progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64, HttpError>;
}

/// 系统密钥链（macOS 登录 Keychain）。Claude Code 在 macOS 把 OAuth 凭据存在这里而非文件。
///
/// 非 macOS 由宿主注入一个 [`available`](KeychainPort::available) 恒为 `false` 的实现，于是插件里
/// 「Keychain 还是文件」这个分支是一次运行时判断，而不是散落各处的 `#[cfg(target_os = "macos")]`。
pub trait KeychainPort: Sync {
    /// 本平台是否有可用的密钥链。false → 调用方退回文件存储。
    fn available(&self) -> bool {
        false
    }
    /// 读该 service 条目的密码。
    fn read_password(&self, _service: &str) -> Option<String> {
        None
    }
    /// 读该 service 条目的 account 名（写回时按同名更新；读不到则调用方用声明的兜底值）。
    fn read_account(&self, _service: &str) -> Option<String> {
        None
    }
    /// 写回（条目存在则更新）。
    fn write_password(&self, _service: &str, _account: &str, _password: &str) -> Result<(), String> {
        Err("本平台无可用密钥链".into())
    }
}

/// 注入给能力方法的一组端口。
pub struct Ports<'a> {
    pub http: &'a dyn HttpPort,
    pub keychain: &'a dyn KeychainPort,
}

/// 无密钥链的平台（Windows / Linux），也可供单测使用。
pub struct NoKeychain;
impl KeychainPort for NoKeychain {}

#[cfg(test)]
pub(crate) mod test_doubles {
    use super::*;

    /// 拒绝一切请求的假 HTTP——用来断言「不该联网的路径确实没联网」。
    pub struct NoHttp;
    impl HttpPort for NoHttp {
        fn send(&self, _req: &HttpRequest) -> Result<String, HttpError> {
            panic!("该路径不应发起网络请求");
        }
        fn download(
            &self,
            _url: &str,
            _dest: &std::path::Path,
            _timeout: Duration,
            _on_progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<u64, HttpError> {
            panic!("该路径不应发起网络下载");
        }
    }
}
