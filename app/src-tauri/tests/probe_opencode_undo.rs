//! 验证 opencode `/undo` 的真机行为，撤回功能接 opencode 全靠它（1.19.3 取证结论）：
//! - 库里只在 `session.revert` 记回退点 `messageID`，消息行原样留着，下次发言才真删；
//!   导出据此截断（store.rs 的 revert_point）；
//! - 不收参数、一次撤最近一轮；
//! - 工作区文件按快照一并回滚（agent 写的文件被删掉）；
//! - 原文放回 opencode 自己的输入框：多行时 Ctrl-U 只清当前行且在行首不并行、配退格也清
//!   不净，默认 keybind `input_clear` = Ctrl-C 整段清空；不清的话下一条消息会把它拼进去
//!   （实测模型又把文件建了回来）。
//!
//! 输入框识别与前端 `undoneInputRows` 同一规则（底边行之上连续以 `┃` 开头的行，数含原文
//! 末行开头的行），这里在真实屏幕上核对「撤回后多出、清空后回基线」。
//!
//! 用**真实**数据目录（隔离目录没有凭据，发不了模型请求）：会花三次模型请求，并在 opencode
//! 库里留下一个 cwd 为临时目录的探针会话。
//!
//! `cargo test -p meowo-app --test probe_opencode_undo -- --ignored --nocapture`

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

/// 同 probe_opencode_input：生产加载打包的 OpenConsole，测试 exe 旁不放它会退回系统 conhost。
fn use_bundled_conpty() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
    let target = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = source.join(name);
        assert!(from.exists(), "缺 {}，先跑 scripts/fetch-conpty.mjs", from.display());
        if !target.join(name).exists() {
            std::fs::copy(&from, target.join(name)).unwrap();
        }
    }
}

/// 探针只读查库。meowo-app 不直接依赖 rusqlite，借本机 python 的 sqlite3（只读 URI）。
fn query(sql: &str, arg: &str) -> Vec<Vec<serde_json::Value>> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap();
    let db = PathBuf::from(home).join(".local/share/opencode/opencode.db");
    let script = concat!(
        "import sqlite3,sys,json\n",
        "c=sqlite3.connect('file:'+sys.argv[1].replace(chr(92),'/')+'?mode=ro',uri=True,timeout=2)\n",
        "print(json.dumps([list(r) for r in c.execute(sys.argv[2],(sys.argv[3],) if '?' in sys.argv[2] else ())]))",
    );
    let out = std::process::Command::new("python")
        .args(["-I", "-c", script, &db.to_string_lossy(), sql, arg])
        .output()
        .expect("需要本机 python");
    if !out.status.success() {
        eprintln!("[probe] python 查库失败: {}", String::from_utf8_lossy(&out.stderr));
    }
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

/// 本次探针会话：cwd 等于临时目录的那条。
fn probe_session(cwd: &Path) -> Option<String> {
    // opencode 存正斜杠（实测 1.19.3）。
    let norm = |p: &str| p.replace('\\', "/").to_lowercase();
    let want = norm(&cwd.to_string_lossy());
    query("SELECT id, directory FROM session ORDER BY time_created DESC LIMIT 20", "")
        .into_iter()
        .find(|row| row[1].as_str().is_some_and(|d| norm(d) == want))
        .and_then(|row| row[0].as_str().map(str::to_string))
}

/// (role, id, completed) 按时间序。
fn messages(session: &str) -> Vec<(String, String, bool)> {
    query("SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created, id", session)
        .into_iter()
        .map(|row| {
            let v: serde_json::Value = serde_json::from_str(row[1].as_str().unwrap_or("{}")).unwrap_or_default();
            let role = v["role"].as_str().unwrap_or("?").to_string();
            (role, row[0].as_str().unwrap_or("").to_string(), v.pointer("/time/completed").is_some())
        })
        .collect()
}

fn revert(session: &str) -> Option<String> {
    query("SELECT revert FROM session WHERE id = ?", session)
        .first()
        .and_then(|row| row[0].as_str().map(str::to_string))
}

/// 与前端 undoneInputRows 同规则：底边行（`╹`）之上连续以 `┃` 开头的行里，含原文末行开头
/// 一小段的行数；认不出输入框返回 None。
fn input_rows(screen: &str, text: &str) -> Option<usize> {
    let snippet: String = text.lines().map(str::trim).rfind(|l| !l.is_empty())?.chars().take(12).collect();
    let lines: Vec<&str> = screen.lines().collect();
    let bottom = lines.iter().rposition(|l| l.trim_start().starts_with('╹'))?;
    let block: Vec<&&str> = lines[..bottom].iter().rev().take_while(|l| l.trim_start().starts_with('┃')).collect();
    (!block.is_empty()).then(|| block.iter().filter(|l| l.contains(&snippet)).count())
}

