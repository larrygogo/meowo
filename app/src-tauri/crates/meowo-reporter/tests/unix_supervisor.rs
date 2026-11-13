#![cfg(target_os = "macos")]

use meowo_reporter::unix_supervisor::{self as supervisor, Launch};
use std::io::{BufRead, BufReader};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const OWNER: &str = "u1.0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn prepared(db: &Path, script: &str) -> (Child, UnixStream) {
    let (mut parent, child) = UnixStream::pair().unwrap();
    parent
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let raw = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
    assert!(raw >= 0);
    use std::os::fd::FromRawFd;
    let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
    let mut command = Command::new(env!("CARGO_BIN_EXE_meowo-reporter"));
    command
        .args(["conversation-supervisor", db.to_str().unwrap(), OWNER])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // child fd 在 exec 后只保留 fd3；控制流不借用 native stdin。
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(raw, supervisor::CONTROL_FD) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(supervisor::CONTROL_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let process = command.spawn().unwrap();
    drop(fd);
    drop(child);
    supervisor::send(
        &mut parent,
        &Launch {
            program: b"/bin/sh".to_vec(),
            args: vec![b"-c".to_vec(), script.as_bytes().to_vec()],
            cwd: db.parent().unwrap().as_os_str().as_encoded_bytes().to_vec(),
            env: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        supervisor::receive::<String>(&mut parent).unwrap(),
        "prepared"
    );
    (process, parent)
}

fn start(db: &Path, script: &str) -> (Child, UnixStream, u32) {
    let (process, mut channel) = prepared(db, script);
    supervisor::send(&mut channel, &"go").unwrap();
    let native = supervisor::receive(&mut channel).unwrap();
    (process, channel, native)
}

#[test]
fn eof_before_bootstrap_ack_does_not_start_native() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let host = supervisor::prepare(&db, OWNER).unwrap();
    let (mut helper, control) = prepared(&db, "touch should-not-exist");
    drop(control);
    assert!(!helper.wait().unwrap().success());
    assert!(!dir.path().join("should-not-exist").exists());
    supervisor::confirmed_after_stop(&db, OWNER).unwrap();
    drop(host);
    supervisor::confirmed(&db, OWNER).unwrap();
}

#[test]
fn oversized_control_frame_is_rejected_before_allocation_or_native_spawn() {
    use std::io::Write;
    let (mut writer, mut reader) = UnixStream::pair().unwrap();
    writer.write_all(&u32::MAX.to_be_bytes()).unwrap();
    assert!(supervisor::receive::<Launch>(&mut reader).is_err());
}

#[test]
fn eof_stops_the_whole_group_and_live_host_lock_blocks_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let host = supervisor::prepare(&db, OWNER).unwrap();
    assert!(supervisor::confirmed(&db, OWNER).is_err());
    let (mut helper, control, _) = start(&db, "sleep 600 & echo READY; wait");
    let mut ready = String::new();
    BufReader::new(helper.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "READY");
    drop(control);
    assert!(helper.wait().unwrap().success());
    supervisor::confirmed_after_stop(&db, OWNER).unwrap();
    assert!(supervisor::confirmed(&db, OWNER).is_err());
    drop(host);
    supervisor::confirmed(&db, OWNER).unwrap();
}

#[test]
#[ignore = "只由下面的隔离父测试调用，模拟宿主突然退出"]
fn crash_host_fixture() {
    let root = std::env::var_os("MEOWO_TEST_UNIX_SUPERVISOR_ROOT").unwrap();
    let db = Path::new(&root).join("board.db");
    let _host = supervisor::prepare(&db, OWNER).unwrap();
    let (mut helper, control, _) = start(&db, "sleep 600 & echo READY; wait");
    let mut ready = String::new();
    BufReader::new(helper.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "READY");
    std::fs::write(Path::new(&root).join("ready"), b"ready").unwrap();
    std::mem::forget((helper, control));
    unsafe { libc::_exit(23) }
}

#[test]
fn abrupt_host_exit_recovers_without_persisting_a_native_pid() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_host_fixture", "--ignored"])
        .env("MEOWO_TEST_UNIX_SUPERVISOR_ROOT", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(23));
    assert!(dir.path().join("ready").is_file());
    supervisor::confirmed(&dir.path().join("board.db"), OWNER).unwrap();
}

#[test]
fn crashed_supervisor_never_creates_an_exit_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let host = supervisor::prepare(&db, OWNER).unwrap();
    // 有限生命周期 native，测试不会遗留常驻进程，也不按 helper 死亡猜整组已停。
    let (mut helper, control, _) = start(&db, "sleep 1");
    helper.kill().unwrap();
    helper.wait().unwrap();
    drop(control);
    drop(host);
    std::thread::sleep(Duration::from_millis(1200));
    assert!(supervisor::confirmed(&db, OWNER).is_err());
}

#[test]
fn vacant_and_invalid_leases_are_distinguished() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let host = supervisor::prepare(&db, OWNER).unwrap();
    drop(host);
    supervisor::confirmed(&db, OWNER).unwrap();
    assert!(supervisor::confirmed(&db, "u1.../elsewhere").is_err());
    let lease = supervisor::lease(&db, OWNER).unwrap();
    std::fs::remove_file(lease.join("receipt")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", lease.join("receipt")).unwrap();
    assert!(supervisor::confirmed(&db, OWNER).is_err());
}

#[test]
fn different_boot_identity_can_retire_an_interrupted_lease_without_signalling() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("board.db");
    let host = supervisor::prepare(&db, OWNER).unwrap();
    drop(host);
    let lease = supervisor::lease(&db, OWNER).unwrap();
    let mut boot = std::fs::read(lease.join("boot")).unwrap();
    boot[0] = if boot[0] == b'0' { b'1' } else { b'0' };
    // 仅注入私有 fixture 的上一轮 boot 身份，覆盖 reboot 分支，不伪称真机重启证据。
    std::fs::write(lease.join("boot"), boot).unwrap();
    std::fs::write(lease.join("receipt"), format!("{OWNER}\nstarting\n")).unwrap();
    supervisor::confirmed(&db, OWNER).unwrap();
}
