//! 取证 claude `/rewind` 的真机行为，撤回功能接 claude 前必须先弄清：菜单长什么样、怎么
//! 选到某条消息、选「只恢复对话」后 transcript 里写了什么、原文回不回 composer。
//!
//! 结论（2.1.314 / Windows，2026-10-30）：
//!   - 菜单标题行 `Rewind`，列表从旧到新、光标停在最底的 `❯ (current)`；上移 k 次落到倒数第
//!     k 条用户消息（多行消息只显示首行加 `…`）。回车进确认页：`│ ` 引出所选消息全文，选项
//!     无代码改动时 `1. Restore conversation`，有改动时 `1. Restore code and conversation`
//!     `2. Restore conversation` `3. Restore code`…，按数字即生效。
//!   - 只恢复对话时文件不动（probe.txt 仍在）。
//!   - **回退零写入 transcript**；下一条消息的 parentUuid 指回回退点，长出新分支。
//!   - 原文回到 composer；双 Esc 整段清空（多行也清），空 composer 的双 Esc 则是打开回退菜单。
//!   - 嵌套跑时须去掉继承的 CLAUDE_CODE_CHILD_SESSION，否则 claude 不写 transcript。
//!
//! 用临时目录做 cwd（首次信任屏回车确认）：会花几次模型请求，并在 ~/.claude 留下该目录的
//! 信任记录与一个探针会话。`CR_STEPS` 按逗号给出 /rewind 之后依次发送的键（`up`/`down`/
//! `enter`/`esc`/`1`…），每步后 dump 屏幕；缺省只打开菜单看一眼再 Esc。
//!
//! `cargo test -p meowo-app --test probe_claude_rewind -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

mod common;

