//! 代码托管账号：添加 / 退出 / 令牌好坏 / 设备码 / 按主机选账号。凭据库与 HTTP 都是假的，不碰真凭据库、不联网。

use super::*;
use crate::forge::http::Req;
use crate::github::Response;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 内存里的凭据库：键是条目名。
#[derive(Default)]
struct MemVault {
    items: std::sync::Arc<Mutex<HashMap<String, String>>>,
}

struct MemEntry {
    target: String,
    items: std::sync::Arc<Mutex<HashMap<String, String>>>,
}

impl Vault for MemEntry {
    fn supported(&self) -> bool {
        true
    }
    fn read(&self) -> Result<Option<Token>, String> {
        Ok(self.items.lock().unwrap().get(&self.target).and_then(|t| Token::secret(t)))
    }
    fn write(&self, token: &Token) -> Result<(), String> {
        self.items.lock().unwrap().insert(self.target.clone(), token.expose().to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        self.items.lock().unwrap().remove(&self.target);
        Ok(())
    }
}

impl MemVault {
    fn open(&self) -> impl Fn(&Account, bool) -> Box<dyn Vault> + '_ {
        move |account: &Account, refresh: bool| {
            Box::new(MemEntry {
                target: vault_for(account, refresh).target,
                items: self.items.clone(),
            }) as Box<dyn Vault>
        }
    }
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.items.lock().unwrap().keys().cloned().collect();
        keys.sort();
        keys
    }
}

type Handler = dyn Fn(&str, &str, Option<&Body>, Option<String>) -> Result<Response, String> + Sync;

struct FakeHttp {
    handler: Box<Handler>,
    calls: Mutex<Vec<(String, String, Option<String>)>>,
}

impl FakeHttp {
    fn new(handler: impl Fn(&str, &str, Option<&Body>, Option<String>) -> Result<Response, String> + Sync + 'static) -> Self {
        FakeHttp {
            handler: Box::new(handler),
            calls: Mutex::new(Vec::new()),
        }
    }
}

impl ForgeHttp for FakeHttp {
    fn send(&self, req: &Req) -> Result<Response, String> {
        let auth = req.auth.map(Auth::header);
        self.calls
            .lock()
            .unwrap()
            .push((req.method.to_string(), req.url.to_string(), auth.clone()));
        (self.handler)(req.method, req.url, req.body.as_ref(), auth)
    }
}

fn resp(status: u16, body: serde_json::Value) -> Result<Response, String> {
    Ok(Response {
        status,
        body: body.to_string(),
        ..Default::default()
    })
}

