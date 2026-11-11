//! 侧栏 PR / CI 与改动数（forge/badge.rs）的节流：每仓库 60 秒内只查一次 GitHub、窗口隐藏时一次 gh 都不调、
//! 没装 gh 只给改动数。gh 全是假的（父模块的 FakeGh），git 是真的本地仓库。

use super::super::badge::{badges_with, DiffCache, DiffCounts};
use super::*;
use std::time::{Duration, Instant};

const DIRTY: Option<DiffCounts> = Some((12, 3, 2));

fn run(fake: &FakeGh, caches: &Caches, diffs: &DiffCache, prefs: &Path, cwds: &[String], online: bool, counts: Option<DiffCounts>) -> Vec<SidebarGitBadgeDto> {
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches, prefs: prefs.to_path_buf(), env: &[] };
    badges_with(&ctx, diffs, cwds, online, &|_| counts)
}

use meowo_protocol::ipc::SidebarGitBadgeDto;

#[test]
fn 侧栏_同一仓库六十秒内只查一次_pr() {
    let sb = Sandbox::new("badge-ttl");
    let repo = github_repo(&sb);
    let fake = FakeGh::new().views(vec![ok(&fixture("pr_checkrun_fail.json"))]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    // 同一目录在列表里出现多次（同仓库多个会话）只算一次。
    let cwds = vec![s(&repo), s(&repo), s(&repo)];
    let first = run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
    assert_eq!(first.len(), 1);
    let pr = first[0].pr.as_ref().expect("有 PR");
    assert_eq!(pr.checks.as_deref(), Some("fail"));
    assert_eq!((first[0].additions, first[0].deletions, first[0].files), (12, 3, 2));
    for _ in 0..5 {
        run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
    }
    assert_eq!(fake.count("pr view"), 1, "60 秒内反复刷新只打一次 gh pr view：{:?}", fake.calls.lock().unwrap());
    // 过了 60 秒才再查。
    for entry in lock(&caches.prs).values_mut() {
        entry.0 = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    }
    run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
    assert_eq!(fake.count("pr view"), 2);
}

#[test]
fn 侧栏_窗口隐藏时一次_gh_都不调() {
    let sb = Sandbox::new("badge-hidden");
    let repo = github_repo(&sb);
    let fake = FakeGh::new().views(vec![ok(&fixture("pr_checkrun_fail.json"))]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let cwds = vec![s(&repo)];
    // 冷缓存 + 隐藏：只给改动数，gh 零调用（连 auth status 都没有）。
    let hidden = run(&fake, &caches, &diffs, &prefs, &cwds, false, DIRTY);
    assert_eq!(hidden.len(), 1);
    assert!(hidden[0].pr.is_none());
    assert!(fake.calls.lock().unwrap().is_empty(), "{:?}", fake.calls.lock().unwrap());
    // 看得见时查一次，之后隐藏：缓存过期了也只读缓存，照样零调用。
    run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
    let before = fake.calls.lock().unwrap().len();
    for entry in lock(&caches.prs).values_mut() {
        entry.0 = Instant::now().checked_sub(Duration::from_secs(600)).unwrap();
    }
    let stale = run(&fake, &caches, &diffs, &prefs, &cwds, false, DIRTY);
    assert!(stale[0].pr.is_some(), "隐藏时沿用缓存里的 PR");
    assert_eq!(fake.calls.lock().unwrap().len(), before);
}

#[test]
fn 侧栏_没装_gh_只给改动数_干净且无_pr_的不回() {
    let sb = Sandbox::new("badge-nogh");
    let repo = github_repo(&sb);
    let mut fake = FakeGh::new();
    fake.installed = false;
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let dirty = run(&fake, &caches, &diffs, &prefs, &[s(&repo)], true, DIRTY);
    assert_eq!(dirty.len(), 1);
    assert!(dirty[0].pr.is_none());
    assert_eq!(dirty[0].additions, 12);
    let clean = run(&fake, &caches, &DiffCache::default(), &prefs, &[s(&repo)], true, Some((0, 0, 0)));
    assert!(clean.is_empty(), "没有改动也没有 PR 的会话行什么都不显示");
}

#[test]
fn 侧栏_改动数按目录缓存() {
    let sb = Sandbox::new("badge-diff");
    let repo = github_repo(&sb);
    let fake = FakeGh::new();
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let counted = std::sync::atomic::AtomicUsize::new(0);
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let ctx = Ctx { gh: &gh, caches: &caches, prefs, env: &[] };
    let compute = |_: &str| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        DIRTY
    };
    for _ in 0..4 {
        badges_with(&ctx, &diffs, &[s(&repo)], true, &compute);
    }
    assert_eq!(counted.load(std::sync::atomic::Ordering::SeqCst), 1);
}
