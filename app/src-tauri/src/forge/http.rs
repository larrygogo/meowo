//! 各代码托管平台共用的 HTTP：与 github.rs 同一套做法，泛化到多主机。
//!
//! - 只发往一台主机：`https://<主机>/…`（主机来自公共平台或用户添加账号时声明的），别的地址、`http://`
//!   一律拒，令牌不会被带去别处。
//! - 自己跟重定向：只跟同一台主机的 https 跳转（301 / 302 把非 GET 改成不带正文的 GET，307 / 308 保留方法
//!   与正文），最多 5 跳；带着原认证头重发。
//! - 传输层失败（还没收到任何回包）有限重试 3 次（0.5s / 1s / 2s），只对能安全重发的请求：GET / HEAD、
//!   GraphQL 查询、OAuth 设备码申请与令牌轮询。写操作只发一次，可能已经生效、只是回包丢了。
//! - 走设置里的代理，与 GitHub 同一个 ureq Agent 缓存。
//! - 证书不受信（自建实例用了私有 CA 或自签证书）：报 `forge/tls: …`，界面说明原因；本轮不支持导入证书。

use crate::github::{Body, Response, Token};
use std::time::Duration;

/// 认证方式。令牌不落日志：Debug 打码。
#[derive(Clone)]
pub(crate) enum Auth {
    /// `Authorization: Bearer <令牌>`（GitHub、GitLab 个人访问令牌与 OAuth 令牌、Bitbucket 访问令牌）。
    Bearer(Token),
    /// `Authorization: token <令牌>`（Gitea / Forgejo 文档写法）。
    Token(Token),
    /// `Authorization: Basic base64(user:令牌)`（Bitbucket Cloud 的 API token：用户是 Atlassian 账号邮箱）。
    Basic { user: String, token: Token },
}

impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Auth(<redacted>)")
    }
}

impl Auth {
    pub(crate) fn header(&self) -> String {
        use base64::Engine as _;
        match self {
            Auth::Bearer(token) => format!("Bearer {}", token.expose()),
            Auth::Token(token) => format!("token {}", token.expose()),
            Auth::Basic { user, token } => {
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!("{user}:{}", token.expose()))
                )
            }
        }
    }
}

pub(crate) struct Req<'a> {
    pub(crate) method: &'a str,
    pub(crate) url: &'a str,
    pub(crate) auth: Option<&'a Auth>,
    pub(crate) body: Option<Body>,
    /// Accept 头；None = `application/json`。
    pub(crate) accept: Option<&'a str>,
}

pub(crate) trait ForgeHttp: Sync {
    /// Err = 没连上（断网、超时、代理不通、证书不受信）。4xx / 5xx 也回 Ok，由调用方判读。
    fn send(&self, req: &Req) -> Result<Response, String>;
}

