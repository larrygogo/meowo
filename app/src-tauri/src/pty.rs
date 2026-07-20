//! Meowo 持有的 PTY broker。结构化对话仍走 transcript；这里仅负责原始 ANSI 终端的双向镜像。

use base64::Engine;
pub(crate) use meowo_protocol::broker::{ApprovalDecision, ApprovalRequest};
#[cfg(not(test))]
use meowo_protocol::broker::{BrokerDiscovery, APPROVAL_BROKER_FILE};
use meowo_protocol::broker::{read_handshake, BrokerRequest, CURRENT_PROTOCOL_VERSION};
pub(crate) use meowo_protocol::ipc::ManagedTerminalSnapshotDto as PtySnapshot;
use meowo_protocol::ipc::{PtyExitEvent as PtyExit, PtyOutputEvent as PtyOutput};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::{Emitter, Manager};

const BACKLOG_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct TerminalSize {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

impl TerminalSize {
    pub(crate) const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows }
    }
}

struct ManagedPty {
    session_id: AtomicI64,
    /// Option 是给收尾用的：ClosePseudoConsole（drop）让 conhost 退出、释放资源。
    /// 注意它**不能**唤醒已阻塞的 reader（本机实证），所以收尾从不等 reader。None = 已关闭。
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    /// 收尾只许跑一次：waiter（轮询到进程退出）与 reader（万一真的 EOF）都会尝试触发。
    finalized: AtomicBool,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    backlog: Mutex<VecDeque<u8>>,
    /// 自 PTY 启动以来累计输出的字节位置；与 backlog 锁内更新，供快照和实时帧去重排序。
    output_end: AtomicU64,
    subscribers: Mutex<Vec<(u64, mpsc::Sender<Vec<u8>>)>>,
}

#[derive(Clone)]
struct CompletedPty {
    data: Vec<u8>,
    start_offset: u64,
    end_offset: u64,
    code: Option<u32>,
}

struct AttachState {
    endpoint: Mutex<Option<SocketAddr>>,
    token: String,
    started: AtomicBool,
    next_subscriber: AtomicU64,
    next_pending: AtomicI64,
    pending: Mutex<HashMap<String, i64>>,
    bindings: Mutex<HashMap<i64, i64>>,
    approvals: Mutex<HashMap<String, PendingApproval>>,
    /// 显式注册的 GUI 审批消费者。窗口存在/可见不等于已经订阅了目标 session。
    approval_consumers: Mutex<HashMap<String, i64>>,
    app: Mutex<Option<tauri::AppHandle>>,
}

struct PendingApproval {
    request: ApprovalRequest,
    response: mpsc::Sender<ApprovalDecision>,
}

#[derive(Clone)]
pub(crate) struct PtyBroker {
    sessions: Arc<Mutex<HashMap<i64, Arc<ManagedPty>>>>,
    completed: Arc<Mutex<HashMap<i64, CompletedPty>>>,
    attach: Arc<AttachState>,
}

impl Default for PtyBroker {
    fn default() -> Self {
        let token = random_token();
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            completed: Arc::new(Mutex::new(HashMap::new())),
            attach: Arc::new(AttachState {
                endpoint: Mutex::new(None),
                token,
                started: AtomicBool::new(false),
                next_subscriber: AtomicU64::new(1),
                next_pending: AtomicI64::new(-1),
                pending: Mutex::new(HashMap::new()),
                bindings: Mutex::new(HashMap::new()),
                approvals: Mutex::new(HashMap::new()),
                approval_consumers: Mutex::new(HashMap::new()),
                app: Mutex::new(None),
            }),
        }
    }
}

