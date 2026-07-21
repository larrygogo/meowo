//! 验证 kimi 在 kitty 键盘协议下的提交键。
//!
//! 背景：kimi 启动时发 `ESC[>7u`（启用 kitty keyboard protocol）。xterm.js 不实现该协议，
//! 但 kimi 认为已启用，于是按协议解析输入——裸 `\r` 变成「插入换行」，提交要用 CSI-u
//! 编码的 Enter（`ESC[13u`）。这正是 GUI 里 `/plan on` 只换行不提交的根因。
//!
//! `cargo test -p meowo-app --test probe_enter -- --ignored --nocapture`

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    out.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore = "拉起真实 kimi 进程；手动调研用"]
fn probe_enter_key() {
    // 分两次写，测「正文→Enter」的间隔多短仍能提交。生产用 20ms，探针此前用 600ms。
    // 判据看状态栏是否切到 plan / 输入框是否清空，而不是「输出里没有原文」。
    for (label, key, split) in [
        ("间隔 20ms", "\r".as_bytes(), true),
        ("间隔 60ms", "\r".as_bytes(), true),
        ("间隔 150ms", "\r".as_bytes(), true),
        ("间隔 400ms", "\r".as_bytes(), true),
    ] {
        let gap_ms: u64 = label
            .trim_start_matches("间隔 ")
            .trim_end_matches("ms")
            .parse()
            .unwrap_or(20);
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
        let exe = format!("{home}/.kimi-code/bin/kimi");
        let cwd = std::env::temp_dir().join(format!("meowo-enter-{}-{}", std::process::id(), label.len()));
        std::fs::create_dir_all(&cwd).unwrap();
        let pair = native_pty_system()
            .openpty(PtySize { rows: 40, cols: 100, pixel_width: 0, pixel_height: 0 })
            .unwrap();
        let mut cmd = CommandBuilder::new(&exe);
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        // probe 拉起的是**真实** agent 进程，它的 hook 会照常上报会话。不隔离 MEOWO_DB
        // 的话，每跑一轮就往用户的 ~/.meowo/board.db 里塞一条空会话，按 last_event_at
        // 排在最前面，把真实会话挤出侧栏首页（曾经攒到 47 条）。
        cmd.env("MEOWO_DB", cwd.join("board.db"));
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
        // 等首屏并应答 DSR。
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut boot = Vec::new();
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(1200)) {
                Ok(c) => {
                    if c.windows(4).any(|w| w == b"\x1b[6n") {
                        let _ = writer.write_all(b"\x1b[1;1R");
                        let _ = writer.flush();
                    }
                    boot.extend_from_slice(&c);
                }
                Err(_) => break,
            }
        }
        if boot.windows(6).any(|w| w == b"\x1b[>7u") {
            eprintln!("[确认] kimi 请求启用 kitty 键盘协议（ESC[>7u）");
        }
        std::thread::sleep(Duration::from_millis(600));
        if split {
            let _ = writer.write_all(b"/plan on").and_then(|_| writer.flush());
            std::thread::sleep(Duration::from_millis(gap_ms));
            while rx.try_recv().is_ok() {} // 清掉回显
            let _ = writer.write_all(key).and_then(|_| writer.flush());
        } else {
            // 生产做法：文字与回车同一次 write。
            let mut payload = b"/plan on".to_vec();
            payload.extend_from_slice(key);
            let _ = writer.write_all(&payload).and_then(|_| writer.flush());
        }
        let mut after = Vec::new();
        let end = Instant::now() + Duration::from_secs(6);
        while Instant::now() < end {
            match rx.recv_timeout(Duration::from_millis(1200)) {
                Ok(c) => after.extend_from_slice(&c),
                Err(_) => break,
            }
        }
        let screen = strip_ansi(&after);
        let lines: Vec<&str> = screen.lines().collect();
        let tail = lines[lines.len().saturating_sub(6)..].join("\n");
        eprintln!("\n### {label} → 按键后画面:\n{tail}\n");
        let _ = child.kill();
        let _ = std::fs::remove_dir_all(&cwd);
    }
}
