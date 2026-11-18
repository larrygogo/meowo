//! 代码托管账号（设置 › 代码托管账号）：平台 + 主机 + 账号名。
//!
//! - 令牌只进系统凭据库（github/credential.rs 的 [`KeyVault`]，条目名 `Meowo/<平台>/<主机>/<账号>`）；
//!   `~/.meowo/forge-accounts.json` 只存非敏感的清单（平台、主机、账号名、展示名、Bitbucket 的 Atlassian 邮箱、
//!   要不要重新登录）。
//! - 按远端主机选账号：每台主机（同一平台）一个账号，再添加就替换。github.com 与其他平台列在一起，账号存在
//!   github.rs（设备码登录或个人访问令牌，凭据条目 `Meowo/github.com`）；Meowo 不调用本机 gh（leader 2026-11-18
//!   「取消gh」）。企业版、GitLab（gitlab.com 与自管）、Gitea / Forgejo、Bitbucket Cloud、Gitee 用个人访问令牌。
//! - gitlab.com 不做设备码登录（leader 2026-11-18「取消gitlab」：不注册 OAuth 应用），与自管 GitLab 一样只用令牌。
//!   旧清单里设备码登录的 GitLab 账号读到时标「要重新添加」，并删掉它的两条凭据（访问令牌与 `#refresh`），见 [`migrate_oauth`]。
//! - 添加时先用令牌问一次「我是谁」（各平台的当前用户接口），认得出才落库。
//! - 远程 RPC 只放行只读的 [`forge_accounts_remote`]（不含进行中的登录代码），添加、退出、登录只在桌面设置页做。

use crate::forge::api::ForgeApi;
use crate::forge::bitbucket::{self, Bitbucket};
use crate::forge::gitea::{self, Gitea};
use crate::forge::gitlab::{self, Gitlab};
use crate::forge::http::{Auth, Client, ForgeHttp, RealForgeHttp};
use crate::forge::platform::{self, Forges, Platform};
use crate::forge::GhState;
use crate::github::{KeyVault, Token, Vault};
use meowo_protocol::ipc::{ForgeAccountDto, ForgeAccountsDto, ForgeDeviceCodeDto};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 一次 github.com 设备码登录结束：`{ state: ok|expired|denied|error, error? }`。取消不发。
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
    #[serde(default)]
    pub(crate) needs_relogin: bool,
    /// 旧版 gitlab.com 设备码登录的账号（只读，迁移用，见 [`migrate_oauth`]；新版不写）。
    #[serde(default, rename = "oauth", skip_serializing)]
    pub(crate) legacy_oauth: bool,
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
            oauth: false,
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

/// 凭据库的条目。
pub(crate) fn vault_for(account: &Account) -> KeyVault {
    KeyVault {
        target: format!("Meowo/{}", account.key()),
        account: format!("{}@{}", account.login, account.host),
        label: format!("Meowo {} {}", account.platform, account.host),
    }
}

/// 旧版 gitlab.com 设备码登录另存的刷新令牌条目：只在迁移时删掉它。
fn legacy_refresh_vault(account: &Account) -> KeyVault {
    KeyVault { target: format!("Meowo/{}#refresh", account.key()), ..vault_for(account) }
}

/// 凭据库的入口（测试换成内存里的）。
pub(crate) type Vaults<'a> = &'a dyn Fn(&Account) -> Box<dyn Vault>;

fn system_vaults(account: &Account) -> Box<dyn Vault> {
    Box::new(vault_for(account))
}

/// 迁移：旧清单里设备码（OAuth）登录的 GitLab 账号 → 标「要重新添加」（令牌），删掉它的访问令牌与刷新令牌两条凭据。
/// legacy_refresh：刷新令牌那一条的入口（测试换成内存里的）。返回 true = 清单改了。删凭据失败不挡迁移（条目留着也
/// 没人再读），下次读清单不会重复处理（标记已经去掉）。
pub(crate) fn migrate_oauth(path: &Path, vaults: Vaults, legacy_refresh: Vaults) -> bool {
    let mut list = load_list(path);
    if !list.iter().any(|a| a.legacy_oauth) {
        return false;
    }
    for account in list.iter_mut().filter(|a| a.legacy_oauth) {
        let _ = vaults(account).delete();
        let _ = legacy_refresh(account).delete();
        account.legacy_oauth = false;
        account.needs_relogin = true;
    }
    save_list(path, &list).is_ok()
}

