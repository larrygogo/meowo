//! GitHub 连接：PR 功能用谁的身份连 GitHub（设置 › GitHub）。
//!
//! 四种方式（settings.github_connection）：
//! - `auto`（默认）：Meowo 登录过就用 Meowo 的令牌，否则用本机 gh——老用户装好的 gh 照旧能用；
//! - `meowo`：只用 Meowo 的令牌，不碰 gh；没登录时 PR 区块引导去设置里登录；
//! - `gh`：只用本机 gh（改版前的行为）；
//! - `off`：不发任何 GitHub 请求，PR 区块不出现。
//!
//! Meowo 登录走 GitHub OAuth App 的设备码流程（[`device`]），scope 与 gh 默认一致（`repo read:org`）。
//! 令牌只进系统凭据库（[`credential`]：Windows 凭据管理器、macOS 钥匙串），不进设置文件、日志、前端、
//! 远程 RPC；进程内只在 [`Token`] 里存一份（Debug 打码、不可序列化）。OAuth App 关了令牌过期，没有
//! refresh_token（续期要 client secret，桌面端不能内置），令牌被拒（401）就标「需要重新登录」。
//!
//! forge 的 gh 调用经 `forge::connected_gh` 按这里的 [`executor`] 分派：本机 gh、内置实现
//! （forge/builtin.rs，直接调 REST / GraphQL）、或关闭。

use meowo_protocol::ipc::{GithubAccountDto, GithubConnectionDto, GithubDeviceCodeDto};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::Emitter;

mod credential;
pub(crate) mod device;
#[cfg(test)]
mod tests;

pub(crate) use credential::{KeyVault, SystemVault, Vault};

/// OAuth App 的 Client ID（larrygogo 名下的「Meowo」，已开 Device Flow）。Client ID 本身可公开；
/// 设备码流程不需要 client secret，代码里也没有。构建时可用 MEOWO_GITHUB_CLIENT_ID 覆盖（自建分发用）。
pub(crate) const CLIENT_ID: &str = match option_env!("MEOWO_GITHUB_CLIENT_ID") {
    Some(id) if !id.is_empty() => id,
    _ => "Ov23lilH0boYrABgr4xl",
};
/// 与 gh 默认登录同一组权限：私有仓库的 PR、组织仓库。
pub(crate) const SCOPE: &str = "repo read:org";
pub(crate) const API: &str = "https://api.github.com";
pub(crate) const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub(crate) const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// 连接状态变了（切模式、登录、退出、令牌失效）：前端重拉状态、PR 区块与侧栏角标刷新。
pub(crate) const CHANGED_EVENT: &str = "github-connection-changed";
/// 一次设备码登录结束：`{ state: ok|expired|denied|error, error? }`。取消不发（前端自己收起）。
pub(crate) const LOGIN_EVENT: &str = "github-login-finished";
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

// ═══ 方式 ═══

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Auto,
    Meowo,
    Gh,
    Off,
}

impl Mode {
    pub(crate) fn parse(text: &str) -> Option<Mode> {
        match text {
            "auto" => Some(Mode::Auto),
            "meowo" => Some(Mode::Meowo),
            "gh" => Some(Mode::Gh),
            "off" => Some(Mode::Off),
            _ => None,
        }
    }
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Meowo => "meowo",
            Mode::Gh => "gh",
            Mode::Off => "off",
        }
    }
}

// ═══ 令牌 ═══

/// OAuth 令牌。刻意不实现 Serialize / Display，Debug 打码：误把它塞进日志、DTO、事件都编译不过或只见星号。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Token(String);

impl Token {
    /// 只收 GitHub 令牌的字符集（`gho_` + 字母数字下划线）：拼进请求头、交给 security 命令行都安全。
    pub(crate) fn new(raw: &str) -> Option<Token> {
        let raw = raw.trim();
        let valid = !raw.is_empty() && raw.len() <= 512 && raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');
        valid.then(|| Token(raw.to_string()))
    }
    /// 各平台的个人访问令牌（GitLab `glpat-…`、Gitea 十六进制、Atlassian API token 带 `=` 等）：可见 ASCII，
    /// 不含空白、引号、反斜杠与反引号（拼进请求头、交给 security 命令行都安全）。
    pub(crate) fn secret(raw: &str) -> Option<Token> {
        let raw = raw.trim();
        let valid = !raw.is_empty() && raw.len() <= 1024 && raw.bytes().all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\'' | b'\\' | b'`'));
        valid.then(|| Token(raw.to_string()))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

