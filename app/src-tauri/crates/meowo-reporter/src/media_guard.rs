//! Kimi 读图前的拦截（PreToolUse，matcher `^ReadMediaFile$`）。
//!
//! 实拍（kimi-code 2.6.1）：Kimi 自己用 ReadMediaFile 读了仓库里的动图 WebP，之后整段对话的每次
//! 请求都被接口拒收（400 Invalid request Error），用户连发「继续」全错，会话报废。被拒的图留在
//! 上下文里，重发必错。
//!
//! 哪些图会被拒，按真机逐张试出来（2026-11-15，不装拦截、各开一个会话读一张图）：
//! - 动图 WebP 一律被拒：1.28MB 的原图、截成 3 帧的 90KB 小图都 400。
//! - 最短边小于 20 像素被拒：1、10、14、16、18 见方的 PNG 和 400×12 的扁图都 400；
//!   20、28、64 见方正常（19 没测，按被拒处理）。
//! - 静态 GIF、3 帧动图 GIF（200×200）、静态 PNG 都正常。
//!
//! Kimi 发图前的压缩（kimi.exe 内嵌 `agent/media/image-compress.ts`）只会缩小、不放大：长边超过
//! 2000 的 PNG/JPEG/静态 WebP 等比缩到 2000，所以按缩完的短边判断。
//!
//! 命中就拒绝这次调用（退出码 2，stderr 交给模型），模型换别的办法看，会话不受影响。
//! 读不到、认不出尺寸的一律放行：宁可漏拦，不误伤正常读图。

use std::io::Read;
use std::path::{Path, PathBuf};

/// 接口收图的最短边下限（像素）。见模块头的真机记录。
const MIN_EDGE: u32 = 20;
/// Kimi 压缩时的长边上限（`resolveMaxImageEdgePx` 默认 2000）。
const MAX_EDGE: u32 = 2000;
/// 文件头读这么多：JPEG 的尺寸在 SOF 段，前面可能有 EXIF 缩略图。
const HEAD_BYTES: u64 = 256 * 1024;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Format {
    Png,
    Jpeg,
    Gif,
    Bmp,
    StaticWebp,
    AnimatedWebp,
}

impl Format {
    /// Kimi 会重编码（因而可能缩小）的格式，见 `RECODABLE_MIME`。
    fn recodable(self) -> bool {
        matches!(self, Format::Png | Format::Jpeg | Format::StaticWebp)
    }
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
    let path = resolve(raw, cwd, home().as_deref())?;
    let mut head = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    let format = sniff(&head)?;
    let why = if format == Format::AnimatedWebp {
        "动图 WebP，Kimi 的接口不收".to_string()
    } else {
        let (w, h) = dimensions(&head, format)?;
        let short = sent_short_edge(w, h, format.recodable());
        if short >= MIN_EDGE {
            return None;
        }
        format!("{w}×{h} 的小图，最短边不到 {MIN_EDGE} 像素，Kimi 的接口不收")
    };
    Some(format!(
        "Meowo 拦下了这次读图：{raw} 是{why}。读进来之后，这段对话的每次请求都会被拒绝。\
         不要读这张图；需要了解它时看文件名、引用它的文档或源码，或者读别的图。"
    ))
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 照 Kimi 的路径规则解析：`~` 展开到用户目录，Git Bash 的 `/c/...` 在 Windows 上换成 `C:/...`，
/// 其余相对路径接在 cwd 后面。
fn resolve(raw: &str, cwd: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    if raw == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return Some(home?.join(rest));
    }
    if cfg!(windows) {
        let b = raw.as_bytes();
        if b.len() >= 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && (b.len() == 2 || b[2] == b'/') {
            let drive = (b[1] as char).to_ascii_uppercase();
            return Some(PathBuf::from(format!("{drive}:/{}", raw.get(3..).unwrap_or(""))));
        }
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    Some(Path::new(cwd?).join(path))
}

/// 发出去时的最短边：可重编码的图长边超过 2000 会被等比缩小。
fn sent_short_edge(w: u32, h: u32, recodable: bool) -> u32 {
    let (long, short) = (w.max(h), w.min(h));
    if recodable && long > MAX_EDGE {
        (u64::from(short) * u64::from(MAX_EDGE) / u64::from(long)) as u32
    } else {
        short
    }
}

fn sniff(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    if head.starts_with(b"BM") {
        return Some(Format::Bmp);
    }
    if head.len() >= 16 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        // VP8X 扩展头的标志字节第 2 位是动画位（WebP 容器规范）。
        let animated = &head[12..16] == b"VP8X" && head.get(20).is_some_and(|f| f & 0x02 != 0);
        return Some(if animated { Format::AnimatedWebp } else { Format::StaticWebp });
    }
    None // 视频等其他媒体不归这里管
}

