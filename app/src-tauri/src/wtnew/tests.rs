use super::*;
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};

// ═══ 临时仓库 ═══

struct Sandbox {
    root: PathBuf,
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = crate::worktree::unlink_links(&self.root);
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

fn ok(dir: &Path, args: &[&str]) -> bool {
    StdCommand::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// bare 远端 + 主检出 `app`（已推 main）。主检出里放好被忽略的 `.env*`、node_modules 和
/// 一个没被忽略的未跟踪文件。
struct Fx {
    sandbox: Sandbox,
    main: PathBuf,
}

impl Fx {
    fn root(&self) -> &Path {
        &self.sandbox.root
    }
    fn opts(&self) -> Options {
        Options {
            location: Location::Sibling,
            branch_prefix: "meowo/".into(),
            shells: setup::detect_shells(),
            log_dir: self.root().join("logs"),
            extra_env: Vec::new(),
        }
    }
    fn config(&self, json: &str) {
        std::fs::create_dir_all(self.main.join(".meowo")).unwrap();
        std::fs::write(self.main.join(".meowo").join("worktree.json"), json).unwrap();
    }
}

fn fixture(tag: &str) -> Fx {
    let root = std::env::temp_dir().join(format!(
        "meowo-wtnew-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root = dunce::canonicalize(&root).unwrap();
    let sandbox = Sandbox { root: root.clone() };
    let remote = root.join("remote.git");
    let main = root.join("app");
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
    std::fs::write(
        main.join(".gitignore"),
        ".env*\n!.env.example\nnode_modules/\nbuild/\n",
    )
    .unwrap();
    std::fs::write(main.join(".env.example"), "KEY=\n").unwrap();
    std::fs::write(main.join("a.txt"), "a\n").unwrap();
    run(&main, &["add", "."]);
    run(&main, &["commit", "-qm", "init"]);
    run(
        &main,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    run(&main, &["push", "-q", "-u", "origin", "main"]);
    // 被忽略、应当复制的
    std::fs::write(main.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(main.join(".env.local"), "LOCAL=1\n").unwrap();
    std::fs::create_dir_all(main.join("apps/web")).unwrap();
    std::fs::write(main.join("apps/web/.env.local"), "WEB=1\n").unwrap();
    // 被忽略、不该复制的（依赖目录，交给 setup）
    std::fs::create_dir_all(main.join("node_modules/pkg")).unwrap();
    std::fs::write(main.join("node_modules/pkg/.env"), "X=1\n").unwrap();
    std::fs::write(main.join("node_modules/pkg/index.js"), "x\n").unwrap();
    // 未跟踪但没被忽略的半成品：不该被带进新分支
    std::fs::write(main.join("notes.txt"), "draft\n").unwrap();
    Fx { sandbox, main }
}

fn always(_: &str, _: &str) -> bool {
    true
}
fn never(_: &str, _: &str) -> bool {
    false
}
fn quiet() -> Arc<dyn Fn(&str) + Send + Sync> {
    Arc::new(|_| {})
}

fn req(slug: Option<&str>, branch: Option<&str>) -> Request {
    Request {
        slug: slug.map(str::to_string),
        branch: branch.map(str::to_string),
        fallback_slug: Some("auto".into()),
    }
}

/// 等 setup 离开 running（最多 60s：Windows 上 PowerShell 冷启动要一两秒）。
fn wait_done(task_id: &str) -> WorktreeSetupDto {
    let task = setup::find(Some(task_id), None).expect("任务已登记");
    let start = Instant::now();
    loop {
        let dto = setup::dto(&task);
        if dto.state != "running" || start.elapsed() > Duration::from_secs(60) {
            return dto;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn wait_running(task_id: &str) {
    let task = setup::find(Some(task_id), None).unwrap();
    let start = Instant::now();
    while task.inner.lock().unwrap().pid.is_none() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

// ═══ #16 建分支、复制、setup、会话 cwd ═══

#[test]
fn 建分支_复制被忽略的_env_跑_setup_会话目录就是新_worktree() {
    let fx = fixture("happy");
    fx.config(r#"{ "version": 1, "copy": [".env*", "apps/*/.env.local", "!.env.example"], "setup": "echo hi > setup-ran.txt" }"#);
    let dto = create(
        &fx.main,
        &req(Some("Feat A"), None),
        &fx.opts(),
        &always,
        quiet(),
    )
    .unwrap();
    let wt = PathBuf::from(&dto.path);
    assert_eq!(
        wt,
        fx.root().join("app-feat-a"),
        "默认和主检出并排 <仓库名>-<用途>"
    );
    assert_eq!(dto.branch, "meowo/feat-a");
    assert_eq!(dto.base, "origin/main");
    assert_eq!(
        run(&wt, &["symbolic-ref", "--short", "HEAD"]),
        "meowo/feat-a"
    );
    // 不设上游：否则推送和领先落后都对着 origin/main 算。
    assert!(
        !ok(&wt, &["config", "branch.meowo/feat-a.merge"]),
        "新分支不该跟踪 origin/main"
    );
    // 复制：被忽略的 .env* 进来了，未忽略的 notes.txt、node_modules 都没进来。
    assert_eq!(
        std::fs::read_to_string(wt.join(".env")).unwrap(),
        "SECRET=1\n"
    );
    assert!(wt.join(".env.local").is_file());
    assert!(wt.join("apps/web/.env.local").is_file());
    assert!(wt.join(".env.example").is_file(), "被跟踪的文件本来就在");
    assert!(!wt.join("notes.txt").exists(), "未跟踪又没被忽略的不复制");
    assert!(
        !wt.join("node_modules").exists(),
        "node_modules 交给 setup，不复制"
    );
    assert_eq!(dto.copied, 3);
    // setup 跑完，日志不在 worktree 里。
    let done = wait_done(&dto.task_id);
    assert_eq!(done.state, "ok", "日志：{}", done.log_tail);
    assert!(
        wt.join("setup-ran.txt").is_file(),
        "setup 在新 worktree 里跑"
    );
    let log = PathBuf::from(done.log_path.unwrap());
    assert!(
        log.starts_with(fx.root().join("logs")),
        "日志写在日志目录，不写进 worktree"
    );
    // 会话 cwd：new_session 收到的就是这个目录，校验原样通过。
    assert_eq!(
        crate::terminal::validate_new_session_cwd(&dto.path).unwrap(),
        dto.path
    );
    assert_eq!(PathBuf::from(&dto.origin_root), fx.main);
    assert_eq!(origin::origin_root_of(&wt), Some(fx.main.clone()));
}

#[test]
fn 没有配置时默认只复制根目录的_env() {
    let fx = fixture("default");
    let dto = create(&fx.main, &req(None, None), &fx.opts(), &always, quiet()).unwrap();
    let wt = PathBuf::from(&dto.path);
    assert_eq!(wt, fx.root().join("app-auto"), "没填用途用前端给的兜底名");
    assert_eq!(dto.branch, "meowo/auto");
    assert!(wt.join(".env").is_file() && wt.join(".env.local").is_file());
    assert!(
        !wt.join("apps/web/.env.local").exists(),
        "默认规则只认根目录"
    );
    assert_eq!(dto.setup.state, "skipped");
    assert_eq!(dto.setup.reason.as_deref(), Some("none"));
}

#[test]
fn 大量被忽略文件时建立不超过两秒() {
    let fx = fixture("perf");
    // 5 万个被忽略的文件（build/ 是整目录忽略）。--directory 把它收成一行，不逐个列。
    for d in 0..500 {
        let dir = fx.main.join("build").join(format!("d{d}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..100 {
            std::fs::write(dir.join(format!("f{f}.o")), b"x").unwrap();
        }
    }
    let start = Instant::now();
    let dto = create(
        &fx.main,
        &req(Some("perf"), None),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap();
    let took = start.elapsed();
    eprintln!("建立耗时 {took:?}（5 万个被忽略文件，不含 setup）");
    assert!(took <= Duration::from_secs(2), "建立耗时 {took:?} 超过 2s");
    assert!(PathBuf::from(&dto.path).join(".env").is_file());
    assert!(!PathBuf::from(&dto.path).join("build").exists());
}

// ═══ #17 setup 失败 / 超时 / 跳过 / 重跑 / 信任 ═══

#[test]
fn setup_失败时可看日志并重跑成功() {
    let fx = fixture("fail");
    fx.config(r#"{ "setup": "echo boom-marker\nexit 3" }"#);
    let dto = create(
        &fx.main,
        &req(Some("f"), None),
        &fx.opts(),
        &always,
        quiet(),
    )
    .unwrap();
    let done = wait_done(&dto.task_id);
    assert_eq!(done.state, "failed");
    assert_eq!(done.exit_code, Some(3));
    assert!(
        done.log_tail.contains("boom-marker"),
        "日志尾巴里有脚本输出：{}",
        done.log_tail
    );
    // 会话照常能开：目录在、校验过。
    assert!(crate::terminal::validate_new_session_cwd(&dto.path).is_ok());
    // 用户修好脚本后重跑。
    fx.config(r#"{ "setup": "echo fixed > fixed.txt" }"#);
    let task = setup::find(Some(&dto.task_id), None).unwrap();
    let after = setup_action(&task, "rerun", &setup::detect_shells(), &always, quiet()).unwrap();
    assert_eq!(after.state, "running");
    let done = wait_done(&dto.task_id);
    assert_eq!(done.state, "ok", "{}", done.log_tail);
    assert!(PathBuf::from(&dto.path).join("fixed.txt").is_file());
    // 按路径也找得到（会话页只知道会话 cwd，可能是 worktree 的子目录）。
    std::fs::create_dir_all(PathBuf::from(&dto.path).join("sub")).unwrap();
    let by_path = setup::find(None, Some(&format!("{}/sub", dto.path))).unwrap();
    assert_eq!(by_path.id, dto.task_id);
}

#[test]
fn setup_超时会结束进程() {
    let fx = fixture("timeout");
    fx.config(r#"{ "timeoutSec": 1, "setup": { "windows": "Start-Sleep -Seconds 30", "unix": "sleep 30" } }"#);
    let mut opts = fx.opts();
    // 超时测的是计时器，不依赖 pwsh：5.1 也认 Start-Sleep。
    opts.shells.git_bash = None;
    let dto = create(&fx.main, &req(Some("t"), None), &opts, &always, quiet()).unwrap();
    let start = Instant::now();
    let done = wait_done(&dto.task_id);
    assert_eq!(done.state, "timeout", "{}", done.log_tail);
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "超时后应该很快收场"
    );
}

#[test]
fn setup_跑着时可停止_可跳过() {
    let fx = fixture("halt");
    fx.config(r#"{ "setup": { "windows": "Start-Sleep -Seconds 60", "unix": "sleep 60" } }"#);
    let dto = create(
        &fx.main,
        &req(Some("h"), None),
        &fx.opts(),
        &always,
        quiet(),
    )
    .unwrap();
    wait_running(&dto.task_id);
    let task = setup::find(Some(&dto.task_id), None).unwrap();
    let pid = task.inner.lock().unwrap().pid.unwrap();
    let stopped = setup_action(&task, "stop", &setup::Shells::default(), &always, quiet()).unwrap();
    assert_eq!(stopped.state, "stopped");
    // 后台线程收尸后也不会把状态改回 failed。
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(setup::dto(&task).state, "stopped");
    assert!(
        !crate::proc::parent_map().contains_key(&pid),
        "shell 进程已结束"
    );
    // 重跑再跳过。
    setup_action(&task, "rerun", &setup::detect_shells(), &always, quiet()).unwrap();
    wait_running(&dto.task_id);
    let skipped = setup_action(&task, "skip", &setup::Shells::default(), &always, quiet()).unwrap();
    assert_eq!(skipped.state, "skipped");
    assert_eq!(skipped.reason.as_deref(), Some("user"));
    assert!(setup_action(&task, "bogus", &setup::Shells::default(), &always, quiet()).is_err());
}

#[test]
fn 没信任过的脚本先等确认_hash_对得上才跑() {
    let fx = fixture("trust");
    let script = "echo trusted > trusted.txt";
    fx.config(&format!(r#"{{ "setup": "{script}" }}"#));
    let dto = create(
        &fx.main,
        &req(Some("tr"), None),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap();
    assert_eq!(dto.setup.state, "needs_trust");
    assert_eq!(
        dto.setup.script.as_deref(),
        Some(script),
        "确认框展示脚本原文"
    );
    assert_eq!(dto.setup.config_source.as_deref(), Some("meowo"));
    let hash = dto.setup.config_hash.clone().unwrap();
    let wt = PathBuf::from(&dto.path);
    assert!(!wt.join("trusted.txt").exists(), "没确认前不跑");
    let task = setup::find(None, Some(&dto.path)).unwrap();
    let remembered = Mutex::new(Vec::<(String, String)>::new());
    let remember = |k: &str, h: &str| {
        remembered
            .lock()
            .unwrap()
            .push((k.to_string(), h.to_string()));
        Ok(())
    };
    // 确认框开着时配置被改了：拒绝，让用户看新的。
    assert_eq!(
        trust_and_run(&task, "stale", &fx.opts().shells, &remember, quiet()).unwrap_err(),
        "worktree/trust_stale"
    );
    let running = trust_and_run(&task, &hash, &fx.opts().shells, &remember, quiet()).unwrap();
    assert_eq!(running.state, "running");
    assert_eq!(wait_done(&dto.task_id).state, "ok");
    assert!(wt.join("trusted.txt").is_file());
    let rows = remembered.lock().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, hash);
    assert_eq!(
        rows[0].0,
        crate::worktree::norm(&fx.main.join(".git")),
        "信任按仓库记，所有 worktree 共用"
    );
    // 没跑过的任务也能跳过。
    let fx2 = fixture("trust-skip");
    fx2.config(r#"{ "setup": "echo x" }"#);
    let dto2 = create(
        &fx2.main,
        &req(Some("s"), None),
        &fx2.opts(),
        &never,
        quiet(),
    )
    .unwrap();
    let task2 = setup::find(Some(&dto2.task_id), None).unwrap();
    assert_eq!(
        setup_action(&task2, "skip", &setup::Shells::default(), &never, quiet())
            .unwrap()
            .state,
        "skipped"
    );
}

#[test]
fn 配置版本过高只复制不跑脚本() {
    let fx = fixture("ver");
    fx.config(r#"{ "version": 2, "setup": "echo no > ran.txt", "somethingNew": true }"#);
    let dto = create(
        &fx.main,
        &req(Some("v"), None),
        &fx.opts(),
        &always,
        quiet(),
    )
    .unwrap();
    assert_eq!(dto.setup.state, "skipped");
    assert_eq!(dto.setup.reason.as_deref(), Some("worktree/config_version"));
    let wt = PathBuf::from(&dto.path);
    assert!(wt.join(".env").is_file(), "照样复制");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!wt.join("ran.txt").exists());
}

#[test]
fn 配置读不懂时标失败_会话照开() {
    let fx = fixture("bad");
    fx.config("{ not json");
    let dto = create(
        &fx.main,
        &req(Some("b"), None),
        &fx.opts(),
        &always,
        quiet(),
    )
    .unwrap();
    assert_eq!(dto.setup.state, "failed");
    assert_eq!(dto.setup.reason.as_deref(), Some("worktree/config_invalid"));
    assert!(PathBuf::from(&dto.path).join(".env").is_file());
}

#[test]
fn 兼容_cursor_与_conductor_配置_只读不写() {
    let fx = fixture("cursor");
    std::fs::create_dir_all(fx.main.join(".cursor")).unwrap();
    let cursor = r#"{ "setup-worktree-windows": ["echo win"], "setup-worktree-unix": ["echo unix"], "setup-worktree": ["echo any"] }"#;
    std::fs::write(fx.main.join(".cursor/worktrees.json"), cursor).unwrap();
    let cfg = config::load(&fx.main);
    assert_eq!(cfg.source, Some(config::Source::Cursor));
    assert_eq!(
        cfg.setup.as_ref().unwrap().resolve(true).as_deref(),
        Some("echo win")
    );
    assert_eq!(
        cfg.setup.as_ref().unwrap().resolve(false).as_deref(),
        Some("echo unix")
    );
    let dto = create(&fx.main, &req(Some("c"), None), &fx.opts(), &never, quiet()).unwrap();
    assert_eq!(dto.setup.config_source.as_deref(), Some("cursor"));
    assert_eq!(
        std::fs::read_to_string(fx.main.join(".cursor/worktrees.json")).unwrap(),
        cursor,
        "只读"
    );
    assert!(!fx.main.join(".meowo").exists(), "不替用户写 meowo 配置");

    let fx = fixture("conductor");
    let conductor = r#"{ "scripts": { "setup": "bun install", "archive": "echo bye" } }"#;
    std::fs::write(fx.main.join("conductor.json"), conductor).unwrap();
    let cfg = config::load(&fx.main);
    assert_eq!(cfg.source, Some(config::Source::Conductor));
    assert_eq!(
        cfg.setup.unwrap().resolve(true).as_deref(),
        Some("bun install")
    );
    assert_eq!(
        cfg.archive.unwrap().resolve(false).as_deref(),
        Some("echo bye")
    );
    assert_eq!(
        std::fs::read_to_string(fx.main.join("conductor.json")).unwrap(),
        conductor
    );
    // meowo 配置在时优先，cursor 那份不读。
    fx.config(r#"{ "setup": "echo meowo" }"#);
    assert_eq!(config::load(&fx.main).source, Some(config::Source::Meowo));
    // .worktreeinclude 并进复制规则。
    std::fs::write(fx.main.join(".worktreeinclude"), "# 注释\nlocal.db\n").unwrap();
    assert!(config::load(&fx.main)
        .copy
        .contains(&"local.db".to_string()));
}

// ═══ #17 目录已存在、分支重名、已检出、add 失败回滚、幂等、中途取消 ═══

#[test]
fn 目录已存在_自动名加后缀_用户名报错带建议名() {
    let fx = fixture("dir");
    std::fs::create_dir_all(fx.root().join("app-auto")).unwrap();
    std::fs::create_dir_all(fx.root().join("app-mine")).unwrap();
    let auto = create(&fx.main, &req(None, None), &fx.opts(), &never, quiet()).unwrap();
    assert_eq!(
        PathBuf::from(&auto.path),
        fx.root().join("app-auto-2"),
        "目录和分支一起加后缀"
    );
    assert_eq!(auto.branch, "meowo/auto-2");
    let err = create(
        &fx.main,
        &req(Some("mine"), None),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap_err();
    assert_eq!(err, "worktree/dir_exists: mine-2");
    assert!(
        !ok(
            &fx.main,
            &["rev-parse", "--verify", "--quiet", "refs/heads/meowo/mine"]
        ),
        "报错时什么都没建"
    );
    // 预览同样标出来，不报错。
    let p = preview(&fx.main, &req(Some("mine"), None), &fx.opts()).unwrap();
    assert!(p.dir_exists && !p.branch_exists);
}

#[test]
fn 分支重名_用户名报错带建议名() {
    let fx = fixture("branch");
    run(&fx.main, &["branch", "meowo/y"]);
    run(&fx.main, &["branch", "feature/z"]);
    assert_eq!(
        create(&fx.main, &req(Some("y"), None), &fx.opts(), &never, quiet()).unwrap_err(),
        "worktree/branch_exists: meowo/y-2"
    );
    assert_eq!(
        create(
            &fx.main,
            &req(Some("z"), Some("feature/z")),
            &fx.opts(),
            &never,
            quiet()
        )
        .unwrap_err(),
        "worktree/branch_exists: feature/z-2"
    );
    assert_eq!(
        create(
            &fx.main,
            &req(None, Some("bad..name")),
            &fx.opts(),
            &never,
            quiet()
        )
        .unwrap_err(),
        "worktree/branch_invalid"
    );
    // 自动名撞了已有分支：跳到下一个。
    run(&fx.main, &["branch", "meowo/auto"]);
    let auto = create(&fx.main, &req(None, None), &fx.opts(), &never, quiet()).unwrap();
    assert_eq!(auto.branch, "meowo/auto-2");
}

#[test]
fn 分支已在别处检出时报错() {
    let fx = fixture("checkedout");
    let err = create(
        &fx.main,
        &req(Some("m"), Some("main")),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap_err();
    assert!(err.starts_with("worktree/branch_checked_out: "), "{err}");
    assert!(
        preview(&fx.main, &req(Some("m"), Some("main")), &fx.opts())
            .unwrap()
            .branch_checked_out
    );
}

#[test]
fn add_失败时删掉它建出的孤儿分支() {
    let fx = fixture("rollback");
    // 上级路径是个文件：git 先建了分支，再建目录时失败（实测会留孤儿分支）。
    let blocker = fx.root().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let mut opts = fx.opts();
    opts.location = Location::Under(blocker);
    let err = create(&fx.main, &req(Some("r"), None), &opts, &never, quiet()).unwrap_err();
    assert!(err.starts_with("worktree/add_failed"), "{err}");
    assert!(
        !ok(
            &fx.main,
            &["rev-parse", "--verify", "--quiet", "refs/heads/meowo/r"]
        ),
        "孤儿分支已删"
    );
}

#[test]
fn 同一_request_id_只建一次() {
    let fx = fixture("idem");
    let opts = fx.opts();
    let id = format!("req-{}", std::process::id());
    let calls = std::sync::atomic::AtomicU32::new(0);
    let make = || {
        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        create(&fx.main, &req(None, None), &opts, &never, quiet())
    };
    let (a, b) = std::thread::scope(|s| {
        let h1 = s.spawn(|| idempotent(&id, make));
        let h2 = s.spawn(|| idempotent(&id, make));
        (h1.join().unwrap().unwrap(), h2.join().unwrap().unwrap())
    });
    assert_eq!(a.task_id, b.task_id);
    assert_eq!(a.path, b.path);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    let again = idempotent(&id, make).unwrap();
    assert_eq!(again.path, a.path);
    let worktrees = run(&fx.main, &["worktree", "list", "--porcelain"]);
    assert_eq!(
        worktrees
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count(),
        2,
        "主检出 + 一个"
    );
    // 失败不记：换个名字重试要真的再建。
    let bad_id = format!("bad-{}", std::process::id());
    assert!(idempotent(&bad_id, || Err("worktree/not_repo".into())).is_err());
    assert!(idempotent(&bad_id, make).is_ok());
}

#[test]
fn 中途取消后保留的干净_worktree_没装_gh_也能直接清理() {
    let fx = fixture("cancel");
    let dto = create(&fx.main, &req(Some("c"), None), &fx.opts(), &never, quiet()).unwrap();
    let status = crate::worktree::status_for_dir(Path::new(&dto.path), &|_| 0, &|_| {
        crate::worktree::PrLookup::Unavailable
    });
    assert!(
        status.eligible,
        "新建没提交的 worktree 本地判定为已合并：{:?}",
        status.reason
    );
}

#[test]
fn 选中的目录本身是_worktree_时新的那个仍和主检出并排() {
    let fx = fixture("nested");
    let first = create(
        &fx.main,
        &req(Some("one"), None),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap();
    let second = create(
        Path::new(&first.path),
        &req(Some("two"), None),
        &fx.opts(),
        &never,
        quiet(),
    )
    .unwrap();
    assert_eq!(PathBuf::from(&second.path), fx.root().join("app-two"));
    assert_eq!(PathBuf::from(&second.origin_root), fx.main);
}

#[test]
fn 不在仓库里报_not_repo() {
    let fx = fixture("norepo");
    let plain = fx.root().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    // 测试机的临时目录不在任何仓库里（CI 与本机都是）。
    if repo_of(&plain).is_ok() {
        return;
    }
    assert_eq!(
        create(&plain, &req(None, None), &fx.opts(), &never, quiet()).unwrap_err(),
        "worktree/not_repo"
    );
}

// ═══ 命名（纯函数）═══

#[test]
fn 用途名转_kebab_小写() {
    assert_eq!(kebab("  Fix Login/Bug!! "), "fix-login-bug");
    assert_eq!(kebab("修复 登录"), "修复-登录");
    assert_eq!(kebab("---"), "");
    assert_eq!(utc_slug(0), "0101-0000");
    // 2026-11-10 09:30 UTC
    assert_eq!(utc_slug(1_794_303_000_000), "1110-0930");
}

// ═══ 选 shell（纯函数）═══

#[test]
fn windows_下_pwsh_优先_51_遇到链式运算符走_git_bash() {
    let p = |s: &str| Some(PathBuf::from(s));
    let all = setup::Shells {
        pwsh: p("pwsh.exe"),
        win_powershell: p("powershell.exe"),
        git_bash: p("bash.exe"),
        login_shell: None,
    };
    assert_eq!(
        setup::choose_runner(true, "bun i && bun run x", &all)
            .unwrap()
            .name(),
        "pwsh"
    );
    let no7 = setup::Shells {
        pwsh: None,
        ..all.clone()
    };
    assert_eq!(
        setup::choose_runner(true, "bun i && bun run x", &no7)
            .unwrap()
            .name(),
        "bash"
    );
    assert_eq!(
        setup::choose_runner(true, "a || b", &no7).unwrap().name(),
        "bash"
    );
    assert_eq!(
        setup::choose_runner(true, "bun install", &no7)
            .unwrap()
            .name(),
        "powershell"
    );
    let only51 = setup::Shells {
        pwsh: None,
        git_bash: None,
        ..all.clone()
    };
    assert_eq!(
        setup::choose_runner(true, "bun i && x", &only51).unwrap_err(),
        "worktree/setup_shell_unsupported"
    );
    assert_eq!(
        setup::choose_runner(true, "bun i", &only51).unwrap().name(),
        "powershell"
    );
    let bash_only = setup::Shells {
        git_bash: p("bash.exe"),
        ..Default::default()
    };
    assert_eq!(
        setup::choose_runner(true, "bun i", &bash_only)
            .unwrap()
            .name(),
        "bash"
    );
    assert_eq!(
        setup::choose_runner(true, "x", &setup::Shells::default()).unwrap_err(),
        "worktree/setup_shell_unsupported"
    );
    // Unix：$SHELL，没有就 /bin/sh。
    let zsh = setup::Shells {
        login_shell: p("/bin/zsh"),
        ..Default::default()
    };
    assert_eq!(
        setup::choose_runner(false, "x && y", &zsh).unwrap(),
        setup::Runner::Bash(PathBuf::from("/bin/zsh"))
    );
    assert_eq!(
        setup::choose_runner(false, "x", &setup::Shells::default()).unwrap(),
        setup::Runner::Sh(PathBuf::from("/bin/sh"))
    );
    // 参数：PowerShell 不读 profile、不交互、错误即停；bash 登录 shell + set -e。
    let args = setup::runner_args(&setup::Runner::Pwsh(PathBuf::from("pwsh")), "bun i");
    assert_eq!(&args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
    assert!(args[3].starts_with("$ErrorActionPreference = 'Stop'") && args[3].ends_with("bun i"));
    assert_eq!(
        setup::runner_args(&setup::Runner::Bash(PathBuf::from("bash")), "x")[0],
        "-lc"
    );
}

// ═══ #18 originRoot 推算 ═══

#[test]
fn 原项目推算_主检出_linked_worktree_子目录_非_git() {
    let fx = fixture("origin");
    assert_eq!(
        origin::origin_root_of(&fx.main),
        None,
        "主检出自己不算 worktree"
    );
    std::fs::create_dir_all(fx.main.join("apps/web")).unwrap();
    assert_eq!(origin::origin_root_of(&fx.main.join("apps/web")), None);
    let wt = fx.root().join("app-wt");
    run(
        &fx.main,
        &["worktree", "add", "-q", "-b", "w", wt.to_str().unwrap()],
    );
    assert_eq!(origin::origin_root_of(&wt), Some(fx.main.clone()));
    std::fs::create_dir_all(wt.join("deep/er")).unwrap();
    assert_eq!(
        origin::origin_root_of(&wt.join("deep/er")),
        Some(fx.main.clone()),
        "worktree 的子目录"
    );
    // git 写的 commondir 是相对路径（"../.."）。
    let gitdir = fx.main.join(".git/worktrees/app-wt");
    assert_eq!(
        std::fs::read_to_string(gitdir.join("commondir"))
            .unwrap()
            .trim(),
        "../.."
    );
    let plain = fx.root().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(origin::origin_root_of(&plain), None, "不在 git 里");
}

#[test]
fn 原项目推算_相对_gitdir_绝对_commondir_子模块_bare() {
    let fx = fixture("origin2");
    // 手写一个 worktree：.git 文件里是相对 gitdir（git 2.48+ worktree.useRelativePaths 就这么写），
    // commondir 写成绝对路径。
    let wt = fx.root().join("hand-wt");
    let meta = fx.main.join(".git/worktrees/hand");
    std::fs::create_dir_all(&meta).unwrap();
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: ../app/.git/worktrees/hand\n").unwrap();
    std::fs::write(
        meta.join("commondir"),
        format!("{}\n", fx.main.join(".git").display()),
    )
    .unwrap();
    assert_eq!(origin::origin_root_of(&wt), Some(fx.main.clone()));
    // 子模块：.git 文件指向 .git/modules/sub，那里没有 commondir。
    let sub = fx.main.join("sub");
    let modules = fx.main.join(".git/modules/sub");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
    assert_eq!(origin::origin_root_of(&sub), None, "子模块不是 worktree");
    // bare 仓库的 worktree：公共目录不叫 .git，没有主检出。
    let bare = fx.root().join("remote.git");
    let bare_wt = fx.root().join("bare-wt");
    run(
        &bare,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "bw",
            bare_wt.to_str().unwrap(),
            "main",
        ],
    );
    assert_eq!(origin::origin_root_of(&bare_wt), None);
}