// ═══ HTTP（可注入；测试不打真网络）═══

#[derive(Clone)]
pub(crate) enum Body {
    Json(serde_json::Value),
    Form(Vec<(&'static str, String)>),
}

pub(crate) struct Request<'a> {
    pub(crate) method: &'a str,
    pub(crate) url: &'a str,
    pub(crate) token: Option<&'a Token>,
    pub(crate) body: Option<Body>,
}

/// 状态码 + 正文 + Link 头（REST 分页）+ Location 头（重定向）。4xx / 5xx 也走这里，由调用方判读。
#[derive(Debug, Clone, Default)]
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) body: String,
    pub(crate) link: Option<String>,
    pub(crate) location: Option<String>,
}

/// 最多跟几次重定向（改名 / 转移过的仓库通常一跳）。
const MAX_REDIRECTS: usize = 5;

/// 重定向去哪：只跟 https 的 api.github.com → api.github.com。别的主机、降级成 http、带端口的一律不跟
///（回包原样交给调用方），令牌不会被带去别处。
pub(crate) fn redirect_target(from: &str, location: Option<&str>) -> Option<String> {
    let base = url::Url::parse(from).ok()?;
    let next = base.join(location?).ok()?;
    let api = |u: &url::Url| u.scheme() == "https" && u.host_str() == Some("api.github.com") && u.port().is_none();
    (api(&base) && api(&next)).then(|| next.to_string())
}

/// 自己跟重定向：ureq 默认跟随时会去掉 Authorization，且 POST / PUT 的 307 不跟——改名或转移过的私有仓库
/// 读评审 404、建 PR / 合并 / 评论失败。gh（Go 的 net/http）同主机跳转保留登录头，这里对齐：
/// 301 / 302 把非 GET 改成不带正文的 GET（同 Go），307 / 308 保留方法与正文；带着原令牌重发。
pub(crate) fn follow_redirects(raw: &dyn Fn(&Request) -> Result<Response, String>, req: &Request) -> Result<Response, String> {
    let mut method = req.method.to_string();
    let mut url = req.url.to_string();
    let mut body = req.body.clone();
    for _ in 0..=MAX_REDIRECTS {
        let resp = raw(&Request { method: &method, url: &url, token: req.token, body: body.clone() })?;
        if !matches!(resp.status, 301 | 302 | 307 | 308) {
            return Ok(resp);
        }
        let Some(next) = redirect_target(&url, resp.location.as_deref()) else {
            return Ok(resp);
        };
        if matches!(resp.status, 301 | 302) && method != "GET" && method != "HEAD" {
            method = "GET".into();
            body = None;
        }
        url = next;
    }
    Err("github/too_many_redirects".into())
}

/// Meowo 专用的 ureq Agent：不让 ureq 自己跟重定向（见 [`follow_redirects`]），代理与 ports.rs 同一套解析。
pub(crate) fn agent(proxy: Option<&str>) -> ureq::Agent {
    static CACHE: Mutex<Option<std::collections::HashMap<String, ureq::Agent>>> = Mutex::new(None);
    let key = proxy.unwrap_or_default().to_string();
    let mut guard = lock(&CACHE);
    let cache = guard.get_or_insert_with(Default::default);
    if let Some(agent) = cache.get(&key) {
        return agent.clone();
    }
    let mut builder = ureq::AgentBuilder::new().redirects(0);
    if let Some(p) = proxy {
        match ureq::Proxy::new(crate::proxy::ureq_compatible_url(p).as_ref()) {
            Ok(px) => builder = builder.proxy(px),
            Err(e) => eprintln!("[github] 代理地址无效，本次直连（{}）：{e}", crate::proxy::redact_credentials(p)),
        }
    }
    let agent = builder.build();
    cache.insert(key, agent.clone());
    agent
}

