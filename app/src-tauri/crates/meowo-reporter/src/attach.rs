//! 外部终端 attach 客户端：PTY 仍由 Meowo GUI 进程持有，本进程只转发 stdin/stdout/resize。

use meowo_protocol::broker::{
    encode_legacy_approval, encode_legacy_attach, encode_legacy_claim, write_v2_handshake,
    ApprovalDecision, ApprovalRequest, BrokerDiscovery, BrokerRequest, APPROVAL_BROKER_FILE,
    CURRENT_PROTOCOL_VERSION,
};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 连接握手的上限。审批本身要等用户，但**建连**不该等——见 `request_approval` 的超时说明。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// PermissionRequest hook 与 GUI 之间的同步审批。只在 Meowo 托管 PTY 注入的鉴权环境中启用；
/// 连接失败或五分钟无人处理就返回 None，让 Agent 自己的 TUI 接管审批，绝不静默放行。
pub(crate) fn request_approval(
    session_id: i64,
    provider: &str,
    tool_name: &str,
    tool_input: Option<&serde_json::Value>,
    permission_suggestions: &[serde_json::Value],
    // PreToolUse 阶段的 AskUserQuestion 代答桥置 true；broker 据此挂起等 GUI 作答，
    // 而不是走 PermissionRequest 的自动放行。
    pre_tool_use: bool,
) -> Option<ApprovalDecision> {
    let primary = approval_broker()?;
    let request_id = format!(
        "{}-{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos()
    );
    let description = tool_input
        .and_then(|input| input.get("description"))
        .and_then(|value| value.as_str());
    let mut input = tool_input
        .and_then(|value| serde_json::to_string_pretty(value).ok())
        .unwrap_or_default();
    if input.len() > 16 * 1024 {
        let mut end = 16 * 1024;
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        input.truncate(end);
        input.push_str("\n…");
    }
    let payload = ApprovalRequest {
        session_id,
        request_id,
        provider: provider.to_string(),
        tool_name: tool_name.to_string(),
        description: description.map(str::to_string),
        input,
        permission_suggestions: permission_suggestions.to_vec(),
        pre_tool_use,
    };
    match send_approval(&primary, &payload) {
        ApprovalAttempt::Answered(decision) => decision,
        // 环境变量里的 broker 连不上或当场拒了凭据：多半是 Meowo 重启过（升级安装后
        // 重开），会话进程却还活着、托管终端也被新实例接着用——env 记的是旧实例的端口
        // 和 scoped token。不退回 discovery 的话，这个会话此后每一次审批/提问都漏接，
        // 只剩终端里的表单（实拍：手机端卡在「请在桌面端作答」）。discovery 文件由当前
        // 活着的实例写、带判活。只在**端点不同**（换过实例）时退回：同一实例拒了 scoped
        // token 是跨会话提交的防线（见 app 侧 handle_approval），不能拿 full token 绕过。
        ApprovalAttempt::Unreachable => match discovery_broker() {
            Some(fallback) if fallback.0 != primary.0 => match send_approval(&fallback, &payload) {
                ApprovalAttempt::Answered(decision) => decision,
                ApprovalAttempt::Unreachable => None,
            },
            _ => None,
        },
    }
}

/// 一次审批请求的结局。Unreachable 只表示「这个 broker 根本没受理」（连不上、握手写失败、
/// 或建连后立刻被关——凭据不对时 broker 不回任何字节就断开），可以换一个 broker 再试；
/// Answered 表示对端受理过（含等满超时无人作答的 None），绝不重试，免得让用户等两轮。
enum ApprovalAttempt {
    Unreachable,
    Answered(Option<ApprovalDecision>),
}

/// 判「当场被拒」的时限：凭据错误时 broker 校验后立即断开，远小于此；真等用户作答至少
/// 也是人的反应时间。空响应且早于它返回 = 没受理。
const REJECTED_WITHIN: Duration = Duration::from_secs(3);

