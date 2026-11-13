//! Codex 原生 TUI 的自动续跑（路线 C1，任务 .lead/20261113-lead-tui）。
//!
//! 启动：Lead 已安装的 codex 会话由 Meowo 先起一个**只属于该会话**的 `codex app-server`
//! （ws://127.0.0.1:<端口> + capability token），再让托管终端里的 TUI 以 `--remote` 连上去。
//! 答复：宿主作为第二个客户端连同一个 app-server，用 `thread/queue/add` 把答复排进 TUI 正在用的
//! thread——等于用户在 TUI 里按 Tab 排队，空闲时立刻开新回合，TUI 上看得到。
//!
//! 为什么这样拆（codex 0.167.2 隔离实测，2026-11-13，假模型探针 + 真模型探针）：
//! - TUI 带 `-c`/`--enable`/`--disable`/`-p`/`--search` 会被迫进入内嵌模式（警告原文
//!   「command-line configuration overrides … requires embedded mode」），外部没有任何通道。
//!   所以这些覆盖全部交给 app-server 进程；`-m`/`-s`/`-a`/`--dangerously-bypass…` 新建时仍由 TUI
//!   带（远程模式下实测生效：请求元数据 model=fake-model-2、sandbox_mode=danger-full-access）；
//!   恢复时 TUI 拒收权限参数，thread 沿用自己存下的权限（见 PERMISSION_FLAGS）。
//! - hook 由 app-server 派生，拿到的是 app-server 的环境（实测 MEOWO_E4=server-env），所以把托管
//!   终端的整份环境（含 MEOWO_PTY_* 认领凭据）交给 app-server；MCP 子进程环境被 codex 过滤
//!   （实测拿不到父环境），仍须靠 `-c mcp_servers.*.env=…` 显式下发——这些 `-c` 正好随迁。
//! - originator（计费归属）由**第一个** initialize 的客户端决定：TUI 先握手时宿主连上来拿到的
//!   userAgent 以 `codex-tui/` 开头，宿主先握手则变成 `meowo/`。宿主自报 clientInfo 名 meowo
//!   （不冒充），连上后核对 userAgent 前缀，不对就不投递。queue 投递的请求头 originator 仍是
//!   codex-tui，只有 UA 末尾客户端名与 turn_trigger=queue 不同。
//! - 不用 `turn/start`：那是插话（打断/并入当前回合）；queue 不动输入框草稿（实测草稿原样保留）。
//! - TUI 退出后 thread 仍在 app-server 里加载着，queue/add 会当场跑完这一轮，下次 resume 看得到；
//!   app-server 也不在时，`codex queue --thread` 只把消息落进 `$CODEX_HOME/queue_1.sqlite`、
//!   不调模型，下次 resume 由 TUI 消费（originator=codex-tui）。
//! - 不走 ACP、不往 PTY 打字、不用 `codex exec`（originator=codex_exec，可能另算）。
use super::ws;
use super::{NativeOutcome, NativeTarget};
use meowo_protocol::ipc::LeadDeliveryDto;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// TUI 读 token 的环境变量名（`--remote-auth-token-env`）。不放进 app-server 的环境：
/// hook/MCP 都是 app-server 的后代，没必要让它们拿到连回服务的凭据。
const TOKEN_ENV: &str = "MEOWO_CODEX_REMOTE_TOKEN";
/// TUI 退出后 app-server 继续存活多久（期间答复当场跑完，下次 resume 可见）。默认 2 小时：
/// 手机上答关卡常在离开电脑之后；更久的由冷路径 `codex queue` 兜住，不靠常驻进程。
const LINGER_ENV: &str = "MEOWO_CODEX_SERVER_LINGER_SECS";
const DEFAULT_LINGER_SECS: u64 = 2 * 60 * 60;
/// TUI 启动到完成 initialize 实测 <1s；宿主至少等这么久才连，避免抢先握手把 originator 变成 meowo。
const TUI_HANDSHAKE_GRACE_MS: i64 = 5_000;
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// 只读探询（服务忙不忙）的单次时限：在启动会话的路径上，宁可判「查不了」也不能卡住开终端。
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// 自动重试（关卡巡检）的最小间隔，避免服务不可达时每轮巡检都刷一次。
const RETRY_COOLDOWN_MS: i64 = 20_000;

// ─── 启动参数拆分 ───────────────────────────────────────────────────────────

/// 原 argv 拆成：可执行前缀（`codex.exe` 或 `node …/codex.js`）、交给 app-server 的配置覆盖、留给 TUI 的其余参数。
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RemoteSplit {
    pub prefix: Vec<String>,
    pub server: Vec<String>,
    pub tui: Vec<String>,
}

/// TUI 上带值的参数：值原样跟着走，不被误当成 `-c`/子命令。
const TUI_VALUE_FLAGS: &[&str] = &[
    "-m",
    "--model",
    "-C",
    "--cd",
    "--add-dir",
    "-i",
    "--image",
    "--local-provider",
];

/// 权限参数（值跟随）。远程模式下 resume/fork 不接受它们：0.167.2 实测原文
/// 「Permission overrides are not supported when resuming a remote task.」直接退出；
/// 恢复的 thread 沿用自己存下的权限（实测服务端 `-c sandbox_mode=…` 也覆盖不了），故恢复时丢弃。
const PERMISSION_VALUE_FLAGS: &[&str] = &["-s", "--sandbox", "-a", "--ask-for-approval"];
const PERMISSION_FLAGS: &[&str] = &[
    "--dangerously-bypass-approvals-and-sandbox",
    "--approve-for-me",
    "--full-auto",
];

/// 返回 None = 这条命令不能改成远程模式，按原样内嵌启动（不丢功能，只是没有自动续跑）。
/// `-p`/`--search` app-server 不认、`--no-daemon`/`--remote` 是用户自己定了连接方式、其余子命令不是交互 TUI。
pub(crate) fn split_argv(argv: &[String], owns: impl Fn(&str) -> bool) -> Option<RemoteSplit> {
    let exe_end = argv.iter().take(3).position(|arg| {
        let name = Path::new(arg)
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        owns(&name) || name == "codex.js"
    })?;
    let prefix = argv[..=exe_end].to_vec();
    let mut server = Vec::new();
    let mut tui = Vec::new();
    let mut rest = argv[exe_end + 1..].iter();
    let mut positional = 0;
    let mut resuming = false;
    let mut permission = Vec::new();
    while let Some(arg) = rest.next() {
        let arg = arg.as_str();
        if let Some((flag, value)) = arg.split_once('=').filter(|(f, _)| f.starts_with("--")) {
            match flag {
                "--config" => server.extend(["-c".into(), value.to_owned()]),
                "--enable" | "--disable" => server.extend([flag.to_owned(), value.to_owned()]),
                "--profile" | "--remote" | "--remote-auth-token-env" => return None,
                "--sandbox" | "--ask-for-approval" => permission.push(arg.to_owned()),
                _ => tui.push(arg.to_owned()),
            }
            continue;
        }
        match arg {
            "-c" | "--config" => server.extend(["-c".into(), rest.next()?.clone()]),
            "--enable" | "--disable" => server.extend([arg.to_owned(), rest.next()?.clone()]),
            "-p"
            | "--profile"
            | "--search"
            | "--no-daemon"
            | "--remote"
            | "--remote-auth-token-env"
            | "--oss" => return None,
            _ if arg.starts_with("-c") && !arg.starts_with("--") && arg.len() > 2 => {
                server.extend(["-c".into(), arg[2..].to_owned()]);
            }
            _ if PERMISSION_VALUE_FLAGS.contains(&arg) => {
                permission.push(arg.to_owned());
                permission.push(rest.next()?.clone());
            }
            _ if PERMISSION_FLAGS.contains(&arg) => permission.push(arg.to_owned()),
            _ if TUI_VALUE_FLAGS.contains(&arg) => {
                tui.push(arg.to_owned());
                tui.push(rest.next()?.clone());
            }
            _ if arg.starts_with('-') => tui.push(arg.to_owned()),
            _ => {
                // 第一个位置参数若是子命令，只放行交互式的 resume/fork；第二个起是会话 id 等。
                if positional == 0 && !matches!(arg, "resume" | "fork") && is_known_subcommand(arg)
                {
                    return None;
                }
                resuming |= positional == 0 && matches!(arg, "resume" | "fork");
                positional += 1;
                tui.push(arg.to_owned());
            }
        }
    }
    if !resuming {
        tui.splice(0..0, permission);
    }
    Some(RemoteSplit {
        prefix,
        server,
        tui,
    })
}

fn is_known_subcommand(arg: &str) -> bool {
    matches!(
        arg,
        "exec"
            | "e"
            | "review"
            | "login"
            | "logout"
            | "mcp"
            | "plugin"
            | "app-server"
            | "remote-control"
            | "app"
            | "completion"
            | "update"
            | "doctor"
            | "sandbox"
            | "debug"
            | "apply"
            | "a"
            | "queue"
            | "archive"
            | "delete"
            | "migrate-rollouts"
            | "migrate-nfs-db"
            | "unarchive"
            | "cloud"
            | "exec-server"
            | "features"
            | "help"
            | "agents"
    )
}

// ─── 每会话服务的状态（落盘，Meowo 重启后能重新发现） ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerState {
    /// 最近一次接上它的会话 id（新建会话是临时负 id；投递按 DB 记的 app-server pid 找，不靠它）。
    launch_session: i64,
    server_pid: u32,
    port: u16,
    started_at_ms: i64,
    tui_pid: Option<u32>,
    tui_exited_at_ms: Option<i64>,
    /// 看守线程最后一次确认 TUI 还活着的时刻。Meowo 退出时 TUI 随托管终端结束，但来不及写
    /// tui_exited_at_ms；重启后按它推算 linger 起点，过期的直接回收，而不是再白等一整个 linger。
    #[serde(default)]
    tui_seen_at_ms: Option<i64>,
}

