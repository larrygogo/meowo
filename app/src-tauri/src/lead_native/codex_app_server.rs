//! Codex 原生 TUI 的自动续跑（路线 C1，任务 .lead/20261113-lead-tui）。
//!
//! 启动：Lead 已安装的 codex 会话由 Meowo 先起一个**只属于该 TUI** 的 `codex app-server`
//! （ws://127.0.0.1:<端口> + capability token），再让托管终端里的 TUI 以 `--remote` 连上去。
//! TUI 一退，服务随之收掉（leader 2026-11-13 定：不需要离线续跑）。
//! 答复：宿主作为第二个客户端连同一个 app-server，用 `thread/queue/add` 把答复排进 TUI 正在用的
//! thread——等于用户在 TUI 里按 Tab 排队，空闲时立刻开新回合，TUI 上看得到。
//! TUI 不在（或还没握手、或不在那个 thread 上）时不投递，记成 waiting：答复已保存，
//! 下次打开会话、TUI 握手后由关卡巡检经同一条在线路径送达。
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
//!   userAgent 以 `codex-tui/` 开头，宿主先握手则变成 `meowo/`。所以宿主在确认 TUI 已握手之前
//!   绝不 initialize（见 [`await_handshake`]）；连上后仍核对 userAgent 前缀，不对就不投递。
//!   宿主自报 clientInfo 名 meowo（不冒充）；queue 投递的请求头 originator 仍是 codex-tui，
//!   只有 UA 末尾客户端名与 turn_trigger=queue 不同。
//! - 不用 `turn/start`：那是插话（打断/并入当前回合）；queue 不动输入框草稿（实测草稿原样保留）。
//! - 不走 ACP、不往 PTY 打字、不用 `codex exec`（originator=codex_exec，可能另算）。
use super::ws;
use super::{NativeOutcome, NativeTarget};
use meowo_protocol::ipc::LeadDeliveryDto;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

/// TUI 读 token 的环境变量名（`--remote-auth-token-env`）。不放进 app-server 的环境：
/// hook/MCP 都是 app-server 的后代，没必要让它们拿到连回服务的凭据。
const TOKEN_ENV: &str = "MEOWO_CODEX_REMOTE_TOKEN";
/// TUI 启动到完成 initialize 实测 <1s；宿主至少等这么久才去找握手证据。
const TUI_HANDSHAKE_GRACE_MS: i64 = 5_000;
/// 看到 TUI 已连上端口后再等这么久才认定它握过手：TUI 建好 WebSocket 后的第一条消息就是
/// initialize（不经任何用户交互），1 秒远够；只看 TCP 已连通会在「刚连上还没发 initialize」的
/// 瞬间抢先（N6）。
const HANDSHAKE_SETTLE: Duration = Duration::from_secs(1);
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// 确定没送出（failed）的自动重试最小间隔，避免服务不可达时每轮巡检都刷一次。
const RETRY_COOLDOWN_MS: i64 = 20_000;
/// 跨进程投递占位的过期时限：一次投递最长是握手等待 + 一次 RPC 时限（约 20s）；占位进程被杀、
/// 或 pid 被别的进程复用时，过了这么久就当作无主，不让那个关卡永远卡在「另一个进程正在送」。
const CLAIM_STALE_MS: i64 = 5 * 60_000;

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

// ─── 每个 TUI 的服务（状态落盘：投递要按会话找到它，Meowo 重启后要能清掉遗留） ──────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerState {
    /// 接上它的会话 id（新建会话是临时负 id；投递按 DB 记的 app-server pid 找，不靠它）。
    launch_session: i64,
    server_pid: u32,
    port: u16,
    started_at_ms: i64,
    tui_pid: Option<u32>,
    /// 已确认 TUI 先于宿主握过手（originator 归 codex-tui）。一个服务进程的 originator 定了就不变，
    /// 确认一次即可，后续投递不必再找证据。旧版本写的 state.json 没有此字段，读作 false。
    #[serde(default)]
    tui_handshaken: bool,
}

