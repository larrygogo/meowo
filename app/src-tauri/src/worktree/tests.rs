use super::*;
use std::process::Command as StdCommand;

// ═══ 判定（纯函数）═══

fn good() -> Facts {
    Facts {
        linked_worktree: true,
        branch: Some("feat/x".into()),
        other_sessions: 0,
        pr: PrLookup::Found {
            number: 7,
            state: "MERGED".into(),
            head_oid: "abc".into(),
        },
        submodules: SubmoduleState::Clean,
        dirty: 0,
        untracked: 0,
        unpushed: false,
        locked: false,
        nested: 0,
        hidden: 0,
    }
}

fn reason(f: Facts) -> Option<&'static str> {
    evaluate(&f).map(|(r, _)| r)
}

#[test]
fn 条件全满足才可清理() {
    assert_eq!(reason(good()), None);
}

#[test]
fn 主检出永远不可清理() {
    assert_eq!(
        reason(Facts {
            linked_worktree: false,
            ..good()
        }),
        Some("not_worktree")
    );
}

#[test]
fn pr_没合并不可清理() {
    let open = Facts {
        pr: PrLookup::Found {
            number: 1,
            state: "OPEN".into(),
            head_oid: "a".into(),
        },
        ..good()
    };
    assert_eq!(reason(open), Some("pr_open"));
    let closed = Facts {
        pr: PrLookup::Found {
            number: 1,
            state: "CLOSED".into(),
            head_oid: "a".into(),
        },
        ..good()
    };
    assert_eq!(reason(closed), Some("pr_closed"));
    assert_eq!(
        reason(Facts {
            pr: PrLookup::NoPr,
            ..good()
        }),
        Some("no_pr")
    );
}

#[test]
fn gh_不可用时判断不了_不可清理() {
    assert_eq!(
        reason(Facts {
            pr: PrLookup::Unavailable,
            ..good()
        }),
        Some("gh_unavailable")
    );
}

#[test]
fn 有未提交改动不可清理并带数量() {
    assert_eq!(
        evaluate(&Facts { dirty: 3, ..good() }),
        Some(("dirty", Some(3)))
    );
}

#[test]
fn 有未跟踪文件不可清理() {
    assert_eq!(
        evaluate(&Facts {
            untracked: 2,
            ..good()
        }),
        Some(("untracked", Some(2)))
    );
}

#[test]
fn 有没推的提交不可清理() {
    assert_eq!(
        reason(Facts {
            unpushed: true,
            ..good()
        }),
        Some("unpushed")
    );
}

#[test]
fn 子模块提交不在远端不可清理() {
    assert_eq!(
        reason(Facts {
            submodules: SubmoduleState::Unpushed,
            ..good()
        }),
        Some("submodule_unpushed")
    );
}

#[test]
fn 子模块有改动不可清理() {
    assert_eq!(
        reason(Facts {
            submodules: SubmoduleState::Dirty,
            ..good()
        }),
        Some("submodule_dirty")
    );
}

#[test]
fn 别的未归档会话在用不可清理() {
    assert_eq!(
        evaluate(&Facts {
            other_sessions: 2,
            ..good()
        }),
        Some(("shared_session", Some(2)))
    );
}

#[test]
fn 被锁定或游离头不可清理() {
    assert_eq!(
        reason(Facts {
            locked: true,
            ..good()
        }),
        Some("locked")
    );
    assert_eq!(
        reason(Facts {
            branch: None,
            ..good()
        }),
        Some("detached")
    );
}

#[test]
fn 解析_gh_输出() {
    assert_eq!(
        parse_pr_view(r#"{"number":12,"state":"MERGED","headRefOid":"deadbeef"}"#),
        Some(PrLookup::Found {
            number: 12,
            state: "MERGED".into(),
            head_oid: "deadbeef".into()
        })
    );
    assert_eq!(parse_pr_view("not json"), None);
}

#[test]
fn 目录包含关系不把同前缀的兄弟目录算进去() {
    assert!(is_under("c:/w/a", "c:/w/a"));
    assert!(is_under("c:/w/a/src", "c:/w/a"));
    assert!(!is_under("c:/w/ab", "c:/w/a"));
}

// ═══ 真实 git 仓库 ═══

struct Sandbox {
    root: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // 先拆链接再删，免得清理测试目录时顺着 junction 删到别处。
        let _ = unlink_links(&self.root);
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn run(dir: &Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?} 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn make_link(target: &Path, link: &Path) {
    #[cfg(target_os = "windows")]
    {
        let ok = StdCommand::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("mklink")
            .status
            .success();
        assert!(ok, "建 junction 失败");
    }
    #[cfg(not(target_os = "windows"))]
    std::os::unix::fs::symlink(target, link).expect("建符号链接");
}

fn count_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|it| it.count()).unwrap_or(0)
}