/// 内存里的活服务：持有 Child 句柄（结束时按句柄杀，不怕 pid 复用）。
struct Live {
    child: std::process::Child,
    /// 起服务时托管终端发的 scoped token（hook/MCP 继承在服务环境里）。复用服务时要让它继续有效，
    /// 否则新一代终端换发 token 后，服务派生的 hook 递审批会认证失败。
    pty_token: Option<String>,
}

/// TUI 已退出、只在等投递的服务 pid。看板判活用的进程快照要剔除它们，否则会话卡片在 linger 期间
/// 一直显示「运行中」（hook 把 app-server 记成了会话 pid）。由看守线程维护。
static LINGERING: LazyLock<Mutex<std::collections::HashSet<u32>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// 已有看守线程的服务目录：复用服务时 bind_tui 不再多起一个。
static REAPING: LazyLock<Mutex<std::collections::HashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

pub(crate) fn lingering_server_pids() -> std::collections::HashSet<u32> {
    LINGERING.lock().map(|set| set.clone()).unwrap_or_default()
}

fn set_lingering(pid: u32, lingering: bool) {
    if let Ok(mut set) = LINGERING.lock() {
        if lingering {
            set.insert(pid);
        } else {
            set.remove(&pid);
        }
    }
}

static LIVE: LazyLock<Mutex<HashMap<PathBuf, Live>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn root_of(db: &Path) -> PathBuf {
    db.parent()
        .unwrap_or(Path::new("."))
        .join("codex-app-server")
}

fn servers_dir(db: &Path) -> PathBuf {
    root_of(db).join("servers")
}

fn read_state(dir: &Path) -> Option<ServerState> {
    serde_json::from_slice(&std::fs::read(dir.join("state.json")).ok()?).ok()
}

fn write_state(dir: &Path, state: &ServerState) {
    if let Ok(body) = serde_json::to_string(state) {
        let _ = meowo_agent::fsutil::write_atomic(&dir.join("state.json"), &body);
    }
}

fn read_token(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("token"))
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}

fn now_ms() -> i64 {
    crate::now_ms()
}

fn linger_ms() -> i64 {
    let secs = std::env::var(LINGER_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_LINGER_SECS);
    i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX)
}

/// pid 还活着且确实是我们起的那个 app-server（命令行里带着本会话的 token 文件路径）。
/// 跨 Meowo 重启只剩 pid 可用，靠命令行核身份，防 pid 被无关进程复用后误杀/误连。
fn server_alive(dir: &Path, state: &ServerState) -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pid = sysinfo::Pid::from_u32(state.server_pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new().with_cmd(UpdateKind::Always),
    );
    let Some(process) = sys.process(pid) else {
        return false;
    };
    let token = dir.join("token").to_string_lossy().to_lowercase();
    process
        .cmd()
        .iter()
        .any(|arg| arg.to_string_lossy().to_lowercase() == token)
}

fn pid_alive(pid: u32) -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = sysinfo::Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new(),
    );
    sys.process(pid).is_some()
}

fn states(db: &Path) -> Vec<(PathBuf, ServerState)> {
    let Ok(entries) = std::fs::read_dir(servers_dir(db)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|dir| read_state(&dir).map(|state| (dir, state)))
        .collect()
}

/// 结束服务并删掉它的目录（token 随之消失）。内存里有句柄按句柄杀；否则先核身份再按 pid 杀。
fn retire(dir: &Path) {
    if let Some(state) = read_state(dir) {
        set_lingering(state.server_pid, false);
    }
    let live = LIVE.lock().ok().and_then(|mut map| map.remove(dir));
    if let Some(mut live) = live {
        crate::proc::kill_descendants(live.child.id());
        let _ = live.child.kill();
        let _ = live.child.wait();
    } else if let Some(state) = read_state(dir).filter(|state| server_alive(dir, state)) {
        crate::proc::kill_descendants(state.server_pid);
        crate::proc::kill_pid(state.server_pid);
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .ok()
}

fn spawn_server(dir: &Path, spec: &SpawnSpec) -> Result<std::process::Child, String> {
    let log = std::fs::File::create(dir.join("server.log")).map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(&spec.argv[0]);
    command
        .args(&spec.argv[1..])
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW：后台服务不弹黑框
    }
    command.spawn().map_err(|e| e.to_string())
}

struct SpawnSpec {
    argv: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<PathBuf>,
}

/// 端口上应答的确实是持有这枚 token 的 capability-token 服务：真 token 握手 101，假 token 握手 401。
/// 只做 WebSocket 握手、不发 initialize（initialize 会抢走 originator）。free_port 拿到端口到服务
/// 真正 bind 之间有窗口，别的进程可能先占了这个端口——只认 TCP 连通会把 TUI 指到别人的服务上。
fn verify_identity(port: u16, token: &str) -> bool {
    let timeout = Duration::from_millis(500);
    if ws::Client::connect(port, token, timeout).is_err() {
        return false;
    }
    let decoy = format!("{token}-meowo-decoy");
    matches!(
        ws::Client::connect(port, &decoy, timeout).map(|_| ()),
        Err(error) if error == "lead/codex_server_auth"
    )
}

