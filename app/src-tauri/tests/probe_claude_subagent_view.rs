//! 取证 claude 的「子代理视图」：在提示框下方的子代理面板里打开某个子代理后，输入框改发给它
//! （占位 `Message @<类型>…`）。对话页发消息前要先切回主会话，必须弄清：面板长什么样、
//! 从输入框怎么把焦点挪进面板、怎么选中 `main` 行、回车后是否真的回到主会话。
//!
//! 不用 Esc 切回：2.1.322 源码里视图模式的 Esc 处理是「被看的子代理在跑 → 中断它的工作」，
//! 只有它已停下才退出视图（见 chat_ui::SubagentView 注释）。
//!
//! 隔离：CLAUDE_CONFIG_DIR 指到临时目录（只拷登录凭据与全局配置，不拷 settings.json，hook
//! 不会触发），DISABLE_AUTOUPDATER 防探针触发 CLI 自更新；cwd 也是临时目录，跑完全删。
//! 花费：一次主会话回合 + 一个只跑 `sleep` 的后台子代理。
//!
//! `cargo test -p meowo-app --test probe_claude_subagent_view -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

#[test]
#[ignore = "拉起真实 claude、派一个后台子代理再进出它的视图；手动调研用"]
fn claude_subagent_view_enter_and_leave() {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap();
    let exe = format!("{home}/.local/bin/claude.exe");
    let root = std::env::temp_dir().join(format!("meowo-sv-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let cwd = root.join("work");
    let config = root.join("config");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::copy(
        format!("{home}/.claude/.credentials.json"),
        config.join(".credentials.json"),
    )
    .unwrap();
    std::fs::copy(format!("{home}/.claude.json"), config.join(".claude.json")).unwrap();

    let (rows, cols) = (40u16, 120u16);
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = common::agent_command(&exe, &cwd);
    command.arg("--no-chrome");
    command.env_remove("CLAUDECODE");
    for (key, _) in std::env::vars() {
        if key.starts_with("CLAUDE_CODE") || (key.starts_with("MEOWO_") && key != "MEOWO_DB") {
            command.env_remove(key);
        }
    }
    command.env("CLAUDE_CONFIG_DIR", &config);
    command.env("DISABLE_AUTOUPDATER", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
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
    // 带粗体标记的整屏：被看的行是粗体，纯文本里分不出，取证时一并打出来。
    let bold_rows = || {
        let parser = screen.lock().unwrap();
        let screen = parser.screen();
        (0..rows)
            .filter_map(|row| {
                let text: String = (0..cols)
                    .filter_map(|col| screen.cell(row, col))
                    .map(|c| c.contents())
                    .collect();
                let bold = (0..cols)
                    .filter_map(|col| screen.cell(row, col))
                    .any(|c| c.bold() && !c.contents().trim().is_empty());
                (bold && !text.trim().is_empty()).then(|| format!("{row:>2}: {}", text.trim_end()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let send = |text: &str| {
        let mut writer = writer.lock().unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.flush().unwrap();
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
        let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
        eprintln!(
            "[probe] {tag} 屏幕:\n{}\n[probe] 粗体行:\n{}\n",
            lines.join("\n"),
            bold_rows()
        );
    };
    let key = |name: &str, data: &str| {
        send(data);
        std::thread::sleep(Duration::from_millis(1200));
        dump(&format!("按 {name} 后"));
    };

    let result = (|| -> Result<(), String> {
        if wait(30, &|| contents().contains("Yes, I trust this folder")) {
            std::thread::sleep(Duration::from_millis(800));
            send("\x1b[B");
            std::thread::sleep(Duration::from_millis(500));
            send("\r");
        }
        if !wait(30, &|| {
            !contents().contains("Yes, I trust this folder") && contents().contains('❯')
        }) {
            dump("启动");
            return Err("30s 内没等到 composer".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        send("用 Agent 工具派一个 general-purpose 子代理，run_in_background 设为 true，description 写 probe-sleeper，prompt 让它用 Bash 执行 sleep 150 后只回复 done。派出后你只回复：已派出");
        std::thread::sleep(Duration::from_millis(500));
        send("\r");
        if !wait(120, &|| {
            contents().contains("probe-sleeper") && contents().contains("已派出")
        }) {
            dump("派子代理");
            return Err("120s 内没看到子代理行".into());
        }
        std::thread::sleep(Duration::from_secs(3));
        if contents().contains("Teach auto mode about your environment?") {
            send("\x1b");
            std::thread::sleep(Duration::from_millis(1500));
        }
        dump("子代理在跑");
        // 进视图：↓ 进面板，再 ↓ 落到子代理行，回车打开。
        key("down#1", "\x1b[B");
        key("down#2", "\x1b[B");
        key("enter(打开子代理)", "\r");
        // 出视图：↓ 进面板（预期落在 main 行），回车切回主会话。
        key("down(回主)", "\x1b[B");
        key("enter(回主)", "\r");
        // 再进一次，换「↓ 后 ↑」看 main 行是不是面板第一行。
        key("down#3", "\x1b[B");
        key("down#4", "\x1b[B");
        key("enter(再开)", "\r");
        key("down#5", "\x1b[B");
        key("down#6", "\x1b[B");
        key("up#1", "\x1b[A");
        key("enter(回主2)", "\r");
        Ok(())
    })();
    let last_screen = contents();
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    std::thread::sleep(Duration::from_millis(500));
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        result.is_ok(),
        "{}；末屏=\n{last_screen}",
        result.unwrap_err()
    );
}