pub(crate) trait Http: Sync {
    /// Err = 没连上（断网、超时、代理不通）。
    fn send(&self, req: &Request) -> Result<Response, String>;
}

/// 只许发往 GitHub：API 与两个设备码端点。令牌只在发往 api.github.com 时带上（见 [`RealHttp`]）。
pub(crate) fn allowed_url(url: &str) -> bool {
    url.starts_with("https://api.github.com/") || url == DEVICE_CODE_URL || url == TOKEN_URL
}

/// 真实 HTTP：走设置里的代理（与用量查询、Bark 推送同一个 ureq Agent 缓存）。
pub(crate) struct RealHttp;

impl Http for RealHttp {
    fn send(&self, req: &Request) -> Result<Response, String> {
        follow_redirects(
            &|r: &Request| {
                // 不许去的主机直接拒，不重试。
                if !allowed_url(r.url) {
                    return Err("github/host_not_allowed".into());
                }
                with_retry(&raw_send, r, &std::thread::sleep)
            },
            req,
        )
    }
}

/// 传输层失败（连不上、TLS 握手失败或 EOF、DNS 临时失败、超时，还没收到任何回包）后的重试间隔。
/// 经 TUN 代理上网时到 GitHub 的握手偶发失败，gh / git push 也一样，重试即成功（leader 安装版实测）。
pub(crate) const RETRY_DELAYS: [Duration; 3] = [Duration::from_millis(500), Duration::from_secs(1), Duration::from_secs(2)];

/// 能不能安全重发：读（GET / HEAD）、GraphQL 查询（不是 mutation）、设备码申请与令牌轮询。
/// 写操作（建 PR、合并、评论、评审、入队等 REST 写与 GraphQL mutation）不重发：可能已经生效、只是回包丢了，
/// 重发会重复建 PR、重复评论。
pub(crate) fn retry_safe(req: &Request) -> bool {
    if matches!(req.method, "GET" | "HEAD") || req.url == DEVICE_CODE_URL || req.url == TOKEN_URL {
        return true;
    }
    if req.url == format!("{API}/graphql") {
        if let Some(Body::Json(body)) = &req.body {
            let query = body.get("query").and_then(|q| q.as_str()).unwrap_or_default().trim_start();
            return !query.starts_with("mutation");
        }
    }
    false
}

/// 传输层失败时有限重试（只对 [`retry_safe`] 的请求）：最多再试 3 次，间隔 0.5s / 1s / 2s；收到任何回包
///（含 4xx / 5xx）都不重试。全部失败才报，带上最后一次的原因。
pub(crate) fn with_retry(raw: &dyn Fn(&Request) -> Result<Response, String>, req: &Request, sleep: &dyn Fn(Duration)) -> Result<Response, String> {
    let first = match raw(req) {
        Ok(resp) => return Ok(resp),
        Err(e) if !retry_safe(req) => return Err(e),
        Err(e) => e,
    };
    let mut last = first;
    for delay in RETRY_DELAYS {
        sleep(delay);
        match raw(req) {
            Ok(resp) => return Ok(resp),
            Err(e) => last = e,
        }
    }
    Err(format!("{last} (retried {} times)", RETRY_DELAYS.len()))
}

/// GraphQL 带上 gh 一直带的预览头：mergeStateStatus 早年是预览字段。2026-11-18 用 gh 2.96 实测 cli/cli，
/// 不带也能拿到（见 progress.md），这里与 gh 保持一致，防 GitHub 对老账号 / 企业设置收紧。
pub(crate) const GRAPHQL_ACCEPT: &str = "application/vnd.github.merge-info-preview+json, application/vnd.github+json";

