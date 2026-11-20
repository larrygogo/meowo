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
            let Some(end) = after.find("?>") else {
                return false;
            };
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("<!--") {
            let Some(end) = after.find("-->") else {
                return false;
            };
            rest = &after[end + 3..];
        } else if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
            let after = &rest[9..];
            // DOCTYPE 可能带内部子集 `[...]`：跳到子集结束后的 `>`。
            let end = match (after.find('['), after.find('>')) {
                (Some(open), Some(close)) if open < close => {
                    let Some(sub_end) = after[open..].find("]") else {
                        return false;
                    };
                    let tail = &after[open + sub_end..];
                    let Some(gt) = tail.find('>') else {
                        return false;
                    };
                    open + sub_end + gt
                }
                (_, Some(close)) => close,
                _ => return false,
            };
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    let Some(after) = rest.strip_prefix("<svg") else {
        return false;
    };
    matches!(after.chars().next(), Some(c) if c.is_whitespace() || c == '>' || c == '/')
}

/// 从文件头读位图尺寸（宽, 高），不解码。读不出（SVG、少见的子格式）返回 None。
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
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
        "png" => Some((be32(bytes, 16)?, be32(bytes, 20)?)),
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
            // 逐个跳过段，找到 SOFn（帧头）读高宽。
            let mut at = 2;
            while at + 9 < bytes.len() {
                if bytes[at] != 0xFF {
                    return None;
                }
                let marker = bytes[at + 1];
                if marker == 0xFF {
                    at += 1;
                    continue;
                }
                let len = be16(bytes, at + 2)? as usize;
                let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
                if is_sof {
                    return Some((be16(bytes, at + 7)?, be16(bytes, at + 5)?));
                }
                at += 2 + len;
            }
            None
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
    if let Some((w, h)) = image_dimensions(bytes) {
        if w as u64 * h as u64 > IMAGE_MAX_PIXELS {
            return Err(ImageError::new(
                ImageErrorCode::TooLarge,
                format!("图片尺寸太大（{w}×{h}），超过 1 亿像素的上限"),
            ));
        }
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
    write_named(&root.join(kind.dir_name()), &name, bytes)
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
    write_named(&root.join(kind.dir_name()), &name, bytes)
}

/// 按键找已缓存的图（任意扩展名）。
pub fn cached_keyed_in(root: &Path, kind: ImageKind, key: &str) -> Option<PathBuf> {
    let stem = hex_digest(key.as_bytes());
    let dir = root.join(kind.dir_name());
    ["png", "jpg", "gif", "webp", "svg"]
        .iter()
        .map(|ext| dir.join(format!("{stem}.{ext}")))
        .find(|path| std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0))
        .inspect(|path| touch(path))
}

/// 读一个本地文件存进图片目录。先看元数据再读：不存在、是目录、超过上限都不读盘。
pub fn store_file_in(root: &Path, kind: ImageKind, path: &Path) -> Result<PathBuf, ImageError> {
    let bytes = read_image_file(path)?;
    store_bytes_in(root, kind, &bytes)
}

pub fn store_file(kind: ImageKind, path: &Path) -> Result<PathBuf, ImageError> {
    store_file_in(&default_root()?, kind, path)
}

/// 读本地图片文件（带存在性与大小检查），不落盘。
pub fn read_image_file(path: &Path) -> Result<Vec<u8>, ImageError> {
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

/// 工具回执里的内嵌 base64 图落进 `tool/`，返回引用路径。transcript 解析用：任何失败都返回
/// None（这张图退化为不显示，正文照常）。声明的 MIME 不可信，仍按文件头判断。
pub fn store_tool_base64(data: &str) -> Option<PathBuf> {
    // 编码后长度 ≈ 4/3 原始大小：先按编码长度挡超大 payload，再解码。
    if data.len() as u64 > IMAGE_MAX_BYTES / 3 * 4 + 4 {
        return None;
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim().as_bytes())
        .ok()?;
    store_bytes(ImageKind::Tool, &bytes).ok()
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

/// 原子落盘：临时文件 + rename，读端不会看到半张图；目标已存在且大小一致就直接复用。
fn write_named(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, ImageError> {
    let path = dir.join(name);
    if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64) {
        touch(&path);
        return Ok(path);
    }
    std::fs::create_dir_all(dir).map_err(io_error)?;
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.tmp.{}.{seq}", std::process::id()));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io_error(e));
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        // 并发写同一内容：别人先 rename 成功了，结果一样。
        if std::fs::metadata(&path).is_ok_and(|m| m.len() == bytes.len() as u64) {
            return Ok(path);
        }
        return Err(io_error(e));
    }
    Ok(path)
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

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
    let before: u64 = files.iter().map(|f| f.2).sum();
    let mut total = before;
    let mut removed = 0;
    if total > cap {
        // false（可再生）排在 true（sent）前面；同类按修改时间从旧到新。
        files.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        for (_, _, len, path) in &files {
            if total <= cap {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
                removed += 1;
            }
        }
    }
    PruneReport {
        before,
        after: total,
        removed,
    }
}

/// 默认根目录按 [`IMAGES_TOTAL_CAP`] 回收（宿主启动时在后台线程调）。
pub fn prune() -> Option<PruneReport> {
    Some(prune_in(&images_root()?, IMAGES_TOTAL_CAP))
}

/// `path` 是否真的落在 `roots` 之一里面。两边都先解析成真实路径（展开 `..`、符号链接、junction）
/// 再比较前缀：`images/../secret`、指向外面的符号链接都会在这一步露出真身而被拒。
/// 路径不存在返回 None。
pub fn resolve_within(roots: &[PathBuf], path: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    roots
        .iter()
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .any(|root| real.starts_with(&root))
        .then_some(real)
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
