use std::io::Write;
use std::process::{Command, Stdio};

/// 跑 meowo-reporter 二进制，喂给定 stdin 与可选 MEOWO_DB，返回退出码。
fn run_with(stdin: &str, db: Option<&str>) -> Option<i32> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_meowo-reporter"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(d) = db {
        cmd.env("MEOWO_DB", d);
    }
    let mut child = cmd.spawn().expect("spawn meowo-reporter");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let status = child.wait().expect("wait");
    status.code()
}

#[test]
fn empty_stdin_exits_zero() {
    assert_eq!(run_with("", None), Some(0));
}

#[test]
fn invalid_json_exits_zero() {
    assert_eq!(run_with("{not json", None), Some(0));
    assert_eq!(run_with("[]", None), Some(0));
    assert_eq!(run_with("null", None), Some(0));
}

#[test]
fn valid_event_exits_zero() {
    let tmp = std::env::temp_dir().join("meowo_reporter_exit_ok.db");
    let _ = std::fs::remove_file(&tmp);
    let json = r#"{"hook_event_name":"SessionStart","session_id":"exit-ok","cwd":"/tmp/x"}"#;
    assert_eq!(run_with(json, Some(tmp.to_str().unwrap())), Some(0));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn unopenable_db_path_still_exits_zero() {
    // 把 MEOWO_DB 指向一个已存在的"目录"，Connection::open 会失败，
    // 但 main 必须吞掉错误仍以 0 退出。
    let dir = std::env::temp_dir();
    let json = r#"{"hook_event_name":"SessionStart","session_id":"baddb","cwd":"/tmp/x"}"#;
    assert_eq!(run_with(json, Some(dir.to_str().unwrap())), Some(0));
}
