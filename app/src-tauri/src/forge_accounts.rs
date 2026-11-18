//! 代码托管账号（设置 › 代码托管账号）：平台 + 主机 + 账号名。
//!
//! - 令牌只进系统凭据库（github/credential.rs 的 [`KeyVault`]，条目名 `Meowo/<平台>/<主机>/<账号>`，GitLab 设备码
//!   登录的刷新令牌另存 `…#refresh`）；`~/.meowo/forge-accounts.json` 只存非敏感的清单（平台、主机、账号名、展示名、
//!   Bitbucket 的 Atlassian 邮箱、是否设备码登录、访问令牌何时过期、要不要重新登录）。
//! - 按远端主机选账号：每台主机（同一平台）一个账号，再添加就替换。github.com 不在这里，沿用 GitHub 连接方式
//!   （github.rs）；企业版、GitLab、Gitea / Forgejo、Bitbucket Cloud 在这里。
//! - 添加时先用令牌问一次「我是谁」（各平台的当前用户接口），认得出才落库。
//! - gitlab.com 预留 OAuth 设备码登录（GitLab 文档 docs.gitlab.com/api/oauth2「Device authorization grant」，17.9 起正式可用）：
//!   要 Meowo 在 gitlab.com 注册的应用 Client ID，打包时用 MEOWO_GITLAB_CLIENT_ID 注入；没注入时只给令牌入口。
//!   GitLab 的 OAuth 访问令牌 2 小时过期，用刷新令牌续（刷新后旧的两个都作废，新的一并存回）。
//! - 远程 RPC 只放行只读的 [`forge_accounts_remote`]（不含进行中的登录代码），添加、退出、登录只在桌面设置页做。

use crate::forge::api::ForgeApi;
use crate::forge::bitbucket::{self, Bitbucket};
use crate::forge::gitea::{self, Gitea};
use crate::forge::gitlab::{self, Gitlab};
use crate::forge::http::{json, Auth, Client, ForgeHttp, RealForgeHttp, Req};
use crate::forge::platform::{self, Forges, Platform};
use crate::forge::GhState;
use crate::github::{Body, KeyVault, Token, Vault};
use meowo_protocol::ipc::{ForgeAccountDto, ForgeAccountsDto, ForgeDeviceCodeDto};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Meowo 在 gitlab.com 注册的 OAuth 应用（非保密应用，开设备码）。还没注册：空串，设置页只给令牌入口。
pub(crate) const GITLAB_CLIENT_ID: &str = match option_env!("MEOWO_GITLAB_CLIENT_ID") {
    Some(id) if !id.is_empty() => id,
    _ => "",
};
/// 建 MR、合并、评论都要写权限：`api`（`read_api` 只够看）。
pub(crate) const GITLAB_SCOPE: &str = "api";
/// 一次设备码登录结束：`{ state: ok|expired|denied|error, error? }`。取消不发。
pub(crate) const LOGIN_EVENT: &str = "forge-login-finished";

// ═══ 清单（不含令牌）═══

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Account {
    pub(crate) platform: String,
    /// 主机（可带端口）。Bitbucket Cloud 恒为 bitbucket.org。
    pub(crate) host: String,
    pub(crate) login: String,
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// Bitbucket：Atlassian 账号邮箱（API token 走 Basic 认证要它）；没有就用 Bearer。
    #[serde(default)]
    pub(crate) auth_user: Option<String>,
    /// 设备码（OAuth）登录的；否则是个人访问令牌。
    #[serde(default)]
    pub(crate) oauth: bool,
    /// OAuth 访问令牌过期的时刻（毫秒）。
    #[serde(default)]
    pub(crate) expires_at: Option<i64>,
    #[serde(default)]
    pub(crate) needs_relogin: bool,
}

impl Account {
    fn platform(&self) -> Option<Platform> {
        Platform::parse(&self.platform)
    }
    fn dto(&self) -> ForgeAccountDto {
        ForgeAccountDto {
            platform: self.platform.clone(),
            host: self.host.clone(),
            login: self.login.clone(),
            name: self.name.clone(),
            needs_relogin: self.needs_relogin,
            oauth: self.oauth,
        }
    }
    /// 令牌缓存与凭据库的键。
    fn key(&self) -> String {
        format!("{}/{}/{}", self.platform, self.host, self.login)
    }
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct AccountFile {
    #[serde(default)]
    accounts: Vec<Account>,
}

fn list_path() -> PathBuf {
    crate::db_path().with_file_name("forge-accounts.json")
}

pub(crate) fn load_list(path: &Path) -> Vec<Account> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<AccountFile>(&raw).ok())
        .map(|f| f.accounts)
        .unwrap_or_default()
}

