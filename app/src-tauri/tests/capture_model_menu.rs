//! 一次性调研工具：在真实 PTY 里把某个 agent 的 `/model` 菜单抓下来。
//!
//! 存在的理由：`/model` 是交互式 TUI 菜单，各家的形态（提示语、光标标记、编号样式）
//! 只能实测，不能照着别家猜。GUI 要把它渲染成按钮，就必须先有一份真实画面。
//!
//! 默认 `#[ignore]`——它会真的拉起一个 agent 进程。手动跑：
//! `cargo test -p meowo-app --test capture_model_menu -- --ignored --nocapture`

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 从通道收到「安静了 quiet 这么久」或超时为止。
///
/// 必须走读线程：PTY 的 `read` 是阻塞的，在主线程里直接读的话「安静」永远判定不到——
/// 它要等下一批数据来才有机会检查，而没有下一批正是安静的定义。
fn read_until_quiet(rx: &mpsc::Receiver<Vec<u8>>, quiet: Duration, cap: Duration) -> Vec<u8> {
    let start = Instant::now();
    let mut out = Vec::new();
    while start.elapsed() < cap {
        match rx.recv_timeout(quiet) {
            Ok(chunk) => out.extend_from_slice(&chunk),
            Err(_) => break,
        }
    }
    out
}

#[test]
#[ignore = "会拉起真实 agent 进程；仅供手动调研"]
fn capture_model_menu() {
    let exe = std::env::var("MEOWO_CAPTURE_EXE").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
        format!("{home}/.kimi-code/bin/kimi")
    });
    let cwd = std::env::temp_dir().join("meowo-capture-model");
    std::fs::create_dir_all(&cwd).unwrap();

    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut command = CommandBuilder::new(&exe);
    command.cwd(&cwd);
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let (dsr_tx, dsr_rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            // TUI 会用 DSR（ESC[6n）问光标在哪，收不到回应就卡在启动上不往下画。
            // 真实终端由终端模拟器应答，这里得自己来。
            if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                let _ = dsr_tx.send(());
            }
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    // 应答线程：写端在主线程用，故这里只发信号，由主循环代写。
    std::thread::spawn(move || {
        while dsr_rx.recv().is_ok() {
            // 占位：实际应答在主线程 write（见下）。
        }
    });

    // 等首屏画完（长会话启动可能慢，给足时间）；期间按需应答 DSR。
    let mut boot = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(1500)) {
            Ok(chunk) => {
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = writer.write_all(b"\x1b[1;1R");
                    let _ = writer.flush();
                }
                boot.extend_from_slice(&chunk);
            }
            Err(_) => break,
        }
    }
    eprintln!("=== 启动画面（可见文本，尾部） ===\n{}", tail_lines(&visible(&boot), 25));

    writer.write_all(b"/model\r").unwrap();
    writer.flush().unwrap();
    let menu = read_until_quiet(&rx, Duration::from_millis(1500), Duration::from_secs(25));

    eprintln!("=== /model 之后的原始输出（带转义，截断） ===\n{:?}", tail(&menu, 2500));
    eprintln!("=== /model 之后的可见文本 ===\n{}", visible(&menu));

    let _ = child.kill();
    let _ = std::fs::remove_dir_all(&cwd);
}

fn tail(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let chars: Vec<char> = text.chars().collect();
    chars[chars.len().saturating_sub(max)..].iter().collect()
}

fn tail_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max)..].join("\n")
}

/// 粗剥 CSI/OSC，只为肉眼看清菜单结构；真正的还原由前端的 xterm 负责。
fn visible(bytes: &[u8]) -> String {
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
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
