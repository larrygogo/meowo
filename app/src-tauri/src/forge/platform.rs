//! 代码托管平台的识别：远端地址 → 主机 + 仓库路径 → 哪个平台、用哪个账号。
//!
//! - 公共主机内置：github.com、gitlab.com、bitbucket.org、codeberg.org（Forgejo，按 Gitea 接口）。
//! - 自建实例（GitHub 企业版、GitLab 自管、Gitea / Forgejo）：用户在设置里添加账号时声明「主机 + 平台」，
//!   之后这台主机的远端才认（[`Forges::platform`]）。没声明的主机照旧当作「不是 GitHub」：push 照常，
//!   标题栏仍可交给本机 gh（gh 自己认得企业版）。
//! - github.com 不走账号表：沿用设置里的「GitHub 连接方式」（本机 gh / Meowo 登录 / 关闭）。
//!
//! 只认主机名，不认路径前缀（装在子路径下的自管 GitLab，如 `https://host/gitlab/group/repo`，本轮不支持）。

use super::api::ForgeApi;

/// 平台。GitHub 企业版与 github.com 同属 Github（接口同一套，基址不同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    Github,
    Gitlab,
    Gitea,
    Bitbucket,
}

impl Platform {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Platform::Github => "github",
            Platform::Gitlab => "gitlab",
            Platform::Gitea => "gitea",
            Platform::Bitbucket => "bitbucket",
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Platform> {
        match text {
            "github" => Some(Platform::Github),
            "gitlab" => Some(Platform::Gitlab),
            "gitea" | "forgejo" => Some(Platform::Gitea),
            "bitbucket" => Some(Platform::Bitbucket),
            _ => None,
        }
    }

    /// 仓库路径的段数要求：GitLab 可以有子组（≥2 段），其余正好 owner/repo 两段。
    fn path_ok(self, segments: usize) -> bool {
        match self {
            Platform::Gitlab => segments >= 2,
            _ => segments == 2,
        }
    }
}

/// 公共主机（别名归一后）。
pub(crate) const GITHUB_COM: &str = "github.com";

/// 别名归一：ssh 专用主机、www 前缀都按网页主机算。
fn canonical_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.as_str() {
        "www.github.com" | "ssh.github.com" => GITHUB_COM.into(),
        "altssh.gitlab.com" | "www.gitlab.com" => "gitlab.com".into(),
        "altssh.bitbucket.org" | "www.bitbucket.org" => "bitbucket.org".into(),
        "www.codeberg.org" => "codeberg.org".into(),
        _ => host,
    }
}

/// 内置认得的公共主机。
pub(crate) fn public_platform(host: &str) -> Option<Platform> {
    match canonical_host(host).as_str() {
        GITHUB_COM => Some(Platform::Github),
        "gitlab.com" => Some(Platform::Gitlab),
        "bitbucket.org" => Some(Platform::Bitbucket),
        "codeberg.org" => Some(Platform::Gitea),
        _ => None,
    }
}

/// 主机名（含可选端口）是否像样：只收字母数字、`-`、`.`，端口是数字。拼进 URL 与凭据库条目名都安全。
pub(crate) fn valid_host(host: &str) -> bool {
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) => (name, Some(port)),
        None => (host, None),
    };
    let name_ok = !name.is_empty()
        && name.len() <= 253
        && name.contains('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && !name.starts_with(['-', '.'])
        && !name.ends_with(['-', '.']);
    let port_ok =
        port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n > 0));
    name_ok && port_ok
}

/// 用户输入的主机 → 归一的写法（去掉协议、路径、末尾斜杠，小写）。不像样返回 None。
pub(crate) fn normalize_host_input(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text.strip_prefix("https://").or_else(|| text.strip_prefix("http://")).unwrap_or(text);
    let host = text.split('/').next()?.rsplit('@').next()?;
    let host = canonical_host(host);
    valid_host(&host).then_some(host)
}

/// 去掉端口的主机名（账号匹配用：ssh 远端的端口是 ssh 端口，和网页端口无关）。
pub(crate) fn host_name(host: &str) -> &str {
    host.rsplit_once(':')
        .map(|(name, port)| if port.bytes().all(|b| b.is_ascii_digit()) { name } else { host })
        .unwrap_or(host)
}

