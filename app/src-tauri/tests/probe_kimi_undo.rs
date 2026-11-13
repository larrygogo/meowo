//! 验证 kimi `/undo N` 的真机行为，撤回功能的前提全靠它：
//! - 带参直接撤、不弹选择菜单；
//! - wire 里追加 `context.undo {count}`（buildUndoSwitchRecords 的兼容行）；
//! - 我们的读取管线据此收回那一轮（read_chat_delta 增量读到撤回 → reset + undone）。
//!
//! 会真的发一条很短的消息（花一次模型请求），并在 kimi 数据目录留下临时目录的信任记录与
//! 一个探针会话。
//!
//! `cargo test -p meowo-app --test probe_kimi_undo -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

mod common;

fn strip_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
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

/// 探针开始后新写的 wire.jsonl（本次会话的主 agent 流）。
fn newest_wire(root: &Path, since: SystemTime) -> Option<PathBuf> {
    let mut best: Option<(SystemTime, PathBuf)> = None;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "wire.jsonl")
                && path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "main")
            {
                let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else { continue };
                if modified >= since && best.as_ref().is_none_or(|(t, _)| modified > *t) {
                    best = Some((modified, path));
                }
            }
        }
    }
    best.map(|(_, path)| path)
}

#[test]
#[ignore = "拉起真实 kimi、发一条消息再 /undo；手动调研用"]
fn kimi_undo_appends_context_undo_and_retracts_turn() {
    let kimi = meowo_agent::by_id("kimi").expect("kimi 已注册");
    let inst = kimi.detect().expect("本机未检测到 kimi 数据目录");
    let argv = inst.launch_argv();
    let sessions = inst.data_dir.join("sessions");
    let cwd = std::env::temp_dir().join(format!("meowo-undo-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();
    let started = SystemTime::now();

    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut cmd = common::agent_command(&argv[0], &cwd);
    cmd.args(&argv[1..]);
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut raw = Vec::new();
    // 抽干输出并应答终端查询（DSR / DA1，见 probe_kimi_workspace_trust），直到 `until` 成立或超时。
    let pump = |writer: &mut Box<dyn Write + Send>, raw: &mut Vec<u8>, secs: u64, until: &dyn Fn(&str) -> bool| -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        let from = raw.len();
        while Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(300)) {
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
                }
                if chunk.windows(3).any(|w| w == b"\x1b[c") {
                    let _ = writer.write_all(b"\x1b[?62;22c").and_then(|_| writer.flush());
                }
                raw.extend_from_slice(&chunk);
            }
            if until(&strip_ansi(&raw[from..])) {
                return true;
            }
        }
        false
    };
    let submit = |writer: &mut Box<dyn Write + Send>, text: &str| {
        let _ = writer.write_all(text.as_bytes()).and_then(|_| writer.flush());
        std::thread::sleep(Duration::from_millis(150));
        let _ = writer.write_all(b"\r").and_then(|_| writer.flush());
    };

    // 新临时目录必弹信任屏：回车确认。没弹也无妨（等到输入框即可）。
    if pump(&mut writer, &mut raw, 30, &|s| s.to_lowercase().contains("trust")) {
        std::thread::sleep(Duration::from_millis(800));
        let _ = writer.write_all(b"\r").and_then(|_| writer.flush());
    }
    pump(&mut writer, &mut raw, 8, &|_| false);

    submit(&mut writer, "只回复一个字：好");
    // 等回答落盘：wire 出现、且有 turn.ended。
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut wire = None;
    while Instant::now() < deadline {
        pump(&mut writer, &mut raw, 1, &|_| false);
        wire = newest_wire(&sessions, started);
        if let Some(path) = &wire {
            if std::fs::read_to_string(path).is_ok_and(|s| s.contains("\"turn.ended\"")) {
                break;
            }
        }
    }
    let wire = wire.expect("90s 内没等到本次会话的 wire.jsonl");
    std::thread::sleep(Duration::from_millis(1500));
    let spec = kimi.telemetry().and_then(|t| t.transcript()).expect("kimi 有 transcript 规格");
    let before = meowo_agent::read_chat_delta(spec, &wire, 0, None);
    let users = |items: &[meowo_agent::ChatItem]| {
        items
            .iter()
            .filter(|item| matches!(item, meowo_agent::ChatItem::UserText { .. }))
            .count()
    };
    eprintln!("[probe] wire {}；撤回前 {} 条，其中用户消息 {}", wire.display(), before.items.len(), users(&before.items));
    assert_eq!(users(&before.items), 1);

    submit(&mut writer, "/undo 1");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut undone = false;
    while Instant::now() < deadline && !undone {
        pump(&mut writer, &mut raw, 1, &|_| false);
        undone = std::fs::read_to_string(&wire).is_ok_and(|s| s.contains("\"context.undo\""));
    }
    let tail = strip_ansi(&raw);
    let lines: Vec<&str> = tail.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    eprintln!("[probe] /undo 后画面末 15 行:\n{}", lines[lines.len().saturating_sub(15)..].join("\n"));
    let body = std::fs::read_to_string(&wire).unwrap_or_default();
    for line in body.lines().filter(|l| l.contains("undo") || l.contains("agent.switched")) {
        eprintln!("[probe] 撤回记录: {line}");
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(undone, "20s 内 wire 没出现 context.undo——/undo N 的行为可能变了");

    let after = meowo_agent::read_chat_delta(spec, &wire, before.offset, before.mtime);
    eprintln!("[probe] 增量读：reset={} undone={} 剩 {} 条", after.reset, after.undone, after.items.len());
    assert!(after.reset && after.undone);
    assert_eq!(users(&after.items), 0, "被撤的那一轮应全部收回：{:?}", after.items);
}
