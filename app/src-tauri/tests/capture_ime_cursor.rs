//! 一次性调研工具：抓 kimi TUI 稳态帧的**帧尾光标状态**。
//!
//! 背景：GUI 托管终端里 kimi 的输入法候选栏钉在输入行最右缘。xterm 组合输入的锚点是
//! 硬件光标（buffer.x，含 pending-wrap 时被钳到 cols-1），若 kimi 自绘假光标、把真光标
//! 留在行尾且 `?25l` 隐藏，这个错位就不是 xterm 的测量问题，而是锚点本身错了。
//! 本探针拉起真实 kimi、往 composer 打字，等画面静止后输出末帧的转义序列尾巴，
//! 用来人工确认：帧尾光标停在哪、可见性如何。默认 `#[ignore]`。手动跑：
//! `cargo test -p meowo-app --test capture_ime_cursor -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod common;

/// 把字节流转成可读转义：ESC → `\e`，其余控制字节按 `\xNN`。
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            0x1b => out.push_str("\\e"),
            0x20..=0x7e => out.push(b as char),
            b'\r' => out.push_str("\\r"),
            b'\n' => out.push_str("\\n\n"),
            _ if b >= 0x80 => out.push('·'), // UTF-8 延续字节：内容无关紧要，占位即可
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

#[test]
#[ignore]
fn capture_ime_cursor_state() {
    let exe = std::env::var("MEOWO_CAPTURE_EXE").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap();
        format!("{home}/.kimi-code/bin/kimi")
    });
    let cwd = std::env::temp_dir().join(format!("meowo-ime-capture-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let command = common::agent_command(&exe, &cwd);
    let mut child = pair.slave.spawn_command(command).unwrap();
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

    let mut all: Vec<u8> = Vec::new();
    // 首屏 + DSR 应答。
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(1500)) {
            Ok(chunk) => {
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = writer.write_all(b"\x1b[1;1R");
                    let _ = writer.flush();
                }
                all.extend_from_slice(&chunk);
            }
            Err(_) => break,
        }
    }
    // 往 composer 打两个字符，逼出「输入行有内容」的稳态帧。
    let _ = writer.write_all(b"ab");
    let _ = writer.flush();
    let typed_deadline = Instant::now() + Duration::from_secs(8);
    let mut last_len = all.len();
    while Instant::now() < typed_deadline {
        match rx.recv_timeout(Duration::from_millis(1500)) {
            Ok(chunk) => all.extend_from_slice(&chunk),
            Err(_) => {
                if all.len() == last_len {
                    break; // 1.5s 无新输出 = 画面静止
                }
                last_len = all.len();
            }
        }
    }

    println!("== 总字节 {} ==", all.len());
    let tail = &all[all.len().saturating_sub(2500)..];
    println!("== 末帧尾巴（转义） ==\n{}", escaped(tail));
    // 光标可见性收尾统计：最后一次 show/hide 谁在后。
    let last_hide = all.windows(6).rposition(|w| w == b"\x1b[?25l");
    let last_show = all.windows(6).rposition(|w| w == b"\x1b[?25h");
    println!("== last ?25l(hide) @ {last_hide:?}, last ?25h(show) @ {last_show:?} ==");

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&cwd);
}