pub(crate) fn save_list(path: &Path, accounts: &[Account]) -> Result<(), String> {
    if accounts.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = serde_json::to_string_pretty(&AccountFile { accounts: accounts.to_vec() }).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(path, &body).map_err(|e| e.to_string())
}

/// 这台主机（不含端口）的账号。
pub(crate) fn account_for<'l>(list: &'l [Account], host: &str) -> Option<&'l Account> {
    // 只比主机名：https 远端带网页端口、ssh 远端带的是 ssh 端口（已去掉），同一台主机一个账号。
    list.iter().find(|a| platform::host_name(&platform::host_key(&a.host)) == platform::host_name(&platform::host_key(host)))
}

/// 凭据库的条目。refresh = GitLab 的刷新令牌那一条。
pub(crate) fn vault_for(account: &Account, refresh: bool) -> KeyVault {
    let suffix = if refresh { "#refresh" } else { "" };
    KeyVault {
        target: format!("Meowo/{}{suffix}", account.key()),
        account: format!("{}@{}", account.login, account.host),
        label: format!("Meowo {} {}", account.platform, account.host),
    }
}

/// 凭据库的入口（测试换成内存里的）。
pub(crate) type Vaults<'a> = &'a dyn Fn(&Account, bool) -> Box<dyn Vault>;

fn system_vaults(account: &Account, refresh: bool) -> Box<dyn Vault> {
    Box::new(vault_for(account, refresh))
}

// ═══ 平台接口的组装 ═══

/// 平台 API 在哪台主机上（Bitbucket Cloud 的 API 在 api.bitbucket.org）。
pub(crate) fn api_host(platform: Platform, host: &str) -> String {
    match platform {
        Platform::Bitbucket => bitbucket::API_HOST.into(),
        _ => host.to_string(),
    }
}

/// 令牌 → 认证头：GitLab / 企业版 Bearer；Gitea `token`；Bitbucket 有邮箱用 Basic，没有用 Bearer。
pub(crate) fn auth_for(platform: Platform, token: Token, auth_user: Option<&str>) -> Auth {
    match (platform, auth_user.filter(|u| !u.is_empty())) {
        (Platform::Gitea, _) => Auth::Token(token),
        (Platform::Bitbucket, Some(user)) => Auth::Basic {
            user: user.to_string(),
            token,
        },
        _ => Auth::Bearer(token),
    }
}

/// 一个账号的平台接口。
pub(crate) fn build<'a>(
    account: &Account,
    token: Option<Token>,
    http: Box<dyn ForgeHttp + 'a>,
    prefs: PathBuf,
    on_auth: Box<dyn Fn(bool) + Sync + 'a>,
) -> Option<Box<dyn ForgeApi + 'a>> {
    let platform = account.platform()?;
    let state = if account.needs_relogin || token.is_none() {
        GhState::Unauthed
    } else {
        GhState::Ok
    };
    let login = Some(account.login.clone());
    if platform == Platform::Github {
        return Some(Box::new(crate::forge::ghe::forge(http, &account.host, token, state, prefs, on_auth)));
    }
    let client = Client {
        http,
        auth: token.map(|t| auth_for(platform, t, account.auth_user.as_deref())),
        on_auth,
    };
    Some(match platform {
        Platform::Gitlab => Box::new(Gitlab {
            client,
            host: account.host.clone(),
            state,
            login,
        }),
        Platform::Gitea => Box::new(Gitea {
            client,
            host: account.host.clone(),
            state,
            login,
        }),
        Platform::Bitbucket => Box::new(Bitbucket { client, state, login }),
        Platform::Github => unreachable!(),
    })
}