/// 从 ANSI 输出流里提取人能读的尾部文本：剥掉 CSI/OSC 转义与控制字符，
/// 取非空行的最后一段，限长 `max_chars`。给「Agent 秒退」的报错信息用。
fn readable_tail(data: &[u8], max_chars: usize) -> String {
    // 先按字节剥转义（UTF-8 多字节原样保留，最后统一 lossy 解码——逐字节转 char 会把中文拆成乱码）。
    let mut bytes: Vec<u8> = Vec::with_capacity(data.len().min(4096));
    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        if byte == 0x1b {
            i += 1;
            match data.get(i) {
                Some(b'[') => {
                    i += 1;
                    while i < data.len() && !(0x40..=0x7e).contains(&data[i]) {
                        i += 1;
                    }
                }
                Some(b']') => {
                    i += 1;
                    while i < data.len() && data[i] != 0x07 && data[i] != 0x1b {
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        if byte == b'\n' || byte >= 0x20 && byte != 0x7f || byte >= 0x80 {
            bytes.push(byte);
        }
        i += 1;
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines
        .into_iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" · ");
    let mut out: String = tail.chars().take(max_chars).collect();
    if out.len() < tail.len() {
        out.push('…');
    }
    out
}

/// PTY 会话的唯一收尾路径：写库、通知看板与对话窗、掐断 attach、释放伪终端。
/// 幂等（finalized 原子门），由两处触发——waiter 轮询到进程退出（主力：Windows ConPTY
/// 在子进程退出后**不会**给 reader EOF，本机实证连 drop master 都唤不醒阻塞中的 read），
/// 以及 reader 万一真读到 EOF。收尾**从不等待 reader**；它若永远阻塞，就带着句柄躺着。
fn finalize_exit(broker: &PtyBroker, app: &tauri::AppHandle, managed: &Arc<ManagedPty>) {
    if managed.finalized.swap(true, Ordering::AcqRel) {
        return;
    }
    // 到这里进程必已退出（waiter try_wait 确认过）或即将退出（EOF 场景），wait 不会久等。
    let code = managed
        .child
        .lock()
        .ok()
        .and_then(|mut child| child.wait().ok())
        .map(|s| s.exit_code());
    // 释放伪终端让 conhost 退出。它救不了阻塞的 reader，但能停掉资源。
    if let Ok(mut master) = managed.master.lock() {
        drop(master.take());
    }
    let session_id = managed.session_id.load(Ordering::Acquire);
    let final_data = managed
        .backlog
        .lock()
        .map(|backlog| backlog.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    let end_offset = managed.output_end.load(Ordering::Acquire);
    let start_offset = end_offset.saturating_sub(final_data.len() as u64);
    if let Ok(mut completed) = broker.completed.lock() {
        // 退出输出只为诊断与终端回放保留；限制条数，避免长期运行无限增长。
        if completed.len() >= 24 {
            if let Some(oldest) = completed.keys().min().copied() {
                completed.remove(&oldest);
            }
        }
        completed.insert(
            session_id,
            CompletedPty {
                data: final_data,
                start_offset,
                end_offset,
                code,
            },
        );
    }
    if let Ok(mut sessions) = broker.sessions.lock() {
        if sessions
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, managed))
        {
            sessions.remove(&session_id);
        }
    }
    // 必须显式掐断订阅，不能指望「subscribers 随 ManagedPty 一起 drop」：attach 的
    // 服务线程自己持有这个 Arc（等客户端输入），tx 又在 Arc 里——彼此等对方先死，
    // 谁都死不了。结果是外部同步终端在会话结束后永远定格在一片静止画面上。
    // 清掉 tx → 转发线程 rx 断开并关 socket → 客户端收到 EOF 正常退出。
    if let Ok(mut subscribers) = managed.subscribers.lock() {
        subscribers.clear();
    }
    if session_id < 0 {
        if let Ok(mut pending) = broker.attach.pending.lock() {
            pending.retain(|_, id| *id != session_id);
        }
    } else {
        // 托管 PTY 是这个 agent 进程的唯一持有者——它退出，会话就真的结束了。必须主动
        // 收尾：resume 路径已经乐观复活过 DB（prepare_resume），没人回滚的话卡片会一直
        // 假显示「已连接」，直到 pid 判活的宽限窗口过期才自愈。这同时覆盖了「PTY 起来了
        // 但 CLI 秒退（不在 PATH）」——那种情况 start() 返回 Ok，调用方的回滚够不着。
        if let Ok(store) = crate::open_store(&crate::db_path()) {
            let _ = store.end_session(session_id, crate::now_ms());
        }
        if let Ok(mut bindings) = broker.attach.bindings.lock() {
            bindings.retain(|_, real| *real != session_id);
        }
        crate::watch::emit_board_changed(app, "pty-exit");
    }
    if let Some(window) = app.get_webview_window("chat") {
        let _ = window.emit("pty-exit", PtyExit { session_id, code });
    }
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    if getrandom::fill(&mut bytes).is_err() {
        // OS RNG 不可用属于极端退化；仍混入进程/时间，且服务只监听 loopback。
        let seed = format!("{}-{:?}", std::process::id(), std::time::SystemTime::now());
        for (i, byte) in seed.bytes().enumerate() {
            bytes[i % 32] ^= byte;
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(2, 500),
        cols: cols.clamp(2, 500),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl PtyBroker {
    pub(crate) fn set_app_handle(&self, app: tauri::AppHandle) {
        if let Ok(mut current) = self.attach.app.lock() {
            *current = Some(app);
        }
    }

    fn emit_approval(&self, event: &str, request: &ApprovalRequest) {
        if let Some(app) = self.attach.app.lock().ok().and_then(|app| app.clone()) {
            let _ = app.emit(event, request.clone());
        }
    }

    /// 审批 broker 只有在对应对话窗确实可用时才能接管请求。外部终端启动的 agent 也能从
    /// discovery 文件发现 broker，但那不代表此刻有 GUI 消费者；若直接入队，会让原 TUI
    /// 无提示等满五分钟。收到请求时主动打开/切换到对应会话，并给窗口创建一个很短的期限；
    /// 创建失败则调用方立即返回 `pass`，由 agent 自己的审批界面接管。
    fn ensure_approval_window(&self, session_id: i64) -> bool {
        if self.has_approval_consumer(session_id) {
            return true;
        }
        let Some(app) = self.attach.app.lock().ok().and_then(|app| app.clone()) else {
            return false;
        };
        crate::window::open_chat_window(app.clone(), session_id);
        // 等前端完成 session 切换并显式注册。窗口可见只能证明 WebView 存在，不能证明它已监听
        // pending-approval；以消费者租约为准，避免请求落在两个 useEffect 之间。
        for _ in 0..80 {
            if self.has_approval_consumer(session_id) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        false
    }

    pub(crate) fn start(
        &self,
        app: tauri::AppHandle,
        session_id: i64,
        argv: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        terminal_size: TerminalSize,
    ) -> Result<(), String> {
        if argv.is_empty() {
            return Err("该 Agent 不支持恢复会话".into());
        }
        // 持有注册表锁直到 spawn+insert 完成，闭合两个并发 start 同时通过 contains 检查的竞态。
        let mut sessions = self.sessions.lock().map_err(|_| "PTY 状态锁已损坏")?;
        if sessions.contains_key(&session_id) {
            return Ok(());
        }
        if let Ok(mut completed) = self.completed.lock() {
            completed.remove(&session_id);
        }

        let pair = native_pty_system()
            .openpty(size(terminal_size.cols, terminal_size.rows))
            .map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        if let Some(cwd) = cwd.filter(|c| !c.trim().is_empty()) {
            command.cwd(cwd);
        }
        for (key, value) in env {
            command.env(key, value);
        }
        // 所有托管会话（新建和恢复）都必须把本机鉴权通道传给 hook 子进程；此前只有
        // start_pending 注入，导致历史会话恢复后 PermissionRequest 无法抵达 GUI。
        if let Ok(endpoint) = self.attach.endpoint.lock() {
            if let Some(endpoint) = *endpoint {
                command.env("MEOWO_PTY_ENDPOINT", endpoint.to_string());
                command.env("MEOWO_PTY_TOKEN", &self.attach.token);
                command.env(
                    "MEOWO_PTY_PROTOCOL",
                    CURRENT_PROTOCOL_VERSION.to_string(),
                );
            }
        }
        command.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        drop(pair.slave);
        let managed = Arc::new(ManagedPty {
            session_id: AtomicI64::new(session_id),
            master: Mutex::new(Some(pair.master)),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            backlog: Mutex::new(VecDeque::new()),
            output_end: AtomicU64::new(0),
            subscribers: Mutex::new(Vec::new()),
            finalized: AtomicBool::new(false),
        });
        sessions.insert(session_id, managed.clone());
        drop(sessions);

        // waiter：收尾的主触发器。Windows ConPTY 在子进程退出后不给 reader EOF（本机实证，
        // 连 drop master 都唤不醒阻塞中的 read），所以收尾绝不能挂在 reader 上——这里轮询
        // try_wait，进程一退就直接执行 finalize_exit。
        // 轮询而非阻塞 wait：wait 要一直握着 child 锁，stop() 的 kill 会和它死锁。
        let waiter = managed.clone();
        let waiter_broker = self.clone();
        let waiter_app = app.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(200));
            if waiter.finalized.load(Ordering::Acquire) {
                return;
            }
            let exited = waiter
                .child
                .lock()
                .ok()
                .and_then(|mut child| child.try_wait().ok())
                .flatten()
                .is_some();
            if exited {
                finalize_exit(&waiter_broker, &waiter_app, &waiter);
                return;
            }
        });

        let broker = self.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let session_id = managed.session_id.load(Ordering::Acquire);
                        let offset = if let Ok(mut backlog) = managed.backlog.lock() {
                            let offset = managed.output_end.load(Ordering::Relaxed);
                            backlog.extend(&buf[..n]);
                            while backlog.len() > BACKLOG_LIMIT {
                                backlog.pop_front();
                            }
                            managed
                                .output_end
                                .store(offset + n as u64, Ordering::Release);
                            offset
                        } else {
                            managed.output_end.fetch_add(n as u64, Ordering::AcqRel)
                        };
                        if let Ok(mut subscribers) = managed.subscribers.lock() {
                            let chunk = buf[..n].to_vec();
                            subscribers.retain(|(_, sender)| sender.send(chunk.clone()).is_ok());
                        }
                        let payload = PtyOutput {
                            session_id,
                            offset,
                            data: base64::engine::general_purpose::STANDARD.encode(&buf[..n]),
                        };
                        if let Some(window) = app.get_webview_window("chat") {
                            let _ = window.emit("pty-output", &payload);
                        }
                    }
                }
            }
            finalize_exit(&broker, &app, &managed);
        });
        Ok(())
    }

    /// 在真实 agent session id 尚未产生前启动 PTY。hook 继承的一次性 token 会在首次落库后
    /// 通过 loopback 服务把这个负数临时 id 原子替换成数据库 id。
    pub(crate) fn start_pending(
        &self,
        app: tauri::AppHandle,
        argv: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        cols: u16,
        rows: u16,
    ) -> Result<i64, String> {
        let endpoint = self
            .attach
            .endpoint
            .lock()
            .map_err(|_| "attach 状态锁已损坏")?
            .ok_or("attach 服务未启动")?;
        let launch_token = random_token();
        let temp_id = self.attach.next_pending.fetch_sub(1, Ordering::Relaxed);
        self.attach
            .pending
            .lock()
            .map_err(|_| "PTY 临时会话锁已损坏")?
            .insert(launch_token.clone(), temp_id);
        let mut launch_env = env.to_vec();
        launch_env.extend([
            ("MEOWO_PTY_ENDPOINT".into(), endpoint.to_string()),
            ("MEOWO_PTY_TOKEN".into(), self.attach.token.clone()),
            ("MEOWO_PTY_LAUNCH".into(), launch_token.clone()),
            (
                "MEOWO_PTY_PROTOCOL".into(),
                CURRENT_PROTOCOL_VERSION.to_string(),
            ),
        ]);
        if let Err(error) = self.start(
            app,
            temp_id,
            argv,
            cwd,
            &launch_env,
            TerminalSize::new(cols, rows),
        ) {
            if let Ok(mut pending) = self.attach.pending.lock() {
                pending.remove(&launch_token);
            }
            return Err(error);
        }
        Ok(temp_id)
    }

    pub(crate) fn write(&self, session_id: i64, data: &[u8]) -> Result<(), String> {
        if data.len() > 64 * 1024 {
            return Err("单次 PTY 输入过大".into());
        }
        let session = self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁已损坏")?
            .get(&session_id)
            .cloned()
            .ok_or("PTY 会话未运行")?;
        let mut writer = session.writer.lock().map_err(|_| "PTY 输入锁已损坏")?;
        writer
            .write_all(data)
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())
    }

    pub(crate) fn resize(&self, session_id: i64, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁已损坏")?
            .get(&session_id)
            .cloned()
            .ok_or("PTY 会话未运行")?;
        let result = session
            .master
            .lock()
            .map_err(|_| "PTY 尺寸锁已损坏")?
            .as_ref()
            .ok_or("PTY 已结束")?
            .resize(size(cols, rows))
            .map_err(|e| e.to_string());
        result
    }

    /// 取会话输出快照。`since` 是调用方已持有的输出末尾偏移（首次传 0）——只返回它之后的
    /// 新字节，避免每次轮询都把整个 backlog（上限 1 MiB）拷贝 + base64 + 过 IPC 传一遍。
    ///
    /// `since` 落在 backlog 起点之前（被裁剪掉了）时返回现存的全部 backlog；晚于当前末尾
    /// （会话换了/被重置）时退化为空增量，**不会**自动回退成全量。调用方一律按响应里的
    /// start_offset/end_offset 对齐，并在重启 PTY 后把自己的 since 归零。
    pub(crate) fn snapshot(&self, session_id: i64, since: u64) -> PtySnapshot {
        let session = self
            .sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(&session_id).cloned());
        let active = session.as_ref().and_then(|s| {
            // 临界区内只做区间计算与切片拷贝：逐字节遍历整个 ring 会阻塞 PTY reader 线程写入。
            s.backlog.lock().ok().map(|b| {
                let end = s.output_end.load(Ordering::Acquire);
                let start = end.saturating_sub(b.len() as u64);
                let skip = since.saturating_sub(start).min(b.len() as u64);
                // since 超前于 end（会话被重置）时 skip 会被夹到 len，退化为空增量；
                // 此时 start_offset == end_offset，前端据此识别并重新对齐。
                let data: Vec<u8> = b.iter().skip(skip as usize).copied().collect();
                (data, start + skip, end)
            })
        });
        let completed = if session.is_none() {
            self.completed
                .lock()
                .ok()
                .and_then(|items| items.get(&session_id).cloned())
        } else {
            None
        };
        let (data, start_offset, end_offset) = if let Some(item) = completed.as_ref() {
            // 已退出的会话：completed 是定格快照，同样按 since 裁剪。
            let skip = since
                .saturating_sub(item.start_offset)
                .min(item.data.len() as u64);
            (
                item.data[skip as usize..].to_vec(),
                item.start_offset + skip,
                item.end_offset,
            )
        } else if let Some((data, start, end)) = active {
            (data, start, end)
        } else {
            (Vec::new(), 0, 0)
        };
        PtySnapshot {
            session_id,
            active: session.is_some(),
            data: base64::engine::general_purpose::STANDARD.encode(data),
            start_offset,
            end_offset,
            exited: completed.is_some(),
            exit_code: completed.and_then(|item| item.code),
        }
    }

    pub(crate) fn stop(&self, session_id: i64) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁已损坏")?
            .get(&session_id)
            .cloned()
            .ok_or("PTY 会话未运行")?;
        let result = session
            .child
            .lock()
            .map_err(|_| "PTY 进程锁已损坏")?
            .kill()
            .map_err(|e| e.to_string());
        // kill 之后收尾由 waiter 在 ~200ms 内接手（finalize_exit）；这里不用做别的。
        result
    }

    /// 启动仅监听 loopback 的 attach 服务。协议不暴露到 LAN，且握手必须携带 256-bit token。
    pub(crate) fn start_attach_server(&self) -> Result<(), String> {
        if self.attach.started.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) => {
                self.attach.started.store(false, Ordering::Release);
                return Err(error.to_string());
            }
        };
        let endpoint = listener.local_addr().map_err(|e| e.to_string())?;
        *self
            .attach
            .endpoint
            .lock()
            .map_err(|_| "attach 状态锁已损坏")? = Some(endpoint);
        // 外部终端没有托管 PTY 注入的环境变量。把仅监听 loopback 的端点和随机 token
        // 登记到当前用户的数据目录，让同一用户启动的 reporter 也能把审批转交 GUI。
        // `pid` 是这份登记的有效性凭据：正常退出时 `shutdown` 会删文件，但崩溃时删不掉，
        // 而端口可能已被无关进程回收——reporter 必须靠 pid 判活来识别陈旧文件（见 attach.rs）。
        #[cfg(not(test))]
        if let Some(dir) = crate::db_path().parent() {
            let discovery = BrokerDiscovery {
                endpoint: endpoint.to_string(),
                token: self.attach.token.clone(),
                pid: std::process::id(),
                protocol_version: CURRENT_PROTOCOL_VERSION,
            };
            if let Ok(json) = serde_json::to_vec(&discovery) {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(APPROVAL_BROKER_FILE), json);
            }
        }
        let broker = self.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let broker = broker.clone();
                std::thread::spawn(move || {
                    let _ = broker.handle_attach(stream);
                });
            }
        });
        Ok(())
    }

    /// GUI 退出时的清理。不做的话：(1) discovery 文件残留，下一个 reporter 会连向一个
    /// 早已不属于我们的端口；(2) 托管 PTY 的子进程被孤儿化（Windows 上 conhost 一并残留）。
    pub(crate) fn shutdown(&self) {
        #[cfg(not(test))]
        if let Some(dir) = crate::db_path().parent() {
            let _ = std::fs::remove_file(dir.join(APPROVAL_BROKER_FILE));
        }
        if let Ok(mut sessions) = self.sessions.lock() {
            for (_, managed) in sessions.drain() {
                if let Ok(mut child) = managed.child.lock() {
                    let _ = child.kill();
                }
                // 同 stop：不关伪终端的话 conhost 会作为孤儿留在系统里。
                if let Ok(mut master) = managed.master.lock() {
                    drop(master.take());
                }
            }
        }
    }

    /// 该会话此刻是否由 Meowo 的 PTY 持有。
    pub(crate) fn is_managed(&self, session_id: i64) -> bool {
        self.sessions
            .lock()
            .is_ok_and(|sessions| sessions.contains_key(&session_id))
    }

    /// 会话已退出时，取退出码与输出尾部的可读文本（诊断用）。仍在运行/从未运行 → None。
    /// CLI 拒绝启动（如 resume 一个正被占用的会话）时，原因只存在于这段输出里。
    pub(crate) fn exit_info(&self, session_id: i64) -> Option<(Option<u32>, String)> {
        let completed = self.completed.lock().ok()?.get(&session_id).cloned()?;
        Some((completed.code, readable_tail(&completed.data, 240)))
    }

    /// 该会话累计产出的输出字节数（仍在运行时）。读一个原子量，可用于高频轮询。
    /// 秒退探测用它当「进程确实活起来了」的信号，好提前结束等待。
    pub(crate) fn output_len(&self, session_id: i64) -> u64 {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| {
                sessions
                    .get(&session_id)
                    .map(|s| s.output_end.load(Ordering::Acquire))
            })
            .unwrap_or(0)
    }

    /// 临时 id → 真实会话 id 的绑定结果。**只读不消费**：对话窗口会重复轮询，负 id 期间
    /// 还可能因 `key={sessionId}` 重挂而再读一次；一次性消费会让其中一方永远等不到真实 id。
    /// 绑定表随 PTY 退出清理（见 reader 线程），不会无限增长。
    pub(crate) fn binding(&self, temp_id: i64) -> Option<i64> {
        self.attach
            .bindings
            .lock()
            .ok()
            .and_then(|bindings| bindings.get(&temp_id).copied())
    }

    pub(crate) fn attach_args(&self, session_id: i64) -> Result<(String, String), String> {
        if !self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁已损坏")?
            .contains_key(&session_id)
        {
            return Err("该会话尚未由 Meowo 接管".into());
        }
        let endpoint = self
            .attach
            .endpoint
            .lock()
            .map_err(|_| "attach 状态锁已损坏")?
            .ok_or("attach 服务未启动")?;
        Ok((endpoint.to_string(), self.attach.token.clone()))
    }

    pub(crate) fn pending_approval(&self, session_id: i64) -> Option<ApprovalRequest> {
        self.attach
            .approvals
            .lock()
            .ok()?
            .values()
            .find(|pending| pending.request.session_id == session_id)
            .map(|pending| pending.request.clone())
    }

    #[cfg(test)]
    pub(crate) fn resolve_approval(
        &self,
        session_id: i64,
        request_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), String> {
        let mut approvals = self
            .attach
            .approvals
            .lock()
            .map_err(|_| "审批状态锁已损坏")?;
        if approvals
            .get(request_id)
            .ok_or("审批请求已结束")?
            .request
            .session_id
            != session_id
        {
            return Err("审批请求不属于该会话".into());
        }
        let pending = approvals.remove(request_id).expect("刚验证存在的审批请求");
        drop(approvals);
        pending
            .response
            .send(decision)
            .map_err(|_| "Agent 已不再等待审批".into())
    }

    pub(crate) fn resolve_approval_choice(
        &self,
        session_id: i64,
        request_id: &str,
        choice: &str,
    ) -> Result<(), String> {
        let mut approvals = self
            .attach
            .approvals
            .lock()
            .map_err(|_| "审批状态锁已损坏")?;
        let pending = approvals.get(request_id).ok_or("审批请求已结束")?;
        if pending.request.session_id != session_id {
            return Err("审批请求不属于该会话".into());
        }
        let decision = match choice {
            "allow_once" => ApprovalDecision::Allow,
            "deny" => ApprovalDecision::Deny,
            value if value.starts_with("suggestion:") => {
                let index = value["suggestion:".len()..]
                    .parse::<usize>()
                    .map_err(|_| "无效的审批选项")?;
                let suggestion = pending
                    .request
                    .permission_suggestions
                    .get(index)
                    .cloned()
                    .ok_or("审批选项已失效")?;
                ApprovalDecision::AllowWithPermissions(vec![suggestion])
            }
            _ => return Err("无效的审批选项".into()),
        };
        let pending = approvals.remove(request_id).expect("刚验证存在的审批请求");
        drop(approvals);
        pending
            .response
            .send(decision)
            .map_err(|_| "Agent 已不再等待审批".into())
    }

    /// 对话窗关闭后 GUI 已不再能消费审批；立即把所有挂起请求交还各自 Agent 的 TUI。
    pub(crate) fn pass_pending_approvals(&self) {
        let pending = self
            .attach
            .approvals
            .lock()
            .map(|mut approvals| approvals.drain().map(|(_, item)| item).collect::<Vec<_>>())
            .unwrap_or_default();
        for item in pending {
            let _ = item.response.send(ApprovalDecision::Pass);
        }
    }

    fn has_approval_consumer(&self, session_id: i64) -> bool {
        self.attach
            .approval_consumers
            .lock()
            .is_ok_and(|consumers| consumers.values().any(|session| *session == session_id))
    }

    pub(crate) fn register_approval_consumer(
        &self,
        session_id: i64,
        consumer_id: String,
    ) -> Result<(), String> {
        if session_id <= 0 || consumer_id.is_empty() || consumer_id.len() > 128 {
            return Err("审批消费者无效".into());
        }
        self.attach
            .approval_consumers
            .lock()
            .map_err(|_| "审批消费者状态锁已损坏".to_string())?
            .insert(consumer_id, session_id);
        Ok(())
    }

    pub(crate) fn unregister_approval_consumer(&self, consumer_id: &str) {
        let session_id = self
            .attach
            .approval_consumers
            .lock()
            .ok()
            .and_then(|mut consumers| {
                let session_id = consumers.remove(consumer_id)?;
                (!consumers.values().any(|session| *session == session_id)).then_some(session_id)
            });
        let Some(session_id) = session_id else { return };
        let pending = self
            .attach
            .approvals
            .lock()
            .map(|mut approvals| {
                let ids = approvals
                    .iter()
                    .filter(|(_, item)| item.request.session_id == session_id)
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>();
                ids.into_iter()
                    .filter_map(|id| approvals.remove(&id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for item in pending {
            let _ = item.response.send(ApprovalDecision::Pass);
        }
    }

    fn handle_attach(&self, mut stream: TcpStream) -> Result<(), String> {
        stream.set_nodelay(true).ok();
        let handshake = read_handshake(&mut stream).map_err(|e| e.to_string())?;
        let BrokerRequest::Attach {
            token,
            session_id,
            cols,
            rows,
            nonce,
        } = handshake
        else {
            return match handshake {
                BrokerRequest::Claim {
                    token,
                    launch_token,
                    session_id,
                } => self.handle_claim(&token, &launch_token, session_id),
                BrokerRequest::Approval { token, request } => {
                    self.handle_approval(&token, request, stream)
                }
                BrokerRequest::Attach { .. } => unreachable!(),
            };
        };
        if token != self.attach.token {
            return Err("attach 认证失败".into());
        }
        // 第六段是客户端 nonce，当前只用于禁止旧五段协议被误接入。
        if nonce.len() < 8 {
            return Err("attach nonce 无效".into());
        }
        // 认证已通过——此后拒绝必须把原因写回给客户端再断开。客户端此时已进入
        // raw 转发模式，收到的字节会原样上屏；一言不发地 drop socket，对端只会
        // 得到一个 exit 0 的 EOF，用户面对的就是一扇纯空白的终端窗。
        let refuse = |mut stream: TcpStream, error: String| -> Result<(), String> {
            let _ = stream.write_all(format!("\r\nMeowo attach: {error}\r\n").as_bytes());
            Err(error)
        };
        if let Err(error) = self.resize(session_id, cols, rows) {
            return refuse(stream, error);
        }
        let session = match self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁已损坏".to_string())
            .and_then(|sessions| {
                sessions
                    .get(&session_id)
                    .cloned()
                    .ok_or_else(|| "PTY 会话未运行".to_string())
            }) {
            Ok(session) => session,
            Err(error) => return refuse(stream, error),
        };
        let subscriber_id = self.attach.next_subscriber.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        // 与 reader 的锁顺序一致：先 backlog 后 subscribers，确保回放与订阅之间没有输出缺口。
        let backlog = {
            let backlog = session.backlog.lock().map_err(|_| "PTY 回放锁已损坏")?;
            session
                .subscribers
                .lock()
                .map_err(|_| "PTY 订阅锁已损坏")?
                .push((subscriber_id, tx));
            backlog.iter().copied().collect::<Vec<_>>()
        };
        let mut output = stream.try_clone().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            if output.write_all(&backlog).is_ok() {
                for chunk in rx {
                    if output.write_all(&chunk).is_err() {
                        break;
                    }
                }
            }
            // PTY 退出时 subscribers 随 ManagedPty 一起 drop；关闭 socket 唤醒客户端与服务端读循环。
            let _ = output.shutdown(Shutdown::Both);
        });

        loop {
            let mut header = [0u8; 5];
            if stream.read_exact(&mut header).is_err() {
                break;
            }
            let kind = header[0];
            let len = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
            if len > 64 * 1024 {
                break;
            }
            let mut payload = vec![0u8; len];
            if stream.read_exact(&mut payload).is_err() {
                break;
            }
            let current_id = session.session_id.load(Ordering::Acquire);
            match kind {
                1 => self.write(current_id, &payload)?,
                2 if payload.len() == 4 => self.resize(
                    current_id,
                    u16::from_be_bytes([payload[0], payload[1]]),
                    u16::from_be_bytes([payload[2], payload[3]]),
                )?,
                _ => break,
            }
        }
        if let Ok(mut subscribers) = session.subscribers.lock() {
            subscribers.retain(|(id, _)| *id != subscriber_id);
        }
        Ok(())
    }

    fn handle_claim(
        &self,
        token: &str,
        launch_token: &str,
        real_id: i64,
    ) -> Result<(), String> {
        if token != self.attach.token {
            return Err("PTY claim 认证失败".into());
        }
        if real_id <= 0 {
            return Err("PTY claim session 无效".into());
        }
        // 先只读 token，直到 sessions 重绑完成才消费。极快启动的 agent 可能在 start() 完成
        // 注册前就触发 hook；这时保留 token，下一次认领仍可成功。
        let temp_id = *self
            .attach
            .pending
            .lock()
            .map_err(|_| "PTY 临时会话锁已损坏")?
            .get(launch_token)
            .ok_or("PTY claim token 无效或已使用")?;
        let managed = {
            let mut sessions = self.sessions.lock().map_err(|_| "PTY 状态锁已损坏")?;
            if sessions.contains_key(&real_id) {
                return Err("真实 PTY 会话已存在".into());
            }
            let managed = sessions.remove(&temp_id).ok_or("临时 PTY 会话已结束")?;
            managed.session_id.store(real_id, Ordering::Release);
            sessions.insert(real_id, managed.clone());
            managed
        };
        if let Ok(mut pending) = self.attach.pending.lock() {
            pending.remove(launch_token);
        }
        if let Ok(mut bindings) = self.attach.bindings.lock() {
            bindings.insert(temp_id, real_id);
        }
        // 保持 Arc 活到映射完成，避免极短命进程在重绑边界提前析构。
        drop(managed);
        Ok(())
    }

    fn handle_approval(
        &self,
        token: &str,
        request: ApprovalRequest,
        mut stream: TcpStream,
    ) -> Result<(), String> {
        if token != self.attach.token {
            return Err("审批通道认证失败".into());
        }
        if request.session_id <= 0 || request.request_id.len() < 8 {
            return Err("审批请求无效".into());
        }
        if !self.ensure_approval_window(request.session_id) {
            return stream.write_all(b"pass\n").map_err(|e| e.to_string());
        }
        let (tx, rx) = mpsc::channel();
        self.attach
            .approvals
            .lock()
            .map_err(|_| "审批状态锁已损坏")?
            .insert(
                request.request_id.clone(),
                PendingApproval {
                    request: request.clone(),
                    response: tx,
                },
            );
        self.emit_approval("pending-approval", &request);
        let decision = rx.recv_timeout(std::time::Duration::from_secs(300)).ok();
        if let Ok(mut approvals) = self.attach.approvals.lock() {
            approvals.remove(&request.request_id);
        }
        self.emit_approval("pending-approval-cleared", &request);
        let response = format!(
            "{}\n",
            decision.unwrap_or(ApprovalDecision::Pass).as_wire()
        );
        stream
            .write_all(response.as_bytes())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 注：曾有一个直接驱动 ConPTY 的实证测试，结论已固化到 finalize_exit / waiter 的注释里
    // （子进程退出后 reader 不 EOF；drop master 也唤不醒阻塞中的 read）。测试本身依赖
    // 测试环境里 ConPTY 的交互行为（cmd /c 甚至不退出），过于 flaky，不保留。

    #[test]
    fn readable_tail_strips_ansi_and_keeps_last_lines() {
        // 秒退诊断的原料是 TUI 首帧：清屏序列 + 标题 OSC + 真正的报错行。
        let raw = b"\x1b[2J\x1b[H\x1b]0;claude\x07noise line\r\nError: Session e9a is already in use\r\n\x1b[31mPlease close the other client.\x1b[0m\r\n";
        let tail = readable_tail(raw, 240);
        assert!(tail.contains("already in use"), "tail={tail}");
        assert!(tail.contains("Please close"), "tail={tail}");
        assert!(!tail.contains('\x1b'));
        // 中文按 UTF-8 完整解码，不得拆成乱码字节。
        let zh = readable_tail("错误：会话被占用\n".as_bytes(), 240);
        assert_eq!(zh, "错误：会话被占用");
        // 限长截断加省略号。
        let long = readable_tail(&[b'a'; 500], 10);
        assert_eq!(long, "aaaaaaaaaa…");
    }

    #[test]
    fn pty_size_is_clamped_to_safe_bounds() {
        let tiny = size(0, 1);
        assert_eq!((tiny.cols, tiny.rows), (2, 2));
        let huge = size(u16::MAX, u16::MAX);
        assert_eq!((huge.cols, huge.rows), (500, 500));
    }

    #[test]
    fn inactive_snapshot_is_empty_and_large_input_is_rejected() {
        let broker = PtyBroker::default();
        let snapshot = broker.snapshot(42, 0);
        assert!(!snapshot.active);
        assert!(!snapshot.exited);
        assert!(snapshot.data.is_empty());
        assert_eq!(snapshot.exit_code, None);
        assert!(broker.write(42, &vec![0; 64 * 1024 + 1]).is_err());
    }

    #[test]
    fn completed_snapshot_preserves_output_offsets() {
        let broker = PtyBroker::default();
        broker.completed.lock().unwrap().insert(
            42,
            CompletedPty {
                data: b"tail".to_vec(),
                start_offset: 96,
                end_offset: 100,
                code: Some(0),
            },
        );

        let snapshot = broker.snapshot(42, 0);
        assert!(!snapshot.active);
        assert!(snapshot.exited);
        assert_eq!(
            snapshot.data,
            base64::engine::general_purpose::STANDARD.encode(b"tail")
        );
        assert_eq!(snapshot.start_offset, 96);
        assert_eq!(snapshot.end_offset, 100);
        assert_eq!(snapshot.exit_code, Some(0));
    }

    /// since 的三种边界：命中中段只回增量、追平后回空、落在已裁剪区之前退化为全量。
    /// 这是「轮询不再每次传整个 backlog」的核心契约。
    #[test]
    fn snapshot_returns_only_bytes_after_since() {
        let decode = |s: &str| base64::engine::general_purpose::STANDARD.decode(s).unwrap();
        let broker = PtyBroker::default();
        broker.completed.lock().unwrap().insert(
            7,
            CompletedPty {
                data: b"abcdef".to_vec(),
                start_offset: 100,
                end_offset: 106,
                code: Some(0),
            },
        );

        // since 在区间中段：只回它之后的字节，start_offset 前移到 since。
        let mid = broker.snapshot(7, 103);
        assert_eq!(decode(&mid.data), b"def");
        assert_eq!((mid.start_offset, mid.end_offset), (103, 106));

        // since 已追平末尾：空增量，稳态轮询的常态——此前每次都要传整份。
        let caught_up = broker.snapshot(7, 106);
        assert!(decode(&caught_up.data).is_empty());
        assert_eq!((caught_up.start_offset, caught_up.end_offset), (106, 106));

        // since 早于 backlog 起点（那段已被裁剪）：退化为全量，不能假装数据还在。
        let stale = broker.snapshot(7, 40);
        assert_eq!(decode(&stale.data), b"abcdef");
        assert_eq!(stale.start_offset, 100);

        // since 超前于末尾（会话被重置）：不得 panic 或越界，回空让前端按 offset 重新对齐。
        let ahead = broker.snapshot(7, 999);
        assert!(decode(&ahead.data).is_empty());
    }

    #[test]
    fn attach_server_rejects_bad_token_and_tokens_are_unique() {
        let first = PtyBroker::default();
        let second = PtyBroker::default();
        assert_eq!(first.attach.token.len(), 64);
        assert_ne!(first.attach.token, second.attach.token);
        first.start_attach_server().unwrap();
        let endpoint = first.attach.endpoint.lock().unwrap().unwrap();
        let mut stream = TcpStream::connect(endpoint).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        writeln!(stream, "MEOWO1 wrong 1 80 24 nonce1234").unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);

        first
            .attach
            .pending
            .lock()
            .unwrap()
            .insert("launch-token".into(), -7);
        assert!(first.handle_claim("wrong", "launch-token", 9).is_err());
        assert_eq!(
            first.attach.pending.lock().unwrap().get("launch-token"),
            Some(&-7)
        );

        first.attach.bindings.lock().unwrap().insert(-7, 9);
        // binding 是只读非消费：对话窗口会重复轮询，重挂后还会再读——一次性消费会让
        // 后到的读方永远等不到真实 id。绑定表由 PTY 退出路径清理（reader 线程 retain）。
        assert_eq!(first.binding(-7), Some(9));
        assert_eq!(first.binding(-7), Some(9));
    }

    #[test]
    fn resolving_wrong_session_does_not_consume_approval() {
        let broker = PtyBroker::default();
        let (tx, rx) = mpsc::channel();
        let request = ApprovalRequest {
            session_id: 7,
            request_id: "request-123".into(),
            provider: "codex".into(),
            tool_name: "Bash".into(),
            description: Some("run tests".into()),
            input: "{}".into(),
            permission_suggestions: vec![],
        };
        broker.attach.approvals.lock().unwrap().insert(
            request.request_id.clone(),
            PendingApproval {
                request: request.clone(),
                response: tx,
            },
        );
        assert_eq!(broker.pending_approval(7).unwrap().tool_name, "Bash");
        assert!(broker
            .resolve_approval(8, "request-123", ApprovalDecision::Allow)
            .is_err());
        broker
            .resolve_approval(7, "request-123", ApprovalDecision::Deny)
            .unwrap();
        assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Deny));
        assert!(broker.pending_approval(7).is_none());
    }

    #[test]
    fn resolving_agent_suggestion_returns_its_original_permission_update() {
        let broker = PtyBroker::default();
        let (tx, rx) = mpsc::channel();
        let suggestion = serde_json::json!({
            "type": "addRules",
            "behavior": "allow",
            "destination": "localSettings",
            "rules": [{ "toolName": "Bash", "ruleContent": "cargo test" }],
        });
        broker.attach.approvals.lock().unwrap().insert(
            "request-options".into(),
            PendingApproval {
                request: ApprovalRequest {
                    session_id: 7,
                    request_id: "request-options".into(),
                    provider: "claude".into(),
                    tool_name: "Bash".into(),
                    description: None,
                    input: r#"{"command":"cargo test"}"#.into(),
                    permission_suggestions: vec![suggestion.clone()],
                },
                response: tx,
            },
        );

        broker
            .resolve_approval_choice(7, "request-options", "suggestion:0")
            .unwrap();
        assert_eq!(
            rx.recv().unwrap(),
            ApprovalDecision::AllowWithPermissions(vec![suggestion])
        );
    }

    #[test]
    fn closing_approval_consumer_passes_every_pending_request() {
        let broker = PtyBroker::default();
        let mut receivers = Vec::new();
        for id in ["request-1", "request-2"] {
            let (tx, rx) = mpsc::channel();
            broker.attach.approvals.lock().unwrap().insert(
                id.into(),
                PendingApproval {
                    request: ApprovalRequest {
                        session_id: 7,
                        request_id: id.into(),
                        provider: "codex".into(),
                        tool_name: "Bash".into(),
                        description: None,
                        input: "{}".into(),
                        permission_suggestions: vec![],
                    },
                    response: tx,
                },
            );
            receivers.push(rx);
        }
        broker.pass_pending_approvals();
        assert!(broker.attach.approvals.lock().unwrap().is_empty());
        assert!(receivers
            .into_iter()
            .all(|rx| matches!(rx.recv().unwrap(), ApprovalDecision::Pass)));
    }

    #[test]
    fn approval_consumer_lease_is_session_scoped_and_releases_on_last_unregister() {
        let broker = PtyBroker::default();
        broker
            .register_approval_consumer(7, "consumer-a".into())
            .unwrap();
        broker
            .register_approval_consumer(7, "consumer-b".into())
            .unwrap();
        let (tx, rx) = mpsc::channel();
        broker.attach.approvals.lock().unwrap().insert(
            "request-7".into(),
            PendingApproval {
                request: ApprovalRequest {
                    session_id: 7,
                    request_id: "request-7".into(),
                    provider: "codex".into(),
                    tool_name: "Bash".into(),
                    description: None,
                    input: "{}".into(),
                    permission_suggestions: vec![],
                },
                response: tx,
            },
        );

        broker.unregister_approval_consumer("consumer-a");
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        assert!(broker.pending_approval(7).is_some());

        broker.unregister_approval_consumer("consumer-b");
        assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Pass));
        assert!(broker.pending_approval(7).is_none());
    }

    #[test]
    fn external_approval_passes_immediately_without_gui_consumer() {
        let broker = PtyBroker::default();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = broker.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server.handle_attach(stream).unwrap();
        });
        let request = ApprovalRequest {
            session_id: 77,
            request_id: "external-request-77".into(),
            provider: "codex".into(),
            tool_name: "Bash".into(),
            description: Some("build release".into()),
            input: "{}".into(),
            permission_suggestions: vec![],
        };
        let encoded =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&request).unwrap());
        let mut stream = TcpStream::connect(endpoint).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        writeln!(stream, "MEOWOAPPROVAL1 {} {}", broker.attach.token, encoded).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert_eq!(response, "pass\n");
        assert!(broker.pending_approval(77).is_none());
        handle.join().unwrap();
    }

    #[test]
    fn external_v2_approval_uses_the_shared_framing_and_passes_without_consumer() {
        let broker = PtyBroker::default();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let server = broker.clone();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            server.handle_attach(stream).unwrap();
        });
        let request = ApprovalRequest {
            session_id: 78,
            request_id: "external-request-78".into(),
            provider: "codex".into(),
            tool_name: "Bash".into(),
            description: Some("build release".into()),
            input: "{}".into(),
            permission_suggestions: vec![],
        };
        let mut stream = TcpStream::connect(endpoint).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        meowo_protocol::broker::write_v2_handshake(
            &mut stream,
            &BrokerRequest::Approval {
                token: broker.attach.token.clone(),
                request,
            },
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert_eq!(response, "pass\n");
        assert!(broker.pending_approval(78).is_none());
        handle.join().unwrap();
    }
}
