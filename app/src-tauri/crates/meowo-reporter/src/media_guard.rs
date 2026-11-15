//! Kimi 读图前的拦截（PreToolUse，matcher `^ReadMediaFile$`）。
//!
//! 实拍（kimi-code 2.6.1）：Kimi 自己用 ReadMediaFile 读了仓库里 1.28MB 的动图 WebP，之后整段
//! 对话的每次请求都被接口拒收（400 Invalid request Error），用户连发「继续」全错，会话报废。
//!
//! 原因在 Kimi 自己的发图前压缩（kimi.exe 内嵌 `agent/media/image-compress.ts`）：
//! - 只重编码 PNG、JPEG、静态 WebP（`RECODABLE_MIME` + `!isAnimatedWebp`），压到 256KB 预算
//!   （`resolveReadImageByteBudget` 默认 262144）、长边 2000px 以内；
//! - 其余格式（动图 WebP、GIF、BMP、AVIF…）走 `passthrough_unsupported`，原图直发；
//! - 可重编码的图超过 1e8 像素或 64MB 时走 `passthrough_guard`，同样原图直发。
//!
//! 原图直发、又超过预算的图就是会把会话毒死的那一类。这里照同一套分支判定，命中就拒绝这次
//! 调用（退出码 2，stderr 交给模型），模型换别的办法看，会话不受影响。
//! 读不到、认不出的一律放行：宁可漏拦，不误伤正常读图。

use std::io::Read;
use std::path::{Path, PathBuf};

/// Kimi 的单张读图预算（字节），与 `resolveReadImageByteBudget` 默认值一致。
const BYTE_BUDGET: u64 = 262_144;
/// Kimi 解码前的像素上限（`MAX_DECODE_PIXELS`）。
const MAX_DECODE_PIXELS: u64 = 100_000_000;
/// Kimi 解码前的字节上限（`MAX_IMAGE_DECODE_BYTES`）。
const MAX_DECODE_BYTES: u64 = 67_108_864;
/// 只读文件头：格式、尺寸、动图标志都在开头几十字节。
const HEAD_BYTES: usize = 64;

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Png,
    Jpeg,
    StaticWebp,
    AnimatedWebp,
    /// Kimi 认得、但不重编码的图片格式（按扩展名）。
    OtherImage(&'static str),
}

/// 这次 ReadMediaFile 该不该拦。返回给模型看的拒绝原因；放行返回 None。
pub fn kimi_read_media_denial(
    tool_name: Option<&str>,
    tool_input: Option<&serde_json::Value>,
    cwd: Option<&str>,
) -> Option<String> {
    if tool_name != Some("ReadMediaFile") {
        return None;
    }
    let raw = tool_input?.get("path")?.as_str()?;
    let path = resolve(raw, cwd)?;
    let size = std::fs::metadata(&path).ok()?.len();
    let mut head = Vec::with_capacity(HEAD_BYTES);
    std::fs::File::open(&path)
        .ok()?
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)
        .ok()?;
    let kind = sniff(&head, &path)?;
    let passthrough = match kind {
        Kind::Png | Kind::Jpeg | Kind::StaticWebp => {
            size > MAX_DECODE_BYTES || pixels(&head, &kind).is_some_and(|p| p > MAX_DECODE_PIXELS)
        }
        Kind::AnimatedWebp | Kind::OtherImage(_) => true,
    };
    if !passthrough || size <= BYTE_BUDGET {
        return None;
    }
    let what = match kind {
        Kind::AnimatedWebp => "动图 WebP".to_string(),
        Kind::OtherImage(name) => name.to_string(),
        _ => "超大图片".to_string(),
    };
    Some(format!(
        "Meowo 拦下了这次读图：{raw} 是{what}（{size_mb:.1} MB）。Kimi 不会压缩这种图，原图发给接口会被拒收，\
         之后这段对话的每次请求都会失败。不要读这张图；需要了解它时看文件名、引用它的文档或源码，\
         或者读一张静态 PNG、JPEG。",
        size_mb = size as f64 / 1_048_576.0,
    ))
}

fn resolve(raw: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    Some(Path::new(cwd?).join(path))
}

fn sniff(head: &[u8], path: &Path) -> Option<Kind> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Kind::Png);
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Kind::Jpeg);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        // VP8X 扩展头的标志字节第 2 位是动画位（WebP 容器规范）。
        let animated = head.len() > 20 && &head[12..16] == b"VP8X" && head[20] & 0x02 != 0;
        return Some(if animated { Kind::AnimatedWebp } else { Kind::StaticWebp });
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let name = match ext.as_str() {
        "gif" => "GIF 图片",
        "bmp" => "BMP 图片",
        "avif" => "AVIF 图片",
        "ico" => "ICO 图标",
        "tif" | "tiff" => "TIFF 图片",
        "heic" | "heif" => "HEIC 图片",
        _ => return None, // 视频等其他媒体不归这里管
    };
    Some(Kind::OtherImage(name))
}