/// 本次会话的 transcript：探针开始后新写、首行 cwd 是临时目录的 jsonl。
fn transcript(since: SystemTime, cwd: &Path) -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    let root = PathBuf::from(home).join(".claude/projects");
    let want = cwd.file_name()?.to_string_lossy().to_string();
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for dir in std::fs::read_dir(root).ok()?.flatten() {
        if !dir.file_name().to_string_lossy().contains(&want) {
            continue;
        }
        for entry in std::fs::read_dir(dir.path()).ok()?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else { continue };
            if modified >= since && best.as_ref().is_none_or(|(t, _)| modified > *t) {
                best = Some((modified, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// 每行的 (type, uuid 前 8 位, parentUuid 前 8 位, 摘要)。
fn summarize(path: &Path) -> Vec<String> {
    let body = std::fs::read_to_string(path).unwrap_or_default();
    body.lines()
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
            let short = |key: &str| v[key].as_str().map(|s| s.chars().take(8).collect::<String>()).unwrap_or_else(|| "-".into());
            let text = v.pointer("/message/content").map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
            let extra = v["subtype"].as_str().map(|s| format!(" subtype={s}")).unwrap_or_default();
            let keys: Vec<&str> = v.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
            format!(
                "{} uuid={} parent={}{extra} | {} | keys={keys:?}",
                v["type"].as_str().unwrap_or("?"),
                short("uuid"),
                short("parentUuid"),
                text.unwrap_or_default().chars().take(60).collect::<String>().replace('\n', "⏎"),
            )
        })
        .collect()
}

#[test]
#[ignore = "拉起真实 claude、发两条消息再 /rewind；手动调研用"]
fn claude_rewind_menu_and_transcript() {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
    let exe = format!("{home}/.local/bin/claude.exe");
    let cwd = std::env::temp_dir().join(format!("meowo-cr-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();
    let started = SystemTime::now();

    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut command = common::agent_command(&exe, &cwd);
    // Chrome 集成确认框挡在 composer 前（probe_draft_residual）；嵌套 claude 别继承本会话的
    // 托管标记，否则宿主把它当成本会话的 /clear 换代（nested-claude 污染）。
    command.arg("--no-chrome");
    // 本探针常在 claude 会话里跑：继承的 CLAUDE_CODE_CHILD_SESSION 会让它不写 transcript
    // （2.1.314 实测「Transcript saving is off」），其余 CLAUDE_CODE_* / MEOWO_* 同理一并去掉。
    command.env_remove("CLAUDECODE");
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
        eprintln!("[probe] {tag} 屏幕:\n{}\n", lines[lines.len().saturating_sub(42)..].join("\n"));
    };
    // 回合结束：屏幕不再有运行提示，且 transcript 里助手回复数达到 n。
    let replies = |n: usize| {
        transcript(started, &cwd).is_some_and(|path| {
            let body = std::fs::read_to_string(path).unwrap_or_default();
            body.lines().filter(|l| l.contains("\"type\":\"assistant\"") && l.contains("\"text\"")).count() >= n
        }) && !contents().to_lowercase().contains("esc to interrupt")
    };

    let result = (|| -> Result<(), String> {
        // 信任屏默认选中「No, exit」：下移到「Yes, I trust this folder」再回车（直接回车会退出）。
        if wait(30, &|| contents().contains("Yes, I trust this folder")) {
            std::thread::sleep(Duration::from_millis(800));
            send("\x1b[B");
            std::thread::sleep(Duration::from_millis(500));
            send("\r");
        }
        if !wait(30, &|| !contents().contains("Yes, I trust this folder") && contents().contains('❯')) {
            dump("启动");
            return Err("30s 内没等到 composer".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        // 第二轮写文件且是多行（ESC+CR = composer 换行）：看有代码改动时确认菜单的选项、只恢复
        // 对话时文件留不留、多行原文回 composer 后怎么清。
        let rounds = std::env::var("CR_ROUNDS").map(|v| v == "3").unwrap_or(false);
        let prompts: &[&str] = if rounds {
            &["只回复一个字：好", "用 Write 工具创建 probe.txt，内容为 hi。\x1b\r完成后只回复一个字：好", "只回复一个字：行"]
        } else {
            &["只回复一个字：好", "只回复一个字：行"]
        };
        // 2.1.314 首个回合后会弹 auto 模式环境学习引导（含扫描 shell 历史），一律 Esc 取消，
        // 别让探针替用户同意；不取消的话下一条消息会被它吞掉。
        let dismiss_setup = || {
            if contents().contains("Teach auto mode about your environment?") {
                send("");
                std::thread::sleep(Duration::from_millis(1500));
            }
        };
        for (index, prompt) in prompts.iter().enumerate() {
            std::thread::sleep(Duration::from_secs(2));
            dismiss_setup();
            submit(prompt);
            if !wait(180, &|| replies(index + 1)) {
                dump(&format!("第 {} 轮", index + 1));
                return Err(format!("第 {} 轮 180s 未完成", index + 1));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        std::thread::sleep(Duration::from_secs(1));
        let path = transcript(started, &cwd).ok_or("找不到 transcript")?;
        let before = std::fs::read_to_string(&path).unwrap_or_default().lines().count();
        eprintln!("[probe] transcript {}\n[probe] 撤回前 {before} 行:\n{}\n", path.display(), summarize(&path).join("\n"));

        // `CR_CMD` 换掉 /rewind（如 `/btw <问题>` 取证侧问是否落盘）。
        let command = std::env::var("CR_CMD").unwrap_or_else(|_| "/rewind".into());
        std::thread::sleep(Duration::from_secs(2));
        dismiss_setup();
        submit(&command);
        std::thread::sleep(Duration::from_secs(2));
        dump(&format!("{command} 后"));
        let steps = std::env::var("CR_STEPS").unwrap_or_else(|_| "esc".into());
        for step in steps.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            // `say:<文本>` = 提交一条消息并等回合结束（看撤回后的下一条怎么挂回 transcript）。
            if let Some(text) = step.strip_prefix("say:") {
                let n = std::fs::read_to_string(&path).unwrap_or_default().lines()
                    .filter(|l| l.contains("\"type\":\"assistant\"") && l.contains("\"text\"")).count();
                submit(text);
                wait(120, &|| replies(n + 1));
                std::thread::sleep(Duration::from_secs(3));
                dump(&format!("发送 {text} 后"));
                continue;
            }
            // `pick:<文案>` = 在确认菜单里按「N. <文案>」那一项的数字（选项随有无代码改动变化）。
            if let Some(label) = step.strip_prefix("pick:") {
                let screen_text = contents();
                let digit = screen_text.lines().find_map(|row| {
                    let row = row.trim().trim_start_matches('❯').trim();
                    let (number, rest) = row.split_once(". ")?;
                    (rest.trim() == label && number.chars().all(|c| c.is_ascii_digit())).then(|| number.to_string())
                });
                eprintln!("[probe] pick {label} → {digit:?}");
                if let Some(digit) = digit {
                    send(&digit);
                }
                std::thread::sleep(Duration::from_millis(1500));
                dump(&format!("pick {label} 后"));
                eprintln!("[probe] probe.txt 存在={}", cwd.join("probe.txt").exists());
                continue;
            }
            // `send:<文本>` = 只提交不等回合（看运行中发 /btw 的表现）。
            if let Some(text) = step.strip_prefix("send:") {
                submit(text);
                std::thread::sleep(Duration::from_millis(1500));
                dump(&format!("提交 {text} 后"));
                continue;
            }
            // `wait:<秒>` = 干等一会儿再看屏幕（侧问回答要走一次模型请求）。
            if let Some(secs) = step.strip_prefix("wait:") {
                std::thread::sleep(Duration::from_secs(secs.parse().unwrap_or(5)));
                dump(&format!("等 {secs}s 后"));
                continue;
            }
            if step == "esc2" {
                send("\x1b");
                std::thread::sleep(Duration::from_millis(300));
                send("\x1b");
                std::thread::sleep(Duration::from_millis(1500));
                dump("双 Esc 后");
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
            std::thread::sleep(Duration::from_millis(1500));
            dump(&format!("按 {step} 后"));
        }
        std::thread::sleep(Duration::from_secs(2));
        let lines = summarize(&path);
        eprintln!("[probe] 撤回后 {} 行，新增:\n{}\n", lines.len(), lines[before.min(lines.len())..].join("\n"));
        Ok(())
    })();
    let last_screen = contents();
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(result.is_ok(), "{}；末屏=\n{last_screen}", result.unwrap_err());
}
