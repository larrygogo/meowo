//! 认领与防串的回归测试（用户实拍 2026-11-18：worktree 会话的 Lead 面板显示了同项目另一个会话的任务）。
//!
//! 起因的命令形状：先 `cd` 进 `.lead`，任务名只出现在变量赋值 `D=<slug>` 里，后面全写 `$D/…`，
//! 命令里没有字面的 `.lead/<slug>`，任务一直没认领上，`.owner` 为空，项目里（含 worktree）所有会话都看得到它。

use super::*;
use serde_json::json;
use std::fs;
use std::time::{Duration, SystemTime};

struct Env {
    _tmp: tempfile::TempDir,
    d: PathBuf,
    reg: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let d = strip_verbatim(tmp.path().canonicalize().unwrap());
    let reg = d.join("sessions.json");
    Env { _tmp: tmp, d, reg }
}

/// 建一个 git 项目根目录。
fn project(e: &Env, name: &str) -> PathBuf {
    let root = e.d.join(name);
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::create_dir_all(root.join(".lead")).unwrap();
    root
}

/// 模拟 agent 刚刚写出的任务目录（文件 mtime 是现在）。
fn task(root: &Path, slug: &str, phase: &str) -> PathBuf {
    let t = root.join(".lead").join(slug);
    fs::create_dir_all(&t).unwrap();
    fs::write(t.join("SPEC.md"), "# 规格\n").unwrap();
    fs::write(t.join("progress.md"), format!("phase: {phase}\n")).unwrap();
    fs::write(
        t.join("acceptance.json"),
        r#"[{"id":1,"check":"x","passes":false}]"#,
    )
    .unwrap();
    fs::write(t.join("decisions.md"), "").unwrap();
    fs::write(root.join(".lead/active"), slug).unwrap();
    t
}

/// 把任务目录里的文件都改成一小时前写的（别的会话早先写的任务）。
fn age(t: &Path) {
    let old = SystemTime::now() - Duration::from_secs(3600);
    for entry in fs::read_dir(t).unwrap().flatten() {
        if entry.path().is_file() {
            let f = fs::OpenOptions::new()
                .write(true)
                .open(entry.path())
                .unwrap();
            f.set_modified(old).unwrap();
        }
    }
}

fn owner(t: &Path) -> String {
    fs::read_to_string(t.join(".owner")).unwrap_or_default()
}

/// 以 Claude Code PostToolUse 的载荷形状调用 claim（带 duration_ms，2.1.x 起有）。
fn post(e: &Env, sid: &str, cwd: &Path, tool: &str, input: Value) {
    let data = json!({"hook_event_name": "PostToolUse", "session_id": sid, "cwd": cwd,
        "tool_name": tool, "tool_input": input, "duration_ms": 800});
    run("claim", &data.to_string(), &e.reg);
}