/// 从文件头读像素数。PNG 的 IHDR、WebP 的 VP8X 画布尺寸都在开头；读不出返回 None。
fn pixels(head: &[u8], kind: &Kind) -> Option<u64> {
    match kind {
        Kind::Png if head.len() >= 24 => {
            let w = u32::from_be_bytes(head[16..20].try_into().ok()?) as u64;
            let h = u32::from_be_bytes(head[20..24].try_into().ok()?) as u64;
            Some(w * h)
        }
        Kind::StaticWebp if head.len() >= 30 && &head[12..16] == b"VP8X" => {
            let le24 = |b: &[u8]| u64::from(b[0]) | u64::from(b[1]) << 8 | u64::from(b[2]) << 16;
            Some((le24(&head[24..27]) + 1) * (le24(&head[27..30]) + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn webp(animated: bool, len: usize) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        b.push(if animated { 0x12 } else { 0x10 });
        b.extend_from_slice(&[0, 0, 0, 0xdf, 0x06, 0, 0xaf, 0x04, 0]);
        b.resize(len, 0);
        b
    }

    fn deny(dir: &Path, name: &str, bytes: &[u8]) -> Option<String> {
        std::fs::write(dir.join(name), bytes).unwrap();
        let input = json!({"path": format!("docs/{name}")});
        kimi_read_media_denial(Some("ReadMediaFile"), Some(&input), dir.parent().and_then(|p| p.to_str()))
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let docs = root.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        (root, docs)
    }

    /// 实拍那张 island.webp：1.28MB 的动图 WebP，Kimi 原图直发，会话从此 400。
    #[test]
    fn large_animated_webp_is_denied_with_reason() {
        let (_root, docs) = setup();
        let reason = deny(&docs, "island.webp", &webp(true, 1_286_858)).expect("要拦");
        assert!(reason.contains("docs/island.webp"), "{reason}");
        assert!(reason.contains("动图 WebP"), "{reason}");
        assert!(reason.contains("不要读这张图"), "{reason}");
    }

    /// 同会话里先读的 panel.png（300KB）没出事：Kimi 会把它压到预算内。静态 WebP 同理。
    #[test]
    fn recodable_images_pass_even_when_large() {
        let (_root, docs) = setup();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\x07\x80\0\0\x04\x38".to_vec();
        png.resize(304_831, 0);
        assert_eq!(deny(&docs, "panel.png", &png), None);
        assert_eq!(deny(&docs, "still.webp", &webp(false, 2_000_000)), None);
        let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
        jpg.resize(3_000_000, 0);
        assert_eq!(deny(&docs, "photo.jpg", &jpg), None);
    }

    /// 原图直发但在预算内的小图不拦：没有证据说接口会拒。
    #[test]
    fn small_passthrough_images_pass() {
        let (_root, docs) = setup();
        assert_eq!(deny(&docs, "tiny.webp", &webp(true, 100_000)), None);
        let mut gif = b"GIF89a".to_vec();
        gif.resize(50_000, 0);
        assert_eq!(deny(&docs, "spin.gif", &gif), None);
    }

    #[test]
    fn large_gif_and_oversized_png_are_denied() {
        let (_root, docs) = setup();
        let mut gif = b"GIF89a".to_vec();
        gif.resize(900_000, 0);
        assert!(deny(&docs, "demo.gif", &gif).unwrap().contains("GIF 图片"));
        // 2 亿像素的 PNG：超过 Kimi 解码上限，同样原图直发。
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\x4e\x20\0\0\x27\x10".to_vec();
        png.resize(400_000, 0);
        assert!(deny(&docs, "huge.png", &png).unwrap().contains("超大图片"));
    }

    /// 其他工具、读不到的文件、视频、没有 cwd 的相对路径：一律放行。
    #[test]
    fn everything_else_passes() {
        let (root, docs) = setup();
        std::fs::write(docs.join("island.webp"), webp(true, 1_286_858)).unwrap();
        let input = json!({"path": "docs/island.webp"});
        let cwd = root.path().to_str();
        assert_eq!(kimi_read_media_denial(Some("ReadFile"), Some(&input), cwd), None);
        assert_eq!(kimi_read_media_denial(Some("ReadMediaFile"), Some(&input), None), None);
        assert_eq!(
            kimi_read_media_denial(Some("ReadMediaFile"), Some(&json!({"path": "docs/missing.webp"})), cwd),
            None
        );
        let mut mp4 = vec![0, 0, 0, 0x20];
        mp4.resize(5_000_000, 0);
        std::fs::write(docs.join("clip.mp4"), mp4).unwrap();
        assert_eq!(
            kimi_read_media_denial(Some("ReadMediaFile"), Some(&json!({"path": "docs/clip.mp4"})), cwd),
            None
        );
        // 绝对路径不依赖 cwd。
        let abs = docs.join("island.webp");
        assert!(kimi_read_media_denial(Some("ReadMediaFile"), Some(&json!({"path": abs})), None).is_some());
    }
}
