//! claude 的**直下安装**：绕开 `claude.ai` 的 Cloudflare，直接从 `downloads.claude.ai` 取二进制。
//!
//! 官方引导脚本（`claude.ai/install.ps1`）做的事只有三步：
//!
//! ```text
//! GET  downloads.claude.ai/claude-code-releases/latest              → "2.1.206"
//! GET  downloads.claude.ai/claude-code-releases/2.1.206/manifest.json → 各平台的 sha256 + size
//! GET  downloads.claude.ai/claude-code-releases/2.1.206/win32-x64/claude.exe
//! 校验 SHA-256，然后 `claude.exe install` 由二进制自己装 launcher 与 shell 集成
//! ```
//!
//! 而 `downloads.claude.ai` 是 GCS（`server: UploadServer`），**不在 Cloudflare 后面**。
//! 也就是说，我们费劲去过人机校验，只是为了拿一段最终会去 GCS 下载的胶水代码。
//!
//! 于是这里把那三步搬进来。附带的好处：多了 SHA-256 校验（裸管道连脚本都不校验），
//! 有 `size` 可以画进度条，也不必再 spawn 一个 PowerShell。
//!
//! 本模块只**解析出计划**（两次小 JSON/文本请求），下载、校验、执行留给宿主——插件层不写大文件、
//! 不 spawn 子进程。

use crate::install::InstallPlan;
use crate::ports::{Body, HttpRequest, Ports};

/// 发布物根地址。GCS 直连，无 Cloudflare。
const BASE: &str = "https://downloads.claude.ai/claude-code-releases";

/// 本机对应的 manifest 平台键。与官方脚本的判定一致（ARM64 用原生包，其余一律 x64）。
///
/// 官方脚本还会拒绝 32 位 Windows。这里不判：`Installation` 的候选路径本就找不到 32 位的产物，
/// 装完自然显示未安装——而 `std::env::consts` 给不出「宿主是 32 位 Windows」这一事实。
fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "win32-arm64",
        ("windows", "x86_64") => "win32-x64",
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", "x86_64") => "linux-x64",
        _ => return None,
    })
}

