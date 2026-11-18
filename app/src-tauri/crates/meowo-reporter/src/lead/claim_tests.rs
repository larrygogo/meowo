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
