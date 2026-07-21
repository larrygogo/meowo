//! 一次性调研工具：在真实 PTY 里把某个 agent 的 TUI 行为抓下来。
//!
//! 存在的理由：TUI 的交互（`/model` 菜单形态、composer 用哪个键提交）各家不同，
//! 只能实测，不能照着别家猜。默认 `#[ignore]`——会真的拉起 agent 进程。手动跑：
//! `cargo test -p meowo-app --test capture_model_menu -- --ignored --nocapture`
//! 或单个：`... capture_submit_key -- --ignored --nocapture`

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// 拉起 agent，自动应答 DSR（ESC[6n），等首屏画完。返回（master、输出通道、child、cwd）。
/// TUI 会用 DSR 问光标位置，收不到回应就卡在启动上——真实终端由模拟器应答，这里替它答。
type AgentSession = (
    Box<dyn Write + Send>,
    mpsc::Receiver<Vec<u8>>,
    Box<dyn portable_pty::Child + Send + Sync>,
    std::path::PathBuf,
);

fn boot_agent() -> AgentSession {
    let exe = std::env::var("MEOWO_CAPTURE_EXE").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
        format!("{home}/.kimi-code/bin/kimi")
    });
    let cwd = std::env::temp_dir().join(format!("meowo-capture-{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut command = CommandBuilder::new(&exe);
    command.cwd(&cwd);
    command.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(command).unwrap();
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
    // 等首屏画完；期间应答 DSR。
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(1500)) {
            Ok(chunk) => {
                if chunk.windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = writer.write_all(b"\x1b[1;1R");
                    let _ = writer.flush();
                }
            }
            Err(_) => break,
        }
    }
    (writer, rx, child, cwd)
}

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

/// 实测「文字 → 隔多久 → 回车」中，间隔多短会导致回车被 composer 当成换行而非提交。
///
/// 现场：GUI 发 `/plan on` 后隔 20ms 补 `\r`，结果只换行没提交。斜杠命令要弹补全/校验，
/// composer 处理需要时间；`\r` 追太快就落进「还在编辑」的状态被当换行。这里逐个间隔试，
/// 找出可靠提交所需的最小延迟。用斜杠命令 `/plan on`（正是出问题的那条）当被测文字。
#[test]
#[ignore = "会拉起真实 agent 进程；仅供手动调研"]
fn capture_submit_key() {
    // 逐个候选提交键实测。**打印真实画面**由人判断，不靠「输出里没有原文=已提交」那种
    // 判据——输出为空时它恒真，会把读不到画面误判成成功（上一版探针正是栽在这上面）。
    let candidates: [(&str, &[u8]); 5] = [
        ("CR (\\r)", b"\r"),
        ("LF (\\n)", b"\n"),
        ("CRLF", b"\r\n"),
        // xterm 在 bracketed paste 之外发送 Enter 的常见形态；某些 TUI 只认 keypad Enter。
        ("ESC OM (keypad Enter)", b"\x1bOM"),
        ("CSI 13u (kitty Enter)", b"\x1b[13u"),
    ];
    for (label, key) in candidates {
        let (mut writer, rx, mut child, cwd) = boot_agent();
        std::thread::sleep(Duration::from_millis(1200));
        let _ = read_until_quiet(&rx, Duration::from_millis(400), Duration::from_secs(3));
        let cmd = "/plan on";
        if writer.write_all(cmd.as_bytes()).and_then(|_| writer.flush()).is_err() {
            eprintln!("### {label}: 跳过（进程不收输入）");
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(&cwd);
            continue;
        }
        std::thread::sleep(Duration::from_millis(500));
        // 记下按键前的画面，便于对照按键后的变化。
        let before = visible(&read_until_quiet(&rx, Duration::from_millis(400), Duration::from_secs(3)));
        let _ = writer.write_all(key).and_then(|_| writer.flush());
        let after = visible(&read_until_quiet(&rx, Duration::from_millis(1500), Duration::from_secs(10)));
        eprintln!("\n### {label}");
        eprintln!("--- 按键前(末4行) ---\n{}", tail_lines(&before, 4));
        eprintln!("--- 按键后(末8行) ---\n{}", tail_lines(&after, 8));
        eprintln!("--- 按键后原始字节(尾500) ---\n{:?}", tail(after.as_bytes(), 500));
        let _ = child.kill();
        let _ = std::fs::remove_dir_all(&cwd);
    }
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
