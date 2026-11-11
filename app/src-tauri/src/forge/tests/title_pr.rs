//! 标题栏的 PR（git_pull_request）与侧栏徽标、「更改」面板共用 forge 的 PR 缓存（UI 优化收尾 #3）。
//! 以前标题栏按目录另存一份 60 秒缓存、自己调 `gh pr view`，同一仓库前台时每分钟可能多打一次 gh。

use super::super::badge::{badges_with, title_pr_with, DiffCache};
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
