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
    /// 启动时的会话 id（新建会话是临时负 id；投递按 DB 记的 app-server pid 找，不靠它）。
    launch_session: i64,
    server_pid: u32,
    port: u16,
    started_at_ms: i64,
    tui_pid: Option<u32>,
    tui_exited_at_ms: Option<i64>,
}

/// 内存里的活服务：持有 Child 句柄（结束时按句柄杀，不怕 pid 复用），以及原样重启所需的参数。
struct Live {
    child: std::process::Child,
    spawn: SpawnSpec,
}

#[derive(Clone)]
struct SpawnSpec {
    argv: Vec<String>,
    env: Vec<(String, String)>,
    cwd: Option<PathBuf>,
}

/// TUI 已退出、只在等投递的服务 pid。看板判活用的进程快照要剔除它们，否则会话卡片在 linger 期间
/// 一直显示「运行中」（hook 把 app-server 记成了会话 pid）。由看守线程维护。
static LINGERING: LazyLock<Mutex<std::collections::HashSet<u32>>> =
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

fn wait_listening(port: u16, child: &mut std::process::Child) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    for _ in 0..150 {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return false;
        }
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// 启动一次远程模式的句柄：spawn 成功后交回 [`bind_tui`]，失败交回 [`abort`]。
pub(crate) struct Launch {
    dir: PathBuf,
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
    let token = crate::remote::generate_token().ok()?;
    let port = free_port()?;
    // 同一会话上一代的服务带着已吊销的 scoped token（hook 用它认领/递审批），不能复用：先收掉。
    retire_for_session(&db, session_id);
    let dir = servers_dir(&db).join(format!("{}-{}", now_ms(), &token[..8]));
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
    let mut child = match spawn_server(&dir, &spec) {
        Ok(child) => child,
        Err(error) => {
            eprintln!("codex app-server 启动失败，按内嵌模式启动: {error}");
            let _ = std::fs::remove_dir_all(&dir);
            return None;
        }
    };
    if !wait_listening(port, &mut child) {
        eprintln!("codex app-server 未在限期内监听，按内嵌模式启动");
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
        },
    );
    if let Ok(mut live) = LIVE.lock() {
        live.insert(dir.clone(), Live { child, spawn: spec });
    }
    let mut tui = split.prefix;
    tui.extend(split.tui);
    tui.extend([
        "--remote".into(),
        format!("ws://127.0.0.1:{port}"),
        "--remote-auth-token-env".into(),
        TOKEN_ENV.into(),
    ]);
    *command.get_argv_mut() = tui.into_iter().map(Into::into).collect();
    command.env(TOKEN_ENV, &token);
    Some(Launch { dir })
}

/// TUI 已 spawn：记下它的 pid，并起看守线程——TUI 退出后服务再活 linger 时长，到点收掉。
pub(crate) fn bind_tui(launch: Launch, tui_pid: Option<u32>) {
    if let Some(mut state) = read_state(&launch.dir) {
        state.tui_pid = tui_pid;
        write_state(&launch.dir, &state);
    }
    spawn_reaper(launch.dir);
}

/// TUI 没起来：服务没有用处，立即收掉。
pub(crate) fn abort(launch: Launch) {
    retire(&launch.dir);
}

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

fn spawn_reaper(dir: PathBuf) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        let Some(mut state) = read_state(&dir) else {
            return; // 已被新一代顶替或收掉
        };
        if !server_alive(&dir, &state) {
            retire(&dir);
            return;
        }
        let tui_alive = state.tui_pid.is_some_and(pid_alive);
        set_lingering(state.server_pid, !tui_alive);
        if tui_alive {
            continue;
        }
        match state.tui_exited_at_ms {
            None => {
                state.tui_exited_at_ms = Some(now_ms());
                write_state(&dir, &state);
            }
            Some(at) if now_ms().saturating_sub(at) >= linger_ms() => {
                retire(&dir);
                return;
            }
            Some(_) => {}
        }
    });
}

