//! 场景逐条移植自初版 ~/.claude/lead/test_hook.py；R1/R2 编号对应两轮做空审查发现的问题。

use super::*;
use serde_json::json;
use std::fs;

const SLUG: &str = "20261109-demo";

struct Env {
    _tmp: tempfile::TempDir,
    d: PathBuf,
    reg: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().canonicalize().unwrap();
    let reg = d.join("sessions.json");
    Env { _tmp: tmp, d, reg }
}

fn make_with(root: &Path, phase: &str, passes: &[Value], active: &str) -> PathBuf {
    let t = root.join(".lead").join(SLUG);
    fs::create_dir_all(&t).unwrap();
    fs::write(root.join(".lead/active"), active).unwrap();
    fs::write(t.join("SPEC.md"), "# SPEC\n目的：演示").unwrap();
    fs::write(t.join("progress.md"), format!("# 进度\n\nphase: {phase}   <!-- x -->\n")).unwrap();
    let items: Vec<Value> =
        passes.iter().enumerate().map(|(i, p)| json!({"id": i + 1, "check": format!("检查{}", i + 1), "passes": p})).collect();
    fs::write(t.join("acceptance.json"), Value::Array(items).to_string()).unwrap();
    fs::write(root.join(".lead/product.md"), "- 用户会拼车").unwrap();
    t
}

fn make(root: &Path) -> PathBuf {
    make_with(root, "implement", &[json!(false), json!(true)], SLUG)
}

fn transcript(root: &Path, name: &str, user: &str, calls: &[(&str, Value)]) -> String {
    let mut lines = vec![json!({"type": "user", "message": {"content": user}})];
    for (n, input) in calls {
        lines.push(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": n, "input": input}]}}));
        lines.push(json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "ok"}]}}));
    }
    let p = root.join(format!("{name}.jsonl"));
    fs::write(&p, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n") + "\n").unwrap();
    p.to_string_lossy().into_owned()
}

fn call(e: &Env, mode: &str, data: Value) -> Option<Value> {
    run(mode, &data.to_string(), &e.reg).stdout.map(|s| serde_json::from_str(&s).unwrap())
}

fn blocked(e: &Env, data: Value) -> bool {
    call(e, "stop", data).is_some_and(|v| v["decision"] == "block")
}

fn claim_as(e: &Env, cwd: &Path, sid: &str, name: &str, input: Value) {
    call(e, "claim", json!({"cwd": cwd, "session_id": sid, "tool_name": name, "tool_input": input}));
}

fn owner(t: &Path) -> Option<String> {
    fs::read_to_string(t.join(".owner")).ok()
}