/// 用令牌问一次「我是谁」：(账号名, 展示名)。
pub(crate) fn whoami(
    http: &dyn ForgeHttp,
    platform: Platform,
    host: &str,
    token: &Token,
    auth_user: Option<&str>,
) -> Result<(String, Option<String>), String> {
    if platform == Platform::Github {
        return crate::forge::ghe::current_user(http, host, token);
    }
    let client = Client {
        http: Box::new(http),
        auth: Some(auth_for(platform, token.clone(), auth_user)),
        on_auth: Box::new(|_| {}),
    };
    match platform {
        Platform::Gitlab => gitlab::current_user(&client, host),
        Platform::Gitea => gitea::current_user(&client, host),
        Platform::Bitbucket => bitbucket::current_user(&client),
        Platform::Github => unreachable!(),
    }
}

// ═══ 添加 / 退出 / 令牌好坏（可注入，单测覆盖）═══

/// 校验平台与主机：github.com 走 GitHub 连接方式；公共主机的平台是固定的；Bitbucket 只有 Cloud。
pub(crate) fn check_target(platform: &str, host: &str) -> Result<(Platform, String), String> {
    let platform = Platform::parse(platform).ok_or("forge/invalid_platform")?;
    let host = platform::normalize_host_input(host).ok_or("forge/invalid_host")?;
    let name = platform::host_name(&host);
    if name == platform::GITHUB_COM {
        return Err("forge/use_github_login".into());
    }
    if platform == Platform::Bitbucket && name != "bitbucket.org" {
        return Err("forge/bitbucket_cloud_only".into());
    }
    if let Some(known) = platform::public_platform(name) {
        if known != platform {
            return Err(format!("forge/platform_mismatch: {}", known.as_str()));
        }
    }
    Ok((platform, host))
}

/// 新账号写进清单：同主机的旧账号（换了人、换了令牌）先从凭据库删掉再替换。
fn put_account(path: &Path, vaults: Vaults, account: Account) -> Result<(), String> {
    let mut list = load_list(path);
    for old in list
        .iter()
        .filter(|a| platform::host_name(&a.host).eq_ignore_ascii_case(platform::host_name(&account.host)) && a.key() != account.key())
    {
        let _ = vaults(old, false).delete();
        let _ = vaults(old, true).delete();
    }
    list.retain(|a| !platform::host_name(&a.host).eq_ignore_ascii_case(platform::host_name(&account.host)));
    list.push(account);
    list.sort_by(|a, b| (a.platform.as_str(), a.host.as_str()).cmp(&(b.platform.as_str(), b.host.as_str())));
    save_list(path, &list)
}

/// 用个人访问令牌添加：先验令牌（问「我是谁」），令牌存进凭据库，再写清单。存凭据失败就不写清单。
pub(crate) fn add_with(
    path: &Path,
    vaults: Vaults,
    http: &dyn ForgeHttp,
    platform: &str,
    host: &str,
    token: &str,
    user: Option<&str>,
) -> Result<Account, String> {
    let (platform, host) = check_target(platform, host)?;
    let token = Token::secret(token).ok_or("forge/invalid_token")?;
    let user = user.map(str::trim).filter(|u| !u.is_empty());
    if user.is_some_and(|u| u.contains(['"', '\\', '\n', '\r', ':'])) {
        return Err("forge/invalid_user".into());
    }
    let (login, name) =
        whoami(http, platform, &host, &token, user).map_err(|e| if e == "gh/not_authed" { "forge/token_rejected".to_string() } else { e })?;
    let account = Account {
        platform: platform.as_str().into(),
        host,
        login,
        name,
        auth_user: if platform == Platform::Bitbucket {
            user.map(str::to_string)
        } else {
            None
        },
        oauth: false,
        expires_at: None,
        needs_relogin: false,
    };
    vaults(&account, false).write(&token)?;
    let _ = vaults(&account, true).delete();
    put_account(path, vaults, account.clone())?;
    Ok(account)
}

/// 退出：删凭据库里的令牌（含刷新令牌）与清单里这一条。没有这一条也算成功。
pub(crate) fn remove_with(path: &Path, vaults: Vaults, platform: &str, host: &str) -> Result<(), String> {
    let mut list = load_list(path);
    let host = platform::normalize_host_input(host).unwrap_or_else(|| host.to_string());
    let (gone, keep): (Vec<Account>, Vec<Account>) = list.drain(..).partition(|a| a.platform == platform && a.host.eq_ignore_ascii_case(&host));
    for account in &gone {
        vaults(account, false).delete()?;
        vaults(account, true).delete()?;
    }
    save_list(path, &keep)
}