/// 地址的「源」：https + 主机（含端口）。不是 https、解析不了时 None。
fn origin(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    Some(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// 只许发往这台主机（含端口）。
pub(crate) fn allowed(host: &str, url: &str) -> bool {
    // url 库会去掉默认端口 443，账号主机按同一规则规整后再比（host:443 与 host 是同一台）。
    origin(url).is_some_and(|o| o == super::platform::host_key(host))
}

/// 重定向去哪：同一台主机的 https 才跟，别的原样回给调用方。
pub(crate) fn redirect_target(host: &str, from: &str, location: Option<&str>) -> Option<String> {
    let base = url::Url::parse(from).ok()?;
    let next = base.join(location?).ok()?;
    (allowed(host, from) && allowed(host, next.as_str())).then(|| next.to_string())
}

/// 自己跟重定向（同 github.rs 的 follow_redirects，主机换成参数）。
pub(crate) fn follow_redirects(host: &str, raw: &dyn Fn(&Req) -> Result<Response, String>, req: &Req) -> Result<Response, String> {
    let mut method = req.method.to_string();
    let mut url = req.url.to_string();
    let mut body = req.body.clone();
    for _ in 0..=5 {
        let resp = raw(&Req {
            method: &method,
            url: &url,
            auth: req.auth,
            body: body.clone(),
            accept: req.accept,
        })?;
        if !matches!(resp.status, 301 | 302 | 307 | 308) {
            return Ok(resp);
        }
        let Some(next) = redirect_target(host, &url, resp.location.as_deref()) else {
            return Ok(resp);
        };
        if matches!(resp.status, 301 | 302) && method != "GET" && method != "HEAD" {
            method = "GET".into();
            body = None;
        }
        url = next;
    }
    Err("forge/too_many_redirects".into())
}

/// 能不能安全重发：读、GraphQL 查询、OAuth 设备码申请与令牌轮询 / 刷新。
pub(crate) fn retry_safe(req: &Req) -> bool {
    if matches!(req.method, "GET" | "HEAD") {
        return true;
    }
    let path = url::Url::parse(req.url).map(|u| u.path().to_string()).unwrap_or_default();
    if path == "/oauth/authorize_device" || path == "/oauth/token" {
        return true;
    }
    if path.ends_with("/graphql") {
        if let Some(Body::Json(body)) = &req.body {
            let query = body.get("query").and_then(|q| q.as_str()).unwrap_or_default().trim_start();
            return !query.starts_with("mutation");
        }
    }
    false
}

/// 传输层失败时有限重试（同 github.rs 的 with_retry）。
pub(crate) fn with_retry(raw: &dyn Fn(&Req) -> Result<Response, String>, req: &Req, sleep: &dyn Fn(Duration)) -> Result<Response, String> {
    let first = match raw(req) {
        Ok(resp) => return Ok(resp),
        Err(e) if !retry_safe(req) || e.starts_with("forge/tls") => return Err(e),
        Err(e) => e,
    };
    let mut last = first;
    for delay in crate::github::RETRY_DELAYS {
        sleep(delay);
        match raw(req) {
            Ok(resp) => return Ok(resp),
            Err(e) => last = e,
        }
    }
    Err(format!("{last} (retried {} times)", crate::github::RETRY_DELAYS.len()))
}

/// 传输层错误归类：证书问题单列（重试没用，要说清楚是证书不受信）。
pub(crate) fn transport_error(text: &str) -> String {
    let low = text.to_lowercase();
    let tls = [
        "invalid peer certificate",
        "unknownissuer",
        "unknown issuer",
        "certificate",
        "badcertificate",
        "notvalidforname",
        "self signed",
        "self-signed",
    ];
    if tls.iter().any(|n| low.contains(n)) && !low.contains("unexpected end of file") {
        format!("forge/tls: {text}")
    } else {
        text.to_string()
    }
}

/// 真实 HTTP，只发往 host（主机，可带端口）。
pub(crate) struct RealForgeHttp {
    pub(crate) host: String,
}

impl ForgeHttp for RealForgeHttp {
    fn send(&self, req: &Req) -> Result<Response, String> {
        let host = self.host.clone();
        follow_redirects(
            &self.host,
            &|r: &Req| {
                if !allowed(&host, r.url) {
                    return Err("forge/host_not_allowed".into());
                }
                with_retry(&raw_send, r, &std::thread::sleep)
            },
            req,
        )
    }
}

/// 一次请求，不跟重定向。
fn raw_send(req: &Req) -> Result<Response, String> {
    let proxy = crate::ports::resolve_proxy(None);
    let mut call = crate::github::agent(proxy.as_deref())
        .request(req.method, req.url)
        .timeout(crate::github::HTTP_TIMEOUT)
        .set("User-Agent", "Meowo")
        .set("Accept", req.accept.unwrap_or("application/json"));
    if let Some(auth) = req.auth {
        call = call.set("Authorization", &auth.header());
    }
    let result = match &req.body {
        None => call.call(),
        Some(Body::Json(value)) => call.send_json(value.clone()),
        Some(Body::Form(pairs)) => {
            let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            call.send_form(&pairs)
        }
    };
    let resp = match result {
        Ok(resp) | Err(ureq::Error::Status(_, resp)) => resp,
        // 传输层错误的文案里只有主机名和原因，没有请求头（令牌不会漏进来）。
        Err(e) => return Err(transport_error(&e.to_string())),
    };
    let status = resp.status();
    let link = resp.header("Link").map(str::to_string);
    let location = resp.header("Location").map(str::to_string);
    let body = resp.into_string().map_err(|e| e.to_string())?;
    Ok(Response {
        status,
        body,
        link,
        location,
    })
}

/// 读 JSON（解析失败当 Null）。
pub(crate) fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or(serde_json::Value::Null)
}

/// URL 路径段 / 查询值的转义（GitLab 的项目路径 `group%2Fsub%2Frepo` 也用它）。
pub(crate) fn enc(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ═══ 各平台共用的请求封装与错误归类 ═══

/// 一个平台身份的请求入口：带认证、回报令牌好坏（401 → 标「要重新登录」，2xx → 清掉）。
pub(crate) struct Client<'a> {
    pub(crate) http: Box<dyn ForgeHttp + 'a>,
    pub(crate) auth: Option<Auth>,
    pub(crate) on_auth: Box<dyn Fn(bool) + Sync + 'a>,
}

/// 测试里常把假 HTTP 借给多个平台实例。
impl<T: ForgeHttp + ?Sized> ForgeHttp for &T {
    fn send(&self, req: &Req) -> Result<Response, String> {
        (**self).send(req)
    }
}

impl Client<'_> {
    pub(crate) fn call(&self, method: &str, url: &str, body: Option<serde_json::Value>) -> Result<Response, String> {
        let req = Req {
            method,
            url,
            auth: self.auth.as_ref(),
            body: body.map(Body::Json),
            accept: None,
        };
        let resp = self.http.send(&req).map_err(|e| network_error(&e))?;
        if self.auth.is_some() {
            if resp.status == 401 {
                (self.on_auth)(false);
            } else if (200..300).contains(&resp.status) {
                (self.on_auth)(true);
            }
        }
        Ok(resp)
    }

    /// GET 并要求 2xx，回 JSON。
    pub(crate) fn get(&self, url: &str) -> Result<serde_json::Value, String> {
        let resp = self.call("GET", url, None)?;
        if !ok(&resp) {
            return Err(http_error(&resp));
        }
        Ok(json(&resp.body))
    }

    /// GET，404 回 None（「没有这个 PR」），别的非 2xx 报错。
    pub(crate) fn get_opt(&self, url: &str) -> Result<Option<serde_json::Value>, String> {
        let resp = self.call("GET", url, None)?;
        if resp.status == 404 {
            return Ok(None);
        }
        if !ok(&resp) {
            return Err(http_error(&resp));
        }
        Ok(Some(json(&resp.body)))
    }
}

