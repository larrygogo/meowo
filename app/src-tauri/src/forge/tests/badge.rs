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
fn 侧栏_gh_出错也按六十秒节流_不每轮重查() {
    // 断网、限流时 gh pr view 失败：原先出错分支不写缓存时间，前端每 30 秒一轮（目录变化、回前台还会立刻再查）
    // 都会重打一次 gh，限流时越查越限（做空审查第二轮指出）。出错也记下这次查询时间，60 秒内只读缓存。
    let sb = Sandbox::new("badge-err-ttl");
    let repo = github_repo(&sb);
    let fake = FakeGh::new().views(vec![fail("HTTP 403: API rate limit exceeded for user")]);
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    let cwds = vec![s(&repo)];
    for _ in 0..6 {
        let out = run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
        assert!(out[0].pr.is_none(), "出错时不显示 PR");
    }
    assert_eq!(fake.count("pr view"), 1, "出错后 60 秒内不再重查：{:?}", fake.calls.lock().unwrap());
    for entry in lock(&caches.prs).values_mut() {
        entry.0 = Instant::now().checked_sub(Duration::from_secs(61)).unwrap();
    }
    run(&fake, &caches, &diffs, &prefs, &cwds, true, DIRTY);
    assert_eq!(fake.count("pr view"), 2, "过了 60 秒再试一次");
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

#[test]
fn 侧栏_切分支后_pr_跟着换_不等缓存过期() {
    let sb = Sandbox::new("badge-switch");
    let repo = github_repo(&sb);
    let fake = FakeGh::new();
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let prefs = sb.root.join("forge.json");
    run(&fake, &caches, &diffs, &prefs, &[s(&repo)], true, DIRTY);
    run(&fake, &caches, &diffs, &prefs, &[s(&repo)], true, DIRTY);
    assert_eq!(fake.count("pr view meowo/drill "), 1);
    // PR 去向按目录缓存了，但 HEAD 变了就得重算：不然切到新分支后一分钟内还挂着旧分支的 PR。
    sb.git(&repo, &["switch", "-q", "-c", "meowo/other"]);
    run(&fake, &caches, &diffs, &prefs, &[s(&repo)], true, DIRTY);
    assert_eq!(fake.count("pr view meowo/other "), 1, "{:?}", fake.calls.lock().unwrap());
    // 换远端（改 .git/config）同理：不再是 GitHub 仓库就不查 PR。
    sb.git(&repo, &["remote", "set-url", "origin", "https://gitlab.com/x/y.git"]);
    sb.git(&repo, &["switch", "-q", "-c", "meowo/third"]);
    run(&fake, &caches, &diffs, &prefs, &[s(&repo)], true, DIRTY);
    assert_eq!(fake.count("pr view meowo/third"), 0);
}

#[test]
fn 侧栏_读_head_文件与_symbolic_ref_得到同一个_pr_键() {
    let sb = Sandbox::new("badge-headfile");
    let repo = github_repo(&sb);
    let dir = s(&repo);
    let prefs = sb.root.join("forge.json");
    let full = repo_info(&dir, &[], &prefs).unwrap();
    let (fast, _, _) = repo_head(&dir, &[], &prefs, true).unwrap();
    assert!(pr_key(&full).is_some());
    assert_eq!(pr_key(&fast), pr_key(&full));
    assert_eq!(fast.branch.as_deref(), Some("meowo/drill"));
    // detached：两边都没有分支。
    sb.git(&repo, &["switch", "-q", "--detach"]);
    let (fast, _, _) = repo_head(&dir, &[], &prefs, true).unwrap();
    assert_eq!(fast.branch, None);
    assert_eq!(repo_info(&dir, &[], &prefs).unwrap().branch, None);
    // worktree：HEAD 在 worktree 自己的 .git 目录里，键里的公共目录仍是主仓库的。
    let wt = sb.root.join("gh-wt");
    sb.git(&repo, &["worktree", "add", "-q", "-b", "meowo/wt", &wt.to_string_lossy()]);
    let (fast, _, _) = repo_head(&s(&wt), &[], &prefs, true).unwrap();
    let full = repo_info(&s(&wt), &[], &prefs).unwrap();
    assert_eq!(fast.branch.as_deref(), Some("meowo/wt"));
    assert_eq!(pr_key(&fast), pr_key(&full));
}

#[test]
fn 侧栏_head_文件读不懂就交给_git() {
    let sb = Sandbox::new("badge-headparse");
    let dir = sb.root.join("g");
    std::fs::create_dir_all(&dir).unwrap();
    let head = |text: &str| {
        std::fs::write(dir.join("HEAD"), text).unwrap();
        branch_from_head_file(&dir)
    };
    assert_eq!(head("ref: refs/heads/feat/a
"), Some(Some("feat/a".into())));
    assert_eq!(head(&format!("{}
", "a".repeat(40))), Some(None));
    assert_eq!(head("ref: refs/remotes/origin/main
"), None);
    assert_eq!(head("garbage"), None);
    // reftable：HEAD 只是占位，必须问 git。
    std::fs::create_dir_all(dir.join("reftable")).unwrap();
    assert_eq!(head("ref: refs/heads/.invalid
"), None);
}

/// 基准：20 个工作目录（1 个带 15000 个未跟踪文件的大仓库、13 个普通仓库、6 个非仓库）跑一遍侧栏摘要，
/// 量冷 / 热 / 稳态轮询的耗时与 git 进程数。gh 是假的（已登录、没有 PR），git 是真的。
/// ignored：要造 15000 个文件，跑 release；`MEOWO_BADGE_BENCH_DIR` 指定造仓库的位置（默认系统临时目录）：
/// `cargo test --release -p meowo-app --lib forge::tests::badge::bench_sidebar_badges_20_dirs -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_sidebar_badges_20_dirs() {
    use super::super::badge::real_diff_counts;
    let base = std::env::var_os("MEOWO_BADGE_BENCH_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!("badge-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("empty.gitconfig");
    std::fs::write(&config, "").unwrap();
    // Drop 时删掉整个目录（含大仓库）。
    let sb = Sandbox { root: root.clone(), config };
    let github = |dir: &Path| {
        sb.git(dir, &["remote", "add", "origin", "https://github.com/larrygogo/meowo-pr-drill.git"]);
        let init = sb.git(dir, &["rev-parse", "HEAD"]);
        sb.git(dir, &["update-ref", "refs/remotes/origin/main", &init]);
        sb.git(dir, &["switch", "-q", "-c", "meowo/drill"]);
    };
    let mut dirs = Vec::new();
    let big = sb.repo("big");
    github(&big);
    for i in 0..15000 {
        let sub = big.join("target-x").join(format!("d{}", i / 100));
        if i % 100 == 0 {
            std::fs::create_dir_all(&sub).unwrap();
        }
        std::fs::write(sub.join(format!("f{i}.rlib")), "x\n").unwrap();
    }
    std::fs::write(big.join("README.md"), "edited\nmore\n").unwrap();
    dirs.push(big.clone());
    for i in 0..13 {
        let repo = sb.repo(&format!("n{i}"));
        if i % 2 == 0 {
            github(&repo);
        }
        if i % 3 != 0 {
            std::fs::write(repo.join("README.md"), "changed\n").unwrap();
            std::fs::write(repo.join(format!("new{i}.txt")), "a\nb\n").unwrap();
        }
        dirs.push(repo);
    }
    for i in 0..6 {
        let plain = root.join(format!("plain{i}"));
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("notes.txt"), "x\n").unwrap();
        dirs.push(plain);
    }
    let cwds: Vec<String> = dirs.iter().map(|d| s(d)).collect();
    let fake = FakeGh::new();
    let gh = |dir: &str, args: &[&str]| fake.run(dir, args);
    let prefs = root.join("forge.json");
    let trace = root.join("trace2.jsonl");
    // git 进程数：GIT_TRACE2_EVENT 每个 git 进程写一条 start 事件；sid 带「/」的是 git 自己起的子进程，不算。
    // 计数单独一轮跑（写 trace 有开销，不和计时混在一起）。
    let spawns = |run: &dyn Fn()| -> usize {
        let _ = std::fs::remove_file(&trace);
        std::env::set_var("GIT_TRACE2_EVENT", &trace);
        run();
        std::env::remove_var("GIT_TRACE2_EVENT");
        std::fs::read_to_string(&trace)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("\"event\":\"start\""))
            .filter(|l| l.split("\"sid\":\"").nth(1).and_then(|r| r.split('"').next()).is_some_and(|sid| !sid.contains('/')))
            .count()
    };
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    let median = |mut v: Vec<Duration>| {
        v.sort();
        v[v.len() / 2]
    };
    // 冷：全新缓存（进程刚起、或缓存都过期）。
    let cold = |list: &[String]| {
        crate::git::invalidate_caches();
        let (caches, diffs) = (Caches::default(), DiffCache::default());
        let ctx = Ctx { gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
        let start = Instant::now();
        let rows = badges_with(&ctx, &diffs, list, true, &real_diff_counts);
        (start.elapsed(), rows)
    };
    let (_, rows) = cold(&cwds);
    let shown: Vec<String> = rows.iter().map(|r| format!("{}:+{}-{}/{}", r.cwd.rsplit('/').next().unwrap(), r.additions, r.deletions, r.files)).collect();
    eprintln!("回包 {} 条：{shown:?}", rows.len());
    let cold_all = median((0..5).map(|_| cold(&cwds).0).collect());
    let cold_big = median((0..5).map(|_| cold(std::slice::from_ref(&cwds[0])).0).collect());
    let cold_spawns = spawns(&|| {
        cold(&cwds);
    });
    let big_spawns = spawns(&|| {
        cold(std::slice::from_ref(&cwds[0]));
    });
    // 热：同一份缓存 15 秒内再问（侧栏切回前台、列表换了引用）。
    // 稳态：前端每 30 秒问一次，改动缓存（15s）与折叠计数（30s）都已过期，PR（60s）与登录状态（5min）还新。
    crate::git::invalidate_caches();
    let (caches, diffs) = (Caches::default(), DiffCache::default());
    let ctx = Ctx { gh: &gh, caches: &caches, prefs: prefs.clone(), env: &[] };
    badges_with(&ctx, &diffs, &cwds, true, &real_diff_counts);
    let hot_all = median(
        (0..5)
            .map(|_| {
                let start = Instant::now();
                badges_with(&ctx, &diffs, &cwds, true, &real_diff_counts);
                start.elapsed()
            })
            .collect(),
    );
    let hot_spawns = spawns(&|| {
        badges_with(&ctx, &diffs, &cwds, true, &real_diff_counts);
    });
    let steady = || {
        crate::git::invalidate_caches();
        diffs.expire_counts();
        let start = Instant::now();
        badges_with(&ctx, &diffs, &cwds, true, &real_diff_counts);
        start.elapsed()
    };
    let steady_all = median((0..5).map(|_| steady()).collect());
    let steady_spawns = spawns(&|| {
        steady();
    });
    eprintln!("冷（20 目录）：{:.1} ms，git 进程 {cold_spawns}", ms(cold_all));
    eprintln!("冷（仅大仓库）：{:.1} ms，git 进程 {big_spawns}", ms(cold_big));
    eprintln!("热（15s 内复查）：{:.1} ms，git 进程 {hot_spawns}", ms(hot_all));
    eprintln!("稳态（每 30s 一轮）：{:.1} ms，git 进程 {steady_spawns}", ms(steady_all));
    eprintln!("gh 调用：{:?}", fake.calls.lock().unwrap().iter().take(3).collect::<Vec<_>>());
}
