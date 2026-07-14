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

/// 一键安装的方式。
///
/// 两条路子：**抓官方引导脚本**（claude/codex/kimi 的 `curl|sh` / `irm|iex` 落点），或**直接跑一条
/// 本地命令**（gemini/opencode 走 `npm i -g …`，官方没有 `curl|sh` 脚本可抓）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallScript {
    /// 抓官方引导脚本再执行。脚本地址取回后**须经** [`is_runnable_script`]（含 [`looks_like_challenge`]）
    /// 判定——`claude.ai`/`chatgpt.com` 在 CF 后面，会间歇把挑战页当 200 返回，不能直接喂给解释器。
    Fetch {
        url: &'static str,
        /// unix 下用哪个解释器（`"bash"` / `"sh"`）。Windows 恒用 PowerShell，此字段忽略。
        /// claude/kimi 的脚本需要 bash（用了 `[[ ]]`），codex 官方命令写的是 `sh`。
        unix_shell: &'static str,
    },
    /// 直接跑一条本地命令（`npm i -g …` 等，官方没有引导脚本）。命令体原样写进临时 `.ps1`/`.sh`
    /// 后执行——**不联网、无需 challenge 判定**（不经 CF）。两平台命令若不同，由 `install_script(windows)`
    /// 各返回一份；npm 命令通常两平台一致。
    Command {
        body: &'static str,
        /// unix 下的解释器；npm 命令用 `"bash"`/`"sh"` 皆可。Windows 恒用 PowerShell。
        unix_shell: &'static str,
    },
}

impl InstallScript {
    /// unix 下的解释器（Windows 恒用 PowerShell，忽略此值）。
    pub fn unix_shell(&self) -> &'static str {
        match self {
            Self::Fetch { unix_shell, .. } | Self::Command { unix_shell, .. } => unix_shell,
        }
    }

    /// 日志 / 进度里展示的「来源」：Fetch 给脚本地址，Command 给命令本身。
    pub fn source(&self) -> &'static str {
        match self {
            Self::Fetch { url, .. } => url,
            Self::Command { body, .. } => body,
        }
    }
}

/// 直下安装的计划：下载什么、怎么校验、装完执行什么。
///
/// 插件只负责**解析出**它（几次小请求），下载大文件、校验摘要、spawn 子进程一律归宿主——
/// 插件层不写大文件、不 spawn，这与它「纯逻辑 + 注入端口」的定位一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// 二进制地址。**必须不在人机校验之后**——直下的全部意义就在于此。
    pub url: String,
    /// 落盘的文件名（不含目录；宿主决定放临时目录哪儿）。
    pub file_name: String,
    /// 期望的 SHA-256（小写十六进制，64 字符）。不匹配必须删文件并报错。
    pub sha256: String,
    /// 期望的字节数。用来画进度，也作为下载完的第一道校验。
    pub size: u64,
    /// 下载校验通过后，用**该二进制自身**执行的参数（claude 是 `["install"]`，
    /// 由它自己装 launcher 与 shell 集成）。空 = 下载完即算装好。
    pub post_install_args: Vec<String>,
    /// 供日志与 UI 展示。
    pub version: String,
}

/// 直下安装能力。声明它的 agent 可以完全绕开引导脚本（及其身后的人机校验）。
///
/// 不声明的 agent 退回 [`AgentPlugin::install_script`](crate::AgentPlugin::install_script)。
pub trait InstallCap: Sync {
    /// 解析出计划。只做小请求（版本号、清单），不下载大文件。
    fn plan(&self, ports: &crate::ports::Ports) -> Result<InstallPlan, String>;
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

    /// 每家的安装声明都得站得住脚：
    /// - `Fetch` 的地址必须是 https（写错会装错东西——kimi 少写 `/kimi-code/` 就装成旧 Python 版）。
    /// - `Command` 的命令体非空。
    /// - 解释器只认 bash/sh。
    ///
    /// 不再强求地址以 `.ps1`/`.sh` 结尾：opencode 的官方安装端点 `https://opencode.ai/install` 就没有
    /// 扩展名（服务端按 UA 决定回什么）。
    #[test]
    fn every_plugin_declares_sane_install_scripts() {
        use crate::install::InstallScript;
        for p in crate::all() {
            for windows in [true, false] {
                let Some(s) = p.install_script(windows) else { continue };
                match s {
                    InstallScript::Fetch { url, .. } => {
                        assert!(url.starts_with("https://"), "{} 的安装地址必须是 https：{url}", p.id());
                    }
                    InstallScript::Command { body, .. } => {
                        assert!(!body.trim().is_empty(), "{} 的安装命令为空", p.id());
                    }
                }
                assert!(
                    matches!(s.unix_shell(), "bash" | "sh"),
                    "{} 声明了未知解释器：{}",
                    p.id(),
                    s.unix_shell()
                );
            }
        }
    }

