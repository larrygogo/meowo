//! 面板浏览器的设备模拟：视口尺寸、UA、附加请求头。
//!
//! 为什么要有：面板常见只有 400～700 CSS 像素宽，响应式网站会走手机版布局；SSR 网站按 UA 和
//! 请求头决定吐哪一版 HTML。Agent 不知道也改不了的话，测出来的「失败」可能只是面板太窄
//! （2026-11-18 leader 提出）。
//!
//! 做法同 Kimi Code 桌面版与 Playwright：走 Chrome 调试协议（CDP）的
//! `Emulation.setDeviceMetricsOverride` / `Emulation.setUserAgentOverride` /
//! `Network.setExtraHTTPHeaders`。WebView2 经 `CallDevToolsProtocolMethod` 调同一套协议；
//! WKWebView 没有 CDP，macOS 上明确返回不支持，不假装成功。
//!
//! 每次调用整体替换之前的模拟（没给的项回到默认、不模拟），语义简单、可预期。

use serde::Serialize;
use serde_json::{json, Map, Value};

/// macOS / Linux 上的统一说法。
#[cfg(not(windows))]
const UNSUPPORTED: &str =
    "当前平台的面板浏览器不支持设备模拟（只有 Windows 的 WebView2 提供 Chrome 调试协议）。";

const IPHONE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";
const IPAD_UA: &str = "Mozilla/5.0 (iPad; CPU OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";

/// 预设：与 Playwright devices 表同档（iPhone 14 / iPad Air），桌面取最常见的 1280×800。
struct Preset {
    id: &'static str,
    width: u32,
    height: u32,
    dpr: f64,
    mobile: bool,
    ua: Option<&'static str>,
    /// navigator.platform：只改 UA 不改它，按 platform 判断设备的脚本仍会认成 Windows。
    platform: Option<&'static str>,
}

const PRESETS: &[Preset] = &[
    Preset {
        id: "desktop",
        width: 1280,
        height: 800,
        dpr: 1.0,
        mobile: false,
        ua: None,
        platform: None,
    },
    Preset {
        id: "tablet",
        width: 820,
        height: 1180,
        dpr: 2.0,
        mobile: true,
        ua: Some(IPAD_UA),
        platform: Some("iPad"),
    },
    Preset {
        id: "mobile",
        width: 390,
        height: 844,
        dpr: 3.0,
        mobile: true,
        ua: Some(IPHONE_UA),
        platform: Some("iPhone"),
    },
];

/// 尺寸上下限（CSS 像素）。太小页面没法用，太大缩到面板里什么都看不清。
const MIN_SIDE: u32 = 200;
const MAX_SIDE: u32 = 4096;
const MAX_HEADERS: usize = 20;
/// 这几个头由网络栈自己管，改了请求会坏。
const FORBIDDEN_HEADERS: &[&str] = &["host", "content-length", "connection", "transfer-encoding"];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Metrics {
    /// desktop / tablet / mobile；尺寸和预设不同时是 custom。
    pub(crate) device: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    #[serde(skip)]
    pub(crate) dpr: f64,
    #[serde(skip)]
    pub(crate) mobile: bool,
}

/// 发给前端的部分不含请求头的值：值里可能有令牌，面板上只需要知道改了哪些头。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Emulation {
    pub(crate) metrics: Option<Metrics>,
    pub(crate) user_agent: Option<String>,
    /// 跟预设 UA 一起改的 navigator.platform；自己给了 UA 就不改（不知道该配什么）。
    #[serde(skip)]
    pub(crate) platform: Option<String>,
    pub(crate) header_names: Vec<String>,
    #[serde(skip)]
    pub(crate) headers: Vec<(String, String)>,
}

fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

fn side(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let n = value
                .as_f64()
                .filter(|n| n.fract() == 0.0)
                .ok_or_else(|| format!("{key} 要是整数（CSS 像素）"))?;
            if !(MIN_SIDE as f64..=MAX_SIDE as f64).contains(&n) {
                return Err(format!("{key} 要在 {MIN_SIDE}～{MAX_SIDE} 之间"));
            }
            Ok(Some(n as u32))
        }
    }
}