/// 远端地址拆出的主机与仓库路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteUrl {
    /// 归一后的主机名（小写，不含端口）。
    pub(crate) host: String,
    /// 仓库路径（去掉 `.git` 与首尾斜杠），如 `owner/repo`、`group/sub/repo`。
    pub(crate) path: String,
}

/// 认 https / http / ssh / git 协议与 scp 写法（`git@host:owner/repo.git`）。本地路径（含 Windows 盘符）不认。
pub(crate) fn parse_remote_url(url: &str) -> Option<RemoteUrl> {
    let url = url.trim();
    let (host, path) = if let Some((scheme, after)) = url.split_once("://") {
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "https" | "http" | "ssh" | "git" | "git+ssh" | "ssh+git"
        ) {
            return None;
        }
        let (authority, path) = after.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        // [IPv6] 不认；端口去掉。
        let host = host.split(':').next()?;
        (host, path)
    } else {
        // scp 写法：[user@]host:path。冒号前不能有斜杠；单个字母是 Windows 盘符（C:/repo）。
        let (left, path) = url.split_once(':')?;
        if left.contains('/') || left.contains('\\') || left.len() <= 1 {
            return None;
        }
        (left.rsplit('@').next()?, path)
    };
    let host = canonical_host(host);
    if !valid_host(&host) {
        return None;
    }
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').collect();
    let seg_ok =
        |s: &&str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) && *s != "." && *s != "..";
    if segments.len() < 2 || !segments.iter().all(seg_ok) {
        return None;
    }
    Some(RemoteUrl {
        host,
        path: path.to_string(),
    })
}

/// 主机 → 平台、平台接口。生产实现在 forge_accounts.rs（公共主机 + 账号里声明的自建主机）；测试用
/// [`NO_ACCOUNTS`]（只认公共主机、谁都没登录）或注入 HTTP 的假实现。
pub(crate) trait Forges: Sync {
    /// 这台主机（不含端口）是哪个平台。
    fn platform(&self, host: &str) -> Option<Platform>;
    /// 这台主机的平台接口（有账号才有）；github.com 不经这里（走 GitHub 连接方式）。None = 没有账号。
    fn open(&self, host: &str) -> Option<Box<dyn ForgeApi + '_>>;
    /// 这台主机能不能查 PR（不联网）：github.com 恒能（走 GitHub 连接方式），别的主机要有账号。挑 PR 所在远端时
    /// 先在能用的主机里挑。
    fn usable(&self, host: &str) -> bool {
        host_name(host) == GITHUB_COM
    }
}

/// 只认公共主机、没有任何账号。
pub(crate) struct NoAccounts;

impl Forges for NoAccounts {
    fn platform(&self, host: &str) -> Option<Platform> {
        public_platform(host)
    }
    fn open(&self, _host: &str) -> Option<Box<dyn ForgeApi + '_>> {
        None
    }
}

pub(crate) static NO_ACCOUNTS: NoAccounts = NoAccounts;

/// 远端地址 → (主机, 仓库路径, 平台)。主机认不出、路径段数不合平台要求时 None。
pub(crate) fn recognize(forges: &dyn Forges, url: &str) -> Option<(String, String, Platform)> {
    let remote = parse_remote_url(url)?;
    let platform = forges.platform(&remote.host)?;
    platform
        .path_ok(remote.path.split('/').count())
        .then_some((remote.host, remote.path, platform))
}

/// 仓库网页首页的主机部分（`https://<主机>`）。
pub(crate) fn web_base(host: &str) -> String {
    format!("https://{host}")
}

/// 「在网页上新建 PR / MR」的链接（没装 gh、没账号时开 PR 打开它）。head_owner：fork 时 head 所在仓库的 owner。
pub(crate) fn compare_url(platform: Platform, host: &str, slug: &str, base: &str, head: &str, head_owner: Option<&str>) -> String {
    let repo = format!("{}/{slug}", web_base(host));
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    match platform {
        Platform::Github => {
            let head = match head_owner {
                Some(owner) => format!("{owner}:{}", path_escape(head)),
                None => path_escape(head),
            };
            format!("{repo}/compare/{}...{head}?expand=1", path_escape(base))
        }
        // GitLab：新建 MR 页带上源与目标分支（fork 时网页上自己选源项目）。
        Platform::Gitlab => format!(
            "{repo}/-/merge_requests/new?{}={}&{}={}",
            enc("merge_request[source_branch]"),
            enc(head),
            enc("merge_request[target_branch]"),
            enc(base)
        ),
        // Gitea / Forgejo：compare 页，fork 写 owner:分支。
        Platform::Gitea => {
            let head = match head_owner {
                Some(owner) => format!("{owner}:{}", path_escape(head)),
                None => path_escape(head),
            };
            format!("{repo}/compare/{}...{head}", path_escape(base))
        }
        Platform::Bitbucket => format!("{repo}/pull-requests/new?source={}&dest={}", enc(head), enc(base)),
    }
}

