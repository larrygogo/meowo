//! 标题栏的 PR（git_pull_request）与侧栏徽标、「更改」面板共用 forge 的 PR 缓存（UI 优化收尾 #3）。
//! 以前标题栏按目录另存一份 60 秒缓存、自己调 `gh pr view`，同一仓库前台时每分钟可能多打一次 gh。

use super::super::badge::{badges_with, is_pr_url, title_pr_with, url_pr_with, DiffCache};
use super::*;

fn title(fake: &FakeGh, caches: &Caches, diffs: &DiffCache, prefs: &Path, cwd: &Path) -> Option<PullRequestDto> {
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches, prefs: prefs.to_path_buf(), env: &[] };
    title_pr_with(&ctx, diffs, &s(cwd))
}

#[test]
fn 标题栏_侧栏_面板_同仓库同分支六十秒内合计只查一次_pr() {
    let sb = Sandbox::new("title-pr-shared");
    let repo = github_repo(&sb);
    // 会话开在子目录：标题栏和侧栏按会话 cwd 问，面板按仓库根问，都是同一个仓库同一个分支。
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    let sub = repo.join("sub");
    let fake = FakeGh::new().views(vec![ok(&fixture("pr_checkrun_fail.json"))]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");

    let pr = title(&fake, &caches, &diffs, &prefs, &sub).expect("标题栏有 PR");
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
    badges_with(&ctx, &diffs, &[s(&sub)], true, &|_| Some((1, 0, 1)));
    status(&fake, &caches, &prefs, &repo, false);
    title(&fake, &caches, &diffs, &prefs, &sub);
    assert_eq!(fake.count("pr view"), 1, "三处 60 秒内合计只打一次 gh pr view：{:?}", fake.calls.lock().unwrap());

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
    let fake = FakeGh::new().views(vec![ok(&fixture("pr_checkrun_fail.json"))]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: sb.root.join("forge.json"), env: &[] };
    let url = "https://github.com/larrygogo/meowo/pull/161";
    let pr = url_pr_with(&ctx, &diffs, &s(&repo), url).expect("按链接查到 PR");
    url_pr_with(&ctx, &diffs, &s(&repo), url);
    assert_eq!(fake.count("pr view"), 1, "{:?}", fake.calls.lock().unwrap());
    // 问 gh 时带的是链接本身，不是分支名。
    assert!(fake.calls.lock().unwrap().iter().any(|call| call.contains(url)), "{:?}", fake.calls.lock().unwrap());
    assert_eq!(pr, PullRequestDto { number: pr.number, state: pr.state.clone(), url: pr.url.clone(), title: pr.title.clone(), ..Default::default() });
    // 不是 PR 链接的字符串不交给 gh。
    assert_eq!(url_pr_with(&ctx, &diffs, &s(&repo), "https://github.com/o/r/issues/1"), None);
    assert_eq!(fake.count("pr view"), 1);
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
    let fake = FakeGh::new();
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &repo), None);
    let plain = sb.root.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &plain), None);
    assert_eq!(title(&fake, &caches, &diffs, &prefs, &sb.root.join("missing")), None);
}

/// 回退修复（coordinator 11-11）：forge 只认 github.com，企业版这类远端解析不出仓库。标题栏以前照样能
/// 直接问 gh 拿到 PR，不能因为改走共用缓存就丢掉；这一支照旧按目录缓存 60 秒。
#[test]
fn 标题栏_企业版远端_照旧直查_六十秒只调一次() {
    let sb = Sandbox::new("title-pr-ghe");
    let repo = sb.repo("ghe");
    sb.git(&repo, &["remote", "add", "origin", "git@github.example.com:o/r.git"]);
    sb.git(&repo, &["switch", "-q", "-c", "feat"]);
    let fake = FakeGh::new().views(vec![ok(&fixture("pr_checkrun_fail.json"))]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let pr = title(&fake, &caches, &diffs, &prefs, &repo).expect("企业版远端也要拿到 PR");
    assert_eq!(pr, PullRequestDto { number: pr.number, state: pr.state.clone(), url: pr.url.clone(), title: pr.title.clone(), ..Default::default() });
    for _ in 0..3 {
        assert!(title(&fake, &caches, &diffs, &prefs, &repo).is_some());
    }
    assert_eq!(fake.count("pr view"), 1, "60 秒内只调一次：{:?}", fake.calls.lock().unwrap());
    assert_eq!(fake.count("auth status"), 0, "直查这一支与以前一样不先问登录状态");
}
