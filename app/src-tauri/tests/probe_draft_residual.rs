//! issue #72 调研：对话页发送时 composer 里残留的终端草稿会拼进消息。
//! 探 claude TUI 能否「收走草稿 → 发送 → 原样还回」。全程只提交本地命令 /help，不产生对话。
//!
//! 结论（2026-09-23 真机实测，Claude Code 2.1.280 / Windows）：
//!   - Ctrl-U / Ctrl-Y（kill/yank）**不可用**：多行草稿 Ctrl-U 只剪当前行；且空 composer
//!     的 Ctrl-U 不清 kill 缓冲，随后 Ctrl-Y 会把用户更早删掉的文本粘出来（误注入）。
//!   - Ctrl-S（stash）可用：多行草稿整段收走（提示 `› stashed`），下一次提交后 CLI 自动
//!     还原（提示 `Draft restored`）。
//!   - Ctrl-S 是切换键：composer 空且无暂存 = 无操作；composer 空但**已有暂存** = 恢复
//!     暂存到 composer——盲按会把用户手动暂存的内容拼进消息。故空 composer 一律不按
//!     （空时整屏上 composer 首行是独行 `❯`，上一行是 `─` 横线）。
//!   - 暂存只有一份：已有暂存时再暂存会覆盖旧的（T4/T5）。
//!   - `› stashed` 是「当前有暂存」的常驻标记，`Draft restored` 是会滞留的通知。渲染器
//!     按字符差分重绘，提示行不变时增量输出里一个字节都没有——判断必须看仿真后的整屏，
//!     不能扫增量字节（生产侧因此新增 managed_terminal_screen 命令）。
//!   - 启动须带 `--no-chrome`：Chrome 集成确认框挡在 composer 前，按 Esc 会改用户配置。
//!
//! `cargo test -p meowo-app --test probe_draft_residual -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod common;

struct Pty {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: mpsc::Receiver<Vec<u8>>,
    parser: vt100::Parser,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Pty {
    fn pump(&mut self, secs: f32) {
        let end = Instant::now() + Duration::from_secs_f32(secs);
        while Instant::now() < end {
            if let Ok(c) = self.rx.recv_timeout(Duration::from_millis(100)) {
                let mut out: Vec<u8> = Vec::new();
                if c.windows(4).any(|w| w == b"\x1b[6n") {
                    out.extend_from_slice(b"\x1b[1;1R");
                }
                if c.windows(3).any(|w| w == b"\x1b[c") || c.windows(4).any(|w| w == b"\x1b[0c") {
                    out.extend_from_slice(b"\x1b[?1;2c");
                }
                if !out.is_empty() {
                    let _ = self.writer.write_all(&out).and_then(|_| self.writer.flush());
                }
                self.parser.process(&c);
            }
        }
    }
    fn send(&mut self, bytes: &[u8], settle: f32) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
        self.pump(settle);
    }
    /// composer 区域：从末尾往上取非空行。
    fn screen(&self) -> String {
        let contents = self.parser.screen().contents();
        let lines: Vec<&str> = contents.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
        lines[lines.len().saturating_sub(8)..].join("\n")
    }
}

fn spawn(exe: &str, cwd: &std::path::Path) -> Pty {
    let pair = native_pty_system()
        .openpty(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut cmd = common::agent_command(exe, cwd);
    // 不弹 Chrome 集成确认框（它挡在 composer 前，按 Esc 会改用户配置）。全程不回车，不产生对话。
    cmd.arg("--no-chrome");
    cmd.env_remove("CLAUDECODE");
    let child = pair.slave.spawn_command(cmd).unwrap();
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Pty { child, writer, rx, parser: vt100::Parser::new(30, 100, 0), _master: pair.master }
}

#[test]
#[ignore = "拉起真实 claude 进程；手动调研用"]
fn probe_claude_draft_kill_yank() {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
    let exe = format!("{home}/.local/bin/claude.exe");
    // 用已信任的仓库目录启动，避开首次信任确认框。
    let cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut pty = spawn(&exe, &cwd);
    pty.pump(12.0);
    eprintln!("=== 启动后\n{}\n", pty.screen());

    let steps: &[(&str, &[u8])] = &[
        // kill/yank：单行可剪可粘，但空 composer 的 Ctrl-U 不清 kill 缓冲。
        ("K1 键入 zzz 后 Ctrl-U", b"zzz\x15"),
        ("K2 空 composer 再 Ctrl-U", b"\x15"),
        ("K3 Ctrl-Y → 粘出旧的 zzz（误注入）", b"\x19"),
        ("K4 多行草稿后 Ctrl-U → 只剪当前行", b"\x15line1\nline2\x15"),
        ("K5 清场", b"\x7f\x15"),
        // stash：多行整段收走，提交后自动恢复。
        ("S1 多行草稿 + Ctrl-S → › stashed", b"line1\nline2\x13"),
        ("S2 键入 /help", b"/help"),
        ("S3 Enter（本地命令，不调模型）", b"\r"),
        ("S4 Esc → 草稿自动恢复", b"\x1b"),
        // 切换语义：空 composer + 有暂存时 Ctrl-S = 恢复。
        ("T1 再 Ctrl-S 暂存", b"\x13"),
        ("T2 空 composer 时 Ctrl-S → Draft restored", b"\x13"),
        ("T3 再 Ctrl-S（收回）", b"\x13"),
        ("T4 有暂存时键入 hi 再 Ctrl-S → 覆盖旧暂存", b"hi\x13"),
        ("T5 空 composer Ctrl-S → 恢复出 hi，line1/line2 已丢", b"\x13"),
    ];
    for (label, bytes) in steps {
        pty.send(bytes, 1.2);
        let hint = pty
            .parser
            .screen()
            .contents()
            .lines()
            .filter(|l| l.contains("stashed") || l.contains("restored"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join(" | ");
        eprintln!("=== {label}\n{}\n[提示行] {hint}\n", pty.screen());
    }
    let _ = pty.child.kill();
}
