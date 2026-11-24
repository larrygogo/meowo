//! 对话里的 HTML 视图卡（MCP 工具 show_html，以及 offer_choices 方案的 html 预览）。任务 20261124-html-cards。
//!
//! agent 给一段 HTML（或本机 .html 文件），Meowo 存一份快照 `<数据根>/views/<指纹>.html`，对话里把它
//! 嵌成一块能点、能动的小页面。存快照而不是记原路径：之后 agent 改原文件，卡片仍是当时那份（与 send_file
//! 同一纪律）。
//!
//! 页面不走主窗口的源：srcdoc iframe 会继承父页 CSP（`default-src 'self'`），内联脚本跑不起来；放宽主窗口
//! script-src 会削弱整个应用。所以桌面走自定义协议 `meowo-view`、手机走远程服务的 `/view/` 路由，两边都由
//! [`serve`] 出同一份响应：响应头 CSP 带 `sandbox allow-scripts`（即使被直接当顶层打开也是不透明源，碰不到
//! Meowo 的存储和接口），`connect-src 'none'`、`form-action 'none'` 堵住主动回传。沙箱约定照 MCP Apps
//! （rich-display 调研第 7 节），消息名也用它的子集，以后接第三方时少改。
//!
//! 页面碰不到 Tauri IPC（2026-11-24 开发版 + WebView2 远程调试实测，探针页见任务目录 progress.md）：
//! - Windows：WebView2 不支持「初始化脚本只注入主框架」，iframe 里有带调用密钥的 `__TAURI_INTERNALS__.invoke`，
//!   而且属性不可删（实测清不掉）。挡住它的是三层：本页 CSP `connect-src 'none'` 拦掉 fetch ipc.localhost；
//!   sandbox 下请求带 `Origin: null`，Tauri 解析 Origin 失败直接拒；WebView2 不把子框架的 postMessage 交给
//!   主窗口处理器。实测 invoke 与伪造原生消息发的事件，主窗口都没收到。**别放宽 connect-src，别给 allow-same-origin。**
//! - macOS：wry 收任何框架的消息，Tauri 也把自定义协议算本地来源；但带密钥的脚本只注入主框架（wry 遵守
//!   forMainFrameOnly），iframe 发来的消息在密钥校验处就被拒（tauri webview/mod.rs on_message）。未经本机实测。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// 回执里给前端认的视图卡摘要行开头（ViewCard.tsx 认这个）。
pub(crate) const VIEW_MARKER: &str = "[View: ";
/// 单页上限：内联了图片和库的演示页也就几百 KB；再大多半是把整个构建产物塞进来了。
pub(crate) const MAX_HTML_BYTES: u64 = 2 * 1024 * 1024;
/// 快照目录总量上限：超出按最近使用时间删最旧的，被删的卡片显示「已失效」。
const TOTAL_CAP: u64 = 256 * 1024 * 1024;
const TITLE_MAX: usize = 80;
/// agent 指定高度的范围：太矮看不见内容，太高一张卡占满好几屏。
const HEIGHT_MIN: u64 = 80;
const HEIGHT_MAX: u64 = 1600;
/// 指纹取前 32 个十六进制字符（128 位）：够唯一，地址也不至于太长。
const ID_LEN: usize = 32;

/// 桌面自定义协议名：Windows 上页面地址是 `http://meowo-view.localhost/<id>.html`，macOS 是
/// `meowo-view://localhost/<id>.html`（前端用 convertFileSrc 拼，两种都由它处理）。
pub(crate) const SCHEME: &str = "meowo-view";
/// 桌面上谁能嵌视图页：Meowo 主窗口的源（Windows 正式版 http(s)://tauri.localhost、macOS tauri://localhost、
/// 开发版 devUrl）。别的源（面板浏览器里打开的网站）嵌不进来。
pub(crate) const DESKTOP_ANCESTORS: &str =
    "http://tauri.localhost https://tauri.localhost tauri://localhost http://localhost:1268";

pub(crate) fn views_root() -> PathBuf {
    crate::setup::meowo_dir().join("views")
}

pub(crate) fn show_html(args: &Value) -> Result<Value, String> {
    show_into(&views_root(), args).map(|text| json!({ "text": text }))
}

