//! 取证 codex 双击 Esc 回溯（编辑上一条消息）的真机行为，撤回接 codex 前必须先弄清：回溯界面
//! 长什么样、怎么选到某条消息、确认后 rollout 写什么、原文回不回 composer。
//!
//! 用临时目录做 cwd：会花几次模型请求，并在 ~/.codex 留下一个探针会话。`CX_STEPS` 按逗号给出
//! 两轮对话之后依次发送的键（`esc`/`enter`/`up`/`down`/`ctrlu`/`say:<文本>`），每步后 dump 屏幕。
//!
//! `cargo test -p meowo-app --test probe_codex_backtrack -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

mod common;

/// 本次会话的 rollout：探针开始后新写、内容里出现临时目录名的那份。
fn rollout(since: SystemTime, cwd: &Path) -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    let want = cwd.file_name()?.to_string_lossy().to_string();
    let mut stack = vec![PathBuf::from(home).join(".codex/sessions")];
    let mut best: Option<(SystemTime, PathBuf)> = None;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else { continue };
            if modified < since || best.as_ref().is_some_and(|(t, _)| modified <= *t) {
                continue;
            }
            let head: String = std::fs::read_to_string(&path).unwrap_or_default().chars().take(4000).collect();
            if head.contains(&want) {
                best = Some((modified, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// 每行的 (外层 type / payload.type, 摘要)，跳过体积大的 response_item。
fn summarize(path: &Path) -> Vec<String> {
    let body = std::fs::read_to_string(path).unwrap_or_default();
    body.lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
            let payload = &v["payload"];
            let text = payload.get("message").or_else(|| payload.pointer("/item/content")).map(|m| m.to_string()).unwrap_or_default();
            format!(
                "{}/{}{} {}",
                v["type"].as_str().unwrap_or("?"),
                payload["type"].as_str().unwrap_or("?"),
                payload.pointer("/item/type").and_then(|t| t.as_str()).map(|t| format!("/{t}")).unwrap_or_default(),
                text.chars().take(60).collect::<String>(),
            )
        })
        .collect()
}

#[test]
#[ignore = "拉起真实 codex、发两条消息再回溯；手动调研用"]
fn codex_backtrack_screen_and_rollout() {
    let argv = meowo_agent::by_id("codex").unwrap().launch_argv();
    let cwd = std::env::temp_dir().join(format!("meowo-cx-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();
    let started = SystemTime::now();

    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut command = common::agent_command(&argv[0], &cwd);
    command.args(&argv[1..]);
    // 启动时的「有新版本」选择器第一项是立即更新：探针曾在这里按了 1，把用户的 codex 从
    // 0.163.0 升到了 0.166.0。关掉启动更新检查，探针不得改动用户安装。
    command.args(["-c", "check_for_update_on_startup=false"]);
    for (key, _) in std::env::vars() {
        if key.starts_with("CLAUDE_CODE") || (key.starts_with("MEOWO_") && key != "MEOWO_DB") {
            command.env_remove(key);
        }
    }
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
    let (answer, parser) = (writer.clone(), screen.clone());
    std::thread::spawn(move || {
        let mut bytes = [0; 16_384];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 {
                break;
            }
            let chunk = &bytes[..count];
            let mut writer = answer.lock().unwrap();
            if chunk.windows(4).any(|part| part == b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
            }
            if chunk.windows(3).any(|part| part == b"\x1b[c") {
                let _ = writer.write_all(b"\x1b[?1;2c").and_then(|_| writer.flush());
            }
            drop(writer);
            parser.lock().unwrap().process(chunk);
        }
    });
    let contents = || screen.lock().unwrap().screen().contents();
    let send = |text: &str| {
        let mut writer = writer.lock().unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.flush().unwrap();
    };
    let submit = |text: &str| {
        send(text);
        std::thread::sleep(Duration::from_millis(400));
        send("\r");
    };
    let wait = |secs: u64, want: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if want() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        false
    };
    let dump = |tag: &str| {
        let text = contents();
        let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
        eprintln!("[probe] {tag} 屏幕:\n{}\n", lines[lines.len().saturating_sub(26)..].join("\n"));
    };
    let completes = |n: usize| {
        rollout(started, &cwd).is_some_and(|path| {
            std::fs::read_to_string(path).unwrap_or_default().matches("\"task_complete\"").count() >= n
        })
    };

    let result = (|| -> Result<(), String> {
        std::thread::sleep(Duration::from_secs(6));
        dump("启动");
        // 只认明确的信任问句才确认；任何别的选择器（更新、登录）一律不碰，dump 出来报错。
        if contents().contains("Do you trust") {
            send("1");
            std::thread::sleep(Duration::from_secs(1));
            send("\r");
            std::thread::sleep(Duration::from_secs(3));
            dump("信任后");
        }
        if !contents().contains("Ask Codex to do anything") {
            return Err("首屏不是输入框（可能是更新/登录选择器），不按任何键".into());
        }
        for (index, prompt) in ["只回复一个字：好", "只回复一个字：行"].iter().enumerate() {
            submit(prompt);
            if !wait(180, &|| completes(index + 1)) {
                dump(&format!("第 {} 轮", index + 1));
                return Err(format!("第 {} 轮 180s 未完成", index + 1));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let path = rollout(started, &cwd).ok_or("找不到 rollout")?;
        let before = summarize(&path).len();
        dump("两轮后");
        let steps = std::env::var("CX_STEPS").unwrap_or_else(|_| "esc,esc".into());
        for step in steps.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(text) = step.strip_prefix("say:") {
                let n = std::fs::read_to_string(&path).unwrap_or_default().matches("\"task_complete\"").count();
                submit(text);
                wait(120, &|| completes(n + 1));
                std::thread::sleep(Duration::from_secs(2));
                dump(&format!("发送 {text} 后"));
                continue;
            }
            let key = match step {
                "up" => "\x1b[A",
                "down" => "\x1b[B",
                "enter" => "\r",
                "esc" => "\x1b",
                "ctrlu" => "\x15",
                other => other,
            };
            send(key);
            std::thread::sleep(Duration::from_millis(1200));
            dump(&format!("按 {step} 后"));
        }
        std::thread::sleep(Duration::from_secs(2));
        let lines = summarize(&path);
        eprintln!("[probe] rollout {}\n[probe] 回溯后新增:\n{}\n", path.display(), lines[before.min(lines.len())..].join("\n"));
        Ok(())
    })();
    let last_screen = contents();
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(result.is_ok(), "{}；末屏=\n{last_screen}", result.unwrap_err());
}
