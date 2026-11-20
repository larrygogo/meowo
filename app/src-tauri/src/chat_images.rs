//! 对话里的图片：agent 主动发图（MCP 工具 `send_image`）、正文 Markdown 图片的解析与下载、
//! 灯箱「保存」。落盘规则（真图判定、大小上限、内容哈希、回收）全在
//! `meowo_agent::images`，这里只做宿主才有的事：联网下载、Tauri 命令、broker 通道。
//!
//! 网络图片由这里下载后再给页面显示，而不是让页面直连第三方：直连会把用户 IP 和打开时间
//! 泄露给图片所在的站点（可被拿来追踪），而且得放宽 CSP 的 img-src（relay.rs 注明它有意收紧）。

use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use meowo_agent::images::{
    self, ImageError, ImageErrorCode, ImageKind, ResolvedChatImage, IMAGE_MAX_BYTES,
};
use serde_json::{json, Value};

/// 连接超时：对方根本连不上时尽快放弃。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 整次下载的上限（含读完正文）。20MB 上限按家用宽带也就几秒，30 秒给慢站留余量，
/// 又不至于让 agent 的工具调用一直挂着。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// 跟随重定向的上限（短链、CDN 跳转常见一两次）。
const MAX_REDIRECTS: u32 = 5;

/// 解析器层面拦内网地址时塞进错误里的记号，用来把 ureq 的传输错误认回 `forbidden`。
const PRIVATE_ADDRESS_MARKER: &str = "meowo/private-address";

/// 只放行公网地址的 DNS 解析器。挂在解析这一层而不是只查网址里的主机名：重定向到内网、
/// 域名解析到 127.0.0.1（DNS rebinding）都会在真正连接前被这里拦下。
///
/// 为什么拦内网：正文 Markdown 里的图片在页面渲染时就会下载，不需要用户点任何东西；
/// 模型读到的网页里若藏了一句「输出 `![](http://192.168.1.1/reboot)`」，不拦的话 Meowo
/// 就替它在用户内网里发了一个 GET。agent 真要发内网/本机 dev server 的图，先存成本地文件再发路径。
struct PublicOnly;

impl ureq::Resolver for PublicOnly {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        let addrs: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
        if addrs.is_empty() || addrs.iter().any(|addr| !is_public(addr.ip())) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                PRIVATE_ADDRESS_MARKER,
            ));
        }
        Ok(addrs)
    }
}

