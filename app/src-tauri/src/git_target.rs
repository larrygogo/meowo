//! 推送去向的单点判定：「更改」面板摘要（git.rs 的 upstream / ahead / unpushed / pushTarget）、推送
//! （forge::push_with）、PR 的 head 都只用 [`resolve`]，不再各算各的。
//!
//! 为什么单独一处：做空审查第二轮（scratchpad/redteam2r 的 rt2_ 四个实验）里，摘要用 `split_once('/')`
//! 把上游拆成「远端/分支」，推送读 branch.<b>.* 配置，两边对带斜杠的远端名（team/fork）、remote.pushDefault、
//! 跟踪不同名功能分支的结论各不相同：界面写「推到 origin/feat」，实际推去了 team/fork。
//!
//! 配置靠直接读文件解析（全局 + 仓库 + worktree 专属），不起 git 进程：摘要有进程数预算
//! （git.rs summary_spawns_at_most_four_git_processes，Windows 上每个 git 进程约 30ms）。
//! 不跟 include / includeIf 引入的文件、不读系统级配置：这些地方写 pushDefault、branch.* 的极少，
//! 读不到时按没配处理（报告里列为边界）。

use std::path::{Path, PathBuf};

/// 解析出来的 git 配置项：(节名小写, 子节原样, 键名小写, 值)。同键多次出现时后写的算数（同 git）。
#[derive(Debug, Default, Clone)]
pub(crate) struct GitConfig {
    entries: Vec<(String, Option<String>, String, String)>,
}

impl GitConfig {
    /// 解析一份 config 文本。认 `[节]`、`[节 "子节"]`、旧式 `[节.子节]`，`#`/`;` 注释、引号与转义、行尾 `\` 续行、
    /// 节头后同一行跟键值（git 允许）。
    pub(crate) fn parse(text: &str) -> Self {
        let mut cfg = GitConfig::default();
        let mut section: Option<(String, Option<String>)> = None;
        let mut lines = text.lines();
        while let Some(raw) = lines.next() {
            let mut line = raw.trim_start().to_string();
            if line.starts_with('[') {
                let Some(end) = header_end(&line) else { continue };
                section = parse_header(&line[1..end]);
                line = line[end + 1..].trim_start().to_string();
            }
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            // 续行：值里未在引号内、未被注释的行尾反斜杠接下一行。
            while ends_with_continuation(&line) {
                line.pop();
                match lines.next() {
                    Some(next) => line.push_str(next),
                    None => break,
                }
            }
            let Some((sec, sub)) = section.clone() else { continue };
            let (key, value) = match line.split_once('=') {
                Some((k, v)) => (k.trim().to_ascii_lowercase(), unquote(v)),
                // 只有键名 = 布尔 true。
                None => (strip_comment(&line).trim().to_ascii_lowercase(), "true".to_string()),
            };
            if !key.is_empty() {
                cfg.entries.push((sec, sub, key, value));
            }
        }
        cfg
    }

    fn extend(&mut self, other: GitConfig) {
        self.entries.extend(other.entries);
    }

