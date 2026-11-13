//! 原生协议IO仅运行在会话worker；队列与帧有界，退出时只回收本次spawn拥有的进程树。

use meowo_agent::conversation::{ConversationSpec, ConversationTransport};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
#[cfg(unix)]
#[path = "spawn_unix.rs"]
mod spawn_unix;
#[cfg(unix)]
use spawn_unix::Child;
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
#[path = "spawn_windows.rs"]
mod spawn_windows;
#[cfg(target_os = "windows")]
use spawn_windows::Child;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const MAX_FRAME: usize = 4 * 1024 * 1024;
// OpenCode 的 /event 会把完整原生变更放进单个 SSE data 事件；真模型探针实测它可超过
// 通用控制帧上限。SSE 仍保留明确上限，避免慢客户端用无界累积耗尽宿主内存。
const MAX_SSE_EVENT: usize = 16 * 1024 * 1024;
// OpenCode 的 prompt_async 会同步返回本轮原生状态；它与 SSE 事件一样可能包含完整变更。
// 仅放宽 HTTP 应答，避免把发送给 CLI 的控制请求和 stdio 帧一并放大。
const MAX_HTTP_RESPONSE: usize = 16 * 1024 * 1024;
const QUEUE: usize = 16;

pub(super) enum TransportEvent {
    Wire(Value),
    Fault(&'static str),
    Exited,
}

struct OwnedProcess {
    child: Child,
    stopped: bool,
    stop_attempted: bool,
    confirmed_exit: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    job: std::os::windows::io::OwnedHandle,
}

impl OwnedProcess {
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        if self.stop_attempted {
            return;
        }
        self.stop_attempted = true;
        #[cfg(unix)]
        {
            if self.child.stop() {
                self.stopped = true;
                self.confirmed_exit.store(true, Ordering::Release);
            }
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            // Job句柄由本次spawn创建，关闭/终止只触及其成员，不按可复用的PID猜进程归属。
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(
                    self.job.as_raw_handle().cast(),
                    1,
                );
            }
        }
        #[cfg(target_os = "windows")]
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let root_exited = self.child.try_wait().ok().flatten().is_some();
                #[cfg(target_os = "windows")]
                let tree_exited = {
                    use std::os::windows::io::AsRawHandle;
                    use windows_sys::Win32::System::JobObjects::*;
                    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                        unsafe { std::mem::zeroed() };
                    let queried = unsafe {
                        QueryInformationJobObject(
                            self.job.as_raw_handle().cast(),
                            JobObjectBasicAccountingInformation,
                            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                            std::mem::size_of_val(&info) as u32,
                            std::ptr::null_mut(),
                        ) != 0
                    };
                    queried && info.ActiveProcesses == 0
                };
                if root_exited && tree_exited {
                    self.stopped = true;
                    self.confirmed_exit.store(true, Ordering::Release);
                    return;
                }
                if Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn exited(&mut self) -> std::io::Result<bool> {
        #[cfg(target_os = "windows")]
        {
            self.child.try_wait().map(|s| s.is_some())
        }
        #[cfg(unix)]
        {
            self.child.try_wait().map(|s| s.is_some())
        }
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

struct HttpChannel {
    base: String,
    auth: String,
    agent: ureq::Agent,
}

pub(super) struct Transport {
    process: Arc<Mutex<OwnedProcess>>,
    writer: Option<SyncSender<Vec<u8>>>,
    http: Option<HttpChannel>,
    stopping: Arc<AtomicBool>,
    pub(super) events: Receiver<TransportEvent>,
}

fn line(reader: &mut impl BufRead, max: usize) -> Result<Option<Vec<u8>>, ()> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf().map_err(|_| ())?;
        if available.is_empty() {
            return Ok((!bytes.is_empty()).then_some(bytes));
        }
        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |n| n + 1);
        if bytes.len() + count > max {
            return Err(());
        }
        let done = available[count - 1] == b'\n';
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if done {
            return Ok(Some(bytes));
        }
    }
}