/// 远端（bare）+ 主检出 + 一个 linked worktree（分支 feat/done，已推到远端），
/// worktree 里的 node_modules 是指向主检出 `deps/` 的链接，和真实项目一样被 .gitignore 忽略。
struct Fixture {
    _sandbox: Sandbox,
    main: PathBuf,
    remote: PathBuf,
    wt: PathBuf,
    deps: PathBuf,
}

fn fixture(tag: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "meowo-wt-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let sandbox = Sandbox { root: root.clone() };
    let remote = root.join("remote.git");
    let main = root.join("main");
    StdCommand::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&remote)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["init", "-q", "-b", "main"])
        .arg(&main)
        .output()
        .unwrap();
    run(&main, &["config", "user.name", "t"]);
    run(&main, &["config", "user.email", "t@t"]);
    run(&main, &["config", "core.autocrlf", "false"]);
    std::fs::write(main.join(".gitignore"), "node_modules\n").unwrap();
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    run(&main, &["add", "."]);
    run(&main, &["commit", "-qm", "init"]);
    run(
        &main,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run(&main, &["push", "-q", "-u", "origin", "main"]);

    let deps = main.join("deps");
    std::fs::create_dir_all(deps.join("pkg")).unwrap();
    for i in 0..5 {
        std::fs::write(deps.join(format!("f{i}.js")), "x").unwrap();
    }

    let wt = root.join("wt-done");
    run(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/done",
            wt.to_str().unwrap(),
        ],
    );
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    run(&wt, &["add", "."]);
    run(&wt, &["commit", "-qm", "feat"]);
    run(&wt, &["push", "-q", "-u", "origin", "feat/done"]);
    make_link(&deps, &wt.join("node_modules"));
    Fixture {
        _sandbox: sandbox,
        main,
        remote,
        wt,
        deps,
    }
}

fn merged(head: String) -> impl Fn(&Path) -> PrLookup {
    move |_| PrLookup::Found {
        number: 1,
        state: "MERGED".into(),
        head_oid: head.clone(),
    }
}

/// 测试里「用户在确认框看到的目录」就是此刻解析出的 worktree 根目录。
fn top_of(dir: &Path) -> String {
    run(dir, &["rev-parse", "--show-toplevel"])
}

fn nobody(_: &str) -> u32 {
    0
}

#[test]
fn 删除已合并的_worktree_只拆链接不伤主仓库() {
    let fx = fixture("ok");
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    let deps_before = count_files(&fx.deps);
    let status = status_for_dir(&fx.wt, &nobody, &merged(head.clone()));
    assert!(status.eligible, "应可清理: {status:?}");

    let result = remove_dir_worktree(&fx.wt, &top_of(&fx.wt), &nobody, &merged(head), &[])
        .expect("删除成功");
    assert!(result.removed);
    assert!(result.branch_deleted);
    assert!(!fx.wt.exists(), "worktree 目录应已删除");
    let list = run(&fx.main, &["worktree", "list", "--porcelain"]);
    assert!(
        !list.to_lowercase().contains("wt-done"),
        "git 不应再列出它: {list}"
    );
    assert!(
        run(&fx.main, &["branch", "--list", "feat/done"]).is_empty(),
        "本地分支应已删"
    );
    assert!(
        run(&fx.remote, &["branch", "--list", "feat/done"]).contains("feat/done"),
        "远端分支不动"
    );
    assert_eq!(
        count_files(&fx.deps),
        deps_before,
        "链接目标里的文件一个都不能少"
    );
}