// ═══ 平台接口的组装 ═══

/// 平台 API 在哪台主机上（Bitbucket Cloud 的 API 在 api.bitbucket.org）。
pub(crate) fn api_host(platform: Platform, host: &str) -> String {
    match platform {
        Platform::Bitbucket => bitbucket::API_HOST.into(),
        Platform::Gitee => crate::forge::gitee::HOST.into(),
        _ => host.to_string(),
    }
}

/// 令牌 → 认证头：GitLab / 企业版 Bearer；Gitea `token`；Bitbucket 有邮箱用 Basic，没有用 Bearer。
pub(crate) fn auth_for(platform: Platform, token: Token, auth_user: Option<&str>) -> Auth {
    match (platform, auth_user.filter(|u| !u.is_empty())) {
        // Gitee 文档写 access_token 查询参数；实测也认 `Authorization: token`，用请求头，令牌不进地址。
        (Platform::Gitea | Platform::Gitee, _) => Auth::Token(token),
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
        Platform::Gitee => Box::new(crate::forge::gitee::Gitee { client, state, login }),
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
        Platform::Gitee => crate::forge::gitee::current_user(&client),
        Platform::Github => unreachable!(),
    }
}

// ═══ 添加 / 退出 / 令牌好坏（可注入，单测覆盖）═══

/// 校验平台与主机：公共主机的平台是固定的（github.com 只能是 GitHub）；Bitbucket 只有 Cloud。
pub(crate) fn check_target(platform: &str, host: &str) -> Result<(Platform, String), String> {
    let platform = Platform::parse(platform).ok_or("forge/invalid_platform")?;
    let host = platform::normalize_host_input(host).ok_or("forge/invalid_host")?;
    let name = platform::host_name(&host);
    if name == platform::GITHUB_COM && platform != Platform::Github {
        return Err("forge/platform_mismatch: github".into());
    }
    if platform == Platform::Bitbucket && name != "bitbucket.org" {
        return Err("forge/bitbucket_cloud_only".into());
    }
    if platform == Platform::Gitee && name != "gitee.com" {
        return Err("forge/gitee_com_only".into());
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
        let _ = vaults(old).delete();
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
        needs_relogin: false,
        legacy_oauth: false,
    };
    vaults(&account).write(&token)?;
    put_account(path, vaults, account.clone())?;
    Ok(account)
}