fn fwd(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// `C:/x/y` → `/c/x/y`（Git Bash 写法）；非 Windows 原样返回。
fn git_bash(p: &Path) -> String {
    let s = fwd(p);
    if cfg!(windows) && s.as_bytes().get(1) == Some(&b':') {
        format!("/{}{}", s[..1].to_ascii_lowercase(), &s[2..])
    } else {
        s
    }
}

/// 起因命令的形状（去掉了业务内容）：cd 进 `.lead`，slug 只在变量赋值里。
fn real_shape(root: &Path, slug: &str) -> String {
    format!(
        "cd {}/.lead && D={slug} && mkdir -p $D && {{ [ -f active ] && cat active || echo $D > active; }} && cat > $D/SPEC.md <<'EOF'\n# 规格\nEOF\npython -I - <<'EOF'\nimport json\njson.dump([], open('$D/acceptance.json','w'))\nEOF\nprintf 'phase: implement\\n' > $D/progress.md",
        git_bash(root)
    )
}

#[test]
fn real_command_shape_with_slug_only_in_a_variable_is_claimed() {
    let e = env();
    let root = project(&e, "proj");
    let slug = "20261118-demo-dvcap";
    let t = task(&root, slug, "implement");
    // 会话的 cwd 在项目里的嵌套目录（起因那次是子仓库目录）
    let cwd = root.join("sub");
    fs::create_dir_all(&cwd).unwrap();
    post(
        &e,
        "S-main",
        &cwd,
        "Bash",
        json!({"command": real_shape(&root, slug)}),
    );
    assert_eq!(owner(&t), "S-main");
    assert_eq!(task_of_session("S-main", &e.reg), Some(t));
}

#[test]
fn powershell_shapes_are_claimed() {
    let e = env();
    let root = project(&e, "proj");
    let win = root.to_string_lossy().replace('/', "\\");
    // Set-Location + $D 变量 + Join-Path + 反斜杠
    let a = task(&root, "20261118-ps-a", "implement");
    let cmd = format!(
        "Set-Location '{win}\\.lead'; $D = '20261118-ps-a'; New-Item -ItemType Directory -Force $D | Out-Null; Set-Content -Path (Join-Path $D 'progress.md') -Value 'phase: implement' -Encoding utf8"
    );
    post(&e, "P1", &e.d, "PowerShell", json!({"command": cmd}));
    assert_eq!(owner(&a), "P1");
    // `.lead` 作为 Join-Path 的单独一段：根目录在前一个参数里
    let b = task(&root, "20261118-ps-b", "spec");
    age(&a);
    let cmd = format!(
        "$L = Join-Path \"{win}\" '.lead'; $T = Join-Path $L 20261118-ps-b; Add-Content -Path \"$T\\progress.md\" -Value '- x'"
    );
    post(&e, "P2", &e.d, "PowerShell", json!({"command": cmd}));
    assert_eq!(owner(&b), "P2");
    assert_eq!(owner(&a), "P1", "没被写的任务不受影响");
}

#[test]
fn lead_reporter_commands_claim_the_task() {
    let e = env();
    let root = project(&e, "proj");
    let t = task(&root, "20261118-gate", "implement");
    let win = t.to_string_lossy().replace('/', "\\");
    let cmd = format!("& \"C:\\Users\\u\\AppData\\Local\\Meowo\\meowo-reporter.exe\" lead initialize --task \"{win}\"");
    post(&e, "R1", &e.d, "PowerShell", json!({"command": cmd}));
    assert_eq!(owner(&t), "R1");
    let cmd = format!(
        "meowo-reporter lead publish --task {} --version none",
        git_bash(&t)
    );
    post(&e, "R2", &e.d, "Bash", json!({"command": cmd}));
    assert_eq!(owner(&t), "R2");
    // status 只读，不认领
    let cmd = format!("meowo-reporter lead status --task {}", git_bash(&t));
    post(&e, "R3", &e.d, "Bash", json!({"command": cmd}));
    assert_eq!(owner(&t), "R2");
}

/// 子代理（载荷带 agent_id）写任务文件：session_id 是主会话的，任务照样认领给主会话。
#[test]
fn subagent_writes_claim_for_the_main_session() {
    let e = env();
    let root = project(&e, "proj");
    let t = task(&root, "20261118-sub", "implement");
    let data = json!({"hook_event_name": "PostToolUse", "session_id": "MAIN", "agent_id": "a1b2c3",
        "agent_type": "general-purpose", "cwd": root, "tool_name": "Bash", "duration_ms": 50,
        "tool_input": {"command": format!("cd {}/.lead/20261118-sub && echo '- x' >> progress.md", git_bash(&root))}});
    run("claim", &data.to_string(), &e.reg);
    assert_eq!(owner(&t), "MAIN");
    // 走显式 `--provider claude` 的入口也一样
    let t2 = task(&root, "20261118-sub2", "implement");
    let data = json!({"hook_event_name": "PostToolUse", "session_id": "MAIN", "agent_id": "a1b2c3",
        "cwd": root, "tool_name": "Write", "tool_input": {"file_path": t2.join("progress.md")}});
    crate::lead_provider::run("claude", "claim", &data.to_string(), &e.reg);
    assert_eq!(owner(&t2), "MAIN");
}

/// 两个会话同时各写各的新任务：各认各的，互不抢；只靠 mtime 又分不清是谁的，就谁都不认。
#[test]
fn concurrent_sessions_each_claim_their_own_task() {
    let e = env();
    let root = project(&e, "proj");
    let a = task(&root, "20261118-aaa", "implement");
    let b = task(&root, "20261118-bbb", "implement");
    post(
        &e,
        "A",
        &root,
        "Bash",
        json!({"command": real_shape(&root, "20261118-aaa")}),
    );
    post(
        &e,
        "B",
        &root,
        "Bash",
        json!({"command": real_shape(&root, "20261118-bbb")}),
    );
    assert_eq!(owner(&a), "A");
    assert_eq!(owner(&b), "B");
    // 两个无主任务都刚被写过，命令里又只有 `$D`：拿不准，谁都不认
    let c = task(&root, "20261118-ccc", "implement");
    let d = task(&root, "20261118-ddd", "implement");
    let lead = root.join(".lead");
    post(
        &e,
        "C",
        &lead,
        "Bash",
        json!({"command": "printf 'phase: verify\\n' > $D/progress.md"}),
    );
    assert_eq!(owner(&c), "");
    assert_eq!(owner(&d), "");
    // 只有一个无主任务在这次调用期间被写过：认给它；早先写的那个不动
    age(&d);
    post(
        &e,
        "C",
        &lead,
        "Bash",
        json!({"command": "printf 'phase: verify\\n' > $D/progress.md"}),
    );
    assert_eq!(owner(&c), "C");
    assert_eq!(owner(&d), "");
    // 已经有主的任务不会被 slug 词元顺手抢走：没在这次调用里写它
    age(&a);
    let cmd = format!(
        "cd {}/.lead && cat 20261118-aaa/SPEC.md > /tmp/x.md",
        git_bash(&root)
    );
    post(&e, "B", &root, "Bash", json!({"command": cmd}));
    assert_eq!(owner(&a), "A");
}

/// 都是很久以前写的：时间窗兜底不认领（别的会话的旧任务）。
#[test]
fn stale_tasks_are_not_claimed_by_the_mtime_fallback() {
    let e = env();
    let root = project(&e, "proj");
    let t = task(&root, "20261118-old", "implement");
    age(&t);
    post(
        &e,
        "X",
        &root.join(".lead"),
        "Bash",
        json!({"command": "echo x > $D/notes.txt"}),
    );
    assert_eq!(owner(&t), "");
}

fn worktree(e: &Env, main: &Path, name: &str) -> PathBuf {
    let gitdir = main.join(".git/worktrees").join(name);
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    let wt = e.d.join(format!("wt-{name}"));
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    wt
}

/// 起因现场：worktree 会话看主检出里一个已进入实施、却没认领上的任务。
#[test]
fn worktree_session_does_not_see_an_unclaimed_task_in_implement() {
    let e = env();
    let main = project(&e, "main");
    let wt = worktree(&e, &main, "cloud");
    let wt_s = wt.to_string_lossy().into_owned();
    let main_s = main.to_string_lossy().into_owned();
    let t = task(&main, "20261118-orphan", "implement");
    // 无主、已在实施：worktree 里别的会话、主检出里别的会话都看不到
    assert!(snapshot_for(&wt_s, Some("W")).is_none());
    assert!(snapshot_for(&main_s, Some("other")).is_none());
    assert!(snapshot_for(&wt_s, None).is_none());
    // 内部汇总（看板、防休眠）不过滤
    assert!(snapshot(&wt_s).is_some());
    // 真正的主人一认领就看得到，别人仍看不到
    post(
        &e,
        "OWNER",
        &main,
        "Bash",
        json!({"command": real_shape(&main, "20261118-orphan")}),
    );
    assert_eq!(owner(&t), "OWNER");
    assert_eq!(
        snapshot_for(&main_s, Some("OWNER"))
            .map(|s| s.slug)
            .as_deref(),
        Some("20261118-orphan")
    );
    assert_eq!(
        snapshot_for(&wt_s, Some("OWNER"))
            .map(|s| s.slug)
            .as_deref(),
        Some("20261118-orphan")
    );
    assert!(snapshot_for(&wt_s, Some("W")).is_none());
    // 还在访谈 / 写规格的无主任务照旧对项目里所有会话可见（包括 worktree）
    let early = task(&main, "20261118-early", "spec");
    assert_eq!(
        snapshot_for(&wt_s, Some("W")).map(|s| s.slug).as_deref(),
        Some("20261118-early")
    );
    let _ = early;
}

/// 兜底：PostToolUse 没认领上（老版本 reporter、hook 当时没装），Stop 时按本轮 transcript 里的写入认领无主的实施中任务。
#[test]
fn stop_adopts_an_orphan_written_this_turn() {
    let e = env();
    let root = project(&e, "proj");
    let t = task(&root, "20261118-late", "implement");
    let other = task(&root, "20261118-other", "implement");
    let lines = [
        json!({"type": "user", "message": {"content": "开始做"}}),
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash",
            "input": {"command": real_shape(&root, "20261118-late")}}]}}),
        json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "ok"}]}}),
    ];
    let tr = e.d.join("t.jsonl");
    fs::write(
        &tr,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let data = json!({"session_id": "LATE", "cwd": root, "transcript_path": tr});
    run("stop", &data.to_string(), &e.reg);
    assert_eq!(owner(&t), "LATE");
    assert_eq!(owner(&other), "", "本轮没写的无主任务不认领");
    // 认领后照常按 Stop 规则拦（本轮推进过、验收没过完）
    assert!(run("stop", &data.to_string(), &e.reg)
        .stdout
        .is_some_and(|s| s.contains("block")));
}