/// 应用启动时调用：上次运行留下的服务重新纳入看守（到点收掉），已死的清目录。
/// Meowo 退出时 TUI 随托管终端结束，但 app-server 不在 ConPTY 里，会活下来——正好用于离线投递。
pub(crate) fn sweep_on_startup() {
    let db = crate::db_path();
    for (dir, state) in states(&db) {
        if server_alive(&dir, &state) {
            spawn_reaper(dir);
        } else {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// 该 pid 若是 Meowo 为某个会话起的 app-server，返回它的 TUI 是否还活着；不是则 None。
/// 「会话进程活着」的判定（看板、恢复守卫、离线推送）要看 TUI：TUI 一退，服务只是在等投递，
/// 不是「外部终端里的会话」，恢复会话时会被新一代顶替。
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
    /// accepted = app-server 已排进 thread；queued = 已落进 codex 的持久队列，下次恢复会话时消费；failed = 没送出。
    state: String,
    reason: Option<String>,
    at_ms: i64,
    submission: Option<String>,
}

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

/// 送出一次答复。投递失败不报错给作答方（答复已保存）：记成 failed，面板显示原因与「继续送达」。
pub(crate) fn deliver(target: &NativeTarget<'_>) -> Result<NativeOutcome, String> {
    let key = delivery_key(target.task, target.gate_id, target.native_session);
    if let Some(record) = read_record(target.db, &key) {
        match record.state.as_str() {
            // 已送出的绝不重发：queue 会真的再开一轮。
            "accepted" | "queued" => return Ok(NativeOutcome::Delivered),
            _ if now_ms().saturating_sub(record.at_ms) < RETRY_COOLDOWN_MS => {
                return Ok(NativeOutcome::Delivered)
            }
            _ => {}
        }
    }
    let record = match send(target, &key) {
        Ok((state, submission)) => DeliveryRecord {
            state: state.into(),
            reason: None,
            at_ms: now_ms(),
            submission,
        },
        Err(reason) => DeliveryRecord {
            state: "failed".into(),
            reason: Some(reason),
            at_ms: now_ms(),
            submission: None,
        },
    };
    write_record(target.db, &key, &record);
    Ok(NativeOutcome::Delivered)
}

fn send(target: &NativeTarget<'_>, key: &str) -> Result<(&'static str, Option<String>), String> {
    let store = crate::open_store(target.db)?;
    let pid = store.session_pid(target.session_id).ok().flatten();
    let live = states(target.db).into_iter().find(|(dir, state)| {
        (pid == Some(i64::from(state.server_pid)) || state.launch_session == target.session_id)
            && server_alive(dir, state)
    });
    if let Some((dir, state)) = live {
        // 刚起的服务给 TUI 留出先握手的时间（originator 由第一个 initialize 的客户端决定）。
        let age = now_ms().saturating_sub(state.started_at_ms);
        if age < TUI_HANDSHAKE_GRACE_MS {
            std::thread::sleep(Duration::from_millis((TUI_HANDSHAKE_GRACE_MS - age) as u64));
        }
        match queue_live(&dir, &state, target, key) {
            Ok(Some(submission)) => return Ok(("accepted", Some(submission))),
            // thread 不在这个服务里（TUI 里 /new 过、或服务刚重启还没 resume）：落持久队列。
            Ok(None) => {}
            Err(reason) => return Err(reason),
        }
    }
    queue_cold(target, &store).map(|submission| ("queued", submission))
}

/// 经活着的 app-server 排队。Ok(None) = thread 未加载在该服务上。
fn queue_live(
    dir: &Path,
    state: &ServerState,
    target: &NativeTarget<'_>,
    key: &str,
) -> Result<Option<String>, String> {
    let token =
        std::fs::read_to_string(dir.join("token")).map_err(|_| "lead/codex_server_unreachable")?;
    let mut client = ws::Client::connect(state.port, token.trim(), RPC_TIMEOUT)?;
    let init = client.call(
        "initialize",
        json!({
            "clientInfo": { "name": "meowo", "title": "Meowo", "version": env!("CARGO_PKG_VERSION") },
            "capabilities": { "experimentalApi": true },
        }),
        RPC_TIMEOUT,
    )?;
    client.notify("initialized")?;
    if !originator_is_tui(init.get("userAgent").and_then(Value::as_str)) {
        // 宿主成了第一个握手的客户端：这个进程后续所有请求都会记在 meowo 名下。原样重启服务
        // （同端口同 token，TUI 自动重连并恢复原 thread），这次不投递，留给重试。
        drop(client);
        restart_in_place(dir);
        return Err("lead/codex_originator_unconfirmed".into());
    }
    let loaded = client.call("thread/loaded/list", json!({}), RPC_TIMEOUT)?;
    let has_thread = loaded
        .get("data")
        .and_then(Value::as_array)
        .is_some_and(|ids| {
            ids.iter()
                .any(|id| id.as_str() == Some(target.native_session))
        });
    if !has_thread {
        return Ok(None);
    }
    let result = client.call(
        "thread/queue/add",
        queue_add_params(target, key),
        RPC_TIMEOUT,
    )?;
    Ok(result
        .pointer("/queuedSubmission/id")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// 只用 thread/queue/add：turn/start 等于插话（打断或并入当前回合），queue 才是「排到这一轮之后」，
/// 且不碰 TUI 输入框里的草稿。clientUserMessageId 用投递键，便于在 codex 侧对账。
fn queue_add_params(target: &NativeTarget<'_>, key: &str) -> Value {
    json!({
        "threadId": target.native_session,
        "clientUserMessageId": key,
        "input": [{ "type": "text", "text": target.message }],
    })
}

fn originator_is_tui(user_agent: Option<&str>) -> bool {
    user_agent.is_some_and(|ua| ua.starts_with("codex-tui/"))
}

fn restart_in_place(dir: &Path) {
    let Ok(mut live) = LIVE.lock() else {
        return;
    };
    let Some(entry) = live.get_mut(dir) else {
        return;
    };
    crate::proc::kill_descendants(entry.child.id());
    let _ = entry.child.kill();
    let _ = entry.child.wait();
    if let Ok(child) = spawn_server(dir, &entry.spawn) {
        let pid = child.id();
        entry.child = child;
        if let Some(mut state) = read_state(dir) {
            state.server_pid = pid;
            state.started_at_ms = now_ms();
            write_state(dir, &state);
        }
    }
}

/// 冷路径：`codex queue --thread <id> --message <答复>` 只写 codex 的持久队列、不调模型；
/// 下次在 Meowo 里恢复这个会话时由 TUI 消费（隔离实测：resume 后答复被执行，originator=codex-tui）。
fn queue_cold(
    target: &NativeTarget<'_>,
    store: &meowo_store::Store,
) -> Result<Option<String>, String> {
    let plugin = meowo_agent::by_id(target.provider).ok_or("lead/provider_missing")?;
    let profile = store.session_profile(target.session_id).ok().flatten();
    let env = crate::terminal::launch_env_for_exact_profile(plugin, profile.as_deref());
    let argv = plugin.launch_argv();
    let mut command = std::process::Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .args(cold_queue_args(target.native_session, target.message))
        .envs(env)
        .stdin(std::process::Stdio::null());
    if let Some(cwd) = target.cwd.filter(|cwd| Path::new(cwd).is_dir()) {
        command.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|_| "lead/codex_queue_failed")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || !stdout.contains("Queued message") {
        return Err("lead/codex_queue_failed".into());
    }
    Ok(stdout
        .split_whitespace()
        .skip_while(|word| *word != "message")
        .nth(1)
        .map(str::to_owned))
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
    let (state, reason) = match read_record(db, &key) {
        Some(record) => (record.state, record.reason),
        None => ("saved".to_owned(), None),
    };
    LeadDeliveryDto {
        session_id,
        cwd,
        can_retry: matches!(state.as_str(), "failed" | "saved"),
        delivery_id: key,
        state,
        reason,
    }
}

/// 面板「继续送达」：清掉失败记录的冷却，立即重投。已送出的不受影响。
pub(crate) fn clear_failure(db: &Path, task: &Path, gate_id: &str, native: &str) {
    let key = delivery_key(task, gate_id, native);
    if read_record(db, &key).is_some_and(|record| record.state == "failed") {
        let _ = std::fs::remove_file(record_path(db, &key));
    }
}

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
        let db = PathBuf::from("db");
        let task = PathBuf::from("task");
        let target = NativeTarget {
            db: &db,
            session_id: 1,
            provider: "codex",
            native_session: "019a-thread",
            cwd: None,
            task: &task,
            gate_id: "Q-1",
            message: "answer",
        };
        let params = queue_add_params(&target, "k");
        assert_eq!(params["threadId"], "019a-thread");
        assert_eq!(params["clientUserMessageId"], "k");
        assert_eq!(params["input"][0]["text"], "answer");
        let source = include_str!("codex_app_server.rs");
        // 源码里所有宿主发出的 RPC 方法名（rustfmt 可能把参数拆行，故按空白宽松匹配）。
        let call = regex::Regex::new(r#"\.call\(\s*"([^"]+)""#).unwrap();
        let rpc_calls: Vec<&str> = call
            .captures_iter(source)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        assert_eq!(
            rpc_calls,
            ["initialize", "thread/loaded/list", "thread/queue/add"]
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
        };
        // 已 accepted：deliver 直接返回，不连服务、不改记录。
        assert_eq!(deliver(&target), Ok(NativeOutcome::Delivered));
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");
        let _ = std::fs::remove_dir_all(&root);
    }
}
