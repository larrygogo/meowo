//! forge.rs 的测试。
//!
//! - push：真 git + 本地 bare 远端（无上游自动设上游、被拒、服务器钩子拒、本地钩子拒、无远端、detached），
//!   凭据失败用本机起的「回 401 的 HTTP 服务」模拟、超时用「接了连接不回话」的服务模拟，确认不会卡在密码提示上。
//! - gh：全部经注入的假 gh。fixtures 里不带 derived_ 前缀的是 2026-11-10 用 gh 2.96 对真实仓库只读录的
//!   `gh pr view --json <PR_FIELDS>`（larrygogo/meowo#161 全是 CheckRun；kubernetes/kubernetes 的
//!   StatusContext 与 CONFLICTING；facebook/react 的 FAILURE，检查项截到 5 条）；`repo_view_drill.json` 录自
//!   larrygogo/meowo-pr-drill。derived_ 开头的是在录制样例上改字段造的（无检查、进行中、UNKNOWN）。

use super::*;
use crate::git::{git_diff_summary_blocking, parse_branch_header, BranchHeader};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/forge/fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

struct Sandbox {
    root: PathBuf,
    config: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("meowo-forge-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("empty.gitconfig");
        std::fs::write(&config, "").unwrap();
        Sandbox { root, config }
    }
    /// 隔离用户的全局 / 系统 git 配置（凭据助手、hooksPath、askpass 都不许串进来）。
    fn env(&self) -> Vec<(&'static str, Option<String>)> {
        vec![
            ("GIT_CONFIG_NOSYSTEM", Some("1".into())),
            ("GIT_CONFIG_GLOBAL", Some(self.config.to_string_lossy().into_owned())),
            ("GIT_ASKPASS", None),
            ("SSH_ASKPASS", None),
            // 本机起的假远端不能被用户的代理环境变量带走。
            ("NO_PROXY", Some("127.0.0.1,localhost".into())),
            ("no_proxy", Some("127.0.0.1,localhost".into())),
            ("HTTP_PROXY", None),
            ("http_proxy", None),
            ("HTTPS_PROXY", None),
            ("https_proxy", None),
            ("ALL_PROXY", None),
            ("all_proxy", None),
        ]
    }
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &self.config)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
    fn bare(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q", "--bare", "-b", "main"]);
        dir
    }
    fn repo(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q", "-b", "main"]);
        self.commit(&dir, "README.md", "init");
        dir
    }
    fn commit(&self, dir: &Path, file: &str, message: &str) {
        std::fs::write(dir.join(file), format!("{message}\n")).unwrap();
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", message]);
    }
    /// 仓库连上 bare 远端，main 推上去并设好上游，再切到新分支 feat 提交两次。
    fn repo_with_remote(&self, tag: &str) -> (PathBuf, PathBuf) {
        let remote = self.bare(&format!("{tag}-remote.git"));
        let repo = self.repo(tag);
        self.git(&repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
        self.git(&repo, &["push", "-q", "-u", "origin", "main"]);
        self.git(&repo, &["switch", "-q", "-c", "feat"]);
        self.commit(&repo, "a.txt", "feat: a");
        self.commit(&repo, "b.txt", "feat: b");
        (repo, remote)
    }
    fn hook(&self, hooks_dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(hooks_dir).unwrap();
        let path = hooks_dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn s(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn push(sb: &Sandbox, dir: &Path) -> Result<PushResultDto, String> {
    push_timeout(sb, dir, Duration::from_secs(60))
}

fn push_timeout(sb: &Sandbox, dir: &Path, timeout: Duration) -> Result<PushResultDto, String> {
    let owned = sb.env();
    let env: Vec<(&str, Option<&str>)> = owned.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    push_with(&s(dir), &env, timeout)
}

// ═══ git status --branch 的分支行 ═══

#[test]
fn 分支行_各种写法都认得() {
    let h = parse_branch_header("main...origin/main [ahead 1, behind 2]");
    assert_eq!((h.branch.as_deref(), h.upstream.as_deref(), h.ahead, h.behind), (Some("main"), Some("origin/main"), Some(1), Some(2)));
    let h = parse_branch_header("feat/x...origin/feat/x");
    assert_eq!((h.ahead, h.behind), (Some(0), Some(0)), "同步时是 0 不是 None（以前 rev-list 也给 0）");
    let h = parse_branch_header("feat [ahead 3]");
    assert_eq!((h.branch.as_deref(), h.upstream), (Some("feat"), None));
    let h = parse_branch_header("feat");
    assert_eq!((h.branch.as_deref(), h.upstream, h.ahead), (Some("feat"), None, None));
    let gone = parse_branch_header("feat...origin/feat [gone]");
    assert_eq!((gone.branch.as_deref(), gone.upstream, gone.ahead), (Some("feat"), None, None), "上游删了按没上游");
    assert_eq!(parse_branch_header("HEAD (no branch)"), BranchHeader::default());
    assert_eq!(parse_branch_header("No commits yet on main"), BranchHeader::default());
    let diff = parse_branch_header("main...origin/main [different]");
    assert_eq!((diff.upstream.as_deref(), diff.ahead), (Some("origin/main"), None));
}

#[test]
fn 摘要_带上游与未推数_没上游时数不在远端上的提交() {
    let sb = Sandbox::new("summary");
    let (repo, _) = sb.repo_with_remote("r");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!(summary.branch.as_deref(), Some("feat"));
    assert_eq!(summary.upstream, None);
    assert_eq!(summary.unpushed, Some(2), "新分支还没推：两个提交都不在远端上");
    sb.git(&repo, &["switch", "-q", "main"]);
    sb.commit(&repo, "c.txt", "main: c");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!((summary.upstream.as_deref(), summary.ahead, summary.behind, summary.unpushed), (Some("origin/main"), Some(1), Some(0), Some(1)));
    // 没有远端：不给未推数（推不了，不显示推送按钮）。
    let lone = sb.repo("lone");
    sb.commit(&lone, "x.txt", "x");
    let summary = git_diff_summary_blocking(&s(&lone)).unwrap();
    assert_eq!((summary.branch.as_deref(), summary.upstream, summary.unpushed), (Some("main"), None, None));
}

// ═══ push ═══

#[test]
fn 推送_没上游时自动设上游_推完未推数归零() {
    let sb = Sandbox::new("push-up");
    let (repo, remote) = sb.repo_with_remote("r");
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "origin".into(), branch: "feat".into(), set_upstream: true, pushed: 2 });
    assert_eq!(sb.git(&remote, &["rev-parse", "feat"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!((summary.upstream.as_deref(), summary.unpushed), (Some("origin/feat"), Some(0)));
    // 有上游之后再提交再推：推到上游，不再设上游。
    sb.commit(&repo, "c.txt", "feat: c");
    let again = push(&sb, &repo).unwrap();
    assert_eq!((again.set_upstream, again.pushed), (false, 1));
}

/// 做空复现（redteam/push-main）：`git worktree add -b feat <路径> origin/main` 默认 autoSetupMerge，
/// feat 的上游是 origin/main（branch.feat.merge=refs/heads/main）。以前照上游推 `HEAD:refs/heads/main`，
/// 功能分支的提交直接进了远端 main。
fn feat_tracking_main(sb: &Sandbox, tag: &str) -> (PathBuf, PathBuf) {
    let (repo, remote) = sb.repo_with_remote(tag);
    sb.git(&repo, &["branch", "-q", "--set-upstream-to=origin/main", "feat"]);
    (repo, remote)
}

#[test]
fn 推送_上游是_origin_main_的功能分支_推到同名分支_远端_main_不动() {
    let sb = Sandbox::new("push-feat-main");
    let (repo, remote) = feat_tracking_main(&sb, "r");
    let main_before = sb.git(&remote, &["rev-parse", "main"]);
    // 摘要按「同名远端分支」口径：不拿 origin/main 算领先数，推送目标写明是新建的 origin/feat。
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!((summary.upstream.as_deref(), summary.ahead, summary.unpushed), (None, None, Some(2)));
    assert_eq!(summary.push_target.as_deref(), Some("origin/feat"));
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "origin".into(), branch: "feat".into(), set_upstream: true, pushed: 2 });
    assert_eq!(sb.git(&remote, &["rev-parse", "main"]), main_before, "远端 main 不能被功能分支推动");
    assert_eq!(sb.git(&remote, &["rev-parse", "feat"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    assert_eq!(sb.git(&repo, &["config", "--get", "branch.feat.merge"]), "refs/heads/feat", "上游改成同名分支");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!((summary.upstream.as_deref(), summary.unpushed, summary.push_target.as_deref()), (Some("origin/feat"), Some(0), Some("origin/feat")));
}

#[test]
fn 推送_本地就在_main_照常推_main() {
    let sb = Sandbox::new("push-main");
    let (repo, remote) = sb.repo_with_remote("r");
    sb.git(&repo, &["switch", "-q", "main"]);
    sb.commit(&repo, "m.txt", "main: m");
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "origin".into(), branch: "main".into(), set_upstream: false, pushed: 1 });
    assert_eq!(sb.git(&remote, &["rev-parse", "main"]), sb.git(&repo, &["rev-parse", "HEAD"]));
}

#[test]
fn 推送_远端有新提交被拒_提示先拉取() {
    let sb = Sandbox::new("push-reject");
    let (repo, remote) = sb.repo_with_remote("r");
    // 另一个克隆往 main 推了一个提交，本地 main 也有自己的提交：non-fast-forward。
    let other = sb.root.join("other");
    sb.git(&sb.root, &["clone", "-q", &remote.to_string_lossy(), &other.to_string_lossy()]);
    sb.commit(&other, "o.txt", "other");
    sb.git(&other, &["push", "-q", "origin", "main"]);
    sb.git(&repo, &["switch", "-q", "main"]);
    sb.commit(&repo, "m.txt", "mine");
    assert_eq!(push(&sb, &repo).unwrap_err(), "git/push_rejected");
    // 永不 force：远端的提交还在。
    assert_eq!(sb.git(&remote, &["rev-parse", "main"]), sb.git(&other, &["rev-parse", "HEAD"]));
}

#[test]
fn 推送_服务器钩子拒绝_归为被保护_带上服务器原话() {
    let sb = Sandbox::new("push-protected");
    let (repo, remote) = sb.repo_with_remote("r");
    sb.hook(&remote.join("hooks"), "pre-receive", "echo 'GH006: Protected branch update failed for refs/heads/feat.'\nexit 1");
    let err = push(&sb, &repo).unwrap_err();
    assert!(err.starts_with("git/push_protected"), "{err}");
    assert!(err.contains("GH006"), "{err}");
}

#[test]
fn 推送_本地_pre_push_钩子失败_带上钩子输出() {
    let sb = Sandbox::new("push-hook");
    let (repo, _) = sb.repo_with_remote("r");
    sb.hook(&repo.join(".git/hooks"), "pre-push", "echo 'lint failed: 3 errors'\nexit 1");
    let err = push(&sb, &repo).unwrap_err();
    assert!(err.starts_with("git/push_hook"), "{err}");
    assert!(err.contains("lint failed"), "{err}");
}

#[test]
fn 推送_没有远端或不在分支上_直接说清楚() {
    let sb = Sandbox::new("push-none");
    let repo = sb.repo("r");
    assert_eq!(push(&sb, &repo).unwrap_err(), "git/no_remote");
    let head = sb.git(&repo, &["rev-parse", "HEAD"]);
    sb.git(&repo, &["switch", "-q", "--detach", &head]);
    assert_eq!(push(&sb, &repo).unwrap_err(), "git/detached");
}

/// 本机起一个 HTTP 服务：每个请求都回 401 要求登录（模拟凭据失效的 https 远端）。
fn serve_401() -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    port
}

#[test]
fn 推送_凭据失败不弹窗不卡住_归为认证失败() {
    let sb = Sandbox::new("push-auth");
    let repo = sb.repo("r");
    let port = serve_401();
    sb.git(&repo, &["remote", "add", "origin", &format!("http://127.0.0.1:{port}/o/r.git")]);
    let start = Instant::now();
    let err = push(&sb, &repo).unwrap_err();
    // 真卡在等输入时，push 会一直耗到 120s 超时、返回 git/push_timeout；拿到 push_auth 就说明没等人。
    // 时限只防“慢到接近超时”：本机 0.3s，Windows CI 实测 20.6s（run 42852133839，runner 自带的
    // Git Credential Manager 在并行负载下启动慢），所以放到 60s，离 120s 超时还有一倍余量。
    assert_eq!(err, "git/push_auth");
    assert!(start.elapsed() < Duration::from_secs(60), "不该等用户输入：{:?}", start.elapsed());
}

#[test]
fn 推送_连不上远端_归为网络错误() {
    let sb = Sandbox::new("push-net");
    let repo = sb.repo("r");
    // 端口 1 没有服务：立刻 connection refused。
    sb.git(&repo, &["remote", "add", "origin", "http://127.0.0.1:1/o/r.git"]);
    assert_eq!(push(&sb, &repo).unwrap_err(), "git/push_network");
}

#[test]
fn 推送_远端不回话_超时杀掉() {
    let sb = Sandbox::new("push-timeout");
    let repo = sb.repo("r");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().take(4).flatten().collect();
        std::thread::sleep(Duration::from_secs(30));
        drop(held);
    });
    sb.git(&repo, &["remote", "add", "origin", &format!("http://127.0.0.1:{port}/o/r.git")]);
    let start = Instant::now();
    assert_eq!(push_timeout(&sb, &repo, Duration::from_secs(2)).unwrap_err(), "git/push_timeout");
    assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
}

