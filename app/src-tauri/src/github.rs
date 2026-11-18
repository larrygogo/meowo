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

pub(crate) use credential::{SystemVault, Vault};

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
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

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
fn agent(proxy: Option<&str>) -> ureq::Agent {
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
        follow_redirects(&raw_send, req)
    }
}

/// GraphQL 带上 gh 一直带的预览头：mergeStateStatus 早年是预览字段。2026-11-18 用 gh 2.96 实测 cli/cli，
/// 不带也能拿到（见 progress.md），这里与 gh 保持一致，防 GitHub 对老账号 / 企业设置收紧。
const GRAPHQL_ACCEPT: &str = "application/vnd.github.merge-info-preview+json, application/vnd.github+json";

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

/// 拿到令牌之后：先问 GitHub 这是谁（顺带验令牌），再存进凭据库，最后写不含令牌的账号记录。
/// 存凭据失败就不写记录：界面不会显示「已登录」而令牌其实没存下。
pub(crate) fn complete_login(http: &dyn Http, vault: &dyn Vault, path: &Path, token: &Token) -> Result<GithubAccountDto, String> {
    let account = fetch_account(http, token)?;
    vault.write(token)?;
    save_record(path, &AccountRecord { account: Some(account.clone()), needs_relogin: false })?;
    Ok(account)
}

// ═══ 进程内状态 ═══

struct Session {
    loaded: bool,
    token: Option<Token>,
}

static SESSION: Mutex<Session> = Mutex::new(Session { loaded: false, token: None });
static MODE: Mutex<Option<Mode>> = Mutex::new(None);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

struct LoginSlot {
    generation: u64,
    pending: Option<GithubDeviceCodeDto>,
}

static LOGIN: Mutex<LoginSlot> = Mutex::new(LoginSlot { generation: 0, pending: None });

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 启动时记下 AppHandle：令牌在后台线程里被拒时要通知前端。
pub(crate) fn init(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

fn emit_changed() {
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
}

impl Executor {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Executor::Off => "off",
            Executor::Gh => "gh",
            Executor::Meowo(_) => "meowo",
        }
    }
}

/// 方式 + 有没有令牌 → 执行器。自动：有令牌用 Meowo（令牌被拒也不偷偷退回 gh，给同一个「重新登录」提示）。
pub(crate) fn resolve(mode: Mode, token: Option<Token>) -> Executor {
    match (mode, token) {
        (Mode::Off, _) => Executor::Off,
        (Mode::Gh, _) => Executor::Gh,
        (Mode::Meowo, token) => Executor::Meowo(token),
        (Mode::Auto, Some(token)) => Executor::Meowo(Some(token)),
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

/// 内置实现收到 401：令牌被撤销或失效。标「需要重新登录」并通知前端（只在状态变了时）。
pub(crate) fn on_unauthorized() {
    let path = record_path();
    let mut record = load_record(&path);
    if record.needs_relogin {
        return;
    }
    record.needs_relogin = true;
    let _ = save_record(&path, &record);
    emit_changed();
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
    crate::settings::patch_settings(app, move |s| s.github_connection = parsed.as_str().into()).await?;
    *lock(&MODE) = Some(parsed);
    tauri::async_runtime::spawn_blocking(|| {
        connection_changed();
        status_now()
    })
    .await
    .map_err(|e| e.to_string())
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
        let code = device::request_code(&RealHttp, CLIENT_ID)?;
        let dto = GithubDeviceCodeDto {
            user_code: code.user_code.clone(),
            verification_uri: code.verification_uri.clone(),
            expires_at: crate::now_ms() + code.expires_in as i64 * 1000,
        };
        let generation = {
            let mut slot = lock(&LOGIN);
            slot.generation += 1;
            slot.pending = Some(dto.clone());
            slot.generation
        };
        std::thread::spawn(move || {
            let cancelled = || lock(&LOGIN).generation != generation;
            let outcome = device::wait_for_token(&RealHttp, CLIENT_ID, &code, &cancelled, &std::thread::sleep);
            finish_login(&app, generation, outcome);
        });
        Ok(dto)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn finish_login(app: &tauri::AppHandle, generation: u64, outcome: device::Outcome) {
    // 拿着登录锁做完收尾：取消要么在这之前（这里看到代号变了，令牌直接丢掉、不落库），要么在这之后（已登录）。
    let mut slot = lock(&LOGIN);
    if slot.generation != generation {
        return;
    }
    slot.pending = None;
    match outcome {
        device::Outcome::Token(token) => match complete_login(&RealHttp, &SystemVault, &record_path(), &token) {
            Ok(_) => {
                set_token(Some(token));
                drop(slot);
                connection_changed();
                emit_login(app, "ok", None);
            }
            Err(e) => emit_login(app, "error", Some(e)),
        },
        device::Outcome::Expired => emit_login(app, "expired", None),
        device::Outcome::Denied => emit_login(app, "denied", None),
        device::Outcome::Cancelled => {}
        device::Outcome::Failed(e) => emit_login(app, "error", Some(e)),
    }
}

/// 取消进行中的登录：作废代号，后台轮询在下一次醒来时退出，晚到的令牌不会落库。
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
        {
            let mut slot = lock(&LOGIN);
            slot.generation += 1;
            slot.pending = None;
        }
        SystemVault.delete()?;
        set_token(None);
        save_record(&record_path(), &AccountRecord::default())?;
        connection_changed();
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}