/// 平台接口每拿到一个回包报一次：401 → 标「要重新登录」，2xx → 清掉。只在用的正是当前令牌时改（旧令牌晚到的
/// 401 不能误伤新添加的账号）。返回 true = 清单变了。
pub(crate) fn auth_result_with(path: &Path, key: &str, used: &Token, current: Option<&Token>, ok: bool) -> bool {
    if current != Some(used) {
        return false;
    }
    let mut list = load_list(path);
    let Some(account) = list.iter_mut().find(|a| a.key() == key) else {
        return false;
    };
    if account.needs_relogin == !ok {
        return false;
    }
    account.needs_relogin = !ok;
    save_list(path, &list).is_ok()
}

// ═══ 进程内状态 ═══

/// 清单与令牌的内存副本：清单在添加、退出、标记时更新；令牌第一次用时从凭据库读一次。
struct Cache {
    list: Option<Vec<Account>>,
    tokens: Option<HashMap<String, Option<Token>>>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { list: None, tokens: None });

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn list() -> Vec<Account> {
    let mut cache = lock(&CACHE);
    cache.list.get_or_insert_with(|| load_list(&list_path())).clone()
}

fn forget_all() {
    let mut cache = lock(&CACHE);
    cache.list = None;
    cache.tokens = None;
}

fn token_of(account: &Account) -> Option<Token> {
    if let Some(token) = lock(&CACHE).tokens.as_ref().and_then(|t| t.get(&account.key())) {
        return token.clone();
    }
    let token = match vault_for(account, false).read() {
        Ok(token) => token,
        Err(e) => {
            eprintln!("[forge] 读凭据库失败：{e}");
            None
        }
    };
    lock(&CACHE).tokens.get_or_insert_with(HashMap::new).insert(account.key(), token.clone());
    token
}

/// 账号变了：forge 的缓存作废，通知前端（设置页、「更改」面板、侧栏角标都重拉）。
fn changed() {
    forget_all();
    crate::forge::invalidate_connection();
    crate::github::emit_changed();
}

/// 一次回包的令牌好坏怎么落到清单上（可注入，单测覆盖）。审查指出：401 一次就永久标重登不对——GitLab 没有合并
/// 权限也回 401。现在：
/// - 2xx：只有清单里正标着「要重新登录」时才去改（不每个回包都读写文件）；
/// - 401：已经标过就什么都不做；没标过先用同一个令牌问一次「我是谁」（confirm，不再回报），确认令牌确实被拒才标。
///
/// 返回 true = 清单变了（要通知前端）。
pub(crate) fn report_with(path: &Path, key: &str, used: &Token, current: Option<&Token>, ok: bool, marked: bool, confirm: &dyn Fn() -> bool) -> bool {
    if ok != marked {
        // ok 且没标 / 被拒且已标：没有要改的。
        return false;
    }
    if !ok && !confirm() {
        return false;
    }
    auth_result_with(path, key, used, current, ok)
}

fn report_auth(key: String, used: Token, ok: bool, confirm: &dyn Fn() -> bool) {
    let account = list().into_iter().find(|a| a.key() == key);
    let marked = account.as_ref().is_some_and(|a| a.needs_relogin);
    let current = account.and_then(|a| token_of(&a));
    if report_with(&list_path(), &key, &used, current.as_ref(), ok, marked, confirm) {
        lock(&CACHE).list = None;
        crate::github::emit_changed();
    }
}

fn refresh_due(account: &Account, now_ms: i64) -> bool {
    account.oauth && account.expires_at.is_some_and(|at| at <= now_ms + 60_000)
}

/// 每个账号一把续期锁：侧栏角标按目录并行查 PR（badge.rs DIR_WORKERS），几个线程同时发现令牌快过期时只能有一个去续。
/// GitLab 续期后旧的刷新令牌立即作废，第二个线程拿旧刷新令牌再续会被拒，以前就把账号误标成「要重新登录」。
fn refresh_lock(key: &str) -> std::sync::Arc<Mutex<()>> {
    static LOCKS: Mutex<Option<HashMap<String, std::sync::Arc<Mutex<()>>>>> = Mutex::new(None);
    lock(&LOCKS).get_or_insert_with(HashMap::new).entry(key.to_string()).or_default().clone()
}

