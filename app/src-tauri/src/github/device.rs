//! GitHub OAuth 设备码流程（RFC 8628，GitHub 文档「Authorizing OAuth apps › Device flow」）。
//!
//! 1. POST github.com/login/device/code（client_id + scope）→ device_code、user_code、verification_uri、
//!    expires_in、interval；
//! 2. 用户在浏览器打开 verification_uri 输入 user_code；
//! 3. 每 interval 秒 POST github.com/login/oauth/access_token，直到拿到令牌或明确失败：
//!    `authorization_pending` 接着等；`slow_down` 按回包里的新间隔（没有就 +5 秒）；`expired_token`
//!    过期；`access_denied` 用户点了拒绝。
//!
//! HTTP 与睡眠都可注入，单测不打真网络、不真等。

use super::{Body, Http, Request, Token, DEVICE_CODE_URL, SCOPE, TOKEN_URL};
use std::time::Duration;

/// 设备码。`device_code` 是轮询凭据：只在后端，Debug 打码。
#[derive(Clone)]
pub(crate) struct DeviceCode {
    pub(crate) device_code: String,
    pub(crate) user_code: String,
    pub(crate) verification_uri: String,
    pub(crate) expires_in: u64,
    pub(crate) interval: u64,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

/// GitHub 文档规定的最短轮询间隔。
const MIN_INTERVAL: u64 = 5;
/// 轮询时连着几次连不上就放弃（偶发断网不至于让整次登录作废）。
const MAX_TRANSPORT_FAILURES: u32 = 3;
/// 等待时每隔多久看一眼「取消了没」：取消后最多这么久轮询线程就退出。
const CANCEL_CHECK: Duration = Duration::from_millis(250);

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

fn error_of(v: &serde_json::Value) -> Option<&str> {
    v.get("error").and_then(|e| e.as_str())
}

/// 第 1 步：要一次性代码。
pub(crate) fn request_code(http: &dyn Http, client_id: &str) -> Result<DeviceCode, String> {
    let req = Request {
        method: "POST",
        url: DEVICE_CODE_URL,
        token: None,
        body: Some(Body::Form(vec![("client_id", client_id.to_string()), ("scope", SCOPE.to_string())])),
    };
    let resp = http.send(&req).map_err(|e| format!("github/network: {e}"))?;
    let v = json(&resp.body);
    if let Some(error) = error_of(&v) {
        return Err(format!("github/login_failed: {error}"));
    }
    if resp.status != 200 {
        return Err(format!("github/login_failed: HTTP {}", resp.status));
    }
    let s = |key: &str| v.get(key).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
    let n = |key: &str| v.get(key).and_then(|x| x.as_u64());
    let missing = || "github/login_failed: bad response".to_string();
    Ok(DeviceCode {
        device_code: s("device_code").ok_or_else(missing)?,
        user_code: s("user_code").ok_or_else(missing)?,
        verification_uri: s("verification_uri").unwrap_or_else(|| "https://github.com/login/device".into()),
        expires_in: n("expires_in").unwrap_or(900),
        interval: n("interval").unwrap_or(MIN_INTERVAL).max(MIN_INTERVAL),
    })
}

/// 一次轮询的结果。
#[derive(Debug)]
pub(crate) enum Poll {
    Pending,
    /// 轮询太快；带 GitHub 给的新间隔（秒）。
    SlowDown(Option<u64>),
    Expired,
    Denied,
    Token(Token),
    /// 没连上（可重试）。
    Transport(String),
    /// 明确失败（Client ID 不对、设备码流程没开等），不再重试。
    Failed(String),
}

pub(crate) fn poll_once(http: &dyn Http, client_id: &str, device_code: &str) -> Poll {
    let req = Request {
        method: "POST",
        url: TOKEN_URL,
        token: None,
        body: Some(Body::Form(vec![
            ("client_id", client_id.to_string()),
            ("device_code", device_code.to_string()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code".to_string()),
        ])),
    };
    let resp = match http.send(&req) {
        Ok(resp) => resp,
        Err(e) => return Poll::Transport(e),
    };
    let v = json(&resp.body);
    match error_of(&v) {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown(v.get("interval").and_then(|x| x.as_u64())),
        Some("expired_token") => Poll::Expired,
        Some("access_denied") => Poll::Denied,
        Some(other) => Poll::Failed(format!("github/login_failed: {other}")),
        None => match v.get("access_token").and_then(|x| x.as_str()).and_then(Token::new) {
            Some(token) => Poll::Token(token),
            // 5xx 之类：当作暂时连不上，下一轮再试。
            None if resp.status >= 500 => Poll::Transport(format!("HTTP {}", resp.status)),
            None => Poll::Failed(format!("github/login_failed: HTTP {}", resp.status)),
        },
    }
}

/// 一次登录的最终结果。
#[derive(Debug)]
pub(crate) enum Outcome {
    Token(Token),
    Expired,
    Denied,
    Cancelled,
    Failed(String),
}

/// 睡 total，期间每 [`CANCEL_CHECK`] 看一眼取消。返回 true = 被取消。
fn nap(total: Duration, cancelled: &dyn Fn() -> bool, sleep: &dyn Fn(Duration)) -> bool {
    let mut left = total;
    while !left.is_zero() {
        if cancelled() {
            return true;
        }
        let step = left.min(CANCEL_CHECK);
        sleep(step);
        left -= step;
    }
    cancelled()
}

/// 第 3 步：按间隔轮询直到有结果。过期按 expires_in 自己算（等的时间累加），不只靠 GitHub 回 expired_token。
pub(crate) fn wait_for_token(http: &dyn Http, client_id: &str, code: &DeviceCode, cancelled: &dyn Fn() -> bool, sleep: &dyn Fn(Duration)) -> Outcome {
    let mut interval = code.interval.max(MIN_INTERVAL);
    let mut waited = 0u64;
    let mut transport_failures = 0u32;
    loop {
        if nap(Duration::from_secs(interval), cancelled, sleep) {
            return Outcome::Cancelled;
        }
        waited += interval;
        match poll_once(http, client_id, &code.device_code) {
            Poll::Token(token) => return Outcome::Token(token),
            Poll::Expired => return Outcome::Expired,
            Poll::Denied => return Outcome::Denied,
            Poll::Failed(e) => return Outcome::Failed(e),
            Poll::Pending => transport_failures = 0,
            Poll::SlowDown(next) => {
                transport_failures = 0;
                interval = next.unwrap_or(interval + 5).max(interval + 1);
            }
            Poll::Transport(e) => {
                transport_failures += 1;
                if transport_failures >= MAX_TRANSPORT_FAILURES {
                    return Outcome::Failed(format!("github/network: {e}"));
                }
            }
        }
        if waited >= code.expires_in {
            return Outcome::Expired;
        }
    }
}