fn show_into(root: &Path, args: &Value) -> Result<String, String> {
    let title = args
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty())
        .map(|t| t.chars().take(TITLE_MAX).collect::<String>())
        .ok_or("没给 title：写一个几个字的标题，比如「按钮动效对比」")?;
    let height = match args.get("height") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|h| (HEIGHT_MIN..=HEIGHT_MAX).contains(h))
                .ok_or(format!("height 要是 {HEIGHT_MIN}~{HEIGHT_MAX} 之间的整数（像素）；不给就跟着内容高度走"))?,
        ),
    };
    let (id, bytes) = store_source(root, args)?;
    let card = json!({ "title": title, "id": id, "bytes": bytes, "height": height });
    Ok(format!(
        "视图已显示给用户：「{title}」（{}）。页面嵌在对话里，用户可以直接点、放大看；页面里调 meowo.send(text) 会把一句话放进用户的输入框，由用户决定发不发。之后改了内容要重新调用。\n{VIEW_MARKER}{card}]",
        crate::chat_files::human_size(bytes)
    ))
}

/// 从参数里取 HTML（`html` 文本或 `path` 本机文件，二选一）存成快照，返回（指纹, 字节数）。
pub(crate) fn store_source(root: &Path, args: &Value) -> Result<(String, u64), String> {
    let bytes = load_source(args)?;
    store_loaded(root, &bytes)
}

/// 只读取、校验，不落盘：方案卡要等所有方案都检查通过再一起存，中途失败不留半套快照。
pub(crate) fn load_source(args: &Value) -> Result<Vec<u8>, String> {
    let html = args.get("html").and_then(Value::as_str).filter(|s| !s.trim().is_empty());
    let path = args.get("path").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let bytes = match (html, path) {
        (Some(_), Some(_)) => return Err("html 和 path 只给一个：直接给页面文本用 html，给本机文件用 path".into()),
        (None, None) => return Err("没给页面：html 填完整的 HTML 文本，或 path 填本机 .html 文件的路径".into()),
        (Some(text), None) => {
            if text.len() as u64 > MAX_HTML_BYTES {
                return Err(too_big(text.len() as u64));
            }
            text.as_bytes().to_vec()
        }
        (None, Some(raw)) => read_html_file(raw)?,
    };
    if std::str::from_utf8(&bytes).is_err() {
        return Err("页面不是 UTF-8 文本：show_html 只收 HTML 文本，图片用 send_image、其他文件用 send_file".into());
    }
    Ok(bytes)
}

pub(crate) fn store_loaded(root: &Path, bytes: &[u8]) -> Result<(String, u64), String> {
    std::fs::create_dir_all(root).map_err(|e| format!("建视图目录失败：{e}"))?;
    let id = store_bytes(root, bytes)?;
    prune(root, TOTAL_CAP, &id);
    Ok((id, bytes.len() as u64))
}

fn too_big(len: u64) -> String {
    format!(
        "页面有 {}，超过 {} 的上限。把大图换成小图或 SVG、别内联整个构建产物；真要看完整站点，起 dev server 让用户在浏览器里看",
        crate::chat_files::human_size(len),
        crate::chat_files::human_size(MAX_HTML_BYTES)
    )
}

fn read_html_file(raw: &str) -> Result<Vec<u8>, String> {
    if raw.contains("://") {
        return Err("path 只收本机文件，不接受网址。网上的页面别嵌进对话，给用户一个链接就行".into());
    }
    // 网络共享路径在碰文件系统之前就拒（免逐次确认的工具不许被一句注入骗去连 SMB，与 send_file 同一条线）。
    if !meowo_agent::images::is_local_absolute(raw) {
        if raw.starts_with("\\\\") || raw.starts_with("//") {
            return Err(format!("path 只收本机磁盘上的文件，不接受网络共享路径：{raw}"));
        }
        return Err(format!("path 要是本机的绝对路径：{raw}"));
    }
    let path = Path::new(raw);
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    if !matches!(ext.as_deref(), Some("html" | "htm")) {
        return Err(format!(
            "path 要是 .html 文件：{raw}。图片用 send_image，其他文件用 send_file"
        ));
    }
    let meta = std::fs::metadata(path).map_err(|_| format!("找不到这个文件：{raw}"))?;
    if meta.is_dir() {
        return Err(format!("这是一个文件夹：{raw}。path 要指向一个 .html 文件"));
    }
    if meta.len() > MAX_HTML_BYTES {
        return Err(too_big(meta.len()));
    }
    std::fs::read(path).map_err(|e| format!("读不了这个文件：{raw}（{e}）"))
}

