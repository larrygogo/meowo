//! 一键安装：官方引导脚本的**地址**，以及「这段文本是不是脚本」的判定。
//!
//! 此前 `install_script()` 返回的是一条命令串（`irm <url> | iex` / `curl -fsSL <url> | bash`），
//! 由 shell 自己联网取脚本再执行。这有个隐蔽的坑：
//!
//! `claude.ai` 与 `chatgpt.com` 都在 Cloudflare 后面，会间歇触发 managed challenge。
//! **challenge 页面返回的是 HTTP 200**，不是 403——`irm` 不抛错，`curl -f` 也不拦，于是那坨
//! `<script>window._cf_chl_opt = …</script>` 的 HTML 被原样喂给 PowerShell/bash 执行，用户看到的是
//! 「Installation failed: Just a moment...」加一屏 CSS。
//!
//! 现在改为：宿主先把脚本取回来，[`looks_like_challenge`] 判定后才落盘执行。shell 只跑本地文件，
//! 不再联网。

/// 官方安装引导脚本。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallScript {
    /// 脚本地址。取回后须经 [`looks_like_challenge`] 判定。
    pub url: &'static str,
    /// unix 下用哪个解释器跑它（`"bash"` / `"sh"`）。Windows 恒用 PowerShell，此字段忽略。
    /// 三家不同：claude/kimi 的脚本需要 bash（用了 `[[ ]]`），codex 的官方命令写的是 `sh`。
    pub unix_shell: &'static str,
}

/// 取前 `n` 个**字符**（不是字节）。直接 `&s[..n]` 会在多字节字符中间切开而 panic——
/// 被判定的内容来自网络，什么都可能有。
fn head(body: &str, n: usize) -> &str {
    match body.char_indices().nth(n) {
        Some((i, _)) => &body[..i],
        None => body,
    }
}

/// 取回的内容是不是 Cloudflare 的人机校验页（而非真脚本）。
///
/// 判据取自 challenge 页面必然出现的标记：`_cf_chl_opt`（挑战参数对象）、`cf-mitigated`、
/// 以及标题 `Just a moment`。不靠 `Content-Type`——它由宿主的 HTTP 层单独校验，且不同 CF
/// 配置下未必是 `text/html`。
///
/// 只做「像不像挑战页」这一个判断，不做「像不像脚本」：后者对 ps1/sh/未来的 py 各有形态，
/// 容易误杀。挑战页的特征则是稳定且唯一的。
pub fn looks_like_challenge(body: &str) -> bool {
    // 只看开头一段：真脚本可能在注释/echo 文案里提到任何字符串，但挑战页的标记必在文档头部。
    let h = head(body, 4096);
    h.contains("_cf_chl_opt") || h.contains("cf-mitigated") || h.contains("Just a moment")
}

/// 取回的内容是不是 HTML（挑战页、错误页、登录跳转页都长这样）。
/// 与 [`looks_like_challenge`] 互补：CF 之外的中间设备也可能塞一张 HTML。
pub fn looks_like_html(body: &str) -> bool {
    let h = head(body.trim_start(), 32).to_ascii_lowercase();
    h.starts_with("<!doctype html") || h.starts_with("<html") || h.starts_with("<head")
}