/// 路径里能原样出现的字符之外都转义（分支名里的 `/` 保留）。
pub(crate) fn path_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// PR / MR 网页链接 → (主机, 仓库路径, 编号, 平台的链接形状)。只认四种形状：
/// GitHub `/<o>/<r>/pull/<n>`、GitLab `/<组…>/<r>/-/merge_requests/<n>`、Gitea `/<o>/<r>/pulls/<n>`、
/// Bitbucket `/<ws>/<r>/pull-requests/<n>`。尾部的 `/files`、`#…`、`?…` 去掉。
pub(crate) fn parse_pr_url(url: &str) -> Option<(String, String, u32, Platform)> {
    let rest = url.trim().strip_prefix("https://")?;
    let rest = rest.split(['#', '?']).next()?;
    let (host, path) = rest.split_once('/')?;
    let host = canonical_host(host);
    if !valid_host(&host) || path.chars().any(char::is_whitespace) {
        return None;
    }
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    let number_at = |i: usize| {
        parts
            .get(i)
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse::<u32>().ok())
    };
    if let Some(i) = parts.iter().position(|p| *p == "-") {
        if parts.get(i + 1) == Some(&"merge_requests") && i >= 2 {
            let number = number_at(i + 2)?;
            return Some((host, parts[..i].join("/"), number, Platform::Gitlab));
        }
        return None;
    }
    if parts.len() < 4 || parts[..2].iter().any(|p| p.is_empty()) {
        return None;
    }
    let platform = match parts[2] {
        "pull" => Platform::Github,
        "pulls" => Platform::Gitea,
        "pull-requests" => Platform::Bitbucket,
        _ => return None,
    };
    let number = number_at(3)?;
    Some((host, format!("{}/{}", parts[0], parts[1]), number, platform))
}