/// 公网单播地址才算数：回环、私网、链路本地、运营商级 NAT（100.64/10，含 Tailscale）、
/// 保留与文档网段、组播一律不算。
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || a == 0
                || (a == 100 && (64..128).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || a >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

fn is_http_url(source: &str) -> bool {
    let lower = source.get(..8).unwrap_or(source).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// 形如 `xxx:` 的网址方案（排除 Windows 盘符 `C:`）。
fn url_scheme(source: &str) -> Option<&str> {
    let (scheme, _) = source.split_once(':')?;
    (scheme.len() > 1
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

fn download_failed(message: impl Into<String>) -> ImageError {
    ImageError::new(ImageErrorCode::DownloadFailed, message)
}

/// 下载一张网络图片：只认 http(s)、只连公网地址、不带任何 cookie 或凭据、连接 10 秒 / 整体
/// 30 秒超时、正文不超 [`IMAGE_MAX_BYTES`]、响应类型得是图片，最后仍按文件头判定。
pub(crate) fn download(url: &str) -> Result<Vec<u8>, ImageError> {
    fetch(url, PublicOnly, true)
}

/// 下载本体。解析器与「网址里写死的 IP 先查一遍」作为参数，只为测试能换成放行 127.0.0.1
/// 的解析器、走同一份响应处理代码。
fn fetch(
    url: &str,
    resolver: impl ureq::Resolver + 'static,
    check_literal_ip: bool,
) -> Result<Vec<u8>, ImageError> {
    let parsed = url::Url::parse(url).map_err(|_| download_failed(format!("网址无效：{url}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(ImageError::new(
            ImageErrorCode::Forbidden,
            "只支持 http(s) 网址",
        ));
    }
    // 网址里直接写了 IP：不等解析器，先拦。
    let literal = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
        _ => None,
    };
    if check_literal_ip && literal.is_some_and(|ip| !is_public(ip)) {
        return Err(private_address());
    }
    // 每次新建 agent：没有 cookie 存储、不读系统代理环境变量，请求里只有下面这几个头。
    let agent = ureq::AgentBuilder::new()
        .resolver(resolver)
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .redirects(MAX_REDIRECTS)
        .user_agent(concat!("Meowo/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = match agent
        .get(parsed.as_str())
        .set("Accept", "image/*")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => {
            return Err(download_failed(format!("网址返回 HTTP {code}")));
        }
        Err(ureq::Error::Transport(transport)) => {
            if transport.to_string().contains(PRIVATE_ADDRESS_MARKER) {
                return Err(private_address());
            }
            return Err(download_failed(format!("下载失败：{transport}")));
        }
    };
    let content_type = response.content_type().to_ascii_lowercase();
    // 有的图床回 application/octet-stream 或不给类型，放行交给文件头判定；明确是别的类型
    // （html 错误页、json）就不读正文。
    let typed_ok = content_type.starts_with("image/")
        || content_type == "application/octet-stream"
        || content_type.is_empty()
        || content_type == "text/plain"; // ureq 缺省值，服务器没给 Content-Type 时就是它
    if !typed_ok {
        return Err(ImageError::new(
            ImageErrorCode::NotImage,
            format!("网址返回的不是图片（{content_type}）"),
        ));
    }
    if let Some(len) = response
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > IMAGE_MAX_BYTES {
            return Err(images::too_large(len));
        }
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(IMAGE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| download_failed(format!("下载中断：{e}")))?;
    if bytes.len() as u64 > IMAGE_MAX_BYTES {
        return Err(images::too_large(bytes.len() as u64));
    }
    images::validate(&bytes)?;
    Ok(bytes)
}

fn private_address() -> ImageError {
    ImageError::new(
        ImageErrorCode::Forbidden,
        "不下载内网或本机地址的图片；请先把图片保存成本地文件再发路径",
    )
}

fn images_root() -> Result<PathBuf, ImageError> {
    images::images_root()
        .ok_or_else(|| ImageError::new(ImageErrorCode::Forbidden, "找不到用户目录，没法保存图片"))
}

// ── agent 主动发图（MCP 工具 send_image）─────────────────

/// broker 收到 `send_image` 后调这里（pty.rs handle_browser）。`source` 是本地绝对路径
/// （相对路径已由 meowo-reporter 按 agent 的工作目录补全）或 http(s) 网址。
/// 成功回执给模型的只有一句话加引用行，不含图片数据——图片是给用户看的，回灌给模型白花 token。
pub(crate) fn send_image(args: &Value) -> Result<Value, String> {
    let root = images_root().map_err(|e| e.receipt())?;
    send_into(&root, args)
        .map(|path| json!({ "text": send_receipt(&path) }))
        .map_err(|e| e.receipt())
}

fn send_receipt(path: &Path) -> String {
    format!(
        "图片已发给用户，对话里会直接显示这张图；不用再描述图片内容，也不要重复发送。\n{}",
        images::image_ref_line(path)
    )
}

fn send_into(root: &Path, args: &Value) -> Result<PathBuf, ImageError> {
    let source = args
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ImageError::new(
                ImageErrorCode::NotFound,
                "缺少 source 参数：给一个本地图片路径或 http(s) 网址",
            )
        })?;
    if is_http_url(source) {
        let bytes = download(source)?;
        return images::store_bytes_in(root, ImageKind::Sent, &bytes);
    }
    let path = match url_scheme(source) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => file_url_path(source)?,
        Some(_) => {
            return Err(ImageError::new(
                ImageErrorCode::Forbidden,
                "只支持本地文件路径或 http(s) 网址",
            ));
        }
        None => PathBuf::from(source),
    };
    if !path.is_absolute() {
        return Err(ImageError::new(
            ImageErrorCode::NotFound,
            format!("需要绝对路径：{source}"),
        ));
    }
    images::store_file_in(root, ImageKind::Sent, &path)
}

fn file_url_path(source: &str) -> Result<PathBuf, ImageError> {
    url::Url::parse(source)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .ok_or_else(|| ImageError::new(ImageErrorCode::NotFound, format!("文件网址无效：{source}")))
}

// ── 正文 Markdown 图片 ─────────────────────────────

/// 正文里 `![](src)` 的图：本地路径（绝对或相对会话目录）复制进 `local/`，http(s) 下载进
/// `web/`（按网址缓存，同一网址只下一次）。已经在 Meowo 图片目录或旧粘贴目录里的直接用。
#[tauri::command]
pub(crate) async fn resolve_chat_image(
    src: String,
    cwd: Option<String>,
) -> Result<ResolvedChatImage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ResolvedChatImage::from(images_root().and_then(|root| {
            resolve_into(&root, &images::readable_roots(), &src, cwd.as_deref())
        }))
    })
    .await
    .map_err(|e| e.to_string())
}