fn clean_text(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// 工具参数 → 要设成的模拟。`Ok(None)` 是还原。参数不对时报中文错误，调用方据此不改任何状态。
pub(crate) fn parse(args: &Value) -> Result<Option<Emulation>, String> {
    let device = match args.get("device") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.trim().to_ascii_lowercase()),
        Some(_) => return Err("device 要是字符串".into()),
    };
    let width = side(args, "width")?;
    let height = side(args, "height")?;
    let user_agent = match args.get("user_agent") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() || s.chars().count() > 512 || !clean_text(s) {
                return Err("user_agent 要是 1～512 个字符、不含换行的字符串".into());
            }
            Some(s.to_string())
        }
        Some(_) => return Err("user_agent 要是字符串".into()),
    };
    let headers = match args.get("headers") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(map)) => parse_headers(map)?,
        Some(_) => return Err("headers 要是对象，如 {\"Accept-Language\": \"en-US\"}".into()),
    };
    if device.as_deref() == Some("reset") {
        if width.is_some() || height.is_some() || user_agent.is_some() || !headers.is_empty() {
            return Err("device: \"reset\" 是全部还原，不能和其他参数一起用".into());
        }
        return Ok(None);
    }
    let base = match device.as_deref() {
        None => None,
        Some(id) => Some(preset(id).ok_or_else(|| {
            format!("device 只能是 desktop、tablet、mobile 或 reset，收到「{id}」")
        })?),
    };
    let metrics = if base.is_some() || width.is_some() || height.is_some() {
        let fallback = base.unwrap_or(&PRESETS[0]);
        let w = width.unwrap_or(fallback.width);
        let h = height.unwrap_or(fallback.height);
        let device = match base {
            Some(p) if p.width == w && p.height == h => p.id.to_string(),
            _ => "custom".to_string(),
        };
        Some(Metrics {
            device,
            width: w,
            height: h,
            dpr: base.map_or(1.0, |p| p.dpr),
            mobile: base.is_some_and(|p| p.mobile),
        })
    } else {
        None
    };
    let platform = match &user_agent {
        Some(_) => None,
        None => base.and_then(|p| p.platform).map(str::to_string),
    };
    let user_agent = user_agent.or_else(|| base.and_then(|p| p.ua).map(str::to_string));
    if metrics.is_none() && user_agent.is_none() && headers.is_empty() {
        return Err("至少给一个参数：device、width/height、user_agent 或 headers；要还原用 device: \"reset\"".into());
    }
    Ok(Some(Emulation {
        metrics,
        user_agent,
        platform,
        header_names: headers.iter().map(|(name, _)| name.clone()).collect(),
        headers,
    }))
}

fn parse_headers(map: &Map<String, Value>) -> Result<Vec<(String, String)>, String> {
    if map.len() > MAX_HEADERS {
        return Err(format!("附加请求头最多 {MAX_HEADERS} 个"));
    }
    let mut out = Vec::with_capacity(map.len());
    for (name, value) in map {
        let valid_name = !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid_name {
            return Err(format!("请求头名「{name}」无效：只能用字母、数字、- 和 _"));
        }
        let lower = name.to_ascii_lowercase();
        if FORBIDDEN_HEADERS.contains(&lower.as_str()) {
            return Err(format!("请求头「{name}」由浏览器自己管理，不能改"));
        }
        if lower == "user-agent" {
            return Err("UA 用 user_agent 参数改，不放在 headers 里".into());
        }
        if out
            .iter()
            .any(|(seen, _): &(String, String)| seen.eq_ignore_ascii_case(name))
        {
            return Err(format!("请求头「{name}」重复了（名字不分大小写）"));
        }
        let value = value
            .as_str()
            .ok_or_else(|| format!("请求头「{name}」的值要是字符串"))?;
        if value.len() > 1024 || !clean_text(value) {
            return Err(format!(
                "请求头「{name}」的值要是 1024 字节以内、不含换行的字符串"
            ));
        }
        out.push((name.clone(), value.to_string()));
    }
    Ok(out)
}