/// 一次请求，不跟重定向。
fn raw_send(req: &Request) -> Result<Response, String> {
    {
        if !allowed_url(req.url) {
            return Err("github/host_not_allowed".into());
        }
        let proxy = crate::ports::resolve_proxy(None);
        let api = req.url.starts_with("https://api.github.com/");
        let accept = if req.url == format!("{API}/graphql") {
            GRAPHQL_ACCEPT
        } else if api {
            "application/vnd.github+json"
        } else {
            "application/json"
        };
        let mut call = agent(proxy.as_deref())
            .request(req.method, req.url)
            .timeout(HTTP_TIMEOUT)
            .set("User-Agent", "Meowo")
            .set("Accept", accept);
        if api {
            call = call.set("X-GitHub-Api-Version", "2022-11-28");
            if let Some(token) = req.token {
                call = call.set("Authorization", &format!("Bearer {}", token.expose()));
            }
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
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let link = resp.header("Link").map(str::to_string);
        let location = resp.header("Location").map(str::to_string);
        let body = resp.into_string().map_err(|e| e.to_string())?;
        Ok(Response { status, body, link, location })
    }
}

// ═══ 账号记录（不含令牌，可缓存给前端）═══

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountRecord {
    #[serde(default)]
    pub(crate) account: Option<GithubAccountDto>,
    #[serde(default)]
    pub(crate) needs_relogin: bool,
}

/// `~/.meowo/github.json`：只有 login / 名字 / 头像地址与「要重新登录」标记。
fn record_path() -> PathBuf {
    crate::db_path().with_file_name("github.json")
}

pub(crate) fn load_record(path: &Path) -> AccountRecord {
    std::fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}

pub(crate) fn save_record(path: &Path, record: &AccountRecord) -> Result<(), String> {
    if record == &AccountRecord::default() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(path, &body).map_err(|e| e.to_string())
}

/// `GET /user` → 展示字段。401 = 令牌不认。
pub(crate) fn fetch_account(http: &dyn Http, token: &Token) -> Result<GithubAccountDto, String> {
    let url = format!("{API}/user");
    let resp = http
        .send(&Request { method: "GET", url: &url, token: Some(token), body: None })
        .map_err(|e| format!("github/network: {e}"))?;
    match resp.status {
        200 => {
            let v: serde_json::Value = serde_json::from_str(&resp.body).map_err(|_| "github/failed: bad json".to_string())?;
            let s = |key: &str| v.get(key).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
            Ok(GithubAccountDto { login: s("login").ok_or("github/failed: no login")?, name: s("name"), avatar_url: s("avatar_url") })
        }
        401 => Err("github/unauthorized".into()),
        code => Err(format!("github/failed: HTTP {code}")),
    }
}


/// 落库：令牌存进凭据库，再写不含令牌的账号记录。存凭据失败就不写记录：界面不会显示「已登录」而令牌其实没存下。
pub(crate) fn store_login(vault: &dyn Vault, path: &Path, token: &Token, account: &GithubAccountDto) -> Result<(), String> {
    vault.write(token)?;
    save_record(path, &AccountRecord { account: Some(account.clone()), needs_relogin: false })
}

// ═══ 进程内状态 ═══

struct Session {
    loaded: bool,
    token: Option<Token>,
}

static SESSION: Mutex<Session> = Mutex::new(Session { loaded: false, token: None });
static MODE: Mutex<Option<Mode>> = Mutex::new(None);
/// 切方式串行：两次 github_set_mode 交错时，设置文件与内存里的方式不会一边一个。
static MODE_WRITE: Mutex<()> = Mutex::new(());
/// 记录里「要重新登录」的内存副本（None = 还没读过）：每次成功请求都要报一下，不能每次都读文件。
static RELOGIN: Mutex<Option<bool>> = Mutex::new(None);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 设备码登录的代次与进行中的代码。每次开始、取消、退出都把代次加一：旧一轮的轮询线程与收尾一看代次
/// 对不上就作废，晚到的令牌不落库。
pub(crate) struct LoginSlot {
    pub(crate) generation: u64,
    pub(crate) pending: Option<GithubDeviceCodeDto>,
}

impl LoginSlot {
    pub(crate) const fn new() -> Self {
        LoginSlot { generation: 0, pending: None }
    }
}

static LOGIN: Mutex<LoginSlot> = Mutex::new(LoginSlot::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 启动时记下的 AppHandle（代码托管账号的登录结果也经它发事件）。
pub(crate) fn app() -> Option<&'static tauri::AppHandle> {
    APP.get()
}

/// 启动时记下 AppHandle：令牌在后台线程里被拒时要通知前端。
pub(crate) fn init(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

pub(crate) fn emit_changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

pub(crate) fn mode() -> Mode {
    let mut guard = lock(&MODE);
    *guard.get_or_insert_with(|| Mode::parse(&crate::settings::load_settings().github_connection).unwrap_or(Mode::Auto))
}

/// 进程内的令牌：第一次用时从凭据库读一次，之后用内存里的（登录、退出时更新）。
pub(crate) fn token() -> Option<Token> {
    let mut session = lock(&SESSION);
    if !session.loaded {
        session.token = match SystemVault.read() {
            Ok(token) => token,
            Err(e) => {
                eprintln!("[github] 读凭据库失败：{e}");
                None
            }
        };
        session.loaded = true;
    }
    session.token.clone()
}

fn set_token(token: Option<Token>) {
    let mut session = lock(&SESSION);
    session.loaded = true;
    session.token = token;
    *lock(&RELOGIN) = None;
}

/// 这一次 gh 调用交给谁。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Executor {
    /// 关闭：不发任何请求（forge 看到的等同「没装 gh」）。
    Off,
    /// 本机 gh。
    Gh,
    /// 内置实现；None = Meowo 模式但还没登录（每条命令都回「没登录」）。
    Meowo(Option<Token>),
    /// 自动且已登录 Meowo：github.com 的仓库走内置实现，别的主机（企业版）继续走本机 gh。
    Auto(Token),
}

impl Executor {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Executor::Off => "off",
            Executor::Gh => "gh",
            Executor::Meowo(_) | Executor::Auto(_) => "meowo",
        }
    }
}