/// TUI 已退出、看守线程还没来得及收掉的服务 pid。看板判活用的进程快照要剔除它们，
/// 否则会话卡片会短暂显示「运行中」（hook 把 app-server 记成了会话 pid）。
static LINGERING: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// 已有看守线程的服务目录：避免重复起看守。
static REAPING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub(crate) fn lingering_server_pids() -> HashSet<u32> {
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

/// 内存里的活服务：持有 Child 句柄（结束时按句柄杀，不怕 pid 复用）。
static LIVE: LazyLock<Mutex<HashMap<PathBuf, std::process::Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    if let Some(mut child) = live {
        crate::proc::kill_descendants(child.id());
        let _ = child.kill();
        let _ = child.wait();
    } else if let Some(state) = read_state(dir).filter(|state| server_alive(dir, state)) {
        crate::proc::kill_descendants(state.server_pid);
        crate::proc::kill_pid(state.server_pid);
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// 收掉某会话名下的全部服务。重开会话时调用：服务随 TUI 生灭，正常情况下这里已经空了；
/// 留下的只会是上一代 TUI 刚退、看守线程还没来得及收的那个。
fn retire_session(db: &Path, session_id: i64) {
    let pid = crate::open_store(db)
        .ok()
        .and_then(|store| store.session_pid(session_id).ok().flatten());
    for (dir, state) in states(db) {
        if state.launch_session == session_id || pid == Some(i64::from(state.server_pid)) {
            retire(&dir);
        }
    }
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
}

/// 托管 PTY spawn 前调用（pty.rs start_spawned）：条件满足就起 app-server 并把 `command` 改成
/// `--remote` 形态；任何一步不成都原样返回 None，会话照旧内嵌启动（只是没有自动续跑）。
pub(crate) fn prepare_launch(
    session_id: i64,
    provider: &str,
    env: &[(String, String)],
    command: &mut CommandBuilder,
) -> Option<Launch> {
    use meowo_agent::lead_install::ServerProtocol;
    // 只给装了 Lead 的账号改形态：没有关卡就用不到投递通道，不必多一个常驻进程。
    // 不直接调 mechanism()：它对有版本门的 provider 会同步探测 CLI 版本，而每个会话启动都经过这里（做空 R2）。
    if !super::launches_server(provider, ServerProtocol::CodexAppServer, || {
        crate::lead_runtime::enabled(provider, env)
    }) {
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
    // 服务只跟一代 TUI：上一代留下的（它带着旧 scoped token 与旧 MCP 环境）一律收掉，不复用。
    retire_session(&db, session_id);
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
            tui_handshaken: false,
        },
    );
    if let Ok(mut live) = LIVE.lock() {
        live.insert(dir.clone(), child);
    }
    point_tui_at(command, split, port, &token);
    Some(Launch { dir, session_id })
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

/// TUI 已 spawn：记下它的 pid，并起看守线程——TUI 一退就收掉服务。
pub(crate) fn bind_tui(launch: Launch, tui_pid: Option<u32>) {
    if let Some(mut state) = read_state(&launch.dir) {
        state.tui_pid = tui_pid;
        state.launch_session = launch.session_id;
        write_state(&launch.dir, &state);
    }
    spawn_reaper(launch.dir);
}

/// TUI 没起来：服务没有用处，立即收掉。
pub(crate) fn abort(launch: Launch) {
    retire(&launch.dir);
}

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
        std::thread::sleep(Duration::from_secs(2));
        let Some(state) = read_state(dir) else {
            return; // 已被收掉
        };
        // 不在 TUI 退出后保留服务：不做离线续跑（leader 2026-11-13），没写完的回合随 TUI 一起结束，
        // 与内嵌模式下关掉 TUI 的效果一致。
        if !server_alive(dir, &state) || !state.tui_pid.is_some_and(pid_alive) {
            set_lingering(state.server_pid, true);
            retire(dir);
            return;
        }
    }
}

/// Meowo 退出时调用（托盘退出的 RunEvent::Exit 与「重启并更新」的 on_before_exit 两条路径）：
/// 收掉本进程起的全部服务。app-server 由 Meowo 直接 spawn，不在 TUI 的 ConPTY 进程树里，
/// 收托管终端杀不到它；不收的话 TUI 正在跑的那一轮会在后台无人看着继续改文件、遇到审批没人能答，
/// 服务与它的 MCP 子进程一直挂到下次启动（做空 R1）。只动内存里持有句柄的（本进程起的）：
/// 盘上别的服务可能属于共库的另一个 Meowo 实例，由它自己收。
pub(crate) fn retire_all() {
    let dirs: Vec<PathBuf> = LIVE
        .lock()
        .map(|live| live.keys().cloned().collect())
        .unwrap_or_default();
    for dir in dirs {
        retire(&dir);
    }
}

/// 应用启动时调用：收掉上次运行遗留的服务（正常退出时 [`retire_all`] 已收，这里兜崩溃/被杀的情况：
/// app-server 不在 ConPTY 里，会活下来）。TUI 还活着的不动：dev 版与安装版共用 ~/.meowo，那是另一个正在运行的
/// Meowo 实例的服务，由它自己的看守线程管。
pub(crate) fn sweep_on_startup() {
    let db = crate::db_path();
    for (dir, state) in states(&db) {
        if !state.tui_pid.is_some_and(pid_alive) {
            retire(&dir);
        }
    }
}

