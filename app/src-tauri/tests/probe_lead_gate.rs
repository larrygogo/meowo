//! 取证：Lead 关卡的端到端送达（验收 6、7）。真实交互式 claude（TUI，跑在 PTY 里），hook 用
//! worktree 编译出的 meowo-reporter 的 lead 子命令，与 Meowo「一键安装」写进 settings 的条目同形。
//!
//! 第一段（挂起后作答）：agent 写 progress.md 认领任务后结束本轮，Stop 上的 asyncRewake `lead wait`
//! 在后台等关卡；挂 `GATE_HANG_SECS` 秒后用 `answer_gate` 作答（远程桥臂 answer_lead_gate 调的就是它），
//! 断言模型被唤醒并读到答复。
//! 第二段（进程死后作答）：再挂一个关卡，**杀掉** claude，作答，再 `claude --resume` 恢复（Meowo 后端
//! 在进程不在时做的就是这一步）。断言 SessionStart 上的 `lead wait` 立刻唤醒模型。
//!
//! 工作目录用已信任的目录（PROBE_CWD，不往里写任何东西），任务目录与 board.db 在临时目录里。
//! `REPORTER=<meowo-reporter.exe> PROBE_CWD=<已信任目录> GATE_HANG_SECS=1800 \
//!   cargo test -p meowo-app --test probe_lead_gate -- --ignored --nocapture`

use meowo_protocol::ipc::LeadGateAnswerDto;
use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

