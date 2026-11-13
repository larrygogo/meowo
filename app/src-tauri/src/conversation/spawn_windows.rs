//! Job在CreateProcessW内原子绑定：宿主在返回前崩溃也不会留下尚未Assign的子进程。
//! Microsoft UpdateProcThreadAttribute的JOB_LIST/HANDLE_LIST约定，Windows 10+。

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::SECURITY_ATTRIBUTES,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

pub(super) struct Child {
    pub(super) stdin: Option<File>,
    pub(super) stdout: Option<File>,
    pub(super) stderr: Option<File>,
    process: OwnedHandle,
    pid: u32,
    status: Option<ExitStatus>,
}
impl Child {
    pub(super) fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(status) = self.status {
            return Ok(Some(status));
        }
        match unsafe { WaitForSingleObject(self.process.as_raw_handle().cast(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle().cast(), &mut code) }
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
                let status = ExitStatus::from_raw(code);
                self.status = Some(status);
                Ok(Some(status))
            }
            _ => Err(io::Error::last_os_error()),
        }
    }
}
impl AsRawHandle for Child {
    fn as_raw_handle(&self) -> RawHandle {
        self.process.as_raw_handle()
    }
}

pub(super) struct Spawned {
    pub(super) child: Child,
    pub(super) job: OwnedHandle,
}

fn wide(value: &OsStr) -> Result<Vec<u16>, String> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err("conversation/command_invalid".into());
    }
    value.push(0);
    Ok(value)
}

fn quoted(value: &OsStr) -> Result<Vec<u16>, String> {
    let value = wide(value)?;
    let mut out = vec![b'"' as u16];
    let mut slashes = 0;
    for &c in &value[..value.len() - 1] {
        if c == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let count = if c == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        out.extend(std::iter::repeat_n(b'\\' as u16, count));
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    out.push(b'"' as u16);
    Ok(out)
}

fn folded(value: &OsStr) -> Vec<u16> {
    value
        .encode_wide()
        .map(|c| {
            if (b'a' as u16..=b'z' as u16).contains(&c) {
                c - 32
            } else {
                c
            }
        })
        .collect()
}
fn environment(command: &Command) -> Result<BTreeMap<Vec<u16>, (OsString, OsString)>, String> {
    // 仅接宿主构造的Command（继承环境+显式覆盖），不接受env_clear/raw_arg之类不可读取的std内部状态。
    let mut values: BTreeMap<_, _> = std::env::vars_os()
        .map(|(k, v)| (folded(&k), (k, v)))
        .collect();
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            values.insert(folded(key), (key.into(), value.into()));
        } else {
            values.remove(&folded(key));
        }
    }
    for (key, value) in values.values() {
        wide(key)?;
        wide(value)?;
    }
    Ok(values)
}

fn executable(
    command: &Command,
    environment: &BTreeMap<Vec<u16>, (OsString, OsString)>,
) -> Result<PathBuf, String> {
    let program = Path::new(command.get_program());
    let cwd = command
        .get_current_dir()
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .ok_or("conversation/cwd_invalid")?;
    let mut candidates = vec![];
    if program.is_absolute() {
        candidates.push(program.to_path_buf());
    } else if program.components().count() > 1 {
        candidates.push(cwd.join(program));
    } else {
        let paths = environment
            .get(&folded(OsStr::new("PATH")))
            .map(|(_, v)| v)
            .ok_or("conversation/cli_missing")?;
        for entry in std::env::split_paths(paths) {
            let candidate = entry.join(program);
            if program.extension().is_none() {
                candidates.push(candidate.with_extension("exe"));
            }
            candidates.push(candidate);
        }
    }
    candidates
        .into_iter()
        .find(|p| {
            p.is_file()
                && p.extension().is_some_and(|s| {
                    s.to_string_lossy().eq_ignore_ascii_case("exe")
                        || s.to_string_lossy().eq_ignore_ascii_case("com")
                })
        })
        .map(|p| if p.is_absolute() { p } else { cwd.join(p) })
        .ok_or_else(|| "conversation/cli_executable_required".into())
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle), String> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err("conversation/stdio_missing".into());
    }
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read.cast()),
            OwnedHandle::from_raw_handle(write.cast()),
        )
    })
}