/// 该 pid 若是 Meowo 为某个会话起的 app-server，返回它的 TUI 是否还活着；不是则 None。
/// 「会话进程活着」的判定（看板、恢复守卫）要看 TUI：TUI 一退，服务马上被收掉，不是「外部终端里的会话」。
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

// ─── TUI 握手证据（N6） ──────────────────────────────────────────────────────

/// 宿主能否 initialize：只有确认 TUI 已先握手才行，否则宿主会成为第一个握手的客户端，这个服务
/// 之后所有回合的 originator 都变成 meowo（可能另算）。证据按代价从低到高：
/// 1. state 里记过已确认（originator 一旦定了不会变）；
/// 2. DB 里会话 pid 就是这个服务：hook 只在回合里触发（codex 要到首个 turn 才认领），而宿主在确认前
///    从不排队，所以这个服务上跑过的回合只能是 TUI 发起的，TUI 必然已握手；
/// 3. TUI（或它的子进程，npm 包装时真正连接的是 node 下面的 codex）对服务端口有一条已建立的 TCP
///    连接，且隔 [`HANDSHAKE_SETTLE`] 后仍在。覆盖「刚恢复会话、还没发过消息」——此时没有 hook。
///
/// `connected` 查第 3 条（None = 本平台查不了），`sleep` 注入是为了测试不真等。
fn await_handshake(
    state: &ServerState,
    db_pid: Option<i64>,
    now: i64,
    mut connected: impl FnMut() -> Option<bool>,
    sleep: impl Fn(Duration),
) -> bool {
    if state.tui_handshaken || db_pid == Some(i64::from(state.server_pid)) {
        return true;
    }
    let age = now.saturating_sub(state.started_at_ms);
    if age < TUI_HANDSHAKE_GRACE_MS {
        sleep(Duration::from_millis((TUI_HANDSHAKE_GRACE_MS - age) as u64));
    }
    if connected() != Some(true) {
        return false;
    }
    sleep(HANDSHAKE_SETTLE);
    connected() == Some(true)
}

/// TUI 进程树里是否有进程连着 127.0.0.1:`port`。None = 本平台查不了连接表。
fn tui_connected(port: u16, tui_pid: u32) -> Option<bool> {
    let peers = tcp_client_pids(port)?;
    let mut tree = crate::proc::descendant_pids(&crate::proc::parent_map(), tui_pid);
    tree.insert(tui_pid);
    Some(!peers.is_disjoint(&tree))
}

/// 本机所有「对端是 127.0.0.1:`port`、已建立」的 TCP 连接的属主 pid。
#[cfg(windows)]
fn tcp_client_pids(port: u16) -> Option<HashSet<u32>> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, MIB_TCP_STATE_ESTAB,
        TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    const AF_INET: u32 = 2;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    let mut size: u32 = 0;
    // SAFETY: 传空缓冲区只为取所需大小。
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET,
            TCP_TABLE_OWNER_PID_CONNECTIONS,
            0,
        );
    }
    for _ in 0..4 {
        // u32 元素保证表结构所需的 4 字节对齐；两次调用之间连接数可能变多，不够就按新大小重来。
        let mut buf = vec![0u32; (size as usize).div_ceil(4) + 1];
        let mut len = (buf.len() * 4) as u32;
        // SAFETY: buf 长度 len 字节，可写；API 只在其中写表。
        let rc = unsafe {
            GetExtendedTcpTable(
                buf.as_mut_ptr().cast(),
                &mut len,
                0,
                AF_INET,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            )
        };
        if rc == ERROR_INSUFFICIENT_BUFFER {
            size = len;
            continue;
        }
        if rc != 0 {
            return None;
        }
        // SAFETY: 成功时缓冲区开头是 MIB_TCPTABLE_OWNER_PID，后接 dwNumEntries 行（都在 len 之内）。
        let rows = unsafe {
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize)
        };
        // 地址与端口都是网络字节序：地址按内存字节比，端口取低 16 位再转主机序。
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        return Some(
            rows.iter()
                .filter(|row| {
                    row.dwState == MIB_TCP_STATE_ESTAB as u32
                        && row.dwRemoteAddr == loopback
                        && u16::from_be(row.dwRemotePort as u16) == port
                })
                .map(|row| row.dwOwningPid)
                .collect(),
        );
    }
    None
}

