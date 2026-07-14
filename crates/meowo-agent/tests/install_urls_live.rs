//! 联网核对三家的安装地址仍然有效（`#[ignore]`：默认不跑，不让 CI 依赖外网与第三方站点）。
//!
//! 手动跑：`cargo test -p meowo-agent --test install_urls_live -- --ignored --nocapture`
//!
//! 存在的理由：`install.rs` 的单测只证明「判定逻辑对」，证明不了「地址仍然指向真脚本」。
//! 官方随时可能改路径（kimi 就有 `/kimi-code/` 与不带它的两个入口，装出来的是不同的东西）。
//! 顺带，这个测试还会真实撞上 Cloudflare 的间歇校验——那正是它该报告的事。

use std::process::Command;

/// 用 curl 取内容（本 crate 不依赖 HTTP 栈；端口由宿主注入，测试里不便构造）。
/// 返回 (状态码, 正文)。
fn curl(url: &str) -> Option<(u32, String)> {
    let out = Command::new("curl")
        .args(["-sS", "-L", "--max-time", "30", "-w", "\n%{http_code}", url])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let (body, code) = s.rsplit_once('\n')?;
    Some((code.trim().parse().ok()?, body.to_string()))
}

/// 顺带核对：没有直下能力的 agent，其入口不得经过 Cloudflare。
/// 这条在单测里靠域名字符串守（`agents_without_direct_install_must_avoid_cloudflare_fronted_hosts`），
/// 这里则真去看响应头——上游随时可能把某个域挪到 CF 后面。
#[test]
#[ignore]
fn agents_without_direct_install_are_not_fronted_by_cloudflare() {
    let mut failures = Vec::new();
    for p in meowo_agent::all() {
        if p.direct_install().is_some() {
            continue; // 引导脚本只是回退，允许在 CF 后面
        }
        for windows in [true, false] {
            let Some(script) = p.install_script(windows) else { continue };
            // Command 变体（npm）没有可 curl 的 URL——CF 前置检查对它无意义，跳过。
            let meowo_agent::InstallScript::Fetch { url, .. } = script else { continue };
            let out = Command::new("curl")
                .args(["-sSIL", "--max-time", "30", url])
                .output()
                .expect("curl 执行失败");
            let headers = String::from_utf8_lossy(&out.stdout).to_lowercase();
            if headers.contains("cf-ray") {
                failures.push(format!("{}: {} 现在挂在 Cloudflare 后面了", p.id(), url));
            } else {
                eprintln!("✓ {:8} {} 无 CF", p.id().as_str(), url);
            }
        }
    }
    assert!(failures.is_empty(), "无直下的 agent 却经过 CF：\n  {}", failures.join("\n  "));
}

#[test]
#[ignore]
fn every_install_url_serves_a_real_script() {
    let mut failures = Vec::new();

    for p in meowo_agent::all() {
        for windows in [true, false] {
            let Some(script) = p.install_script(windows) else { continue };
            let meowo_agent::InstallScript::Fetch { url, .. } = script else { continue };
            let Some((code, body)) = curl(url) else {
                failures.push(format!("{}: curl 执行失败（本机没有 curl？）", url));
                continue;
            };

            if code != 200 {
                failures.push(format!("{}: HTTP {code}", url));
                continue;
            }
            if meowo_agent::looks_like_challenge(&body) {
                // 这不是代码 bug——是 Cloudflare 此刻正在挑战。如实报告。
                failures.push(format!(
                    "{}: 返回 Cloudflare 人机校验页（HTTP 200）。间歇性，稍后重试",
                    url
                ));
                continue;
            }
            if !meowo_agent::is_runnable_script(&body) {
                failures.push(format!("{}: 返回的不是脚本（{} 字节）", url, body.len()));
                continue;
            }
            eprintln!("✓ {:8} {:6} {} ({} 字节)", p.id().as_str(), if windows { "win" } else { "unix" }, url, body.len());
        }
    }

    assert!(failures.is_empty(), "安装地址核对失败：\n  {}", failures.join("\n  "));
}

/// claude 的引导脚本只是段胶水：真正的二进制在 `downloads.claude.ai`（GCS，**不在** Cloudflare
/// 后面）。这条断言守住那个域仍然直连——它是「彻底绕开 CF 直下二进制」那条路的前提。
///
/// 不下整个 250 MB：用 Range 请求取前两个字节，确认是可执行文件的魔数即可。
/// （完整的下载 + SHA-256 校验我手动跑过一次：size 与 checksum 都与 manifest 精确匹配。）
#[test]
#[ignore]
fn claude_direct_install_plan_resolves_to_a_real_binary() {
    const BASE: &str = "https://downloads.claude.ai/claude-code-releases";

    let (code, version) = curl(&format!("{BASE}/latest")).expect("取版本号失败");
    assert_eq!(code, 200);
    let version = version.trim();
    assert!(
        version.split('.').count() == 3 && version.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit())),
        "版本号形态不对：{version:?}"
    );

    let (code, manifest) = curl(&format!("{BASE}/{version}/manifest.json")).expect("取 manifest 失败");
    assert_eq!(code, 200);
    assert!(!meowo_agent::looks_like_challenge(&manifest), "downloads.claude.ai 竟然也被 CF 挑战了");

    // 用插件自己的解析器读 manifest——这才是「代码真的能用」的证明，而不是字符串 contains。
    use meowo_agent::plugins::claude::install::parse_manifest;
    for platform in ["win32-x64", "win32-arm64", "darwin-arm64", "darwin-x64", "linux-x64", "linux-arm64"] {
        let (sum, size, binary) = parse_manifest(&manifest, platform)
            .unwrap_or_else(|| panic!("manifest 里解不出 {platform}"));
        assert_eq!(sum.len(), 64, "{platform} 的 checksum 不是 sha256");
        assert!(size > 100_000_000, "{platform} 的 size 小得可疑：{size}");
        assert!(binary.starts_with("claude"), "{platform} 的产物名不对：{binary}");
    }

    // 取二进制的前两个字节：Windows 上应是 PE 的 `MZ`。
    let url = format!("{BASE}/{version}/win32-x64/claude.exe");
    let out = Command::new("curl")
        .args(["-sS", "--max-time", "60", "-r", "0-1", &url])
        .output()
        .expect("curl 执行失败");
    assert_eq!(&out.stdout, b"MZ", "{url} 返回的不是 PE 可执行文件");

    eprintln!("✓ claude {version}: manifest 六个平台都有 sha256，二进制是真的 PE，且全程无 CF");
}