#[test]
fn 推送失败归类_录制的服务器输出() {
    let out = |stderr: &str| CmdOut { code: Some(1), stderr: stderr.into(), ..Default::default() };
    // github.com 凭据失效（https）。
    assert_eq!(classify_push_failure(&out("remote: Invalid username or token. Password authentication is not supported for Git operations.\nfatal: Authentication failed for 'https://github.com/o/r.git/'")), "git/push_auth");
    // ssh 公钥不对。
    assert_eq!(classify_push_failure(&out("git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights")), "git/push_auth");
    // 规则集拒绝（GH013）也算保护分支。
    let ruleset = classify_push_failure(&out("remote: error: GH013: Repository rule violations found for refs/heads/main.\nremote: - Changes must be made through a pull request.\n ! [remote rejected] main -> main (push declined due to repository rule violations)\nerror: failed to push some refs"));
    assert!(ruleset.starts_with("git/push_protected"), "{ruleset}");
    assert!(ruleset.contains("pull request"), "{ruleset}");
    assert_eq!(classify_push_failure(&out("fatal: unable to access 'https://github.com/o/r.git/': Could not resolve host: github.com")), "git/push_network");
    assert_eq!(classify_push_failure(&CmdOut { timed_out: true, ..Default::default() }), "git/push_timeout");
    let other = classify_push_failure(&out("fatal: 'nowhere' does not appear to be a git repository"));
    assert!(other.starts_with("git/push_failed: "), "{other}");
}

