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
    Ok(Response { status, body: body.to_string(), ..Default::default() })
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
    assert_eq!(resolve(Mode::Auto, token.clone()), Executor::Auto(token.clone().unwrap()));
    assert_eq!(Executor::Auto(token.unwrap()).label(), "meowo");
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

const ACCOUNT_JSON: &str = r#"{"login":"octocat","name":"The Octocat","avatar_url":"https://avatars.githubusercontent.com/u/1"}"#;

/// 一轮已开始的登录（代次 1）。
fn slot_with_pending() -> Mutex<LoginSlot> {
    let slot = Mutex::new(LoginSlot::new());
    slot.lock().unwrap().generation = 1;
    slot.lock().unwrap().pending = Some(GithubDeviceCodeDto { user_code: "ABCD-1234".into(), ..Default::default() });
    slot
}

#[test]
fn 登录收尾_令牌只进凭据库_记录文件与状态里都没有() {
    let path = temp_file("login");
    let vault = MemVault(Mutex::new(None));
    let http = Script::new(vec![reply(200, ACCOUNT_JSON)]);
    let token = Token::new("gho_TOPSECRET42").unwrap();
    let slot = slot_with_pending();
    let stored = Mutex::new(None);
    let finish = finish_login_with(&slot, 1, Outcome::Token(token.clone()), &http, &vault, &path, &|t| *stored.lock().unwrap() = Some(t.clone()));
    assert!(matches!(finish, Finish::SignedIn(ref a) if a.login == "octocat"));
    assert_eq!(vault.read().unwrap(), Some(token.clone()));
    assert_eq!(stored.lock().unwrap().clone(), Some(token.clone()));
    assert!(slot.lock().unwrap().pending.is_none());
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains("octocat") && !file.contains("TOPSECRET"));
    let status = status_with(Mode::Auto, Some(token), &load_record(&path), true, true, None);
    assert_eq!(status.active, "meowo");
    assert_eq!(status.account.as_ref().unwrap().login, "octocat");
    assert!(!serde_json::to_string(&status).unwrap().contains("TOPSECRET"));
    // 退出：凭据库与记录都删掉，代次作废，回调（清进程内令牌、作废缓存）跑了。
    let signed_out = AtomicUsize::new(0);
    logout_with(&slot, &vault, &path, &|| {
        signed_out.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    assert_eq!(vault.read().unwrap(), None);
    assert!(!path.exists());
    assert_eq!(slot.lock().unwrap().generation, 2);
    assert_eq!(signed_out.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn 登录收尾_令牌不认就不落库() {
    let path = temp_file("login401");
    let vault = MemVault(Mutex::new(None));
    let http = Script::new(vec![reply(401, r#"{"message":"Bad credentials"}"#)]);
    let slot = slot_with_pending();
    let finish = finish_login_with(&slot, 1, Outcome::Token(Token::new("gho_bad").unwrap()), &http, &vault, &path, &|_| panic!("不该落库"));
    assert_eq!(finish, Finish::Failed("github/unauthorized".into()));
    assert_eq!(vault.read().unwrap(), None);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// 回归（独立审查 #3）：问 GitHub「这是谁」时不持登录锁——期间点取消立刻生效（状态查询也不被卡），
/// 令牌不落库、不报「登录成功」。修复前收尾全程持锁，取消要等网络，令牌照样落库。
struct CancelDuringFetch<'a> {
    slot: &'a Mutex<LoginSlot>,
}
impl Http for CancelDuringFetch<'_> {
    fn send(&self, _: &Request) -> Result<Response, String> {
        // 用户在这时点了取消：锁必须拿得到（不能被收尾占着）。
        let mut s = self.slot.try_lock().expect("网络阶段不能持有登录锁");
        s.generation += 1;
        s.pending = None;
        drop(s);
        Ok(Response { status: 200, body: ACCOUNT_JSON.into(), ..Default::default() })
    }
}

#[test]
fn 登录收尾_网络阶段点取消_令牌不落库() {
    let path = temp_file("login-cancel");
    let vault = MemVault(Mutex::new(None));
    let slot = slot_with_pending();
    let http = CancelDuringFetch { slot: &slot };
    let finish = finish_login_with(&slot, 1, Outcome::Token(Token::new("gho_late").unwrap()), &http, &vault, &path, &|_| panic!("不该落库"));
    assert_eq!(finish, Finish::Stale);
    assert_eq!(vault.read().unwrap(), None);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn 登录收尾_已取消的一轮_过期拒绝失败都不报() {
    let path = temp_file("login-stale");
    let vault = MemVault(Mutex::new(None));
    let http = Script::new(vec![]);
    let slot = slot_with_pending();
    slot.lock().unwrap().generation = 5;
    for outcome in [Outcome::Expired, Outcome::Denied, Outcome::Failed("x".into()), Outcome::Cancelled, Outcome::Token(Token::new("gho_t").unwrap())] {
        assert_eq!(finish_login_with(&slot, 1, outcome, &http, &vault, &path, &|_| panic!("不该落库")), Finish::Stale);
    }
    assert!(http.calls().is_empty(), "已取消的一轮不再联网");
    // 同一轮的过期：清掉进行中的代码。
    let slot = slot_with_pending();
    assert_eq!(finish_login_with(&slot, 1, Outcome::Expired, &http, &vault, &path, &|_| {}), Finish::Expired);
    assert!(slot.lock().unwrap().pending.is_none());
}

/// 回归（审查 #8）：要代码期间又点了一次登录（或取消），前一次作废，不会把旧代码顶上来。
struct BumpDuringRequest<'a> {
    slot: &'a Mutex<LoginSlot>,
}
impl Http for BumpDuringRequest<'_> {
    fn send(&self, _: &Request) -> Result<Response, String> {
        self.slot.try_lock().expect("要代码时不能持有登录锁").generation += 1;
        Ok(Response { status: 200, body: r#"{"device_code":"d","user_code":"OLD-CODE","expires_in":900,"interval":5}"#.into(), ..Default::default() })
    }
}

#[test]
fn 开始登录_期间被顶替就作废() {
    let slot = Mutex::new(LoginSlot::new());
    let err = begin_login_with(&slot, &BumpDuringRequest { slot: &slot }, "cid", 0).unwrap_err();
    assert_eq!(err, "github/cancelled");
    assert!(slot.lock().unwrap().pending.is_none());
    let ok = Script::new(vec![reply(200, r#"{"device_code":"d","user_code":"NEW-CODE","expires_in":900,"interval":5}"#)]);
    let (generation, _, dto) = begin_login_with(&slot, &ok, "cid", 1_000).unwrap();
    assert_eq!((generation, dto.user_code.as_str(), dto.expires_at), (3, "NEW-CODE", 901_000));
    assert_eq!(slot.lock().unwrap().pending.as_ref().unwrap().user_code, "NEW-CODE");
}

/// 回归（审查 #5）：旧令牌晚到的 401 不能把新登录标成要重登；之后成功的请求清掉标记。
#[test]
fn 令牌被拒_只认当前令牌_成功后清掉() {
    let path = temp_file("relogin");
    save_record(&path, &AccountRecord { account: Some(GithubAccountDto { login: "me".into(), ..Default::default() }), needs_relogin: false }).unwrap();
    let known = Mutex::new(None);
    let old = Token::new("gho_old").unwrap();
    let new = Token::new("gho_new").unwrap();
    assert!(!auth_result_with(&path, Some(&new), &old, false, &known), "旧令牌的 401 不算");
    assert!(!load_record(&path).needs_relogin);
    assert!(!auth_result_with(&path, Some(&new), &new, true, &known), "本来就好：不改记录");
    assert!(auth_result_with(&path, Some(&new), &new, false, &known), "当前令牌被拒：标要重登");
    assert!(load_record(&path).needs_relogin);
    assert!(!auth_result_with(&path, Some(&new), &new, false, &known), "重复 401 不重复通知");
    assert!(auth_result_with(&path, Some(&new), &new, true, &known), "之后成功：清掉");
    assert!(!load_record(&path).needs_relogin);
    assert_eq!(load_record(&path).account.unwrap().login, "me");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn 切方式_写设置与内存后作废缓存_写失败不动内存() {
    let before = *MODE.lock().unwrap();
    let changed = AtomicUsize::new(0);
    let on_changed = || {
        changed.fetch_add(1, Ordering::SeqCst);
    };
    let err = set_mode_with(Mode::Off, &|_| Err("settings/write_failed: x".into()), &on_changed).unwrap_err();
    assert!(err.starts_with("settings/write_failed"));
    assert_eq!(*MODE.lock().unwrap(), before);
    assert_eq!(changed.load(Ordering::SeqCst), 0);
    let written = Mutex::new(None);
    set_mode_with(Mode::Gh, &|m| {
        *written.lock().unwrap() = Some(m);
        Ok(())
    }, &on_changed)
    .unwrap();
    assert_eq!((*written.lock().unwrap(), *MODE.lock().unwrap()), (Some(Mode::Gh), Some(Mode::Gh)));
    assert_eq!(changed.load(Ordering::SeqCst), 1);
    *MODE.lock().unwrap() = before;
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

// ═══ 重定向（改名 / 转移过的仓库）═══

type Seen = Mutex<Vec<(String, String, bool, Option<serde_json::Value>)>>;

fn redirecting<'s>(seen: &'s Seen, status: u16, location: &str) -> impl Fn(&Request) -> Result<Response, String> + 's {
    let location = location.to_string();
    move |req: &Request| {
        let body = match &req.body {
            Some(Body::Json(v)) => Some(v.clone()),
            _ => None,
        };
        seen.lock().unwrap().push((req.method.to_string(), req.url.to_string(), req.token.is_some(), body));
        if req.url.contains("/old/") {
            Ok(Response { status, location: Some(location.clone()), ..Default::default() })
        } else {
            Ok(Response { status: 200, body: "{}".into(), ..Default::default() })
        }
    }
}

#[test]
fn 重定向_307_308_保留方法正文与令牌() {
    let token = Token::new("gho_x").unwrap();
    for status in [307, 308] {
        let seen = Seen::default();
        let raw = redirecting(&seen, status, "https://api.github.com/repos/new/repo/pulls/1/merge");
        let req = Request { method: "PUT", url: "https://api.github.com/repos/old/repo/pulls/1/merge", token: Some(&token), body: Some(Body::Json(serde_json::json!({ "merge_method": "squash" }))) };
        assert_eq!(follow_redirects(&raw, &req).unwrap().status, 200);
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].0, "PUT");
        assert_eq!(seen[1].1, "https://api.github.com/repos/new/repo/pulls/1/merge");
        assert!(seen[1].2, "同主机跳转要带着令牌");
        assert_eq!(seen[1].3, Some(serde_json::json!({ "merge_method": "squash" })));
    }
}

#[test]
fn 重定向_301_302_get_照跟_post_改成不带正文的_get() {
    let token = Token::new("gho_x").unwrap();
    let seen = Seen::default();
    let raw = redirecting(&seen, 301, "/repositories/42/pulls/1/files");
    let req = Request { method: "GET", url: "https://api.github.com/repos/old/repo/pulls/1/files", token: Some(&token), body: None };
    assert_eq!(follow_redirects(&raw, &req).unwrap().status, 200);
    let seen = seen.lock().unwrap().clone();
    assert_eq!((seen[1].0.as_str(), seen[1].1.as_str(), seen[1].2), ("GET", "https://api.github.com/repositories/42/pulls/1/files", true));

    let seen = Seen::default();
    let raw = redirecting(&seen, 302, "https://api.github.com/repos/new/repo/issues/1/comments");
    let req = Request { method: "POST", url: "https://api.github.com/repos/old/repo/issues/1/comments", token: Some(&token), body: Some(Body::Json(serde_json::json!({ "body": "x" }))) };
    follow_redirects(&raw, &req).unwrap();
    let seen = seen.lock().unwrap().clone();
    assert_eq!((seen[1].0.as_str(), seen[1].3.is_none()), ("GET", true));
}

#[test]
fn 重定向_别的主机与_http_一律不跟_回包原样返回() {
    let token = Token::new("gho_x").unwrap();
    for location in ["https://evil.example.com/steal", "http://api.github.com/repos/new/repo", "https://api.github.com:8443/x", "https://github.com/login"] {
        let seen = Seen::default();
        let raw = redirecting(&seen, 307, location);
        let req = Request { method: "GET", url: "https://api.github.com/repos/old/repo", token: Some(&token), body: None };
        let resp = follow_redirects(&raw, &req).unwrap();
        assert_eq!(resp.status, 307, "{location}");
        assert_eq!(seen.lock().unwrap().clone().len(), 1, "{location} 不能跟");
    }
}

#[test]
fn 重定向_跳数有上限() {
    let token = Token::new("gho_x").unwrap();
    let count = AtomicUsize::new(0);
    let raw = |_: &Request| {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Ok(Response { status: 301, location: Some(format!("https://api.github.com/repos/old/r{n}")), ..Default::default() })
    };
    let req = Request { method: "GET", url: "https://api.github.com/repos/old/r", token: Some(&token), body: None };
    assert_eq!(follow_redirects(&raw, &req).unwrap_err(), "github/too_many_redirects");
    assert_eq!(count.load(Ordering::SeqCst), 6);
}

// ═══ 传输层失败重试（leader 安装版：TUN 代理下 TLS 握手偶发 EOF）═══

/// 先回 fails 次传输错误，之后成功；记下调用次数与睡眠。
fn flaky(fails: usize, calls: &AtomicUsize) -> impl Fn(&Request) -> Result<Response, String> + '_ {
    move |_: &Request| {
        let n = calls.fetch_add(1, Ordering::SeqCst);
        if n < fails {
            Err("https://github.com/login/device/code: Connection Failed: tls connection init failed: unexpected end of file".into())
        } else {
            Ok(Response { status: 200, body: "{}".into(), ..Default::default() })
        }
    }
}

#[test]
fn 重试_读与设备码请求_传输失败后退避重试() {
    let token = Token::new("gho_x").unwrap();
    let query = Body::Json(serde_json::json!({ "query": "query($id:ID!){node(id:$id){id}}", "variables": {} }));
    let safe: Vec<Request> = vec![
        Request { method: "GET", url: "https://api.github.com/user", token: Some(&token), body: None },
        Request { method: "POST", url: DEVICE_CODE_URL, token: None, body: Some(Body::Form(vec![("client_id", "c".into())])) },
        Request { method: "POST", url: TOKEN_URL, token: None, body: Some(Body::Form(vec![("client_id", "c".into())])) },
        Request { method: "POST", url: "https://api.github.com/graphql", token: Some(&token), body: Some(query) },
    ];
    for req in &safe {
        let calls = AtomicUsize::new(0);
        let slept = Mutex::new(Vec::new());
        let resp = with_retry(&flaky(2, &calls), req, &|d| slept.lock().unwrap().push(d)).unwrap();
        assert_eq!(resp.status, 200, "{}", req.url);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "{}", req.url);
        assert_eq!(*slept.lock().unwrap(), [Duration::from_millis(500), Duration::from_secs(1)]);
    }
}

#[test]
fn 重试_全部失败才报_带原因() {
    let calls = AtomicUsize::new(0);
    let slept = Mutex::new(Vec::new());
    let req = Request { method: "POST", url: DEVICE_CODE_URL, token: None, body: None };
    let err = with_retry(&flaky(99, &calls), &req, &|d| slept.lock().unwrap().push(d)).unwrap_err();
    assert!(err.contains("unexpected end of file") && err.contains("retried 3 times"), "{err}");
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(*slept.lock().unwrap(), [Duration::from_millis(500), Duration::from_secs(1), Duration::from_secs(2)]);
    // 设备码申请失败的提示仍带原因。
    let http = Script::new(vec![Err(err)]);
    assert!(request_code(&http, "cid").unwrap_err().contains("unexpected end of file"));
}

#[test]
fn 重试_写操作只发一次() {
    let token = Token::new("gho_x").unwrap();
    let mutation = Body::Json(serde_json::json!({ "query": "mutation($input:EnqueuePullRequestInput!){enqueuePullRequest(input:$input){mergeQueueEntry{id}}}" }));
    let writes: Vec<Request> = vec![
        Request { method: "POST", url: "https://api.github.com/repos/o/r/pulls", token: Some(&token), body: Some(Body::Json(serde_json::json!({}))) },
        Request { method: "PUT", url: "https://api.github.com/repos/o/r/pulls/1/merge", token: Some(&token), body: None },
        Request { method: "POST", url: "https://api.github.com/repos/o/r/issues/1/comments", token: Some(&token), body: None },
        Request { method: "POST", url: "https://api.github.com/graphql", token: Some(&token), body: Some(mutation) },
    ];
    for req in &writes {
        let calls = AtomicUsize::new(0);
        assert!(with_retry(&flaky(1, &calls), req, &|_| panic!("写操作不许等待重试")).is_err(), "{}", req.url);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{} 只能发一次", req.url);
    }
}

#[test]
fn 重试_收到回包就不重试() {
    let calls = AtomicUsize::new(0);
    let raw = |_: &Request| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Response { status: 502, ..Default::default() })
    };
    let req = Request { method: "GET", url: "https://api.github.com/user", token: None, body: None };
    assert_eq!(with_retry(&raw, &req, &|_| {}).unwrap().status, 502);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