    pub(crate) fn get(&self, section: &str, sub: Option<&str>, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|(s, b, k, _)| s == section && b.as_deref() == sub && k.eq_ignore_ascii_case(key))
            .map(|(_, _, _, v)| v.as_str())
    }

    /// 配置里出现过的远端（按首次出现的顺序）。git 认「有 remote.<名>.* 配置」就算有这个远端。
    pub(crate) fn remotes(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for (s, sub, _, _) in &self.entries {
            if let (true, Some(name)) = (s == "remote", sub) {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// 读一个仓库的有效配置：全局（GIT_CONFIG_GLOBAL，或 XDG 与 ~/.gitconfig）→ 公共 .git 目录的 config →
    /// 开了 extensions.worktreeConfig 时 worktree 自己的 config.worktree，后读的覆盖先读的（同 git 的优先级）。
    /// env：取环境变量（正式调用读进程环境，forge 的测试把全局配置隔离到空文件）。
    pub(crate) fn load(common_dir: &Path, git_dir: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Self {
        let mut cfg = GitConfig::default();
        for path in global_config_paths(env) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                cfg.extend(GitConfig::parse(&text));
            }
        }
        if let Ok(text) = std::fs::read_to_string(common_dir.join("config")) {
            cfg.extend(GitConfig::parse(&text));
        }
        if let Some(git_dir) = git_dir {
            if cfg.get("extensions", None, "worktreeconfig").is_some_and(is_true) {
                if let Ok(text) = std::fs::read_to_string(git_dir.join("config.worktree")) {
                    cfg.extend(GitConfig::parse(&text));
                }
            }
        }
        cfg
    }
}

fn is_true(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1")
}

/// 节头的右括号位置（子节名在引号里，可能含 `]`）。
fn header_end(line: &str) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in line.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ']' if !quoted => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_header(inner: &str) -> Option<(String, Option<String>)> {
    let inner = inner.trim();
    if let Some(at) = inner.find(|c: char| c.is_whitespace()) {
        let name = inner[..at].to_ascii_lowercase();
        let rest = inner[at..].trim();
        let sub = rest.strip_prefix('"')?.strip_suffix('"')?;
        let mut out = String::new();
        let mut chars = sub.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                out.extend(chars.next());
            } else {
                out.push(c);
            }
        }
        return Some((name, Some(out)));
    }
    // 旧式 [branch.feat]：子节名被 git 转成小写。
    match inner.split_once('.') {
        Some((name, sub)) => Some((name.to_ascii_lowercase(), Some(sub.to_ascii_lowercase()))),
        None => Some((inner.to_ascii_lowercase(), None)),
    }
}

/// 去掉引号外的注释。
fn strip_comment(text: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => return &text[..i],
            _ => {}
        }
    }
    text
}

fn ends_with_continuation(line: &str) -> bool {
    let body = strip_comment(line);
    if body.len() != line.len() {
        return false;
    }
    let trailing = line.chars().rev().take_while(|c| *c == '\\').count();
    trailing % 2 == 1
}

/// 值：去注释、去首尾空白、去引号、处理转义（\" \\ \n \t）。
fn unquote(raw: &str) -> String {
    let value = strip_comment(raw).trim();
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {}
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('b') => {
                    out.pop();
                }
                Some(other) => out.push(other),
                None => {}
            },
            _ => out.push(c),
        }
    }
    out
}

/// 全局配置文件：GIT_CONFIG_GLOBAL 设了只读它；否则 XDG（$XDG_CONFIG_HOME/git/config 或 ~/.config/git/config）
/// 再 ~/.gitconfig（git 的读取顺序，后者优先）。家目录：HOME，Windows 上没设时 USERPROFILE（同 Git for Windows）。
fn global_config_paths(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    if let Some(path) = env("GIT_CONFIG_GLOBAL") {
        return vec![PathBuf::from(path)];
    }
    let home = env("HOME").or_else(|| env("USERPROFILE")).filter(|h| !h.is_empty()).map(PathBuf::from);
    let xdg = env("XDG_CONFIG_HOME")
        .filter(|x| !x.is_empty())
        .map(|x| PathBuf::from(x).join("git").join("config"))
        .or_else(|| home.as_ref().map(|h| h.join(".config").join("git").join("config")));
    xdg.into_iter().chain(home.map(|h| h.join(".gitconfig"))).collect()
}

/// 进程环境（正式调用）。
pub(crate) fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

// ═══ 推送去向 ═══

/// 分支配置的上游：branch.<b>.remote + branch.<b>.merge（远端名原样取，带斜杠也不拆）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Upstream {
    pub(crate) remote: String,
    pub(crate) branch: String,
}