fn stderr_fault(bytes: &[u8]) -> Option<&'static str> {
    // CLI 的stderr可能带配置、路径甚至凭据；归类规则与协议错误共用，返回值不含原文。
    meowo_agent::conversation::classify_native_fault(&String::from_utf8_lossy(bytes))
}

impl Transport {
    #[cfg(test)]
    pub(super) fn spawn(
        argv: &[String],
        spec: &ConversationSpec,
        cwd: &str,
        env: &[(String, String)],
    ) -> Result<Self, String> {
        Self::spawn_tracked(argv, spec, cwd, env, Arc::new(AtomicBool::new(true)))
    }

    #[cfg(test)]
    pub(super) fn spawn_tracked(
        argv: &[String],
        spec: &ConversationSpec,
        cwd: &str,
        env: &[(String, String)],
        confirmed_exit: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        #[cfg(unix)]
        let fixture = std::env::temp_dir().join(format!(
            "meowo-transport-{}",
            crate::remote::generate_token()?
        ));
        #[cfg(unix)]
        std::fs::create_dir(&fixture).map_err(|_| "conversation/lease_failed")?;
        #[cfg(unix)]
        let db = fixture.join("board.db");
        #[cfg(windows)]
        let db = std::path::PathBuf::from(".");
        let owner = super::owner::new(&db)?;
        Self::spawn_owned(argv, spec, cwd, env, confirmed_exit, &owner)
    }