fn sandbox(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("meowo-forge-accounts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("forge-accounts.json")
}

#[test]
fn 平台与主机校验() {
    assert_eq!(check_target("gitlab", "https://gitlab.com/"), Ok((Platform::Gitlab, "gitlab.com".into())));
    assert_eq!(
        check_target("gitlab", "git.corp.example:8443"),
        Ok((Platform::Gitlab, "git.corp.example:8443".into()))
    );
    assert_eq!(check_target("forgejo", "codeberg.org"), Ok((Platform::Gitea, "codeberg.org".into())));
    assert_eq!(
        check_target("github", "github.com"),
        Err("forge/use_github_login".into()),
        "github.com 走 GitHub 连接方式"
    );
    assert_eq!(
        check_target("github", "ghe.corp.example"),
        Ok((Platform::Github, "ghe.corp.example".into()))
    );
    assert_eq!(check_target("gitea", "gitlab.com"), Err("forge/platform_mismatch: gitlab".into()));
    assert_eq!(
        check_target("bitbucket", "bitbucket.corp.example"),
        Err("forge/bitbucket_cloud_only".into())
    );
    assert_eq!(check_target("svn", "x.example"), Err("forge/invalid_platform".into()));
    assert_eq!(check_target("gitlab", "not a host"), Err("forge/invalid_host".into()));
}

#[test]
fn 添加_先验令牌_令牌只进凭据库_清单不含令牌_同主机替换() {
    let path = sandbox("add");
    let vault = MemVault::default();
    let vaults = vault.open();
    let http = FakeHttp::new(|_, url, _, auth| match (url, auth.as_deref()) {
        ("https://gitlab.com/api/v4/user", Some("Bearer glpat-GOOD")) => resp(200, json!({ "username": "alice", "name": "Alice" })),
        ("https://gitlab.com/api/v4/user", Some("Bearer glpat-OTHER")) => resp(200, json!({ "username": "bob", "name": null })),
        ("https://gitlab.com/api/v4/user", _) => resp(401, json!({ "message": "401 Unauthorized" })),
        ("https://codeberg.org/api/v1/user", Some("token c0ffee")) => resp(200, json!({ "login": "carol", "full_name": "Carol" })),
        ("https://api.bitbucket.org/2.0/user", Some(a)) if a.starts_with("Basic ") => {
            resp(200, json!({ "nickname": "dave", "display_name": "Dave" }))
        }
        _ => resp(404, json!({})),
    });
    // 令牌不对：不落库。
    assert_eq!(
        add_with(&path, &vaults, &http, "gitlab", "gitlab.com", "glpat-BAD", None),
        Err("forge/token_rejected".into())
    );
    assert!(vault.keys().is_empty() && load_list(&path).is_empty());
    let a = add_with(&path, &vaults, &http, "gitlab", "gitlab.com", " glpat-GOOD ", None).unwrap();
    assert_eq!((a.login.as_str(), a.name.as_deref(), a.oauth), ("alice", Some("Alice"), false));
    add_with(&path, &vaults, &http, "gitea", "codeberg.org", "c0ffee", None).unwrap();
    add_with(
        &path,
        &vaults,
        &http,
        "bitbucket",
        "bitbucket.org",
        "ATATT3xFf=AB",
        Some("dave@example.com"),
    )
    .unwrap();
    assert_eq!(
        vault.keys(),
        [
            "Meowo/bitbucket/bitbucket.org/dave",
            "Meowo/gitea/codeberg.org/carol",
            "Meowo/gitlab/gitlab.com/alice"
        ]
    );
    let raw = std::fs::read_to_string(&path).unwrap();
    for secret in ["glpat-GOOD", "c0ffee", "ATATT3xFf"] {
        assert!(!raw.contains(secret), "清单文件里不能有令牌：{raw}");
    }
    assert!(raw.contains("dave@example.com"), "Bitbucket 记下 Basic 认证用的邮箱");
    // 同一台主机再添加（换了人）：旧的令牌从凭据库删掉，清单里替换。
    add_with(&path, &vaults, &http, "gitlab", "gitlab.com", "glpat-OTHER", None).unwrap();
    let list = load_list(&path);
    assert_eq!(
        list.iter()
            .filter(|a| a.host == "gitlab.com")
            .map(|a| a.login.as_str())
            .collect::<Vec<_>>(),
        ["bob"]
    );
    assert!(!vault.keys().iter().any(|k| k.ends_with("/alice")));
    // 退出：删凭据（含刷新令牌那一条）与清单；重复退出也成功。
    remove_with(&path, &vaults, "gitea", "codeberg.org").unwrap();
    remove_with(&path, &vaults, "gitea", "codeberg.org").unwrap();
    assert!(!vault.keys().iter().any(|k| k.contains("codeberg")));
    assert_eq!(load_list(&path).len(), 2);
}

#[test]
fn 令牌好坏_只认当前令牌_401_标重登_成功清掉() {
    let path = sandbox("auth");
    let account = Account {
        platform: "gitlab".into(),
        host: "gitlab.com".into(),
        login: "alice".into(),
        ..Default::default()
    };
    save_list(&path, std::slice::from_ref(&account)).unwrap();
    let (current, old) = (Token::secret("glpat-NEW").unwrap(), Token::secret("glpat-OLD").unwrap());
    assert!(
        !auth_result_with(&path, &account.key(), &old, Some(&current), false),
        "旧令牌晚到的 401 不算"
    );
    assert!(auth_result_with(&path, &account.key(), &current, Some(&current), false));
    assert!(load_list(&path)[0].needs_relogin);
    assert!(!auth_result_with(&path, &account.key(), &current, Some(&current), false), "已经标过");
    assert!(auth_result_with(&path, &account.key(), &current, Some(&current), true));
    assert!(!load_list(&path)[0].needs_relogin);
}

/// 按主机选账号：公共主机自动认平台；自建主机看账号声明（带端口的账号也认 ssh 远端的主机名）。
struct ListForges<'h> {
    list: Vec<Account>,
    http: &'h FakeHttp,
}