/// 改了 UA 或请求头就要刷新：SSR 网站按请求决定吐哪一版 HTML，不刷新看到的还是旧的那版。
/// 只改尺寸不刷新——响应式 CSS 自己会重排，刷新反而丢掉页面上的状态。
pub(crate) fn needs_reload(before: Option<&Emulation>, after: Option<&Emulation>) -> bool {
    let ua = |e: Option<&Emulation>| e.and_then(|e| e.user_agent.clone());
    let headers = |e: Option<&Emulation>| e.map(|e| e.headers.clone()).unwrap_or_default();
    ua(before) != ua(after) || headers(before) != headers(after)
}

/// 模拟尺寸比面板大时按比例缩小显示，整页都在面板里看得到（DevTools 设备模式同理）。
pub(crate) fn fit_scale(metrics: &Metrics, (view_w, view_h): (f64, f64)) -> f64 {
    if view_w < 1.0 || view_h < 1.0 {
        return 1.0;
    }
    (view_w / metrics.width as f64)
        .min(view_h / metrics.height as f64)
        .clamp(0.1, 1.0)
}

/// 视口尺寸相关的 CDP 调用。面板拖宽、收起时只重发这一组（缩放比例跟着变）。
pub(crate) fn metrics_calls(
    metrics: Option<&Metrics>,
    view: (f64, f64),
) -> Vec<(&'static str, Value)> {
    match metrics {
        Some(m) => vec![
            (
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": m.width,
                    "height": m.height,
                    "deviceScaleFactor": m.dpr,
                    "mobile": m.mobile,
                    "scale": fit_scale(m, view),
                    "screenWidth": m.width,
                    "screenHeight": m.height,
                }),
            ),
            (
                "Emulation.setTouchEmulationEnabled",
                json!({ "enabled": m.mobile, "maxTouchPoints": if m.mobile { 5 } else { 1 } }),
            ),
        ],
        None => vec![
            ("Emulation.clearDeviceMetricsOverride", json!({})),
            (
                "Emulation.setTouchEmulationEnabled",
                json!({ "enabled": false }),
            ),
        ],
    }
}

/// 从 `before` 换到 `next`（None = 全部还原）要发的 CDP 调用，按顺序执行。
pub(crate) fn calls(
    before: Option<&Emulation>,
    next: Option<&Emulation>,
    view: (f64, f64),
) -> Vec<(&'static str, Value)> {
    let mut out = metrics_calls(next.and_then(|e| e.metrics.as_ref()), view);
    // 空字符串 = 撤掉 UA 覆盖，回到 WebView2 自己的 UA（DevTools 设备模式也这样清）。
    let ua = next.and_then(|e| e.user_agent.as_deref()).unwrap_or("");
    let mut ua_params = json!({ "userAgent": ua });
    if let Some(platform) = next.and_then(|e| e.platform.as_deref()) {
        ua_params["platform"] = Value::String(platform.to_string());
    }
    out.push(("Emulation.setUserAgentOverride", ua_params));
    // Network 域只在有附加请求头时开着：开着时 DevTools 会话会为页面缓存响应体，白占内存
    // （独立审查提出）。缓冲上限设 0，开着也不攒。
    let has_headers = |e: Option<&Emulation>| e.is_some_and(|e| !e.headers.is_empty());
    if let Some(e) = next.filter(|e| !e.headers.is_empty()) {
        let headers: Map<String, Value> = e
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        out.push((
            "Network.enable",
            json!({ "maxTotalBufferSize": 0, "maxResourceBufferSize": 0 }),
        ));
        out.push(("Network.setExtraHTTPHeaders", json!({ "headers": headers })));
    } else if has_headers(before) {
        out.push(("Network.setExtraHTTPHeaders", json!({ "headers": {} })));
        out.push(("Network.disable", json!({})));
    }
    out
}