/// 续期的结果。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// 不用续，或别的线程刚续过：用凭据库里现在的令牌。
    Current(Option<Token>),
    /// 续好了。
    Renewed(Token),
    /// 刷新令牌被拒（撤销、过期）：已标「要重新登录」。
    Rejected,
}

/// 续期（可注入，单测覆盖并发）。拿到这个账号的锁之后重读清单与凭据库：别的线程可能刚续过，清单里的过期时刻已经
/// 往后挪了，这时直接用新令牌。连不上（还没收到回包）不算被拒，不标重登，下次再续。
pub(crate) fn refresh_with(path: &Path, vaults: Vaults, http: &dyn ForgeHttp, client_id: &str, key: &str, now_ms: i64) -> Refresh {
    let guard = refresh_lock(key);
    let _held = lock(&guard);
    let Some(account) = load_list(path).into_iter().find(|a| a.key() == key) else { return Refresh::Current(None) };
    let current = || vaults(&account, false).read().ok().flatten();
    if !refresh_due(&account, now_ms) || account.needs_relogin {
        return Refresh::Current(current());
    }
    let Some(refresh) = vaults(&account, true).read().ok().flatten() else { return Refresh::Current(current()) };
    match oauth_refresh(http, &account.host, client_id, &refresh) {
        Ok(tokens) => match store_oauth(path, vaults, account.clone(), &tokens, now_ms) {
            Ok(_) => Refresh::Renewed(tokens.access),
            Err(_) => Refresh::Current(current()),
        },
        Err(e) if e.starts_with("forge/network") || e.starts_with("forge/tls") => Refresh::Current(current()),
        Err(_) => {
            let mut list = load_list(path);
            if let Some(a) = list.iter_mut().find(|a| a.key() == key) {
                a.needs_relogin = true;
            }
            let _ = save_list(path, &list);
            Refresh::Rejected
        }
    }
}

/// GitLab 设备码登录的访问令牌快过期了：用刷新令牌续，新的一对存回凭据库。刷新令牌被拒才标「要重新登录」。
fn refresh_if_due(account: &Account) -> Option<Token> {
    if !refresh_due(account, crate::now_ms()) || GITLAB_CLIENT_ID.is_empty() {
        return token_of(account);
    }
    let http = RealForgeHttp { host: account.host.clone() };
    match refresh_with(&list_path(), &system_vaults, &http, GITLAB_CLIENT_ID, &account.key(), crate::now_ms()) {
        Refresh::Current(token) => token,
        Refresh::Renewed(token) => {
            forget_all();
            Some(token)
        }
        Refresh::Rejected => {
            forget_all();
            crate::github::emit_changed();
            None
        }
    }
}

/// 生产的主机 → 平台 / 账号。
pub(crate) struct Accounts;

impl Forges for Accounts {
    fn platform(&self, host: &str) -> Option<Platform> {
        platform::public_platform(host).or_else(|| account_for(&list(), host).and_then(Account::platform))
    }

    fn usable(&self, host: &str) -> bool {
        platform::host_name(host) == platform::GITHUB_COM || account_for(&list(), host).is_some()
    }

    fn host_of(&self, host: &str) -> String {
        match account_for(&list(), host) {
            Some(account) if platform::public_platform(host).is_none() => platform::host_key(&account.host),
            _ => platform::host_key(host),
        }
    }

    fn open(&self, host: &str) -> Option<Box<dyn ForgeApi + '_>> {
        let account = account_for(&list(), host)?.clone();
        let platform = account.platform()?;
        // 公共主机的平台是固定的：清单里写错了平台（手改文件）就不用它。
        if platform::public_platform(host).is_some_and(|p| p != platform) || platform::host_name(host) == platform::GITHUB_COM {
            return None;
        }
        let token = refresh_if_due(&account);
        let http = RealForgeHttp {
            host: api_host(platform, &account.host),
        };
        let key = account.key();
        let used = token.clone();
        let (confirm_host, confirm_user, confirm_api) = (account.host.clone(), account.auth_user.clone(), api_host(platform, &account.host));
        let on_auth = move |ok: bool| {
            if let Some(used) = &used {
                // 确认令牌被拒：用同一个令牌问「我是谁」，只有它也 401 才算。
                let confirm = || {
                    let probe = RealForgeHttp { host: confirm_api.clone() };
                    whoami(&probe, platform, &confirm_host, used, confirm_user.as_deref()).err().as_deref() == Some("gh/not_authed")
                };
                report_auth(key.clone(), used.clone(), ok, &confirm);
            }
        };
        build(
            &account,
            token,
            Box::new(http),
            crate::db_path().with_file_name("forge.json"),
            Box::new(on_auth),
        )
    }
}

