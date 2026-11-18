//! 标题栏的 PR（git_pull_request）与侧栏徽标、「更改」面板共用 forge 的 PR 缓存（UI 优化收尾 #3）。
//! 以前标题栏按目录另存一份 60 秒缓存、自己查，同一仓库前台时每分钟可能多查一次。GitHub 是假的 HTTP（FakeGithub）。

use super::super::badge::{badges_with, is_pr_url, title_pr_with, url_pr_with, DiffCache};
use super::*;

fn title(fake: &FakeGithub, caches: &Caches, diffs: &DiffCache, prefs: &Path, cwd: &Path) -> Option<PullRequestDto> {
    title_pr_with(&ctx_of(fake, caches, prefs), diffs, &s(cwd))
}

#[test]
fn 标题栏_侧栏_面板_同仓库同分支六十秒内合计只查一次_pr() {
    let sb = Sandbox::new("title-pr-shared");
    let repo = github_repo(&sb);
    // 会话开在子目录：标题栏和侧栏按会话 cwd 问，面板按仓库根问，都是同一个仓库同一个分支。
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    let sub = repo.join("sub");
    let fake = FakeGithub::new().views(&["pr_checkrun_fail.json"]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");

    let pr = title(&fake, &caches, &diffs, &prefs, &sub).expect("标题栏有 PR");
    badges_with(&ctx_of(&fake, &caches, &prefs), &diffs, &[s(&sub)], true, &|_| Some((1, 0, 1)));
    status(&fake, &caches, &prefs, &repo, false);
    title(&fake, &caches, &diffs, &prefs, &sub);
    assert_eq!(fake.count("view"), 1, "三处 60 秒内合计只查一次 PR：{:?}", fake.calls());

    // 标题栏回的形状不变：只有号、状态、链接、标题，检查与分支等面板字段不带。
    let panel = status(&fake, &caches, &prefs, &repo, false).pr.expect("面板有 PR");
    assert_eq!((pr.number, pr.state.as_str(), pr.url.as_str(), pr.title.as_str()), (panel.number, panel.state.as_str(), panel.url.as_str(), panel.title.as_str()));
    assert_eq!(pr, PullRequestDto { number: pr.number, state: pr.state.clone(), url: pr.url.clone(), title: pr.title.clone(), ..Default::default() });
}

/// 会话里 agent 建的 PR 按链接查（2026-11-16 实拍：临时 worktree 里建了 #536，会话目录还在旧分支上，
/// 按分支只能查到已合并的 #530）。链接里有仓库和编号，不看目录在哪个分支；60 秒内同一链接只问一次。
#[test]
fn 按链接查会话里建的_pr_不看目录分支_六十秒只调一次() {
    let sb = Sandbox::new("url-pr");
    let repo = github_repo(&sb);
    let fake = FakeGithub::new().views(&["pr_checkrun_fail.json"]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let ctx = ctx_of(&fake, &caches, &prefs);
    let url = "https://github.com/larrygogo/meowo/pull/161";
    let pr = url_pr_with(&ctx, &diffs, &s(&repo), url).expect("按链接查到 PR");
    url_pr_with(&ctx, &diffs, &s(&repo), url);
    assert_eq!(fake.count("view"), 1, "{:?}", fake.calls());
    // 按链接里的仓库与编号查，不是分支名。
    let (_, _, body) = fake.first("view");
    assert_eq!(body["variables"], serde_json::json!({ "owner": "larrygogo", "name": "meowo", "number": 161 }));
    assert_eq!(pr, PullRequestDto { number: pr.number, state: pr.state.clone(), url: pr.url.clone(), title: pr.title.clone(), ..Default::default() });
    // 不是 PR 链接的字符串不查。
    assert_eq!(url_pr_with(&ctx, &diffs, &s(&repo), "https://github.com/o/r/issues/1"), None);
    assert_eq!(fake.count("view"), 1);
    // github.com 没添加账号：链接也不查，一个请求都不发（改版前问本机 gh）。
    let none = FakeGithub { account: None, ..FakeGithub::new() };
    let caches = Caches::default();
    assert_eq!(url_pr_with(&ctx_of(&none, &caches, &prefs), &DiffCache::default(), &s(&repo), url), None);
    assert!(none.calls().is_empty());
}

#[test]
fn 只认_pr_链接的形状() {
    assert!(is_pr_url("https://github.com/o/r/pull/536"));
    assert!(is_pr_url("https://github.example.com/o/r/pull/7"));
    for bad in ["http://github.com/o/r/pull/1", "https://github.com/o/r/issues/1", "https://github.com/o/r/pull/1/files", "https://github.com/o/r/pull/abc", "https://github.com/o/r/pull/1 --web", "pull/1"] {
        assert!(!is_pr_url(bad), "{bad}");
    }
}

#[test]
fn 标题栏_没有_pr_或不是仓库_返回空() {
    let sb = Sandbox::new("title-pr-none");
    let repo = github_repo(&sb);
    let fake = FakeGithub::new();
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &repo), None);
    let plain = sb.root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &plain), None);
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &sb.root.join("missing")), None);
}

/// 改版前（coordinator 11-11）：企业版这类认不出平台的远端，标题栏直接问本机 gh 拿 PR。现在 Meowo 不调用 gh：
/// 没加账号的企业版主机认不出平台，标题栏不显示 PR、一个请求都不发；加了企业版账号之后走统一接口（platforms.rs）。
#[test]
fn 标题栏_没加账号的企业版远端_不查() {
    let sb = Sandbox::new("title-pr-ghe");
    let repo = sb.repo("ghe");
    sb.git(&repo, &["remote", "add", "origin", "git@github.example.com:o/r.git"]);
    sb.git(&repo, &["switch", "-q", "-c", "feat"]);
    let fake = FakeGithub::new().views(&["pr_checkrun_fail.json"]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    for _ in 0..3 {
        assert_eq!(title(&fake, &caches, &diffs, &prefs, &repo), None);
    }
    assert!(fake.calls().is_empty(), "{:?}", fake.calls());
}