fn send_approval(broker: &(String, String, u16), payload: &ApprovalRequest) -> ApprovalAttempt {
    let (endpoint, token, protocol) = broker;
    let Some(addr) = endpoint.to_socket_addrs().ok().and_then(|mut a| a.next()) else {
        return ApprovalAttempt::Unreachable;
    };
    // 305s 读超时是给**用户**决策留的时间，前提是对端确实是 Meowo。建连则必须有独立上限：
    // 裸 connect 在对端端口被无关进程回收时会一直挂着，把 PermissionRequest hook 拖满
    // 310s（install-hooks.mjs 为审批放宽的上限），期间 agent 完全卡死。
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return ApprovalAttempt::Unreachable;
    };
    // 提问代答不限时（见 QUESTION_WAIT_SECS），读超时跟着放长，比 broker 的等待上限多 5s。
    let read_timeout = if payload.tool_name == "AskUserQuestion" && payload.pre_tool_use {
        meowo_protocol::broker::QUESTION_WAIT_SECS + 5
    } else {
        305
    };
    if stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .and_then(|_| stream.set_read_timeout(Some(Duration::from_secs(read_timeout))))
        .is_err()
    {
        return ApprovalAttempt::Unreachable;
    }
    let started = std::time::Instant::now();
    let written = if *protocol >= CURRENT_PROTOCOL_VERSION {
        write_v2_handshake(
            &mut stream,
            &BrokerRequest::Approval {
                token: token.clone(),
                request: payload.clone(),
            },
        )
        .is_ok()
    } else {
        encode_legacy_approval(token, payload)
            .ok()
            .is_some_and(|handshake| stream.write_all(handshake.as_bytes()).is_ok())
    };
    if !written {
        return ApprovalAttempt::Unreachable;
    }
    let mut response = String::new();
    let read = stream.read_to_string(&mut response);
    if response.trim().is_empty() && started.elapsed() < REJECTED_WITHIN {
        return ApprovalAttempt::Unreachable;
    }
    if read.is_err() {
        return ApprovalAttempt::Answered(None);
    }
    ApprovalAttempt::Answered(ApprovalDecision::from_wire(&response))
}

fn approval_broker() -> Option<(String, String, u16)> {
    match (
        std::env::var("MEOWO_PTY_ENDPOINT"),
        std::env::var("MEOWO_PTY_TOKEN"),
    ) {
        (Ok(endpoint), Ok(token)) if !endpoint.is_empty() && !token.is_empty() => {
            let protocol = std::env::var("MEOWO_PTY_PROTOCOL")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            return Some((endpoint, token, protocol));
        }
        _ => {}
    }
    discovery_broker()
}

/// 当前活着的 Meowo 实例写下的 broker 地址（`~/.meowo` 的 discovery 文件）。
fn discovery_broker() -> Option<(String, String, u16)> {
    let path = crate::db_path()?.parent()?.join(APPROVAL_BROKER_FILE);
    let discovery: BrokerDiscovery = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if discovery.endpoint.is_empty() || discovery.token.is_empty() {
        return None;
    }
    // GUI 崩溃时这个文件会留在盘上（正常退出才会删）。写它的进程已经不在 → 端口随时可能
    // 被无关进程占用，此时连过去毫无意义：要么被拒，要么在一个不懂我方协议的对端上白等。
    // 直接放弃桥接，让 agent 自己的 TUI 接管审批——绝不因为发现文件过期就静默放行。
    if !pid_alive(discovery.pid) {
        return None;
    }
    Some((
        discovery.endpoint,
        discovery.token,
        discovery.protocol_version,
    ))
}

/// discovery 文件里的 GUI 进程是否还活着。只按单个 pid 刷新，不做全量进程扫描——
/// 这段跑在 hook 的关键路径上。
fn pid_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    if pid == 0 {
        return false;
    }
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    // remove_dead_processes=true：System 是本函数新建的空实例，这里只影响「已死的 pid 不会被
    // 留在表里」，正是判活需要的语义。
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new(),
    );
    sys.process(pid).is_some()
}

