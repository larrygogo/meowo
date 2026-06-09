use std::io::Write;
use std::process::{Command, Stdio};

use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};

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

/// 从 claude PID 向祖先收集进程名（root-first），用于判定终端宿主。
fn ancestor_names(pid: i64) -> Vec<String> {
    let sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::new()),
    );
    let mut names = Vec::new();
    let mut cur = Pid::from_u32(pid as u32);
    for _ in 0..32 {
        let Some(proc_) = sys.process(cur) else { break };
        names.push(proc_.name().to_string_lossy().to_string());
        match proc_.parent() {
            Some(p) => cur = p,
            None => break,
        }
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

/// 点连接中的卡片：切到该 claude 进程所在的终端 tab；未知宿主或未命中且有 session_id 则回退新开 Terminal resume。
pub fn focus_session_terminal(pid: i64, cwd: Option<&str>, session_id: Option<&str>) {
    if focus_existing_tab(pid) {
        return;
    }
    if let Some(id) = session_id {
        resume_session_mac(cwd, id);
    }
}

/// 点已断开的卡片（或跳转回退）：默认在 Terminal.app 新开窗口 claude --resume；有 cwd 则先 cd。
pub fn resume_session_mac(cwd: Option<&str>, session_id: &str) {
    match cwd {
        Some(dir) if !dir.trim().is_empty() => {
            let _ = run_osascript(resume_script(TermKind::Terminal), &[dir, session_id]);
        }
        _ => {
            let _ = run_osascript(resume_script_cwdless(TermKind::Terminal), &[session_id]);
        }
    }
}