/// 取回的脚本是否可安全交给解释器执行。空内容、挑战页、HTML 一律拒绝。
pub fn is_runnable_script(body: &str) -> bool {
    !body.trim().is_empty() && !looks_like_challenge(body) && !looks_like_html(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实的 CF managed challenge 片段（取自实际安装日志，已截断）。
    const CHALLENGE: &str = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title>
<script>(function(){window._cf_chl_opt = {cFPWv: 'g', cType: 'managed', cZone: 'claude.ai'};})();</script>"#;

    /// claude 的真实 install.ps1 开头。
    const PS1: &str = "param(\n    [Parameter(Position=0)]\n    [string]$Target = \"latest\"\n)\n\nSet-StrictMode -Version Latest\n";

    /// claude 的真实 install.sh 开头。
    const SH: &str = "#!/bin/bash\n\nset -e\n\nTARGET=\"$1\"\n";

    #[test]
    fn challenge_page_is_rejected() {
        assert!(looks_like_challenge(CHALLENGE));
        assert!(looks_like_html(CHALLENGE));
        assert!(!is_runnable_script(CHALLENGE));
    }

    #[test]
    fn real_scripts_are_accepted() {
        for s in [PS1, SH] {
            assert!(!looks_like_challenge(s), "误判为挑战页：{s:?}");
            assert!(!looks_like_html(s), "误判为 HTML：{s:?}");
            assert!(is_runnable_script(s));
        }
    }

    #[test]
    fn empty_body_is_rejected() {
        // 200 + 空体（代理/中断）也不该交给解释器。
        assert!(!is_runnable_script(""));
        assert!(!is_runnable_script("   \n\t "));
    }

    /// 脚本正文里若恰好出现 `Just a moment` 这类字符串（注释、echo 文案），不该被误杀——
    /// 故只扫描开头 4KB，且挑战页的标记必在文档头部。
    #[test]
    fn marker_deep_in_a_long_script_does_not_false_positive() {
        let mut s = String::from("#!/bin/bash\nset -e\n");
        s.push_str(&"# padding comment line\n".repeat(400)); // 远超 4096 字节
        s.push_str("echo 'Just a moment...'\n");
        assert!(s.len() > 4096);
        assert!(!looks_like_challenge(&s));
        assert!(is_runnable_script(&s));
    }

    /// 反过来：挑战页即使被塞进很长的内容，其标记也在头部，仍应命中。
    #[test]
    fn challenge_marker_at_head_is_caught_even_in_long_body() {
        let s = format!("{CHALLENGE}{}", "x".repeat(10_000));
        assert!(looks_like_challenge(&s));
    }

    /// 真实回归：这段是用户实际安装日志里 CF 吐回来的东西（HTTP 200）。
    /// 旧代码把它交给了 PowerShell，于是报「Installation failed: Just a moment...」加一屏 CSS。
    #[test]
    fn the_actual_log_that_started_this_is_rejected() {
        let real = "Just a moment...*{box-sizing:border-box;margin:0;padding:0}html{line-height:1.15;\
            -webkit-text-size-adjust:100%;color:#313131}body{display:flex}\
            Enable JavaScript and cookies to continue(function(){window._cf_chl_opt = {cFPWv: 'g',\
            cType: 'managed',cZone: 'claude.ai',cRay: 'a18fc0811ad0b585'};})();";
        assert!(looks_like_challenge(real), "必须认出这段 challenge");
        assert!(!is_runnable_script(real), "绝不能把它交给解释器");
    }

    /// 三家的安装地址：必须是 https，且扩展名与平台匹配。写错会让一键安装装错东西
    /// （kimi 少写 `/kimi-code/` 就会装成旧 Python 版，落到检测不到的路径）。
    #[test]
    fn every_plugin_declares_sane_install_urls() {
        for p in crate::all() {
            for windows in [true, false] {
                let Some(s) = p.install_script(windows) else { continue };
                assert!(s.url.starts_with("https://"), "{} 的安装地址必须是 https：{}", p.id(), s.url);
                let want_ext = if windows { ".ps1" } else { ".sh" };
                assert!(s.url.ends_with(want_ext), "{} 的 {windows} 版地址应以 {want_ext} 结尾：{}", p.id(), s.url);
                assert!(
                    matches!(s.unix_shell, "bash" | "sh"),
                    "{} 声明了未知解释器：{}",
                    p.id(),
                    s.unix_shell
                );
            }
        }
    }

    /// kimi 的地址必须带 `/kimi-code/`——不带它装的是旧 Python `kimi-cli`，落到
    /// `~/.local/bin/kimi-cli.exe`，变体表的候选一个都命中不了，装完仍显示「未安装」。
    #[test]
    fn kimi_install_url_targets_the_node_edition() {
        let p = crate::by_id("kimi").unwrap();
        for windows in [true, false] {
            let s = p.install_script(windows).expect("kimi 有一键安装");
            assert!(s.url.contains("/kimi-code/"), "kimi 地址漏了 /kimi-code/：{}", s.url);
        }
    }

    /// 被判定的内容来自网络。按字节切片会在多字节字符中间 panic——中文注释、emoji、
    /// 甚至一段乱码都能触发。这几条断言只要求「不 panic」。
    #[test]
    fn multibyte_content_does_not_panic() {
        let cjk = "#!/bin/bash\n# 安装脚本：下载并校验二进制\n".repeat(500); // 远超 4096 字节
        assert!(is_runnable_script(&cjk));

        // 恰好让 4096 字节边界落在一个三字节汉字中间。
        let mut s = "a".repeat(4095);
        s.push('中');
        assert!(!looks_like_challenge(&s));

        // 开头就是多字节字符（HTML 判定的 32 字符切点同理）。
        assert!(!looks_like_html("中文开头的脚本注释"));
        assert!(!is_runnable_script("")); // 顺带守住空串
    }
}