pub(crate) static ACCOUNTS: Accounts = Accounts;

// ═══ GitLab 设备码（RFC 8628；GitLab 文档 docs.gitlab.com/api/oauth2）═══

/// 设备码。`device_code` 是轮询凭据：只在后端，Debug 打码。
#[derive(Clone)]
pub(crate) struct DeviceCode {
    pub(crate) device_code: String,
    pub(crate) user_code: String,
    pub(crate) verification_uri: String,
    pub(crate) verification_uri_complete: Option<String>,
    pub(crate) expires_in: u64,
    pub(crate) interval: u64,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// 一组 OAuth 令牌。
pub(crate) struct OauthTokens {
    pub(crate) access: Token,
    pub(crate) refresh: Option<Token>,
    pub(crate) expires_in: Option<u64>,
}

fn form(http: &dyn ForgeHttp, url: &str, pairs: Vec<(&'static str, String)>) -> Result<crate::github::Response, String> {
    http.send(&Req {
        method: "POST",
        url,
        auth: None,
        body: Some(Body::Form(pairs)),
        accept: None,
    })
}

fn tokens_of(v: &serde_json::Value) -> Option<OauthTokens> {
    Some(OauthTokens {
        access: v.get("access_token").and_then(|t| t.as_str()).and_then(Token::secret)?,
        refresh: v.get("refresh_token").and_then(|t| t.as_str()).and_then(Token::secret),
        expires_in: v.get("expires_in").and_then(|t| t.as_u64()),
    })
}

/// 第 1 步：要一次性代码。
pub(crate) fn request_code(http: &dyn ForgeHttp, host: &str, client_id: &str) -> Result<DeviceCode, String> {
    let url = format!("https://{host}/oauth/authorize_device");
    let resp = form(
        http,
        &url,
        vec![("client_id", client_id.to_string()), ("scope", GITLAB_SCOPE.to_string())],
    )
    .map_err(|e| crate::forge::http::network_error(&e))?;
    let v = json(&resp.body);
    if let Some(error) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("forge/login_failed: {error}"));
    }
    if resp.status != 200 {
        return Err(format!("forge/login_failed: HTTP {}", resp.status));
    }
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_string);
    let missing = || "forge/login_failed: bad response".to_string();
    Ok(DeviceCode {
        device_code: s("device_code").ok_or_else(missing)?,
        user_code: s("user_code").ok_or_else(missing)?,
        verification_uri: s("verification_uri").unwrap_or_else(|| format!("https://{host}/oauth/device")),
        verification_uri_complete: s("verification_uri_complete"),
        expires_in: v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(300),
        interval: v.get("interval").and_then(|x| x.as_u64()).unwrap_or(5).max(1),
    })
}

#[derive(Debug)]
pub(crate) enum Poll {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Tokens(Box<OauthTokens>),
    Transport(String),
    Failed(String),
}

impl std::fmt::Debug for OauthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OauthTokens(<redacted>)")
    }
}

pub(crate) fn poll_once(http: &dyn ForgeHttp, host: &str, client_id: &str, device_code: &str) -> Poll {
    let url = format!("https://{host}/oauth/token");
    let pairs = vec![
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code".to_string()),
        ("device_code", device_code.to_string()),
        ("client_id", client_id.to_string()),
    ];
    let resp = match form(http, &url, pairs) {
        Ok(resp) => resp,
        Err(e) => return Poll::Transport(e),
    };
    let v = json(&resp.body);
    match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown,
        Some("expired_token") => Poll::Expired,
        Some("access_denied") => Poll::Denied,
        Some(other) => Poll::Failed(format!("forge/login_failed: {other}")),
        None => match tokens_of(&v) {
            Some(tokens) => Poll::Tokens(Box::new(tokens)),
            None if resp.status >= 500 => Poll::Transport(format!("HTTP {}", resp.status)),
            None => Poll::Failed(format!("forge/login_failed: HTTP {}", resp.status)),
        },
    }
}

