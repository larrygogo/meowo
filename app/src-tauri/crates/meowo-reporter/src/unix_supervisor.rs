//! 独立监督进程保留 native leader 的 wait 权；宿主崩溃不能把整组收尾变成无主 PID 猜测。
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const MAX_FRAME: usize = 1024 * 1024;
pub const CONTROL_FD: i32 = 3;

#[cfg(target_os = "macos")]
fn boot_id() -> io::Result<String> {
    let mut bytes = [0u8; 128];
    let mut size = bytes.len();
    // kern.bootsessionuuid 由内核为本次 boot 生成；不能用墙钟/uptime 推测是否重启。
    if unsafe {
        libc::sysctlbyname(
            c"kern.bootsessionuuid".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if size != 37 || bytes[36] != 0 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let text = std::str::from_utf8(&bytes[..36]).map_err(|_| io::ErrorKind::InvalidData)?;
    if !text.bytes().enumerate().all(|(i, b)| {
        if [8, 13, 18, 23].contains(&i) {
            b == b'-'
        } else {
            b.is_ascii_hexdigit()
        }
    }) {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(text.to_ascii_lowercase())
}

#[cfg(not(target_os = "macos"))]
fn boot_id() -> io::Result<String> {
    Err(io::ErrorKind::Unsupported.into())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub program: Vec<u8>,
    pub args: Vec<Vec<u8>>,
    pub cwd: Vec<u8>,
    pub env: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

pub fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)
}

pub fn receive<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let mut size = [0; 4];
    stream.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size > MAX_FRAME {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn private(path: &Path, directory: bool) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
        || meta.file_type().is_symlink()
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

pub fn lease(db: &Path, owner: &str) -> io::Result<PathBuf> {
    let token = owner
        .strip_prefix("u1.")
        .ok_or(io::ErrorKind::InvalidInput)?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let root = db
        .parent()
        .ok_or(io::ErrorKind::InvalidInput)?
        .canonicalize()?
        .join(".conversation-leases");
    private(&root, true)?;
    let path = root.join(token);
    private(&path, true)?;
    Ok(path)
}

pub fn lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    private(path, false)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

pub fn prepare(db: &Path, owner: &str) -> io::Result<File> {
    let boot = boot_id()?;
    let root = db
        .parent()
        .ok_or(io::ErrorKind::InvalidInput)?
        .canonicalize()?
        .join(".conversation-leases");
    match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    private(&root, true)?;
    let token = owner
        .strip_prefix("u1.")
        .ok_or(io::ErrorKind::InvalidInput)?;
    if token.len() != 64
        || !token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let dir = root.join(token);
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    File::open(&root)?.sync_all()?;
    for name in ["host.lock", "supervisor.lock"] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(dir.join(name))?;
    }
    let host = lock(&dir.join("host.lock"))?;
    let mut boot_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(dir.join("boot"))?;
    boot_file.write_all(boot.as_bytes())?;
    boot_file.sync_all()?;
    receipt(&dir, owner, "vacant")?;
    Ok(host)
}

fn receipt(dir: &Path, owner: &str, state: &str) -> io::Result<()> {
    let temp = dir.join("receipt.next");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    write!(file, "{owner}\n{state}\n")?;
    file.sync_all()?;
    std::fs::rename(temp, dir.join("receipt"))?;
    File::open(dir)?.sync_all()
}

pub fn confirmed(db: &Path, owner: &str) -> io::Result<()> {
    let dir = lease(db, owner)?;
    let _host = lock(&dir.join("host.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(_supervisor) = lock(&dir.join("supervisor.lock")) {
            private(&dir.join("boot"), false)?;
            let mut boot = String::new();
            File::open(dir.join("boot"))?
                .take(128)
                .read_to_string(&mut boot)?;
            let current = boot_id()?;
            if boot.len() == 36
                && boot.bytes().enumerate().all(|(i, b)| {
                    if [8, 13, 18, 23].contains(&i) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
                && boot.to_ascii_lowercase() != current
            {
                // 旧 boot 的 PGID/PID 已不属于当前内核；不对当前任何同号进程发信号。
                return Ok(());
            }
            private(&dir.join("receipt"), false)?;
            let mut text = String::new();
            File::open(dir.join("receipt"))?
                .take(256)
                .read_to_string(&mut text)?;
            return if text == format!("{owner}\nvacant\n") || text == format!("{owner}\nstopped\n")
            {
                Ok(())
            } else {
                Err(io::ErrorKind::Other.into())
            };
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn confirmed_after_stop(db: &Path, owner: &str) -> io::Result<()> {
    let dir = lease(db, owner)?;
    let _supervisor = lock(&dir.join("supervisor.lock"))?;
    private(&dir.join("receipt"), false)?;
    let mut text = String::new();
    File::open(dir.join("receipt"))?
        .take(256)
        .read_to_string(&mut text)?;
    if text == format!("{owner}\nstopped\n") {
        Ok(())
    } else {
        Err(io::ErrorKind::Other.into())
    }
}

fn leader_exited(child: &Child) -> io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    if unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } != 0)
}

#[cfg(target_os = "macos")]
fn only_leader(pid: u32) -> io::Result<bool> {
    // Apple xnu proc_listpids 返回字节数；0 同时代表错误，绝不能当空组。
    // xnu proc_info.c 的 proc_list_lock 同时保护 allproc/zombproc 遍历；仅精确一项可通过。
    // 内核缓冲至少 nprocs+20，故返回一项不可能是其内部截断；0/满缓冲一律拒绝。
    // 来源：apple-oss-distributions/xnu bsd/kern/proc_info.c::proc_listpids。
    let mut pids = vec![0i32; 65536];
    let capacity = (pids.len() * std::mem::size_of::<i32>()) as i32;
    let bytes = unsafe { libc::proc_listpids(2, pid, pids.as_mut_ptr().cast(), capacity) };
    if bytes <= 0 || bytes >= capacity || !(bytes as usize).is_multiple_of(4) {
        return Err(io::ErrorKind::Other.into());
    }
    pids.truncate(bytes as usize / 4);
    pids.retain(|p| *p != 0);
    Ok(pids == [pid as i32])
}

#[cfg(not(target_os = "macos"))]
fn only_leader(_pid: u32) -> io::Result<bool> {
    // Linux 暂无等价的原子完整组查询，不能用 /proc 瞬时缺席伪造 whole-group 证明。
    Err(io::ErrorKind::Unsupported.into())
}

fn terminate(child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if leader_exited(child)? && only_leader(child.id())? {
            child.wait()?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn run(args: &[String]) -> io::Result<()> {
    if args.len() != 3 {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let owner = &args[2];
    // 若宿主环境曾忽略 SIGCHLD，必须恢复默认；自动回收会破坏 WNOWAIT 的 PID 保留证明。
    if unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) } == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    let dir = lease(Path::new(&args[1]), owner)?;
    let _guard = lock(&dir.join("supervisor.lock"))?;
    let mut channel = unsafe { UnixStream::from_raw_fd(CONTROL_FD) };
    // native 不继承 control 写端/宿主锁；否则宿主崩溃后 EOF 永远不到。
    if unsafe { libc::fcntl(CONTROL_FD, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    channel.set_read_timeout(Some(Duration::from_secs(10)))?;
    let launch: Launch = match receive(&mut channel) {
        Ok(launch) => launch,
        Err(error) => {
            receipt(&dir, owner, "stopped")?;
            return Err(error);
        }
    };
    receipt(&dir, owner, "starting")?;
    if let Err(error) = send(&mut channel, &"prepared") {
        receipt(&dir, owner, "stopped")?;
        return Err(error);
    }
    let go: String = match receive(&mut channel) {
        Ok(go) => go,
        Err(error) => {
            receipt(&dir, owner, "stopped")?;
            return Err(error);
        }
    };
    if go != "go" {
        receipt(&dir, owner, "stopped")?;
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut command = Command::new(std::ffi::OsString::from_vec(launch.program));
    command
        .args(launch.args.into_iter().map(std::ffi::OsString::from_vec))
        .current_dir(std::ffi::OsString::from_vec(launch.cwd));
    for (key, value) in launch.env {
        let key = std::ffi::OsString::from_vec(key);
        match value {
            Some(value) => {
                command.env(key, std::ffi::OsString::from_vec(value));
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .process_group(0);
    channel.set_read_timeout(Some(Duration::from_millis(100)))?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            receipt(&dir, owner, "stopped")?;
            return Err(error);
        }
    };
    let started = send(&mut channel, &child.id());
    if started.is_ok() {
        let mut byte = [0];
        loop {
            match channel.read(&mut byte) {
                Ok(_) => break,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => break,
            }
            match leader_exited(&child) {
                Ok(false) => {}
                _ => break,
            }
        }
    }
    terminate(&mut child)?;
    receipt(&dir, owner, "stopped")
}