/// 方式 + 有没有令牌 → 执行器。自动：有令牌时 github.com 用 Meowo（令牌被拒也不偷偷退回 gh，给同一个
/// 「重新登录」提示），企业版仍用 gh。
pub(crate) fn resolve(mode: Mode, token: Option<Token>) -> Executor {
    match (mode, token) {
        (Mode::Off, _) => Executor::Off,
        (Mode::Gh, _) => Executor::Gh,
        (Mode::Meowo, token) => Executor::Meowo(token),
        (Mode::Auto, Some(token)) => Executor::Auto(token),
        (Mode::Auto, None) => Executor::Gh,
    }
}

pub(crate) fn executor() -> Executor {
    match mode() {
        // 关闭 / 只用 gh 时不碰凭据库（macOS 上读钥匙串要起进程）。
        Mode::Off => Executor::Off,
        Mode::Gh => Executor::Gh,
        mode => resolve(mode, token()),
    }
}

/// 内置实现每拿到一个回包报一次：ok=false 是 401（这个令牌被拒），ok=true 是 2xx（这个令牌好用）。
/// 只在用的正是当前令牌时改「要重新登录」：旧令牌晚到的 401 不能把新登录标成要重登；之后成功的请求把标记清掉。
/// 返回 true = 记录变了（要通知前端）。
pub(crate) fn auth_result_with(path: &Path, current: Option<&Token>, used: &Token, ok: bool, known: &Mutex<Option<bool>>) -> bool {
    if current != Some(used) {
        return false;
    }
    let want = !ok;
    let mut known = lock(known);
    if *known == Some(want) {
        return false;
    }
    let mut record = load_record(path);
    let changed = record.needs_relogin != want;
    if changed {
        record.needs_relogin = want;
        let _ = save_record(path, &record);
    }
    *known = Some(want);
    changed
}

pub(crate) fn auth_result(used: &Token, ok: bool) {
    if auth_result_with(&record_path(), token().as_ref(), used, ok, &RELOGIN) {
        emit_changed();
    }
}

/// 切方式、登录、退出之后：forge 的登录状态、PR、合并方式缓存全部作废，通知前端刷新。
fn connection_changed() {
    crate::forge::invalidate_connection();
    emit_changed();
}