/// 一句话说清当前模拟，给 Agent 的快照和工具回执用。
pub(crate) fn describe(emulation: Option<&Emulation>) -> String {
    let Some(e) = emulation else {
        return "未模拟（面板实际大小、WebView2 默认 UA）".into();
    };
    let mut parts = Vec::new();
    if let Some(m) = &e.metrics {
        let name = match m.device.as_str() {
            "desktop" => "桌面",
            "tablet" => "平板",
            "mobile" => "手机",
            _ => "自定义尺寸",
        };
        parts.push(format!(
            "模拟{name} {}×{}{}",
            m.width,
            m.height,
            if m.mobile { "（触屏）" } else { "" }
        ));
    }
    if e.user_agent.is_some() {
        parts.push("UA 已改".into());
    }
    if !e.header_names.is_empty() {
        parts.push(format!("附加请求头：{}", e.header_names.join("、")));
    }
    parts.join("，")
}

/// 快照开头的视口说明：让 Agent 先知道自己看到的是哪一版布局，再决定要不要模拟。
pub(crate) fn viewport_note(
    emulation: Option<&Emulation>,
    (inner_w, inner_h): (u32, u32),
) -> String {
    let state = describe(emulation);
    let narrow =
        emulation.and_then(|e| e.metrics.as_ref()).is_none() && inner_w > 0 && inner_w < 768;
    let hint = if narrow {
        "。宽度不到 768，响应式网站多半显示手机版布局；要测桌面版先调用 browser_emulate（device: \"desktop\"），SSR 网站按 UA 出页面的也用它改 UA"
    } else {
        ""
    };
    format!("视口：{inner_w}×{inner_h}（{state}）{hint}。\n\n")
}

