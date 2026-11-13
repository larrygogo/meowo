//! 取证：交互式 claude（TUI，跑在 PTY 里）空闲时，`asyncRewake` 后台 Stop hook 退出码 2
//! 能不能把模型唤醒。这是 Lead 关卡送达机制的前提——答复靠后台 hook 送回 agent，
//! 不往终端打字。`-p` 模式下已有实测（11 分钟可唤醒），交互式此前未验证。
//!
//! 做法：临时目录里放项目级 `.claude/settings.json`，Stop hook 设 asyncRewake，
//! 首次触发时睡 `REWAKE_SECS` 秒（默认 90，跨过 TUI 进入空闲），把倒序的暗号写到
//! stderr 并 exit 2；第二次起直接 exit 0，防止循环唤醒。模型只有真被唤醒、读到 stderr，
//! 才会在屏幕上写出正序暗号。
//!
//! `REWAKE_SECS=90 cargo test -p meowo-app --test probe_lead_rewake -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

const SECRET: &str = "PURPLE-42";

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

#[test]
#[ignore = "拉起真实 claude 交互进程并等待数分钟；手动调研用"]
fn probe_async_rewake_wakes_idle_tui() {
    let secs: u64 = std::env::var("REWAKE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90);
    // claude 的信任记录在 ~/.claude.json，别的 claude 进程会同时改写它，探针不碰。改为把目录放在
    // 已信任目录之下（PROBE_PARENT，例如主检出的 target/，已被 gitignore），看信任是否随父目录继承。
    // 信任不随父目录继承（实测），按方向键过信任屏也不稳（ESC 被当成「取消」直接退出）。
    // 所以工作目录用一个已经信任过的目录（PROBE_CWD，例如主检出），hook 走 --settings 单独的文件，
    // 不往那个目录写任何东西；临时文件全在 dir 里。
    let dir = std::env::temp_dir().join(format!("meowo-rewake-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let work = std::env::var("PROBE_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| dir.clone());
    let marker = dir
        .join("rewake.marker")
        .to_string_lossy()
        .replace('\\', "/");
    let reversed: String = SECRET.chars().rev().collect();
    // 只用系统自带的 PowerShell，不引入额外依赖。
    let ps = format!(
        "if (Test-Path '{marker}') {{ exit 0 }}; New-Item '{marker}' | Out-Null; Start-Sleep -Seconds {secs}; \
         [Console]::Error.WriteLine('LEAD-ANSWER {reversed} (把它倒过来写出来)'); exit 2"
    );
    let settings = serde_json::json!({
        "hooks": { "Stop": [ { "hooks": [ {
            "type": "command",
            "command": format!("powershell -NoProfile -Command \"{ps}\""),
            "asyncRewake": true,
            "timeout": secs + 600
        } ] } ] }
    });
    let settings_file = dir.join("probe-settings.json");
    std::fs::write(
        &settings_file,
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 40,
            cols: 140,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = common::agent_command("claude", &work);
    cmd.env("MEOWO_DB", dir.join("board.db")); // 不往工作目录里写 board.db
    cmd.arg("--settings");
    cmd.arg(settings_file.to_string_lossy().to_string());
    cmd.env_remove("CLAUDE_CODE_CHILD_SESSION");
    cmd.arg("只回复 READY 这一个词。之后如果收到 LEAD-ANSWER 开头的消息，按它说的做。");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
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
    let screen = || strip_ansi(&buf.lock().unwrap());

    let start = Instant::now();
    let mut trusted = false;
    let mut ready_at = None;
    let deadline = Duration::from_secs(secs + 300);
    let woke = loop {
        std::thread::sleep(Duration::from_millis(500));
        // TUI 用光标移动代替空格，剥掉控制序列后词会粘在一起，比对前去掉空白。
        let s: String = screen().chars().filter(|c| !c.is_whitespace()).collect();
        // 信任屏默认停在「No, exit」：下移一格到「Yes, I trust this folder」再回车（实测 2.1.319）。
        if !trusted && s.contains("Itrustthisfolder") && s.contains("Entertoconfirm") {
            writer.write_all(b"\x1b[B").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            writer.write_all(b"\r").unwrap();
            trusted = true;
        }
        if ready_at.is_none() && s.contains("READY") {
            ready_at = Some(start.elapsed());
            println!(
                "[probe] 首轮完成 READY @ {:?}，等待后台 hook 唤醒（{secs}s）",
                start.elapsed()
            );
        }
        if s.contains(SECRET) {
            break true;
        }
        if start.elapsed() > deadline {
            break false;
        }
    };
    let elapsed = start.elapsed();
    let _ = child.kill();
    let tail: String = screen()
        .chars()
        .rev()
        .take(1500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    println!(
        "[probe] 结果：{} @ {:?}（首轮 READY @ {:?}）\n----- 屏幕末尾 -----\n{tail}",
        if woke { "已唤醒" } else { "未唤醒" },
        elapsed,
        ready_at
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ready_at.is_some(), "首轮没有完成，探针无效");
    assert!(woke, "asyncRewake 没有在交互式 TUI 空闲时唤醒模型");
}