fn strip(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if !c.is_whitespace() {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                for c in chars.by_ref() {
                    if c == '\u{7}' || c == '\u{1b}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

struct Tui {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    buf: Arc<Mutex<Vec<u8>>>,
    _writer: Box<dyn Write + Send>,
}

impl Tui {
    fn spawn(work: &Path, db: &Path, settings: &Path, args: &[String]) -> Tui {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 160,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = common::agent_command("claude", work);
        cmd.env_remove("CLAUDE_CODE_CHILD_SESSION");
        cmd.env("MEOWO_DB", db);
        cmd.arg("--settings");
        cmd.arg(settings.to_string_lossy().to_string());
        cmd.arg("--permission-mode");
        cmd.arg("bypassPermissions");
        for a in args {
            cmd.arg(a);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let buf = Arc::new(Mutex::new(Vec::new()));
        let b2 = buf.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Ok(n) = reader.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                b2.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        });
        // 保住 master，PTY 才不会提前关
        std::mem::forget(pair.master);
        Tui {
            child,
            buf,
            _writer: writer,
        }
    }

    fn screen(&self) -> String {
        strip(&self.buf.lock().unwrap())
    }

    fn tail(&self) -> String {
        let s = self.screen();
        s.chars()
            .rev()
            .take(800)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

fn write_gate(task: &Path, id: &str) {
    let gate = serde_json::json!({
        "v": 1, "id": id, "type": "B", "title": "选颜色", "summary": "选一个颜色",
        "options": [{"id": "1", "label": "红"}, {"id": "2", "label": "蓝"}],
        "status": "pending", "answer": null
    });
    std::fs::write(task.join("gate.json"), gate.to_string()).unwrap();
}

/// 以 transcript 为准等 agent 的回复（抓屏不可靠：--resume 后会重绘历史，30 分钟版本里抓屏误判过一次
/// 「已唤醒」并提前杀掉了进程）。返回该回复在 assistant 消息里出现的次数。
fn transcript_count(sid: &str, needle: &str) -> u32 {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let mut n = 0;
    let Ok(dirs) = std::fs::read_dir(PathBuf::from(home).join(".claude/projects")) else {
        return 0;
    };
    for d in dirs.flatten() {
        let Ok(text) = std::fs::read_to_string(d.path().join(format!("{sid}.jsonl"))) else {
            continue;
        };
        for line in text.lines() {
            let Ok(o) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if o["type"] == "assistant" && o["message"]["content"].to_string().contains(needle) {
                n += 1;
            }
        }
    }
    n
}

fn wait_transcript(sid: &str, needle: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if transcript_count(sid, needle) > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

fn registry_sid(db: &Path) -> Option<String> {
    let text = std::fs::read_to_string(db.with_file_name("lead-sessions.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.as_object()?.keys().next().cloned()
}

#[test]
#[ignore = "拉起真实 claude 交互进程并等待数分钟；手动调研用"]
fn probe_gate_delivery_hold_and_after_restart() {
    let reporter = std::env::var("REPORTER").expect("REPORTER=meowo-reporter.exe 的路径");
    let hang: u64 = std::env::var("GATE_HANG_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let dir = std::env::temp_dir().join(format!("meowo-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let proj = dir.join("proj");
    let task = proj.join(".lead").join("20261109-gateprobe");
    std::fs::create_dir_all(proj.join(".git")).unwrap();
    std::fs::create_dir_all(&task).unwrap();
    std::fs::write(proj.join(".lead/active"), "20261109-gateprobe").unwrap();
    std::fs::write(task.join("SPEC.md"), "# SPEC\n目的：演示关卡送达").unwrap();
    std::fs::write(task.join("progress.md"), "phase: design\n").unwrap();
    std::fs::write(
        task.join("acceptance.json"),
        r#"[{"id":1,"check":"x","passes":false}]"#,
    )
    .unwrap();
    write_gate(&task, "B-1");
    let work =
        PathBuf::from(std::env::var("PROBE_CWD").expect("PROBE_CWD=一个已信任、没有 .lead 的目录"));
    let db = dir.join("board.db");
    let cmd = |m: &str| format!("\"{reporter}\" lead {m}");
    let settings = serde_json::json!({"hooks": {
        "SessionStart": [{"hooks": [
            {"type": "command", "command": cmd("session"), "timeout": 10},
            {"type": "command", "command": cmd("wait"), "asyncRewake": true, "timeout": 700000}
        ]}],
        "PostToolUse": [{"matcher": "Write|Edit|MultiEdit|Bash|PowerShell", "hooks": [{"type": "command", "command": cmd("claim"), "timeout": 10}]}],
        "Stop": [{"hooks": [
            {"type": "command", "command": cmd("stop"), "timeout": 10},
            {"type": "command", "command": cmd("wait"), "asyncRewake": true, "timeout": 700000}
        ]}]
    }});
    let settings_file = dir.join("settings.json");
    std::fs::write(&settings_file, settings.to_string()).unwrap();
    let prog = task
        .join("progress.md")
        .to_string_lossy()
        .replace('\\', "/");
    let rule =
        "收到 [lead] 开头的消息时，只回复一行：已收到关卡答复，选择=<选择的编号>。不要做别的。";

    // ---------- 第一段：挂起后作答 ----------
    let tui = Tui::spawn(
        &work,
        &db,
        &settings_file,
        &[format!(
            "用 Bash 执行：echo 等关卡 >> \"{prog}\" 。然后只回复 OK 并结束。{rule}"
        )],
    );
    let claimed = {
        let start = Instant::now();
        loop {
            if registry_sid(&db).is_some() || start.elapsed() > Duration::from_secs(180) {
                break registry_sid(&db);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    };
    println!("[probe] 认领：{claimed:?}");
    assert!(claimed.is_some(), "没有认领任务，探针无效\n{}", tui.tail());
    std::thread::sleep(Duration::from_secs(10)); // 让本轮结束、后台 wait 起来
    println!("[probe] 关卡挂起 {hang}s …");
    std::thread::sleep(Duration::from_secs(hang));
    meowo_reporter::lead::answer_gate(
        &task,
        "B-1",
        &LeadGateAnswerDto {
            choice: "2".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let sid = claimed.clone().unwrap();
    let woke1 = wait_transcript(&sid, "选择=2", Duration::from_secs(120));
    println!(
        "[probe] 第一段（挂 {hang}s 后作答）：{}\n{}",
        if woke1 { "已唤醒" } else { "未唤醒" },
        tui.tail()
    );

    // ---------- 第二段：进程死后作答 ----------
    write_gate(&task, "B-2");
    std::thread::sleep(Duration::from_secs(5));
    let mut tui = tui;
    let _ = tui.child.kill();
    std::thread::sleep(Duration::from_secs(3));
    meowo_reporter::lead::answer_gate(
        &task,
        "B-2",
        &LeadGateAnswerDto {
            choice: "1".into(),
            note: String::new(),
        },
    )
    .unwrap();
    let tui2 = Tui::spawn(
        &work,
        &db,
        &settings_file,
        &["--resume".into(), sid.clone()],
    );
    let woke2 = wait_transcript(&sid, "选择=1", Duration::from_secs(180));
    // 再等一会儿：确认之后的 Stop 不会把同一个答复再送一次
    std::thread::sleep(Duration::from_secs(20));
    println!(
        "[probe] 第二段（进程死后作答，--resume 恢复）：{}\n{}",
        if woke2 { "已唤醒" } else { "未唤醒" },
        tui2.tail()
    );
    let mut tui2 = tui2;
    let _ = tui2.child.kill();
    std::thread::sleep(Duration::from_secs(2));
    let replies = (
        transcript_count(&sid, "选择=2"),
        transcript_count(&sid, "选择=1"),
    );
    println!(
        "[probe] transcript 里的答复次数：选择=2 ×{}，选择=1 ×{}",
        replies.0, replies.1
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(woke1, "挂起后作答没有唤醒");
    assert!(woke2, "进程死后作答、恢复会话后没有唤醒");
    assert_eq!(replies, (1, 1), "每个答复应恰好唤醒一次");
}