    pub(super) fn spawn_owned(
        argv: &[String],
        spec: &ConversationSpec,
        cwd: &str,
        env: &[(String, String)],
        confirmed_exit: Arc<AtomicBool>,
        owner: &str,
    ) -> Result<Self, String> {
        confirmed_exit.store(true, Ordering::Release);
        let (exe, base_args) = argv.split_first().ok_or("conversation/cli_missing")?;
        let mut args = spec.args.clone();
        let http_setup = if let ConversationTransport::HttpServer {
            health_path,
            events_path,
            username,
            password_env,
        } = &spec.transport
        {
            let reservation =
                TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "conversation/listen_failed")?;
            let port = reservation
                .local_addr()
                .map_err(|_| "conversation/listen_failed")?
                .port();
            let password = crate::remote::generate_token()?;
            for arg in &mut args {
                *arg = arg.replace("{meowo_port}", &port.to_string());
            }
            drop(reservation);
            Some((
                port,
                password,
                username.clone(),
                password_env.clone(),
                health_path.clone(),
                events_path.clone(),
            ))
        } else {
            None
        };
        let mut command = Command::new(exe);
        command
            .args(base_args)
            .args(&args)
            .current_dir(cwd)
            .envs(env.iter().cloned())
            .envs(spec.environment.iter().cloned())
            .env("MEOWO_PROTOCOL_MANAGED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some((_, password, _, key, _, _)) = &http_setup {
            command.env(key, password);
        }
        #[cfg(target_os = "windows")]
        let mut owned = {
            let spawned = spawn_windows::spawn(
                &command,
                &super::owner::job_name(owner),
                confirmed_exit.clone(),
            )?;
            OwnedProcess {
                child: spawned.child,
                job: spawned.job,
                stopped: false,
                stop_attempted: false,
                confirmed_exit,
            }
        };
        #[cfg(not(target_os = "windows"))]
        let mut owned = {
            // 从 helper 启动起就必须保留 owner；失败也不能凭未拿到 native PID 宣称无子进程。
            let child = spawn_unix::spawn(&command, owner, confirmed_exit.clone())?;
            OwnedProcess {
                child,
                stopped: false,
                stop_attempted: false,
                confirmed_exit,
            }
        };
        let stdin = owned
            .child
            .stdin
            .take()
            .ok_or("conversation/stdio_missing")?;
        let stdout = owned
            .child
            .stdout
            .take()
            .ok_or("conversation/stdio_missing")?;
        let stderr = owned
            .child
            .stderr
            .take()
            .ok_or("conversation/stdio_missing")?;
        let process = Arc::new(Mutex::new(owned));
        let stopping = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        // stderr不回显原始CLI配置或凭据，也不积攒无界诊断内容；仅给出固定错误码。
        let stderr_events = tx.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut reported = false;
            loop {
                match line(&mut reader, 16 * 1024) {
                    Ok(Some(bytes)) => {
                        if !reported {
                            if let Some(reason) = stderr_fault(&bytes) {
                                let _ = stderr_events.try_send(TransportEvent::Fault(reason));
                                reported = true;
                            }
                        }
                    }
                    Ok(None) | Err(()) => return,
                }
            }
        });
        let (writer, http) = if let Some((port, password, username, _, health, event_path)) =
            http_setup
        {
            drop(stdin);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut BufReader::new(stdout), &mut std::io::sink());
            });
            use base64::Engine;
            let auth = format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"))
            );
            let base = format!("http://127.0.0.1:{port}");
            let agent = ureq::AgentBuilder::new()
                .redirects(0)
                .timeout_connect(Duration::from_secs(2))
                .timeout_read(Duration::from_secs(30))
                .build();
            let deadline =
                Instant::now() + Duration::from_millis(spec.ready_timeout_ms.clamp(1000, 60_000));
            loop {
                if agent
                    .get(&format!("{base}{health}"))
                    .set("Authorization", &auth)
                    .timeout(Duration::from_millis(500))
                    .call()
                    .is_ok()
                {
                    if !matches!(
                        agent
                            .get(&format!("{base}{health}"))
                            .timeout(Duration::from_millis(500))
                            .call(),
                        Err(ureq::Error::Status(401, _))
                    ) {
                        return Err("conversation/server_auth_missing".into());
                    }
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("conversation/start_timeout".into());
                }
                if process
                    .lock()
                    .map_err(|_| "conversation/lock_failed")?
                    .exited()
                    .map_err(|_| "conversation/process_failed")?
                {
                    return Err("conversation/process_exited".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let sse_agent = agent.clone();
            let sse_auth = auth.clone();
            let url = format!("{base}{event_path}");
            let stop = stopping.clone();
            let events = tx.clone();
            let (subscribed, subscription) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                match sse_agent.get(&url).set("Authorization", &sse_auth).call() {
                    Ok(response) => {
                        let _ = subscribed.send(true);
                        let mut reader = BufReader::new(response.into_reader());
                        let mut data = Vec::new();
                        while !stop.load(Ordering::Acquire) {
                            match line(&mut reader, MAX_SSE_EVENT) {
                                Ok(Some(bytes)) => {
                                    let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
                                    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
                                    if bytes.is_empty() {
                                        if !data.is_empty() {
                                            match serde_json::from_slice(&data) {
                                                Ok(value) => {
                                                    if events
                                                        .send(TransportEvent::Wire(value))
                                                        .is_err()
                                                    {
                                                        return;
                                                    }
                                                }
                                                Err(_) => {
                                                    let _ = events.send(TransportEvent::Fault(
                                                        "conversation/protocol_invalid",
                                                    ));
                                                    return;
                                                }
                                            }
                                            data.clear();
                                        }
                                    } else if let Some(part) = bytes.strip_prefix(b"data:") {
                                        if append_sse_data(
                                            &mut data,
                                            part.strip_prefix(b" ").unwrap_or(part),
                                            MAX_SSE_EVENT,
                                        )
                                        .is_err()
                                        {
                                            let _ = events.send(TransportEvent::Fault(
                                                "conversation/sse_event_too_large",
                                            ));
                                            return;
                                        }
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                    Err(_) => {
                        let _ = subscribed.send(false);
                    }
                }
                if !stop.load(Ordering::Acquire) {
                    let _ = events.send(TransportEvent::Fault("conversation/event_stream_closed"));
                }
            });
            // 首次载入账号和原生插件可能晚于health，订阅须共享整个启动预算，不能另设更短的假失败门槛。
            if subscription.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                != Ok(true)
            {
                return Err("conversation/event_stream_closed".into());
            }
            (None, Some(HttpChannel { base, auth, agent }))
        } else {
            let events = tx.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match line(&mut reader, MAX_FRAME) {
                        Ok(Some(bytes)) => match serde_json::from_slice(&bytes) {
                            Ok(value) => {
                                if events.send(TransportEvent::Wire(value)).is_err() {
                                    break;
                                }
                            }
                            Err(_) if bytes.iter().all(u8::is_ascii_whitespace) => {}
                            Err(_) => {
                                let _ = events
                                    .send(TransportEvent::Fault("conversation/protocol_invalid"));
                                break;
                            }
                        },
                        Ok(None) => {
                            let _ =
                                events.send(TransportEvent::Fault("conversation/stream_closed"));
                            break;
                        }
                        Err(()) => {
                            let _ =
                                events.send(TransportEvent::Fault("conversation/frame_invalid"));
                            break;
                        }
                    }
                }
            });
            let (writes, pending) = mpsc::sync_channel::<Vec<u8>>(QUEUE);
            let writer_events = tx.clone();
            std::thread::spawn(move || {
                let mut stdin = stdin;
                while let Ok(bytes) = pending.recv() {
                    if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                        let _ =
                            writer_events.send(TransportEvent::Fault("conversation/write_failed"));
                        break;
                    }
                }
            });
            (Some(writes), None)
        };
        let monitor = process.clone();
        let monitor_events = tx;
        let monitor_stop = stopping.clone();
        std::thread::spawn(move || {
            while !monitor_stop.load(Ordering::Acquire) {
                let exited = monitor.lock().ok().and_then(|mut p| p.exited().ok());
                match exited {
                    Some(true) => {
                        let _ = monitor_events.send(TransportEvent::Exited);
                        return;
                    }
                    None => {
                        let _ = monitor_events
                            .send(TransportEvent::Fault("conversation/process_failed"));
                        return;
                    }
                    Some(false) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        });
        Ok(Self {
            process,
            writer,
            http,
            stopping,
            events: rx,
        })
    }

    pub(super) fn pid(&self) -> Option<i64> {
        self.process.lock().ok().map(|p| i64::from(p.child.id()))
    }

    pub(super) fn send(&self, value: Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| "conversation/protocol_invalid")?;
        if bytes.len() > MAX_FRAME {
            return Err("conversation/stdio_request_too_large".into());
        }
        bytes.push(b'\n');
        self.writer
            .as_ref()
            .ok_or("conversation/transport_mismatch")?
            .try_send(bytes)
            .map_err(|_| "conversation/write_unavailable".into())
    }

    pub(super) fn request(
        &self,
        tag: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let channel = self
            .http
            .as_ref()
            .ok_or("conversation/transport_mismatch")?;
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['\r', '\n', '\\'])
            || !matches!(method, "GET" | "POST" | "PATCH" | "DELETE" | "PUT")
        {
            return Err("conversation/request_invalid".into());
        }
        let request = channel
            .agent
            .request(method, &format!("{}{path}", channel.base))
            .set("Authorization", &channel.auth)
            .timeout(Duration::from_secs(15));
        let response = match body {
            Some(body) => {
                let bytes =
                    serde_json::to_vec(body).map_err(|_| "conversation/protocol_invalid")?;
                if bytes.len() > MAX_FRAME {
                    return Err("conversation/http_request_too_large".into());
                }
                request
                    .set("Content-Type", "application/json")
                    .send_bytes(&bytes)
            }
            None => request.call(),
        };
        let response = match response {
            Ok(response) | Err(ureq::Error::Status(_, response)) => response,
            Err(_) => return Err("conversation/request_failed".into()),
        };
        let status = response.status();
        let bytes = read_http_response(response.into_reader())?;
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| "conversation/protocol_invalid")?
        };
        Ok(serde_json::json!({"_meowo_http":{"tag":tag,"status":status,"body":value}}))
    }
}