#[test]
#[ignore = "拉起真实 opencode、发一条会写文件的消息再 /undo；手动调研用"]
fn opencode_undo_records_revert_and_restores_files() {
    use_bundled_conpty();
    let argv = meowo_agent::by_id("opencode").unwrap().launch_argv();
    let cwd = std::env::temp_dir().join(format!("meowo-oc-undo-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cwd);
    std::fs::create_dir_all(&cwd).unwrap();
    // 快照依赖项目识别，给个 git 仓库（有一次提交）贴近真实使用。
    std::fs::write(cwd.join("seed.txt"), "seed\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "."], &["-c", "user.name=p", "-c", "user.email=p@p", "commit", "-qm", "seed"]] {
        assert!(std::process::Command::new("git").args(args).current_dir(&cwd).status().unwrap().success());
    }

    let pair = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    let mut command = common::agent_command(&argv[0], &cwd);
    command.args(&argv[1..]);
    command.env("OPENCODE_DISABLE_AUTOUPDATE", "1");
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
                let _ = writer.write_all(b"\x1b[?62;22c").and_then(|_| writer.flush());
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
        std::thread::sleep(Duration::from_millis(300));
        send("\r");
    };
    let wait = |secs: u64, want: &dyn Fn() -> bool| {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if want() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    };
    let dump = |tag: &str| {
        let text = contents();
        let lines: Vec<&str> = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
        eprintln!("[probe] {tag} 末屏:\n{}", lines[lines.len().saturating_sub(8)..].join("\n"));
    };
    let file = cwd.join("probe.txt");
    // 多行原文（LF 是 opencode 的换行键）：单行 Ctrl-U 就能清，多行才见真章。
    let prompt = "用 write 工具创建 probe.txt，内容为 hi。\n完成后只回复一个字：好";

    let result = (|| -> Result<(), String> {
        if !wait(30, &|| contents().to_lowercase().contains("ask anything")) {
            return Err("首屏未出现输入框".into());
        }
        // 第一轮纯问答，第二轮写文件：撤一次应只撤第二轮，并看文件是否随之还原。
        submit("只回复一个字：好");
        let session = {
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut found = None;
            while found.is_none() && Instant::now() < deadline {
                found = probe_session(&cwd);
                std::thread::sleep(Duration::from_millis(250));
            }
            found.ok_or("30s 内库里没出现本次会话")?
        };
        let settled = |n: usize| {
            let msgs = messages(&session);
            msgs.iter().filter(|m| m.0 == "user").count() == n
                && msgs.last().is_some_and(|m| m.0 == "assistant" && m.2)
                && !contents().to_lowercase().contains("esc interrupt")
        };
        if !wait(120, &|| settled(1)) {
            return Err("第一轮 120s 未完成".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        submit(prompt);
        if !wait(180, &|| settled(2) && file.exists()) {
            return Err(format!("第二轮 180s 未完成；文件存在={}", file.exists()));
        }
        std::thread::sleep(Duration::from_secs(2));
        let before = messages(&session);
        let baseline = input_rows(&contents(), prompt);
        eprintln!("[probe] 撤回前 {} 条消息；输入框原文行基线={baseline:?}", before.len());

        submit("/undo");
        if !wait(15, &|| revert(&session).is_some()) {
            return Err("15s 内 session.revert 没出现——/undo 的落库方式可能变了".into());
        }
        std::thread::sleep(Duration::from_secs(2));
        let point = revert(&session).unwrap_or_default();
        let after_undo = messages(&session);
        let rows_after_undo = input_rows(&contents(), prompt);
        dump("/undo 后");
        eprintln!("[probe] revert={point}\n        消息 {} 条；probe.txt 存在={}；输入框原文行={rows_after_undo:?}", after_undo.len(), file.exists());
        if after_undo.len() != before.len() {
            return Err("消息行在 /undo 时就被删了——与「只记回退点」的取证不符".into());
        }
        if !point.contains(&before[2].1) {
            return Err(format!("回退点应是第二条用户消息 {}", before[2].1));
        }
        if file.exists() {
            return Err("文件没随撤回还原".into());
        }
        let (Some(baseline), Some(rows)) = (baseline, rows_after_undo) else {
            return Err("认不出输入框（┃/╹ 形态变了）".into());
        };
        if rows <= baseline {
            return Err("撤回后输入框里没多出原文".into());
        }

        send("\x03");
        std::thread::sleep(Duration::from_millis(800));
        dump("Ctrl-C 后");
        let rows_after_clear = input_rows(&contents(), prompt);
        eprintln!("[probe] Ctrl-C 后输入框原文行={rows_after_clear:?}");
        if rows_after_clear != Some(baseline) {
            return Err("Ctrl-C 后输入框没回到基线".into());
        }

        // 清空后正常发言：回退点之后的消息此刻才被清理，revert 归零，文件不会被残留原文再建回来。
        submit("只回复一个字：行");
        wait(120, &|| settled(2) && revert(&session).is_none());
        std::thread::sleep(Duration::from_secs(2));
        let last = messages(&session);
        eprintln!("[probe] 再发言后 revert={:?} 消息 {} 条；probe.txt 存在={}", revert(&session), last.len(), file.exists());
        if revert(&session).is_some() || last.iter().any(|m| m.1 == before[2].1) || file.exists() {
            return Err("再发言后回退点之后的消息没清理，或文件被建回来了".into());
        }
        Ok(())
    })();
    let last_screen = contents();
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(&cwd);
    assert!(result.is_ok(), "{}；末屏=\n{last_screen}", result.unwrap_err());
}