impl Forges for ListForges<'_> {
    fn platform(&self, host: &str) -> Option<Platform> {
        platform::public_platform(host).or_else(|| account_for(&self.list, host).and_then(Account::platform))
    }
    fn usable(&self, host: &str) -> bool {
        platform::host_name(host) == platform::GITHUB_COM || account_for(&self.list, host).is_some()
    }
    fn host_of(&self, host: &str) -> String {
        match account_for(&self.list, host) {
            Some(account) if platform::public_platform(host).is_none() => platform::host_key(&account.host),
            _ => platform::host_key(host),
        }
    }
    fn open(&self, host: &str) -> Option<Box<dyn ForgeApi + '_>> {
        let account = account_for(&self.list, host)?;
        build(
            account,
            Token::secret("tok"),
            Box::new(self.http),
            std::env::temp_dir().join("meowo-forge-test.json"),
            Box::new(|_| {}),
        )
    }
}

#[test]
fn 按主机选账号_自建要声明_没账号的公共主机认平台不开接口() {
    let http = FakeHttp::new(|_, _, _, _| resp(404, json!({})));
    let forges = ListForges {
        list: vec![
            Account {
                platform: "gitlab".into(),
                host: "git.corp.example:8443".into(),
                login: "me".into(),
                ..Default::default()
            },
            Account {
                platform: "github".into(),
                host: "ghe.corp.example".into(),
                login: "me".into(),
                needs_relogin: true,
                ..Default::default()
            },
        ],
        http: &http,
    };
    assert_eq!(
        platform::recognize(&forges, "ssh://git@git.corp.example:2222/team/app.git").map(|r| r.2),
        Some(Platform::Gitlab)
    );
    assert_eq!(
        platform::recognize(&forges, "git@ghe.corp.example:team/app.git").map(|r| r.2),
        Some(Platform::Github)
    );
    assert_eq!(platform::recognize(&forges, "https://gitea.other.example/o/r").map(|r| r.2), None);
    assert_eq!(
        platform::recognize(&forges, "https://codeberg.org/o/r").map(|r| r.2),
        Some(Platform::Gitea)
    );
    assert!(forges.open("codeberg.org").is_none(), "没账号：不开接口（不发请求）");
    let gl = forges.open("git.corp.example").unwrap();
    assert_eq!((gl.platform(), gl.state(false)), (Platform::Gitlab, crate::forge::GhState::Ok));
    let ghe = forges.open("ghe.corp.example").unwrap();
    assert_eq!(
        (ghe.platform(), ghe.state(false)),
        (Platform::Github, crate::forge::GhState::Unauthed),
        "要重新登录的账号"
    );
    assert!(http.calls.lock().unwrap().is_empty());
}

#[test]
fn 认证头_各平台() {
    let t = Token::secret("abc").unwrap();
    assert_eq!(auth_for(Platform::Gitlab, t.clone(), None).header(), "Bearer abc");
    assert_eq!(auth_for(Platform::Gitea, t.clone(), None).header(), "token abc");
    assert_eq!(auth_for(Platform::Github, t.clone(), None).header(), "Bearer abc");
    assert!(auth_for(Platform::Bitbucket, t.clone(), Some("me@example.com"))
        .header()
        .starts_with("Basic "));
    assert_eq!(auth_for(Platform::Bitbucket, t, None).header(), "Bearer abc", "没填邮箱用 Bearer");
    assert_eq!(api_host(Platform::Bitbucket, "bitbucket.org"), "api.bitbucket.org");
}

// ═══ GitLab 设备码 ═══

fn code() -> DeviceCode {
    DeviceCode {
        device_code: "dev".into(),
        user_code: "ABCD-1234".into(),
        verification_uri: "https://gitlab.com/oauth/device".into(),
        verification_uri_complete: None,
        expires_in: 300,
        interval: 5,
    }
}