#[test]
fn 不满足条件时拒绝删除且原样保留() {
    let fx = fixture("dirty");
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    std::fs::write(fx.wt.join("a.txt"), "changed\n").unwrap();
    let err = remove_dir_worktree(&fx.wt, &top_of(&fx.wt), &nobody, &merged(head.clone()), &[])
        .unwrap_err();
    assert_eq!(err, "worktree/dirty");
    assert!(fx.wt.join("a.txt").exists());
    assert!(fx.wt.join("node_modules").exists(), "链接也不该被拆");
    assert!(!run(&fx.main, &["branch", "--list", "feat/done"]).is_empty());

    // PR 没合并：同样拒绝。
    run(&fx.wt, &["checkout", "-q", "--", "a.txt"]);
    let open = |_: &Path| PrLookup::Found {
        number: 1,
        state: "OPEN".into(),
        head_oid: head.clone(),
    };
    assert_eq!(
        remove_dir_worktree(&fx.wt, &top_of(&fx.wt), &nobody, &open, &[]).unwrap_err(),
        "worktree/pr_open"
    );
    assert!(fx.wt.exists());
}

#[test]
fn 本地多出没推的提交时拒绝() {
    let fx = fixture("ahead");
    let pr_head = run(&fx.wt, &["rev-parse", "HEAD"]);
    std::fs::write(fx.wt.join("c.txt"), "c\n").unwrap();
    run(&fx.wt, &["add", "."]);
    run(&fx.wt, &["commit", "-qm", "local only"]);
    let status = status_for_dir(&fx.wt, &nobody, &merged(pr_head));
    assert_eq!(status.reason.as_deref(), Some("unpushed"));
}

#[test]
fn 主检出和共用目录的会话都不可清理() {
    let fx = fixture("main");
    let head = run(&fx.main, &["rev-parse", "HEAD"]);
    let status = status_for_dir(&fx.main, &nobody, &merged(head.clone()));
    assert_eq!(status.reason.as_deref(), Some("not_worktree"));
    let wt_head = run(&fx.wt, &["rev-parse", "HEAD"]);
    let one = |_: &str| 1;
    let status = status_for_dir(&fx.wt, &one, &merged(wt_head));
    assert_eq!(status.reason.as_deref(), Some("shared_session"));
}

#[test]
fn 有进程占着目录时先列出来_确认后结束进程再删() {
    let fx = fixture("busy");
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    // 一个当前目录在 worktree 里的进程，命令行里不出现这个路径——只能靠读当前目录发现。
    #[cfg(target_os = "windows")]
    let mut child = StdCommand::new("cmd")
        .args(["/c", "ping -n 60 127.0.0.1 >nul"])
        .current_dir(&fx.wt)
        .spawn()
        .unwrap();
    #[cfg(not(target_os = "windows"))]
    let mut child = StdCommand::new("sleep")
        .arg("60")
        .current_dir(&fx.wt)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));

    let first = remove_dir_worktree(&fx.wt, &top_of(&fx.wt), &nobody, &merged(head.clone()), &[])
        .expect("返回占用清单");
    assert!(!first.removed);
    assert!(
        first.blockers.iter().any(|p| p.pid == child.id()),
        "占用清单里应有子进程 {}: {:?}",
        child.id(),
        first.blockers
    );
    assert!(fx.wt.exists(), "没确认前什么都不动");

    // 确认之后又冒出一个新进程：用户没看到它的名字，不能顺手结束，要再问一次。
    #[cfg(target_os = "windows")]
    let mut late = StdCommand::new("cmd")
        .args(["/c", "ping -n 60 127.0.0.1 >nul"])
        .current_dir(&fx.wt)
        .spawn()
        .unwrap();
    #[cfg(not(target_os = "windows"))]
    let mut late = StdCommand::new("sleep")
        .arg("60")
        .current_dir(&fx.wt)
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let seen: Vec<u32> = first.blockers.iter().map(|p| p.pid).collect();
    let again = remove_dir_worktree(
        &fx.wt,
        &top_of(&fx.wt),
        &nobody,
        &merged(head.clone()),
        &seen,
    )
    .expect("再问一次");
    assert!(!again.removed, "有没确认过的进程时不能删");
    assert!(again.blockers.iter().any(|p| p.pid == late.id()));
    assert!(fx.wt.exists());

    let all: Vec<u32> = again.blockers.iter().map(|p| p.pid).collect();
    let second = remove_dir_worktree(&fx.wt, &top_of(&fx.wt), &nobody, &merged(head), &all)
        .expect("确认后删除");
    assert!(second.removed);
    assert!(!fx.wt.exists());
    let _ = child.kill();
    let _ = child.wait();
    let _ = late.kill();
    let _ = late.wait();
}