/// 按内容指纹存 `<id>.html`；同内容已存过只刷新修改时间（回收按它算新旧）。先写临时文件再改名，
/// 中途崩了不会留下半截页面被当成快照。
fn store_bytes(root: &Path, bytes: &[u8]) -> Result<String, String> {
    let digest = Sha256::digest(bytes);
    let id: String = digest.iter().map(|b| format!("{b:02x}")).collect::<String>()[..ID_LEN].to_string();
    let target = root.join(format!("{id}.html"));
    if target.is_file() {
        let _ = std::fs::File::options()
            .append(true)
            .open(&target)
            .and_then(|f| f.set_modified(std::time::SystemTime::now()));
        return Ok(id);
    }
    let temp = root.join(format!(".incoming-{id}-{}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(|e| format!("保存页面失败：{e}"))?;
    std::fs::rename(&temp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("保存页面失败：{e}")
    })?;
    Ok(id)
}

/// 总量超过上限时按修改时间删最旧的页面，刚存的那份不删；顺手清掉一小时前的半截临时文件。
fn prune(root: &Path, cap: u64, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let keep = format!("{keep}.html");
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if name.starts_with(".incoming-") {
            if modified.elapsed().is_ok_and(|age| age.as_secs() > 3600) {
                let _ = std::fs::remove_file(entry.path());
            }
            continue;
        }
        if meta.is_file() && name.ends_with(".html") {
            files.push((modified, meta.len(), name, entry.path()));
        }
    }
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, size, name, path) in files {
        if total <= cap {
            break;
        }
        if name == keep {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

/// 视图指纹必须是 32 位小写十六进制：协议和手机路由都只按它拼文件名，杜绝 `..`、盘符、分隔符。
pub(crate) fn valid_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// 从请求路径（`/<id>.html` 或 `<id>.html`）取出指纹；不合规返回 None。
pub(crate) fn id_from_path(path: &str) -> Option<&str> {
    let name = path.trim_start_matches('/');
    let id = name.strip_suffix(".html").unwrap_or(name);
    valid_id(id).then_some(id)
}

/// 一份可以直接回给浏览器的响应：状态码、头、正文。桌面协议和手机路由共用。
pub(crate) struct Served {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// 页面 CSP。`frame_ancestors` 由调用方给：桌面是 Meowo 主窗口的源，手机是 `'self'`。
/// - `sandbox allow-scripts`：即使被直接打开成顶层页面，也是不透明源，读不到 Meowo 的存储、调不到接口；
///   iframe 那一侧另外也只给 allow-scripts，两道都要破才出得去。
/// - 脚本 / 样式只许内联和 https（agent 常从 CDN 引图表库）；图片 / 字体 / 媒体许 data: blob: https:。
/// - `connect-src 'none'`、`form-action 'none'`：不能 fetch / XHR / WebSocket / 提交表单往外送东西。
/// - `frame-src 'none'`：页面里不能再嵌别的站。
pub(crate) fn csp(frame_ancestors: &str) -> String {
    format!(
        "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https: blob:; style-src 'unsafe-inline' https:; \
img-src data: blob: https:; font-src data: https:; media-src data: blob: https:; worker-src blob:; \
connect-src 'none'; form-action 'none'; frame-src 'none'; base-uri 'none'; \
frame-ancestors {frame_ancestors}; sandbox allow-scripts"
    )
}

/// 按指纹出页面：找不到（被回收、指纹不对）给一张「已失效」占位页，由它通知宿主卡片换成失效态
/// （文案在前端按界面语言出，这里不写死中文或英文）。
pub(crate) fn serve(root: &Path, id: Option<&str>, frame_ancestors: &str) -> Served {
    let found = id.and_then(|id| std::fs::read(root.join(format!("{id}.html"))).ok());
    let (status, body) = match found {
        Some(bytes) => (200, page(&bytes)),
        None => (404, missing_page()),
    };
    Served {
        status,
        headers: vec![
            ("content-type", "text/html; charset=utf-8".into()),
            // 页面是 agent 写的任意内容：不许浏览器猜类型，也不进任何缓存（手机地址带每页口令）。
            ("x-content-type-options", "nosniff".into()),
            ("cache-control", "no-store".into()),
            ("referrer-policy", "no-referrer".into()),
            ("content-security-policy", csp(frame_ancestors)),
        ],
        body,
    }
}

/// 注进每个页面最前面的桥：
/// - 报高度（ResizeObserver → `ui/notifications/size-changed`），卡片跟着内容变高；
/// - 主题：地址里的 `theme=` 与宿主后续发来的 `ui/notifications/host-context-changed`，写到
///   `<html data-theme>` 和 `color-scheme` 上，页面用 `prefers-color-scheme` 或 `[data-theme=dark]` 都能跟；
/// - `meowo.send(text)` → `ui/message`（宿主只放进输入框，不替用户发）；http(s) 链接点击 → `ui/open-link`；
/// - 不透明源下访问 localStorage / sessionStorage 会抛 SecurityError，agent 写的演示页常用它存状态，
///   换成内存版，页面不至于一打开就报错停住。
const BRIDGE: &str = concat!("<script>", include_str!("view_bridge.js"), "</script>");

const HEAD: &str = r#"<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark">"#;

/// 把桥和基础 meta 插到页面最前面。页面以 `<!doctype …>` 开头时插在它后面（插在前面会掉进怪异模式）；
/// 没有 doctype 的片段（agent 常只给一个 `<div>`）补一个。插在 `<html>` 之前的 meta / script 会被解析器
/// 自动归进 head，片段和完整页面都成立。
fn page(source: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(source);
    let body = text.trim_start_matches('\u{feff}');
    let trimmed = body.trim_start();
    let (doctype, rest) = if trimmed.len() >= 9 && trimmed[..9].eq_ignore_ascii_case("<!doctype") {
        match trimmed.find('>') {
            Some(end) => (&trimmed[..=end], &trimmed[end + 1..]),
            None => ("<!doctype html>", trimmed),
        }
    } else {
        ("<!doctype html>", body)
    };
    format!("{doctype}{HEAD}{BRIDGE}{rest}").into_bytes()
}

fn missing_page() -> Vec<u8> {
    format!(
        "<!doctype html>{HEAD}<script>try{{parent.postMessage({{jsonrpc:\"2.0\",method:\"meowo/view-missing\",params:{{}}}},\"*\")}}catch(e){{}}</script>"
    )
    .into_bytes()
}

/// 手机地址里的每页口令：只对这一页有效，页面脚本读得到自己的地址也换不来别的东西。
/// `secret` 是远程服务每次启动新发的 /file 降级凭据，重启即全部作废。指纹定长 32 位十六进制，
/// 拼接后再做摘要不存在长度扩展可利用的余地。
pub(crate) fn page_key(secret: &str, id: &str) -> String {
    let digest = Sha256::digest(format!("meowo-view:{secret}:{id}").as_bytes());
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("meowo-chat-views-{tag}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn card(text: &str) -> Value {
        let line = text.lines().last().unwrap();
        serde_json::from_str(line.strip_prefix(VIEW_MARKER).unwrap().strip_suffix(']').unwrap()).unwrap()
    }

    #[test]
    fn html_text_is_snapshotted_and_card_carries_id() {
        let root = temp("text").join("views");
        let text = show_into(&root, &json!({ "title": " 按钮\n动效 ", "html": "<button>hi</button>", "height": 240 })).unwrap();
        let c = card(&text);
        assert_eq!(c["title"], "按钮 动效");
        assert_eq!(c["height"], 240);
        let id = c["id"].as_str().unwrap();
        assert!(valid_id(id));
        assert_eq!(std::fs::read_to_string(root.join(format!("{id}.html"))).unwrap(), "<button>hi</button>");
    }

    #[test]
    fn path_snapshot_does_not_follow_later_edits() {
        let tmp = temp("path");
        let root = tmp.join("views");
        let src = tmp.join("demo.html");
        std::fs::write(&src, "<p>v1</p>").unwrap();
        let text = show_into(&root, &json!({ "title": "t", "path": src.to_string_lossy() })).unwrap();
        std::fs::write(&src, "<p>v2</p>").unwrap();
        let id = card(&text)["id"].as_str().unwrap().to_string();
        assert_eq!(std::fs::read_to_string(root.join(format!("{id}.html"))).unwrap(), "<p>v1</p>");
    }

    #[test]
    fn bad_arguments_explain_what_to_do() {
        let tmp = temp("bad");
        let root = tmp.join("views");
        let err = |args: Value| show_into(&root, &args).unwrap_err();
        assert!(err(json!({ "html": "<p>" })).contains("title"));
        assert!(err(json!({ "title": "t" })).contains("没给页面"));
        assert!(err(json!({ "title": "t", "html": "<p>", "path": "C:/a.html" })).contains("只给一个"));
        assert!(err(json!({ "title": "t", "html": "<p>", "height": 5 })).contains("height"));
        assert!(err(json!({ "title": "t", "path": "https://x.dev/a.html" })).contains("网址"));
        assert!(err(json!({ "title": "t", "path": "\\\\server\\share\\a.html" })).contains("网络共享"));
        assert!(err(json!({ "title": "t", "path": "a.html" })).contains("绝对路径"));
        let png = tmp.join("a.png");
        std::fs::write(&png, b"x").unwrap();
        assert!(err(json!({ "title": "t", "path": png.to_string_lossy() })).contains(".html"));
        let missing = tmp.join("none.html");
        assert!(err(json!({ "title": "t", "path": missing.to_string_lossy() })).contains("找不到"));
        let big = "x".repeat(MAX_HTML_BYTES as usize + 1);
        assert!(err(json!({ "title": "t", "html": big })).contains("上限"));
        assert!(!root.exists() || std::fs::read_dir(&root).unwrap().next().is_none(), "失败不留快照");
    }

    #[test]
    fn page_injects_bridge_after_doctype_and_wraps_fragments() {
        let full = String::from_utf8(page(b"\xef\xbb\xbf  <!DOCTYPE html><html><body>x</body></html>")).unwrap();
        assert!(full.starts_with("<!DOCTYPE html><meta charset"), "{full}");
        assert!(full.contains("ui/notifications/size-changed"));
        let frag = String::from_utf8(page(b"<div>hi</div>")).unwrap();
        assert!(frag.starts_with("<!doctype html><meta charset"));
        assert!(frag.ends_with("<div>hi</div>"));
    }

    #[test]
    fn serve_only_reads_valid_ids_and_marks_missing() {
        let root = temp("serve");
        let id = store_bytes(&root, b"<p>ok</p>").unwrap();
        let ok = serve(&root, id_from_path(&format!("/{id}.html")), "'self'");
        assert_eq!(ok.status, 200);
        let csp = &ok.headers.iter().find(|h| h.0 == "content-security-policy").unwrap().1;
        assert!(csp.contains("sandbox allow-scripts") && csp.contains("connect-src 'none'"), "{csp}");
        assert!(!csp.contains("allow-same-origin"));
        assert!(id_from_path("/../secret.html").is_none());
        assert!(id_from_path("/C:/x.html").is_none());
        assert!(id_from_path(&format!("/{}.html", id.to_uppercase())).is_none());
        let gone = serve(&root, id_from_path(&format!("/{}.html", "0".repeat(32))), "'self'");
        assert_eq!(gone.status, 404);
        assert!(String::from_utf8(gone.body).unwrap().contains("meowo/view-missing"));
    }

    #[test]
    fn prune_drops_oldest_but_keeps_the_new_page() {
        let root = temp("prune");
        let old = store_bytes(&root, &[b'a'; 600]).unwrap();
        let old_path = root.join(format!("{old}.html"));
        std::fs::File::options()
            .append(true)
            .open(&old_path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        let new = store_bytes(&root, &[b'b'; 600]).unwrap();
        prune(&root, 1000, &new);
        assert!(!old_path.exists());
        assert!(root.join(format!("{new}.html")).exists());
    }

    #[test]
    fn page_key_is_per_page_and_per_secret() {
        let a = page_key("s1", &"a".repeat(32));
        assert_eq!(a.len(), 32);
        assert_ne!(a, page_key("s1", &"b".repeat(32)));
        assert_ne!(a, page_key("s2", &"a".repeat(32)));
    }
}
