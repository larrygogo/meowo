//! 手机推送（Bark）：会话出错、等审批、屏幕上卡着等人、回合结束等你回复时，除了桌面通知，
//! 再推一条到 iPhone 上的 Bark App。
//!
//! 为什么不是网页推送：手机远程页是明文 http（局域网 / Tailscale），浏览器只在 https 下
//! 开放推送。Bark 只要电脑能连上它的服务器，手机上装个 App、填一个地址就能收（用户选定）。
//!
//! 内容会经过 Bark 服务器转发（项目名、会话标题、在等什么）；地址也可以填自建的 Bark 服务。
//! 点通知打开的链接不带远程访问 token——token 只在宿主机和已配对的手机上，不能交给第三方。
//!
//! 何时推送由 watch.rs 的通知判定决定，与桌面通知同一套去重指纹；这里只管发。桌面通知
//! 总开关不影响它：人不在电脑前时，桌面通知关着才是常态。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 同一会话同一条内容多久内不重推。桌面通知被点开会整清已投递的 toast 并重置审批类指纹
/// （watch.rs RESET_DECISION_RENOTIFY），下一轮会给仍在等的会话补发桌面入口——人就在
/// 电脑前，手机上不必再响一遍。
const DEDUPE_WINDOW: Duration = Duration::from_secs(10 * 60);
const TIMEOUT: Duration = Duration::from_secs(8);

/// 推送出去的一条。`urgent` = 要人拍板（审批、屏幕阻塞）：用 Bark 的 timeSensitive 级别，
/// 专注模式下也能弹出。
pub(crate) struct PushMessage {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) session_id: i64,
    pub(crate) urgent: bool,
}

/// 校验并规整用户填的地址：Bark App 里复制出来的是 `https://api.day.app/<key>/`（后面
/// 可能还带着示例文字），只认 http(s) 且路径第一段是 key 的形态，统一成 `scheme://host/<key>`。
pub(crate) fn normalize_bark_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let parsed = url::Url::parse(raw).map_err(|_| "push/invalid_url".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("push/invalid_url".into());
    }
    let key = parsed
        .path_segments()
        .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
        .ok_or_else(|| "push/missing_key".to_string())?;
    let mut base = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or_default());
    if let Some(port) = parsed.port() {
        base.push_str(&format!(":{port}"));
    }
    Ok(format!("{base}/{key}"))
}

/// Bark 的 JSON 推送体。纯函数，便于单测。
fn payload(message: &PushMessage, link: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({
        "title": message.title,
        "body": message.body,
        "group": "Meowo",
        "level": if message.urgent { "timeSensitive" } else { "active" },
    });
    if let Some(link) = link {
        body["url"] = serde_json::Value::String(link.to_string());
    }
    body
}

/// 点通知打开的远程页链接：远程访问开着且探得到地址时，指到这条会话。不带 token
/// （见模块注释），手机上已配对过的浏览器会用自己存着的 token。
fn session_link(settings: &crate::settings::Settings, session_id: i64) -> Option<String> {
    if !settings.remote_access_enabled {
        return None;
    }
    let ip = crate::remote::preferred_ip(&settings.remote_access_bind)?;
    Some(format!("http://{ip}:{}/?sessionId={session_id}", settings.remote_access_port))
}

fn send(url: &str, body: &serde_json::Value) -> Result<(), String> {
    let proxy = crate::ports::resolve_proxy(None);
    let response = crate::ports::agent_for(proxy.as_deref())
        .post(url)
        .timeout(TIMEOUT)
        .send_json(body.clone());
    match response {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(format!("push/http_{code}")),
        Err(error) => Err(format!("push/send_failed：{error}")),
    }
}

/// 去重：同会话同内容在窗口期内只推一次。返回 true = 该推。
fn should_push(session_id: i64, fingerprint: &str, now: Instant) -> bool {
    static SENT: OnceLock<Mutex<HashMap<i64, (String, Instant)>>> = OnceLock::new();
    let mut sent = SENT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((last, at)) = sent.get(&session_id) {
        if last == fingerprint && now.duration_since(*at) < DEDUPE_WINDOW {
            return false;
        }
    }
    sent.insert(session_id, (fingerprint.to_string(), now));
    true
}

/// watch.rs 通知判定命中时调用。没填地址就什么都不做；发送在独立线程里，失败只打日志——
/// 通知循环不能被网络卡住。
pub(crate) fn push_session(settings: &crate::settings::Settings, message: PushMessage) {
    if settings.bark_url.trim().is_empty() {
        return;
    }
    let Ok(url) = normalize_bark_url(&settings.bark_url) else {
        return;
    };
    if !should_push(message.session_id, &format!("{}\n{}", message.title, message.body), Instant::now()) {
        return;
    }
    let settings = settings.clone();
    std::thread::spawn(move || {
        // 探测地址要做一次路由查询，放进发送线程，不占通知循环。
        let link = session_link(&settings, message.session_id);
        let body = payload(&message, link.as_deref());
        if let Err(error) = send(&url, &body) {
            eprintln!("[push] {error}");
        }
    });
}

/// 设置页「发一条测试」：用输入框里的地址（还没保存也能测），同步返回成败给界面。
#[tauri::command]
pub(crate) async fn test_bark_push(url: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let url = normalize_bark_url(&url)?;
        let lang = crate::settings::ui_lang(&crate::settings::load_settings());
        let message = PushMessage {
            title: "Meowo".into(),
            body: crate::settings::tr(lang, "notify.pushTest").to_string(),
            session_id: 0,
            urgent: false,
        };
        send(&url, &payload(&message, None))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_bark_app_copy_and_strips_sample_text() {
        assert_eq!(
            normalize_bark_url("https://api.day.app/AbCdEf123/这里改成你自己的推送内容").unwrap(),
            "https://api.day.app/AbCdEf123"
        );
        assert_eq!(normalize_bark_url(" https://api.day.app/key/ ").unwrap(), "https://api.day.app/key");
        assert_eq!(normalize_bark_url("http://10.0.0.2:8080/key").unwrap(), "http://10.0.0.2:8080/key");
    }

    #[test]
    fn normalize_rejects_non_http_and_missing_key() {
        assert!(normalize_bark_url("ftp://api.day.app/key").is_err());
        assert!(normalize_bark_url("https://api.day.app/").is_err());
        assert!(normalize_bark_url("不是地址").is_err());
    }

    #[test]
    fn payload_marks_decisions_time_sensitive_and_carries_link() {
        let message = PushMessage { title: "t".into(), body: "b".into(), session_id: 3, urgent: true };
        let body = payload(&message, Some("http://100.1.2.3:18620/?sessionId=3"));
        assert_eq!(body["level"], "timeSensitive");
        assert_eq!(body["group"], "Meowo");
        assert_eq!(body["url"], "http://100.1.2.3:18620/?sessionId=3");
        let calm = payload(&PushMessage { urgent: false, ..message }, None);
        assert_eq!(calm["level"], "active");
        assert!(calm.get("url").is_none());
    }

    #[test]
    fn dedupe_skips_same_content_within_window_only() {
        let now = Instant::now();
        assert!(should_push(-901, "a", now));
        assert!(!should_push(-901, "a", now + Duration::from_secs(60)));
        assert!(should_push(-901, "b", now + Duration::from_secs(61)));
        assert!(should_push(-901, "b", now + DEDUPE_WINDOW + Duration::from_secs(62)));
    }
}
