//! 真实 GitHub 演练（二期验收 #26，leader 关卡 D-20261110-pr-drill-repo 授权）：
//! 在一个专门的私有演练仓库里，用二期新写的后端函数完整走一遍
//! 建 worktree → 提交 → push → 开 PR → 等 CI → CI 失败的 PR 合不了 → 合并 → 本地判定可清理 → 删 worktree。
//!
//! 会对远端写入，所以默认 ignored，并且只在显式给出演练仓库路径时才跑：
//! `MEOWO_DRILL_REPO=<演练仓库的本地克隆> cargo test -p meowo-app --lib drill -- --ignored --nocapture`
//! 演练仓库要求：main 上有一个必跑的 CI 检查；另有一个 `ci-fail` 分支会让检查失败。
//! 不碰 ~/.meowo：合并方式偏好写到临时目录，setup 日志也写临时目录。

use super::*;
use crate::wtnew::{self, Location, Options, Request};
use std::sync::Arc;

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn gh(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("gh")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("gh");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// 每 15s 强制刷新一次（后端 force 最短 10s），直到 checks 不再是 pending，最多 6 分钟。
fn wait_checks(ctx: &Ctx, dir: &str) -> ForgeStatusDto {
    let start = std::time::Instant::now();
    loop {
        let status = forge_status_with(ctx, dir, true).expect("forge status");
        let summary = status.pr.as_ref().and_then(|p| p.checks_summary.clone());
        eprintln!(
            "[drill] {:>4}s checks={summary:?}",
            start.elapsed().as_secs()
        );
        // 还要等 GitHub 算出能不能合并：刚开的 PR mergeable 常是 UNKNOWN，后端按“进行中”拒绝合并
        // （2026-11-11 重跑实测，第一次演练只是赶巧没碰上）。
        let mergeable_known = status
            .pr
            .as_ref()
            .and_then(|p| p.mergeable.as_deref())
            .is_some_and(|m| !m.eq_ignore_ascii_case("UNKNOWN"));
        if summary.as_deref().is_some_and(|s| s != "pending") && mergeable_known {
            return status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(360),
            "CI 6 分钟没出结果"
        );
        std::thread::sleep(Duration::from_secs(15));
    }
}

#[test]
#[ignore]
fn drill_real_github_end_to_end() {
    let Ok(repo) = std::env::var("MEOWO_DRILL_REPO") else {
        eprintln!("没设 MEOWO_DRILL_REPO，跳过");
        return;
    };
    let repo = PathBuf::from(repo);
    let tmp = std::env::temp_dir().join(format!("meowo-drill-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let ctx = Ctx {
        gh: &real_gh,
        caches: &CACHES,
        prefs: tmp.join("forge.json"),
        env: &[],
    };

    sh(&repo, &["checkout", "-q", "main"]);
    sh(&repo, &["pull", "-q", "--ff-only"]);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 1. 建 worktree（同步部分：分支、目录、复制）。
    let opts = Options {
        location: Location::Sibling,
        branch_prefix: "meowo/".into(),
        shells: Default::default(),
        log_dir: tmp.clone(),
        extra_env: vec![],
    };
    let req = Request {
        slug: Some(format!("drill-{stamp}")),
        branch: None,
        fallback_slug: None,
    };
    let created =
        wtnew::create(&repo, &req, &opts, &|_, _| true, Arc::new(|_| {})).expect("建 worktree");
    eprintln!("[drill] worktree {} 分支 {}", created.path, created.branch);
    let wt = PathBuf::from(&created.path);

    // 2. 提交一个改动，push（无上游，应自动设上游）。
    std::fs::write(wt.join(format!("drill-{stamp}.txt")), "drill\n").unwrap();
    sh(&wt, &["add", "."]);
    sh(
        &wt,
        &[
            "-c",
            "user.name=meowo-drill",
            "-c",
            "user.email=drill@localhost",
            "commit",
            "-qm",
            "drill: 演练提交",
        ],
    );
    let pushed = push_with(&created.path, ctx.env, PUSH_TIMEOUT).expect("push");
    assert!(pushed.set_upstream, "第一次推送应设上游: {pushed:?}");

    // 3. 开 PR（已推过，push 回调不该再被要求做事，但照样给真实实现）。
    let pr = pr_create_with(&ctx, &created.path, None, None, None, false, &|c| {
        push_with(c, &[], PUSH_TIMEOUT)
    })
    .expect("开 PR");
    eprintln!("[drill] PR #{} {}", pr.number, pr.url);

    // 4. CI 失败的 PR 合不了：基于 ci-fail 分支另开一个 PR。
    let fail_wt = tmp.join("ci-fail");
    sh(&repo, &["fetch", "-q", "origin", "ci-fail"]);
    sh(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-B",
            "ci-fail",
            fail_wt.to_str().unwrap(),
            "origin/ci-fail",
        ],
    );
    sh(
        &fail_wt,
        &["branch", "-q", "--set-upstream-to", "origin/ci-fail"],
    );
    let fail_dir = fail_wt.to_string_lossy().into_owned();
    let fail_pr = match pr_create_with(
        &ctx,
        &fail_dir,
        Some("drill: CI 故意失败"),
        Some("演练用"),
        None,
        false,
        &|c| push_with(c, &[], PUSH_TIMEOUT),
    ) {
        Ok(p) => p,
        // 上一次演练留下的同分支 PR 还开着：后端返回已存在。
        Err(e) if e.starts_with("gh/pr_exists") => forge_status_with(&ctx, &fail_dir, true)
            .unwrap()
            .pr
            .expect("已有 PR"),
        Err(e) => panic!("开失败 PR: {e}"),
    };
    let fail_status = wait_checks(&ctx, &fail_dir);
    let fail_head = fail_status
        .pr
        .as_ref()
        .and_then(|p| p.head_oid.clone())
        .unwrap();
    assert_eq!(
        fail_status.pr.as_ref().unwrap().checks_summary.as_deref(),
        Some("fail")
    );
    let refused = pr_merge_with(&ctx, &fail_dir, fail_pr.number, "squash", &fail_head).unwrap_err();
    eprintln!("[drill] CI 失败的 PR 合并被拒：{refused}");
    assert!(refused.starts_with("gh/checks_failed"), "{refused}");
    gh(&repo, &["pr", "close", &fail_pr.number.to_string()]);
    sh(
        &repo,
        &["worktree", "remove", "--force", fail_wt.to_str().unwrap()],
    );

    // 5. 等自己的 PR CI 通过后合并（squash，校验 head 提交）。
    let status = wait_checks(&ctx, &created.path);
    let mine = status.pr.expect("PR 状态");
    assert_eq!(mine.checks_summary.as_deref(), Some("pass"), "{mine:?}");
    assert!(
        status.merge_methods.contains(&"squash".to_string()),
        "{:?}",
        status.merge_methods
    );
    let merged = pr_merge_with(
        &ctx,
        &created.path,
        mine.number,
        "squash",
        mine.head_oid.as_deref().unwrap(),
    )
    .expect("合并");
    assert!(merged.merged, "{merged:?}");
    eprintln!("[drill] PR #{} 已合并", mine.number);

    // 6. 合并后 worktree 可清理（gh 判定），并真的删掉；远端分支不动。
    let nobody = |_: &str| 0u32;
    let st = crate::worktree::status_for_dir(&wt, &nobody, &crate::worktree::gh_pr_lookup);
    assert!(st.eligible, "合并后应可清理: {st:?}");
    // 同一个 worktree，假装没装 gh：本地判定也要认出这次 squash 合并。
    let no_gh = |_: &Path| crate::worktree::PrLookup::Unavailable;
    let local = crate::worktree::status_for_dir(&wt, &nobody, &no_gh);
    assert!(local.eligible, "没 gh 时本地判定也应可清理: {local:?}");
    let top = sh(&wt, &["rev-parse", "--show-toplevel"]);
    let removed = crate::worktree::remove_dir_worktree(
        &wt,
        &top,
        &nobody,
        &crate::worktree::gh_pr_lookup,
        &[],
    )
    .expect("删 worktree");
    assert!(removed.removed && !wt.exists());
    eprintln!("[drill] worktree 已清理，全程通过");
    let _ = std::fs::remove_dir_all(&tmp);
}