#[derive(Debug)]
pub(crate) enum Outcome {
    Tokens(Box<OauthTokens>),
    Expired,
    Denied,
    Cancelled,
    Failed(String),
}

/// 第 3 步：按间隔轮询，直到拿到令牌、过期、被拒、取消或明确失败。slow_down 每次 +5 秒（RFC 8628 §3.5）；
/// 连着 3 次连不上才放弃；过期按 expires_in 自己累计。
pub(crate) fn wait_for_token(
    http: &dyn ForgeHttp,
    host: &str,
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
    sleep: &dyn Fn(Duration),
) -> Outcome {
    let mut interval = code.interval;
    let mut waited = 0;
    let mut failures = 0;
    loop {
        // 每 250ms 看一眼取消。
        let mut left = Duration::from_secs(interval);
        while !left.is_zero() {
            if cancelled() {
                return Outcome::Cancelled;
            }
            let step = left.min(Duration::from_millis(250));
            sleep(step);
            left -= step;
        }
        if cancelled() {
            return Outcome::Cancelled;
        }
        waited += interval;
        match poll_once(http, host, client_id, &code.device_code) {
            Poll::Tokens(tokens) => return Outcome::Tokens(tokens),
            Poll::Expired => return Outcome::Expired,
            Poll::Denied => return Outcome::Denied,
            Poll::Failed(e) => return Outcome::Failed(e),
            Poll::Pending => failures = 0,
            Poll::SlowDown => {
                failures = 0;
                interval += 5;
            }
            Poll::Transport(e) => {
                failures += 1;
                if failures >= 3 {
                    return Outcome::Failed(format!("forge/network: {e}"));
                }
            }
        }
        if waited >= code.expires_in {
            return Outcome::Expired;
        }
    }
}

/// 用刷新令牌换新的一对（非保密应用，不带 client secret）。
pub(crate) fn oauth_refresh(http: &dyn ForgeHttp, host: &str, client_id: &str, refresh: &Token) -> Result<OauthTokens, String> {
    let url = format!("https://{host}/oauth/token");
    let pairs = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh.expose().to_string()),
        ("client_id", client_id.to_string()),
    ];
    let resp = form(http, &url, pairs).map_err(|e| crate::forge::http::network_error(&e))?;
    let v = json(&resp.body);
    if resp.status != 200 {
        return Err(format!(
            "forge/login_failed: {}",
            v.get("error").and_then(|e| e.as_str()).unwrap_or("refresh")
        ));
    }
    tokens_of(&v).ok_or_else(|| "forge/login_failed: bad response".into())
}

/// 设备码登录的令牌落库：访问令牌与刷新令牌进凭据库，清单记 oauth 与过期时刻。
pub(crate) fn store_oauth(path: &Path, vaults: Vaults, mut account: Account, tokens: &OauthTokens, now_ms: i64) -> Result<Account, String> {
    account.oauth = true;
    account.needs_relogin = false;
    account.expires_at = tokens.expires_in.map(|s| now_ms + s as i64 * 1000);
    vaults(&account, false).write(&tokens.access)?;
    match &tokens.refresh {
        Some(refresh) => vaults(&account, true).write(refresh)?,
        None => vaults(&account, true).delete()?,
    }
    put_account(path, vaults, account.clone())?;
    Ok(account)
}

// ═══ 设备码登录的进程内状态（同 github.rs：代次作废旧的一轮）═══

struct LoginSlot {
    generation: u64,
    pending: Option<ForgeDeviceCodeDto>,
}

static LOGIN: Mutex<LoginSlot> = Mutex::new(LoginSlot {
    generation: 0,
    pending: None,
});

fn status_now() -> ForgeAccountsDto {
    ForgeAccountsDto {
        accounts: list().iter().map(Account::dto).collect(),
        credential_supported: crate::github::SystemVault.supported(),
        gitlab_device: !GITLAB_CLIENT_ID.is_empty(),
        pending: lock(&LOGIN).pending.clone(),
    }
}

#[derive(Clone, serde::Serialize)]
struct LoginFinished {
    state: &'static str,
    error: Option<String>,
}

fn emit_login(state: &'static str, error: Option<String>) {
    if let Some(app) = crate::github::app() {
        use tauri::Emitter;
        let _ = app.emit(LOGIN_EVENT, LoginFinished { state, error });
    }
}