/// SessionStart 落库后，用继承自托管 PTY 的一次性环境变量把临时 PTY 绑定到真实数据库会话。
/// 失败必须静默：reporter 的首要契约是永不阻塞 agent hook。
///
/// `agent_pid` = 本次 hook 的会话本体 pid（proc::owner_pid）。这些环境变量会被会话内 Bash
/// 起的嵌套 agent（`claude -p` 探针等）原样继承——broker 端靠这个 pid 把嵌套 agent 的
/// 误认领与真 /clear 换代区分开（见 app 侧 pty.rs 的换代守卫）。
pub(crate) fn notify_claim(session_id: i64, agent_pid: Option<u32>) {
    // 会话 id 是 reporter 落库的那个库里的编号。broker 用的是另一个库（会话内 Bash 跑的测试、
    // 探针用 MEOWO_DB 指开了临时库，却继承了外层的 PTY 凭据），这个 id 拿去认领只会把外层
    // 会话的终端换绑到一个不存在或不相干的会话上（实拍 1537 → 1）。旧 broker 不给
    // MEOWO_PTY_DB 时照旧认领。
    if let Ok(broker_db) = std::env::var("MEOWO_PTY_DB") {
        match meowo_reporter::db_path() {
            Some(own) if same_db(&own.to_string_lossy(), &broker_db) => {}
            _ => return,
        }
    }
    let Ok(endpoint) = std::env::var("MEOWO_PTY_ENDPOINT") else {
        return;
    };
    let Ok(token) = std::env::var("MEOWO_PTY_TOKEN") else {
        return;
    };
    let Ok(launch) = std::env::var("MEOWO_PTY_LAUNCH") else {
        return;
    };
    let protocol = std::env::var("MEOWO_PTY_PROTOCOL")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    // 与 request_approval 同理（见 CONNECT_TIMEOUT 的说明）：裸 connect 在对端端口被无关
    // 进程回收时会一直挂着，hook 进程随之卡死。claim 只是条一次性通知，解析失败/超时直接
    // 放弃——首要契约是绝不阻塞 agent。
    let Ok(mut addrs) = endpoint.to_socket_addrs() else {
        return;
    };
    let Some(addr) = addrs.next() else {
        return;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    if protocol >= CURRENT_PROTOCOL_VERSION {
        let _ = write_v2_handshake(
            &mut stream,
            &BrokerRequest::Claim {
                token,
                launch_token: launch,
                session_id,
                pid: agent_pid,
            },
        );
    } else {
        let handshake = encode_legacy_claim(&token, &launch, session_id);
        let _ = stream.write_all(handshake.as_bytes());
    }
}

/// 两个库路径是不是同一个文件。不做 canonicalize（库可能还没建、网络盘慢）：统一斜杠方向，
/// Windows 上路径不分大小写。
fn same_db(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.trim().replace('\\', "/");
        let p = p.trim_end_matches('/').to_string();
        if cfg!(windows) {
            p.to_lowercase()
        } else {
            p
        }
    };
    !a.trim().is_empty() && norm(a) == norm(b)
}

/// TUI 遗留的私有模式收回序列。agent 自己正常退出时会收，**崩溃 / 被 Meowo 结束 /
/// Ctrl-C 强退时来不及**——而 attach 客户端一退出，窗口就还给宿主 shell，本地终端却
/// 还开着这些模式：
///
/// - **鼠标上报**（1000/1002/1003 + 编码 1005/1006/1015）：鼠标一动，终端就往 shell 的
///   stdin 灌 `ESC[<35;x;yM`，PSReadLine 逐个回显——屏幕上字符不停地刷（实拍报告
///   「退出 agent 后管道内的字符一直在刷」）。这是本序列的主要动机。
/// - **括号粘贴**（2004）：之后在 shell 里粘贴会带上 `ESC[200~` 包裹，成为杂字符。
/// - **备用屏**（1049 / 老式 1047、47）：shell 落在没有 scrollback 的备用屏里。
/// - **硬件光标**（25）：claude 启动即 `?25l` 自绘光标，残留则 shell 里看不见光标。
///
/// 全部幂等：模式本来没开时写关闭序列无副作用。顺序上鼠标/粘贴在前、备用屏在后
/// （`?1049l` 切回主屏并恢复光标位置，之后的 SGR 复位作用在主屏上）。
///
/// GUI 终端页有同款收回（`ManagedTerminal` 的 `MOUSE_MODES_OFF`），但那里**刻意不动
/// 备用屏**——那是个只读画面容器，最后一帧正是用户要看的。外部终端这边处境相反：
/// 窗口要交还给 shell 继续用，备用屏必须切回。
const MODES_OFF: &[u8] = b"\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1006l\x1b[?1005l\x1b[?1015l\x1b[?2004l\x1b[?1049l\x1b[?1047l\x1b[?47l\x1b[?25h\x1b[0m";