    /// 声明了直下的 agent 必须**同时**留着引导脚本作回退：发布物 schema 变了、或下载服务本地区
    /// 不可用时，`plan()` 会失败，此时得有路可走。
    #[test]
    fn direct_install_always_keeps_a_script_fallback() {
        for p in crate::all() {
            if p.direct_install().is_some() {
                assert!(
                    p.install_script(true).is_some() && p.install_script(false).is_some(),
                    "{} 声明了直下，但没留引导脚本回退",
                    p.id()
                );
            }
        }
    }

    /// claude 走直下（绕开 claude.ai 的 Cloudflare）；kimi/codex 暂时只有引导脚本。
    /// codex 的二进制在 GitHub Releases，也能直下——但 GitHub API 有未认证限流，另议。
    #[test]
    fn only_claude_declares_direct_install_for_now() {
        assert!(crate::by_id("claude").unwrap().direct_install().is_some());
        assert!(crate::by_id("kimi").unwrap().direct_install().is_none());
        assert!(crate::by_id("codex").unwrap().direct_install().is_none());
    }

    /// **没有直下能力的 agent，其引导脚本入口不得落在已知会做人机校验的域上。**
    ///
    /// `claude.ai` 与 `chatgpt.com` 实测 `server: cloudflare` + `cf-ray`，其校验页以 HTTP 200 返回，
    /// 会被裸管道当脚本执行。
    ///
    /// claude 允许把 `claude.ai` 留作回退：它的常规路径是直下（`downloads.claude.ai`，无 CF），
    /// 只有当发布物 schema 变了、`plan()` 失败时才落到引导脚本——那时被 CF 拦反倒是最不重要的问题。
    ///
    /// codex 没有直下，所以它**必须**避开 CF：改取 `chatgpt.com` 那个 302 的终点，也就是
    /// GitHub Releases（无 CF，内容逐字节相同）。
    ///
    /// kimi 的 `code.kimi.com` 是 nginx 直服，本就不在名单上。
    #[test]
    fn agents_without_direct_install_must_avoid_cloudflare_fronted_hosts() {
        /// 实测在 Cloudflare 后面（`server: cloudflare` + `cf-ray`）的域。
        const CF_FRONTED: [&str; 2] = ["claude.ai", "chatgpt.com"];

        for p in crate::all() {
            // 有直下 → 引导脚本只是回退路径，允许它落在 CF 后面。
            if p.direct_install().is_some() {
                continue;
            }
            for windows in [true, false] {
                let Some(s) = p.install_script(windows) else { continue };
                for host in CF_FRONTED {
                    assert!(
                        !s.source().contains(host),
                        "{} 没有直下能力，其唯一的安装入口却落在 Cloudflare 前置的 {host} 上：{}\n\
                         （该域的人机校验页以 HTTP 200 返回，会被当成脚本执行）",
                        p.id(),
                        s.source()
                    );
                }
            }
        }
    }

    /// 反过来守住上一条的前提：claude 之所以获准把 `claude.ai` 留作回退，正因为它有直下。
    /// 哪天直下被移除，上面那条会立刻把 `claude.ai` 判为违规。
    #[test]
    fn claude_may_keep_a_cloudflare_fallback_only_because_it_has_direct_install() {
        let claude = crate::by_id("claude").unwrap();
        assert!(claude.direct_install().is_some(), "claude 失去直下后，其 claude.ai 回退就不再可接受");
        assert!(claude.install_script(true).unwrap().source().contains("claude.ai"));
    }

    /// codex 必须直取 GitHub Releases 的稳定跳转。`chatgpt.com/codex/install.ps1` 只是它的 302，
    /// 内容逐字节相同，但那个域在 Cloudflare 后面。
    #[test]
    fn codex_bootstrap_comes_from_github_releases() {
        let p = crate::by_id("codex").unwrap();
        for windows in [true, false] {
            let s = p.install_script(windows).expect("codex 有一键安装");
            assert!(
                s.source().starts_with("https://github.com/openai/codex/releases/latest/download/"),
                "codex 应直取 GitHub Releases：{}",
                s.source()
            );
        }
    }

    /// kimi 的地址必须带 `/kimi-code/`——不带它装的是旧 Python `kimi-cli`，落到
    /// `~/.local/bin/kimi-cli.exe`，变体表的候选一个都命中不了，装完仍显示「未安装」。
    #[test]
    fn kimi_install_url_targets_the_node_edition() {
        let p = crate::by_id("kimi").unwrap();
        for windows in [true, false] {
            let s = p.install_script(windows).expect("kimi 有一键安装");
            assert!(s.source().contains("/kimi-code/"), "kimi 地址漏了 /kimi-code/：{}", s.source());
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