pub(crate) fn ok(resp: &Response) -> bool {
    (200..300).contains(&resp.status)
}

/// 没连上：证书问题原样（`forge/tls: …`），别的归为 `forge/network: …`。
pub(crate) fn network_error(e: &str) -> String {
    if e.starts_with("forge/") {
        e.to_string()
    } else {
        format!("forge/network: {e}")
    }
}

/// 回包里的说明：`message`（字符串、字符串数组或对象）、`error`（字符串）或 `error.message`（Bitbucket），
/// 都没有就取正文第一行（截 200 字）。
pub(crate) fn message_of(resp: &Response) -> String {
    let v = json(&resp.body);
    let text = |m: &serde_json::Value| -> Option<String> {
        match m {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Array(items) => Some(
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_string).or_else(|| (!i.is_null()).then(|| i.to_string())))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            serde_json::Value::Object(map) => Some(
                map.iter()
                    .map(|(k, v)| format!("{k}: {}", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
            _ => None,
        }
    };
    let found = v
        .get("message")
        .and_then(text)
        .or_else(|| v.pointer("/error/message").and_then(text))
        .or_else(|| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|m| !m.trim().is_empty());
    found.unwrap_or_else(|| resp.body.lines().next().unwrap_or_default().chars().take(200).collect())
}

/// 非 2xx → forge 的错误码：401 没登录或令牌失效；403 权限不够；404 找不到（仓库没权限看时多数平台也回
/// 404）；429 限流；别的带状态码与原话。
pub(crate) fn http_error(resp: &Response) -> String {
    let message = message_of(resp);
    match resp.status {
        401 => "gh/not_authed".into(),
        403 => format!("forge/forbidden: {message}"),
        404 => format!("forge/not_found: {message}"),
        429 => format!("forge/rate_limited: {message}"),
        status => format!("gh/failed: HTTP {status}: {message}"),
    }
}

/// 分页：`Link: <…>; rel="next"`（GitLab、Gitea）。
pub(crate) fn next_link(resp: &Response) -> Option<String> {
    resp.link.as_deref()?.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"")
            .then(|| url.trim().trim_start_matches('<').trim_end_matches('>').to_string())
    })
}