/// 把终端交还给宿主 shell：收回 TUI 遗留的模式，再退出 raw mode。幂等，可重复调用。
fn restore_terminal() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(MODES_OFF);
    let _ = stdout.flush();
    let _ = crossterm::terminal::disable_raw_mode();
}

/// 退出守卫：**任何**返回路径都要还原终端——正常 EOF、`?` 传播的写失败、握手后的早退。
/// 漏掉一条，用户就会拿到一扇「一动鼠标就刷字符」的 shell 窗口。
struct RawGuard;
impl Drop for RawGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// crossterm 的 raw mode 只关 LINE/ECHO/PROCESSED；不开 ENABLE_VIRTUAL_TERMINAL_INPUT 的话，
/// `stdin.read()` 读不到终端以 VT 序列注入的输入——方向键、功能键、以及终端对
/// `ESC[6n` 的自动回应全都到不了转发线程。
#[cfg(windows)]
fn enable_vt_input() {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_INPUT,
        STD_INPUT_HANDLE,
    };
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) != 0 {
            let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_INPUT);
        }
    }
}
#[cfg(not(windows))]
fn enable_vt_input() {}

/// 从服务端字节流里拦截 `ESC[6n`（光标位置查询）。TUI（claude 等）启动时靠它探测终端，
/// **得不到回应就永远不画第一帧**；本地终端对它的回应又未必能穿过 stdin 链路回到 PTY，
/// 故由 attach 客户端代答（检测到即回 `ESC[1;1R`），并把查询从展示流中吞掉，防止本地
/// 终端也回一份。分工（与 app 侧约定）：**启动探测由 app 的 PTY reader 单趟代答并从
/// 流中摘除**（pty.rs StartupProbeScanner）——本过滤器根本看不到首帧前的探测，不存在
/// 「attach 先建立、探测后到时两头各答一遍」的窗口；回放 backlog 里的历史查询也已被
/// 服务端滤掉（strip_dsr_queries）——曾经每次重开外部同步终端都把历史查询再代答一遍，
/// 迟到的应答打进 agent 输入框（composer 里凭空多个 C）。本过滤器如今只会遇到订阅
/// 之后、首帧之后的**实时**查询，那才是它该答的。
struct DsrFilter {
    pending: Vec<u8>,
}