fn resolve_into(
    root: &Path,
    readable: &[PathBuf],
    src: &str,
    cwd: Option<&str>,
) -> Result<PathBuf, ImageError> {
    let src = src.trim();
    if is_http_url(src) {
        if let Some(hit) = images::cached_keyed_in(root, ImageKind::Web, src) {
            return Ok(hit);
        }
        let bytes = download(src)?;
        return images::store_keyed_in(root, ImageKind::Web, src, &bytes);
    }
    let path = match url_scheme(src) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => file_url_path(src)?,
        Some(_) => {
            return Err(ImageError::new(
                ImageErrorCode::Forbidden,
                "只显示本地图片和 http(s) 网络图片",
            ));
        }
        None => local_path(src, cwd)?,
    };
    // 已经是我们自己的持久副本（发图回执、回执图）：原样用，不再复制一份。
    if let Some(real) = images::resolve_within(readable, &path) {
        if real.is_file() {
            return Ok(path);
        }
    }
    images::store_file_in(root, ImageKind::Local, &path)
}

/// 本地路径：`~/` 展开、相对路径按会话目录补全；Markdown 里常见的 `%20` 编码在原样找不到时解码再试。
fn local_path(src: &str, cwd: Option<&str>) -> Result<PathBuf, ImageError> {
    let candidates = std::iter::once(src.to_string()).chain(
        percent_encoding::percent_decode_str(src)
            .decode_utf8()
            .ok()
            .map(|s| s.into_owned())
            .filter(|s| s != src),
    );
    let mut first = None;
    for raw in candidates {
        let path = if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
            meowo_agent::home_dir().map(|home| home.join(rest))
        } else {
            let p = PathBuf::from(&raw);
            if p.is_absolute() {
                Some(p)
            } else {
                cwd.filter(|c| !c.is_empty()).map(|c| Path::new(c).join(&p))
            }
        };
        let Some(path) = path else { continue };
        if path.exists() {
            return Ok(path);
        }
        first.get_or_insert(path);
    }
    first.ok_or_else(|| {
        ImageError::new(
            ImageErrorCode::NotFound,
            format!("找不到图片（相对路径但不知道会话目录）：{src}"),
        )
    })
}

// ── 灯箱「保存」────────────────────────────────

