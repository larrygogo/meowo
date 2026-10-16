//! 脚本化真机取证：拉起任一 agent 的 TUI，按环境变量给的步骤发键、等屏、打印仿真后的整屏。
//! 改步骤不必重编译，取证对话页各操作（排队、模式键、审批框、草稿收起…）在各 CLI 上的真实反应。
//!
//! ```text
//! PROBE_AGENT=opencode PROBE_STEPS='wait=ask anything;;send=hello;;dump=typed' \
//!   cargo test -p meowo-app --test probe_tui_script -- --ignored --nocapture
//! ```
//! 步骤以 `;;` 分隔：`wait=文本`（等它出现在屏上，不分大小写，30s）、`gone=文本`（等它消失）、
//! `send=字节`（支持 `\r` `\t` `\n` `\x1b` 等转义）、`sleep=毫秒`、`dump=标签`（打印整屏）。
//! `PROBE_ARGS` 追加启动参数（空格分隔），`PROBE_CWD` 指定工作目录（默认临时目录，内含
//! notes.txt 供 @ 引用）。用的是用户真实的 CLI 配置与凭据——会话会真的调模型，步骤里的
//! prompt 保持极短。
//!
//! 与 probe_opencode_input 同样的三条纪律：打包 OpenConsole、应答 CPR 与 DA1、按 vt100 重建
//! 的屏幕判定（单元格差量重绘下，原始流里的字面串不可信）。

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

const ROWS: u16 = 40;
const COLS: u16 = 120;

fn use_bundled_conpty() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = source.join(name);
        assert!(
            from.exists(),
            "缺 {}，先跑 scripts/fetch-conpty.mjs",
            from.display()
        );
        if !target.join(name).exists() {
            std::fs::copy(&from, target.join(name)).unwrap();
        }
    }
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).unwrap() as char);
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[test]
#[ignore = "启动真实 CLI 并可能调用模型；手动取证用"]
fn tui_script() {
    use_bundled_conpty();
    let agent = std::env::var("PROBE_AGENT").expect("PROBE_AGENT");
    let steps = std::env::var("PROBE_STEPS").unwrap_or_default();
    let mut argv = meowo_agent::by_id(&agent).unwrap().launch_argv();
    if let Ok(extra) = std::env::var("PROBE_ARGS") {
        argv.extend(extra.split_whitespace().map(String::from));
    }
    let temp = std::env::temp_dir().join(format!("meowo-tui-script-{}", std::process::id()));
    let cwd = match std::env::var("PROBE_CWD") {
        Ok(dir) => std::path::PathBuf::from(dir),
        Err(_) => {
            std::fs::create_dir_all(&temp).unwrap();
            std::fs::write(temp.join("notes.txt"), "the secret word is PELICAN\n").unwrap();
            // gemini 关自更新只有 settings 一条路（无 env 开关）；项目级覆盖用户级，不改用户装机。
            std::fs::create_dir_all(temp.join(".gemini")).unwrap();
            std::fs::write(
                temp.join(".gemini/settings.json"),
                r#"{"general":{"enableAutoUpdate":false,"enableAutoUpdateNotification":false}}"#,
            )
            .unwrap();
            temp.clone()
        }
    };
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = common::agent_command(&argv[0], &cwd);
    command.args(&argv[1..]);
    // 数据库隔离落到临时目录（PROBE_CWD 可能是真实仓库，不往里写 board.db）。
    std::fs::create_dir_all(&temp).unwrap();
    command.env("MEOWO_DB", temp.join("board.db"));
    // opencode 默认会在会话里静默自更新（实测 1.18.32 跑一轮探针就升到了 1.19.2），取证不得改用户装机。
    command.env("OPENCODE_DISABLE_AUTOUPDATE", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(ROWS, COLS, 0)));
    let raw = Arc::new(Mutex::new(Vec::<u8>::new()));
    let (answer, parser, log) = (writer.clone(), screen.clone(), raw.clone());
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
                let _ = writer
                    .write_all(b"\x1b[?62;22c")
                    .and_then(|_| writer.flush());
            }
            drop(writer);
            parser.lock().unwrap().process(chunk);
            log.lock().unwrap().extend_from_slice(chunk);
        }
    });
    let contents = || screen.lock().unwrap().screen().contents();
    let dump = |label: &str| {
        let text = contents();
        let lines: Vec<&str> = text.lines().collect();
        let end = lines
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(0, |i| i + 1);
        println!("===== {label} =====");
        for line in &lines[..end] {
            println!("|{}", line.trim_end());
        }
    };
    let wait_until = |want: &dyn Fn(&str) -> bool| {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            if want(&contents().to_lowercase()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    let mut mark = 0;
    let mut failure = None;
    for step in steps.split(";;").map(str::trim).filter(|s| !s.is_empty()) {
        let (op, arg) = step.split_once('=').unwrap_or((step, ""));
        let ok = match op {
            "wait" => wait_until(&|s| s.contains(&arg.to_lowercase())),
            "gone" => wait_until(&|s| !s.contains(&arg.to_lowercase())),
            "send" => {
                let mut w = writer.lock().unwrap();
                w.write_all(unescape(arg).as_bytes())
                    .and_then(|_| w.flush())
                    .is_ok()
            }
            "sleep" => {
                std::thread::sleep(Duration::from_millis(arg.parse().unwrap()));
                true
            }
            "dump" => {
                dump(arg);
                true
            }
            // mark + raw：打印两步之间的原始输出（前端的回显/识别读的是增量流而非整屏）。
            "mark" => {
                mark = raw.lock().unwrap().len();
                true
            }
            "raw" => {
                let bytes = raw.lock().unwrap()[mark..].to_vec();
                println!(
                    "===== raw {arg} ({} bytes) =====
{}",
                    bytes.len(),
                    String::from_utf8_lossy(&bytes).escape_debug()
                );
                true
            }
            other => panic!("未知步骤 {other}"),
        };
        if !ok {
            failure = Some(step.to_string());
            break;
        }
    }
    dump("final");
    // 失败路径也必须收掉我们拉起的 CLI，别留进程锁与后台任务。
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(&temp);
    assert!(failure.is_none(), "步骤未达成：{}", failure.unwrap());
}