/// 真机探针：真实的 gh、真实的 worktree。默认不跑（会删东西），手动指定目录才跑：
/// `MEOWO_PROBE_WT=<worktree 路径> cargo test --lib probe_real_worktree -- --ignored --nocapture`
/// 用一个 PR 已合并的分支建 worktree（`git worktree add <路径> -b <分支> origin/<分支>`）来验。
#[test]
#[ignore]
fn probe_real_worktree() {
    let Ok(dir) = std::env::var("MEOWO_PROBE_WT") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let status = status_for_dir(&dir, &nobody, &gh_pr_lookup);
    println!("判定: {status:?}");
    assert!(status.eligible, "真实环境下应可清理");
    let first =
        remove_dir_worktree(&dir, &top_of(&dir), &nobody, &gh_pr_lookup, &[]).expect("删除");
    let pids: Vec<u32> = first.blockers.iter().map(|p| p.pid).collect();
    println!("占用: {:?}", first.blockers);
    let result = if first.removed {
        first
    } else {
        remove_dir_worktree(&dir, &top_of(&dir), &nobody, &gh_pr_lookup, &pids).expect("删除")
    };
    println!("结果: {result:?}");
    assert!(result.removed && !dir.exists());
}

// ═══ 做空验证复现过的误删场景（2026-11-09）═══

fn reason_of(fx: &Fixture) -> Option<String> {
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    status_for_dir(&fx.wt, &nobody, &merged(head)).reason
}

/// 子模块远端 + 在 worktree 里初始化好的子模块 `dir_name`。
fn add_submodule(fx: &Fixture, dir_name: &str) -> PathBuf {
    let root = fx.main.parent().unwrap();
    let sub_remote = root.join(format!("sub-{}.git", dir_name.replace(' ', "-")));
    let seed = root.join(format!("sub-seed-{}", dir_name.replace(' ', "-")));
    StdCommand::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&sub_remote)
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["init", "-q", "-b", "main"])
        .arg(&seed)
        .output()
        .unwrap();
    run(&seed, &["config", "user.name", "t"]);
    run(&seed, &["config", "user.email", "t@t"]);
    std::fs::write(seed.join("s.txt"), "s\n").unwrap();
    run(&seed, &["add", "."]);
    run(&seed, &["commit", "-qm", "sub"]);
    run(&seed, &["push", "-q", sub_remote.to_str().unwrap(), "main"]);
    run(
        &fx.wt,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            sub_remote.to_str().unwrap(),
            dir_name,
        ],
    );
    run(&fx.wt, &["commit", "-qm", "add sub"]);
    run(&fx.wt, &["push", "-q"]);
    let sub = fx.wt.join(dir_name);
    run(&sub, &["config", "user.name", "t"]);
    run(&sub, &["config", "user.email", "t@t"]);
    sub
}

#[test]
fn 子模块里只在本地的提交_父仓库也提交推送了_仍不可清理() {
    let fx = fixture("sublocal");
    let sub = add_submodule(&fx, "libs");
    std::fs::write(sub.join("local.txt"), "x\n").unwrap();
    run(&sub, &["add", "."]);
    run(&sub, &["commit", "-qm", "local only"]);
    run(&fx.wt, &["add", "libs"]);
    run(&fx.wt, &["commit", "-qm", "bump"]);
    run(&fx.wt, &["push", "-q"]);
    assert_eq!(reason_of(&fx).as_deref(), Some("submodule_unpushed"));
}

#[test]
fn 带空格的子模块路径里有新文件_不可清理() {
    let fx = fixture("subspace");
    let sub = add_submodule(&fx, "lib sub");
    std::fs::write(sub.join("new.txt"), "x\n").unwrap();
    assert_eq!(reason_of(&fx).as_deref(), Some("submodule_dirty"));
}

