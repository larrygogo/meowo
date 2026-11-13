//! owner 同时保存宿主进程强指纹与随机命名空间；恢复不能把 PID 缺席误当 owned 子树已退出。
#[cfg(target_os = "windows")]
use sha2::{Digest, Sha256};

#[cfg(target_os = "windows")]
pub(super) fn job_name(owner: &str) -> Vec<u16> {
    format!(
        "Local\\MeowoConversation-{:x}",
        Sha256::digest(owner.as_bytes())
    )
    .encode_utf16()
    .chain([0])
    .collect()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::{
        Foundation::*,
        System::RemoteDesktop::ProcessIdToSessionId,
        System::{JobObjects::*, Threading::*},
    };

    fn creation(process: HANDLE) -> Result<u64, String> {
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited = created;
        let mut kernel = created;
        let mut user = created;
        if unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err("conversation/previous_process_unverified".into());
        }
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub(super) fn new() -> Result<String, String> {
        let created = creation(unsafe { GetCurrentProcess() })?;
        let session = current_session()?;
        Ok(format!(
            "r1.{}.{created}.{session}.{}",
            std::process::id(),
            crate::remote::generate_token()?
        ))
    }

    fn current_session() -> Result<u32, String> {
        let mut session = 0;
        if unsafe { ProcessIdToSessionId(std::process::id(), &mut session) } == 0 {
            return Err("conversation/previous_process_unverified".into());
        }
        Ok(session)
    }

    fn fingerprint(owner: &str) -> Result<(u32, u64, u32), String> {
        let mut fields = owner.split('.');
        let version = fields.next();
        let pid = fields
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0);
        let created = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0);
        let session = fields.next().and_then(|value| value.parse::<u32>().ok());
        let token = fields.next();
        match (version, pid, created, session, token, fields.next()) {
            (Some("r1"), Some(pid), Some(created), Some(session), Some(token), None)
                if owner == format!("r1.{pid}.{created}.{session}.{token}")
                    && token.len() == 64
                    && token
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) =>
            {
                Ok((pid, created, session))
            }
            _ => Err("conversation/previous_process_unverified".into()),
        }
    }

    fn host_gone(pid: u32, expected_creation: u64) -> Result<(), String> {
        let raw = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if raw.is_null() {
            return if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                Ok(())
            } else {
                Err("conversation/previous_process_unverified".into())
            };
        }
        let process = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        if creation(process.as_raw_handle().cast())? != expected_creation {
            return Ok(());
        }
        if unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) } == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err("conversation/previous_process_unverified".into())
        }
    }

    fn active(job: HANDLE) -> Result<u32, String> {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err("conversation/exit_unverified".into());
        }
        Ok(info.ActiveProcesses)
    }

    pub(super) fn confirm_orphan_exit(owner: &str) -> Result<(), String> {
        let (pid, created, session) = fingerprint(owner)?;
        if current_session()? != session {
            return Err("conversation/previous_process_unverified".into());
        }
        host_gone(pid, created)?;
        let name = job_name(owner);
        // 官方 Job security rights 的 QUERY(0x4)/TERMINATE(0x8)，只打开此 owner 的随机命名空间。
        let raw = unsafe { OpenJobObjectW(0x0004 | 0x0008, 0, name.as_ptr()) };
        if raw.is_null() {
            // 原 host 已死且 job 真正消失才是正证；AccessDenied 等错误绝不当作不存在。
            return if unsafe { GetLastError() } == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err("conversation/exit_unverified".into())
            };
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        if active(job.as_raw_handle().cast())? > 0
            && unsafe { TerminateJobObject(job.as_raw_handle().cast(), 1) } == 0
        {
            return Err("conversation/exit_unverified".into());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if active(job.as_raw_handle().cast())? == 0 {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("conversation/exit_unverified".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use meowo_agent::conversation::{ConversationSpec, ConversationTransport};
        use std::sync::{atomic::AtomicBool, Arc};
        struct ChildHost(std::process::Child);
        impl Drop for ChildHost {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        #[test]
        fn live_host_cannot_be_recovered_and_creation_time_detects_pid_reuse() {
            let owner = new().unwrap();
            let (pid, created, session) = fingerprint(&owner).unwrap();
            assert!(confirm_orphan_exit(&owner).is_err());
            assert!(host_gone(pid, created + 1).is_ok());
            let other_session = format!(
                "r1.{pid}.{created}.{}.{}",
                session.wrapping_add(1),
                crate::remote::generate_token().unwrap()
            );
            assert!(confirm_orphan_exit(&other_session).is_err());
            for invalid in ["old-random-owner", "r1.0.1.abc", "r1.1.0.abc", "r1.1.1.abc"] {
                assert!(fingerprint(invalid).is_err());
            }
        }

        #[test]
        #[ignore = "仅由隔离父测试以专用临时环境启动，ExitProcess模拟宿主崩溃"]
        fn crash_fixture_host() {
            let root = std::path::PathBuf::from(
                std::env::var_os("MEOWO_TEST_OWNER_CRASH_ROOT").expect("private fixture root"),
            );
            let id = std::env::var("MEOWO_TEST_OWNER_CRASH_SESSION")
                .unwrap()
                .parse::<i64>()
                .unwrap();
            let store = crate::open_store(&root.join("board.db")).unwrap();
            let owner = new().unwrap();
            assert!(store
                .managed_acquire_runtime(id, &owner, crate::now_ms())
                .unwrap());
            store
                .managed_set_delivery_state(
                    id,
                    "interrupted",
                    "queued",
                    "sending",
                    None,
                    None,
                    crate::now_ms(),
                )
                .unwrap();
            store
                .managed_set_delivery_state(
                    id,
                    "interrupted",
                    "sending",
                    "accepted",
                    Some("native-receipt"),
                    None,
                    crate::now_ms(),
                )
                .unwrap();
            let spec = ConversationSpec {
                args: vec![],
                environment: vec![],
                transport: ConversationTransport::JsonLines,
                ready_timeout_ms: 5000,
            };
            let script = "console.log(JSON.stringify({ready:true}));setInterval(()=>{},1000)";
            let transport = super::super::super::transport::Transport::spawn_owned(
                &["node".into(), "-e".into(), script.into()],
                &spec,
                &root.to_string_lossy(),
                &[],
                Arc::new(AtomicBool::new(true)),
                &owner,
            )
            .unwrap();
            assert!(
                matches!(transport.events.recv_timeout(Duration::from_secs(5)),Ok(super::super::super::transport::TransportEvent::Wire(value)) if value["ready"]==true)
            );
            // 故意不落 runtime_pid，覆盖 spawn 成功而 PID 尚未持久化时宿主崩溃的窗口。
            assert!(store
                .managed_state(id)
                .unwrap()
                .unwrap()
                .runtime_pid
                .is_none());
            std::fs::write(root.join("ready"), owner).unwrap();
            std::mem::forget(transport);
            unsafe { ExitProcess(23) };
        }

        #[test]
        fn crashed_host_recovers_the_named_tree_even_before_runtime_pid_was_persisted() {
            use std::os::windows::process::CommandExt;
            let root = std::env::temp_dir().join(format!(
                "meowo-owner-crash-{}",
                &crate::remote::generate_token().unwrap()[..16]
            ));
            std::fs::create_dir(&root).unwrap();
            let db = root.join("board.db");
            let store = crate::open_store(&db).unwrap();
            let id = store
                .managed_create(
                    "missing-provider",
                    &root.to_string_lossy(),
                    None,
                    crate::now_ms(),
                )
                .unwrap();
            store
                .managed_enqueue(
                    id,
                    "interrupted",
                    "never automatically resend",
                    "[]",
                    None,
                    crate::now_ms(),
                )
                .unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "conversation::owner::windows::tests::crash_fixture_host",
                    "--ignored",
                    "--nocapture",
                ])
                .creation_flags(0x0800_0000)
                .env("MEOWO_TEST_OWNER_CRASH_ROOT", &root)
                .env("MEOWO_TEST_OWNER_CRASH_SESSION", id.to_string())
                .env("HOME", &root)
                .env("USERPROFILE", &root)
                .env("MEOWO_DB", &db)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let mut child = ChildHost(command.spawn().unwrap());
            let deadline = Instant::now() + Duration::from_secs(12);
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                assert!(Instant::now() < deadline, "isolated host did not exit");
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(status.code(), Some(23));
            assert!(root.join("ready").is_file());
            assert!(store
                .managed_state(id)
                .unwrap()
                .unwrap()
                .runtime_pid
                .is_none());
            assert_eq!(
                super::super::super::ensure_worker(&db, id, None).unwrap_err(),
                "conversation/provider_unknown"
            );
            let state = store.managed_state(id).unwrap().unwrap();
            assert!(state.runtime_owner.is_none());
            let receipt = store.managed_delivery(id, "interrupted").unwrap().unwrap();
            assert_eq!(receipt.state, "uncertain");
            assert_eq!(receipt.native_message_id.as_deref(), Some("native-receipt"));
            assert!(store.managed_next_queued(id).unwrap().is_none());
            drop(store);
            drop(child);
            if root
                .canonicalize()
                .ok()
                .is_some_and(|p| p.parent() == std::env::temp_dir().canonicalize().ok().as_deref())
            {
                let _ = std::fs::remove_dir_all(&root);
            }
        }
    }
}

pub(super) fn new(db: &std::path::Path) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = db;
        windows::new()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let owner = format!("u1.{}", crate::remote::generate_token()?);
        let guard = meowo_reporter::unix_supervisor::prepare(db, &owner)
            .map_err(|_| "conversation/lease_failed")?;
        UNIX_OWNERS
            .lock()
            .map_err(|_| "conversation/lock_failed")?
            .insert(owner.clone(), guard);
        UNIX_DATABASES
            .lock()
            .map_err(|_| "conversation/lock_failed")?
            .insert(owner.clone(), db.to_path_buf());
        Ok(owner)
    }
}

pub(super) fn confirm_orphan_exit(db: &std::path::Path, owner: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let _ = db;
        windows::confirm_orphan_exit(owner)
    }
    #[cfg(not(target_os = "windows"))]
    {
        meowo_reporter::unix_supervisor::confirmed(db, owner)
            .map_err(|_| "conversation/previous_process_unverified".into())
    }
}

#[cfg(unix)]
static UNIX_OWNERS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::fs::File>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub(super) fn retire(owner: &str) {
    #[cfg(unix)]
    {
        if let Ok(mut owners) = UNIX_OWNERS.lock() {
            owners.remove(owner);
        }
        if let Ok(mut databases) = UNIX_DATABASES.lock() {
            databases.remove(owner);
        }
    }
    #[cfg(windows)]
    let _ = owner;
}

#[cfg(unix)]
pub(super) fn db_for(owner: &str) -> Result<std::path::PathBuf, String> {
    UNIX_DATABASES
        .lock()
        .map_err(|_| "conversation/lock_failed")?
        .get(owner)
        .cloned()
        .ok_or("conversation/lease_missing".into())
}

#[cfg(unix)]
static UNIX_DATABASES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::path::PathBuf>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