impl DsrFilter {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// 返回（应打印到本地终端的字节, 检测到的查询个数）。跨 chunk 的部分前缀留待下一轮。
    fn feed(&mut self, chunk: &[u8]) -> (Vec<u8>, usize) {
        const PATTERN: &[u8] = b"\x1b[6n";
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(chunk);
        let mut out = Vec::with_capacity(data.len());
        let mut hits = 0;
        let mut i = 0;
        while i < data.len() {
            if data[i] == 0x1b {
                let rest = &data[i..];
                if rest.len() >= PATTERN.len() {
                    if &rest[..PATTERN.len()] == PATTERN {
                        hits += 1;
                        i += PATTERN.len();
                        continue;
                    }
                } else if PATTERN.starts_with(rest) {
                    self.pending = rest.to_vec();
                    break;
                }
            }
            out.push(data[i]);
            i += 1;
        }
        (out, hits)
    }
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn write_frame(stream: &Arc<Mutex<TcpStream>>, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut stream = stream
        .lock()
        .map_err(|_| std::io::Error::other("attach writer poisoned"))?;
    stream.write_all(&[kind])?;
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

pub(crate) fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let session = arg_value(args, "--session").ok_or("missing --session")?;
    // token 常规不走 argv（进程参数对同机其他进程可见）：GUI 只传 --session，
    // endpoint/token/protocol 从 discovery 文件解析——与审批桥接同一来源，
    // 同样的 pid 判活挡掉陈旧文件。显式传参保留为调试后门。
    let (endpoint, token, protocol) = match arg_value(args, "--token") {
        Some(token) => (
            arg_value(args, "--endpoint").ok_or("missing --endpoint")?,
            token,
            arg_value(args, "--protocol")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
        ),
        None => approval_broker().ok_or("未发现运行中的 Meowo（attach 需要 GUI 先启动）")?,
    };
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let nonce = format!(
        "{:x}{:x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );

    let mut stream = TcpStream::connect(endpoint)?;
    stream.set_nodelay(true)?;
    if protocol >= CURRENT_PROTOCOL_VERSION {
        let session_id = session.parse().map_err(|_| "invalid --session")?;
        write_v2_handshake(
            &mut stream,
            &BrokerRequest::Attach {
                token,
                session_id,
                cols,
                rows,
                nonce,
                // 自身 pid 沿祖先链能定位到宿主终端窗口，GUI 查重时据此精确聚焦。
                pid: Some(std::process::id()),
            },
        )?;
    } else {
        let handshake = encode_legacy_attach(&token, &session, cols, rows, &nonce);
        stream.write_all(handshake.as_bytes())?;
    }
    crossterm::terminal::enable_raw_mode()?;
    let _raw = RawGuard;
    enable_vt_input();

    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let done = Arc::new(AtomicBool::new(false));
    let input_writer = writer.clone();
    let input_done = done.clone();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        while !input_done.load(Ordering::Acquire) {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) if write_frame(&input_writer, 1, &buf[..n]).is_err() => break,
                Ok(_) => {}
            }
        }
    });
    let resize_writer = writer.clone();
    let resize_done = done.clone();
    std::thread::spawn(move || {
        let mut previous = (cols, rows);
        while !resize_done.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(150));
            let current = crossterm::terminal::size().unwrap_or(previous);
            if current != previous {
                let payload = [current.0.to_be_bytes(), current.1.to_be_bytes()].concat();
                if write_frame(&resize_writer, 2, &payload).is_err() {
                    break;
                }
                previous = current;
            }
        }
    });

    let mut stdout = std::io::stdout().lock();
    let mut buf = [0u8; 16 * 1024];
    let mut dsr = DsrFilter::new();
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let (visible, queries) = dsr.feed(&buf[..n]);
                // 代答光标位置查询：TUI 只是要一个答案当基准，(1,1) 足够；真实排版
                // 靠的是后续的清屏与绝对定位序列，不依赖这个值。
                for _ in 0..queries {
                    write_frame(&writer, 1, b"\x1b[1;1R")?;
                }
                if !visible.is_empty() {
                    stdout.write_all(&visible)?;
                    stdout.flush()?;
                }
            }
        }
    }
    done.store(true, Ordering::Release);
    drop(stdout);
    // 先还原终端再说话：raw mode 不退，\n 不回车、文本叠在残留画面上；备用屏不切回，
    // 这句话就打在马上要被丢弃的那一屏上（见 MODES_OFF）。RawGuard 里还有一遍兜底
    // 覆盖早退路径，幂等重复无副作用。
    // 连接断开必须有一句人话：服务端拒绝时错误已在上面原样上屏，这里补的是
    // 「正常结束」的情形——否则窗口就是一片无解释的静止画面（或纯空白）。
    restore_terminal();
    println!("\n[Meowo] 连接已关闭（会话已结束，或在 Meowo 中被停止）。");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_only_against_the_same_db() {
        assert!(same_db(
            r"C:\Users\me\.meowo\board.db",
            "C:/Users/me/.meowo/board.db"
        ));
        // 会话内跑的测试 / 探针用 MEOWO_DB 指开的临时库：不认领。
        assert!(!same_db(
            r"C:\Temp\probe\board.db",
            r"C:\Users\me\.meowo\board.db"
        ));
        assert!(!same_db("", ""));
        #[cfg(windows)]
        assert!(same_db(
            r"c:\users\ME\.meowo\board.db",
            r"C:\Users\me\.meowo\board.db"
        ));
    }

    fn sample_request() -> ApprovalRequest {
        ApprovalRequest {
            session_id: 7,
            request_id: "req-12345678".into(),
            provider: "claude".into(),
            tool_name: "AskUserQuestion".into(),
            description: None,
            input: String::new(),
            permission_suggestions: vec![],
            pre_tool_use: true,
        }
    }

    /// 起一个只接一次连接的假 broker：读完握手后按 reply 回包（None = 不回字节直接断开，
    /// 即凭据不对时 broker 的行为）。
    fn fake_broker(reply: Option<&'static str>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // 读干净整段握手再回包：留着未读字节就关闭，Windows 会发 RST，客户端读不到回包。
            stream
                .set_read_timeout(Some(Duration::from_millis(300)))
                .unwrap();
            let mut buf = [0u8; 4096];
            while matches!(stream.read(&mut buf), Ok(n) if n > 0) {}
            if let Some(reply) = reply {
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        endpoint
    }

    /// 换过 Meowo 实例后 env 里的 broker 失效：连不上与「当场拒收」都要判成 Unreachable，
    /// 调用方才会退回 discovery；对端受理并作答的照常返回决定。
    #[test]
    fn send_approval_tells_unreachable_from_answered() {
        let dead = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().to_string()
        };
        let request = sample_request();
        assert!(matches!(
            send_approval(&(dead, "t".into(), CURRENT_PROTOCOL_VERSION), &request),
            ApprovalAttempt::Unreachable
        ));
        let rejecting = fake_broker(None);
        assert!(matches!(
            send_approval(&(rejecting, "t".into(), CURRENT_PROTOCOL_VERSION), &request),
            ApprovalAttempt::Unreachable
        ));
        let answering = fake_broker(Some("allow\n"));
        assert!(matches!(
            send_approval(&(answering, "t".into(), CURRENT_PROTOCOL_VERSION), &request),
            ApprovalAttempt::Answered(Some(ApprovalDecision::Allow))
        ));
    }

    #[test]
    fn parses_exact_flag_values() {
        let args = vec!["attach".into(), "--token".into(), "abc".into()];
        assert_eq!(arg_value(&args, "--token").as_deref(), Some("abc"));
        assert_eq!(arg_value(&args, "--session"), None);
    }

    #[test]
    fn dsr_filter_intercepts_queries_including_split_chunks() {
        let mut filter = DsrFilter::new();
        // 完整查询被吞、可见内容保留。
        let (visible, hits) = filter.feed(b"ab\x1b[6ncd");
        assert_eq!((visible.as_slice(), hits), (b"abcd".as_slice(), 1));
        // 查询跨 chunk 边界：前缀暂存，补齐后计数，不漏也不把前缀当内容打出去。
        let (visible, hits) = filter.feed(b"xy\x1b[");
        assert_eq!((visible.as_slice(), hits), (b"xy".as_slice(), 0));
        let (visible, hits) = filter.feed(b"6nz");
        assert_eq!((visible.as_slice(), hits), (b"z".as_slice(), 1));
        // 非查询的 ESC 序列原样通过（只认精确的 ESC[6n）。
        let (visible, hits) = filter.feed(b"\x1b[31mred\x1b[0m");
        assert_eq!(
            (visible.as_slice(), hits),
            (b"\x1b[31mred\x1b[0m".as_slice(), 0)
        );
        // 尾部恰好是 ESC：暂存，下一轮是普通序列则完整放行。
        let (visible, _) = filter.feed(b"tail\x1b");
        assert_eq!(visible.as_slice(), b"tail");
        let (visible, hits) = filter.feed(b"[2J");
        assert_eq!((visible.as_slice(), hits), (b"\x1b[2J".as_slice(), 0));
    }
}