#[test]
fn 嵌套在忽略目录里的_worktree_不可清理() {
    let fx = fixture("nested");
    std::fs::write(fx.wt.join(".gitignore"), "node_modules\n.claude/\n").unwrap();
    run(&fx.wt, &["add", ".gitignore"]);
    run(&fx.wt, &["commit", "-qm", "ignore"]);
    run(&fx.wt, &["push", "-q"]);
    let nested = fx.wt.join(".claude").join("worktrees").join("n");
    run(
        &fx.main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/nested",
            nested.to_str().unwrap(),
        ],
    );
    std::fs::write(nested.join("work.txt"), "wip\n").unwrap();
    assert_eq!(reason_of(&fx).as_deref(), Some("nested_repo"));
}

#[test]
fn 直接_add_进来的嵌套仓库_不可清理() {
    let fx = fixture("embedded");
    let tool = fx.wt.join("tool");
    StdCommand::new("git")
        .args(["init", "-q", "-b", "main"])
        .arg(&tool)
        .output()
        .unwrap();
    run(&tool, &["config", "user.name", "t"]);
    run(&tool, &["config", "user.email", "t@t"]);
    std::fs::write(tool.join("t.txt"), "t\n").unwrap();
    run(&tool, &["add", "."]);
    run(&tool, &["commit", "-qm", "local"]);
    run(&fx.wt, &["add", "tool"]);
    run(&fx.wt, &["commit", "-qm", "embed"]);
    run(&fx.wt, &["push", "-q"]);
    let reason = reason_of(&fx);
    assert!(
        matches!(
            reason.as_deref(),
            Some("nested_repo") | Some("submodule_unpushed")
        ),
        "{reason:?}"
    );
}

#[test]
fn 关掉未跟踪文件显示时_新文件仍算未提交() {
    let fx = fixture("showuntracked");
    run(&fx.wt, &["config", "status.showUntrackedFiles", "no"]);
    std::fs::write(fx.wt.join("notes.md"), "n\n").unwrap();
    assert_eq!(reason_of(&fx).as_deref(), Some("untracked"));
}

#[test]
fn 被_skip_worktree_藏起来的改动_不可清理() {
    let fx = fixture("skipwt");
    std::fs::write(fx.wt.join("a.txt"), "local config\n").unwrap();
    run(&fx.wt, &["update-index", "--skip-worktree", "a.txt"]);
    assert_eq!(reason_of(&fx).as_deref(), Some("hidden_changes"));
}

#[test]
fn 本地_head_不在_pr_head_之下时不可清理_即使已推到远端() {
    // 分支名沿用旧名：gh 拿到的是旧的已合并 PR，本地有新提交且推上去了。
    let fx = fixture("reuse");
    let old_head = run(&fx.wt, &["rev-parse", "HEAD"]);
    std::fs::write(fx.wt.join("c.txt"), "c\n").unwrap();
    run(&fx.wt, &["add", "."]);
    run(&fx.wt, &["commit", "-qm", "new work"]);
    run(&fx.wt, &["push", "-q"]);
    let status = status_for_dir(&fx.wt, &nobody, &merged(old_head));
    assert_eq!(status.reason.as_deref(), Some("unpushed"));
}

