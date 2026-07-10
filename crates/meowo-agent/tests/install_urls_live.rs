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

#[test]
#[ignore]
fn every_install_url_serves_a_real_script() {
    let mut failures = Vec::new();

    for p in meowo_agent::all() {
        for windows in [true, false] {
            let Some(script) = p.install_script(windows) else { continue };
            let Some((code, body)) = curl(script.url) else {
                failures.push(format!("{}: curl 执行失败（本机没有 curl？）", script.url));
                continue;
            };

            if code != 200 {
                failures.push(format!("{}: HTTP {code}", script.url));
                continue;
            }
            if meowo_agent::looks_like_challenge(&body) {
                // 这不是代码 bug——是 Cloudflare 此刻正在挑战。如实报告。
                failures.push(format!(
                    "{}: 返回 Cloudflare 人机校验页（HTTP 200）。间歇性，稍后重试",
                    script.url
                ));
                continue;
            }
            if !meowo_agent::is_runnable_script(&body) {
                failures.push(format!("{}: 返回的不是脚本（{} 字节）", script.url, body.len()));
                continue;
            }
            eprintln!("✓ {:8} {:6} {} ({} 字节)", p.id().as_str(), if windows { "win" } else { "unix" }, script.url, body.len());
        }
    }

    assert!(failures.is_empty(), "安装地址核对失败：\n  {}", failures.join("\n  "));
}

/// claude 的引导脚本只是段胶水：真正的二进制在 `downloads.claude.ai`（GCS，**不在** Cloudflare
/// 后面）。这条断言守住那个域仍然直连——它是「彻底绕开 CF 直下二进制」那条路的前提。
#[test]
#[ignore]
fn claude_binaries_are_not_behind_cloudflare() {
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

    // 每个平台都得有 checksum——那是直下方案的安全前提。
    for platform in ["win32-x64", "darwin-arm64", "linux-x64"] {
        assert!(manifest.contains(platform), "manifest 缺平台 {platform}");
    }
    assert!(manifest.contains("checksum"), "manifest 没有 checksum 字段");
    eprintln!("✓ claude {version}: manifest 带 checksum，downloads.claude.ai 无 CF");
}