#[test]
fn scenarios() {
    let e = env();
    let root = e.d.join("proj");
    fs::create_dir_all(root.join(".git")).unwrap();
    let sub = root.join("src/deep");
    fs::create_dir_all(&sub).unwrap();

    // 没有任务：都不输出
    assert!(call(&e, "session", json!({"cwd": sub})).is_none());
    assert!(call(&e, "stop", json!({"cwd": sub, "session_id": "A"})).is_none());

    let t = make(&root);
    let prog = t.join("progress.md").to_string_lossy().into_owned();
    let edit_prog = ("Edit", json!({"file_path": prog}));

    // session：从子目录找到所在 git 根目录的任务
    let ctx = call(&e, "session", json!({"cwd": sub, "source": "compact"})).unwrap();
    let ctx = ctx["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_owned();
    assert!(ctx.contains("目的：演示") && ctx.contains("#1 检查1") && ctx.contains("用户会拼车"), "{ctx}");

    // R1-P1：上层非 git 目录的任务不注入到下面；嵌套 git 项目不继承
    let outer = e.d.join("home");
    make(&outer);
    fs::create_dir_all(outer.join("Desktop")).unwrap();
    assert!(call(&e, "session", json!({"cwd": outer.join("Desktop")})).is_none());
    let nested = root.join("nested");
    fs::create_dir_all(nested.join(".git")).unwrap();
    assert!(call(&e, "session", json!({"cwd": nested})).is_none());

    // R2-P2：只有写入才算认领
    for cmd in [
        format!("cat \"{prog}\""),
        format!("git add \"{prog}\""),
        format!("cat \"{prog}\" 2>/dev/null"),
        format!("grep phase \"{prog}\" 2>&1"),
    ] {
        claim_as(&e, &root, "X", "Bash", json!({"command": cmd}));
        assert!(owner(&t).is_none(), "{cmd}");
    }
    claim_as(&e, &root, "X", "Read", json!({"file_path": prog}));
    assert!(owner(&t).is_none());

    // 认领：Edit；R2-P3：会话在项目外用绝对路径写也能认领
    claim_as(&e, &e.d, "A", edit_prog.0, edit_prog.1.clone());
    assert_eq!(owner(&t).as_deref(), Some("A"));

    // R2-P1：本轮推进了任务 → 拦；只是回答问题 → 不拦
    let tr_work = transcript(&root, "A1", "继续做", std::slice::from_ref(&edit_prog));
    let tr_ask = transcript(&root, "A2", "README 里的项目名叫什么？", &[]);
    assert!(blocked(&e, json!({"session_id": "A", "transcript_path": tr_work, "cwd": e.d})));
    assert!(!blocked(&e, json!({"session_id": "A", "transcript_path": tr_ask, "cwd": sub})));
    let tr_read = transcript(&root, "A3", "看看进度", &[("Bash", json!({"command": format!("cat \"{prog}\"")}))]);
    assert!(!blocked(&e, json!({"session_id": "A", "transcript_path": tr_read})));
    let tr_bash = transcript(
        &root,
        "A4",
        "做下一条",
        &[("Bash", json!({"command": format!("echo x >> \"{prog}\"").replace('/', "\\")}))],
    );
    assert!(blocked(&e, json!({"session_id": "A", "transcript_path": tr_bash})));
    // 真机实测漏拦过：Bash 用相对路径写进度文件
    let tr_rel = transcript(&root, "A5", "做下一条", &[("Bash", json!({"command": format!("echo x >> .lead/{SLUG}/progress.md")}))]);
    assert!(blocked(&e, json!({"session_id": "A", "transcript_path": tr_rel})));

    // R1-P2：其他会话不受影响
    assert!(!blocked(&e, json!({"session_id": "B", "transcript_path": tr_work, "cwd": sub})));

    // R1-P3：一轮内最多连续 5 次；新一轮重新计数
    let w = json!({"session_id": "A", "transcript_path": tr_work});
    let w_same = json!({"session_id": "A", "transcript_path": tr_work, "stop_hook_active": true});
    assert!(blocked(&e, w.clone()));
    for _ in 0..4 {
        assert!(blocked(&e, w_same.clone()));
    }
    assert!(!blocked(&e, w_same.clone()));
    assert!(blocked(&e, w.clone()));

    // 新会话 D 接手：归属转移，A 不再被拦；一个任务只归一个会话
    claim_as(&e, &root, "D", "Write", json!({"file_path": prog, "content": "phase: implement"}));
    assert_eq!(owner(&t).as_deref(), Some("D"));
    let d_work = json!({"session_id": "D", "transcript_path": tr_work});
    assert!(blocked(&e, d_work.clone()));
    assert!(!blocked(&e, w.clone()));
    let reg: Value = serde_json::from_str(&fs::read_to_string(&e.reg).unwrap()).unwrap();
    // 登记表里的路径不带 Windows 的 `\\?\` 前缀
    assert_eq!(reg, json!({"D": strip_verbatim(t.canonicalize().unwrap()).to_string_lossy()}));
    assert!(!reg["D"].as_str().unwrap().starts_with(r"\\?\"));

    // R2-P3：worktree / 任意目录：stop 按会话找任务，session 也能按会话注入
    let wt = e.d.join("wt");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: x").unwrap();
    assert!(blocked(&e, json!({"session_id": "D", "transcript_path": tr_work, "cwd": wt})));
    assert!(call(&e, "session", json!({"session_id": "D", "cwd": wt, "source": "compact"})).is_some());
    assert!(call(&e, "session", json!({"session_id": "Z", "cwd": wt})).is_none());

    // R1-P4：明确叫停 → 放行；"停车场""就这样做吧"不算叫停
    for (txt, expect) in [
        ("先停一下，我看看", false),
        ("加一行然后结束，别的什么都不要做", false),
        ("please stop here", false),
        ("实现停车场功能", true),
        ("就这样做吧", true),
        ("修复 stopwatch 组件", true),
    ] {
        let tr = transcript(&root, "S", txt, std::slice::from_ref(&edit_prog));
        assert_eq!(blocked(&e, json!({"session_id": "D", "transcript_path": tr})), expect, "{txt}");
    }

    // 全部通过 / 非实施阶段 → 放行
    make_with(&root, "implement", &[json!(true), json!(true)], SLUG);
    assert!(!blocked(&e, d_work.clone()));
    for ph in ["blocked", "verify", "design", "done"] {
        make_with(&root, ph, &[json!(false)], SLUG);
        assert!(!blocked(&e, d_work.clone()), "{ph}");
    }
    make_with(&root, "done", &[json!(false)], SLUG);
    assert!(call(&e, "session", json!({"session_id": "D", "cwd": wt})).is_none());

    // R1-P6：大小写、markdown 加粗、BOM、字符串 "false"、带路径的任务名
    for line in ["Phase: Implement", "**phase**: implement"] {
        make(&root);
        fs::write(t.join("progress.md"), format!("{line}\n")).unwrap();
        assert!(blocked(&e, d_work.clone()), "{line}");
    }
    make_with(&root, "implement", &[json!("false")], SLUG);
    assert!(blocked(&e, d_work.clone()));
    make(&root);
    fs::write(root.join(".lead/active"), format!("\u{feff}{SLUG}")).unwrap();
    assert!(call(&e, "session", json!({"cwd": sub})).is_some());
    make_with(&root, "implement", &[json!(false)], "..\\..");
    assert!(call(&e, "session", json!({"cwd": sub})).is_none());

    // 坏输入不报错
    make(&root);
    fs::write(t.join("acceptance.json"), "{坏的").unwrap();
    assert!(!blocked(&e, d_work.clone()));
    assert!(!blocked(&e, json!({"session_id": "D", "transcript_path": e.d.join("不存在.jsonl")})));
    for mode in ["stop", "session", "claim"] {
        assert_eq!(run(mode, "[1,2", &e.reg), Output::default());
    }
}

#[test]
fn chinese_progress_lines_do_not_panic() {
    let e = env();
    let t = e.d.join(".lead/x");
    fs::create_dir_all(&t).unwrap();
    fs::write(t.join("progress.md"), "# 进度

阶段说明
phase: verify
").unwrap();
    assert_eq!(phase_of(&t), "verify");
}

#[test]
fn shell_write_detection() {
    for (cmd, w) in [
        ("echo a > f", true),
        ("echo a >> f", true),
        ("cat f 2>/dev/null", false),
        ("cmd 2>&1", false),
        ("cmd > /dev/null", false),
        ("Get-Item x > $null", false),
        ("sed -i 's/a/b/' f", true),
        ("echo a | tee f", true),
        ("Set-Content f a", true),
        ("mv a b", true),
        ("git add f", false),
        ("grep cp f", true), // 词边界保守：宁可多认领一次，也不漏
    ] {
        assert_eq!(shell_writes(cmd), w, "{cmd}");
    }
}

#[test]
fn task_ref_extraction() {
    assert_eq!(find_task_refs(r#"echo x >> "C:/p/.lead/a-1/progress.md""#), vec!["C:/p/.lead/a-1".to_string()]);
    assert_eq!(find_task_refs("echo x >> .lead/x/progress.md"), vec![".lead/x".to_string()]);
    // 带空格的路径在引号里
    assert_eq!(find_task_refs(r#"echo x >> "C:/a b/.lead/x/progress.md""#), vec!["C:/a b/.lead/x".to_string()]);
    assert_eq!(find_task_refs("echo x >> 'C:/a b/.lead/x/progress.md'"), vec!["C:/a b/.lead/x".to_string()]);
    // 盘符的冒号不是边界
    assert_eq!(find_task_refs("echo x >> C:/p/.lead/x/progress.md"), vec!["C:/p/.lead/x".to_string()]);
    // 先 cd 进任务目录再写（实拍：认领不上的主要原因）
    assert_eq!(find_task_refs("cd /c/w/.lead/20261109-x && cat >> progress.md"), vec!["/c/w/.lead/20261109-x".to_string()]);
    // 不是 `.lead` 目录、跳出目录的不算
    assert!(find_task_refs("/x/foo.lead/a/progress.md").is_empty());
    assert!(find_task_refs("/.lead/../progress.md").is_empty());
    // `.lead/active` 也会被列出来，由调用方按「是不是目录」筛掉
    assert_eq!(find_task_refs("echo x > .lead/active"), vec![".lead/active".to_string()]);
}

fn write_gate(t: &Path, id: &str, kind: &str, status: &str) {
    fs::write(
        t.join("gate.json"),
        json!({"v": 1, "id": id, "type": kind, "title": "选机制", "summary": "**摘要**",
               "options": [{"id": "1", "label": "方案一", "recommended": true}, {"id": "2", "label": "方案二"}],
               "status": status, "answer": null})
        .to_string(),
    )
    .unwrap();
}

fn ans(choice: &str, note: &str) -> LeadGateAnswerDto {
    LeadGateAnswerDto { choice: choice.into(), note: note.into() }
}

#[test]
fn gate_answer_is_checked_and_idempotent() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make_with(&root, "design", &[json!(false)], SLUG);
    assert_eq!(answer_gate(&t, "g1", &ans("1", "")), Err("lead/gate_missing".into()));
    write_gate(&t, "g1", "B", "pending");
    let g = read_gate(&t).unwrap();
    assert_eq!((g.kind.as_str(), g.options.len(), g.options[0].recommended), ("B", 2, true));
    assert_eq!(answer_gate(&t, "g0", &ans("1", "")), Err("lead/gate_changed".into()));
    answer_gate(&t, "g1", &ans("2", "按推荐来不行")).unwrap();
    assert_eq!(read_gate(&t).unwrap().status, "answered");
    // 手机断线重发同一答案 → 成功；换答案 → 拒绝
    answer_gate(&t, "g1", &ans("2", "按推荐来不行")).unwrap();
    assert_eq!(answer_gate(&t, "g1", &ans("1", "")), Err("lead/gate_closed".into()));
    assert!(!t.join("gate.json.tmp").exists());
    // 格式坏的关卡 / 未知类型 → 视为没有关卡
    fs::write(t.join("gate.json"), json!({"id": "x", "type": "Z", "status": "pending"}).to_string()).unwrap();
    assert!(read_gate(&t).is_none());
}

/// 实拍（2026-11-09）：写进度时先 cd 进任务目录、或写的是 SPEC.md，任务一直认领不上，`.owner` 全空。
#[test]
fn claim_by_any_write_into_the_task_dir() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make(&root);
    let task = t.to_string_lossy().replace('\\', "/");
    let bash = json!({"session_id": "s1", "cwd": root, "tool_name": "Bash",
        "tool_input": {"command": format!("cd {task} && cat >> progress.md <<'EOF'\n- x\nEOF")}});
    call(&e, "claim", bash);
    assert_eq!(owner(&t).as_deref(), Some("s1"));
    let write = json!({"session_id": "s2", "cwd": root, "tool_name": "Write",
        "tool_input": {"file_path": format!("{task}/SPEC.md"), "content": "x"}});
    call(&e, "claim", write);
    assert_eq!(owner(&t).as_deref(), Some("s2"));
    // 只读不算认领
    let read = json!({"session_id": "s3", "cwd": root, "tool_name": "Bash", "tool_input": {"command": format!("cat {task}/SPEC.md")}});
    call(&e, "claim", read);
    assert_eq!(owner(&t).as_deref(), Some("s2"));
}

/// 用户反馈（2026-11-09）：同一项目的另一个会话里也看到了别的会话的 Lead。认领了的任务只给认领它的会话。
#[test]
fn snapshot_is_only_for_the_owning_session() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make(&root);
    let cwd = root.to_str().unwrap();
    // 还没认领：谁都看得到。
    assert!(snapshot_for(cwd, Some("other")).is_some());
    fs::write(t.join(".owner"), "mine\n").unwrap();
    assert!(snapshot_for(cwd, Some("mine")).is_some());
    assert!(snapshot_for(cwd, Some("other")).is_none());
    // 不带会话 = 不过滤（看板、防休眠等宿主内部用）。
    assert!(snapshot(cwd).is_some());
}

#[test]
fn snapshot_reads_task_only_inside_project() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    assert!(snapshot(root.to_str().unwrap()).is_none());
    let t = make_with(&root, "implement", &[json!(true), json!("false"), json!(false)], SLUG);
    fs::write(t.join("decisions.md"), "| 决策 |").unwrap();
    write_gate(&t, "g1", "A", "pending");
    fs::create_dir_all(root.join("src")).unwrap();
    let s = snapshot(root.join("src").to_str().unwrap()).expect("从子目录也要找到项目根目录的任务");
    assert_eq!(s.slug, SLUG);
    assert_eq!(s.phase, "implement");
    assert_eq!(s.checks.iter().filter(|c| c.passes).count(), 1);
    assert_eq!(s.checks[0].id, "1");
    assert!(s.spec.contains("目的：演示") && s.decisions.contains("决策"));
    assert_eq!(s.gate.unwrap().kind, "A");
    // active 指到目录外 → 拒绝
    fs::write(root.join(".lead/active"), "../../etc").unwrap();
    assert!(snapshot(root.to_str().unwrap()).is_none());
}

#[test]
fn stop_yields_and_session_injects_for_gates() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make(&root);
    let prog = t.join("progress.md").to_string_lossy().into_owned();
    claim_as(&e, &root, "A", "Edit", json!({"file_path": prog}));
    let tr = transcript(&root, "w", "继续", &[("Edit", json!({"file_path": prog}))]);
    let w = json!({"session_id": "A", "transcript_path": tr});
    assert!(blocked(&e, w.clone()));
    write_gate(&t, "g1", "B", "pending");
    assert!(!blocked(&e, w.clone()), "有挂起关卡时停下是对的");
    answer_gate(&t, "g1", &ans("2", "")).unwrap();
    assert!(!blocked(&e, w.clone()));
    let ctx = call(&e, "session", json!({"session_id": "A", "cwd": root})).unwrap();
    assert!(ctx["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("选择 2"));
    // 消费后恢复正常拦截
    write_gate(&t, "g1", "B", "consumed");
    assert!(blocked(&e, w));
}

#[test]
fn wait_wakes_on_answer_and_dedupes_waiters() {
    use std::time::Duration;
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make_with(&root, "design", &[json!(false)], SLUG);
    let data = json!({"session_id": "S", "cwd": root});
    let fast = |d: &Value| wait(d, &e.reg, Duration::from_millis(20), Duration::from_secs(5));
    // 只唤醒归属会话（做空审查 H1）：S 还没认领时，哪怕关卡已作答也不唤醒
    write_gate(&t, "g0", "B", "pending");
    answer_gate(&t, "g0", &ans("1", "")).unwrap();
    assert_eq!(fast(&data).exit, 0, "没认领任务的会话不能被唤醒");
    claim_as(&e, &root, "S", "Edit", json!({"file_path": t.join("progress.md").to_string_lossy()}));
    let other = json!({"session_id": "OTHER", "cwd": root});
    assert_eq!(fast(&other).exit, 0, "同项目的其他会话不被唤醒");
    fs::remove_file(t.join("gate.json")).unwrap();
    // 没有关卡 / 已消费 → 立刻 0
    assert_eq!(fast(&data).exit, 0);
    write_gate(&t, "g1", "B", "consumed");
    assert_eq!(fast(&data).exit, 0);
    // 已作答（会话恢复后）→ 立刻 2，stderr 带答复
    write_gate(&t, "g1", "B", "pending");
    answer_gate(&t, "g1", &ans("1", "好")).unwrap();
    let out = fast(&data);
    assert_eq!(out.exit, 2);
    assert!(out.stderr.unwrap().contains("选择 1，意见：好"));
    // 同一答复不重复唤醒（真机实测：agent 没标 consumed 时，下一次 Stop 又被唤醒）
    assert_eq!(fast(&data).exit, 0, "Stop 时同一答复第二次不唤醒");
    let start = json!({"session_id": "S", "cwd": root, "hook_event_name": "SessionStart"});
    assert_eq!(fast(&start).exit, 2, "新起的进程（SessionStart）再送一次");
    // 挂起中：另一线程稍后作答 → 唤醒
    write_gate(&t, "g2", "A", "pending");
    let t2 = t.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        answer_gate(&t2, "g2", &ans("confirm", "")).unwrap();
    });
    let out = fast(&data);
    h.join().unwrap();
    assert_eq!(out.exit, 2);
    assert!(!t.join(".waiter").exists(), "等待者退出后要清掉 .waiter");
    // 挂起中被撤掉 → 0
    write_gate(&t, "g3", "C", "pending");
    let t3 = t.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        fs::remove_file(t3.join("gate.json")).unwrap();
    });
    assert_eq!(fast(&data).exit, 0);
    h.join().unwrap();
    // 后来者接管（做空审查 H2：旧会话的孤儿等待者不能挡住新会话）：
    // 旧等待者看到 .waiter 被换掉就退出 0；新等待者收到答复
    write_gate(&t, "g4", "C", "pending");
    fs::write(t.join(".waiter"), "999999:g4").unwrap(); // 一个已经不在的旧等待者
    let t4 = t.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        answer_gate(&t4, "g4", &ans("accept", "")).unwrap();
    });
    assert_eq!(fast(&data).exit, 2, "新等待者直接接管，不被旧记录挡住");
    h.join().unwrap();
    // 被接管的等待者退出
    write_gate(&t, "g5", "C", "pending");
    let t5 = t.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        fs::write(t5.join(".waiter"), "1:g5").unwrap(); // 新等待者写入
    });
    assert_eq!(fast(&data).exit, 0, "被接管后退出");
    h.join().unwrap();
    // 偶发读失败不退出（做空审查 M5）：短暂删掉 gate.json 再放回，仍能收到答复
    write_gate(&t, "g6", "A", "pending");
    let t6 = t.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let body = fs::read_to_string(t6.join("gate.json")).unwrap();
        fs::remove_file(t6.join("gate.json")).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        fs::write(t6.join("gate.json"), body).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        answer_gate(&t6, "g6", &ans("confirm", "")).unwrap();
    });
    assert_eq!(fast(&data).exit, 2);
    h.join().unwrap();
    // 超时 → 0
    write_gate(&t, "g7", "C", "pending");
    let out = wait(&data, &e.reg, Duration::from_millis(20), Duration::from_millis(100));
    assert_eq!(out.exit, 0);
}