/// 从 manifest JSON 里取某平台的 (checksum, size, binary_name)。纯函数，便于单测。
///
/// manifest 形如：
/// ```jsonc
/// { "version": "2.1.206",
///   "platforms": { "win32-x64": { "binary": "claude.exe", "checksum": "d507…", "size": 248682144 } } }
/// ```
pub fn parse_manifest(manifest: &str, platform: &str) -> Option<(String, u64, String)> {
    let v: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let p = v.get("platforms")?.get(platform)?;
    let checksum = p.get("checksum")?.as_str()?.to_ascii_lowercase();
    let size = p.get("size")?.as_u64()?;
    let binary = p.get("binary")?.as_str()?.to_string();
    // sha256 十六进制必须是 64 个字符——manifest 若换了摘要算法，宁可退回引导脚本也不要跳过校验。
    (checksum.len() == 64 && checksum.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some((checksum, size, binary))
}

/// 版本号形态校验：`2.1.206`。取回的是纯文本，若被中间设备换成一页 HTML，这里就拦下。
fn parse_version(body: &str) -> Option<String> {
    let v = body.trim();
    let ok = !v.is_empty()
        && v.split('.').count() == 3
        && v.split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    ok.then(|| v.to_string())
}

fn get(ports: &Ports, url: &str) -> Result<String, String> {
    ports
        .http
        .send(&HttpRequest {
            method: "GET",
            url,
            headers: &[],
            body: Body::Empty,
            timeout: std::time::Duration::from_secs(30),
        })
        .map_err(|e| format!("GET {url} 失败：{e}"))
}

pub struct ClaudeDirectInstall;
pub static DIRECT_INSTALL: ClaudeDirectInstall = ClaudeDirectInstall;

impl crate::install::InstallCap for ClaudeDirectInstall {
    fn plan(&self, ports: &Ports) -> Result<InstallPlan, String> {
        plan(ports)
    }
}

/// 解析出安装计划：下载哪个 URL、期望的 sha256 与大小、下载完执行什么。
///
/// 两次请求都很小（版本号几个字节、manifest 几 KB），故走 `HttpPort::send`。
pub fn plan(ports: &Ports) -> Result<InstallPlan, String> {
    let platform = platform().ok_or_else(|| {
        format!(
            "claude 不支持本平台（{}/{}）",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let version = parse_version(&get(ports, &format!("{BASE}/latest"))?)
        .ok_or("取到的版本号形态不对（下载服务不可达，或本地区不可用）")?;

    let manifest = get(ports, &format!("{BASE}/{version}/manifest.json"))?;
    let (sha256, size, binary) = parse_manifest(&manifest, platform)
        .ok_or_else(|| format!("manifest 里没有平台 {platform} 的校验和"))?;

    Ok(InstallPlan {
        url: format!("{BASE}/{version}/{platform}/{binary}"),
        // 落在临时目录：装完即删。官方脚本落 `~/.claude/downloads`，那目录不该由我们凭空创建
        // （`~/.claude` 不存在＝没装过 Claude Code，见 `is_configured`）。
        file_name: format!("claude-{version}-{platform}"),
        sha256,
        size,
        // 二进制自己完成 launcher 与 shell 集成的安装（落到 `~/.local/bin`，正是变体表的首选候选）。
        post_install_args: vec!["install".to_string()],
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取自 downloads.claude.ai 的真实 manifest（截断到两个平台）。
    const MANIFEST: &str = r#"{
      "version": "2.1.206",
      "commit": "edc8ebf7f852d3abffad32a5bf8e49e439f92afb",
      "platforms": {
        "win32-x64":   { "binary": "claude.exe", "checksum": "d5072b25b9a20bffb24625d36129a05ed2be4d2eb7e35625aad6aa35596892c2", "size": 248682144 },
        "darwin-arm64":{ "binary": "claude",     "checksum": "3197aba4442dbd5b3df42b6f35e6d7bd03b5e48ce18b7a3c5c6f5f8c28e03b7f", "size": 240395024 }
      }
    }"#;

    #[test]
    fn parse_manifest_extracts_checksum_size_and_binary() {
        let (sum, size, bin) = parse_manifest(MANIFEST, "win32-x64").expect("应解出 win32-x64");
        assert_eq!(
            sum,
            "d5072b25b9a20bffb24625d36129a05ed2be4d2eb7e35625aad6aa35596892c2"
        );
        assert_eq!(size, 248_682_144);
        assert_eq!(bin, "claude.exe");

        let (_, _, bin) = parse_manifest(MANIFEST, "darwin-arm64").unwrap();
        assert_eq!(bin, "claude", "unix 上的产物没有 .exe");
    }

    #[test]
    fn parse_manifest_rejects_unknown_platform() {
        assert!(parse_manifest(MANIFEST, "solaris-sparc").is_none());
    }

    /// 摘要算法若被换掉（或字段被投毒成非法值），宁可整个失败、退回引导脚本，
    /// 也绝不带着一个跳过校验的路径继续下载 250 MB 的可执行文件。
    #[test]
    fn parse_manifest_rejects_malformed_checksum() {
        let bad =
            r#"{"platforms":{"win32-x64":{"binary":"claude.exe","checksum":"deadbeef","size":1}}}"#;
        assert!(parse_manifest(bad, "win32-x64").is_none(), "短摘要必须拒绝");

        let not_hex = format!(
            r#"{{"platforms":{{"win32-x64":{{"binary":"c.exe","checksum":"{}","size":1}}}}}}"#,
            "z".repeat(64)
        );
        assert!(
            parse_manifest(&not_hex, "win32-x64").is_none(),
            "非十六进制必须拒绝"
        );
    }

    #[test]
    fn parse_manifest_rejects_html() {
        // 中间设备塞了一页 HTML：serde_json 直接解析失败。
        assert!(parse_manifest("<!DOCTYPE html><html>…", "win32-x64").is_none());
    }

    #[test]
    fn parse_version_accepts_semver_and_rejects_garbage() {
        assert_eq!(parse_version("2.1.206\n").as_deref(), Some("2.1.206"));
        assert_eq!(parse_version("  2.1.206  ").as_deref(), Some("2.1.206"));
        assert!(parse_version("").is_none());
        assert!(parse_version("latest").is_none());
        assert!(parse_version("2.1").is_none());
        assert!(parse_version("2.1.x").is_none());
        // 被换成一页 HTML（错误页/挑战页）时必须拒绝，而不是把它当版本号拼进 URL。
        assert!(parse_version("<!DOCTYPE html><html><head>").is_none());
    }

    /// 本机平台必须能映射出一个 manifest 键——映射不出来就没法直下，得退回引导脚本。
    #[test]
    fn current_platform_maps_to_a_manifest_key() {
        let p = platform().expect("本机平台应被支持");
        assert!(MANIFEST.contains("platforms"));
        assert!(p.contains('-'), "平台键形如 os-arch：{p}");
    }
}