#[test]
fn bare_仓库布局下删对仓库_不碰旁边仓库的同名分支() {
    let fx = fixture("bare");
    let ws = fx.main.parent().unwrap().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    // ws 本身也是个仓库，并且有同名分支 feat/x——旧实现会在这里执行 branch -D。
    StdCommand::new("git")
        .args(["init", "-q", "-b", "main"])
        .arg(&ws)
        .output()
        .unwrap();
    run(&ws, &["config", "user.name", "t"]);
    run(&ws, &["config", "user.email", "t@t"]);
    std::fs::write(ws.join("w.txt"), "w\n").unwrap();
    run(&ws, &["add", "w.txt"]);
    run(&ws, &["commit", "-qm", "ws"]);
    run(&ws, &["branch", "feat/x"]);
    std::fs::write(ws.join(".gitignore"), "*\n").unwrap();

    let bare = ws.join("proj.git");
    StdCommand::new("git")
        .args(["clone", "-q", "--bare"])
        .arg(&fx.remote)
        .arg(&bare)
        .output()
        .unwrap();
    let wt = ws.join("feat-x");
    run(
        &bare,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat/x",
            wt.to_str().unwrap(),
        ],
    );
    run(&wt, &["config", "user.name", "t"]);
    run(&wt, &["config", "user.email", "t@t"]);
    let head = run(&wt, &["rev-parse", "HEAD"]);

    let result = remove_dir_worktree(&wt, &top_of(&wt), &nobody, &merged(head), &[]).expect("删除");
    assert!(result.removed && result.branch_deleted);
    assert!(!wt.exists());
    assert!(
        run(&bare, &["branch", "--list", "feat/x"]).is_empty(),
        "bare 仓库里的分支应删"
    );
    assert!(
        !run(&ws, &["branch", "--list", "feat/x"]).is_empty(),
        "旁边仓库的同名分支不能动"
    );
    assert!(
        !run(&bare, &["worktree", "list"]).contains("feat-x"),
        "不留 worktree 元数据"
    );
}

#[test]
fn 锁定的_worktree_不可清理() {
    let fx = fixture("locked");
    run(&fx.main, &["worktree", "lock", fx.wt.to_str().unwrap()]);
    assert_eq!(reason_of(&fx).as_deref(), Some("locked"));
}

// ═══ 第二轮做空验证复现的场景 ═══

#[test]
fn 子模块侧分支上的本地提交_不可清理() {
    let fx = fixture("subside");
    let sub = add_submodule(&fx, "libs");
    run(&sub, &["checkout", "-q", "-b", "side"]);
    std::fs::write(sub.join("side.txt"), "x\n").unwrap();
    run(&sub, &["add", "."]);
    run(&sub, &["commit", "-qm", "side only"]);
    run(&sub, &["checkout", "-q", "main"]);
    assert_eq!(reason_of(&fx).as_deref(), Some("submodule_unpushed"));
}

#[test]
fn 子模块里有_stash_不可清理() {
    let fx = fixture("substash");
    let sub = add_submodule(&fx, "libs");
    std::fs::write(sub.join("s.txt"), "changed\n").unwrap();
    run(&sub, &["stash", "-q"]);
    assert_eq!(reason_of(&fx).as_deref(), Some("submodule_unpushed"));
}

#[test]
fn 可清理时列出被忽略的本地文件_构建产物不列() {
    let fx = fixture("ignored");
    let exclude = fx.main.join(".git").join("info").join("exclude");
    std::fs::write(&exclude, ".lead/\n.env\ntarget/\n").unwrap();
    std::fs::create_dir_all(fx.wt.join(".lead")).unwrap();
    std::fs::write(fx.wt.join(".lead").join("product.md"), "facts\n").unwrap();
    std::fs::write(fx.wt.join(".env"), "KEY=1\n").unwrap();
    std::fs::create_dir_all(fx.wt.join("target")).unwrap();
    std::fs::write(fx.wt.join("target").join("x.o"), "o").unwrap();
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    let status = status_for_dir(&fx.wt, &nobody, &merged(head));
    assert!(status.eligible, "{status:?}");
    assert!(
        status.ignored.iter().any(|p| p == ".env"),
        "{:?}",
        status.ignored
    );
    assert!(
        status.ignored.iter().any(|p| p.starts_with(".lead")),
        "{:?}",
        status.ignored
    );
    assert!(!status
        .ignored
        .iter()
        .any(|p| p.contains("target") || p.contains("node_modules")));
}

#[test]
fn 确认时看到的目录和此刻解析出的不一致时拒绝() {
    let fx = fixture("target");
    let head = run(&fx.wt, &["rev-parse", "HEAD"]);
    let other = fx.main.to_string_lossy().into_owned();
    let err = remove_dir_worktree(&fx.wt, &other, &nobody, &merged(head), &[]).unwrap_err();
    assert_eq!(err, "worktree/target_changed");
    assert!(fx.wt.join("a.txt").exists());
    assert!(fx.wt.join("node_modules").exists());
}
