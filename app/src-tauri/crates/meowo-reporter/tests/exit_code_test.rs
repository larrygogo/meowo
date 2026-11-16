use std::io::Write;
use std::process::{Command, Stdio};

/// 跑 meowo-reporter 二进制，喂给定 stdin 与可选 MEOWO_DB，返回退出码。
fn run_with(stdin: &str, db: Option<&str>) -> Option<i32> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_meowo-reporter"));
    // 清掉从外层托管终端继承的认领凭据：在 Meowo 托管的会话里跑 cargo test 时，喂进去的
    // SessionStart 会拿临时库里的会话 id 去真 broker 认领，把外层会话的终端换绑走
    // （实拍：会话 1537 被换代到不存在的会话 1，对话页从此显示「在外部终端运行」）。
    for key in ["MEOWO_PTY_ENDPOINT", "MEOWO_PTY_TOKEN", "MEOWO_PTY_LAUNCH", "MEOWO_PTY_PROTOCOL", "MEOWO_PTY_DB"] {
        cmd.env_remove(key);
    }
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

/// 库被别的连接长期占着写锁时，reporter 不能拖过 CLI 的 5 秒时限（2026-11-16 实拍
/// UserPromptSubmit 超时）：以 0 退出，并在库旁的 logs/reporter.log 留下这次慢的记录。
#[test]
fn locked_db_exits_in_time_and_logs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    // 先让 reporter 正常建一次库（建表、切 WAL），再由测试连接拿住写锁不放。
    let seed = r#"{"hook_event_name":"SessionStart","session_id":"lock-1","cwd":"/tmp/x"}"#;
    assert_eq!(run_with(seed, Some(db.to_str().unwrap())), Some(0));
    let holder = rusqlite::Connection::open(&db).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();

    let started = std::time::Instant::now();
    let json = r#"{"hook_event_name":"UserPromptSubmit","session_id":"lock-1","cwd":"/tmp/x","prompt":"hi"}"#;
    assert_eq!(run_with(json, Some(db.to_str().unwrap())), Some(0));
    let elapsed = started.elapsed();
    assert!(elapsed < std::time::Duration::from_millis(4_800), "必须在 5 秒内退出：{elapsed:?}");
    holder.execute_batch("ROLLBACK;").unwrap();

    let log = std::fs::read_to_string(dir.path().join("logs").join("reporter.log")).unwrap();
    assert!(log.contains("UserPromptSubmit · provider=claude"), "{log}");
}

/// 到时限还没做完，看门狗以 0 结束进程并记下停在哪一步。时限用环境变量压到 1ms 来触发。
#[test]
fn watchdog_ends_hook_at_budget_and_logs() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let json = r#"{"hook_event_name":"UserPromptSubmit","session_id":"dog-1","cwd":"/tmp/x","prompt":"hi"}"#;
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_meowo-reporter"));
    for key in ["MEOWO_PTY_ENDPOINT", "MEOWO_PTY_TOKEN", "MEOWO_PTY_LAUNCH", "MEOWO_PTY_PROTOCOL", "MEOWO_PTY_DB"] {
        cmd.env_remove(key);
    }
    cmd.env("MEOWO_DB", &db)
        .env("MEOWO_HOOK_BUDGET_MS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(json.as_bytes()).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    let log = std::fs::read_to_string(dir.path().join("logs").join("reporter.log")).unwrap();
    assert!(log.contains("到时限提前结束：UserPromptSubmit"), "{log}");
}

/// 平常的事件很快做完，不写慢日志。
#[test]
fn fast_event_leaves_no_slow_log() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let json = r#"{"hook_event_name":"SessionStart","session_id":"fast-1","cwd":"/tmp/x"}"#;
    assert_eq!(run_with(json, Some(db.to_str().unwrap())), Some(0));
    assert!(!dir.path().join("logs").join("reporter.log").exists());
}