/// 按 Link 头翻页取全（数组拼起来），最多 pages 页；下一页必须还在同一台主机。
pub(crate) fn get_all(client: &Client, host: &str, url: &str, pages: usize) -> Result<Vec<serde_json::Value>, String> {
    let mut out = Vec::new();
    let mut next = Some(url.to_string());
    let mut count = 0;
    while let Some(current) = next.take() {
        let resp = client.call("GET", &current, None)?;
        if !ok(&resp) {
            return Err(http_error(&resp));
        }
        out.extend(json(&resp.body).as_array().cloned().unwrap_or_default());
        count += 1;
        next = next_link(&resp).filter(|n| allowed(host, n) && count < pages);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn 只许发往声明的主机_https_端口要一致() {
        assert!(allowed("gitlab.com", "https://gitlab.com/api/v4/user"));
        assert!(allowed("git.example.com:8443", "https://git.example.com:8443/api/v1/user"));
        assert!(!allowed("git.example.com:8443", "https://git.example.com/api/v1/user"));
        // 账号主机写了默认端口：url 库去掉 :443 后仍算同一台（审查：以前全部报 host_not_allowed）。
        assert!(allowed("git.example.com:443", "https://git.example.com/api/v1/user"));
        assert!(allowed("git.example.com:443", "https://git.example.com:443/api/v1/user"));
        assert!(!allowed("gitlab.com", "http://gitlab.com/api/v4/user"), "不许降级成 http");
        assert!(!allowed("gitlab.com", "https://gitlab.com.evil.net/api"));
        assert!(!allowed("gitlab.com", "https://evil.net/?https://gitlab.com/"));
    }

    #[test]
    fn 重定向只跟同主机_https_非_get_的_302_改成_get() {
        let calls = Mutex::new(Vec::<(String, String, bool)>::new());
        let raw = |r: &Req| {
            calls.lock().unwrap().push((r.method.to_string(), r.url.to_string(), r.auth.is_some()));
            Ok(match r.url {
                "https://gitlab.com/api/v4/projects/old%2Fname" => Response {
                    status: 302,
                    location: Some("/api/v4/projects/new%2Fname".into()),
                    ..Default::default()
                },
                "https://gitlab.com/api/v4/projects/new%2Fname" => Response {
                    status: 200,
                    body: "{}".into(),
                    ..Default::default()
                },
                "https://gitlab.com/api/v4/away" => Response {
                    status: 301,
                    location: Some("https://evil.net/steal".into()),
                    ..Default::default()
                },
                _ => Response {
                    status: 404,
                    ..Default::default()
                },
            })
        };
        let auth = Auth::Bearer(Token::new("glpat-abc").unwrap());
        let req = Req {
            method: "POST",
            url: "https://gitlab.com/api/v4/projects/old%2Fname",
            auth: Some(&auth),
            body: Some(Body::Json(serde_json::json!({}))),
            accept: None,
        };
        let resp = follow_redirects("gitlab.com", &raw, &req).unwrap();
        assert_eq!(resp.status, 200);
        let seen = calls.lock().unwrap().clone();
        assert_eq!(seen[1], ("GET".into(), "https://gitlab.com/api/v4/projects/new%2Fname".into(), true));
        // 跳去别的主机：不跟，原样回。
        let req = Req {
            method: "GET",
            url: "https://gitlab.com/api/v4/away",
            auth: Some(&auth),
            body: None,
            accept: None,
        };
        assert_eq!(follow_redirects("gitlab.com", &raw, &req).unwrap().status, 301);
        assert!(!calls.lock().unwrap().iter().any(|(_, url, _)| url.contains("evil")));
    }

    #[test]
    fn 传输失败只重发能安全重发的_证书问题不重试() {
        let count = Mutex::new(0);
        let slept = Mutex::new(Vec::new());
        let raw = |_: &Req| {
            *count.lock().unwrap() += 1;
            Err::<Response, String>("Connection Failed: tls connection init failed: unexpected end of file".into())
        };
        let get = Req {
            method: "GET",
            url: "https://codeberg.org/api/v1/user",
            auth: None,
            body: None,
            accept: None,
        };
        let err = with_retry(&raw, &get, &|d| slept.lock().unwrap().push(d)).unwrap_err();
        assert!(err.ends_with("(retried 3 times)"), "{err}");
        assert_eq!(*count.lock().unwrap(), 4);
        *count.lock().unwrap() = 0;
        let post = Req {
            method: "POST",
            url: "https://codeberg.org/api/v1/repos/o/r/pulls",
            auth: None,
            body: None,
            accept: None,
        };
        with_retry(&raw, &post, &|_| {}).unwrap_err();
        assert_eq!(*count.lock().unwrap(), 1, "写操作只发一次");
        let tls = |_: &Req| Err::<Response, String>(transport_error("invalid peer certificate: UnknownIssuer"));
        let err = with_retry(&tls, &get, &|_| panic!("证书问题不重试")).unwrap_err();
        assert!(err.starts_with("forge/tls: "), "{err}");
        // GraphQL 查询可重发，mutation 不行；OAuth 轮询可重发。
        let q = Req {
            method: "POST",
            url: "https://gitlab.com/api/graphql",
            auth: None,
            body: Some(Body::Json(serde_json::json!({"query": "query { x }"}))),
            accept: None,
        };
        let m = Req {
            method: "POST",
            url: "https://gitlab.com/api/graphql",
            auth: None,
            body: Some(Body::Json(serde_json::json!({"query": "mutation { x }"}))),
            accept: None,
        };
        let o = Req {
            method: "POST",
            url: "https://gitlab.com/oauth/token",
            auth: None,
            body: None,
            accept: None,
        };
        assert!(retry_safe(&q) && !retry_safe(&m) && retry_safe(&o));
    }

    #[test]
    fn 错误归类_各平台回包的说明都取得出() {
        let r = |status: u16, body: &str| Response {
            status,
            body: body.into(),
            ..Default::default()
        };
        // GitLab 文档与实测：401 回 {"message":"401 Unauthorized"}；409 的 message 是数组。
        assert_eq!(http_error(&r(401, r#"{"message":"401 Unauthorized"}"#)), "gh/not_authed");
        assert_eq!(
            message_of(&r(
                409,
                r#"{"message":["Another open merge request already exists for this source branch: !7"]}"#
            )),
            "Another open merge request already exists for this source branch: !7"
        );
        // Gitea / Forgejo：{"message":"…","url":"…"}（Codeberg 实测）。
        assert_eq!(
            http_error(&r(
                403,
                r#"{"message":"token does not have at least one of required scope(s)","url":"https://codeberg.org/api/swagger"}"#
            )),
            "forge/forbidden: token does not have at least one of required scope(s)"
        );
        // Bitbucket：{"type":"error","error":{"message":"…"}}（官方 OpenAPI 的 error 定义）。
        assert_eq!(
            http_error(&r(404, r#"{"type":"error","error":{"message":"Repository not found"}}"#)),
            "forge/not_found: Repository not found"
        );
        assert_eq!(http_error(&r(429, "")), "forge/rate_limited: ");
        assert_eq!(
            http_error(&r(502, "<html>Bad gateway</html>")),
            "gh/failed: HTTP 502: <html>Bad gateway</html>"
        );
        assert_eq!(
            network_error("forge/tls: invalid peer certificate"),
            "forge/tls: invalid peer certificate"
        );
        assert_eq!(network_error("dns error"), "forge/network: dns error");
    }

    #[test]
    fn 认证头_三种写法_debug_打码() {
        let token = Token::new("abc").unwrap();
        assert_eq!(Auth::Bearer(token.clone()).header(), "Bearer abc");
        assert_eq!(Auth::Token(token.clone()).header(), "token abc");
        assert_eq!(
            Auth::Basic {
                user: "me@example.com".into(),
                token: token.clone()
            }
            .header(),
            "Basic bWVAZXhhbXBsZS5jb206YWJj"
        );
        assert_eq!(format!("{:?}", Auth::Bearer(token)), "Auth(<redacted>)");
    }
}