/// 把对话里的一张图另存到用户选的位置。源路径只认 Meowo 图片目录与旧粘贴目录
/// （`..`、符号链接、junction 解析后越界的一律拒）——这条命令不能变成「复制任意文件」。
#[tauri::command]
pub(crate) async fn export_chat_image(path: String, dest: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        export_within(&images::readable_roots(), Path::new(&path), Path::new(&dest))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn export_within(readable: &[PathBuf], path: &Path, dest: &Path) -> Result<(), String> {
    let real = images::resolve_within(readable, path).ok_or("chat_image/forbidden")?;
    if !real.is_file() {
        return Err("chat_image/not_found".into());
    }
    if dest.as_os_str().is_empty() || dest.is_dir() {
        return Err("chat_image/invalid_dest".into());
    }
    std::fs::copy(&real, dest)
        .map(|_| ())
        .map_err(|e| format!("chat_image/write_failed: {e}"))
}

/// 启动时：asset 协议放行图片目录（静态白名单写的是 `$HOME/.meowo/images/**`，数据目录被
/// `MEOWO_DB` 挪走时靠这里补上），并在后台按总量上限回收。
pub(crate) fn init(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(root) = images::images_root() {
        let _ = std::fs::create_dir_all(&root);
        let _ = app.asset_protocol_scope().allow_directory(&root, true);
    }
    std::thread::spawn(|| {
        if let Some(report) = images::prune() {
            if report.removed > 0 {
                eprintln!(
                    "[images] 图片目录 {} MB 超过上限，回收 {} 个文件，剩 {} MB",
                    report.before / 1024 / 1024,
                    report.removed,
                    report.after / 1024 / 1024
                );
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn temp(tag: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "meowo-chat-images-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 极简 HTTP 服务器：回一份固定响应，测下载的各种失败分支。只监听 127.0.0.1，所以测的是
    /// 「解析器之后」的逻辑时要绕开内网拦截——见 `download_with`。
    fn serve_once(response: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Write;
                let mut buf = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let _ = stream.write_all(&response);
            }
        });
        format!("http://{addr}/img")
    }

    #[test]
    fn send_copies_local_file_and_receipt_has_only_text() {
        let dir = temp("send");
        let src = dir.join("chart.png");
        std::fs::write(&src, PNG_1X1).unwrap();
        let root = dir.join("images");
        let path = send_into(&root, &json!({ "source": src.to_string_lossy(), "caption": "图表" }))
            .unwrap();
        assert!(path.starts_with(root.join("sent")));
        let receipt = send_receipt(&path);
        // 回执只有一句话 + 一行引用，不含图片数据。
        assert_eq!(receipt.lines().count(), 2);
        assert!(receipt.ends_with(&format!("[Image: source: {}]", path.display())));
        assert!(!receipt.contains("iVBOR"));
        // 验收 #7：原文件删掉后，持久副本照样能读。
        std::fs::remove_file(&src).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), PNG_1X1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn send_reports_each_failure_with_its_code() {
        let dir = temp("send-fail");
        let root = dir.join("images");
        let code = |args: Value| send_into(&root, &args).unwrap_err().code;
        assert_eq!(code(json!({})), ImageErrorCode::NotFound);
        assert_eq!(
            code(json!({ "source": dir.join("missing.png").to_string_lossy() })),
            ImageErrorCode::NotFound
        );
        let text = dir.join("notes.png");
        std::fs::write(&text, "这不是图").unwrap();
        assert_eq!(code(json!({ "source": text.to_string_lossy() })), ImageErrorCode::NotImage);
        let big = dir.join("big.png");
        std::fs::File::create(&big).unwrap().set_len(IMAGE_MAX_BYTES + 1).unwrap();
        assert_eq!(code(json!({ "source": big.to_string_lossy() })), ImageErrorCode::TooLarge);
        assert_eq!(code(json!({ "source": "relative.png" })), ImageErrorCode::NotFound);
        assert_eq!(code(json!({ "source": "ftp://example.com/a.png" })), ImageErrorCode::Forbidden);
        assert_eq!(code(json!({ "source": "data:image/png;base64,AAAA" })), ImageErrorCode::Forbidden);
        // 内网 / 本机网址：不发请求，直接拒。
        assert_eq!(code(json!({ "source": "http://127.0.0.1:9/a.png" })), ImageErrorCode::Forbidden);
        assert_eq!(code(json!({ "source": "http://192.168.1.1/a.png" })), ImageErrorCode::Forbidden);
        assert_eq!(code(json!({ "source": "http://[::1]/a.png" })), ImageErrorCode::Forbidden);
        // 域名解析到本机（localhost）同样在解析器那层拦下。
        assert_eq!(code(json!({ "source": "http://localhost:9/a.png" })), ImageErrorCode::Forbidden);
        let receipt = send_into(&root, &json!({ "source": "relative.png" })).unwrap_err().receipt();
        assert!(receipt.starts_with("[Image error: not_found] "), "{receipt}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn public_address_classification() {
        let public = |s: &str| is_public(s.parse().unwrap());
        assert!(public("93.184.216.34"));
        assert!(public("2606:2800:220:1:248:1893:25c8:1946"));
        for private in [
            "127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.100.1.1",
            "0.0.0.0", "255.255.255.255", "224.0.0.1", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1",
        ] {
            assert!(!public(private), "{private} 应算内网");
        }
    }

    /// 下载的响应处理：测试里换成放行 127.0.0.1 的解析器，走的仍是 `fetch` 同一份代码。
    fn download_with(url: &str) -> Result<Vec<u8>, ImageError> {
        struct Loopback;
        impl ureq::Resolver for Loopback {
            fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
                netloc.to_socket_addrs().map(|a| a.collect())
            }
        }
        fetch(url, Loopback, false)
    }

    fn http(status: &str, ct: &str, body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    #[test]
    fn download_handles_status_type_and_header_sniff() {
        let ok = download_with(&serve_once(http("200 OK", "image/png", PNG_1X1))).unwrap();
        assert_eq!(ok, PNG_1X1);
        let e404 = download_with(&serve_once(http("404 Not Found", "text/html", b"nope"))).unwrap_err();
        assert_eq!(e404.code, ImageErrorCode::DownloadFailed);
        assert!(e404.message.contains("404"));
        let html = download_with(&serve_once(http("200 OK", "text/html", b"<html>"))).unwrap_err();
        assert_eq!(html.code, ImageErrorCode::NotImage);
        // 类型谎称 image/png、内容是文本：按文件头拒。
        let liar = download_with(&serve_once(http("200 OK", "image/png", b"hello"))).unwrap_err();
        assert_eq!(liar.code, ImageErrorCode::NotImage);
        // Content-Length 超过上限：不读正文。
        let mut big = format!(
            "HTTP/1.1 200 OK
Content-Type: image/png
Content-Length: {}
Connection: close

",
            IMAGE_MAX_BYTES + 1
        )
        .into_bytes();
        big.extend_from_slice(PNG_1X1);
        let too_large = download_with(&serve_once(big)).unwrap_err();
        assert_eq!(too_large.code, ImageErrorCode::TooLarge);
        // 连不上（端口没人听）。
        let refused = download_with("http://127.0.0.1:1/x.png").unwrap_err();
        assert_eq!(refused.code, ImageErrorCode::DownloadFailed);
    }

    #[test]
    fn resolve_local_relative_and_existing_copies() {
        let dir = temp("resolve");
        let root = dir.join("images");
        let readable = vec![root.clone()];
        let project = dir.join("project");
        std::fs::create_dir_all(project.join("docs")).unwrap();
        std::fs::write(project.join("docs").join("my shot.png"), PNG_1X1).unwrap();
        let cwd = project.to_string_lossy().into_owned();
        // 相对会话目录 + %20 编码。
        let copied = resolve_into(&root, &readable, "docs/my%20shot.png", Some(&cwd)).unwrap();
        assert!(copied.starts_with(root.join("local")));
        // 已在图片目录里的：原样返回，不再复制。
        let again = resolve_into(&root, &readable, &copied.to_string_lossy(), None).unwrap();
        assert_eq!(again, copied);
        // 相对路径但没有会话目录、文件不存在、奇怪方案。
        assert_eq!(
            resolve_into(&root, &readable, "docs/a.png", None).unwrap_err().code,
            ImageErrorCode::NotFound
        );
        assert_eq!(
            resolve_into(&root, &readable, "missing.png", Some(&cwd)).unwrap_err().code,
            ImageErrorCode::NotFound
        );
        assert_eq!(
            resolve_into(&root, &readable, "javascript:alert(1)", Some(&cwd)).unwrap_err().code,
            ImageErrorCode::Forbidden
        );
        assert_eq!(
            resolve_into(&root, &readable, "http://10.0.0.1/x.png", None).unwrap_err().code,
            ImageErrorCode::Forbidden
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_only_from_image_dirs() {
        let dir = temp("export");
        let root = dir.join("images");
        std::fs::create_dir_all(root.join("sent")).unwrap();
        let inside = root.join("sent").join("a.png");
        std::fs::write(&inside, PNG_1X1).unwrap();
        let secret = dir.join("secret.txt");
        std::fs::write(&secret, "secret").unwrap();
        let readable = vec![root.clone()];
        let dest = dir.join("saved.png");
        export_within(&readable, &inside, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), PNG_1X1);
        // ../ 越界。
        let escape = root.join("sent").join("..").join("..").join("secret.txt");
        assert_eq!(export_within(&readable, &escape, &dir.join("x")).unwrap_err(), "chat_image/forbidden");
        // 目标是目录。
        assert_eq!(export_within(&readable, &inside, &dir).unwrap_err(), "chat_image/invalid_dest");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