struct Attributes {
    storage: Vec<usize>,
    jobs: Box<[HANDLE; 1]>,
    handles: Box<[HANDLE; 3]>,
    initialized: bool,
}
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn new(job: HANDLE, handles: [HANDLE; 3]) -> Result<Self, String> {
        let mut bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut bytes);
        }
        if bytes == 0 || bytes > 64 * 1024 {
            return Err("conversation/process_attributes".into());
        }
        let mut list = Self {
            storage: vec![0; bytes.div_ceil(std::mem::size_of::<usize>())],
            jobs: Box::new([job]),
            handles: Box::new(handles),
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(list.pointer(), 2, 0, &mut bytes) } == 0 {
            return Err("conversation/process_attributes".into());
        }
        list.initialized = true;
        // Box保证lpValue在CreateProcess和DeleteProcThreadAttributeList之间地址稳定，不引用临时数组。
        let jobs = list.jobs.as_ptr();
        let handles = list.handles.as_ptr();
        if unsafe {
            UpdateProcThreadAttribute(
                list.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.cast(),
                std::mem::size_of::<[HANDLE; 1]>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
            || unsafe {
                UpdateProcThreadAttribute(
                    list.pointer(),
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.cast(),
                    std::mem::size_of::<[HANDLE; 3]>(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            } == 0
        {
            return Err("conversation/process_attributes".into());
        }
        Ok(list)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}

pub(super) fn spawn(
    command: &Command,
    name: &[u16],
    confirmed_exit: Arc<AtomicBool>,
) -> Result<Spawned, String> {
    if name.last() != Some(&0) || name[..name.len().saturating_sub(1)].contains(&0) {
        return Err("conversation/process_group".into());
    }
    let environment = environment(command)?;
    let exe = executable(command, &environment)?;
    let application = wide(exe.as_os_str())?;
    let mut args = quoted(command.get_program())?;
    for arg in command.get_args() {
        args.push(b' ' as u16);
        args.extend(quoted(arg)?);
    }
    args.push(0);
    if args.len() > 32767 {
        return Err("conversation/command_too_large".into());
    }
    let mut block = Vec::new();
    for (key, value) in environment.values() {
        let key = wide(key)?;
        let value = wide(value)?;
        block.extend_from_slice(&key[..key.len() - 1]);
        block.push(b'=' as u16);
        block.extend(value);
    }
    block.push(0);
    if environment.is_empty() {
        block.push(0);
    }
    let cwd = command
        .get_current_dir()
        .map(|p| wide(p.as_os_str()))
        .transpose()?;
    unsafe {
        SetLastError(0);
    }
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), name.as_ptr()) };
    if raw.is_null() {
        return Err("conversation/process_group".into());
    }
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let job = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
    if existed {
        return Err("conversation/process_group_conflict".into());
    }
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err("conversation/process_group".into());
    }
    let (child_in, parent_in) = pipe()?;
    let (parent_out, child_out) = pipe()?;
    let (parent_err, child_err) = pipe()?;
    for handle in [&parent_in, &parent_out, &parent_err] {
        if unsafe { SetHandleInformation(handle.as_raw_handle().cast(), HANDLE_FLAG_INHERIT, 0) }
            == 0
        {
            return Err("conversation/stdio_missing".into());
        }
    }
    let handles = [
        child_in.as_raw_handle().cast(),
        child_out.as_raw_handle().cast(),
        child_err.as_raw_handle().cast(),
    ];
    let mut attributes = Attributes::new(raw, handles)?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    startup.lpAttributeList = attributes.pointer();
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    confirmed_exit.store(false, Ordering::Release);
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            args.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            block.as_ptr().cast(),
            cwd.as_ref().map_or(std::ptr::null(), |v| v.as_ptr()),
            &startup.StartupInfo,
            &mut info,
        )
    };
    if created == 0 {
        confirmed_exit.store(true, Ordering::Release);
        return Err("conversation/spawn_failed".into());
    }
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess.cast()) };
    let _thread = unsafe { OwnedHandle::from_raw_handle(info.hThread.cast()) };
    Ok(Spawned {
        child: Child {
            stdin: Some(File::from(parent_in)),
            stdout: Some(File::from(parent_out)),
            stderr: Some(File::from(parent_err)),
            process,
            pid: info.dwProcessId,
            status: None,
        },
        job,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_arguments_and_case_insensitive_environment_roundtrip_without_shell() {
        use std::io::Read;
        let mut command = Command::new("node");
        command.arg("-e").arg("process.stdout.write(JSON.stringify({args:process.argv.slice(1),value:process.env.MEOWO_JOB_PROBE_VALUE}))");
        let args = [
            "",
            "with spaces",
            "quote\"value",
            "trailing\\",
            "中文路径\\\"",
            "$() & | literal",
        ];
        command
            .args(args)
            .env("meowo_job_probe_value", "first")
            .env("MEOWO_JOB_PROBE_VALUE", "last");
        let name = std::ffi::OsString::from(format!(
            "Local\\MeowoAtomicTest-{}",
            crate::remote::generate_token().unwrap()
        ));
        let name = wide(&name).unwrap();
        let proof = Arc::new(AtomicBool::new(true));
        let mut spawned = spawn(&command, &name, proof.clone()).unwrap();
        assert!(!proof.load(Ordering::Acquire));
        let mut text = String::new();
        spawned
            .child
            .stdout
            .take()
            .unwrap()
            .take(8192)
            .read_to_string(&mut text)
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["args"], serde_json::json!(args));
        assert_eq!(value["value"], "last");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while spawned.child.try_wait().unwrap().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