/// macOS/Linux：借 lsof（macOS 自带）。没装 lsof 就返回 None，只靠前两条证据。
/// lsof 的 `-i @addr:port` 两端都匹配，服务端自己接受的那条也会列出来——它不在 TUI 进程树里，无碍。
#[cfg(not(windows))]
fn tcp_client_pids(port: u16) -> Option<HashSet<u32>> {
    let output = std::process::Command::new("lsof")
        .args([
            "-nP",
            &format!("-iTCP@127.0.0.1:{port}"),
            "-sTCP:ESTABLISHED",
            "-Fp",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    // 没有匹配时 lsof 以 1 退出、输出为空：这是「没有连接」，不是「查不了」。
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix('p')?.parse().ok())
            .collect(),
    )
}

// ─── 投递 ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DeliveryRecord {
    /// sending = 正在送（发送前先落的占位）；accepted = app-server 已排进 thread；
    /// waiting = 没有可投的 TUI（会话没开 / TUI 还没握手 / 不在那个 thread 上），答复已保存，
    /// 巡检每轮再试；failed = 确定没送出（冷却后自动重试）；
    /// uncertain = 请求已发出但没拿到确认，可能已送达（绝不自动重投，面板给手动重试）。
    /// 旧版本还会写 queued（已落进 codex 持久队列），按已送出处理。
    state: String,
    reason: Option<String>,
    at_ms: i64,
    submission: Option<String>,
    /// 写 sending 占位的进程。Meowo 中途退出留下的 sending 由下一个进程按 uncertain 处理。
    #[serde(default)]
    pid: Option<u32>,
}

/// 发送结果里不成功的三类：请求还没发出（可以放心自动重试）、已经发出（可能已送达）、
/// 眼下没有可投的 TUI（等会话打开，reason 说明在等什么）。
#[derive(Debug, PartialEq, Eq)]
enum SendError {
    NotSent(String),
    MaybeSent(String),
    Waiting(Option<String>),
}

/// 可能已送达、但没拿到确认（超时、断开）。
const UNCERTAIN: &str = "lead/codex_delivery_uncertain";
/// 会话开着但还认不出 TUI 已握手（或握手被抢、originator 不对）：宿主不连，等它。
const TUI_NOT_CONNECTED: &str = "lead/codex_tui_not_connected";
/// TUI 开着，但答复要去的 thread 没加载在它的服务上（TUI 里 /new 或切到了别的会话）。
const THREAD_NOT_OPEN: &str = "lead/codex_thread_not_open";

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

/// 按投递键串行化（进程内）：手机作答（answer_and_deliver）与关卡巡检（reconcile_task，每 10s）会并发
/// 调用 deliver；读记录→发送→写记录之间不加锁，两边都读到「未送」就会 queue/add 两次（真开两轮）。
/// 按键分开：别让不相干的关卡排队等。跨进程见 [`Claim`]。
static INFLIGHT: LazyLock<Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key_lock(key: &str) -> std::sync::Arc<Mutex<()>> {
    INFLIGHT
        .lock()
        .map(|mut map| map.entry(key.to_owned()).or_default().clone())
        .unwrap_or_default()
}

/// N9：跨进程投递占位。dev 版与安装版共用 ~/.meowo（用户要求不隔离），两个进程的巡检会同时看到
/// 同一个已作答关卡；进程内的锁管不到对方，读记录到写 sending 之间的窄窗口会双投。
/// 用 create_new 建锁文件做原子占位（文件系统保证只有一个进程建得成），Drop 时删除。
struct Claim(PathBuf);

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn claim_path(db: &Path, key: &str) -> PathBuf {
    root_of(db).join("deliveries").join(format!("{key}.lock"))
}

/// 锁文件内容 `<pid> <毫秒时刻>`；读不出按无主处理（写入途中被杀）。
fn claim_owner(path: &Path) -> Option<(u32, i64)> {
    let body = std::fs::read_to_string(path).ok()?;
    let mut parts = body.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// 别的进程正持有这个键的占位（`foreign_alive` 认它、且没过期）。
fn claim_held_elsewhere(path: &Path, now: i64, foreign_alive: impl Fn(u32) -> bool) -> bool {
    match claim_owner(path) {
        Some((pid, at)) => now.saturating_sub(at) < CLAIM_STALE_MS && foreign_alive(pid),
        // 文件在、内容还读不出：多半是对方刚 create_new、还没写完 pid。刚建的当作有主，
        // 否则会在这一瞬把对方的占位删掉抢走；放久了（写入途中被杀）才算无主。
        None => std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(10)),
    }
}

/// 生产用的判定：占位 pid 不是本进程、且还活着。本进程自己的占位不算：进程内已有 key_lock 管着，
/// 留在盘上的只会是上次 Drop 没删成的残留。
fn foreign_alive(pid: u32) -> bool {
    pid != std::process::id() && pid_alive(pid)
}