#[test]
fn 设备码_要代码_字段齐全_出错说清楚() {
    // docs.gitlab.com/api/oauth2 的响应示例。
    let http = FakeHttp::new(|_, url, body, _| {
        assert_eq!(url, "https://gitlab.com/oauth/authorize_device");
        match body {
            Some(Body::Form(pairs)) => {
                assert!(pairs.contains(&("client_id", "cid".into())) && pairs.contains(&("scope", "api".into())));
            }
            _ => panic!("要表单"),
        }
        resp(
            200,
            json!({ "device_code": "GmRhm", "user_code": "0A44L90H", "verification_uri": "https://gitlab.example.com/oauth/device", "verification_uri_complete": "https://gitlab.example.com/oauth/device?user_code=0A44L90H", "expires_in": 300, "interval": 5 }),
        )
    });
    let code = request_code(&http, "gitlab.com", "cid").unwrap();
    assert_eq!((code.user_code.as_str(), code.expires_in, code.interval), ("0A44L90H", 300, 5));
    assert_eq!(
        code.verification_uri_complete.as_deref(),
        Some("https://gitlab.example.com/oauth/device?user_code=0A44L90H")
    );
    assert!(!format!("{code:?}").contains("GmRhm"), "设备码不进日志");
    let bad = FakeHttp::new(|_, _, _, _| {
        resp(
            401,
            json!({ "error": "invalid_client", "error_description": "Client authentication failed" }),
        )
    });
    assert_eq!(request_code(&bad, "gitlab.com", "cid").unwrap_err(), "forge/login_failed: invalid_client");
}

#[test]
fn 设备码_轮询_等待_放慢_拿到令牌() {
    let n = AtomicUsize::new(0);
    let http = FakeHttp::new(move |_, url, _, _| {
        assert_eq!(url, "https://gitlab.com/oauth/token");
        match n.fetch_add(1, Ordering::SeqCst) {
            0 => resp(400, json!({ "error": "authorization_pending" })),
            1 => resp(400, json!({ "error": "slow_down" })),
            2 => Err("tls handshake eof".into()),
            _ => resp(
                200,
                json!({ "access_token": "ACC", "refresh_token": "REF", "expires_in": 7200, "token_type": "Bearer" }),
            ),
        }
    });
    let slept = Mutex::new(Duration::ZERO);
    let outcome = wait_for_token(&http, "gitlab.com", "cid", &code(), &|| false, &|d| *slept.lock().unwrap() += d);
    let Outcome::Tokens(tokens) = outcome else { panic!("{outcome:?}") };
    assert_eq!(
        (tokens.access.expose(), tokens.refresh.as_ref().map(Token::expose), tokens.expires_in),
        ("ACC", Some("REF"), Some(7200))
    );
    // 5 + 5 + 10（slow_down 后 +5）+ 10。
    assert_eq!(*slept.lock().unwrap(), Duration::from_secs(30));
}

#[test]
fn 设备码_过期_拒绝_取消_连不上三次() {
    for (error, want) in [("expired_token", "Expired"), ("access_denied", "Denied")] {
        let http = FakeHttp::new(move |_, _, _, _| resp(400, json!({ "error": error })));
        let outcome = wait_for_token(&http, "gitlab.com", "cid", &code(), &|| false, &|_| {});
        assert!(format!("{outcome:?}").starts_with(want), "{outcome:?}");
    }
    let http = FakeHttp::new(|_, _, _, _| resp(400, json!({ "error": "authorization_pending" })));
    let outcome = wait_for_token(&http, "gitlab.com", "cid", &code(), &|| false, &|_| {});
    assert!(matches!(outcome, Outcome::Expired), "等满 expires_in 自己判过期");
    let calls = AtomicUsize::new(0);
    let cancelled = || calls.fetch_add(1, Ordering::SeqCst) > 3;
    assert!(matches!(
        wait_for_token(&http, "gitlab.com", "cid", &code(), &cancelled, &|_| {}),
        Outcome::Cancelled
    ));
    let down = FakeHttp::new(|_, _, _, _| Err("connection refused".into()));
    let outcome = wait_for_token(&down, "gitlab.com", "cid", &code(), &|| false, &|_| {});
    assert!(matches!(&outcome, Outcome::Failed(e) if e.starts_with("forge/network")), "{outcome:?}");
    assert_eq!(down.calls.lock().unwrap().len(), 3);
}