fn read_http_response(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_HTTP_RESPONSE + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "conversation/read_failed")?;
    if bytes.len() > MAX_HTTP_RESPONSE {
        return Err("conversation/http_response_too_large".into());
    }
    Ok(bytes)
}

fn append_sse_data(data: &mut Vec<u8>, part: &[u8], limit: usize) -> Result<(), ()> {
    let separator = usize::from(!data.is_empty());
    if data
        .len()
        .saturating_add(separator)
        .saturating_add(part.len())
        > limit
    {
        return Err(());
    }
    if separator != 0 {
        data.push(b'\n');
    }
    data.extend_from_slice(part);
    Ok(())
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut process) = self.process.lock() {
            process.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestHome(std::path::PathBuf);
    impl TestHome {
        fn new() -> Self {
            let suffix = crate::remote::generate_token().unwrap();
            let path = std::env::temp_dir().join(format!("meowo-transport-{}", &suffix[..16]));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            let temp = std::env::temp_dir().canonicalize().ok();
            if self
                .0
                .canonicalize()
                .ok()
                .is_some_and(|p| p.parent() == temp.as_deref())
            {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
    #[test]
    fn bounded_lines_handle_split_frames_and_reject_large_native_output() {
        let mut reader = BufReader::with_capacity(2, &b"{\"id\":1}\n{\"id\":2}\n"[..]);
        assert_eq!(line(&mut reader, 10).unwrap().unwrap(), b"{\"id\":1}\n");
        assert_eq!(line(&mut reader, 10).unwrap().unwrap(), b"{\"id\":2}\n");
        assert!(line(&mut reader, 10).unwrap().is_none());
        assert!(line(&mut BufReader::new(&b"0123456789\n"[..]), 5).is_err());
    }

    #[test]
    fn sse_parts_have_a_separate_bounded_budget() {
        let mut event = Vec::new();
        append_sse_data(&mut event, b"first", 12).unwrap();
        append_sse_data(&mut event, b"second", 12).unwrap();
        assert_eq!(event, b"first\nsecond");
        assert!(append_sse_data(&mut event, b"overflow", 12).is_err());

        let mut larger_than_control_frame = Vec::new();
        append_sse_data(
            &mut larger_than_control_frame,
            &vec![b'x'; MAX_FRAME + 1],
            MAX_SSE_EVENT,
        )
        .unwrap();
        assert_eq!(larger_than_control_frame.len(), MAX_FRAME + 1);
    }

    #[test]
    fn http_response_allows_opencode_state_but_keeps_a_hard_bound() {
        let accepted = read_http_response(std::io::Cursor::new(vec![b'x'; MAX_FRAME + 1]))
            .expect("OpenCode prompt_async response may exceed a control frame");
        assert_eq!(accepted.len(), MAX_FRAME + 1);

        let err = read_http_response(std::io::Cursor::new(vec![
            b'x';
            MAX_HTTP_RESPONSE + 1
        ]))
        .unwrap_err();
        assert_eq!(err, "conversation/http_response_too_large");
    }

    #[test]
    fn fake_stdio_owned_tree_exit_has_positive_confirmation_and_spawn_failure_has_no_owner() {
        let home = TestHome::new();
        let proof = Arc::new(AtomicBool::new(true));
        let script = "const {spawn}=require('child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore',windowsHide:true});console.log(JSON.stringify({childPid:child.pid}));setInterval(()=>{},1000)";
        let transport = Transport::spawn_tracked(
            &["node".into(), "-e".into(), script.into()],
            &stdio_spec(),
            &home.path().to_string_lossy(),
            &[],
            proof.clone(),
        )
        .unwrap();
        let child = wire(&transport, |value| value["childPid"].is_number());
        assert!(child["childPid"].as_u64().unwrap() > 0);
        assert!(!proof.load(Ordering::Acquire));
        drop(transport);
        assert!(
            proof.load(Ordering::Acquire),
            "returning from Drop alone is not exit evidence; owned tree must be empty"
        );
        let failed = Arc::new(AtomicBool::new(false));
        assert!(Transport::spawn_tracked(
            &[home
                .path()
                .join("missing-executable")
                .to_string_lossy()
                .into()],
            &stdio_spec(),
            &home.path().to_string_lossy(),
            &[],
            failed.clone()
        )
        .is_err());
        assert!(
            failed.load(Ordering::Acquire),
            "spawn failed before any child was created"
        );
    }

    fn stdio_spec() -> ConversationSpec {
        ConversationSpec {
            args: vec![],
            environment: vec![],
            transport: ConversationTransport::JsonLines,
            ready_timeout_ms: 5000,
        }
    }

    #[test]
    fn stderr_classification_is_bounded_and_never_returns_source_text() {
        assert_eq!(
            stderr_fault(b"Error: invalid API key Bearer private-token"),
            Some("conversation/native_auth_failed")
        );
        assert_eq!(
            stderr_fault(b"Error: Resource exhausted (429)"),
            Some("conversation/native_rate_limited")
        );
        assert_eq!(
            stderr_fault(b"Error: fetch failed: ETIMEDOUT"),
            Some("conversation/native_network_failed")
        );
        assert_eq!(
            stderr_fault(b"Error: context length exceeds token limit"),
            Some("conversation/native_context_limit")
        );
        // 2026-11-13 Gemini 默认模型实测原文形状：服务繁忙单列，不并进限流或网络。
        assert_eq!(
            stderr_fault(b"Attempt 2 failed with status 503. Retrying with backoff... ApiError: This model is currently experiencing high demand"),
            Some("conversation/native_busy")
        );
        assert_eq!(stderr_fault(b"warning: cache directory unavailable"), None);
    }

    fn wire(transport: &Transport, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match transport
                .events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                Ok(TransportEvent::Wire(value)) if predicate(&value) => return value,
                Ok(TransportEvent::Wire(_)) => {}
                Ok(TransportEvent::Fault(reason)) => panic!("{reason}"),
                _ => panic!("conversation/probe_timeout"),
            }
        }
    }

    #[test]
    #[ignore = "真实CLI无模型探针，显式给MEOWO_PROBE_CODEX_EXE；只用临时home"]
    fn native_codex_stdio_uses_private_job_and_real_handshake() {
        let exe = std::env::var("MEOWO_PROBE_CODEX_EXE").expect("probe executable required");
        let home = TestHome::new();
        let cwd = home.path().to_string_lossy().into_owned();
        let env = vec![
            ("CODEX_HOME".into(), cwd.clone()),
            ("HOME".into(), cwd.clone()),
            ("USERPROFILE".into(), cwd.clone()),
            (
                "MEOWO_DB".into(),
                home.path().join("board.db").to_string_lossy().into_owned(),
            ),
        ];
        let mut spec = stdio_spec();
        spec.args = vec!["app-server".into(), "--stdio".into()];
        let transport = Transport::spawn(&[exe], &spec, &cwd, &env).unwrap();
        transport.send(serde_json::json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"meowo-job-probe","version":"1.0"},"capabilities":{"experimentalApi":true}}})).unwrap();
        let initialized = wire(&transport, |v| v["id"] == 1);
        assert!(initialized.get("result").is_some());
        transport
            .send(serde_json::json!({"method":"initialized","params":{}}))
            .unwrap();
        transport.send(serde_json::json!({"id":2,"method":"thread/start","params":{"cwd":cwd,"historyMode":"legacy"}})).unwrap();
        let thread = wire(&transport, |v| v["id"] == 2);
        assert!(thread["result"]["thread"]["id"].is_string());
        assert!(thread["result"]["thread"]["parentThreadId"].is_null());
        let process = transport.process.clone();
        drop(transport);
        let mut process = process.lock().unwrap();
        assert!(process.stopped);
        assert!(process.child.try_wait().unwrap().is_some());
    }

    #[test]
    #[ignore = "真实OpenCode无模型HTTP/SSE探针，显式给MEOWO_PROBE_OPENCODE_EXE；独立home"]
    fn native_opencode_http_auth_sse_and_restart_same_session() {
        let exe = std::env::var("MEOWO_PROBE_OPENCODE_EXE").expect("probe executable required");
        let home = TestHome::new();
        let cwd = home.path().to_string_lossy().into_owned();
        let mut env = vec![
            ("HOME".into(), cwd.clone()),
            ("USERPROFILE".into(), cwd.clone()),
            (
                "MEOWO_DB".into(),
                home.path().join("board.db").to_string_lossy().into_owned(),
            ),
            ("OPENCODE_DISABLE_DEFAULT_PLUGINS".into(), "true".into()),
        ];
        for (key, dir) in [
            ("OPENCODE_CONFIG_DIR", "config"),
            ("XDG_CONFIG_HOME", "xdg-config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
        ] {
            env.push((
                key.into(),
                home.path().join(dir).to_string_lossy().into_owned(),
            ));
        }
        let spec = ConversationSpec {
            args: vec![
                "serve".into(),
                "--pure".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                "{meowo_port}".into(),
            ],
            environment: vec![("OPENCODE_SERVER_USERNAME".into(), "opencode".into())],
            transport: ConversationTransport::HttpServer {
                health_path: "/global/health".into(),
                events_path: "/event".into(),
                username: "opencode".into(),
                password_env: "OPENCODE_SERVER_PASSWORD".into(),
            },
            ready_timeout_ms: 15000,
        };
        let argv = vec![exe];
        let transport = Transport::spawn(&argv, &spec, &cwd, &env).unwrap();
        let event = wire(&transport, |v| v["type"] == "server.connected");
        assert!(event.is_object());
        let created = transport
            .request("create", "POST", "/session", Some(&serde_json::json!({})))
            .unwrap();
        let root = created["_meowo_http"]["body"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let first = transport.process.clone();
        drop(transport);
        assert!(first.lock().unwrap().child.try_wait().unwrap().is_some());
        let restored = Transport::spawn(&argv, &spec, &cwd, &env).unwrap();
        let loaded = restored
            .request("load", "GET", &format!("/session/{root}"), None)
            .unwrap();
        assert_eq!(loaded["_meowo_http"]["body"]["id"], root);
        let messages = restored
            .request("history", "GET", &format!("/session/{root}/message"), None)
            .unwrap();
        assert_eq!(messages["_meowo_http"]["body"], serde_json::json!([]));
        let second = restored.process.clone();
        drop(restored);
        assert!(second.lock().unwrap().child.try_wait().unwrap().is_some());
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "真实系统进程树探针，私有Job清理两层临时PowerShell，不接触用户CLI"]
    fn private_job_owns_child_before_resume_and_reaps_descendants() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let home = TestHome::new();
        let cwd = home.path().to_string_lossy().into_owned();
        let system = std::env::var("SystemRoot").unwrap();
        let exe = format!("{system}/System32/WindowsPowerShell/v1.0/powershell.exe");
        let script = r#"$child = Start-Process powershell.exe -ArgumentList @('-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 60') -WindowStyle Hidden -PassThru; Write-Output ('{"childPid":'+$child.Id+'}'); Start-Sleep -Seconds 60"#;
        let argv = vec![
            exe,
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ];
        let transport = Transport::spawn(&argv, &stdio_spec(), &cwd, &[]).unwrap();
        let event = wire(&transport, |v| v["childPid"].is_number());
        assert!(event["childPid"].as_u64().unwrap() > 0);
        let process = transport.process.clone();
        let count = |process: &OwnedProcess| {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            assert_ne!(
                unsafe {
                    QueryInformationJobObject(
                        process.job.as_raw_handle().cast(),
                        JobObjectBasicAccountingInformation,
                        (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        std::mem::size_of_val(&info) as u32,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            info.ActiveProcesses
        };
        assert!(count(&process.lock().unwrap()) >= 2);
        drop(transport);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if count(&process.lock().unwrap()) == 0 {
                break;
            }
            assert!(Instant::now() < deadline, "owned descendants survived");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(process.lock().unwrap().child.try_wait().unwrap().is_some());
    }
}