/// 退出：删凭据库里的令牌与清单里这一条。没有这一条也算成功。
pub(crate) fn remove_with(path: &Path, vaults: Vaults, platform: &str, host: &str) -> Result<(), String> {
    let mut list = load_list(path);
    let host = platform::normalize_host_input(host).unwrap_or_else(|| host.to_string());
    let (gone, keep): (Vec<Account>, Vec<Account>) = list.drain(..).partition(|a| a.platform == platform && a.host.eq_ignore_ascii_case(&host));
    for account in &gone {
        vaults(account).delete()?;
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
    cache
        .list
        .get_or_insert_with(|| {
            let path = list_path();
            migrate_oauth(&path, &system_vaults, &|a| Box::new(legacy_refresh_vault(a)));
            load_list(&path)
        })
        .clone()
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
    let token = match vault_for(account).read() {
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

/// 生产的主机 → 平台 / 账号。
pub(crate) struct Accounts;

impl Forges for Accounts {
    fn platform(&self, host: &str) -> Option<Platform> {
        platform::public_platform(host).or_else(|| account_for(&list(), host).and_then(Account::platform))
    }

    fn usable(&self, host: &str) -> bool {
        if platform::host_name(host) == platform::GITHUB_COM {
            return crate::github::token().is_some();
        }
        account_for(&list(), host).is_some()
    }

    fn host_of(&self, host: &str) -> String {
        match account_for(&list(), host) {
            Some(account) if platform::public_platform(host).is_none() => platform::host_key(&account.host),
            _ => platform::host_key(host),
        }
    }

    fn open(&self, host: &str) -> Option<Box<dyn ForgeApi + '_>> {
        if platform::host_name(host) == platform::GITHUB_COM {
            return github_com();
        }
        let account = account_for(&list(), host)?.clone();
        let platform = account.platform()?;
        // 公共主机的平台是固定的：清单里写错了平台（手改文件）就不用它。
        if platform::public_platform(host).is_some_and(|p| p != platform) {
            return None;
        }
        let token = token_of(&account);
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

/// github.com 的平台接口：github.rs 的账号（没有就 None：不发请求，界面引导去设置添加）+ 内置实现。
fn github_com() -> Option<Box<dyn ForgeApi + 'static>> {
    let token = crate::github::token()?;
    let state = if crate::github::record().needs_relogin { GhState::Unauthed } else { GhState::Ok };
    let used = token.clone();
    Some(Box::new(crate::forge::builtin::forge(
        Box::new(crate::github::RealHttp),
        crate::forge::builtin::Endpoints::github_com(),
        Some(token),
        state,
        crate::db_path().with_file_name("forge.json"),
        Box::new(move |ok| crate::github::auth_result(&used, ok)),
    )))
}

/// 设置页列出的账号：github.com（github.rs 的记录）在前，其余按清单。has_token：凭据库里有 github.com 的令牌
///（没有就不显示旧记录里的账号，视为没添加）。
pub(crate) fn listed(github: &crate::github::AccountRecord, has_token: bool, list: &[Account]) -> Vec<ForgeAccountDto> {
    let mut out: Vec<ForgeAccountDto> = github
        .account
        .iter()
        .filter(|_| has_token)
        .map(|a| ForgeAccountDto {
            platform: "github".into(),
            host: platform::GITHUB_COM.into(),
            login: a.login.clone(),
            name: a.name.clone(),
            needs_relogin: github.needs_relogin,
            oauth: !github.via_token,
        })
        .collect();
    out.extend(list.iter().filter(|a| platform::host_name(&a.host) != platform::GITHUB_COM).map(Account::dto));
    out
}

fn is_github_com(platform: &str, host: &str) -> bool {
    platform == "github" && platform::normalize_host_input(host).is_some_and(|h| platform::host_name(&h) == platform::GITHUB_COM)
}

// ═══ 设备码登录（github.com）═══

fn device_dto(code: meowo_protocol::ipc::GithubDeviceCodeDto) -> ForgeDeviceCodeDto {
    ForgeDeviceCodeDto {
        platform: "github".into(),
        host: platform::GITHUB_COM.into(),
        user_code: code.user_code,
        verification_uri: code.verification_uri.clone(),
        verification_uri_complete: None,
        expires_at: code.expires_at,
    }
}

fn status_now() -> ForgeAccountsDto {
    ForgeAccountsDto {
        accounts: listed(&crate::github::record(), crate::github::token().is_some(), &list()),
        credential_supported: crate::github::SystemVault.supported(),
        pending: crate::github::pending().map(device_dto),
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

/// 用个人访问令牌添加账号。user：Bitbucket 的 Atlassian 邮箱（可空）。github.com 的令牌存进 github.rs 的账号。
#[tauri::command]
pub(crate) async fn forge_account_add(platform: String, host: String, token: String, user: Option<String>) -> Result<ForgeAccountsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !crate::github::SystemVault.supported() {
            return Err("github/credential_unsupported".to_string());
        }
        let (parsed, normalized) = check_target(&platform, &host)?;
        if is_github_com(&platform, &host) {
            crate::github::add_token(&Token::secret(&token).ok_or("forge/invalid_token")?)?;
            return Ok(status_now());
        }
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
        if is_github_com(&platform, &host) {
            crate::github::logout()?;
            return Ok(status_now());
        }
        remove_with(&list_path(), &system_vaults, &platform, &host)?;
        changed();
        Ok(status_now())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// github.com 设备码登录：要代码、立刻返回给设置页显示；后台线程轮询，结果经 [`LOGIN_EVENT`] 推给前端。
/// 再点一次 = 作废上一轮、重新要代码。
#[tauri::command]
pub(crate) async fn forge_login_start() -> Result<ForgeDeviceCodeDto, String> {
    tauri::async_runtime::spawn_blocking(|| crate::github::start_login(emit_login).map(device_dto)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn forge_login_cancel() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(crate::github::cancel_login).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