#[test]
fn 设备码令牌落库_刷新令牌另存_刷新换新的一对() {
    let path = sandbox("oauth");
    let vault = MemVault::default();
    let vaults = vault.open();
    let tokens = OauthTokens {
        access: Token::secret("ACC1").unwrap(),
        refresh: Token::secret("REF1"),
        expires_in: Some(7200),
    };
    let account = Account {
        platform: "gitlab".into(),
        host: "gitlab.com".into(),
        login: "alice".into(),
        ..Default::default()
    };
    let stored = store_oauth(&path, &vaults, account, &tokens, 1_000).unwrap();
    assert_eq!((stored.oauth, stored.expires_at), (true, Some(1_000 + 7_200_000)));
    assert_eq!(vault.keys(), ["Meowo/gitlab/gitlab.com/alice", "Meowo/gitlab/gitlab.com/alice#refresh"]);
    let http = FakeHttp::new(|_, url, body, _| {
        assert_eq!(url, "https://gitlab.com/oauth/token");
        let Some(Body::Form(pairs)) = body else { panic!() };
        assert!(pairs.contains(&("grant_type", "refresh_token".into())) && pairs.contains(&("refresh_token", "REF1".into())));
        assert!(!pairs.iter().any(|(k, _)| *k == "client_secret"), "桌面端不带 client secret");
        resp(200, json!({ "access_token": "ACC2", "refresh_token": "REF2", "expires_in": 7200 }))
    });
    let next = oauth_refresh(&http, "gitlab.com", "cid", &Token::secret("REF1").unwrap()).unwrap();
    assert_eq!(next.access.expose(), "ACC2");
    let rejected = FakeHttp::new(|_, _, _, _| resp(400, json!({ "error": "invalid_grant" })));
    assert_eq!(
        oauth_refresh(&rejected, "gitlab.com", "cid", &Token::secret("REF1").unwrap())
            .err()
            .as_deref(),
        Some("forge/login_failed: invalid_grant")
    );
}

#[test]
fn 没注册_gitlab_应用时设备码不可用() {
    // 默认构建没有注入 MEOWO_GITLAB_CLIENT_ID：设置页只给令牌入口，代码里也没有 client secret。
    if option_env!("MEOWO_GITLAB_CLIENT_ID").is_none_or(str::is_empty) {
        assert!(GITLAB_CLIENT_ID.is_empty());
    }
    let source = include_str!("../forge_accounts.rs");
    assert!(!source.contains(&["client", "_secret="].concat()));
}

/// forge 上层接到 GitLab：「更改」面板的状态用 MR 与流水线，一个 gh 调用都不发。
#[test]
fn 更改面板_gitlab_仓库走平台接口_不碰_gh() {
    let dir = std::env::temp_dir().join(format!("meowo-forge-gl-status-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .current_dir(&dir)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@example.com",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);
    git(&["remote", "add", "origin", "git@gitlab.com:gitlab-org/cli.git"]);
    let head = git(&["rev-parse", "HEAD"]);
    git(&["update-ref", "refs/remotes/origin/main", &head]);
    git(&["switch", "-q", "-c", "feat/login-detect-ssh-host"]);
    let mut mr = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/forge/fixtures/platforms/gitlab_mr_4365.json")).unwrap(),
    )
    .unwrap();
    mr["source_project_id"] = mr["target_project_id"].clone();
    let http = FakeHttp::new(move |_, url, _, _| match url {
        "https://gitlab.com/api/v4/projects/gitlab-org%2Fcli" => {
            resp(200, json!({ "id": 34675721, "squash_option": "default_off", "merge_method": "merge" }))
        }
        u if u.contains("/merge_requests?source_branch=feat%2Flogin-detect-ssh-host") => resp(200, json!([mr.clone()])),
        u if u.ends_with("/merge_requests/4365") => resp(200, mr.clone()),
        u if u.contains("/jobs") => resp(
            200,
            json!([{ "name": "test", "stage": "test", "status": "running", "web_url": "https://gitlab.com/x/-/jobs/1", "allow_failure": false }]),
        ),
        _ => resp(404, json!({})),
    });
    let forges = ListForges {
        list: vec![Account {
            platform: "gitlab".into(),
            host: "gitlab.com".into(),
            login: "me".into(),
            ..Default::default()
        }],
        http: &http,
    };
    let gh_calls = AtomicUsize::new(0);
    let gh = |_: &str, _: &[&str]| {
        gh_calls.fetch_add(1, Ordering::SeqCst);
        None
    };
    let caches = crate::forge::Caches::default();
    let ctx = crate::forge::Ctx {
        forges: &forges,
        gh: &gh,
        caches: &caches,
        prefs: dir.join("forge.json"),
        env: &[("GIT_CONFIG_NOSYSTEM", Some("1"))],
    };
    let dto = crate::forge::forge_status_with(&ctx, &dir.to_string_lossy(), false).unwrap();
    assert_eq!(
        (dto.gh.as_str(), dto.forge.as_deref(), dto.forge_host.as_deref()),
        ("ok", Some("gitlab"), Some("gitlab.com"))
    );
    assert_eq!(dto.merge_methods, vec!["merge", "squash"]);
    let pr = dto.pr.unwrap();
    assert_eq!(
        (pr.number, pr.forge.as_deref(), pr.checks_summary.as_deref()),
        (4365, Some("gitlab"), Some("pending"))
    );
    assert_eq!(gh_calls.load(Ordering::SeqCst), 0, "GitLab 仓库不碰 gh");
    // 合并：流水线还在跑，上层直接拦下，不发合并请求。
    let err = crate::forge::pr_merge_with(&ctx, &dir.to_string_lossy(), 4365, "merge", pr.head_oid.as_deref().unwrap()).unwrap_err();
    assert_eq!(err, "gh/checks_pending");
    assert!(!http.calls.lock().unwrap().iter().any(|(m, _, _)| m == "PUT"));
    let _ = std::fs::remove_dir_all(&dir);
}