fn try_claim(db: &Path, key: &str, foreign_alive: impl Fn(u32) -> bool) -> Option<Claim> {
    use std::io::Write;
    let path = claim_path(db, key);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    for _ in 0..2 {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let _ = write!(file, "{} {}", std::process::id(), now_ms());
                return Some(Claim(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if claim_held_elsewhere(&path, now_ms(), &foreign_alive) {
                    return None;
                }
                // 无主的旧占位（持有进程已死 / 过期 / 本进程残留）：删掉再抢一次。两个进程同时删、
                // 同时建时，create_new 仍只让一个成功。
                let _ = std::fs::remove_file(&path);
            }
            Err(_) => return None,
        }
    }
    None
}

/// 送出一次答复。投递失败不报错给作答方（答复已保存）：记成 waiting/failed/uncertain，面板显示原因。
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
    // 另一个 Meowo 进程正在送这一条：它会写结果，这里什么都不做。
    let Some(_claim) = try_claim(target.db, &key, foreign_alive) else {
        return Ok(NativeOutcome::Delivered);
    };
    if let Some(record) = read_record(target.db, &key) {
        match record.state.as_str() {
            // 已送出的绝不重发：queue 会真的再开一轮。queued 是旧版本冷队列的记录，同样已送出。
            "accepted" | "queued" => return Ok(NativeOutcome::Delivered),
            // 可能已送达：只等用户在面板上手动重试（clear_failure 清掉记录后才会再走到这里）。
            // 拿到占位时看到 sending = 上一个进程送到一半退出了，同样当作「可能已送达」。
            "uncertain" | "sending" => return Ok(NativeOutcome::Delivered),
            // 等会话打开：每轮巡检都再看一眼，TUI 一握手就送（不受失败冷却限制）。
            "waiting" => {}
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
        Err(SendError::Waiting(reason)) => ("waiting", reason, None),
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
    // 会话没开（或 TUI 刚退、服务正被收掉）：不投，等下次打开。
    let Some((dir, mut state)) = live else {
        return Err(SendError::Waiting(None));
    };
    let Some(tui_pid) = state.tui_pid.filter(|tui| pid_alive(*tui)) else {
        return Err(SendError::Waiting(None));
    };
    let token = read_token(&dir)
        .ok_or_else(|| SendError::NotSent("lead/codex_server_unreachable".into()))?;
    let handshaken = await_handshake(
        &state,
        pid,
        now_ms(),
        || tui_connected(state.port, tui_pid),
        std::thread::sleep,
    );
    if !handshaken {
        return Err(SendError::Waiting(Some(TUI_NOT_CONNECTED.into())));
    }
    match queue_live(state.port, &token, target, key, RPC_TIMEOUT)? {
        LiveOutcome::Queued(submission) => {
            remember_handshake(&dir, &mut state);
            Ok(("accepted", submission))
        }
        // thread 不在这个服务里（TUI 里 /new 过、或切到了别的会话）：等 TUI 回到这个会话。
        LiveOutcome::NotLoaded => {
            remember_handshake(&dir, &mut state);
            Err(SendError::Waiting(Some(THREAD_NOT_OPEN.into())))
        }
        // 证据判断失误、宿主成了第一个握手的客户端（不该出现）：这个服务的请求都会记在 meowo 名下，
        // 不投也不重启服务（会杀掉 TUI 正在跑的回合）。TUI 下次重开会换新服务，届时再送。
        LiveOutcome::OriginatorMismatch => Err(SendError::Waiting(Some(TUI_NOT_CONNECTED.into()))),
    }
}

/// 宿主拿到的 userAgent 是 codex-tui：originator 已定，后续投递不必再找握手证据。
fn remember_handshake(dir: &Path, state: &mut ServerState) {
    if !state.tui_handshaken {
        state.tui_handshaken = true;
        write_state(dir, state);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LiveOutcome {
    Queued(Option<String>),
    /// 主线程没加载在这个服务上。
    NotLoaded,
    OriginatorMismatch,
}

/// 经活着的 app-server 排队。调用前必须已确认 TUI 握过手（[`await_handshake`]）。
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
    if !originator_is_tui(init.get("userAgent").and_then(Value::as_str)) {
        return Ok(LiveOutcome::OriginatorMismatch);
    }
    let main = main_thread(&mut client, target.native_session, timeout);
    let loaded = client
        .call("thread/loaded/list", json!({}), timeout)
        .map_err(not_sent)?;
    if !loaded_ids(&loaded).contains(&main) {
        return Ok(LiveOutcome::NotLoaded);
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
    // sending 只有某个进程正持占位发送时才是真的「正在送」（本进程看进程内锁，别的进程看锁文件）；
    // 否则是上次送到一半中断留下的，可能已送达。
    if state == "sending"
        && key_lock(&key).try_lock().is_ok()
        && !claim_held_elsewhere(&claim_path(db, &key), now_ms(), foreign_alive)
    {
        state = "uncertain".into();
        reason = Some(UNCERTAIN.into());
    }
    LeadDeliveryDto {
        session_id,
        cwd,
        // waiting 不给按钮：巡检每轮都在试，会话一打开就送，手动重试不会更快。
        can_retry: matches!(state.as_str(), "failed" | "saved" | "uncertain"),
        delivery_id: key,
        state,
        reason,
    }
}

/// 面板「继续送达」：清掉失败/不确定记录，紧接着的 deliver 立即重投。已送出的不受影响。
/// 持投递锁：正在发送的那次结束前不动记录，结束后若已 accepted 也就不清了；别的进程正在送时也不动。
pub(crate) fn clear_failure(db: &Path, task: &Path, gate_id: &str, native: &str) {
    let key = delivery_key(task, gate_id, native);
    let lock = key_lock(&key);
    let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if claim_held_elsewhere(&claim_path(db, &key), now_ms(), foreign_alive) {
        return;
    }
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
                "thread/read"
            ]
        );
        // 不做离线续跑（leader 2026-11-13）：冷排队路径（`codex queue`）已整条删除，不得复活。
        assert!(!source.contains(concat!("fn queue_", "cold")));
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
        // 主线程没加载在服务上（TUI 切到了别的会话）：不排进子代理，等 TUI 回到主线程。
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
        assert_eq!(live(&fake, "sub"), Ok(LiveOutcome::NotLoaded));
        assert!(fake.params_of("thread/queue/add").is_empty());
    }

    #[test]
    fn subagent_parent_fields_from_protocol() {
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
        assert_eq!(live(&fake, "main"), Ok(LiveOutcome::OriginatorMismatch));
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
            // 模拟等 TUI 握手的那几秒。
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
            Ok(("accepted", None))
        })
        .unwrap();
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");
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

    fn plant_server(db: &Path, name: &str, session: i64, port: u16) -> PathBuf {
        let dir = servers_dir(db).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("token"), "t").unwrap();
        write_state(
            &dir,
            &ServerState {
                launch_session: session,
                // 不存在的 pid：retire 核身份失败就只删目录，不会误杀任何进程。
                server_pid: 4_000_000_000,
                port,
                started_at_ms: 0,
                tui_pid: None,
                tui_handshaken: false,
            },
        );
        dir
    }

    /// 服务只跟一代 TUI（不做离线续跑）：重开会话时上一代留下的服务一律收掉，不复用；
    /// 别的会话的服务不动。
    #[test]
    fn reopening_session_retires_previous_servers() {
        let root = temp_root("reopen");
        let db = root.join("board.db");
        let old = plant_server(&db, "old", 5, 1);
        let older = plant_server(&db, "older", 5, 2);
        let other = plant_server(&db, "other", 6, 3);
        retire_session(&db, 5);
        assert!(!old.exists() && !older.exists());
        assert!(other.exists());
        let source = include_str!("codex_app_server.rs");
        for gone in [
            concat!("fn settle_", "existing"),
            concat!("inherited_", "pty_token"),
        ] {
            assert!(!source.contains(gone), "{gone} 复活了");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 会话没开 / TUI 没握手 / 不在那个 thread 上：记 waiting（答复已保存、不给重试按钮），
    /// 巡检每轮都再试（不受失败冷却限制），TUI 一就绪就经在线 queue/add 送达。
    #[test]
    fn waiting_is_retried_every_patrol_until_tui_is_ready() {
        let root = temp_root("waiting");
        let db = root.join("board.db");
        let task = root.join("task");
        let t = target(&db, &task, "n", "Q-wait");
        let key = delivery_key(&task, "Q-wait", "n");
        let calls = AtomicUsize::new(0);
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(SendError::Waiting(None))
        })
        .unwrap();
        let st = status(&db, 7, "c".into(), &task, "Q-wait", "n");
        assert_eq!(
            (st.state.as_str(), st.can_retry, st.reason.as_deref()),
            ("waiting", false, None)
        );
        // 刚记下就再巡检：不等冷却，照样再试。
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(SendError::Waiting(Some(TUI_NOT_CONNECTED.into())))
        })
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let st = status(&db, 7, "c".into(), &task, "Q-wait", "n");
        assert_eq!(st.reason.as_deref(), Some(TUI_NOT_CONNECTED));
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            Ok(("accepted", Some("qs".into())))
        })
        .unwrap();
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");
        // 旧版本冷队列留下的 queued 记录按已送出处理，不再投。
        let key = delivery_key(&task, "Q-legacy", "n");
        write_record(
            &db,
            &key,
            &DeliveryRecord {
                state: "queued".into(),
                reason: None,
                at_ms: 0,
                submission: None,
                pid: None,
            },
        );
        deliver_with(
            &target(&db, &task, "n", "Q-legacy"),
            |_: &NativeTarget<'_>, _: &str| -> SendResult { panic!("queued 被重投") },
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 一个活着的、不是本进程的 pid（测试进程的父进程：cargo / 测试驱动，测试期间一直在）。
    fn foreign_live_pid() -> u32 {
        let parents = crate::proc::parent_map();
        let parent = parents[&std::process::id()];
        assert!(pid_alive(parent));
        parent
    }

    /// N9：dev 版与安装版共用 ~/.meowo，进程内锁管不到对方。另一个活着的进程持有占位时本进程不送、
    /// 面板如实显示「正在送」；占位过期或持有者已死则视为无主；送完删掉占位。
    #[test]
    fn cross_process_claim_prevents_double_delivery() {
        let root = temp_root("n9");
        let db = root.join("board.db");
        let task = root.join("task");
        let key = delivery_key(&task, "Q", "n");
        let lock = claim_path(&db, &key);
        std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
        let other = foreign_live_pid();
        // 另一进程已写 sending 占位、正在送。
        std::fs::write(&lock, format!("{other} {}", now_ms())).unwrap();
        write_record(
            &db,
            &key,
            &DeliveryRecord {
                state: "sending".into(),
                reason: None,
                at_ms: now_ms(),
                submission: None,
                pid: Some(other),
            },
        );
        let t = target(&db, &task, "n", "Q");
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            panic!("另一进程正在送，不该再送")
        })
        .unwrap();
        let st = status(&db, 7, "c".into(), &task, "Q", "n");
        assert_eq!((st.state.as_str(), st.can_retry), ("sending", false));
        // 另一进程正在送时，面板重试也不能清掉它的记录。
        clear_failure(&db, &task, "Q", "n");
        assert_eq!(read_record(&db, &key).unwrap().state, "sending");

        // 持有者已死：占位无主；但记录是 sending（送到一半）→ 按可能已送达，不自动重投。
        std::fs::write(&lock, format!("4000000000 {}", now_ms())).unwrap();
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            panic!("sending 记录不该自动重投")
        })
        .unwrap();
        assert!(!lock.exists(), "用完的占位文件要删掉");
        let st = status(&db, 7, "c".into(), &task, "Q", "n");
        assert_eq!(st.state, "uncertain");

        // 占位过期（持有 pid 可能已被复用）：接手发送。
        let _ = std::fs::remove_file(record_path(&db, &key));
        std::fs::write(&lock, format!("{other} {}", now_ms() - CLAIM_STALE_MS - 1)).unwrap();
        deliver_with(&t, |_: &NativeTarget<'_>, _: &str| -> SendResult {
            Ok(("accepted", None))
        })
        .unwrap();
        assert_eq!(read_record(&db, &key).unwrap().state, "accepted");
        assert!(!lock.exists());

        // create_new 的原子性：8 个「进程」同时抢同一个键，只有一个拿到；其余都认出对方在送。
        let key2 = delivery_key(&task, "Q2", "n");
        let winners = Mutex::new(Vec::new());
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    barrier.wait();
                    // alive 恒真：把同进程的其他线程也当成活着的外部进程。
                    if let Some(claim) = try_claim(&db, &key2, |_| true) {
                        winners.lock().unwrap().push(claim);
                    }
                });
            }
        });
        let held = winners.into_inner().unwrap();
        assert_eq!(held.len(), 1);
        drop(held);
        assert!(!claim_path(&db, &key2).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// N6：宿主确认 TUI 已握手之前绝不 initialize。证据：记过 / hook 认领过这个服务 / TUI 进程树
    /// 连着端口且隔一秒仍在。都没有就不连（记 waiting），不再「等 5 秒就上」。
    #[test]
    fn host_waits_for_tui_handshake_evidence() {
        let state = |handshaken: bool, started: i64| ServerState {
            launch_session: 1,
            server_pid: 42,
            port: 1,
            started_at_ms: started,
            tui_pid: Some(43),
            tui_handshaken: handshaken,
        };
        let never = || -> Option<bool> { panic!("已有证据时不该再查连接") };
        let no_sleep = |_: Duration| {};
        assert!(await_handshake(
            &state(true, 0),
            None,
            10_000,
            never,
            no_sleep
        ));
        // hook 认领了这个服务（会话 pid = 服务 pid）：服务上跑过回合，只能是 TUI 发起的。
        assert!(await_handshake(
            &state(false, 0),
            Some(42),
            10_000,
            never,
            no_sleep
        ));
        // 服务才起 1 秒：先补足 5 秒宽限；TUI 连着，隔 HANDSHAKE_SETTLE 复查仍连着。
        let slept = Mutex::new(Vec::new());
        assert!(await_handshake(
            &state(false, 9_000),
            Some(7),
            10_000,
            || Some(true),
            |d| slept.lock().unwrap().push(d)
        ));
        assert_eq!(
            *slept.lock().unwrap(),
            [Duration::from_millis(4_000), HANDSHAKE_SETTLE]
        );
        // 只连通一瞬（可能还没发 initialize）、没连、查不了：一律不连。
        let mut seq = [Some(true), Some(false)].into_iter();
        assert!(!await_handshake(
            &state(false, 0),
            None,
            10_000,
            || seq.next().flatten(),
            no_sleep
        ));
        assert!(!await_handshake(
            &state(false, 0),
            None,
            10_000,
            || Some(false),
            no_sleep
        ));
        assert!(!await_handshake(
            &state(false, 0),
            None,
            10_000,
            || None,
            no_sleep
        ));
    }

    /// 连接表探测（N6 第 3 条证据）走真实系统接口：本进程连上一个本机端口，必须查得到本进程。
    #[test]
    fn tcp_table_sees_tui_connection_to_server_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let idle_port = free_port().unwrap();
        let _client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let _server_side = listener.accept().unwrap();
        let me = std::process::id();
        match tcp_client_pids(port) {
            Some(pids) => {
                assert!(pids.contains(&me), "{pids:?}");
                assert_eq!(tui_connected(port, me), Some(true));
                assert_eq!(tui_connected(idle_port, me), Some(false));
                // 连着端口的进程不在 TUI 进程树里：不算（父进程不行——本进程是它的子孙）。
                assert_eq!(tui_connected(port, 4_000_000_000), Some(false));
                // 反过来，TUI 是包装进程、真正连接的是它的子孙（npm 包装时 node → codex）：算。
                assert_eq!(tui_connected(port, foreign_live_pid()), Some(true));
            }
            // 只有没装 lsof 的 Linux 会到这里；Windows 必须查得到。
            None => {
                #[cfg(windows)]
                panic!("Windows 连接表读取失败");
            }
        }
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

    /// 旧版本写的 state.json（带 linger 时代的字段、没有 tui_handshaken）也能读，读作未确认握手。
    /// 做空 R1：退出时收掉本进程持有句柄的全部服务（进程被杀、目录删除）。
    #[test]
    fn retire_all_kills_every_owned_server() {
        let root = temp_root("retire-all");
        let dir = root.join("servers").join("s1");
        std::fs::create_dir_all(&dir).unwrap();
        // 一个会一直活着的子进程代替 app-server（不调用真实 CLI）。
        #[cfg(windows)]
        let child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
        let pid = child.id();
        LIVE.lock().unwrap().insert(dir.clone(), child);
        assert!(pid_alive(pid));
        retire_all();
        assert!(!LIVE.lock().unwrap().contains_key(&dir));
        assert!(!dir.exists(), "目录（含 token）随服务删除");
        assert!(!pid_alive(pid), "服务进程已被收掉");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 两条退出路径都要收：托盘退出（RunEvent::Exit）与「重启并更新」（插件直接 process::exit，
    /// RunEvent::Exit 不触发，只有 on_before_exit）。
    #[test]
    fn both_exit_paths_retire_codex_servers() {
        let lib = include_str!("../lib.rs");
        let before_exit = lib.find("on_before_exit(").unwrap();
        let run_exit = lib.find("tauri::RunEvent::Exit)").unwrap();
        for start in [before_exit, run_exit] {
            let window = &lib[start..(start + 1500).min(lib.len())];
            assert!(window.contains("codex_app_server::retire_all()"), "{}", &window[..80]);
        }
    }

    #[test]
    fn legacy_server_state_still_reads() {
        let legacy: ServerState = serde_json::from_str(
            r#"{"launch_session":1,"server_pid":2,"port":3,"started_at_ms":4,"tui_pid":null,"tui_exited_at_ms":null,"tui_seen_at_ms":5}"#,
        )
        .unwrap();
        assert!(!legacy.tui_handshaken);
    }

    /// L1：服务目录名会出现在 app-server 命令行里（--ws-token-file），不得含 token 片段。
    #[test]
    fn server_dir_name_is_not_derived_from_token() {
        let source = include_str!("codex_app_server.rs");
        assert!(!source.contains(concat!("&tok", "en[..")));
    }
}