fn wait_listening(port: u16, token: &str, child: &mut std::process::Child) -> bool {
    for _ in 0..150 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        if verify_identity(port, token) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 服务上是否还有回合在跑（或排着队）。只读：initialize + thread/loaded/list + thread/read +
/// thread/queue/list，不发任何会开回合的请求。只用于 TUI 已经握过手的服务（originator 已定）。
/// 字段出处：0.167.2 `codex app-server generate-ts --experimental`——`Thread.status: ThreadStatus`
/// = notLoaded | idle | systemError | active{activeFlags}；`ThreadQueueListResponse.data`。
fn server_busy(port: u16, token: &str, timeout: Duration) -> Result<bool, String> {
    let mut client = ws::Client::connect(port, token, timeout)?;
    client.call("initialize", meowo_client_info(), timeout)?;
    client.notify("initialized")?;
    let loaded = client.call("thread/loaded/list", json!({}), timeout)?;
    for id in loaded_ids(&loaded) {
        let read = client.call(
            "thread/read",
            json!({ "threadId": id, "includeTurns": false }),
            timeout,
        )?;
        if read.pointer("/thread/status/type").and_then(Value::as_str) == Some("active") {
            return Ok(true);
        }
        let queued = client.call("thread/queue/list", json!({ "threadId": id }), timeout)?;
        if queued
            .get("data")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn loaded_ids(loaded: &Value) -> Vec<String> {
    loaded
        .get("data")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn meowo_client_info() -> Value {
    json!({
        "clientInfo": { "name": "meowo", "title": "Meowo", "version": env!("CARGO_PKG_VERSION") },
        "capabilities": { "experimentalApi": true },
    })
}

/// 启动一次远程模式的句柄：spawn 成功后交回 [`bind_tui`]，失败交回 [`abort`]。
pub(crate) struct Launch {
    dir: PathBuf,
    session_id: i64,
    /// 接上的是已在跑的旧服务（有回合在后台跑），不是这次新起的。
    reused: bool,
    inherited_pty_token: Option<String>,
}

impl Launch {
    /// 复用旧服务时，旧服务环境里那枚 scoped token：调用方须让它对本会话继续有效（pty.rs）。
    pub(crate) fn inherited_pty_token(&self) -> Option<&str> {
        self.inherited_pty_token.as_deref()
    }
}

/// 同一会话已有自有服务时怎么办。
#[derive(Debug, PartialEq, Eq)]
enum Existing {
    /// 服务上有回合在跑：新 TUI 用 --remote 接上它（同端口同 token），绝不杀。
    Reuse,
    /// 确认空闲（或已连不上）：收掉，按新一代参数重起（新 token、新 MCP 环境）。
    Retire,
}

/// busy：Ok(true) 有回合在跑 / Ok(false) 空闲 / Err 查不了。查不了 = 握手都过不去，TUI 同样连不上，
/// 留着也没用；但 originator 核对之类只读失败不该杀回合——那些在 server_busy 里都会返回 Ok。
fn existing_action(busy: &Result<bool, String>) -> Existing {
    match busy {
        Ok(true) => Existing::Reuse,
        Ok(false) | Err(_) => Existing::Retire,
    }
}

/// H1：重开会话前先处理同一会话的旧服务。TUI 退出后服务还在 linger，手机作答可能正让它在后台
/// 跑那一轮（记录已是 accepted，不会重发）；此时无条件收掉会把回合连同答复一起杀掉。
/// 返回可复用的那个（目录、状态、token）；其余空闲的收掉。
fn settle_existing(
    db: &Path,
    session_id: i64,
    alive: impl Fn(&Path, &ServerState) -> bool,
) -> Option<(PathBuf, ServerState, String)> {
    let pid = crate::open_store(db)
        .ok()
        .and_then(|store| store.session_pid(session_id).ok().flatten());
    let mut reuse = None;
    for (dir, state) in states(db) {
        if state.launch_session != session_id && pid != Some(i64::from(state.server_pid)) {
            continue;
        }
        let token = read_token(&dir);
        let busy = match (&token, alive(&dir, &state)) {
            (Some(token), true) => server_busy(state.port, token, PROBE_TIMEOUT),
            _ => Err("lead/codex_server_unreachable".into()),
        };
        match (existing_action(&busy), token) {
            (Existing::Reuse, Some(token)) if reuse.is_none() => {
                reuse = Some((dir, state, token));
            }
            // 第二个忙着的（不该出现）：也不杀，只是不接，等它自己跑完被看守线程收掉。
            (Existing::Reuse, _) => {}
            (Existing::Retire, _) => retire(&dir),
        }
    }
    reuse
}

/// 托管 PTY spawn 前调用（pty.rs start_spawned）：条件满足就起 app-server 并把 `command` 改成
/// `--remote` 形态；任何一步不成都原样返回 None，会话照旧内嵌启动（只是没有自动续跑）。
pub(crate) fn prepare_launch(
    session_id: i64,
    provider: &str,
    env: &[(String, String)],
    command: &mut CommandBuilder,
) -> Option<Launch> {
    use meowo_agent::lead_install::{NativeContinue, ServerProtocol};
    if super::mechanism(provider)
        != Some(NativeContinue::ServerQueue(ServerProtocol::CodexAppServer))
    {
        return None;
    }
    // 只给装了 Lead 的账号改形态：没有关卡就用不到投递通道，不必多一个常驻进程。
    if !crate::lead_runtime::enabled(provider, env) {
        return None;
    }
    let plugin = meowo_agent::by_id(provider)?;
    let argv: Vec<String> = command
        .get_argv()
        .iter()
        .map(|arg| arg.to_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    let split = split_argv(&argv, |name| plugin.owns_process(name))?;
    let db = crate::db_path();
    // 同一会话上一代的服务：空闲就收掉重起（它带着旧 scoped token 与旧 MCP 环境）；
    // 有回合在跑就接上它，不杀（见 settle_existing）。
    if let Some((dir, state, token)) = settle_existing(&db, session_id, server_alive) {
        if verify_identity(state.port, &token) {
            let inherited_pty_token = LIVE
                .lock()
                .ok()
                .and_then(|live| live.get(&dir).and_then(|entry| entry.pty_token.clone()));
            point_tui_at(command, split, state.port, &token);
            return Some(Launch {
                dir,
                session_id,
                reused: true,
                inherited_pty_token,
            });
        }
        // 端口上已不是它（不该出现）：不杀进程（可能仍在跑回合），只是不接，按新一代起。
    }
    let token = crate::remote::generate_token().ok()?;
    let port = free_port()?;
    // 目录名与 token 无关：目录路径会出现在 app-server 的命令行里（--ws-token-file），任何本机进程
    // 都能读到命令行，token 的任何片段都不该跟着出现。
    let id = crate::remote::generate_token().ok()?;
    let dir = servers_dir(&db).join(format!("{}-{}", now_ms(), &id[..12]));
    std::fs::create_dir_all(&dir).ok()?;
    let token_file = dir.join("token");
    if std::fs::write(&token_file, &token).is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    }
    let mut server_argv = split.prefix.clone();
    server_argv.push("app-server".into());
    server_argv.extend(split.server.iter().cloned());
    server_argv.extend([
        "--listen".into(),
        format!("ws://127.0.0.1:{port}"),
        "--ws-auth".into(),
        "capability-token".into(),
        "--ws-token-file".into(),
        token_file.to_string_lossy().into_owned(),
    ]);
    let spec = SpawnSpec {
        argv: server_argv,
        env: command
            .iter_full_env_as_str()
            .filter(|(key, _)| !key.eq_ignore_ascii_case(TOKEN_ENV))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        cwd: command.get_cwd().map(PathBuf::from),
    };
    let pty_token = spec
        .env
        .iter()
        .find(|(key, _)| key == "MEOWO_PTY_TOKEN")
        .map(|(_, value)| value.clone());
    let mut child = match spawn_server(&dir, &spec) {
        Ok(child) => child,
        Err(error) => {
            eprintln!("codex app-server 启动失败，按内嵌模式启动: {error}");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
    };
    if !wait_listening(port, &token, &mut child) {
        eprintln!("codex app-server 未在限期内监听（或端口被别的进程占用），按内嵌模式启动");
        crate::proc::kill_descendants(child.id());
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    }
    write_state(
        &dir,
        &ServerState {
            launch_session: session_id,
            server_pid: child.id(),
            port,
            started_at_ms: now_ms(),
            tui_pid: None,
            tui_exited_at_ms: None,
            tui_seen_at_ms: None,
        },
    );
    if let Ok(mut live) = LIVE.lock() {
        live.insert(dir.clone(), Live { child, pty_token });
    }
    point_tui_at(command, split, port, &token);
    Some(Launch {
        dir,
        session_id,
        reused: false,
        inherited_pty_token: None,
    })
}

fn point_tui_at(command: &mut CommandBuilder, split: RemoteSplit, port: u16, token: &str) {
    let mut tui = split.prefix;
    tui.extend(split.tui);
    tui.extend([
        "--remote".into(),
        format!("ws://127.0.0.1:{port}"),
        "--remote-auth-token-env".into(),
        TOKEN_ENV.into(),
    ]);
    *command.get_argv_mut() = tui.into_iter().map(Into::into).collect();
    command.env(TOKEN_ENV, token);
}

/// TUI 已 spawn：记下它的 pid，并起看守线程——TUI 退出后服务再活 linger 时长，到点收掉。
pub(crate) fn bind_tui(launch: Launch, tui_pid: Option<u32>) {
    if let Some(mut state) = read_state(&launch.dir) {
        state.tui_pid = tui_pid;
        state.tui_exited_at_ms = None;
        state.tui_seen_at_ms = Some(now_ms());
        state.launch_session = launch.session_id;
        write_state(&launch.dir, &state);
        set_lingering(state.server_pid, false);
    }
    spawn_reaper(launch.dir);
}

/// TUI 没起来：新起的服务没有用处，立即收掉；复用的旧服务还在跑回合，原样留给它的看守线程。
pub(crate) fn abort(launch: Launch) {
    if !launch.reused {
        retire(&launch.dir);
    }
}

/// 探针用：模拟「服务已到期被收掉」的冷路径场景。
#[cfg(test)]
fn retire_for_session(db: &Path, session_id: i64) {
    let pid = crate::open_store(db)
        .ok()
        .and_then(|store| store.session_pid(session_id).ok().flatten());
    for (dir, state) in states(db) {
        if state.launch_session == session_id || pid == Some(i64::from(state.server_pid)) {
            retire(&dir);
        }
    }
}

/// TUI 退出（或推算的退出时刻）起算，过了 linger 且服务空闲才收。有回合在跑就再等：
/// 到点硬杀会把正在跑的答复回合一起杀掉，记录却已是 accepted、不会重发。
fn linger_expired(state: &ServerState, now: i64, linger: i64) -> bool {
    let since = state
        .tui_exited_at_ms
        .or(state.tui_seen_at_ms)
        .unwrap_or(state.started_at_ms);
    now.saturating_sub(since) >= linger
}

/// 心跳间隔：TUI 活着时多久落一次 tui_seen_at_ms（只为 Meowo 重启后推算退出时刻，粗一点无妨）。
const HEARTBEAT_MS: i64 = 60_000;

fn spawn_reaper(dir: PathBuf) {
    if !REAPING.lock().is_ok_and(|mut set| set.insert(dir.clone())) {
        return;
    }
    std::thread::spawn(move || {
        reap_loop(&dir);
        if let Ok(mut set) = REAPING.lock() {
            set.remove(&dir);
        }
    });
}

fn reap_loop(dir: &Path) {
    loop {
        std::thread::sleep(Duration::from_secs(3));
        let Some(mut state) = read_state(dir) else {
            return; // 已被收掉
        };
        if !server_alive(dir, &state) {
            retire(dir);
            return;
        }
        let tui_alive = state.tui_pid.is_some_and(pid_alive);
        set_lingering(state.server_pid, !tui_alive);
        if tui_alive {
            if now_ms().saturating_sub(state.tui_seen_at_ms.unwrap_or(0)) >= HEARTBEAT_MS {
                state.tui_seen_at_ms = Some(now_ms());
                write_state(dir, &state);
            }
            continue;
        }
        if state.tui_exited_at_ms.is_none() {
            state.tui_exited_at_ms = Some(now_ms());
            write_state(dir, &state);
            continue;
        }
        if linger_expired(&state, now_ms(), linger_ms()) && !busy_now(dir, &state) {
            retire(dir);
            return;
        }
    }
}

/// 只有确认在跑才算忙；查不了（连不上）按空闲处理——那样的服务连 TUI 也接不上，留着没用。
fn busy_now(dir: &Path, state: &ServerState) -> bool {
    read_token(dir)
        .is_some_and(|token| server_busy(state.port, &token, PROBE_TIMEOUT).unwrap_or(false))
}

/// 应用启动时调用：上次运行留下的服务重新纳入看守，已死的清目录。
/// Meowo 退出时 TUI 随托管终端结束，但 app-server 不在 ConPTY 里，会活下来——正好用于离线投递；
/// 代价是 Meowo 不运行期间没人回收它（0.167.2 `codex app-server --help` 没有空闲自退参数）。
/// 所以启动时按 TUI 最后在世的时刻补算 linger：已过期且空闲的当场收掉，不再白等一整个 linger。
pub(crate) fn sweep_on_startup() {
    let db = crate::db_path();
    for (dir, mut state) in states(&db) {
        if !server_alive(&dir, &state) {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        let tui_alive = state.tui_pid.is_some_and(pid_alive);
        if !tui_alive && state.tui_exited_at_ms.is_none() {
            // 上次运行没来得及记退出时刻：按最后一次心跳算，没有心跳就按服务启动时刻算。
            state.tui_exited_at_ms = Some(state.tui_seen_at_ms.unwrap_or(state.started_at_ms));
            write_state(&dir, &state);
        }
        if !tui_alive && linger_expired(&state, now_ms(), linger_ms()) && !busy_now(&dir, &state) {
            retire(&dir);
            continue;
        }
        spawn_reaper(dir);
    }
}

/// 该 pid 若是 Meowo 为某个会话起的 app-server，返回它的 TUI 是否还活着；不是则 None。
/// 「会话进程活着」的判定（看板、恢复守卫、离线推送）要看 TUI：TUI 一退，服务只是在等投递，
/// 不是「外部终端里的会话」；恢复会话时空闲就被新一代顶替，忙就被新 TUI 接上（settle_existing）。
pub(crate) fn owned_server_tui_alive(pid: i64) -> Option<bool> {
    let db = crate::db_path();
    states(&db)
        .into_iter()
        .find(|(dir, state)| i64::from(state.server_pid) == pid && server_alive(dir, state))
        .map(|(_, state)| state.tui_pid.is_some_and(pid_alive))
}

/// /clear 换代时认领方是 app-server（hook 的父链在它下面），不在 TUI 的进程树里；
/// 它若正是这个 TUI 的服务端，就等同于树内。
pub(crate) fn serves_tui(server_pid: u32, tui_pid: u32) -> bool {
    states(&crate::db_path())
        .into_iter()
        .any(|(_, state)| state.server_pid == server_pid && state.tui_pid == Some(tui_pid))
}

// ─── 投递 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DeliveryRecord {
    /// sending = 正在送（发送前先落的占位）；accepted = app-server 已排进 thread；
    /// queued = 已落进 codex 的持久队列，下次恢复会话时消费；failed = 确定没送出（可自动重试）；
    /// uncertain = 请求已发出但没拿到确认，可能已送达（绝不自动重投，面板给手动重试）。
    state: String,
    reason: Option<String>,
    at_ms: i64,
    submission: Option<String>,
    /// 写 sending 占位的进程。Meowo 中途退出留下的 sending 由下一个进程按 uncertain 处理。
    #[serde(default)]
    pid: Option<u32>,
}

/// 发送失败分两类：请求还没发出（可以放心自动重试）和已经发出（可能已送达）。
#[derive(Debug, PartialEq, Eq)]
enum SendError {
    NotSent(String),
    MaybeSent(String),
}

/// 可能已送达、但没拿到确认（超时、断开、冷路径进程卡死）。
const UNCERTAIN: &str = "lead/codex_delivery_uncertain";

fn delivery_key(task: &Path, gate_id: &str, native: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [
        task.to_string_lossy().as_bytes(),
        gate_id.as_bytes(),
        native.as_bytes(),
    ] {
        hash.update(part);
        hash.update([0]);
    }
    format!("lead-codex-{:x}", hash.finalize())
}

fn record_path(db: &Path, key: &str) -> PathBuf {
    root_of(db).join("deliveries").join(format!("{key}.json"))
}

fn read_record(db: &Path, key: &str) -> Option<DeliveryRecord> {
    serde_json::from_slice(&std::fs::read(record_path(db, key)).ok()?).ok()
}

fn write_record(db: &Path, key: &str, record: &DeliveryRecord) {
    let path = record_path(db, key);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(body) = serde_json::to_string(record) {
        let _ = meowo_agent::fsutil::write_atomic(&path, &body);
    }
}

/// 按投递键串行化：手机作答（answer_and_deliver）与关卡巡检（reconcile_task，每 10s）会并发调用
/// deliver；读记录→发送→写记录之间不加锁，两边都读到「未送」就会 queue/add 两次（真开两轮）。
/// 同 opencode_http.rs 的 LOCK，但按键分开：冷路径要起 codex 进程，别让不相干的关卡排队等。
static INFLIGHT: LazyLock<Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key_lock(key: &str) -> std::sync::Arc<Mutex<()>> {
    INFLIGHT
        .lock()
        .map(|mut map| map.entry(key.to_owned()).or_default().clone())
        .unwrap_or_default()
}

/// 送出一次答复。投递失败不报错给作答方（答复已保存）：记成 failed/uncertain，面板显示原因与「继续送达」。
pub(crate) fn deliver(target: &NativeTarget<'_>) -> Result<NativeOutcome, String> {
    deliver_with(target, send)
}

fn deliver_with(
    target: &NativeTarget<'_>,
    send: impl FnOnce(&NativeTarget<'_>, &str) -> Result<(&'static str, Option<String>), SendError>,
) -> Result<NativeOutcome, String> {
    let key = delivery_key(target.task, target.gate_id, target.native_session);
    let lock = key_lock(&key);
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(record) = read_record(target.db, &key) {
        match record.state.as_str() {
            // 已送出的绝不重发：queue 会真的再开一轮。
            "accepted" | "queued" => return Ok(NativeOutcome::Delivered),
            // 可能已送达：只等用户在面板上手动重试（clear_failure 清掉记录后才会再走到这里）。
            // 持锁时看到 sending = 上一个进程送到一半退出了，同样当作「可能已送达」。
            "uncertain" | "sending" => return Ok(NativeOutcome::Delivered),
            _ if now_ms().saturating_sub(record.at_ms) < RETRY_COOLDOWN_MS => {
                return Ok(NativeOutcome::Delivered)
            }
            _ => {}
        }
    }
    // 先落占位再发：发送途中 Meowo 被杀，重启后看到的是 sending 而不是「没记录」，不会再送一次。
    write_record(
        target.db,
        &key,
        &DeliveryRecord {
            state: "sending".into(),
            reason: None,
            at_ms: now_ms(),
            submission: None,
            pid: Some(std::process::id()),
        },
    );
    let (state, reason, submission) = match send(target, &key) {
        Ok((state, submission)) => (state, None, submission),
        Err(SendError::NotSent(reason)) => ("failed", Some(reason), None),
        Err(SendError::MaybeSent(reason)) => ("uncertain", Some(reason), None),
    };
    write_record(
        target.db,
        &key,
        &DeliveryRecord {
            state: state.into(),
            reason,
            at_ms: now_ms(),
            submission,
            pid: None,
        },
    );
    Ok(NativeOutcome::Delivered)
}

fn send(target: &NativeTarget<'_>, key: &str) -> Result<(&'static str, Option<String>), SendError> {
    let store = crate::open_store(target.db).map_err(SendError::NotSent)?;
    let pid = store.session_pid(target.session_id).ok().flatten();
    let live = states(target.db).into_iter().find(|(dir, state)| {
        (pid == Some(i64::from(state.server_pid)) || state.launch_session == target.session_id)
            && server_alive(dir, state)
    });
    let mut thread = target.native_session.to_owned();
    if let Some((dir, state)) = live {
        // 刚起的服务给 TUI 留出先握手的时间（originator 由第一个 initialize 的客户端决定）。
        let age = now_ms().saturating_sub(state.started_at_ms);
        if age < TUI_HANDSHAKE_GRACE_MS {
            std::thread::sleep(Duration::from_millis((TUI_HANDSHAKE_GRACE_MS - age) as u64));
        }
        let token = read_token(&dir)
            .ok_or_else(|| SendError::NotSent("lead/codex_server_unreachable".into()))?;
        match queue_live(state.port, &token, target, key, RPC_TIMEOUT)? {
            LiveOutcome::Queued(submission) => return Ok(("accepted", submission)),
            // thread 不在这个服务里（TUI 里 /new 过、或服务刚重启还没 resume）：落持久队列。
            LiveOutcome::NotLoaded { main } => thread = main,
            // M4：宿主成了第一个握手的客户端（这个进程的请求都会记在 meowo 名下）。不投、也不重启
            // 服务——重启会把 TUI 正在跑的回合一起杀掉，且 Meowo 重启后 LIVE 为空根本重启不了，
            // 记录会永远停在这里。改走冷路径：只落 codex 的持久队列，由 TUI 下次恢复时消费。
            LiveOutcome::OriginatorMismatch { main } => thread = main,
        }
    } else if let Some(main) = cold_main_thread(target) {
        thread = main;
    }
    queue_cold(target, &store, &thread).map(|submission| ("queued", submission))
}

#[derive(Debug, PartialEq, Eq)]
enum LiveOutcome {
    Queued(Option<String>),
    /// 主线程没加载在这个服务上；main 是解析出的主线程 id（给冷路径用）。
    NotLoaded {
        main: String,
    },
    OriginatorMismatch {
        main: String,
    },
}

/// 经活着的 app-server 排队。
fn queue_live(
    port: u16,
    token: &str,
    target: &NativeTarget<'_>,
    key: &str,
    timeout: Duration,
) -> Result<LiveOutcome, SendError> {
    let not_sent = SendError::NotSent;
    let mut client = ws::Client::connect(port, token, timeout).map_err(not_sent)?;
    let init = client
        .call("initialize", meowo_client_info(), timeout)
        .map_err(not_sent)?;
    client.notify("initialized").map_err(not_sent)?;
    let main = main_thread(&mut client, target.native_session, timeout);
    if !originator_is_tui(init.get("userAgent").and_then(Value::as_str)) {
        return Ok(LiveOutcome::OriginatorMismatch { main });
    }
    let loaded = client
        .call("thread/loaded/list", json!({}), timeout)
        .map_err(not_sent)?;
    if !loaded_ids(&loaded).contains(&main) {
        return Ok(LiveOutcome::NotLoaded { main });
    }
    // 从这里起请求已写出：超时/断开都可能是「服务已收下、只是回执没到」，一律当可能已送达。
    let result = client
        .call(
            "thread/queue/add",
            queue_add_params(&main, target.message, key),
            timeout,
        )
        .map_err(|reason| match reason.as_str() {
            // 服务明确回了 error：没收下。
            "lead/codex_server_rejected" => SendError::NotSent(reason),
            _ => SendError::MaybeSent(UNCERTAIN.into()),
        })?;
    Ok(LiveOutcome::Queued(
        result
            .pointer("/queuedSubmission/id")
            .and_then(Value::as_str)
            .map(str::to_owned),
    ))
}

/// H3：.owner 记的可能是子代理的 thread（子代理里的 PostToolUse 也会认领任务）。答复排进子代理
/// 线程，主 agent 永远看不到；所以沿父链找到主线程再投。
/// 字段出处：0.167.2 `generate-ts --experimental` 的 Thread.ts——`parentThreadId`「This will only be
/// set if this thread is a subagent」；兜底看 `source.subAgent.thread_spawn.parent_thread_id`
/// （SessionSource.ts / SubAgentSource.ts）。读不到就按原 id 投（等同改动前的行为）。
fn main_thread(client: &mut ws::Client, thread: &str, timeout: Duration) -> String {
    let mut current = thread.to_owned();
    let mut seen = vec![current.clone()];
    for _ in 0..8 {
        let Ok(read) = client.call(
            "thread/read",
            json!({ "threadId": current, "includeTurns": false }),
            timeout,
        ) else {
            break;
        };
        let Some(parent) = read.get("thread").and_then(parent_of) else {
            break;
        };
        if seen.contains(&parent) {
            break;
        }
        seen.push(parent.clone());
        current = parent;
    }
    current
}

fn parent_of(thread: &Value) -> Option<String> {
    thread
        .get("parentThreadId")
        .and_then(Value::as_str)
        .or_else(|| {
            thread
                .pointer("/source/subAgent/thread_spawn/parent_thread_id")
                .and_then(Value::as_str)
        })
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// 冷路径没有服务可问：读该 thread rollout 首行的 session_meta，看 source 是不是子代理。
/// rollout 用 codex core 的序列化（`source: {"subagent": {"thread_spawn": {"parent_thread_id": …}}}`），
/// 与 app-server v2 的驼峰写法不同，两种都认。读不到就按原 id（与改动前一致）。
fn rollout_parent(first_line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(first_line).ok()?;
    if value.get("type")?.as_str()? != "session_meta" {
        return None;
    }
    let source = value.pointer("/payload/source")?;
    ["subagent", "subAgent", "sub_agent"]
        .iter()
        .find_map(|key| source.get(*key))
        .and_then(|sub| sub.pointer("/thread_spawn/parent_thread_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn cold_main_thread(target: &NativeTarget<'_>) -> Option<String> {
    use std::io::BufRead;
    let spec = meowo_agent::by_id(target.provider)?
        .telemetry()?
        .transcript()?;
    let mut current = target.native_session.to_owned();
    let mut found = false;
    for _ in 0..8 {
        let Some(path) = spec.resolve_transcript_path(None, target.cwd, &current) else {
            break;
        };
        let mut line = String::new();
        let file = std::fs::File::open(path).ok()?;
        std::io::BufReader::new(file).read_line(&mut line).ok()?;
        let Some(parent) = rollout_parent(&line).filter(|parent| *parent != current) else {
            break;
        };
        current = parent;
        found = true;
    }
    found.then_some(current)
}

/// 只用 thread/queue/add：turn/start 等于插话（打断或并入当前回合），queue 才是「排到这一轮之后」，
/// 且不碰 TUI 输入框里的草稿。clientUserMessageId 用投递键，便于在 codex 侧对账。
/// 审批守卫：queue/add 是 client→server 的新请求，只往 thread 的排队区加一条用户消息；待批的审批是
/// server→client 请求（ServerRequest.ts：item/commandExecution/requestApproval 等），只能由收到它的
/// 客户端回同 id 响应作答，queue/add 在结构上答不了它。research C2 实测「审批框不受影响」。
fn queue_add_params(thread: &str, message: &str, key: &str) -> Value {
    json!({
        "threadId": thread,
        "clientUserMessageId": key,
        "input": [{ "type": "text", "text": message }],
    })
}

fn originator_is_tui(user_agent: Option<&str>) -> bool {
    user_agent.is_some_and(|ua| ua.starts_with("codex-tui/"))
}

/// 冷路径进程的时限：`codex queue` 只写本地 sqlite，实测秒级；卡住时不能一直占着投递锁。
const COLD_QUEUE_TIMEOUT: Duration = Duration::from_secs(60);

/// 冷路径：`codex queue --thread <id> --message <答复>` 只写 codex 的持久队列、不调模型；
/// 下次在 Meowo 里恢复这个会话时由 TUI 消费（隔离实测：resume 后答复被执行，originator=codex-tui）。
fn queue_cold(
    target: &NativeTarget<'_>,
    store: &meowo_store::Store,
    thread: &str,
) -> Result<Option<String>, SendError> {
    let plugin = meowo_agent::by_id(target.provider)
        .ok_or_else(|| SendError::NotSent("lead/provider_missing".into()))?;
    let profile = store.session_profile(target.session_id).ok().flatten();
    let env = crate::terminal::launch_env_for_exact_profile(plugin, profile.as_deref());
    let argv = plugin.launch_argv();
    let mut command = std::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .args(cold_queue_args(thread, target.message))
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = target.cwd.filter(|cwd| Path::new(cwd).is_dir()) {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let failed = || SendError::NotSent("lead/codex_queue_failed".into());
    let mut child = command.spawn().map_err(|_| failed())?;
    let (status, stdout) = wait_output(&mut child, COLD_QUEUE_TIMEOUT)
        // 进程已起、可能已写进队列：超时按可能已送达处理。
        .ok_or_else(|| SendError::MaybeSent(UNCERTAIN.into()))?;
    if !status.success() {
        return Err(failed());
    }
    if !stdout.contains("Queued message") {
        // 退出码成功但没打印回执：可能已入队，不能当没送。
        return Err(SendError::MaybeSent(UNCERTAIN.into()));
    }
    Ok(stdout
        .split_whitespace()
        .skip_while(|word| *word != "message")
        .nth(1)
        .map(str::to_owned))
}

/// 等子进程结束并收 stdout；超时杀掉返回 None。stdout 在线程里读，免得管道写满把子进程卡住。
fn wait_output(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<(std::process::ExitStatus, String)> {
    use std::io::Read;
    let mut pipe = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = pipe.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                crate::proc::kill_descendants(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    Some((status, reader.join().unwrap_or_default()))
}

fn cold_queue_args(thread: &str, message: &str) -> Vec<String> {
    // 不带 --resume：那会在本机共享 daemon 上直接跑这一轮，不经 TUI；留给用户恢复会话时由 TUI 执行。
    vec![
        "queue".into(),
        "--thread".into(),
        thread.into(),
        "--message".into(),
        message.into(),
    ]
}

/// 面板上的送达状态。没有记录且关卡已作答 = 还没送（或保存后进程中断），给「继续送达」。
pub(crate) fn status(
    db: &Path,
    session_id: i64,
    cwd: String,
    task: &Path,
    gate_id: &str,
    native: &str,
) -> LeadDeliveryDto {
    let key = delivery_key(task, gate_id, native);
    let (mut state, mut reason) = match read_record(db, &key) {
        Some(record) => (record.state, record.reason),
        None => ("saved".to_owned(), None),
    };
    // sending 只有本进程正持锁发送时才是真的「正在送」；否则是上次送到一半中断留下的，可能已送达。
    if state == "sending" && key_lock(&key).try_lock().is_ok() {
        state = "uncertain".into();
        reason = Some(UNCERTAIN.into());
    }
    LeadDeliveryDto {
        session_id,
        cwd,
        can_retry: matches!(state.as_str(), "failed" | "saved" | "uncertain"),
        delivery_id: key,
        state,
        reason,
    }
}

/// 面板「继续送达」：清掉失败/不确定记录，紧接着的 deliver 立即重投。已送出的不受影响。
/// 持投递锁：正在发送的那次结束前不动记录，结束后若已 accepted 也就不清了。
pub(crate) fn clear_failure(db: &Path, task: &Path, gate_id: &str, native: &str) {
    let key = delivery_key(task, gate_id, native);
    let lock = key_lock(&key);
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if read_record(db, &key)
        .is_some_and(|record| matches!(record.state.as_str(), "failed" | "uncertain" | "sending"))
    {
        let _ = std::fs::remove_file(record_path(db, &key));
    }
}

#[cfg(test)]
#[path = "codex_app_server_probe.rs"]
mod probe;

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }
    fn owns(name: &str) -> bool {
        matches!(name, "codex" | "codex.exe")
    }

    /// 迁移清单：Meowo 现有的每一类 codex 启动注入都必须落到正确的一侧。
    /// browser_agent / lead_runtime 的 MCP `-c`、relay 的 model_provider `-c` → app-server；
    /// 启动选项（审批/沙箱，仅新建；恢复时丢弃）、resume/fork 子命令与会话 id → TUI。
    #[test]
    fn meowo_launch_injections_split_between_server_and_tui() {
        let argv = s(&[
            r"C:\x\codex.exe",
            "--ask-for-approval",
            "never",
            "--sandbox",
            "workspace-write",
            "-c",
            "model_provider=\"meowo-relay\"",
            "resume",
            "019a-thread",
            "-c",
            "mcp_servers.meowo.command='r.exe'",
            "-c",
            "mcp_servers.meowo_lead.env={ MEOWO_PTY_TOKEN = \"t\" }",
            "--enable",
            "x",
            "--config=foo=1",
        ]);
        let split = split_argv(&argv, owns).unwrap();
        assert_eq!(split.prefix, s(&[r"C:\x\codex.exe"]));
        assert_eq!(
            split.server,
            s(&[
                "-c",
                "model_provider=\"meowo-relay\"",
                "-c",
                "mcp_servers.meowo.command='r.exe'",
                "-c",
                "mcp_servers.meowo_lead.env={ MEOWO_PTY_TOKEN = \"t\" }",
                "--enable",
                "x",
                "-c",
                "foo=1",
            ])
        );
        // 恢复时权限参数丢弃（远程 resume 不接受，thread 沿用自己存下的权限）。
        assert_eq!(split.tui, s(&["resume", "019a-thread"]));
        let fresh = split_argv(
            &s(&[
                "codex",
                "-m",
                "x",
                "--dangerously-bypass-approvals-and-sandbox",
                "-s",
                "read-only",
            ]),
            owns,
        )
        .unwrap();
        assert_eq!(
            fresh.tui,
            s(&[
                "--dangerously-bypass-approvals-and-sandbox",
                "-s",
                "read-only",
                "-m",
                "x"
            ])
        );
    }

    #[test]
    fn node_wrapper_prefix_and_fork_are_kept() {
        let argv = s(&[
            "node",
            "/npm/@openai/codex/bin/codex.js",
            "fork",
            "id",
            "-m",
            "resume",
        ]);
        let split = split_argv(&argv, owns).unwrap();
        assert_eq!(
            split.prefix,
            s(&["node", "/npm/@openai/codex/bin/codex.js"])
        );
        assert_eq!(split.tui, s(&["fork", "id", "-m", "resume"]));
        assert!(split.server.is_empty());
    }

    /// app-server 不认 -p/--search；用户自己指定了 --remote/--no-daemon；非交互子命令——都退回内嵌。
    #[test]
    fn unsupported_shapes_fall_back_to_embedded() {
        for argv in [
            s(&["codex", "-p", "work"]),
            s(&["codex", "--profile=work"]),
            s(&["codex", "--search"]),
            s(&["codex", "--no-daemon", "resume", "x"]),
            s(&["codex", "--remote", "ws://h:1"]),
            s(&["codex", "exec", "hi"]),
            s(&["codex", "-c"]),
            s(&["unrelated.exe", "-c", "a=1"]),
        ] {
            assert_eq!(split_argv(&argv, owns), None, "{argv:?}");
        }
    }

    /// 回归：投递只用 queue（排队），不用 turn/start（插话）；目标 thread 与幂等键都来自参数。
    #[test]
    fn delivery_uses_queue_add_not_turn_start() {
        let params = queue_add_params("019a-thread", "answer", "k");
        assert_eq!(params["threadId"], "019a-thread");
        assert_eq!(params["clientUserMessageId"], "k");
        assert_eq!(params["input"][0]["text"], "answer");
        let source = include_str!("codex_app_server.rs");
        // 源码里所有宿主发出的 RPC 方法名（rustfmt 可能把参数拆行，故按空白宽松匹配）。
        let call = regex::Regex::new(r#"\.call\(\s*"([^"]+)""#).unwrap();
        let mut rpc_calls: Vec<&str> = call
            .captures_iter(source)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        rpc_calls.sort_unstable();
        rpc_calls.dedup();
        // 除 queue/add 外全是只读请求；绝不出现 turn/*（插话/打断）。
        assert_eq!(
            rpc_calls,
            [
                "initialize",
                "thread/loaded/list",
                "thread/queue/add",
                "thread/queue/list",
                "thread/read"
            ]
        );
        // 冷路径同样只排队：不带 --resume（那会在共享 daemon 上不经 TUI 直接跑）。
        assert!(!cold_queue_args("t", "m").iter().any(|a| a == "--resume"));
    }

    #[test]
    fn originator_check_only_accepts_tui_first_handshake() {
        // 实测原文（codex 0.167.2）：TUI 先握手 / 宿主先握手。
        assert!(originator_is_tui(Some(
            "codex-tui/0.167.2 (Windows 10.0.26200; x86_64) xterm-256color (meowo; 0.0.1)"
        )));
        assert!(!originator_is_tui(Some(
            "meowo/0.167.2 (Windows 10.0.26200; x86_64) xterm-256color (meowo; 0.0.1)"
        )));
        assert!(!originator_is_tui(None));
    }

    #[test]
    fn status_reflects_records_and_never_resends_success() {
        let root = std::env::temp_dir().join(format!(
            "meowo-codex-as-{}",
            crate::remote::generate_token().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let db = root.join("board.db");
        let task = root.join("task");
        let st = status(&db, 7, "c".into(), &task, "Q", "n");
        assert_eq!((st.state.as_str(), st.can_retry), ("saved", true));
        let key = delivery_key(&task, "Q", "n");
        write_record(
            &db,
            &key,
            &DeliveryRecord {
                state: "failed".into(),
                reason: Some("lead/codex_queue_failed".into()),
                at_ms: 0,
                submission: None,
                pid: None,
            },
        );
        let st = status(&db, 7, "c".into(), &task, "Q", "n");
        assert_eq!(
            (st.state.as_str(), st.can_retry, st.reason.as_deref()),
            ("failed", true, Some("lead/codex_queue_failed"))
        );
        clear_failure(&db, &task, "Q", "n");
        assert!(read_record(&db, &key).is_none());
        write_record(
            &db,
            &key,
            &DeliveryRecord {
                state: "accepted".into(),
                reason: None,
                at_ms: 0,
                submission: Some("s".into()),
                pid: None,
            },
        );
        let target = NativeTarget {
            db: &db,
            session_id: 7,
            provider: "codex",
            native_session: "n",
            cwd: None,
            task: &task,
            gate_id: "Q",
            message: "m",
            force: false,
        };
        // 已 accepted：deliver 直接返回，不连服务、不改记录。
        assert_eq!(deliver(&target), Ok(NativeOutcome::Delivered));
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ─── 假 app-server（WebSocket + JSON-RPC），不起 codex、不调模型 ─────────────

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Fake {
        port: u16,
        token: String,
        /// 客户端发来的每条消息（请求、通知、以及若有的响应）。
        seen: Arc<Mutex<Vec<Value>>>,
    }

    impl Fake {
        fn methods(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter_map(|m| m.get("method").and_then(Value::as_str).map(str::to_owned))
                .collect()
        }
        fn params_of(&self, method: &str) -> Vec<Value> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m["method"] == method)
                .map(|m| m["params"].clone())
                .collect()
        }
    }

    fn server_frame(text: &str) -> Vec<u8> {
        let bytes = text.as_bytes();
        let mut frame = vec![0x81];
        if bytes.len() < 126 {
            frame.push(bytes.len() as u8);
        } else {
            frame.push(126);
            frame.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(bytes);
        frame
    }

    fn reply(request: &Value, result: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": request["id"], "result": result })
    }

    fn rpc_error(request: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": request["id"], "error": { "code": -32600, "message": "no" } })
    }

    /// handler 收到每个带 id 的请求，返回要回给客户端的消息（可为空 = 不回，模拟超时）。
    fn fake_server(handler: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static) -> Fake {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = crate::remote::generate_token().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler = Arc::new(handler);
        let (expected, log) = (token.clone(), seen.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (expected, log, handler) = (expected.clone(), log.clone(), handler.clone());
                std::thread::spawn(move || {
                    let mut writer = stream.try_clone().unwrap();
                    let mut reader = std::io::BufReader::new(stream);
                    let mut authorized = false;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        if line == "\r\n" {
                            break;
                        }
                        authorized |=
                            line.trim_end() == format!("Authorization: Bearer {expected}");
                    }
                    if !authorized {
                        let _ = writer.write_all(b"HTTP/1.1 401 Unauthorized\r\n\r\n");
                        return;
                    }
                    let _ = writer.write_all(
                        b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                    );
                    while let Ok((_, opcode, payload)) = ws::read_frame(&mut reader) {
                        if opcode == 0x8 {
                            return;
                        }
                        let Ok(message) = serde_json::from_slice::<Value>(&payload) else {
                            continue;
                        };
                        log.lock().unwrap().push(message.clone());
                        if message.get("id").is_some() && message.get("method").is_some() {
                            for out in handler(&message) {
                                let _ = writer.write_all(&server_frame(&out.to_string()));
                            }
                        }
                    }
                });
            }
        });
        Fake { port, token, seen }
    }

    const TUI_UA: &str =
        "codex-tui/0.167.2 (Windows 10.0.26200; x86_64) xterm-256color (meowo; 0.0.1)";

    type FakeThread = (&'static str, Option<&'static str>, &'static str);

    /// 一台「TUI 先握手过」的假服务：threads = (id, 父 id, 状态)，queue/add 回 qs-1。
    fn codex_like(threads: Vec<FakeThread>, queued: usize) -> Fake {
        fake_server(move |request| {
            let method = request["method"].as_str().unwrap_or_default();
            let params = &request["params"];
            match method {
                "initialize" => vec![reply(request, json!({ "userAgent": TUI_UA }))],
                "thread/loaded/list" => {
                    let ids: Vec<&str> = threads.iter().map(|t| t.0).collect();
                    vec![reply(request, json!({ "data": ids, "nextCursor": null }))]
                }
                "thread/read" => match threads.iter().find(|t| params["threadId"] == t.0) {
                    Some(thread) => vec![reply(
                        request,
                        json!({ "thread": { "id": thread.0, "parentThreadId": thread.1,
                                            "status": { "type": thread.2, "activeFlags": [] } } }),
                    )],
                    None => vec![rpc_error(request)],
                },
                "thread/queue/list" => vec![reply(
                    request,
                    json!({ "data": vec![json!({ "id": "q" }); queued], "nextCursor": null }),
                )],
                "thread/queue/add" => {
                    vec![reply(
                        request,
                        json!({ "queuedSubmission": { "id": "qs-1" } }),
                    )]
                }
                _ => vec![],
            }
        })
    }

    fn target<'a>(
        db: &'a Path,
        task: &'a Path,
        native: &'a str,
        gate: &'a str,
    ) -> NativeTarget<'a> {
        NativeTarget {
            db,
            session_id: 7,
            provider: "codex",
            native_session: native,
            cwd: None,
            task,
            gate_id: gate,
            message: "answer",
            force: false,
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "meowo-codex-{tag}-{}",
            &crate::remote::generate_token().unwrap()[..12]
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    const FAST: Duration = Duration::from_millis(800);

    fn live(fake: &Fake, native: &str) -> Result<LiveOutcome, SendError> {
        let (db, task) = (PathBuf::from("db"), PathBuf::from("task"));
        queue_live(
            fake.port,
            &fake.token,
            &target(&db, &task, native, "Q"),
            "k",
            FAST,
        )
    }

    /// H3：.owner 是子代理 thread 时，答复改投父链顶端的主线程（Thread.parentThreadId）。
    #[test]
    fn queue_live_redirects_subagent_thread_to_main() {
        let fake = codex_like(
            vec![
                ("grandchild", Some("sub"), "active"),
                ("sub", Some("main"), "active"),
                ("main", None, "active"),
            ],
            0,
        );
        assert_eq!(
            live(&fake, "grandchild"),
            Ok(LiveOutcome::Queued(Some("qs-1".into())))
        );
        let added = fake.params_of("thread/queue/add");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0]["threadId"], "main");
        assert_eq!(added[0]["clientUserMessageId"], "k");
        // 主线程本身不受影响。
        let fake = codex_like(vec![("main", None, "idle")], 0);
        assert_eq!(
            live(&fake, "main"),
            Ok(LiveOutcome::Queued(Some("qs-1".into())))
        );
        assert_eq!(fake.params_of("thread/queue/add")[0]["threadId"], "main");
        // 主线程没加载在服务上：交给冷路径，且带上解析出的主线程 id（不是子代理 id）。
        let fake = fake_server(
            |request| match request["method"].as_str().unwrap_or_default() {
                "initialize" => vec![reply(request, json!({ "userAgent": TUI_UA }))],
                "thread/read" if request["params"]["threadId"] == "sub" => vec![reply(
                    request,
                    json!({ "thread": { "id": "sub", "parentThreadId": "main" } }),
                )],
                "thread/read" => vec![reply(request, json!({ "thread": { "id": "main" } }))],
                "thread/loaded/list" => vec![reply(request, json!({ "data": ["sub"] }))],
                _ => vec![reply(request, json!({}))],
            },
        );
        assert_eq!(
            live(&fake, "sub"),
            Ok(LiveOutcome::NotLoaded {
                main: "main".into()
            })
        );
        assert!(fake.params_of("thread/queue/add").is_empty());
    }

    #[test]
    fn subagent_parent_fields_from_protocol_and_rollout() {
        // app-server v2（Thread.ts / SessionSource.ts / SubAgentSource.ts）。
        assert_eq!(
            parent_of(&json!({ "parentThreadId": "p" })).as_deref(),
            Some("p")
        );
        let spawned = json!({ "parentThreadId": null,
            "source": { "subAgent": { "thread_spawn": { "parent_thread_id": "p2", "depth": 1 } } } });
        assert_eq!(parent_of(&spawned).as_deref(), Some("p2"));
        assert_eq!(
            parent_of(&json!({ "parentThreadId": null, "source": "cli" })),
            None
        );
        // rollout 首行（codex core 序列化）。
        let line = r#"{"type":"session_meta","payload":{"id":"s","source":{"subagent":{"thread_spawn":{"parent_thread_id":"m","depth":1}}}}}"#;
        assert_eq!(rollout_parent(line).as_deref(), Some("m"));
        assert_eq!(
            rollout_parent(r#"{"type":"session_meta","payload":{"id":"m","source":"cli"}}"#),
            None
        );
        assert_eq!(
            rollout_parent(r#"{"type":"turn_context","payload":{}}"#),
            None
        );
    }

    /// H3 审批守卫：有待批审批（服务端发来 requestApproval 请求）时，宿主只排队，绝不回响应替用户答审批，
    /// 也不发任何 turn/*。
    #[test]
    fn pending_approval_is_never_answered_by_delivery() {
        let fake = fake_server(
            |request| match request["method"].as_str().unwrap_or_default() {
                "initialize" => vec![
                    // 握手刚完成服务就推来一个审批请求（与 TUI 同连一台服务时可能发给各客户端）。
                    json!({ "jsonrpc": "2.0", "id": 900, "method": "item/commandExecution/requestApproval",
                        "params": { "threadId": "main", "turnId": "t", "itemId": "i" } }),
                    reply(request, json!({ "userAgent": TUI_UA })),
                ],
                "thread/read" => vec![reply(
                    request,
                    json!({ "thread": { "id": "main", "parentThreadId": null,
                                    "status": { "type": "active", "activeFlags": ["waitingOnApproval"] } } }),
                )],
                "thread/loaded/list" => vec![reply(request, json!({ "data": ["main"] }))],
                "thread/queue/add" => vec![
                    json!({ "jsonrpc": "2.0", "id": 901, "method": "item/fileChange/requestApproval",
                        "params": { "threadId": "main" } }),
                    reply(request, json!({ "queuedSubmission": { "id": "qs-9" } })),
                ],
                _ => vec![],
            },
        );
        assert_eq!(
            live(&fake, "main"),
            Ok(LiveOutcome::Queued(Some("qs-9".into())))
        );
        std::thread::sleep(Duration::from_millis(200));
        let seen = fake.seen.lock().unwrap().clone();
        assert!(
            seen.iter()
                .all(|m| m.get("result").is_none() && m.get("error").is_none()),
            "宿主回了服务端请求（等于替用户答审批）：{seen:?}"
        );
        assert!(
            fake.methods().iter().all(|m| !m.starts_with("turn/")),
            "{:?}",
            fake.methods()
        );
    }

    /// M4：originator 核对失败只返回结果、不排队，也不再有原地重启（会杀 TUI 正在跑的回合）。
    #[test]
    fn originator_mismatch_neither_queues_nor_restarts() {
        let fake = fake_server(
            |request| match request["method"].as_str().unwrap_or_default() {
                "initialize" => vec![reply(
                    request,
                    json!({ "userAgent": "meowo/0.167.2 (Windows)" }),
                )],
                "thread/read" => vec![reply(request, json!({ "thread": { "id": "main" } }))],
                "thread/loaded/list" => vec![reply(request, json!({ "data": ["main"] }))],
                _ => vec![reply(request, json!({}))],
            },
        );
        assert_eq!(
            live(&fake, "main"),
            Ok(LiveOutcome::OriginatorMismatch {
                main: "main".into()
            })
        );
        assert!(fake.params_of("thread/queue/add").is_empty());
        let source = include_str!("codex_app_server.rs");
        assert!(!source.contains(concat!("fn restart_", "in_place")));
    }

    /// H2：queue/add 已写出但回执超时 = 可能已送达（uncertain），不是 failed；服务明确拒绝才是没送。
    #[test]
    fn queue_add_without_receipt_is_uncertain() {
        fn answering(add: fn(&Value) -> Vec<Value>) -> Fake {
            fake_server(
                move |request| match request["method"].as_str().unwrap_or_default() {
                    "initialize" => vec![reply(request, json!({ "userAgent": TUI_UA }))],
                    "thread/read" => vec![reply(request, json!({ "thread": { "id": "main" } }))],
                    "thread/loaded/list" => vec![reply(request, json!({ "data": ["main"] }))],
                    _ => add(request),
                },
            )
        }
        let silent = answering(|_| vec![]); // queue/add 收下了但不回
        assert_eq!(
            live(&silent, "main"),
            Err(SendError::MaybeSent(UNCERTAIN.into()))
        );
        assert_eq!(silent.params_of("thread/queue/add").len(), 1);
        let rejecting = answering(|request| vec![rpc_error(request)]);
        assert_eq!(
            live(&rejecting, "main"),
            Err(SendError::NotSent("lead/codex_server_rejected".into()))
        );
        // 连不上 = 确定没发出。
        let (db, task) = (PathBuf::from("db"), PathBuf::from("task"));
        let outcome = queue_live(1, "x", &target(&db, &task, "main", "Q"), "k", FAST);
        assert!(matches!(outcome, Err(SendError::NotSent(_))), "{outcome:?}");
    }

    type SendResult = Result<(&'static str, Option<String>), SendError>;

    /// H2：作答入口与巡检并发投同一关卡，只发一次；uncertain 与中断留下的 sending 都不自动重投，
    /// 面板手动重试（clear_failure）后才再发。
    #[test]
    fn concurrent_deliveries_send_once_and_uncertain_never_auto_resends() {
        let root = temp_root("h2");
        let db = root.join("board.db");
        let task = root.join("task");
        let sends = AtomicUsize::new(0);
        let slow_send = |_: &NativeTarget<'_>, _: &str| -> SendResult {
            sends.fetch_add(1, Ordering::SeqCst);
            // 模拟等 TUI 握手 / 冷路径起进程的那几秒。
            std::thread::sleep(Duration::from_millis(300));
            Ok(("accepted", Some("qs".to_owned())))
        };
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let t = target(&db, &task, "n", "Q-race");
                    assert_eq!(deliver_with(&t, slow_send), Ok(NativeOutcome::Delivered));
                });
            }
        });
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        let key = delivery_key(&task, "Q-race", "n");
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");

        // 回执超时：记 uncertain；冷却过了也不自动重投。
        let t = target(&db, &task, "n", "Q-timeout");
        let key = delivery_key(&task, "Q-timeout", "n");
        let calls = AtomicUsize::new(0);
        let timeout_send = |_: &NativeTarget<'_>, _: &str| -> SendResult {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(SendError::MaybeSent(UNCERTAIN.into()))
        };
        deliver_with(&t, timeout_send).unwrap();
        let mut record = read_record(&db, &key).unwrap();
        assert_eq!(record.state, "uncertain");
        record.at_ms = 0; // 远超 RETRY_COOLDOWN_MS
        write_record(&db, &key, &record);
        deliver_with(&t, timeout_send).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "uncertain 被自动重投");
        let st = status(&db, 7, "c".into(), &task, "Q-timeout", "n");
        assert_eq!((st.state.as_str(), st.can_retry), ("uncertain", true));
        assert_eq!(st.reason.as_deref(), Some(UNCERTAIN));
        // 面板手动重试：清掉后再送一次。
        clear_failure(&db, &task, "Q-timeout", "n");
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            Ok(("accepted", None))
        })
        .unwrap();
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");

        // 上一个进程送到一半被杀，留下 sending 占位：不自动重投，面板显示 uncertain 可手动重试。
        let key = delivery_key(&task, "Q-crash", "n");
        write_record(
            &db,
            &key,
            &DeliveryRecord {
                state: "sending".into(),
                reason: None,
                at_ms: 0,
                submission: None,
                pid: Some(1),
            },
        );
        let t = target(&db, &task, "n", "Q-crash");
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            panic!("sending 占位不该触发重投")
        })
        .unwrap();
        let st = status(&db, 7, "c".into(), &task, "Q-crash", "n");
        assert_eq!((st.state.as_str(), st.can_retry), ("uncertain", true));

        // 确定没发出的 failed 仍按冷却自动重试（原有行为）。
        let t = target(&db, &task, "n", "Q-failed");
        let key = delivery_key(&task, "Q-failed", "n");
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            Err(SendError::NotSent("lead/codex_server_unreachable".into()))
        })
        .unwrap();
        let mut record = read_record(&db, &key).unwrap();
        assert_eq!(record.state, "failed");
        record.at_ms = 0;
        write_record(&db, &key, &record);
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            Ok(("queued", None))
        })
        .unwrap();
        assert_eq!(read_record(&db, &key).unwrap().state, "queued");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 发送途中面板查询：持锁期间如实显示 sending、不给重试按钮。
    #[test]
    fn status_shows_sending_while_in_flight() {
        let root = temp_root("inflight");
        let db = root.join("board.db");
        let task = root.join("task");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (db_ref, task_ref) = (&db, &task);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let t = target(db_ref, task_ref, "n", "Q");
                deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    Ok(("accepted", None))
                })
                .unwrap();
            });
            started_rx.recv().unwrap();
            let st = status(&db, 7, "c".into(), &task, "Q", "n");
            assert_eq!((st.state.as_str(), st.can_retry), ("sending", false));
            release_tx.send(()).unwrap();
        });
        let st = status(&db, 7, "c".into(), &task, "Q", "n");
        assert_eq!((st.state.as_str(), st.can_retry), ("accepted", false));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// H1：判忙只看服务真实状态（thread 状态 active 或队列非空），全程只读。
    #[test]
    fn server_busy_reads_thread_status_and_queue() {
        let busy = codex_like(
            vec![("sub", Some("main"), "idle"), ("main", None, "active")],
            0,
        );
        assert_eq!(server_busy(busy.port, &busy.token, FAST), Ok(true));
        let idle = codex_like(vec![("main", None, "idle")], 0);
        assert_eq!(server_busy(idle.port, &idle.token, FAST), Ok(false));
        let pending = codex_like(vec![("main", None, "idle")], 1);
        assert_eq!(server_busy(pending.port, &pending.token, FAST), Ok(true));
        for fake in [&busy, &idle, &pending] {
            assert!(fake
                .methods()
                .iter()
                .all(|m| !m.starts_with("turn/") && m != "thread/queue/add"));
        }
        assert!(server_busy(1, "x", FAST).is_err());
        assert_eq!(existing_action(&Ok(true)), Existing::Reuse);
        assert_eq!(existing_action(&Ok(false)), Existing::Retire);
        assert_eq!(existing_action(&Err("x".into())), Existing::Retire);
    }

    fn plant_server(db: &Path, name: &str, session: i64, fake: &Fake) -> PathBuf {
        let dir = servers_dir(db).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), &fake.token).unwrap();
        write_state(
            &dir,
            &ServerState {
                launch_session: session,
                // 不存在的 pid：retire 核身份失败就只删目录，不会误杀任何进程。
                server_pid: 4_000_000_000,
                port: fake.port,
                started_at_ms: 0,
                tui_pid: None,
                tui_exited_at_ms: Some(0),
                tui_seen_at_ms: None,
            },
        );
        dir
    }

    /// H1：重开会话时，后台正在跑答复回合的旧服务被接上（同端口同 token），不被收掉；
    /// 空闲的旧服务照旧收掉重起；别的会话的服务不动。
    #[test]
    fn reopening_session_reuses_busy_server_and_retires_idle_one() {
        let root = temp_root("h1");
        let db = root.join("board.db");
        let busy = codex_like(vec![("main", None, "active")], 0);
        let idle = codex_like(vec![("main", None, "idle")], 0);
        let other = codex_like(vec![("x", None, "idle")], 0);
        let busy_dir = plant_server(&db, "busy", 5, &busy);
        let idle_dir = plant_server(&db, "idle", 5, &idle);
        let other_dir = plant_server(&db, "other", 6, &other);
        let reused = settle_existing(&db, 5, |_, _| true).expect("忙的服务应被复用");
        assert_eq!(reused.0, busy_dir);
        assert_eq!(
            (reused.1.port, reused.2.as_str()),
            (busy.port, busy.token.as_str())
        );
        assert!(busy_dir.exists(), "忙的服务被收掉了");
        assert!(!idle_dir.exists(), "空闲的旧服务应收掉");
        assert!(other_dir.exists());
        assert!(other.methods().is_empty(), "不该探询别的会话的服务");
        assert!(verify_identity(busy.port, &busy.token));
        // 进程已不在的服务：只清目录。
        assert!(settle_existing(&db, 5, |_, _| false).is_none());
        assert!(!busy_dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// L4：端口上是别人的监听（不认 token 也回 101，或根本不是 WebSocket）不算我们的服务。
    #[test]
    fn identity_check_rejects_foreign_listener() {
        use std::io::{Read, Write};
        fn canned(response: &'static [u8]) -> u16 {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for mut stream in listener.incoming().flatten() {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let _ = stream.write_all(response);
                }
            });
            port
        }
        let ours = codex_like(vec![], 0);
        assert!(verify_identity(ours.port, &ours.token));
        assert!(!verify_identity(ours.port, "wrong-token"));
        // 冒充者：任何握手都回 101。
        let imposter = canned(b"HTTP/1.1 101 Switching Protocols\r\n\r\n");
        assert!(!verify_identity(imposter, &ours.token));
        // 普通 HTTP 服务。
        let http = canned(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        assert!(!verify_identity(http, &ours.token));
        assert!(
            ours.methods().is_empty(),
            "身份核对不得发 initialize（会抢 originator）"
        );
    }

    /// M5：Meowo 退出时没来得及记 TUI 退出时刻，重启后按最后一次心跳补算 linger，过期即回收。
    #[test]
    fn linger_counts_from_last_seen_when_exit_unrecorded() {
        let mut state = ServerState {
            launch_session: 1,
            server_pid: 1,
            port: 1,
            started_at_ms: 0,
            tui_pid: None,
            tui_exited_at_ms: None,
            tui_seen_at_ms: Some(1_000),
        };
        assert!(!linger_expired(&state, 1_500, 1_000));
        assert!(linger_expired(&state, 2_000, 1_000));
        state.tui_exited_at_ms = Some(5_000);
        assert!(!linger_expired(&state, 5_500, 1_000));
        state.tui_exited_at_ms = None;
        state.tui_seen_at_ms = None;
        assert!(
            linger_expired(&state, 1_000, 1_000),
            "无心跳按服务启动时刻算"
        );
        // 旧版本写的 state.json 没有 tui_seen_at_ms 也能读。
        let legacy: ServerState = serde_json::from_str(
            r#"{"launch_session":1,"server_pid":2,"port":3,"started_at_ms":4,"tui_pid":null,"tui_exited_at_ms":null}"#,
        )
        .unwrap();
        assert_eq!(legacy.tui_seen_at_ms, None);
    }

    /// L1：服务目录名会出现在 app-server 命令行里（--ws-token-file），不得含 token 片段。
    #[test]
    fn server_dir_name_is_not_derived_from_token() {
        let source = include_str!("codex_app_server.rs");
        assert!(!source.contains(concat!("&tok", "en[..")));
    }
}
