use std::io::Write;
use std::process::{Command, Stdio};

use crate::term_script::{
    detect_term_kind, focus_script, normalize_tty, resume_script, resume_script_cwdless, TermKind,
};

/// 由 PID 取控制终端 tty，规范化为 /dev/ttysNNN。
fn tty_for_pid(pid: i64) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    normalize_tty(String::from_utf8_lossy(&out.stdout).trim())
}

/// 从 claude PID 沿父链收集进程名（claude 自身在前 → 祖先在后），用于判定终端宿主。
/// 单次 ps 快照后在内存里走 ppid —— macOS 上 sysinfo parent() 会过早断链（见 lib::pid_is_claude 注释），
/// 链一断就到不了 iTerm/Terminal，iTerm 多 tab 会话会被识成 Other 而无法聚焦，只能回退新开 Terminal。
fn ancestor_names(pid: i64) -> Vec<String> {
    let Ok(out) = Command::new("ps").args(["-axo", "pid=,ppid=,comm="]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    // pid -> (ppid, comm 全路径)。comm 在 macOS 上是可执行文件全路径，含 iTerm.app/Terminal.app 便于判定。
    let mut table: std::collections::HashMap<i64, (i64, String)> = std::collections::HashMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(p), Some(pp)) = (it.next(), it.next()) else { continue };
        let (Ok(p), Ok(pp)) = (p.parse::<i64>(), pp.parse::<i64>()) else { continue };
        table.insert(p, (pp, it.collect::<Vec<_>>().join(" ")));
    }
    let mut names = Vec::new();
    let mut cur = pid;
    for _ in 0..32 {
        let Some((ppid, comm)) = table.get(&cur) else { break };
        if !comm.is_empty() {
            names.push(comm.clone());
        }
        if *ppid <= 1 || *ppid == cur {
            break;
        }
        cur = *ppid;
    }
    names
}

/// 用 stdin 传脚本、argv 传参数地运行 osascript（防注入）。返回 stdout（trim）。
fn run_osascript(script: &str, args: &[&str]) -> std::io::Result<String> {
    let mut child = Command::new("osascript")
        .arg("-") // 从 stdin 读脚本
        .args(args) // 作为 on run argv 的参数
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 尝试切到 claude 进程所在的 Terminal.app/iTerm2 tab。命中返回 true。
fn focus_existing_tab(pid: i64) -> bool {
    let kind = detect_term_kind(&ancestor_names(pid));
    let (Some(tty), Some(script)) = (tty_for_pid(pid), focus_script(kind)) else {
        return false;
    };
    matches!(run_osascript(script, &[&tty]), Ok(r) if r == "FOUND")
}

/// 点连接中的卡片：切到该 claude 进程所在的终端 tab；未命中且有 session_id 则按 resume_kind 回退新开终端 resume。
pub fn focus_session_terminal(
    pid: i64,
    cwd: Option<&str>,
    session_id: Option<&str>,
    resume_kind: TermKind,
) {
    if focus_existing_tab(pid) {
        return;
    }
    if let Some(id) = session_id {
        resume_session_mac(cwd, id, resume_kind);
    }
}

/// 点已断开的卡片（或跳转回退）：按设置在 Terminal.app / iTerm2 新开窗口 claude --resume；有 cwd 则先 cd。
pub fn resume_session_mac(cwd: Option<&str>, session_id: &str, kind: TermKind) {
    match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            let _ = run_osascript(resume_script(kind), &[dir, session_id]);
        }
        _ => {
            let _ = run_osascript(resume_script_cwdless(kind), &[session_id]);
        }
    }
}