/// 按顺序执行一组 CDP 调用，等每一步的结果；任何一步失败就停下报错。
/// 回调在主线程，调用方必须在非主线程（broker 线程或 blocking 线程）上等。
/// `check` 在每一步发出前调用：用户中途接管就立刻停下，已发出的部分由调用方回滚。
#[cfg(windows)]
pub(crate) fn run_calls(
    webview: &tauri::Webview,
    calls: Vec<(&'static str, Value)>,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    use std::sync::mpsc;
    use std::time::Duration;
    for (method, params) in calls {
        check()?;
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        let fail = tx.clone();
        let params = params.to_string();
        webview
            .with_webview(move |platform| {
                use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
                use windows::core::HSTRING;
                let started = (|| -> windows::core::Result<()> {
                    let core = unsafe { platform.controller().CoreWebView2()? };
                    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                        move |result, json| {
                            let _ = tx
                                .send(result.map_err(|e| format!("{method} 失败：{e}（{json}）")));
                            Ok(())
                        },
                    ));
                    let name = HSTRING::from(method);
                    let body = HSTRING::from(params.as_str());
                    unsafe { core.CallDevToolsProtocolMethod(&name, &body, &handler) }
                })();
                if let Err(error) = started {
                    let _ = fail.send(Err(format!("{method} 没能发出：{error}")));
                }
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| format!("{method} 超时没有回应"))??;
    }
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn run_calls(
    _webview: &tauri::Webview,
    _calls: Vec<(&'static str, Value)>,
    _check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

/// 面板拖宽、收起时重算缩放：只发不等（在 set_bounds 里，不能为它卡住位置同步）。
/// `current` 在主线程上**执行时**才读目标尺寸和视口，不在发出时捕获：和正在进行的
/// set_emulation 交错时，排在后面的补发不会把刚换掉（或刚还原）的旧尺寸写回去（独立审查发现）。
/// 调用方不能持有页签状态锁：`current` 要拿这把锁。
#[cfg(windows)]
pub(crate) fn refit(
    webview: &tauri::Webview,
    current: impl FnOnce() -> Option<(Metrics, (f64, f64))> + Send + 'static,
) {
    let _ = webview.with_webview(move |platform| {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        use windows::core::HSTRING;
        let Some((metrics, view)) = current() else {
            return;
        };
        let Ok(core) = (unsafe { platform.controller().CoreWebView2() }) else {
            return;
        };
        for (method, params) in metrics_calls(Some(&metrics), view) {
            let handler =
                CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
            let name = HSTRING::from(method);
            let body = HSTRING::from(params.to_string().as_str());
            let _ = unsafe { core.CallDevToolsProtocolMethod(&name, &body, &handler) };
        }
    });
}

#[cfg(not(windows))]
pub(crate) fn refit(
    _webview: &tauri::Webview,
    _current: impl FnOnce() -> Option<(Metrics, (f64, f64))> + Send + 'static,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_fill_size_touch_and_ua() {
        let e = parse(&json!({ "device": "mobile" })).unwrap().unwrap();
        let m = e.metrics.as_ref().unwrap();
        assert_eq!(
            (m.device.as_str(), m.width, m.height, m.mobile),
            ("mobile", 390, 844, true)
        );
        assert!(e.user_agent.as_deref().unwrap().contains("iPhone"));
        let d = parse(&json!({ "device": "desktop" })).unwrap().unwrap();
        assert_eq!(d.metrics.as_ref().unwrap().width, 1280);
        // 桌面不改 UA：WebView2 的默认 UA 本来就是桌面 Chrome/Edge。
        assert_eq!(d.user_agent, None);
        let t = parse(&json!({ "device": "tablet", "user_agent": "X" }))
            .unwrap()
            .unwrap();
        assert_eq!(t.user_agent.as_deref(), Some("X"), "显式 UA 优先于预设");
    }

    #[test]
    fn custom_size_and_partial_override() {
        let e = parse(&json!({ "device": "mobile", "width": 412 }))
            .unwrap()
            .unwrap();
        let m = e.metrics.unwrap();
        assert_eq!(
            (m.device.as_str(), m.width, m.height, m.mobile),
            ("custom", 412, 844, true)
        );
        let only_h = parse(&json!({ "height": 1000 }))
            .unwrap()
            .unwrap()
            .metrics
            .unwrap();
        assert_eq!(
            (only_h.width, only_h.height, only_h.mobile),
            (1280, 1000, false)
        );
        let ua_only = parse(&json!({ "user_agent": "Bot/1.0" })).unwrap().unwrap();
        assert!(ua_only.metrics.is_none(), "只改 UA 不动视口");
    }

    #[test]
    fn bad_arguments_are_rejected_with_reasons() {
        for (args, needle) in [
            (json!({}), "至少给一个参数"),
            (json!({ "device": "watch" }), "device 只能是"),
            (json!({ "width": 50 }), "200～4096"),
            (json!({ "width": 800.5 }), "整数"),
            (json!({ "device": "reset", "width": 800 }), "不能和其他参数"),
            (json!({ "user_agent": "a\nb" }), "不含换行"),
            (json!({ "headers": { "Host": "x" } }), "不能改"),
            (json!({ "headers": { "X-A": "1\r\nX-B: 2" } }), "不含换行"),
            (json!({ "headers": { "a b": "1" } }), "无效"),
            (json!({ "headers": "x" }), "要是对象"),
            (json!({ "headers": { "X-A": "1", "x-a": "2" } }), "重复"),
            (json!({ "headers": { "User-Agent": "x" } }), "user_agent 参数"),
        ] {
            let error = parse(&args).unwrap_err();
            assert!(error.contains(needle), "{args} → {error}");
        }
        assert_eq!(parse(&json!({ "device": "reset" })).unwrap(), None);
    }

    #[test]
    fn reload_only_when_request_side_changes() {
        let mobile = parse(&json!({ "device": "mobile" })).unwrap();
        let wide = parse(&json!({ "width": 1440 })).unwrap();
        let wider = parse(&json!({ "width": 1920 })).unwrap();
        let header = parse(&json!({ "width": 1920, "headers": { "X-Test": "1" } })).unwrap();
        assert!(needs_reload(None, mobile.as_ref()), "换了 UA");
        assert!(needs_reload(mobile.as_ref(), None), "还原 UA");
        assert!(!needs_reload(wide.as_ref(), wider.as_ref()), "只改尺寸");
        assert!(needs_reload(wider.as_ref(), header.as_ref()), "加了请求头");
    }

    #[test]
    fn scale_fits_the_panel_but_never_enlarges() {
        let desktop = parse(&json!({ "device": "desktop" }))
            .unwrap()
            .unwrap()
            .metrics
            .unwrap();
        assert!((fit_scale(&desktop, (640.0, 900.0)) - 0.5).abs() < 1e-9);
        assert_eq!(fit_scale(&desktop, (2000.0, 2000.0)), 1.0);
        let mobile = parse(&json!({ "device": "mobile" }))
            .unwrap()
            .unwrap()
            .metrics
            .unwrap();
        assert!(
            (fit_scale(&mobile, (600.0, 422.0)) - 0.5).abs() < 1e-9,
            "按较紧的一边算"
        );
    }

    #[test]
    fn reset_clears_every_override() {
        let before = parse(&json!({ "device": "mobile", "headers": { "X-Test": "1" } })).unwrap();
        let calls = calls(before.as_ref(), None, (600.0, 800.0));
        let names: Vec<_> = calls.iter().map(|(m, _)| *m).collect();
        assert!(names.contains(&"Emulation.clearDeviceMetricsOverride"));
        let ua = &calls
            .iter()
            .find(|(m, _)| *m == "Emulation.setUserAgentOverride")
            .unwrap()
            .1;
        assert_eq!((ua["userAgent"].as_str(), ua.get("platform")), (Some(""), None));
        // 之前有附加头：清空后关掉 Network 域。
        assert_eq!(
            &names[names.len() - 2..],
            &["Network.setExtraHTTPHeaders", "Network.disable"]
        );
    }

    #[test]
    fn network_domain_is_only_enabled_for_extra_headers() {
        let mobile = parse(&json!({ "device": "mobile" })).unwrap();
        let network = |c: Vec<(&'static str, Value)>| c.iter().any(|(m, _)| m.starts_with("Network."));
        assert!(!network(calls(None, mobile.as_ref(), (600.0, 800.0))));
        assert!(!network(calls(mobile.as_ref(), None, (600.0, 800.0))));
        let set = calls(None, mobile.as_ref(), (600.0, 800.0));
        let ua = &set
            .iter()
            .find(|(m, _)| *m == "Emulation.setUserAgentOverride")
            .unwrap()
            .1;
        assert_eq!(ua["platform"], "iPhone", "预设 UA 带上 navigator.platform");
        let custom = parse(&json!({ "device": "mobile", "user_agent": "X" }))
            .unwrap()
            .unwrap();
        assert_eq!(custom.platform, None, "自己给的 UA 不配 platform");
    }

    #[test]
    fn header_values_never_reach_the_frontend() {
        let e = parse(&json!({ "headers": { "Authorization": "Bearer secret" } }))
            .unwrap()
            .unwrap();
        let wire = serde_json::to_string(&e).unwrap();
        assert!(
            wire.contains("Authorization") && !wire.contains("secret"),
            "{wire}"
        );
    }

    #[test]
    fn viewport_note_warns_only_for_narrow_unemulated_views() {
        let narrow = viewport_note(None, (520, 800));
        assert!(
            narrow.starts_with("视口：520×800") && narrow.contains("browser_emulate"),
            "{narrow}"
        );
        assert!(!viewport_note(None, (1024, 800)).contains("browser_emulate"));
        let mobile = parse(&json!({ "device": "mobile", "headers": { "X-Test": "1" } })).unwrap();
        let note = viewport_note(mobile.as_ref(), (390, 844));
        assert!(
            note.contains("模拟手机 390×844")
                && note.contains("UA 已改")
                && note.contains("X-Test"),
            "{note}"
        );
        assert!(!note.contains("不到 768"), "已经在模拟就不再提示");
    }
}