pub(crate) fn status_with(mode: Mode, token: Option<Token>, record: &AccountRecord, gh_installed: bool, credential_supported: bool, pending: Option<GithubDeviceCodeDto>) -> GithubConnectionDto {
    let signed_in = token.is_some();
    GithubConnectionDto {
        mode: mode.as_str().into(),
        active: resolve(mode, token).label().into(),
        account: if signed_in { record.account.clone() } else { None },
        needs_relogin: signed_in && record.needs_relogin,
        gh_installed,
        credential_supported,
        pending,
    }
}

fn status_now() -> GithubConnectionDto {
    let pending = lock(&LOGIN).pending.clone();
    status_with(mode(), token(), &load_record(&record_path()), crate::forge::gh_installed(), SystemVault.supported(), pending)
}

// ═══ 登录流程（可注入，单测覆盖取消与并发）═══

/// 开始一轮登录：先占代次（作废之前的一轮），再向 GitHub 要代码（不持锁）；拿回代码时代次已经变了
///（期间点了取消或又开了一轮）就作废这一轮，回 `github/cancelled`。
pub(crate) fn begin_login_with(slot: &Mutex<LoginSlot>, http: &dyn Http, client_id: &str, now_ms: i64) -> Result<(u64, device::DeviceCode, GithubDeviceCodeDto), String> {
    let generation = {
        let mut s = lock(slot);
        s.generation += 1;
        s.pending = None;
        s.generation
    };
    let code = device::request_code(http, client_id)?;
    let dto = GithubDeviceCodeDto {
        user_code: code.user_code.clone(),
        verification_uri: code.verification_uri.clone(),
        expires_at: now_ms + code.expires_in as i64 * 1000,
    };
    let mut s = lock(slot);
    if s.generation != generation {
        return Err("github/cancelled".into());
    }
    s.pending = Some(dto.clone());
    Ok((generation, code, dto))
}

/// 一轮登录收尾的结果。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    SignedIn(GithubAccountDto),
    /// 被取消、退出或被新一轮顶替：什么都不做，令牌不落库。
    Stale,
    Expired,
    Denied,
    Failed(String),
}

/// 收尾：问 GitHub「这是谁」的网络阶段不持锁（不卡取消、不卡状态查询）；落库前拿锁再核对代次，
/// 已取消 / 已被顶替就把令牌丢掉。落库与 on_stored（更新进程内令牌）都在锁内：退出登录要等它做完，
/// 不会出现「刚退出又被晚到的登录写回」。
pub(crate) fn finish_login_with(
    slot: &Mutex<LoginSlot>,
    generation: u64,
    outcome: device::Outcome,
    http: &dyn Http,
    vault: &dyn Vault,
    path: &Path,
    on_stored: &dyn Fn(&Token),
) -> Finish {
    let close = |result: Finish| {
        let mut s = lock(slot);
        if s.generation != generation {
            return Finish::Stale;
        }
        s.pending = None;
        result
    };
    let token = match outcome {
        device::Outcome::Token(token) => token,
        device::Outcome::Cancelled => return Finish::Stale,
        device::Outcome::Expired => return close(Finish::Expired),
        device::Outcome::Denied => return close(Finish::Denied),
        device::Outcome::Failed(e) => return close(Finish::Failed(e)),
    };
    if lock(slot).generation != generation {
        return Finish::Stale;
    }
    let account = match fetch_account(http, &token) {
        Ok(account) => account,
        Err(e) => return close(Finish::Failed(e)),
    };
    let mut s = lock(slot);
    if s.generation != generation {
        return Finish::Stale;
    }
    s.pending = None;
    match store_login(vault, path, &token, &account) {
        Ok(()) => {
            on_stored(&token);
            Finish::SignedIn(account)
        }
        Err(e) => Finish::Failed(e),
    }
}

/// 退出：作废进行中的登录、删凭据库里的令牌、删账号记录，再 on_signed_out（清进程内令牌、作废缓存）。
pub(crate) fn logout_with(slot: &Mutex<LoginSlot>, vault: &dyn Vault, path: &Path, on_signed_out: &dyn Fn()) -> Result<(), String> {
    let mut s = lock(slot);
    s.generation += 1;
    s.pending = None;
    vault.delete()?;
    save_record(path, &AccountRecord::default())?;
    on_signed_out();
    Ok(())
}