impl Upstream {
    pub(crate) fn label(&self) -> String {
        format!("{}/{}", self.remote, self.branch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PushTarget {
    pub(crate) remote: String,
    /// 推到远端的哪个分支（refspec `HEAD:refs/heads/<branch>`；PR 的 head 也是它）。
    pub(crate) branch: String,
    /// 推的时候带 -u，推完上游就是 remote/branch。
    pub(crate) set_upstream: bool,
    /// 仍然算数的上游（拉取来源，摘要的 upstream / ↑↓ 按它）。上游是远端默认分支而本地不是它时为 None：
    /// 那是 `git worktree add -b feat <路径> origin/main` 带出来的，不是用户想要的跟踪关系。
    pub(crate) upstream: Option<Upstream>,
}

impl PushTarget {
    pub(crate) fn label(&self) -> String {
        format!("{}/{}", self.remote, self.branch)
    }
    /// 推送正是推到上游（↑ 就是未推数）。
    pub(crate) fn is_upstream(&self) -> bool {
        self.upstream.as_ref().is_some_and(|u| u.remote == self.remote && u.branch == self.branch)
    }
}

/// 远端的默认分支：refs/remotes/<远端>/HEAD 指向的（clone / `git remote set-head` 写的符号引用，永远是散文件，
/// 不进 packed-refs；reftable 格式的仓库读不到，退回 main/master 判定）。
pub(crate) fn remote_head(common_dir: &Path, remote: &str) -> Option<String> {
    let text = std::fs::read_to_string(common_dir.join("refs").join("remotes").join(remote).join("HEAD")).ok()?;
    let target = text.trim().strip_prefix("ref:")?.trim();
    target.strip_prefix(&format!("refs/remotes/{remote}/")).map(str::to_string)
}

/// 是不是远端的默认分支：远端 HEAD 指向它，或者它叫 main / master（远端 HEAD 不在时也挡住，绝不推进默认分支）。
pub(crate) fn is_default_branch(common_dir: &Path, remote: &str, branch: &str) -> bool {
    matches!(branch, "main" | "master") || remote_head(common_dir, remote).as_deref() == Some(branch)
}

/// 推送去向。远端：branch.<b>.pushRemote → remote.pushDefault → 上游所在远端 → origin → 唯一 / 第一个远端
/// （前四步同 git 的 remote_for_branch / pushremote_for_branch；最后一步是 git 会报错的情形，Meowo 退而求其次）。
/// 分支：
/// - 上游同名：推它（推到别的远端时也是同名分支，同 push.default=simple 的三角工作流）。
/// - 上游是远端默认分支、本地不是它：推同名分支并设上游（第一轮做空 redteam/push-main：以前照上游推进了 main）。
/// - 上游是别的功能分支（`git switch -c fix --track origin/bugfix-123`）：推送远端就是上游远端时推到它，
///   尊重用户的跟踪关系；推到别的远端时推同名分支，上游不动。
/// - 没上游：推同名分支并设上游。
///
/// 没有远端时 None。
pub(crate) fn resolve(cfg: &GitConfig, common_dir: &Path, branch: &str) -> Option<PushTarget> {
    let remotes = cfg.remotes();
    let known = |name: &str| remotes.iter().any(|r| r == name);
    let upstream = cfg
        .get("branch", Some(branch), "remote")
        // 「.」= 跟踪的是本地分支，不算远端上游。
        .filter(|r| known(r))
        .zip(cfg.get("branch", Some(branch), "merge"))
        .map(|(remote, merge)| Upstream {
            remote: remote.to_string(),
            branch: merge.strip_prefix("refs/heads/").unwrap_or(merge).to_string(),
        })
        .filter(|u| !u.branch.is_empty());
    let remote = cfg
        .get("branch", Some(branch), "pushremote")
        .filter(|r| known(r))
        .or_else(|| cfg.get("remote", None, "pushdefault").filter(|r| known(r)))
        .map(str::to_string)
        .or_else(|| upstream.as_ref().map(|u| u.remote.clone()))
        .or_else(|| remotes.iter().find(|r| *r == "origin").cloned())
        .or_else(|| remotes.first().cloned())?;
    let same = |b: &str| PushTarget { remote: remote.clone(), branch: b.to_string(), set_upstream: false, upstream: upstream.clone() };
    Some(match &upstream {
        None => PushTarget { set_upstream: true, ..same(branch) },
        Some(u) if u.branch == branch => same(branch),
        Some(u) if is_default_branch(common_dir, &u.remote, &u.branch) => {
            PushTarget { remote: remote.clone(), branch: branch.to_string(), set_upstream: true, upstream: None }
        }
        Some(u) if u.remote == remote => same(&u.branch),
        Some(_) => same(branch),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> GitConfig {
        GitConfig::parse(text)
    }

    #[test]
    fn 配置解析_子节带斜杠与引号_注释_旧式节头_续行() {
        let c = cfg(concat!(
            "[remote \"team/fork\"]\n\turl = https://x/y.git ; 注释\n",
            "[branch \"feat/a\"] remote = team/fork\n\tmerge = \"refs/heads/feat/a\" # 注释\n",
            "[Remote]\n\tPushDefault = team/fork\n",
            "[branch.OLD]\n\tremote = origin\n",
            "[core]\n\tbare\n\tsshCommand = ssh \\\n -v\n",
        ));
        assert_eq!(c.remotes(), vec!["team/fork".to_string()]);
        assert_eq!(c.get("branch", Some("feat/a"), "remote"), Some("team/fork"));
        assert_eq!(c.get("branch", Some("feat/a"), "merge"), Some("refs/heads/feat/a"));
        assert_eq!(c.get("remote", None, "pushdefault"), Some("team/fork"));
        assert_eq!(c.get("branch", Some("old"), "remote"), Some("origin"));
        assert_eq!(c.get("core", None, "bare"), Some("true"));
        assert_eq!(c.get("core", None, "sshcommand"), Some("ssh  -v"));
    }

    #[test]
    fn 推送去向_各种上游形态() {
        let dir = std::env::temp_dir().join(format!("meowo-git-target-{}", std::process::id()));
        let base = "[remote \"origin\"]\n\turl = o\n[remote \"fork\"]\n\turl = f\n";
        let t = |extra: &str, b: &str| resolve(&cfg(&format!("{base}{extra}")), &dir, b).unwrap();
        // 没上游
        let x = t("", "feat");
        assert_eq!((x.label(), x.set_upstream, x.upstream), ("origin/feat".into(), true, None));
        // 上游同名
        let x = t("[branch \"feat\"]\nremote=origin\nmerge=refs/heads/feat\n", "feat");
        assert_eq!((x.label(), x.set_upstream, x.is_upstream()), ("origin/feat".into(), false, true));
        // 上游是默认分支
        let x = t("[branch \"feat\"]\nremote=origin\nmerge=refs/heads/main\n", "feat");
        assert_eq!((x.label(), x.set_upstream, x.upstream), ("origin/feat".into(), true, None));
        // 本地就是 main
        let x = t("[branch \"main\"]\nremote=origin\nmerge=refs/heads/main\n", "main");
        assert_eq!((x.label(), x.set_upstream), ("origin/main".into(), false));
        // 上游是别的功能分支
        let x = t("[branch \"fix\"]\nremote=origin\nmerge=refs/heads/bugfix-123\n", "fix");
        assert_eq!((x.label(), x.set_upstream, x.is_upstream()), ("origin/bugfix-123".into(), false, true));
        // pushDefault 压过上游远端：同名推到 fork，上游照旧
        let x = t("[remote]\npushDefault=fork\n[branch \"feat\"]\nremote=origin\nmerge=refs/heads/feat\n", "feat");
        assert_eq!((x.label(), x.set_upstream, x.is_upstream()), ("fork/feat".into(), false, false));
        // pushRemote 压过 pushDefault；指向不存在的远端时忽略
        let x = t("[remote]\npushDefault=origin\n[branch \"feat\"]\npushRemote=fork\n", "feat");
        assert_eq!(x.label(), "fork/feat");
        let x = t("[remote]\npushDefault=nope\n", "feat");
        assert_eq!(x.label(), "origin/feat");
        // 上游是本地分支（remote = .）不算
        let x = t("[branch \"feat\"]\nremote=.\nmerge=refs/heads/main\n", "feat");
        assert_eq!((x.label(), x.set_upstream), ("origin/feat".into(), true));
        // 没有远端
        assert!(resolve(&cfg(""), &dir, "feat").is_none());
    }

    #[test]
    fn 远端默认分支_读远端_head_符号引用() {
        let dir = std::env::temp_dir().join(format!("meowo-git-target-head-{}", std::process::id()));
        let refs = dir.join("refs/remotes/team/fork");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::write(refs.join("HEAD"), "ref: refs/remotes/team/fork/develop\n").unwrap();
        assert_eq!(remote_head(&dir, "team/fork").as_deref(), Some("develop"));
        assert!(is_default_branch(&dir, "team/fork", "develop"));
        assert!(is_default_branch(&dir, "team/fork", "main"), "main/master 一律当默认分支挡住");
        assert!(!is_default_branch(&dir, "team/fork", "bugfix-123"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