/// 仓库在缓存、合并方式记录里的键：github.com 只用 owner/repo（与改版前一致，老记录照用），别的主机带主机名。
pub(crate) fn repo_key(host: &str, slug: &str) -> String {
    if host == GITHUB_COM {
        slug.to_string()
    } else {
        format!("{host}/{slug}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 远端地址_各种写法都拆得出主机与路径() {
        let cases = [
            ("https://github.com/o/r.git", "github.com", "o/r"),
            ("git@github.com:o/r.git", "github.com", "o/r"),
            ("ssh://git@ssh.github.com:443/o/r.git", "github.com", "o/r"),
            ("https://gitlab.com/group/sub/repo.git", "gitlab.com", "group/sub/repo"),
            ("git@gitlab.com:group/sub/repo.git", "gitlab.com", "group/sub/repo"),
            ("ssh://git@altssh.gitlab.com:443/g/r.git", "gitlab.com", "g/r"),
            ("https://user@bitbucket.org/ws/repo.git", "bitbucket.org", "ws/repo"),
            ("git@bitbucket.org:ws/repo.git", "bitbucket.org", "ws/repo"),
            ("https://codeberg.org/forgejo/forgejo.git", "codeberg.org", "forgejo/forgejo"),
            ("https://git.example.com:8443/team/app/", "git.example.com", "team/app"),
            ("ssh://git@git.example.com:2222/team/app.git", "git.example.com", "team/app"),
        ];
        for (url, host, path) in cases {
            let parsed = parse_remote_url(url).unwrap_or_else(|| panic!("{url}"));
            assert_eq!((parsed.host.as_str(), parsed.path.as_str()), (host, path), "{url}");
        }
        for bad in [
            "C:/repos/r.git",
            "/srv/git/r.git",
            "file:///srv/r.git",
            "https://host/onlyone",
            "https://localhost/o/r",
            "https://evil.com/o/../r",
        ] {
            assert_eq!(parse_remote_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn 公共主机自动识别_自建主机要账号声明() {
        let f = &NO_ACCOUNTS;
        assert_eq!(recognize(f, "https://github.com/o/r").map(|r| r.2), Some(Platform::Github));
        assert_eq!(recognize(f, "git@gitlab.com:g/s/r.git").map(|r| r.2), Some(Platform::Gitlab));
        assert_eq!(recognize(f, "https://bitbucket.org/ws/r").map(|r| r.2), Some(Platform::Bitbucket));
        assert_eq!(recognize(f, "https://codeberg.org/o/r").map(|r| r.2), Some(Platform::Gitea));
        assert_eq!(recognize(f, "https://git.example.com/o/r"), None, "没声明的自建主机不认");
        // 段数不合平台：GitHub / Bitbucket / Gitea 只认两段。
        assert_eq!(recognize(f, "https://github.com/o/r/extra"), None);
        assert_eq!(recognize(f, "https://bitbucket.org/ws/a/b"), None);
    }

    #[test]
    fn pr_链接_四种形状() {
        assert_eq!(
            parse_pr_url("https://github.com/o/r/pull/12"),
            Some(("github.com".into(), "o/r".into(), 12, Platform::Github))
        );
        assert_eq!(
            parse_pr_url("https://github.com/o/r/pull/12/files#diff"),
            Some(("github.com".into(), "o/r".into(), 12, Platform::Github))
        );
        assert_eq!(
            parse_pr_url("https://gitlab.com/gitlab-org/cli/-/merge_requests/2045"),
            Some(("gitlab.com".into(), "gitlab-org/cli".into(), 2045, Platform::Gitlab))
        );
        assert_eq!(
            parse_pr_url("https://gitlab.com/a/b/c/-/merge_requests/7/diffs"),
            Some(("gitlab.com".into(), "a/b/c".into(), 7, Platform::Gitlab))
        );
        assert_eq!(
            parse_pr_url("https://codeberg.org/forgejo/forgejo/pulls/9"),
            Some(("codeberg.org".into(), "forgejo/forgejo".into(), 9, Platform::Gitea))
        );
        assert_eq!(
            parse_pr_url("https://bitbucket.org/ws/r/pull-requests/3"),
            Some(("bitbucket.org".into(), "ws/r".into(), 3, Platform::Bitbucket))
        );
        for bad in [
            "http://github.com/o/r/pull/1",
            "https://github.com/o/r/issues/1",
            "https://github.com/o/r/pull/x",
            "https://gitlab.com/g/-/issues/1",
            "https://github.com/o r/r/pull/1",
        ] {
            assert_eq!(parse_pr_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn 新建_pr_网页链接_按平台() {
        assert_eq!(
            compare_url(Platform::Github, "github.com", "o/r", "main", "feat/x", None),
            "https://github.com/o/r/compare/main...feat/x?expand=1"
        );
        assert_eq!(
            compare_url(Platform::Github, "github.com", "o/r", "main", "x", Some("me")),
            "https://github.com/o/r/compare/main...me:x?expand=1"
        );
        assert_eq!(
            compare_url(Platform::Gitlab, "gitlab.com", "g/r", "main", "feat/x", None),
            "https://gitlab.com/g/r/-/merge_requests/new?merge_request%5Bsource_branch%5D=feat%2Fx&merge_request%5Btarget_branch%5D=main"
        );
        assert_eq!(
            compare_url(Platform::Gitea, "codeberg.org", "o/r", "main", "x", None),
            "https://codeberg.org/o/r/compare/main...x"
        );
        assert_eq!(
            compare_url(Platform::Bitbucket, "bitbucket.org", "w/r", "main", "feat/x", None),
            "https://bitbucket.org/w/r/pull-requests/new?source=feat%2Fx&dest=main"
        );
    }

    #[test]
    fn 用户输入的主机_归一与拒绝() {
        assert_eq!(normalize_host_input(" https://Git.Example.com/ "), Some("git.example.com".into()));
        assert_eq!(normalize_host_input("git.example.com:8443/path"), Some("git.example.com:8443".into()));
        assert_eq!(normalize_host_input("localhost"), None, "没有点的主机名不收");
        assert_eq!(normalize_host_input("bad host.com"), None);
        assert_eq!(normalize_host_input("a.com:99999"), None);
        assert_eq!(host_name("git.example.com:8443"), "git.example.com");
    }
}
