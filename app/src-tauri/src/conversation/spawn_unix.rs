//! native 的 stdio 直接继承监督进程管道；生命周期另走私有 socket，不与 JSON 协议混流。
use meowo_reporter::unix_supervisor as supervisor;
use std::io;
use std::os::unix::net::UnixStream;
#[cfg(target_os = "macos")]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "macos")]
use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
#[cfg(target_os = "macos")]
use supervisor::Launch;

pub(super) struct Child {
    process: std::process::Child,
    control: Option<UnixStream>,
    db: PathBuf,
    owner: String,
    native_pid: u32,
    proof: Arc<AtomicBool>,
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
}

impl Child {
    pub fn id(&self) -> u32 {
        self.native_pid
    }
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.process.try_wait()
    }
    pub fn stop(&mut self) -> bool {
        // shutdown 两端读写即可产生 EOF；不按可能复用的 supervisor/native PID 发信号。
        if let Some(control) = self.control.take() {
            let _ = control.shutdown(std::net::Shutdown::Both);
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if self.process.try_wait().ok().flatten().is_some() {
                let confirmed = supervisor::confirmed_after_stop(&self.db, &self.owner).is_ok();
                if confirmed {
                    self.proof.store(true, Ordering::Release);
                }
                return confirmed;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(super) fn spawn(
    command: &Command,
    owner: &str,
    proof: Arc<AtomicBool>,
) -> Result<Child, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (command, owner, proof);
        Err("conversation/platform_supervision_unsupported".into())
    }
    #[cfg(target_os = "macos")]
    {
        let db = super::super::owner::db_for(owner)?;
        let reporter = crate::setup::sibling_reporter();
        #[cfg(test)]
        let reporter = reporter.or_else(|| {
            // Cargo 的 integration test 会构建真实 reporter；app 测试 exe 则位于 deps 子目录。
            let binary = std::env::current_exe()
                .ok()?
                .parent()?
                .parent()?
                .join("meowo-reporter");
            binary
                .is_file()
                .then(|| binary.to_string_lossy().into_owned())
        });
        let reporter = reporter.ok_or("conversation/reporter_missing")?;
        let (parent, child) = UnixStream::pair().map_err(|_| "conversation/supervisor_failed")?;
        parent
            .set_read_timeout(Some(Duration::from_secs(12)))
            .map_err(|_| "conversation/supervisor_failed")?;
        parent
            .set_write_timeout(Some(Duration::from_secs(12)))
            .map_err(|_| "conversation/supervisor_failed")?;
        // F_DUPFD_CLOEXEC 避免 fd3 与宿主端相撞；pre_exec 中只执行 async-signal-safe dup2。
        let raw = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if raw < 0 {
            return Err("conversation/supervisor_failed".into());
        }
        use std::os::fd::FromRawFd;
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
        let mut helper = Command::new(reporter);
        helper
            .arg("conversation-supervisor")
            .arg(&db)
            .arg(owner)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        unsafe {
            helper.pre_exec(move || {
                if libc::dup2(raw, supervisor::CONTROL_FD) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        proof.store(false, Ordering::Release);
        let process = helper.spawn().map_err(|_| {
            proof.store(true, Ordering::Release);
            "conversation/supervisor_failed"
        })?;
        drop(fd);
        drop(child);
        let mut result = Child {
            process,
            control: Some(parent),
            db,
            owner: owner.into(),
            native_pid: 0,
            proof,
            stdin: None,
            stdout: None,
            stderr: None,
        };
        let launch = Launch {
            program: command.get_program().as_bytes().to_vec(),
            args: command
                .get_args()
                .map(|arg| arg.as_bytes().to_vec())
                .collect(),
            cwd: command
                .get_current_dir()
                .ok_or("conversation/cwd_invalid")?
                .as_os_str()
                .as_bytes()
                .to_vec(),
            env: command
                .get_envs()
                .map(|(key, value)| {
                    (
                        key.as_bytes().to_vec(),
                        value.map(|v| v.as_bytes().to_vec()),
                    )
                })
                .collect(),
        };
        let channel = result
            .control
            .as_mut()
            .ok_or("conversation/supervisor_failed")?;
        supervisor::send(channel, &launch).map_err(|_| "conversation/supervisor_failed")?;
        let prepared: String =
            supervisor::receive(channel).map_err(|_| "conversation/supervisor_failed")?;
        if prepared != "prepared" {
            return Err("conversation/supervisor_failed".into());
        }
        supervisor::send(channel, &"go").map_err(|_| "conversation/supervisor_failed")?;
        result.native_pid =
            supervisor::receive(channel).map_err(|_| "conversation/supervisor_failed")?;
        if result.native_pid == 0 {
            return Err("conversation/supervisor_failed".into());
        }
        result.stdin = result.process.stdin.take();
        result.stdout = result.process.stdout.take();
        result.stderr = result.process.stderr.take();
        Ok(result)
    }
}