/// 切方式：串行写设置与内存，再 on_changed（作废缓存、通知前端）。
pub(crate) fn set_mode_with(mode: Mode, write: &dyn Fn(Mode) -> Result<(), String>, on_changed: &dyn Fn()) -> Result<(), String> {
    let _guard = lock(&MODE_WRITE);
    write(mode)?;
    *lock(&MODE) = Some(mode);
    on_changed();
    Ok(())
}

// ═══ 命令 ═══

#[tauri::command]
pub(crate) async fn github_status() -> Result<GithubConnectionDto, String> {
    tauri::async_runtime::spawn_blocking(status_now).await.map_err(|e| e.to_string())
}

/// 远程 RPC 用：同 [`github_status`]，但不带进行中的登录代码（登录只在桌面设置页做）。
pub(crate) async fn github_status_remote() -> Result<GithubConnectionDto, String> {
    github_status().await.map(|mut dto| {
        dto.pending = None;
        dto
    })
}

#[tauri::command]
pub(crate) async fn github_set_mode(app: tauri::AppHandle, mode: String) -> Result<GithubConnectionDto, String> {
    let parsed = Mode::parse(&mode).ok_or("github/invalid_mode")?;
    let emit_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let write = |mode: Mode| {
            let settings = crate::settings::update_settings(|s| {
                s.github_connection = mode.as_str().into();
                Ok(s.clone())
            })?;
            crate::settings::emit_settings_changed(&emit_app, settings);
            Ok(())
        };
        set_mode_with(parsed, &write, &connection_changed)?;
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, serde::Serialize)]
struct LoginFinished {
    state: &'static str,
    error: Option<String>,
}

fn emit_login(app: &tauri::AppHandle, state: &'static str, error: Option<String>) {
    let _ = app.emit(LOGIN_EVENT, LoginFinished { state, error });
}

/// 开始设备码登录：向 GitHub 要一次性代码，立刻返回给设置页显示；后台线程按 GitHub 给的间隔轮询，
/// 结果经 [`LOGIN_EVENT`] 推给前端。再点一次「登录」= 作废上一轮、重新要代码。
#[tauri::command]
pub(crate) async fn github_login_start(app: tauri::AppHandle) -> Result<GithubDeviceCodeDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !SystemVault.supported() {
            return Err("github/credential_unsupported".to_string());
        }
        let (generation, code, dto) = begin_login_with(&LOGIN, &RealHttp, CLIENT_ID, crate::now_ms())?;
        std::thread::spawn(move || {
            let cancelled = || lock(&LOGIN).generation != generation;
            let outcome = device::wait_for_token(&RealHttp, CLIENT_ID, &code, &cancelled, &std::thread::sleep);
            let stored = |token: &Token| set_token(Some(token.clone()));
            match finish_login_with(&LOGIN, generation, outcome, &RealHttp, &SystemVault, &record_path(), &stored) {
                Finish::SignedIn(_) => {
                    connection_changed();
                    emit_login(&app, "ok", None);
                }
                Finish::Stale => {}
                Finish::Expired => emit_login(&app, "expired", None),
                Finish::Denied => emit_login(&app, "denied", None),
                Finish::Failed(e) => emit_login(&app, "error", Some(e)),
            }
        });
        Ok(dto)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 取消进行中的登录：作废代次，后台轮询在 250ms 内退出，晚到的令牌不会落库。
#[tauri::command]
pub(crate) async fn github_login_cancel() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut slot = lock(&LOGIN);
        slot.generation += 1;
        slot.pending = None;
    })
    .await
    .map_err(|e| e.to_string())
}

/// 退出登录：删凭据库里的令牌、删账号记录、作废缓存。令牌本身在 GitHub 上的撤销要 client secret，
/// 桌面端做不了；设置页写明可以去 GitHub 的「Applications」里撤销授权。
#[tauri::command]
pub(crate) async fn github_logout() -> Result<GithubConnectionDto, String> {
    tauri::async_runtime::spawn_blocking(|| {
        logout_with(&LOGIN, &SystemVault, &record_path(), &|| {
            set_token(None);
            connection_changed();
        })?;
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}