// ═══ 命令 ═══

#[tauri::command]
pub(crate) async fn forge_accounts() -> Result<ForgeAccountsDto, String> {
    tauri::async_runtime::spawn_blocking(status_now).await.map_err(|e| e.to_string())
}

/// 远程 RPC 用：同 [`forge_accounts`]，不带进行中的登录代码。
pub(crate) async fn forge_accounts_remote() -> Result<ForgeAccountsDto, String> {
    forge_accounts().await.map(|mut dto| {
        dto.pending = None;
        dto
    })
}

/// 用个人访问令牌添加账号。user：Bitbucket 的 Atlassian 邮箱（可空）。
#[tauri::command]
pub(crate) async fn forge_account_add(platform: String, host: String, token: String, user: Option<String>) -> Result<ForgeAccountsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !crate::github::SystemVault.supported() {
            return Err("github/credential_unsupported".to_string());
        }
        let (parsed, normalized) = check_target(&platform, &host)?;
        let http = RealForgeHttp {
            host: api_host(parsed, &normalized),
        };
        add_with(&list_path(), &system_vaults, &http, &platform, &host, &token, user.as_deref())?;
        changed();
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn forge_account_remove(platform: String, host: String) -> Result<ForgeAccountsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        remove_with(&list_path(), &system_vaults, &platform, &host)?;
        changed();
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// gitlab.com 设备码登录：要代码、立刻返回给设置页显示；后台线程轮询，结果经 [`LOGIN_EVENT`] 推给前端。
#[tauri::command]
pub(crate) async fn forge_login_start() -> Result<ForgeDeviceCodeDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if GITLAB_CLIENT_ID.is_empty() {
            return Err("forge/device_unavailable".to_string());
        }
        if !crate::github::SystemVault.supported() {
            return Err("github/credential_unsupported".to_string());
        }
        let host = "gitlab.com".to_string();
        let generation = {
            let mut slot = lock(&LOGIN);
            slot.generation += 1;
            slot.pending = None;
            slot.generation
        };
        let http = RealForgeHttp { host: host.clone() };
        let code = request_code(&http, &host, GITLAB_CLIENT_ID)?;
        let dto = ForgeDeviceCodeDto {
            platform: "gitlab".into(),
            host: host.clone(),
            user_code: code.user_code.clone(),
            verification_uri: code.verification_uri.clone(),
            verification_uri_complete: code.verification_uri_complete.clone(),
            expires_at: crate::now_ms() + code.expires_in as i64 * 1000,
        };
        {
            let mut slot = lock(&LOGIN);
            if slot.generation != generation {
                return Err("forge/cancelled".to_string());
            }
            slot.pending = Some(dto.clone());
        }
        std::thread::spawn(move || {
            let stale = || lock(&LOGIN).generation != generation;
            let outcome = wait_for_token(&http, &host, GITLAB_CLIENT_ID, &code, &stale, &std::thread::sleep);
            let finish = |state: &'static str, error: Option<String>| {
                let mut slot = lock(&LOGIN);
                if slot.generation != generation {
                    return false;
                }
                slot.pending = None;
                drop(slot);
                emit_login(state, error);
                true
            };
            match outcome {
                Outcome::Cancelled => {}
                Outcome::Expired => {
                    finish("expired", None);
                }
                Outcome::Denied => {
                    finish("denied", None);
                }
                Outcome::Failed(e) => {
                    finish("error", Some(e));
                }
                Outcome::Tokens(tokens) => {
                    let result = whoami(&http, Platform::Gitlab, &host, &tokens.access, None).and_then(|(login, name)| {
                        if stale() {
                            return Err("forge/cancelled".into());
                        }
                        let account = Account {
                            platform: "gitlab".into(),
                            host: host.clone(),
                            login,
                            name,
                            ..Default::default()
                        };
                        store_oauth(&list_path(), &system_vaults, account, &tokens, crate::now_ms())
                    });
                    match result {
                        Err(e) if e == "forge/cancelled" => {}
                        Err(e) => {
                            finish("error", Some(e));
                        }
                        Ok(_) => {
                            if finish("ok", None) {
                                changed();
                            }
                        }
                    }
                }
            }
        });
        Ok(dto)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn forge_login_cancel() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut slot = lock(&LOGIN);
        slot.generation += 1;
        slot.pending = None;
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