/// 从文件头读宽高。读不出返回 None（放行）。
fn dimensions(b: &[u8], format: Format) -> Option<(u32, u32)> {
    let be16 = |i: usize| Some(u32::from(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?)));
    let le16 = |i: usize| Some(u32::from(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?)));
    let le24 = |i: usize| {
        let s = b.get(i..i + 3)?;
        Some(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
    };
    match format {
        Format::Png => Some((
            u32::from_be_bytes(b.get(16..20)?.try_into().ok()?),
            u32::from_be_bytes(b.get(20..24)?.try_into().ok()?),
        )),
        Format::Gif => Some((le16(6)?, le16(8)?)),
        Format::Bmp => {
            let w = i32::from_le_bytes(b.get(18..22)?.try_into().ok()?);
            let h = i32::from_le_bytes(b.get(22..26)?.try_into().ok()?);
            Some((w.unsigned_abs(), h.unsigned_abs()))
        }
        Format::StaticWebp | Format::AnimatedWebp => match b.get(12..16)? {
            b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
            b"VP8 " => Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF)),
            b"VP8L" => {
                let bits = u32::from_le_bytes(b.get(21..25)?.try_into().ok()?);
                Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
            }
            _ => None,
        },
        Format::Jpeg => {
            // 逐段跳到 SOFn（C0–CF，除去 C4 DHT、C8、CC DAC）。
            let mut i = 2;
            while i + 9 < b.len() {
                if b[i] != 0xFF {
                    return None;
                }
                let marker = b[i + 1];
                if marker == 0xFF {
                    i += 1;
                    continue;
                }
                let len = be16(i + 2)? as usize;
                if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    return Some((be16(i + 7)?, be16(i + 5)?));
                }
                i += 2 + len;
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0]);
        b
    }

    fn webp_vp8x(animated: bool, w: u32, h: u32) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0".to_vec();
        b.push(if animated { 0x12 } else { 0x10 });
        b.extend_from_slice(&[0, 0, 0]);
        b.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
        b.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
        b
    }

    fn gif(w: u16, h: u16) -> Vec<u8> {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&[0x80, 0, 0]);
        b
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        // SOI、一段 APP0、SOF0。
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        b.extend_from_slice(b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[0x03, 0x01, 0x22, 0x00]);
        b
    }

    struct Ws(tempfile::TempDir);

    impl Ws {
        fn new() -> Self {
            Ws(tempfile::tempdir().unwrap())
        }
        fn check(&self, name: &str, bytes: &[u8]) -> Option<String> {
            std::fs::write(self.0.path().join(name), bytes).unwrap();
            let input = json!({ "path": name });
            kimi_read_media_denial(Some("ReadMediaFile"), Some(&input), self.0.path().to_str())
        }
    }

    /// 实拍那张 island.webp（1760×1200 动图）和截成 3 帧的 90KB 小图：都 400，体积无关。
    #[test]
    fn animated_webp_is_denied_regardless_of_size() {
        let ws = Ws::new();
        let mut big = webp_vp8x(true, 1760, 1200);
        big.resize(1_286_858, 0);
        let reason = ws.check("island.webp", &big).expect("要拦");
        assert!(reason.contains("island.webp") && reason.contains("动图 WebP"), "{reason}");
        assert!(reason.contains("不要读这张图"), "{reason}");
        let mut small = webp_vp8x(true, 1760, 1200);
        small.resize(90_902, 0);
        assert!(ws.check("small_anim.webp", &small).is_some());
    }

    /// 真机：短边 1、10、14、16、18 的 PNG 和 400×12 都 400；20、28、64 正常。
    #[test]
    fn short_edge_below_twenty_is_denied() {
        let ws = Ws::new();
        for (w, h) in [(1, 1), (10, 10), (16, 16), (18, 18), (400, 12), (19, 300)] {
            let reason = ws.check("tiny.png", &png(w, h)).unwrap_or_else(|| panic!("{w}×{h} 要拦"));
            assert!(reason.contains(&format!("{w}×{h}")), "{reason}");
        }
        for (w, h) in [(20, 20), (28, 28), (64, 64), (1760, 1200)] {
            assert_eq!(ws.check("ok.png", &png(w, h)), None, "{w}×{h} 应放行");
        }
        // 其他格式同一条规则：GIF、JPEG、静态 WebP、BMP。
        assert!(ws.check("icon.gif", &gif(16, 16)).is_some());
        assert_eq!(ws.check("demo.gif", &gif(200, 200)), None);
        assert!(ws.check("thumb.jpg", &jpeg(12, 12)).is_some());
        assert_eq!(ws.check("photo.jpg", &jpeg(1920, 1080)), None);
        assert!(ws.check("dot.webp", &webp_vp8x(false, 8, 8)).is_some());
        assert_eq!(ws.check("still.webp", &webp_vp8x(false, 800, 600)), None);
        let mut bmp = b"BM".to_vec();
        bmp.resize(18, 0);
        bmp.extend_from_slice(&10i32.to_le_bytes());
        bmp.extend_from_slice(&(-10i32).to_le_bytes());
        assert!(ws.check("x.bmp", &bmp).is_some());
    }

    /// Kimi 把长边超过 2000 的可重编码图等比缩到 2000：4000×30 发出去是 2000×15，会被拒。
    /// GIF 不重编码，原样发，4000×30 照旧 30。
    #[test]
    fn short_edge_is_measured_after_kimi_downscale() {
        let ws = Ws::new();
        assert!(ws.check("banner.png", &png(4000, 30)).is_some());
        assert_eq!(ws.check("banner.png", &png(4000, 40)), None);
        assert_eq!(ws.check("banner.gif", &gif(4000, 30)), None);
    }

    /// 同会话里先读的 panel.png 没出事；视频、其他工具、读不到的文件、认不出的格式一律放行。
    #[test]
    fn everything_else_passes() {
        let ws = Ws::new();
        let mut panel = png(1600, 1000);
        panel.resize(304_831, 0);
        assert_eq!(ws.check("panel.png", &panel), None);
        assert_eq!(ws.check("clip.mp4", &[0, 0, 0, 0x20, b'f', b't', b'y', b'p']), None);
        assert_eq!(ws.check("broken.png", b"\x89PNG\r\n\x1a\n"), None);
        let input = json!({ "path": "tiny.png" });
        std::fs::write(ws.0.path().join("tiny.png"), png(4, 4)).unwrap();
        let cwd = ws.0.path().to_str();
        assert_eq!(kimi_read_media_denial(Some("ReadFile"), Some(&input), cwd), None);
        assert_eq!(kimi_read_media_denial(Some("ReadMediaFile"), Some(&input), None), None);
        let missing = json!({ "path": "missing.png" });
        assert_eq!(kimi_read_media_denial(Some("ReadMediaFile"), Some(&missing), cwd), None);
    }

    /// Kimi 认的路径写法：绝对路径、`~/`、Git Bash 的 `/c/...`。
    #[test]
    fn paths_resolve_like_kimi() {
        let home = Path::new("C:/Users/u");
        assert_eq!(resolve("~/a/b.png", Some("D:/w"), Some(home)), Some(home.join("a/b.png")));
        assert_eq!(resolve("~", None, Some(home)), Some(home.to_path_buf()));
        assert_eq!(resolve("docs/x.png", Some("D:/w"), Some(home)), Some(Path::new("D:/w").join("docs/x.png")));
        assert_eq!(resolve("docs/x.png", None, Some(home)), None);
        if cfg!(windows) {
            assert_eq!(resolve("/c/Users/u/x.png", None, None), Some(PathBuf::from("C:/Users/u/x.png")));
            assert_eq!(resolve("/d", None, None), Some(PathBuf::from("D:/")));
        }
        // 绝对路径不依赖 cwd：真实文件走一遍完整判定。
        let ws = Ws::new();
        let abs = ws.0.path().join("tiny.png");
        std::fs::write(&abs, png(4, 4)).unwrap();
        let input = json!({ "path": abs });
        assert!(kimi_read_media_denial(Some("ReadMediaFile"), Some(&input), None).is_some());
    }
}