/// 用户诊断包 2026-11-24 的形状：会话工作目录是 git 仓库 `<父>/repo`，agent 把任务建在仓库外的
/// `<父>/.lead` 下。认领照样成功，面板却只读 `repo/.lead`，用户看不到面板，agent 也不知道。
/// 修复后：认领时当场回一句挪到哪；建在仓库根下的任务不提示。
#[test]
fn task_outside_the_panel_location_gets_a_move_hint() {
    let e = env();
    let parent = e.d.join("szx");
    fs::create_dir_all(parent.join(".lead")).unwrap();
    let repo = parent.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let outside = task(&parent, "2026-11-24-inbound-flow", "spec");
    let claim = |path: &Path| {
        let data = json!({"session_id":"s1","cwd":repo,"tool_name":"Write","tool_input":{"file_path":path.join("progress.md")}});
        run("claim", &data.to_string(), &e.reg).stdout
    };

    let out = claim(&outside).expect("建在仓库外要提示");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    let hint = v["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
    let want = repo.join(".lead").join("2026-11-24-inbound-flow");
    assert!(hint.contains(&want.display().to_string()), "{hint}");
    // 提示不妨碍认领
    assert_eq!(owner(&outside), "s1");

    let inside = task(&repo, "2026-11-24-szx-dev", "design");
    fs::create_dir_all(repo.join(".lead")).unwrap();
    assert_eq!(claim(&inside), None);
    // 面板确实看得到仓库根下的那个
    assert_eq!(snapshot_for(&repo.to_string_lossy(), Some("s1")).unwrap().slug, "2026-11-24-szx-dev");
}

/// 实拍 2026-11-24：会话在 Desktop 启动（库里 cwd=Desktop），AI 先 cd 进 `workspace/work-log/.lead` 再写任务，
/// hook 带的 cwd 跟着成了 work-log 仓库，任务就在它的 `.lead` 里，提醒没发；面板按 Desktop 找，看不到。
/// 判位置要用库里的会话 cwd。
#[test]
fn move_hint_uses_the_session_cwd_from_the_db_not_the_drifted_hook_cwd() {
    let e = env();
    let desktop = e.d.join("Desktop");
    let work_log = desktop.join("workspace").join("work-log");
    fs::create_dir_all(work_log.join(".git")).unwrap();
    let t = task(&work_log, "2026-11-24-work-split", "spec");

    let db = e.d.join("board.db");
    let store = meowo_store::Store::open(&db).unwrap();
    let project = store.upsert_project_by_root(&desktop.to_string_lossy(), "Desktop", 1).unwrap();
    store.import_session("s1", project, "抖音视频主要内容", Some(&desktop.to_string_lossy()), 1).unwrap();
    drop(store);

    let cwd = panel_cwd("s1", Some(&db)).expect("库里有这个会话");
    assert_eq!(cwd, desktop.to_string_lossy());
    // 按漂移后的 hook cwd 判，任务看起来就在面板位置里（这就是漏报的原因）；按库里的 cwd 判才提醒。
    assert_eq!(misplaced_hint(&t, &work_log.join(".lead").to_string_lossy()), None);
    let hint = misplaced_hint(&t, &cwd).expect("按会话 cwd 判要提醒");
    assert!(hint.contains(&desktop.join(".lead").join("2026-11-24-work-split").display().to_string()), "{hint}");
    // 库里没有这个会话、或没有库：返回 None，调用方退回 hook cwd。
    assert_eq!(panel_cwd("s2", Some(&db)), None);
    assert_eq!(panel_cwd("s1", Some(&e.d.join("missing.db"))), None);
}

/// 面板没显示任务时给诊断日志的原因：列出找的位置、各任务阶段与归属；位置下没有任务时不记。
#[test]
fn hidden_reason_lists_tasks_with_phase_and_owner() {
    let e = env();
    let root = project(&e, "p");
    let cwd = root.to_string_lossy().into_owned();
    assert_eq!(hidden_reason(&cwd, Some("s1")), None);
    let done = task(&root, "a-done", "done");
    fs::write(done.join(".owner"), "s1").unwrap();
    let other = task(&root, "b-other", "implement");
    fs::write(other.join(".owner"), "s2").unwrap();
    task(&root, "c-orphan", "implement");
    let reason = hidden_reason(&cwd, Some("s1")).unwrap();
    assert!(reason.contains(&root.join(".lead").display().to_string()), "{reason}");
    assert!(reason.contains("a-done（phase=done，本会话）"), "{reason}");
    assert!(reason.contains("b-other（phase=implement，别的会话）"), "{reason}");
    assert!(reason.contains("c-orphan（phase=implement，无主）"), "{reason}");
    assert!(reason.contains("active=c-orphan"), "{reason}");
}