use serde_json::json;

/// 审查指出：自建实例的端口在远端解析里丢了，网页链接与 API 地址没端口；账号填 host:443 时比较对不上。
#[test]
fn 自建实例的端口_ssh_与_https_远端都按账号的主机走() {
    let dir = std::env::temp_dir().join(format!("meowo-forge-port-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").current_dir(&dir).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "--allow-empty", "-m", "init"]);
    git(&["switch", "-q", "-c", "feat"]);
    let http = FakeHttp::new(|_, url, _, _| match url {
        u if u.contains("/merge_requests?") => resp(200, json!([])),
        u if u.ends_with("/projects/g%2Fapp") => resp(200, json!({ "id": 1, "squash_option": "default_off" })),
        _ => resp(404, json!({})),
    });
    let gh = |_: &str, _: &[&str]| None;
    for (account_host, remote, want_host) in [
        ("git.corp.example:8443", "ssh://git@git.corp.example:2222/g/app.git", "git.corp.example:8443"),
        ("git.corp.example:8443", "https://git.corp.example:8443/g/app.git", "git.corp.example:8443"),
        ("git.corp.example:443", "https://git.corp.example/g/app.git", "git.corp.example"),
    ] {
        let _ = std::process::Command::new("git").current_dir(&dir).args(["remote", "remove", "origin"]).output();
        git(&["remote", "add", "origin", remote]);
        let account = Account { platform: "gitlab".into(), host: platform::normalize_host_input(account_host).unwrap(), login: "me".into(), ..Default::default() };
        let forges = ListForges { list: vec![account], http: &http };
        http.calls.lock().unwrap().clear();
        let caches = crate::forge::Caches::default();
        let ctx = crate::forge::Ctx { forges: &forges, gh: &gh, caches: &caches, prefs: dir.join("forge.json"), env: &[("GIT_CONFIG_NOSYSTEM", Some("1"))] };
        let dto = crate::forge::forge_status_with(&ctx, &dir.to_string_lossy(), false).unwrap();
        assert_eq!((dto.gh.as_str(), dto.forge_host.as_deref()), ("ok", Some(want_host)), "{remote}");
        assert_eq!(dto.repo_url, Some(format!("https://{want_host}/g/app")), "{remote}");
        assert!(dto.compare_url.as_deref().unwrap_or_default().starts_with(&format!("https://{want_host}/g/app/-/merge_requests/new")), "{remote}");
        let calls = http.calls.lock().unwrap().clone();
        assert!(!calls.is_empty() && calls.iter().all(|(_, url, _)| url.starts_with(&format!("https://{want_host}/api/v4/"))), "{remote}: {calls:?}");
        // 真实 HTTP 的白名单也认这台主机（含默认端口写法）。
        assert!(crate::forge::http::allowed(&api_host(Platform::Gitlab, &forges.list[0].host), &format!("https://{want_host}/api/v4/user")));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 审查指出：侧栏按目录并行查 PR，几个线程同时发现 GitLab 令牌快过期会各自续期；GitLab 续期后旧刷新令牌作废，
/// 后到的线程拿旧刷新令牌被拒，账号被误标「要重新登录」。现在每个账号一把锁、拿锁后重读，只续一次。
#[test]
fn 设备码令牌并发续期_只续一次_不误标重登() {
    let path = sandbox("refresh-race");
    let vault = MemVault::default();
    let vaults = vault.open();
    let account = Account { platform: "gitlab".into(), host: "gitlab.com".into(), login: "alice".into(), ..Default::default() };
    let tokens = OauthTokens { access: Token::secret("ACC1").unwrap(), refresh: Token::secret("REF1"), expires_in: Some(30) };
    let stored = store_oauth(&path, &vaults, account, &tokens, 0).unwrap();
    let calls = AtomicUsize::new(0);
    let http = FakeHttp::new(move |_, _, body, _| {
        let Some(Body::Form(pairs)) = body else { panic!() };
        let used = pairs.iter().find(|(k, _)| *k == "refresh_token").map(|(_, v)| v.clone()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        if used == "REF1" {
            resp(200, json!({ "access_token": "ACC2", "refresh_token": "REF2", "expires_in": 7200 }))
        } else {
            resp(400, json!({ "error": "invalid_grant" }))
        }
    });
    let results: Vec<Refresh> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    refresh_with(&path, &vaults, &http, "cid", &stored.key(), 1_000)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(http.calls.lock().unwrap().len(), 1, "只续一次");
    let acc2 = Token::secret("ACC2").unwrap();
    assert!(results.iter().all(|r| *r == Refresh::Renewed(acc2.clone()) || *r == Refresh::Current(Some(acc2.clone()))), "{results:?}");
    assert!(!load_list(&path)[0].needs_relogin);
    // 连不上不算被拒：不标重登，下次再续。
    let path2 = sandbox("refresh-down");
    let account = Account { platform: "gitlab".into(), host: "gitlab.com".into(), login: "bob".into(), ..Default::default() };
    let stored = store_oauth(&path2, &vaults, account, &tokens, 0).unwrap();
    let down = FakeHttp::new(|_, _, _, _| Err("connection refused".into()));
    assert_eq!(refresh_with(&path2, &vaults, &down, "cid", &stored.key(), 1_000), Refresh::Current(Some(Token::secret("ACC1").unwrap())));
    assert!(!load_list(&path2)[0].needs_relogin);
    // 刷新令牌真的被拒：标重登。
    let rejected = FakeHttp::new(|_, _, _, _| resp(400, json!({ "error": "invalid_grant" })));
    assert_eq!(refresh_with(&path2, &vaults, &rejected, "cid", &stored.key(), 1_000), Refresh::Rejected);
    assert!(load_list(&path2)[0].needs_relogin);
}

/// 审查指出：401 一次就永久标重登。现在 401 先用同一令牌问一次「我是谁」，确认被拒才标；已标过的不再回查；
/// 好用的回包只在正标着重登时才改清单。
#[test]
fn 令牌被拒要回查确认才标重登() {
    let path = sandbox("confirm");
    let account = Account { platform: "gitlab".into(), host: "gitlab.com".into(), login: "alice".into(), ..Default::default() };
    save_list(&path, std::slice::from_ref(&account)).unwrap();
    let token = Token::secret("glpat-A").unwrap();
    let asked = AtomicUsize::new(0);
    let fine = || {
        asked.fetch_add(1, Ordering::SeqCst);
        false
    };
    let rejected = || {
        asked.fetch_add(1, Ordering::SeqCst);
        true
    };
    // 没权限的 401（GitLab 合并）：回查 /user 还好用，不标。
    assert!(!report_with(&path, &account.key(), &token, Some(&token), false, false, &fine));
    assert!(!load_list(&path)[0].needs_relogin);
    // 回查也被拒：标。
    assert!(report_with(&path, &account.key(), &token, Some(&token), false, false, &rejected));
    assert!(load_list(&path)[0].needs_relogin);
    // 已经标过：不再回查。
    let before = asked.load(Ordering::SeqCst);
    assert!(!report_with(&path, &account.key(), &token, Some(&token), false, true, &rejected));
    assert_eq!(asked.load(Ordering::SeqCst), before);
    // 好用的回包：没标时什么都不做，标着时清掉。
    assert!(!report_with(&path, &account.key(), &token, Some(&token), true, false, &rejected));
    assert!(report_with(&path, &account.key(), &token, Some(&token), true, true, &rejected));
    assert!(!load_list(&path)[0].needs_relogin);
}