#[test]
fn 后台推送任务_同一仓库连点只推一次_结果可轮询() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Mutex::new(rx);
    let first = start_push_job("C:/forge-job-test".into(), move |_| {
        let _ = rx.lock().unwrap().recv_timeout(Duration::from_secs(5));
        Ok(PushResultDto { remote: "origin".into(), branch: "feat".into(), set_upstream: true, pushed: 2 })
    });
    let second = start_push_job("c:\\forge-job-test".into(), |_| Err("不该跑到这里".into()));
    assert_eq!(first, second, "同一仓库推送进行中：返回同一个任务号");
    assert_eq!(job_status(&first).unwrap().state, "running");
    tx.send(()).unwrap();
    let start = Instant::now();
    let done = loop {
        let job = job_status(&first).unwrap();
        if job.state != "running" || start.elapsed() > Duration::from_secs(5) {
            break job;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(done.state, "ok");
    assert_eq!(done.push.unwrap().pushed, 2);
    assert_eq!(job_status("push-nope").unwrap_err(), "git/job_not_found");
}

// ═══ pull（walkthrough P-1：推送被拒后的「拉取并重试」）═══

fn pull(sb: &Sandbox, dir: &Path) -> Result<PullResultDto, String> {
    let owned = sb.env();
    let env: Vec<(&str, Option<&str>)> = owned.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    pull_with(&s(dir), &env, Duration::from_secs(60))
}

/// 被拒的场景：另一个克隆往远端 main 推了 o.txt，本地 main 有自己的提交（同「推送_远端有新提交被拒」）。
fn diverged_main(sb: &Sandbox, tag: &str, other_file: &str, other_text: &str) -> (PathBuf, PathBuf, PathBuf) {
    let (repo, remote) = sb.repo_with_remote(tag);
    let other = sb.root.join(format!("{tag}-other"));
    sb.git(&sb.root, &["clone", "-q", &remote.to_string_lossy(), &other.to_string_lossy()]);
    std::fs::write(other.join(other_file), other_text).unwrap();
    sb.git(&other, &["add", "-A"]);
    sb.git(&other, &["commit", "-q", "-m", "other"]);
    sb.git(&other, &["push", "-q", "origin", "main"]);
    sb.git(&repo, &["switch", "-q", "main"]);
    sb.commit(&repo, "m.txt", "mine");
    (repo, remote, other)
}

/// 合并提交要身份：沙箱的全局配置是空文件，测试里写进去（同用户在面板里填了身份）。
fn with_identity(sb: &Sandbox) {
    std::fs::write(&sb.config, "[user]\n\tname = t\n\temail = t@t\n[commit]\n\tgpgsign = false\n").unwrap();
}

#[test]
fn 拉取_分叉无冲突_合并后再推就成功() {
    let sb = Sandbox::new("pull-merge");
    let (repo, remote, other) = diverged_main(&sb, "r", "o.txt", "other\n");
    with_identity(&sb);
    assert_eq!(push(&sb, &repo).unwrap_err(), "git/push_rejected");
    let result = pull(&sb, &repo).unwrap();
    assert_eq!((result.remote.as_str(), result.branch.as_str(), result.pulled, result.mode.as_str()), ("origin", "main", 1, "merge"));
    assert!(repo.join("o.txt").exists(), "远端的提交接进来了");
    // 合并后推送是快进，不用 force。
    push(&sb, &repo).unwrap();
    assert_eq!(sb.git(&remote, &["rev-parse", "main"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    let theirs = sb.git(&other, &["rev-parse", "HEAD"]);
    sb.git(&remote, &["merge-base", "--is-ancestor", &theirs, "main"]);
}

#[test]
fn 拉取_有冲突_不合并_仓库原样() {
    let sb = Sandbox::new("pull-conflict");
    let (repo, _, _) = diverged_main(&sb, "r", "m.txt", "theirs\n");
    with_identity(&sb);
    let head = sb.git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(pull(&sb, &repo).unwrap_err(), "git/pull_conflict");
    assert_eq!(sb.git(&repo, &["rev-parse", "HEAD"]), head, "HEAD 没动");
    assert!(sb.git(&repo, &["status", "--porcelain"]).is_empty(), "工作区、暂存区都没动，没有冲突标记");
    assert!(!repo.join(".git/MERGE_HEAD").exists(), "不停在合并中间");
    assert_eq!(std::fs::read_to_string(repo.join("m.txt")).unwrap(), "mine\n");
}

#[test]
fn 拉取_本地落后_快进_不要身份() {
    let sb = Sandbox::new("pull-ff");
    let (repo, _, other) = diverged_main(&sb, "r", "o.txt", "other\n");
    sb.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    let result = pull(&sb, &repo).unwrap();
    assert_eq!((result.pulled, result.mode.as_str()), (1, "ff"));
    assert_eq!(sb.git(&repo, &["rev-parse", "HEAD"]), sb.git(&other, &["rev-parse", "HEAD"]));
    // 再拉一次：已是最新。
    assert_eq!(pull(&sb, &repo).unwrap().mode, "none");
}

#[test]
fn 拉取_没配身份_先说清楚_不动仓库() {
    let sb = Sandbox::new("pull-ident");
    let (repo, _, _) = diverged_main(&sb, "r", "o.txt", "other\n");
    let head = sb.git(&repo, &["rev-parse", "HEAD"]);
    // 没有 user.name / user.email 时 git var 会退回主机名猜的身份；EMAIL 等变量也清掉，逼出「没配身份」。
    let owned: Vec<(&'static str, Option<String>)> = sb
        .env()
        .into_iter()
        .chain([("GIT_AUTHOR_NAME", None), ("GIT_AUTHOR_EMAIL", None), ("GIT_COMMITTER_NAME", None), ("GIT_COMMITTER_EMAIL", None), ("EMAIL", None)])
        .collect();
    let env: Vec<(&str, Option<&str>)> = owned.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    std::fs::write(&sb.config, "[user]\n\tuseConfigOnly = true\n").unwrap();
    assert_eq!(pull_with(&s(&repo), &env, Duration::from_secs(60)).unwrap_err(), "git/identity");
    assert_eq!(sb.git(&repo, &["rev-parse", "HEAD"]), head);
    // 面板里填了身份（写进全局配置）之后再拉就成了。
    set_identity_with(&s(&repo), " Ada ", "ada@example.com", &env).unwrap();
    let config = std::fs::read_to_string(&sb.config).unwrap();
    assert!(config.contains("Ada") && config.contains("ada@example.com"), "{config}");
    assert_eq!(pull_with(&s(&repo), &env, Duration::from_secs(60)).unwrap().mode, "merge");
}

#[test]
fn 拉取_配了变基_按变基接_工作区有改动就不动() {
    let sb = Sandbox::new("pull-rebase");
    let (repo, _, other) = diverged_main(&sb, "r", "o.txt", "other\n");
    with_identity(&sb);
    sb.git(&repo, &["config", "pull.rebase", "true"]);
    std::fs::write(repo.join("README.md"), "dirty\n").unwrap();
    let head = sb.git(&repo, &["rev-parse", "HEAD"]);
    assert_eq!(pull(&sb, &repo).unwrap_err(), "git/pull_dirty");
    assert_eq!(sb.git(&repo, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read_to_string(repo.join("README.md")).unwrap(), "dirty\n", "未提交的改动原样");
    sb.git(&repo, &["checkout", "--", "README.md"]);
    assert_eq!(pull(&sb, &repo).unwrap().mode, "rebase");
    // 变基后本地提交接在远端提交之后，历史是直的（没有合并提交）。
    assert_eq!(sb.git(&repo, &["rev-parse", "HEAD~1"]), sb.git(&other, &["rev-parse", "HEAD"]));
    // 不往 stash 列表里塞东西（不用 autostash）。
    assert!(sb.git(&repo, &["stash", "list"]).is_empty());
}

#[test]
fn 拉取失败归类() {
    let out = |stderr: &str| CmdOut { code: Some(1), stderr: stderr.into(), ..Default::default() };
    // git 2.5x 原文（LC_ALL=C）。
    assert_eq!(classify_pull_failure(&out("error: Your local changes to the following files would be overwritten by merge:\n\tm.txt")), "git/pull_dirty");
    assert_eq!(classify_pull_failure(&out("fatal: couldn't find remote ref refs/heads/feat")), "git/pull_no_branch");
    assert_eq!(classify_pull_failure(&out("fatal: Unable to create '/r/.git/index.lock': File exists.")), "git/locked");
    assert_eq!(classify_pull_failure(&out("fatal: unable to access 'https://x/': Could not resolve host: x")), "git/pull_network");
    assert_eq!(classify_pull_failure(&out("remote: Invalid username or password.\nfatal: Authentication failed for 'https://x/'")), "git/pull_auth");
    assert_eq!(classify_pull_failure(&CmdOut { timed_out: true, ..Default::default() }), "git/pull_timeout");
}

#[test]
fn 设置身份_只收一行名字和像样的邮箱() {
    let sb = Sandbox::new("ident-bad");
    let repo = sb.repo("r");
    let owned = sb.env();
    let env: Vec<(&str, Option<&str>)> = owned.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    for (name, email) in [("", "a@b"), ("Ada", "no-at"), ("Ada\nx", "a@b"), ("Ada", "a b@c")] {
        assert_eq!(set_identity_with(&s(&repo), name, email, &env).unwrap_err(), "git/identity_invalid", "{name:?} {email:?}");
    }
    assert_eq!(std::fs::read_to_string(&sb.config).unwrap(), "", "非法输入一个字都不写");
}

#[test]
fn 后台拉取与推送任务各管各的() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let rx = std::sync::Mutex::new(rx);
    let push_id = start_push_job("C:/forge-job-kinds".into(), move |_| {
        let _ = rx.lock().unwrap().recv_timeout(Duration::from_secs(5));
        Err("git/push_rejected".into())
    });
    let pull_id = start_pull_job("C:/forge-job-kinds".into(), |_| {
        Ok(PullResultDto { remote: "origin".into(), branch: "main".into(), pulled: 1, mode: "merge".into() })
    });
    assert_ne!(push_id, pull_id, "推送在跑时拉取另起任务，不拿推送的任务号顶替");
    tx.send(()).unwrap();
    let start = Instant::now();
    let done = loop {
        let job = job_status(&pull_id).unwrap();
        if job.state != "running" || start.elapsed() > Duration::from_secs(5) {
            break job;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!((done.state.as_str(), done.push.is_none(), done.pull.map(|p| p.pulled)), ("ok", true, Some(1)));
}

// ═══ gh 解析 ═══

#[test]
fn 检查映射_checkrun_与_statuscontext_都认() {
    let merged = parse_pr_full(&fixture("pr_checkrun_merged.json")).unwrap();
    assert_eq!((merged.state.as_str(), merged.checks_summary.as_deref()), ("merged", Some("pass")), "SKIPPED 不算失败");
    assert!(merged.checks.iter().any(|c| c.state == "skipped"));
    assert_eq!(merged.checks[0].workflow.as_deref(), Some("CI"));
    assert!(merged.checks[0].url.as_deref().unwrap().contains("/actions/runs/"));

    let status = parse_pr_full(&fixture("pr_status_context_pending.json")).unwrap();
    assert_eq!(status.checks_summary.as_deref(), Some("pending"));
    let tide = status.checks.iter().find(|c| c.name == "tide").unwrap();
    assert_eq!((tide.state.as_str(), tide.workflow.as_deref()), ("pending", None));
    assert!(tide.url.as_deref().unwrap().starts_with("https://prow.k8s.io/"), "StatusContext 的链接是 targetUrl");

    let conflict = parse_pr_full(&fixture("pr_conflict.json")).unwrap();
    assert_eq!((conflict.mergeable.as_deref(), conflict.merge_state.as_deref(), conflict.checks_summary.as_deref()), (Some("CONFLICTING"), Some("DIRTY"), Some("fail")), "StatusContext ERROR 算失败");

    let fail = parse_pr_full(&fixture("pr_checkrun_fail.json")).unwrap();
    assert_eq!(fail.checks_summary.as_deref(), Some("fail"));
    assert_eq!(fail.checks.iter().filter(|c| c.state == "fail").count(), 1);

    let none = parse_pr_full(&fixture("derived_pr_no_checks.json")).unwrap();
    assert_eq!((none.checks_summary.as_deref(), none.checks.len()), (Some("none"), 0));
    let running = parse_pr_full(&fixture("derived_pr_in_progress.json")).unwrap();
    assert_eq!(running.checks_summary.as_deref(), Some("pending"));
}

#[test]
fn 合并方式_只列仓库允许的() {
    assert_eq!(parse_merge_methods(&fixture("repo_view_drill.json")), vec!["merge", "squash", "rebase"]);
    assert_eq!(parse_merge_methods(r#"{"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"squashMergeAllowed":true}"#), vec!["squash"]);
}

#[test]
fn github_远端地址_三种写法都认_别的主机不认() {
    for url in [
        "https://github.com/larrygogo/meowo-pr-drill.git",
        "https://larry@github.com/larrygogo/meowo-pr-drill",
        "git@github.com:larrygogo/meowo-pr-drill.git",
        "ssh://git@ssh.github.com:443/larrygogo/meowo-pr-drill.git",
    ] {
        assert_eq!(github_slug(url).as_deref(), Some("larrygogo/meowo-pr-drill"), "{url}");
    }
    assert_eq!(github_slug("https://gitlab.com/o/r.git"), None);
    assert_eq!(github_slug("C:/repos/r.git"), None);
}

#[test]
fn gh_合并失败归类() {
    let out = |stderr: &str| CmdOut { code: Some(1), stderr: stderr.into(), ..Default::default() };
    assert_eq!(classify_gh_failure(&out("GraphQL: Head branch was modified. Review and try the merge again. (mergePullRequest)"), None), "gh/head_changed");
    assert_eq!(classify_gh_failure(&out("X Pull request #3 is not mergeable: the merge commit cannot be cleanly created."), Some("https://x/pull/3")), "gh/merge_conflict: https://x/pull/3");
    assert_eq!(classify_gh_failure(&out("GraphQL: At least 1 approving review is required by reviewers with write access. (mergePullRequest)"), None), "gh/review_required");
    assert_eq!(classify_gh_failure(&out("GraphQL: Squash merges are not allowed on this repository. (mergePullRequest)"), None), "gh/method_not_allowed");
    assert_eq!(classify_gh_failure(&out("HTTP 401: Bad credentials (https://api.github.com/graphql)\nTry authenticating with:  gh auth login"), None), "gh/not_authed");
    assert!(classify_gh_failure(&out("X Pull request #3 is not mergeable: the base branch policy prohibits the merge."), None).starts_with("gh/merge_blocked"));
}

// ═══ 状态 / 开 PR / 合并（假 gh）═══

/// 假 gh：按参数前缀回录制好的输出，记下每次调用。
struct FakeGh {
    calls: Mutex<Vec<String>>,
    installed: bool,
    authed: bool,
    pr_view: Mutex<Vec<CmdOut>>,
    repo_view: String,
    merge_help: String,
    merge_out: CmdOut,
    create_out: CmdOut,
    /// 模拟 fork + upstream 两个远端、没设 gh default repo：pr 子命令不带 --repo 就报 set-default（gh 2.x 原话）。
    require_repo: bool,
}

fn ok(stdout: &str) -> CmdOut {
    CmdOut { code: Some(0), stdout: stdout.into(), ..Default::default() }
}

fn fail(stderr: &str) -> CmdOut {
    CmdOut { code: Some(1), stderr: stderr.into(), ..Default::default() }
}

impl FakeGh {
    fn new() -> Self {
        FakeGh {
            calls: Mutex::new(Vec::new()),
            installed: true,
            authed: true,
            pr_view: Mutex::new(vec![fail("no pull requests found for branch \"meowo/drill\"")]),
            repo_view: fixture("repo_view_drill.json"),
            merge_help: "Flags:\n      --match-head-commit SHA   Commit SHA that the pull request head must match to allow merge".into(),
            merge_out: ok(""),
            create_out: ok("https://github.com/larrygogo/meowo-pr-drill/pull/12\n"),
            require_repo: false,
        }
    }
    /// pr view 依次回这些（最后一个之后一直回最后一个）。
    fn views(self, outs: Vec<CmdOut>) -> Self {
        *self.pr_view.lock().unwrap() = outs;
        self
    }
    fn run(&self, _dir: &str, args: &[&str]) -> Option<CmdOut> {
        if !self.installed {
            return None;
        }
        let line = args.join(" ");
        self.calls.lock().unwrap().push(line.clone());
        if self.require_repo && line.starts_with("pr ") && line != "pr merge --help" && !line.contains("--repo ") {
            return Some(fail("X No default remote repository has been set. To learn more about the default repository, run: gh repo set-default --help"));
        }
        Some(if line.starts_with("auth status") {
            if self.authed { ok("") } else { fail("You are not logged into any GitHub hosts. To log in, run: gh auth login") }
        } else if line.starts_with("pr view") {
            let mut views = self.pr_view.lock().unwrap();
            if views.len() > 1 { views.remove(0) } else { views[0].clone() }
        } else if line.starts_with("repo view") {
            ok(&self.repo_view)
        } else if line == "pr merge --help" {
            ok(&self.merge_help)
        } else if line.starts_with("pr merge") {
            self.merge_out.clone()
        } else if line.starts_with("pr create") {
            self.create_out.clone()
        } else {
            fail("unexpected")
        })
    }
    fn count(&self, prefix: &str) -> usize {
        self.calls.lock().unwrap().iter().filter(|c| c.starts_with(prefix)).count()
    }
}

/// 连着 GitHub 地址的仓库（不联网：只有 git 本地操作，gh 是假的）。分支 meowo/drill 上一个提交，
/// origin/main 指向初始提交。
fn github_repo(sb: &Sandbox) -> PathBuf {
    let repo = sb.repo("gh");
    sb.git(&repo, &["remote", "add", "origin", "https://github.com/larrygogo/meowo-pr-drill.git"]);
    let init = sb.git(&repo, &["rev-parse", "HEAD"]);
    sb.git(&repo, &["update-ref", "refs/remotes/origin/main", &init]);
    sb.git(&repo, &["switch", "-q", "-c", "meowo/drill"]);
    sb.commit(&repo, "d.txt", "feat: drill one");
    repo
}

fn status(fake: &FakeGh, caches: &Caches, prefs: &Path, repo: &Path, force: bool) -> ForgeStatusDto {
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches, prefs: prefs.to_path_buf(), env: &[] };
    forge_status_with(&ctx, &s(repo), force).unwrap()
}

#[test]
fn 状态_没装_gh_只给_compare_网页与默认标题() {
    let sb = Sandbox::new("st-missing");
    let repo = github_repo(&sb);
    let fake = FakeGh { installed: false, ..FakeGh::new() };
    let dto = status(&fake, &Caches::default(), &sb.root.join("forge.json"), &repo, false);
    assert_eq!(dto.gh, "missing");
    assert_eq!(dto.compare_url.as_deref(), Some("https://github.com/larrygogo/meowo-pr-drill/compare/main...meowo/drill?expand=1"));
    assert_eq!(dto.repo_url.as_deref(), Some("https://github.com/larrygogo/meowo-pr-drill"));
    assert!(dto.pr.is_none() && dto.merge_methods.is_empty());
    let defaults = dto.pr_defaults.unwrap();
    assert_eq!((defaults.title.as_str(), defaults.body.as_str(), defaults.base.as_str()), ("feat: drill one", "- feat: drill one", "main"));
}

#[test]
fn 状态_没登录与不是_github() {
    let sb = Sandbox::new("st-auth");
    let repo = github_repo(&sb);
    let fake = FakeGh { authed: false, ..FakeGh::new() };
    let dto = status(&fake, &Caches::default(), &sb.root.join("forge.json"), &repo, false);
    assert_eq!(dto.gh, "unauthenticated");
    assert_eq!(fake.count("pr view"), 0, "没登录不去查 PR");
    sb.git(&repo, &["remote", "set-url", "origin", "https://gitlab.com/o/r.git"]);
    let dto = status(&FakeGh::new(), &Caches::default(), &sb.root.join("forge.json"), &repo, false);
    assert_eq!((dto.gh.as_str(), dto.compare_url), ("not_github", None));
}

#[test]
fn 状态_有_gh_每仓库_60_秒最多查一次_手动刷新最短_10_秒() {
    let sb = Sandbox::new("st-cache");
    let repo = github_repo(&sb);
    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_in_progress.json"))]);
    let caches = Caches::default();
    let prefs = sb.root.join("forge.json");
    let first = status(&fake, &caches, &prefs, &repo, false);
    let pr = first.pr.unwrap();
    assert_eq!((pr.number, pr.checks_summary.as_deref(), pr.checks.len()), (12, Some("pending"), 2));
    assert_eq!(first.merge_methods, vec!["merge", "squash", "rebase"]);
    assert!(first.pr_defaults.is_none(), "已有 PR 不给开 PR 的默认值");
    status(&fake, &caches, &prefs, &repo, false);
    status(&fake, &caches, &prefs, &repo, true);
    assert_eq!(fake.count("pr view"), 1, "60 秒内（含 10 秒内的手动刷新）只查一次 GitHub");
    assert_eq!(fake.count("repo view"), 1, "合并方式缓存 1 小时");
    // 缓存过了 10 秒：手动刷新会重查。
    {
        let mut prs = caches.prs.lock().unwrap();
        for (at, _) in prs.values_mut() {
            *at -= Duration::from_secs(11);
        }
    }
    status(&fake, &caches, &prefs, &repo, false);
    assert_eq!(fake.count("pr view"), 1, "没到 60 秒，轮询不重查");
    status(&fake, &caches, &prefs, &repo, true);
    assert_eq!(fake.count("pr view"), 2);
}

fn merge(fake: &FakeGh, caches: &Caches, prefs: &Path, repo: &Path, method: &str, head: &str) -> Result<MergeResultDto, String> {
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches, prefs: prefs.to_path_buf(), env: &[] };
    pr_merge_with(&ctx, &s(repo), 12, method, head)
}

const HEAD: &str = "1111111111111111111111111111111111111111";

#[test]
fn 合并_检查没过或在跑_有冲突_头提交变了_都不调_gh_pr_merge() {
    let sb = Sandbox::new("merge-guard");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let cases = [
        ("derived_pr_in_progress.json", "gh/checks_pending".to_string()),
        ("derived_pr_unknown_mergeable.json", "gh/checks_pending".to_string()),
    ];
    for (file, want) in cases {
        let fake = FakeGh::new().views(vec![ok(&fixture(file))]);
        assert_eq!(merge(&fake, &Caches::default(), &prefs, &repo, "squash", HEAD).unwrap_err(), want, "{file}");
        assert_eq!(fake.count("pr merge 12"), 0, "{file}");
    }
    // 检查失败：拿录制的 react 样例改成 12 号、头提交对上。
    let mut failing: serde_json::Value = serde_json::from_str(&fixture("pr_checkrun_fail.json")).unwrap();
    failing["number"] = 12.into();
    failing["headRefOid"] = HEAD.into();
    let fake = FakeGh::new().views(vec![ok(&failing.to_string())]);
    assert_eq!(merge(&fake, &Caches::default(), &prefs, &repo, "squash", HEAD).unwrap_err(), "gh/checks_failed");
    // 冲突：检查全绿也不合，带上网页链接。
    let mut conflict: serde_json::Value = serde_json::from_str(&fixture("pr_conflict.json")).unwrap();
    conflict["number"] = 12.into();
    conflict["headRefOid"] = HEAD.into();
    conflict["statusCheckRollup"] = serde_json::json!([]);
    conflict["isDraft"] = false.into();
    let fake = FakeGh::new().views(vec![ok(&conflict.to_string())]);
    let err = merge(&fake, &Caches::default(), &prefs, &repo, "squash", HEAD).unwrap_err();
    assert!(err.starts_with("gh/merge_conflict: https://github.com/kubernetes/kubernetes/pull/"), "{err}");
    // 确认框打开后有人推了新提交。
    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json"))]);
    assert_eq!(merge(&fake, &Caches::default(), &prefs, &repo, "squash", "2222").unwrap_err(), "gh/head_changed");
    // 仓库不允许的方式。
    let fake = FakeGh { repo_view: r#"{"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"squashMergeAllowed":true}"#.into(), ..FakeGh::new() };
    assert_eq!(merge(&fake, &Caches::default(), &prefs, &repo, "rebase", HEAD).unwrap_err(), "gh/method_not_allowed");
    // gh 太旧：没有 --match-head-commit 就不合。
    let fake = FakeGh { merge_help: "Flags:\n  --squash".into(), ..FakeGh::new() };
    assert_eq!(merge(&fake, &Caches::default(), &prefs, &repo, "squash", HEAD).unwrap_err(), "gh/too_old");
    assert_eq!(fake.count("pr merge 12"), 0);
}

#[test]
fn 合并_无检查允许_带头提交_不删分支_记住方式() {
    let sb = Sandbox::new("merge-ok");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let caches = Caches::default();
    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json")), ok(&fixture("derived_pr_no_checks_merged.json"))]);
    let result = merge(&fake, &caches, &prefs, &repo, "squash", HEAD).unwrap();
    assert_eq!(result, MergeResultDto { merged: true, queued: false });
    let calls = fake.calls.lock().unwrap().clone();
    let merge_call = calls.iter().find(|c| c.starts_with("pr merge 12")).unwrap();
    assert_eq!(merge_call, &format!("pr merge 12 --squash --match-head-commit {HEAD} --repo larrygogo/meowo-pr-drill"));
    assert!(!merge_call.contains("--delete-branch"));
    assert_eq!(last_merge_method(&prefs, "larrygogo/meowo-pr-drill").as_deref(), Some("squash"));
    let fake = FakeGh::new();
    assert_eq!(status(&fake, &caches, &prefs, &repo, false).last_merge_method.as_deref(), Some("squash"));
}

/// 做空审查：合并成功后 GitHub 有延迟，立刻 view 还是 OPEN，以前就被当成「已加入合并队列」，
/// 还记进 queued 缓存，之后刷新一直挂着「在队列里」。只有 gh 明说进了队列才算排队。
#[test]
fn 合并_成功后立刻查还是_open_不当成进了队列_按已合并_下次刷新以服务器为准() {
    let sb = Sandbox::new("merge-lag");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let caches = Caches::default();
    let open = ok(&fixture("derived_pr_no_checks.json"));
    let fake = FakeGh {
        merge_out: CmdOut { code: Some(0), stderr: "✓ Squashed and merged pull request larrygogo/meowo-pr-drill#12".into(), ..Default::default() },
        ..FakeGh::new().views(vec![open.clone(), open, ok(&fixture("derived_pr_no_checks_merged.json"))])
    };
    let result = merge(&fake, &caches, &prefs, &repo, "squash", HEAD).unwrap();
    assert_eq!(result, MergeResultDto { merged: true, queued: false });
    assert!(caches.queued.lock().unwrap().is_empty(), "没进队列就不记队列标记");
    let pr = status(&fake, &caches, &prefs, &repo, false).pr.unwrap();
    assert_eq!((pr.state.as_str(), pr.queued), ("merged", false), "下一次刷新以服务器为准");
}

#[test]
fn 合并_进了合并队列不说已合并_刷新后仍显示在队列里() {
    let sb = Sandbox::new("merge-queue");
    let repo = github_repo(&sb);
    let prefs = sb.root.join("forge.json");
    let caches = Caches::default();
    let open = ok(&fixture("derived_pr_no_checks.json"));
    let fake = FakeGh {
        merge_out: CmdOut { code: Some(0), stderr: "✓ Pull request larrygogo/meowo-pr-drill#12 will be added to the merge queue for main when ready".into(), ..Default::default() },
        ..FakeGh::new().views(vec![open.clone(), open])
    };
    let result = merge(&fake, &caches, &prefs, &repo, "merge", HEAD).unwrap();
    assert_eq!(result, MergeResultDto { merged: false, queued: true });
    let dto = status(&fake, &caches, &prefs, &repo, false);
    let pr = dto.pr.unwrap();
    assert_eq!((pr.state.as_str(), pr.queued), ("open", true));
}

#[test]
fn 开_pr_没装_gh_报错_前端改开网页() {
    let sb = Sandbox::new("create-missing");
    let repo = github_repo(&sb);
    let fake = FakeGh { installed: false, ..FakeGh::new() };
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs: sb.root.join("forge.json"), env: &[] };
    let err = pr_create_with(&ctx, &s(&repo), None, None, None, false, &|_| panic!("没 gh 不该推")).unwrap_err();
    assert_eq!(err, "gh/not_installed");
}

#[test]
fn 开_pr_先推再建_默认标题正文_已有_pr_带回链接() {
    let sb = Sandbox::new("create-ok");
    let repo = github_repo(&sb);
    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json"))]);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let caches = Caches::default();
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: sb.root.join("forge.json"), env: &[] };
    let pushed = std::sync::atomic::AtomicUsize::new(0);
    let push = |_: &str| {
        pushed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(PushResultDto { remote: "origin".into(), branch: "meowo/drill".into(), set_upstream: true, pushed: 1 })
    };
    let pr = pr_create_with(&ctx, &s(&repo), None, None, None, false, &push).unwrap();
    assert_eq!(pr.number, 12);
    assert_eq!(pushed.load(std::sync::atomic::Ordering::SeqCst), 1, "没上游：先推");
    let create = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert_eq!(create, "pr create --title feat: drill one --body - feat: drill one --base main --head meowo/drill --repo larrygogo/meowo-pr-drill");

    let exists = FakeGh {
        create_out: fail("a pull request for branch \"meowo/drill\" into branch \"main\" already exists:\nhttps://github.com/larrygogo/meowo-pr-drill/pull/7"),
        ..FakeGh::new()
    };
    let gh = |dir: &str, args: &[&str]| exists.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs: sb.root.join("forge.json"), env: &[] };
    let err = pr_create_with(&ctx, &s(&repo), Some("自定义标题"), Some("正文"), Some("main"), true, &push).unwrap_err();
    assert_eq!(err, "gh/pr_exists: https://github.com/larrygogo/meowo-pr-drill/pull/7");
    let create = exists.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert!(create.starts_with("pr create --title 自定义标题 --body 正文 --base main") && create.contains(" --draft"), "{create}");
}

#[test]
fn 开_pr_上游是_origin_main_先推同名分支_head_是功能分支_不碰_main() {
    let sb = Sandbox::new("create-feat-main");
    let (repo, remote) = feat_tracking_main(&sb, "r");
    let main_before = sb.git(&remote, &["rev-parse", "main"]);
    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json"))]);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let owned = sb.env();
    let env: Vec<(&str, Option<&str>)> = owned.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    let caches = Caches::default();
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: sb.root.join("forge.json"), env: &env };
    let real_push = |dir: &str| push_with(dir, &env, Duration::from_secs(60));
    pr_create_with(&ctx, &s(&repo), None, None, None, false, &real_push).unwrap();
    assert_eq!(sb.git(&remote, &["rev-parse", "main"]), main_before, "开 PR 前的推送不能进 main");
    assert_eq!(sb.git(&remote, &["rev-parse", "feat"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    let create = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert!(create.contains("--base main --head feat"), "{create}");
}

/// 做空第一轮：fork + upstream 两个远端、没设 gh default repo 时，gh pr view/create/merge 不带 --repo
/// 会报 set-default，所以一律显式带 --repo；pr view 带了 --repo 之后 gh 不再认当前分支，必须把分支名作参数。
/// 做空第二轮：两个都是 GitHub 远端又没定时，静默挑 origin 会把 PR 开错仓库。查状态暂按 origin，
/// 开 PR / 合并返回 gh/ambiguous_repo，界面给出远端列表；选了之后按选的仓库、head 带 fork owner。
#[test]
fn 多个_github_远端没定_查状态按_origin_开_pr_合并要先选_选了按选的仓库() {
    let sb = Sandbox::new("gh-repo");
    let repo = github_repo(&sb);
    sb.git(&repo, &["remote", "add", "upstream", "https://github.com/someone-else/meowo-pr-drill.git"]);
    let prefs = sb.root.join("forge.json");
    let caches = Caches::default();
    let open = ok(&fixture("derived_pr_no_checks.json"));
    let fake = FakeGh { require_repo: true, ..FakeGh::new().views(vec![open.clone()]) };
    let dto = status(&fake, &caches, &prefs, &repo, false);
    assert_eq!(dto.pr.map(|p| p.number), Some(12), "带 --repo 才查得到 PR");
    assert!(dto.repo_needs_choice);
    assert_eq!(dto.repo_remote.as_deref(), Some("origin"));
    let choices: Vec<(String, String)> = dto.repo_choices.iter().map(|c| (c.remote.clone(), c.slug.clone())).collect();
    assert_eq!(
        choices,
        vec![
            ("origin".to_string(), "larrygogo/meowo-pr-drill".to_string()),
            ("upstream".to_string(), "someone-else/meowo-pr-drill".to_string())
        ]
    );
    let view = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr view")).cloned().unwrap();
    assert!(view.starts_with("pr view meowo/drill --repo larrygogo/meowo-pr-drill"), "{view}");
    assert_eq!(merge(&fake, &caches, &prefs, &repo, "squash", HEAD), Err("gh/ambiguous_repo".into()));
    assert_eq!(fake.count("pr merge 12"), 0);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
    assert_eq!(pr_create_with(&ctx, &s(&repo), None, None, None, false, &|_| panic!("没定仓库不该推")), Err("gh/ambiguous_repo".into()));
    assert_eq!(fake.count("pr create"), 0);

    // 界面上选了 upstream：记在测试的 forge.json 里，按公共 .git 目录。
    assert_eq!(choose_repo_with(&prefs, &s(&repo), "nope", &[]), Err("git/no_remote".into()));
    choose_repo_with(&prefs, &s(&repo), "upstream", &[]).unwrap();
    assert!(std::fs::read_to_string(&prefs).unwrap().contains("\"repoRemote\""));
    let fake = FakeGh { require_repo: true, ..FakeGh::new().views(vec![open.clone(), open.clone(), ok(&fixture("derived_pr_no_checks_merged.json"))]) };
    let dto = status(&fake, &caches, &prefs, &repo, false);
    assert!(!dto.repo_needs_choice);
    assert_eq!((dto.repo_remote.as_deref(), dto.repo_choices.len()), (Some("upstream"), 2), "选过之后还能换");
    assert_eq!(dto.repo_url.as_deref(), Some("https://github.com/someone-else/meowo-pr-drill"));
    assert!(merge(&fake, &caches, &prefs, &repo, "squash", HEAD).unwrap().merged);
    let calls = fake.calls.lock().unwrap().clone();
    let view = calls.iter().find(|c| c.starts_with("pr view")).unwrap();
    assert!(view.starts_with("pr view larrygogo:meowo/drill --repo someone-else/meowo-pr-drill"), "{view}");
    let merge_call = calls.iter().find(|c| c.starts_with("pr merge 12")).unwrap();
    assert!(merge_call.contains("--repo someone-else/meowo-pr-drill"), "{merge_call}");
    assert_eq!(last_merge_method(&prefs, "someone-else/meowo-pr-drill").as_deref(), Some("squash"), "合并方式按 PR 所在仓库记");

    let fake = FakeGh { require_repo: true, ..FakeGh::new().views(vec![open]) };
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs: prefs.clone(), env: &[] };
    let push = |_: &str| Ok(PushResultDto { remote: "origin".into(), branch: "meowo/drill".into(), set_upstream: true, pushed: 1 });
    pr_create_with(&ctx, &s(&repo), None, None, None, false, &push).unwrap();
    let create = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert!(create.ends_with("--head larrygogo:meowo/drill --repo someone-else/meowo-pr-drill"), "{create}");
}

/// 只有一个 GitHub 远端（另一个是公司 GitLab 镜像）：直接用它，不要求选。
#[test]
fn 只有一个_github_远端_直接用_不要求选() {
    let sb = Sandbox::new("gh-one");
    let repo = github_repo(&sb);
    sb.git(&repo, &["remote", "add", "mirror", "https://gitlab.com/o/meowo-pr-drill.git"]);
    let prefs = sb.root.join("forge.json");
    let dto = status(&FakeGh::new(), &Caches::default(), &prefs, &repo, false);
    assert!(!dto.repo_needs_choice && dto.repo_choices.is_empty());
    assert_eq!(dto.repo_url.as_deref(), Some("https://github.com/larrygogo/meowo-pr-drill"));
}

// ═══ 推送 / PR 目标判定（做空第二轮，scratchpad/redteam2r 的 rt2_ 实验转正）═══

fn has_branch(bare: &Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(bare)
        .args(["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")])
        .output()
        .unwrap()
        .status
        .success()
}

/// 做空第二轮 rt2_远端名带斜杠：远端叫 team/fork，feat 跟踪同名的 team/fork/feat。以前摘要用
/// split_once('/') 把上游拆成「team」+「fork/feat」，认不出同名上游：upstream 丢了、pushTarget 写成
/// origin/feat，推送却去了 team/fork（摘要与真实去向不一致）。
#[test]
fn 目标判定_远端名带斜杠_摘要认出上游且与推送去向一致() {
    let sb = Sandbox::new("tgt-slash");
    let (repo, origin) = sb.repo_with_remote("r");
    let fork = sb.bare("fork.git");
    sb.git(&repo, &["remote", "add", "team/fork", &fork.to_string_lossy()]);
    sb.git(&repo, &["push", "-q", "-u", "team/fork", "feat"]);
    sb.commit(&repo, "c.txt", "feat: c");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!(
        (summary.upstream.as_deref(), summary.ahead, summary.unpushed, summary.push_target.as_deref()),
        (Some("team/fork/feat"), Some(1), Some(1), Some("team/fork/feat")),
        "带斜杠的远端名：上游、领先数、未推数、推送目标"
    );
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "team/fork".into(), branch: "feat".into(), set_upstream: false, pushed: 1 });
    assert_eq!(sb.git(&fork, &["rev-parse", "feat"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    assert!(!has_branch(&origin, "feat"), "不该推到 origin");
}

/// 做空第二轮 rt2_本地名与上游功能分支不同名：`git switch -c fix --track origin/bugfix-123`。上游是别人的
/// 功能分支（不是默认分支），用户的跟踪关系要尊重：推到 bugfix-123、不新建远端 fix、上游不改。
/// 以前「上游不同名一律推同名分支」：推出一个远端 fix 并把上游改成 origin/fix，bugfix-123 没动。
#[test]
fn 目标判定_跟踪不同名的功能分支_推到这个上游_不另建同名分支() {
    let sb = Sandbox::new("tgt-rename");
    let (repo, remote) = sb.repo_with_remote("r");
    sb.git(&repo, &["push", "-q", "origin", "feat:bugfix-123"]);
    sb.git(&repo, &["fetch", "-q"]);
    sb.git(&repo, &["switch", "-q", "-c", "fix", "--track", "origin/bugfix-123"]);
    sb.commit(&repo, "f.txt", "fix: f");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!(
        (summary.upstream.as_deref(), summary.ahead, summary.unpushed, summary.push_target.as_deref()),
        (Some("origin/bugfix-123"), Some(1), Some(1), Some("origin/bugfix-123"))
    );
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "origin".into(), branch: "bugfix-123".into(), set_upstream: false, pushed: 1 });
    assert_eq!(sb.git(&remote, &["rev-parse", "bugfix-123"]), sb.git(&repo, &["rev-parse", "HEAD"]), "推进了跟踪的 bugfix-123");
    assert!(!has_branch(&remote, "fix"), "不该另建远端 fix");
    assert_eq!(sb.git(&repo, &["rev-parse", "--abbrev-ref", "fix@{upstream}"]), "origin/bugfix-123", "上游不改");
}

/// 同上，PR 这一侧：PR 的 head 是推去的远端分支 bugfix-123，查 PR、开 PR、compare 链接都按它。
#[test]
fn 目标判定_跟踪不同名的功能分支_pr_按上游分支查和开() {
    let sb = Sandbox::new("tgt-rename-pr");
    let repo = github_repo(&sb);
    let head = sb.git(&repo, &["rev-parse", "HEAD"]);
    sb.git(&repo, &["update-ref", "refs/remotes/origin/bugfix-123", &head]);
    sb.git(&repo, &["config", "branch.meowo/drill.remote", "origin"]);
    sb.git(&repo, &["config", "branch.meowo/drill.merge", "refs/heads/bugfix-123"]);
    let prefs = sb.root.join("forge.json");
    let fake = FakeGh::new();
    let dto = status(&fake, &Caches::default(), &prefs, &repo, false);
    assert_eq!(dto.compare_url.as_deref(), Some("https://github.com/larrygogo/meowo-pr-drill/compare/main...bugfix-123?expand=1"));
    let view = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr view")).cloned().unwrap();
    assert!(view.starts_with("pr view bugfix-123 --repo larrygogo/meowo-pr-drill"), "{view}");

    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json"))]);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs, env: &[] };
    pr_create_with(&ctx, &s(&repo), None, None, None, false, &|_| panic!("已推到上游，不该再推")).unwrap();
    let create = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert!(create.contains("--base main --head bugfix-123 --repo larrygogo/meowo-pr-drill"), "{create}");
}

/// 做空第二轮 rt2_push_default_与同名上游：feat 跟踪 origin/feat，remote.pushDefault=fork。git 原生 push
/// 去 fork（pushDefault 优先于 branch.<b>.remote）；以前 Meowo 沿用同名上游推回 origin，摘要也写 origin/feat。
/// 再看 branch.<b>.pushRemote 压过 pushDefault。
#[test]
fn 目标判定_push_default_与_push_remote_优先于上游远端_与_git_一致() {
    let sb = Sandbox::new("tgt-pushdefault");
    let (repo, origin) = sb.repo_with_remote("r");
    let fork = sb.bare("fork.git");
    sb.git(&repo, &["remote", "add", "fork", &fork.to_string_lossy()]);
    sb.git(&repo, &["push", "-q", "-u", "origin", "feat"]);
    let origin_feat = sb.git(&origin, &["rev-parse", "feat"]);
    sb.git(&repo, &["config", "remote.pushDefault", "fork"]);
    sb.commit(&repo, "c.txt", "feat: c");
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    // fork 上什么都没有：四个提交（init、a、b、c）都要推过去。上游照旧是拉取来源 origin/feat。
    assert_eq!(
        (summary.upstream.as_deref(), summary.unpushed, summary.push_target.as_deref()),
        (Some("origin/feat"), Some(4), Some("fork/feat"))
    );
    let result = push(&sb, &repo).unwrap();
    assert_eq!(result, PushResultDto { remote: "fork".into(), branch: "feat".into(), set_upstream: false, pushed: 4 });
    assert_eq!(sb.git(&fork, &["rev-parse", "feat"]), sb.git(&repo, &["rev-parse", "HEAD"]));
    assert_eq!(sb.git(&origin, &["rev-parse", "feat"]), origin_feat, "origin/feat 不动");
    assert_eq!(sb.git(&repo, &["config", "--get", "branch.feat.remote"]), "origin", "跟踪关系不改");

    let other = sb.bare("other.git");
    sb.git(&repo, &["remote", "add", "other", &other.to_string_lossy()]);
    sb.git(&repo, &["config", "branch.feat.pushRemote", "other"]);
    let summary = git_diff_summary_blocking(&s(&repo)).unwrap();
    assert_eq!(summary.push_target.as_deref(), Some("other/feat"), "branch.<b>.pushRemote 压过 remote.pushDefault");
}

/// 做空第二轮 rt2_fork_工作流_set_default_被无视：origin 是自己的 fork，upstream 是上游仓库，已
/// `gh repo set-default upstream`（gh 写 remote.upstream.gh-resolved=base）。PR 应开在上游仓库、head 写
/// `<fork owner>:<分支>`；以前一律 `--repo` origin，PR 开到了自己的 fork 里。
#[test]
fn 目标判定_fork_工作流_认_gh_set_default_head_带_fork_owner() {
    let sb = Sandbox::new("tgt-fork");
    let repo = github_repo(&sb);
    sb.git(&repo, &["remote", "add", "upstream", "https://github.com/someone-else/meowo-pr-drill.git"]);
    sb.git(&repo, &["config", "remote.upstream.gh-resolved", "base"]);
    let prefs = sb.root.join("forge.json");
    let fake = FakeGh::new();
    let dto = status(&fake, &Caches::default(), &prefs, &repo, false);
    assert_eq!(dto.repo_url.as_deref(), Some("https://github.com/someone-else/meowo-pr-drill"));
    assert_eq!(
        dto.compare_url.as_deref(),
        Some("https://github.com/someone-else/meowo-pr-drill/compare/main...larrygogo:meowo/drill?expand=1")
    );
    let calls = fake.calls.lock().unwrap().clone();
    assert!(calls.iter().any(|c| c.starts_with("repo view someone-else/meowo-pr-drill ")), "合并方式按上游仓库查：{calls:?}");
    let view = calls.iter().find(|c| c.starts_with("pr view")).unwrap();
    assert!(view.starts_with("pr view larrygogo:meowo/drill --repo someone-else/meowo-pr-drill"), "{view}");

    let fake = FakeGh::new().views(vec![ok(&fixture("derived_pr_no_checks.json"))]);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &Caches::default(), prefs, env: &[] };
    let push = |_: &str| Ok(PushResultDto { remote: "origin".into(), branch: "meowo/drill".into(), set_upstream: true, pushed: 1 });
    pr_create_with(&ctx, &s(&repo), None, None, None, false, &push).unwrap();
    let create = fake.calls.lock().unwrap().iter().find(|c| c.starts_with("pr create")).cloned().unwrap();
    assert!(create.ends_with("--base main --head larrygogo:meowo/drill --repo someone-else/meowo-pr-drill"), "{create}");
}

// 侧栏 PR / CI 与改动数的节流（UI 优化 #54），单独一个文件。
mod badge;

mod title_pr;