#[test]
fn verbatim_prefix_is_stripped() {
    assert_eq!(strip_verbatim(PathBuf::from(r"\\?\C:\a\.lead\x")), PathBuf::from(r"C:\a\.lead\x"));
    assert_eq!(strip_verbatim(PathBuf::from(r"\\?\UNC\srv\share")), PathBuf::from(r"\\?\UNC\srv\share"));
    assert_eq!(strip_verbatim(PathBuf::from("/tmp/x")), PathBuf::from("/tmp/x"));
}

#[test]
fn gates_of_all_tasks_are_visible() {
    // 做空审查 M4：新开任务改写 active 后，旧任务挂着的关卡不能隐身
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let old = make_with(&root, "design", &[json!(false)], SLUG);
    write_gate(&old, "B-old", "B", "pending");
    let new = root.join(".lead/20261110-new");
    fs::create_dir_all(&new).unwrap();
    fs::write(new.join("progress.md"), "phase: interview
").unwrap();
    fs::write(root.join(".lead/active"), "20261110-new").unwrap();
    let cwd = root.to_string_lossy().into_owned();
    let snap = snapshot(&cwd).unwrap();
    assert_eq!(snap.slug, "20261110-new");
    assert_eq!(snap.gate.unwrap().id, "B-old", "active 没有关卡时带出别的任务挂起的关卡");
    assert_eq!(task_of_gate(&cwd, "B-old").unwrap().file_name().unwrap(), SLUG);
    assert!(!snapshot(&cwd).unwrap().spec.contains("\\?"));
}

#[test]
fn answer_injected_only_into_owner_and_choice_validated() {
    let e = env();
    let root = e.d.join("p");
    fs::create_dir_all(root.join(".git")).unwrap();
    let t = make_with(&root, "design", &[json!(false)], SLUG);
    write_gate(&t, "g1", "B", "pending");
    // 第二轮 L5：不存在的方案 id、A/C 的非法选项都拒绝
    assert_eq!(answer_gate(&t, "g1", &ans("9", "")), Err("lead/gate_bad_choice".into()));
    write_gate(&t, "gA", "A", "pending");
    assert_eq!(answer_gate(&t, "gA", &ans("accept", "")), Err("lead/gate_bad_choice".into()));
    answer_gate(&t, "gA", &ans("confirm", "")).unwrap();
    // 第二轮 M1：答复只注入归属会话
    claim_as(&e, &root, "OWNER", "Edit", json!({"file_path": t.join("progress.md").to_string_lossy()}));
    let ctx = |sid: &str| {
        call(&e, "session", json!({"session_id": sid, "cwd": root})).unwrap()["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(ctx("OWNER").contains("选择 confirm"));
    assert!(!ctx("OTHER").contains("选择 confirm"), "无关会话不能拿到「按答复继续」");
}
