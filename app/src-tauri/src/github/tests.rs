//! github.rs 的测试：设备码各分支、模式分派、令牌不进可序列化的结构。HTTP 全部是假的，不打真网络。

use super::device::{poll_once, request_code, wait_for_token, DeviceCode, Outcome, Poll};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 一次请求：方法、地址、表单。
type Call = (String, String, Vec<(String, String)>);

/// 按调用顺序回脚本里的响应；记下每次请求的地址与表单。
struct Script {
    replies: Mutex<Vec<Result<Response, String>>>,
    calls: Mutex<Vec<Call>>,
}

impl Script {
    fn new(replies: Vec<Result<Response, String>>) -> Self {
        Script { replies: Mutex::new(replies.into_iter().rev().collect()), calls: Mutex::new(Vec::new()) }
    }
    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl Http for Script {
    fn send(&self, req: &Request) -> Result<Response, String> {
        let form = match &req.body {
            Some(Body::Form(pairs)) => pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            _ => Vec::new(),
        };
        self.calls.lock().unwrap().push((req.method.to_string(), req.url.to_string(), form));
        self.replies.lock().unwrap().pop().unwrap_or_else(|| Err("script exhausted".into()))
    }
}

fn reply(status: u16, body: &str) -> Result<Response, String> {
    Ok(Response { status, body: body.to_string(), link: None })
}

fn code(expires_in: u64, interval: u64) -> DeviceCode {
    DeviceCode {
        device_code: "dev-secret-123".into(),
        user_code: "ABCD-1234".into(),
        verification_uri: "https://github.com/login/device".into(),
        expires_in,
        interval,
    }
}

/// 假睡眠：只累加，不真等。
struct Clock(Mutex<Duration>);
impl Clock {
    fn new() -> Self {
        Clock(Mutex::new(Duration::ZERO))
    }
    fn sleep(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
    fn total(&self) -> Duration {
        *self.0.lock().unwrap()
    }
}

const PENDING: &str = r#"{"error":"authorization_pending","error_description":"pending"}"#;

#[test]
fn 要代码_带上_client_id_与_scope_并解析回包() {
    let http = Script::new(vec![reply(
        200,
        r#"{"device_code":"dc","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":899,"interval":5}"#,
    )]);
    let got = request_code(&http, "cid").unwrap();
    assert_eq!((got.user_code.as_str(), got.expires_in, got.interval), ("WDJB-MJHT", 899, 5));
    let calls = http.calls();
    assert_eq!(calls[0].1, DEVICE_CODE_URL);
    assert!(calls[0].2.contains(&("client_id".into(), "cid".into())));
    assert!(calls[0].2.contains(&("scope".into(), "repo read:org".into())));
    // device_code 是轮询凭据：Debug 里不能出现。
    assert!(!format!("{got:?}").contains("dc\""));
}

#[test]
fn 要代码_失败分支() {
    let disabled = Script::new(vec![reply(200, r#"{"error":"device_flow_disabled"}"#)]);
    assert_eq!(request_code(&disabled, "cid").unwrap_err(), "github/login_failed: device_flow_disabled");
    let offline = Script::new(vec![Err("dns failed".into())]);
    assert!(request_code(&offline, "cid").unwrap_err().starts_with("github/network"));
    let broken = Script::new(vec![reply(200, r#"{"user_code":"X"}"#)]);
    assert!(request_code(&broken, "cid").unwrap_err().starts_with("github/login_failed"));
}

#[test]
fn 单次轮询_各种回包() {
    let cases = [
        (PENDING, "pending"),
        (r#"{"error":"slow_down","interval":10}"#, "slow"),
        (r#"{"error":"expired_token"}"#, "expired"),
        (r#"{"error":"access_denied"}"#, "denied"),
        (r#"{"error":"incorrect_client_credentials"}"#, "failed"),
        (r#"{"access_token":"gho_abc123","token_type":"bearer","scope":"repo,read:org"}"#, "token"),
    ];
    for (body, want) in cases {
        let http = Script::new(vec![reply(200, body)]);
        let got = match poll_once(&http, "cid", "dc") {
            Poll::Pending => "pending",
            Poll::SlowDown(Some(10)) => "slow",
            Poll::Expired => "expired",
            Poll::Denied => "denied",
            Poll::Failed(_) => "failed",
            Poll::Token(t) if t.expose() == "gho_abc123" => "token",
            other => panic!("{body}: {other:?}"),
        };
        assert_eq!(got, want, "{body}");
        let form = &http.calls()[0].2;
        assert!(form.contains(&("grant_type".into(), "urn:ietf:params:oauth:grant-type:device_code".into())));
        assert!(form.contains(&("device_code".into(), "dc".into())));
    }
}

#[test]
fn 等待_挂起两次后授权成功_按间隔睡() {
    let http = Script::new(vec![reply(200, PENDING), reply(200, PENDING), reply(200, r#"{"access_token":"gho_ok"}"#)]);
    let clock = Clock::new();
    let outcome = wait_for_token(&http, "cid", &code(900, 5), &|| false, &|d| clock.sleep(d));
    assert!(matches!(outcome, Outcome::Token(ref t) if t.expose() == "gho_ok"));
    assert_eq!(clock.total(), Duration::from_secs(15));
    assert_eq!(http.calls().len(), 3);
}

#[test]
fn 等待_slow_down_按回包的新间隔放慢() {
    let http = Script::new(vec![reply(200, r#"{"error":"slow_down","interval":10}"#), reply(200, PENDING), reply(200, r#"{"access_token":"gho_ok"}"#)]);
    let clock = Clock::new();
    let outcome = wait_for_token(&http, "cid", &code(900, 5), &|| false, &|d| clock.sleep(d));
    assert!(matches!(outcome, Outcome::Token(_)));
    // 5（第一次）+ 10 + 10。
    assert_eq!(clock.total(), Duration::from_secs(25));
}

#[test]
fn 等待_slow_down_没给间隔就加五秒() {
    let http = Script::new(vec![reply(200, r#"{"error":"slow_down"}"#), reply(200, r#"{"access_token":"gho_ok"}"#)]);
    let clock = Clock::new();
    wait_for_token(&http, "cid", &code(900, 5), &|| false, &|d| clock.sleep(d));
    assert_eq!(clock.total(), Duration::from_secs(15));
}

#[test]
fn 等待_过期与拒绝() {
    let expired = Script::new(vec![reply(200, PENDING), reply(200, r#"{"error":"expired_token"}"#)]);
    assert!(matches!(wait_for_token(&expired, "cid", &code(900, 5), &|| false, &|_| {}), Outcome::Expired));
    let denied = Script::new(vec![reply(200, r#"{"error":"access_denied"}"#)]);
    assert!(matches!(wait_for_token(&denied, "cid", &code(900, 5), &|| false, &|_| {}), Outcome::Denied));
}

#[test]
fn 等待_到了有效期自己判过期_不再轮询() {
    let http = Script::new(vec![reply(200, PENDING), reply(200, PENDING), reply(200, PENDING)]);
    let outcome = wait_for_token(&http, "cid", &code(10, 5), &|| false, &|_| {});
    assert!(matches!(outcome, Outcome::Expired));
    assert_eq!(http.calls().len(), 2);
}

#[test]
fn 等待_取消后立刻退出_不再发请求() {
    let http = Script::new(vec![reply(200, PENDING), reply(200, r#"{"access_token":"gho_late"}"#)]);
    let naps = AtomicUsize::new(0);
    // 第一轮轮询之后（睡了 20 个 250ms 小段 = 5 秒）取消。
    let cancelled = || naps.load(Ordering::SeqCst) > 20;
    let outcome = wait_for_token(&http, "cid", &code(900, 5), &cancelled, &|_| {
        naps.fetch_add(1, Ordering::SeqCst);
    });
    assert!(matches!(outcome, Outcome::Cancelled));
    assert_eq!(http.calls().len(), 1, "取消后不能再拿令牌");
}

#[test]
fn 等待_连着断网三次才放弃_中间恢复不算() {
    let http = Script::new(vec![Err("reset".into()), reply(200, PENDING), Err("reset".into()), Err("reset".into()), Err("reset".into())]);
    let outcome = wait_for_token(&http, "cid", &code(900, 5), &|| false, &|_| {});
    assert!(matches!(outcome, Outcome::Failed(ref e) if e.starts_with("github/network")));
    assert_eq!(http.calls().len(), 5);
}

#[test]
fn 模式分派_四种方式() {
    let token = Token::new("gho_x");
    assert_eq!(resolve(Mode::Off, token.clone()), Executor::Off);
    assert_eq!(resolve(Mode::Gh, token.clone()), Executor::Gh);
    assert_eq!(resolve(Mode::Meowo, token.clone()), Executor::Meowo(token.clone()));
    assert_eq!(resolve(Mode::Meowo, None), Executor::Meowo(None));
    assert_eq!(resolve(Mode::Auto, token.clone()), Executor::Meowo(token));
    assert_eq!(resolve(Mode::Auto, None), Executor::Gh);
    for text in ["auto", "meowo", "gh", "off"] {
        assert_eq!(Mode::parse(text).unwrap().as_str(), text);
    }
    assert_eq!(Mode::parse("bogus"), None);
}

#[test]
fn 令牌_debug_打码_字符集受限() {
    let token = Token::new("gho_SECRET123").unwrap();
    assert_eq!(format!("{token:?}"), "Token(<redacted>)");
    assert!(Token::new("gho x").is_none());
    assert!(Token::new("a\"-w").is_none());
    assert!(Token::new("").is_none());
}

#[test]
fn client_id_是注册的那个且没有_secret() {
    if option_env!("MEOWO_GITHUB_CLIENT_ID").is_none() {
        assert_eq!(CLIENT_ID, "Ov23lilH0boYrABgr4xl");
    }
    let source = include_str!("../github.rs").to_lowercase() + &include_str!("device.rs").to_lowercase();
    assert!(!source.contains("client_secret"), "设备码流程不需要也不许出现 client secret");
}

#[test]
fn 只许发往_github() {
    assert!(allowed_url("https://api.github.com/user"));
    assert!(allowed_url(DEVICE_CODE_URL) && allowed_url(TOKEN_URL));
    for bad in ["https://evil.com/", "http://api.github.com/user", "https://api.github.com.evil.com/", "https://github.com/login/other"] {
        assert!(!allowed_url(bad), "{bad}");
        // 真实 HTTP 在发请求前就拒（不联网）。
        assert_eq!(RealHttp.send(&Request { method: "GET", url: bad, token: None, body: None }).unwrap_err(), "github/host_not_allowed");
    }
}

struct MemVault(Mutex<Option<Token>>);
impl Vault for MemVault {
    fn supported(&self) -> bool {
        true
    }
    fn read(&self) -> Result<Option<Token>, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, token: &Token) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(token.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

fn temp_file(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("meowo-github-{tag}-{}-{}", std::process::id(), crate::now_ms()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("github.json")
}

#[test]
fn 登录收尾_令牌只进凭据库_记录文件与状态里都没有() {
    let path = temp_file("login");
    let vault = MemVault(Mutex::new(None));
    let http = Script::new(vec![reply(200, r#"{"login":"octocat","name":"The Octocat","avatar_url":"https://avatars.githubusercontent.com/u/1"}"#)]);
    let token = Token::new("gho_TOPSECRET42").unwrap();
    let account = complete_login(&http, &vault, &path, &token).unwrap();
    assert_eq!(account.login, "octocat");
    assert_eq!(vault.read().unwrap(), Some(token.clone()));
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains("octocat") && !file.contains("TOPSECRET"));
    let status = status_with(Mode::Auto, Some(token), &load_record(&path), true, true, None);
    assert_eq!(status.active, "meowo");
    assert_eq!(status.account.as_ref().unwrap().login, "octocat");
    assert!(!serde_json::to_string(&status).unwrap().contains("TOPSECRET"));
    // 退出：记录删掉。
    save_record(&path, &AccountRecord::default()).unwrap();
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn 登录收尾_令牌不认就不落库() {
    let path = temp_file("login401");
    let vault = MemVault(Mutex::new(None));
    let http = Script::new(vec![reply(401, r#"{"message":"Bad credentials"}"#)]);
    let err = complete_login(&http, &vault, &path, &Token::new("gho_bad").unwrap()).unwrap_err();
    assert_eq!(err, "github/unauthorized");
    assert_eq!(vault.read().unwrap(), None);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn 状态_没令牌不显示旧账号_要重新登录只在有令牌时() {
    let record = AccountRecord { account: Some(GithubAccountDto { login: "old".into(), ..Default::default() }), needs_relogin: true };
    let signed_out = status_with(Mode::Meowo, None, &record, false, true, None);
    assert!(signed_out.account.is_none() && !signed_out.needs_relogin);
    assert_eq!(signed_out.active, "meowo");
    let stale = status_with(Mode::Gh, Token::new("gho_x"), &record, true, true, None);
    assert!(stale.needs_relogin);
    assert_eq!(stale.active, "gh");
    assert_eq!(status_with(Mode::Off, Token::new("gho_x"), &record, true, true, None).active, "off");
}

#[test]
fn 设置_老设置文件默认自动() {
    let settings: crate::settings::Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(settings.github_connection, "auto");
    assert_eq!(crate::settings::Settings::default().github_connection, "auto");
}
