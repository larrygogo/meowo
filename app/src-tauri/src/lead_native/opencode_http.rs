//! `ServerQueue(OpencodeHttp)`：OpenCode 原生 TUI 会话的关卡答复自动续跑（任务 20261113-lead-tui，B 路）。
//!
//! 路线（research-codex-opencode.md 实测，1.19.5 本模块真机复核）：
//! - 在线 O1：OpenCode 的 TUI 本身就是内置 HTTP 服务的客户端，但只有带 `--port` 才对外监听。
//!   Meowo 在托管终端里起 TUI 时追加 `--hostname 127.0.0.1 --port <空闲端口>`，并设
//!   `OPENCODE_SERVER_PASSWORD`（不设则服务无鉴权，同机任何进程都能驱动会话）。答复用
//!   `POST /session/:id/prompt_async` 送进同一会话：空闲即开新回合，运行中 TUI 显示 QUEUED 排队，
//!   输入框里的草稿不动。**不用** `/tui/append-prompt`、`/tui/submit-prompt`——它们改的就是输入框。
//! - 离线 O2：TUI 已退出时 Meowo 起 `opencode serve`（同样带密码）投递，模型照常执行；
//!   用户这时打开会话，托管终端改用 `opencode attach <url> --session <id>` 连上这个服务，
//!   而不是再起一个普通 TUI——「普通 TUI + 另一个 serve」实测模型执行了但 TUI 看不见，
//!   且两个进程并发驱动同一会话（research 已排除）。serve 在会话空闲且没有 attach 客户端时由
//!   回收线程结束，此后再打开就回到普通 TUI（O1）。同一会话任一时刻只有一个服务在驱动。
//! - 计费：注入请求与 TUI 自发请求对上游的请求头逐字一致（research 实测）；这里只对本机服务做
//!   Basic Auth，不给模型请求加任何自定义头。agent/model/variant 取自该会话最后一条用户消息，
//!   与用户在 TUI 里接着发一句话时 TUI 自己会带的值一致（不按默认 build agent 跑，免得 plan 模式下
//!   被注入的回合拿到写权限）。
//!
//! 端口与密码按服务进程持久化在 `<数据目录>/lead-native/opencode-http.json`（Meowo 重启后仍能找回
//! 存活的 serve 并接着回收）；投递记录也在这里，保证巡检反复调用 deliver 时同一答复只送一次。

use super::{NativeOutcome, NativeTarget};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PASSWORD_ENV: &str = "OPENCODE_SERVER_PASSWORD";
const USERNAME: &str = "opencode";
/// 投递记录上限：只需覆盖「答复已送、模型尚未接手关卡」这段窗口，留够余量即可。
const MAX_DELIVERIES: usize = 200;
/// 失败后巡检自动重试的退避基数（手动重试不受限）。
const RETRY_BACKOFF_MS: i64 = 30_000;
const MAX_AUTO_ATTEMPTS: u32 = 5;

/// 注册表与投递记录的进程内互斥：手机作答（answer_and_deliver）与关卡巡检（reconcile_task）可能
/// 同时对同一关卡调用 deliver，不串行化就会各起一个 serve、各送一次答复。
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    /// 托管终端里带 `--port` 的原生 TUI（O1）。
    Tui,
    /// Meowo 起的 `opencode serve`（O2）。
    Serve,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    kind: Kind,
    pid: u32,
    port: u16,
    password: String,
    /// serve 驱动的原生会话；TUI 启动时还不知道会话 id（新建会话首条消息后才有）。
    #[serde(default)]
    native_session: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    /// 连在 serve 上的 `opencode attach` 进程（托管终端的子进程）。
    #[serde(default)]
    attach_pid: Option<u32>,
    started_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Delivery {
    /// accepted = 服务已接收（204）；failed = 送达失败可重试；unsupported = 本次做不到（面板提示手动继续）。
    state: String,
    message_id: String,
    part_id: String,
    attempts: u32,
    at_ms: i64,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    via: Option<Kind>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
struct Registry {
    #[serde(default)]
    endpoints: Vec<Endpoint>,
    #[serde(default)]
    deliveries: BTreeMap<String, Delivery>,
}

fn registry_path(db: &Path) -> PathBuf {
    db.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("lead-native")
        .join("opencode-http.json")
}

impl Registry {
    fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// 先写临时文件再改名：文件里有密码，半截写入既丢端点又可能留下可解析的残片。
    fn save(&mut self, path: &Path) -> Result<(), String> {
        while self.deliveries.len() > MAX_DELIVERIES {
            let oldest = self
                .deliveries
                .iter()
                .min_by_key(|(_, d)| d.at_ms)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(key) => self.deliveries.remove(&key),
                None => break,
            };
        }
        let parent = path.parent().ok_or("lead/native_registry_failed")?;
        std::fs::create_dir_all(parent).map_err(|_| "lead/native_registry_failed")?;
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "lead/native_registry_failed")?;
        std::fs::write(&tmp, bytes).map_err(|_| "lead/native_registry_failed")?;
        std::fs::rename(&tmp, path).map_err(|_| "lead/native_registry_failed".into())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 与 lead_conversation::delivery_id 同一口径（任务目录 + 关卡 + 原生会话），换代后的新关卡是新键。
fn delivery_key(task: &Path, gate_id: &str, native: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(task.to_string_lossy().as_bytes());
    hash.update([0]);
    hash.update(gate_id.as_bytes());
    hash.update([0]);
    hash.update(native.as_bytes());
    format!("lead-{:x}", hash.finalize())
}

/// OpenCode 的 Identifier.ascending：`<前缀>_`、(毫秒时间戳 × 0x1000 + 计数) 的低 48 位十二位十六进制、
/// 14 位 base62 三段相接。会话内消息按 id 字典序排序——托管会话用的 `msg_meowo_…` 在原生会话里会排到
/// 所有 TUI 消息之后，打乱时间线，所以这里按原生格式生成。随机段由投递键派生，同一投递重试时
/// messageID 与 part id 都不变，服务端按 id upsert，不会出现两条答复（1.19.3 实测：part id 也须稳定）。
fn ascending_id(prefix: &str, ts_ms: i64, seed: &str) -> String {
    use sha2::{Digest, Sha256};
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let now = (ts_ms.max(0) as u64).wrapping_mul(0x1000).wrapping_add(1) & 0xFFFF_FFFF_FFFF;
    let digest = Sha256::digest(format!("{prefix}\0{seed}").as_bytes());
    let tail: String = digest
        .iter()
        .take(14)
        .map(|b| BASE62[(*b as usize) % 62] as char)
        .collect();
    format!("{prefix}_{now:012x}{tail}")
}

// ---------------------------------------------------------------------------------------------
// 本机 HTTP

struct Client {
    base: String,
    auth: String,
}

impl Client {
    fn new(port: u16, password: &str) -> Self {
        use base64::Engine;
        let token =
            base64::engine::general_purpose::STANDARD.encode(format!("{USERNAME}:{password}"));
        Self {
            base: format!("http://127.0.0.1:{port}"),
            auth: format!("Basic {token}"),
        }
    }
    fn of(endpoint: &Endpoint) -> Self {
        Self::new(endpoint.port, &endpoint.password)
    }
    fn agent(timeout: Duration) -> ureq::Agent {
        // 只连 127.0.0.1：显式不走代理，用户设的 HTTP(S)_PROXY 不该截走本机控制流量。
        ureq::AgentBuilder::new()
            .timeout(timeout)
            .try_proxy_from_env(false)
            .build()
    }
    fn get(&self, path: &str, timeout: Duration) -> Result<Value, String> {
        let response = Self::agent(timeout)
            .get(&format!("{}{path}", self.base))
            .set("authorization", &self.auth)
            .call()
            .map_err(http_reason)?;
        response
            .into_json::<Value>()
            .map_err(|_| "lead/opencode_http_invalid".into())
    }
    fn post(&self, path: &str, body: &Value, timeout: Duration) -> Result<u16, String> {
        Self::agent(timeout)
            .post(&format!("{}{path}", self.base))
            .set("authorization", &self.auth)
            .send_json(body.clone())
            .map(|response| response.status())
            .map_err(http_reason)
    }
    /// 健康检查同时证明「这个端口上是我们起的、密码对得上的服务」——pid 会被系统复用，端口会被别人占，
    /// 唯有带密码的 200 能确认身份。
    fn healthy(&self, timeout: Duration) -> bool {
        self.get("/global/health", timeout)
            .is_ok_and(|v| v.get("healthy").and_then(Value::as_bool) == Some(true))
    }
}

fn http_reason(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(401, _) => "lead/opencode_http_unauthorized".into(),
        ureq::Error::Status(404, _) => "lead/native_session_missing".into(),
        ureq::Error::Status(code, _) => format!("lead/opencode_http_status_{code}"),
        ureq::Error::Transport(_) => "lead/opencode_http_unreachable".into(),
    }
}

fn query(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .map_err(|_| "lead/opencode_port_unavailable".into())
}

fn pid_alive(pid: u32) -> bool {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        crate::terminal::pid_alive_agent_quick(i64::from(pid))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = pid;
        true
    }
}

fn kill_tree(pid: u32) {
    // serve 会派生 LSP/MCP 子进程；只杀根进程会把它们留成孤儿。
    crate::proc::kill_descendants(pid);
    crate::proc::kill_pid(pid);
}

// ---------------------------------------------------------------------------------------------
// 启动：托管终端里起 opencode 时由 pty.rs 调用

/// 一次托管终端启动要追加/替换的东西。`argv` 为 Some 时整体替换（attach），否则在原 argv 后追加 `args`。
pub(crate) struct LaunchPlan {
    pub argv: Option<Vec<String>>,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    endpoint: Endpoint,
    attach_to: Option<u32>,
}

/// 从恢复 argv 里认出原生会话 id（`--session <id>`；带 `--fork` 是分叉出新会话，不算）。
fn resumed_session(argv: &[String]) -> Option<&str> {
    if argv.iter().any(|a| a == "--fork") {
        return None;
    }
    let at = argv.iter().position(|a| a == "--session" || a == "-s")?;
    argv.get(at + 1)
        .map(String::as_str)
        .filter(|id| crate::session_command::is_safe_id(id))
}

/// 这个会话此刻是否正由 Meowo 起的 serve 驱动（活着且密码对得上）。
fn live_serve(reg: &Registry, native: &str) -> Option<Endpoint> {
    reg.endpoints
        .iter()
        .find(|e| e.kind == Kind::Serve && e.native_session.as_deref() == Some(native))
        .filter(|e| Client::of(e).healthy(Duration::from_millis(1500)))
        .cloned()
}

/// 恢复守卫用：该原生会话是否由 Meowo 自己的 serve 驱动。是的话「会话仍在外部运行」的拦截不适用——
/// 那个进程就是我们的 serve，恢复会改走 attach 连上它，而不是另起一个驱动者。
pub(crate) fn serving(native: &str) -> bool {
    let _guard = LOCK.lock();
    let reg = Registry::load(&registry_path(&crate::db_path()));
    live_serve(&reg, native).is_some()
}

/// 只对声明了 OpencodeHttp 机制、且本会话启用了 Lead 的启动生效；其余原样返回 None，启动行为不变。
pub(crate) fn launch_plan(
    provider: &str,
    argv: &[String],
    env: &[(String, String)],
) -> Option<LaunchPlan> {
    use meowo_agent::lead_install::{NativeContinue, ServerProtocol};
    if super::mechanism(provider) != Some(NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp))
        || !crate::lead_runtime::enabled(provider, env)
        || argv.is_empty()
    {
        return None;
    }
    plan_for(&crate::db_path(), argv)
}

fn plan_for(db: &Path, argv: &[String]) -> Option<LaunchPlan> {
    let _guard = LOCK.lock();
    let reg = Registry::load(&registry_path(db));
    if let Some(serve) = resumed_session(argv).and_then(|native| live_serve(&reg, native)) {
        // O2 → 打开：serve 还在驱动这个会话（多半正在跑注入的回合），TUI 以客户端连上去。
        // 密码只走环境变量（attach 默认读 OPENCODE_SERVER_PASSWORD），不进可见命令行。
        let native = serve.native_session.clone()?;
        return Some(LaunchPlan {
            argv: Some(vec![
                argv[0].clone(),
                "attach".into(),
                format!("http://127.0.0.1:{}", serve.port),
                "--session".into(),
                native,
            ]),
            args: vec![],
            env: vec![(PASSWORD_ENV.into(), serve.password.clone())],
            attach_to: Some(serve.pid),
            endpoint: serve,
        });
    }
    let port = free_port().ok()?;
    let password = crate::remote::generate_token().ok()?;
    Some(LaunchPlan {
        argv: None,
        args: vec![
            "--hostname".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
        ],
        env: vec![(PASSWORD_ENV.into(), password.clone())],
        attach_to: None,
        endpoint: Endpoint {
            kind: Kind::Tui,
            pid: 0,
            port,
            password,
            native_session: None,
            cwd: None,
            attach_pid: None,
            started_ms: now_ms(),
        },
    })
}

impl LaunchPlan {
    /// spawn 成功后登记进程。失败只丢失自动续跑能力（面板退回手动继续），不影响终端本身。
    pub(crate) fn launched(self, child_pid: Option<u32>) {
        let Some(pid) = child_pid else { return };
        let _ = record_launch(&crate::db_path(), self, pid);
    }
}

fn record_launch(db: &Path, plan: LaunchPlan, pid: u32) -> Result<(), String> {
    let _guard = LOCK.lock();
    let path = registry_path(db);
    let mut reg = Registry::load(&path);
    // 顺手清掉已退出的 TUI 条目：TUI 退出时服务随之消失，留着只会让文件里的旧密码越积越多。
    reg.endpoints
        .retain(|e| e.kind != Kind::Tui || (e.pid != pid && pid_alive(e.pid)));
    match plan.attach_to {
        Some(serve) => {
            if let Some(entry) = reg
                .endpoints
                .iter_mut()
                .find(|e| e.kind == Kind::Serve && e.pid == serve)
            {
                entry.attach_pid = Some(pid);
            }
        }
        None => reg.endpoints.push(Endpoint {
            pid,
            ..plan.endpoint
        }),
    }
    reg.save(&path)
}

// ---------------------------------------------------------------------------------------------
// 投递

/// 宿主对这个会话「谁在跑」的事实，由调用方从 PTY 注册表与进程表取得（测试/探针可直接构造）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    /// 托管终端里有进程；`Some(pid)` 是它的 PTY 子进程。
    Managed(Option<u32>),
    /// 托管终端外（外部终端/别处）有活着的进程；pid 来自会话库。
    Elsewhere(u32),
    Gone,
}

pub(crate) fn deliver(
    target: &NativeTarget<'_>,
    app: Option<&tauri::AppHandle>,
) -> Result<NativeOutcome, String> {
    let presence = presence(target, app)?;
    deliver_with(target, presence)
}

fn presence(target: &NativeTarget<'_>, app: Option<&tauri::AppHandle>) -> Result<Presence, String> {
    if let Some(app) = app {
        use tauri::Manager;
        if let Some(state) = app.try_state::<crate::AppState>() {
            if state.ptys.is_managed(target.session_id) {
                return Ok(Presence::Managed(state.ptys.child_pid(target.session_id)));
            }
        }
    }
    let store = crate::open_store(target.db)?;
    let pid = store
        .session_pid(target.session_id)
        .map_err(|e| e.to_string())?
        .filter(|pid| *pid > 0)
        .map(|pid| pid as u32);
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let alive = crate::terminal::session_agent_alive(&store, target.session_id)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let alive = pid.is_some();
    Ok(match pid {
        Some(pid) if alive => Presence::Elsewhere(pid),
        _ => Presence::Gone,
    })
}

pub(crate) fn deliver_with(
    target: &NativeTarget<'_>,
    presence: Presence,
) -> Result<NativeOutcome, String> {
    let _guard = LOCK.lock();
    let path = registry_path(target.db);
    let mut reg = Registry::load(&path);
    let key = delivery_key(target.task, target.gate_id, target.native_session);
    let previous = reg.deliveries.get(&key).cloned();
    if let Some(previous) = &previous {
        match previous.state.as_str() {
            // 已被服务接收：答复在会话里了，巡检再调也不重复送。
            "accepted" => return Ok(NativeOutcome::Delivered),
            "failed"
                if !target.force
                    && (previous.attempts >= MAX_AUTO_ATTEMPTS
                        || now_ms() - previous.at_ms
                            < RETRY_BACKOFF_MS * i64::from(previous.attempts.max(1))) =>
            {
                return Ok(NativeOutcome::Delivered);
            }
            _ => {}
        }
    }
    // 首次生成后 id 随记录保存，重试沿用：时间段取首次投递时刻，排序位置不随重试后移。
    let first_ms = now_ms();
    let message_id = previous
        .as_ref()
        .map(|p| p.message_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| ascending_id("msg", first_ms, &key));
    let part_id = previous
        .as_ref()
        .map(|p| p.part_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| ascending_id("prt", first_ms, &key));
    let attempts = previous.as_ref().map_or(0, |p| p.attempts) + 1;

    let resolved = resolve_endpoint(&mut reg, target, presence);
    let record = |state: &str, reason: Option<String>, via: Option<Kind>| Delivery {
        state: state.into(),
        message_id: message_id.clone(),
        part_id: part_id.clone(),
        attempts,
        at_ms: now_ms(),
        reason,
        via,
    };
    let endpoint = match resolved {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => {
            // 进程活着但没开端口（Lead 安装前启动的 TUI、外部终端里的 opencode）：不另起服务抢会话，
            // 如实告诉面板在会话里发一句话继续。
            let entry = record("unsupported", None, None);
            reg.deliveries.insert(key, entry);
            reg.save(&path)?;
            return Ok(NativeOutcome::Unsupported);
        }
        Err(reason) => {
            let entry = record("failed", Some(reason), None);
            reg.deliveries.insert(key, entry);
            reg.save(&path)?;
            return Ok(NativeOutcome::Delivered);
        }
    };
    let result = post_prompt(&endpoint, target, &message_id, &part_id);
    let entry = match &result {
        Ok(()) => record("accepted", None, Some(endpoint.kind)),
        Err(reason) => record("failed", Some(reason.clone()), Some(endpoint.kind)),
    };
    reg.deliveries.insert(key, entry);
    reg.save(&path)?;
    if endpoint.kind == Kind::Serve {
        // 投递失败也要交给回收线程：空闲的 serve 不能一直挂着。
        spawn_reaper(target.db.to_path_buf(), endpoint.pid);
    }
    Ok(NativeOutcome::Delivered)
}

/// Ok(None) = 会话有别的驱动者且我们够不着，不能投递；Err = 应能投递但失败（可重试）。
fn resolve_endpoint(
    reg: &mut Registry,
    target: &NativeTarget<'_>,
    presence: Presence,
) -> Result<Option<Endpoint>, String> {
    // 1. Meowo 的 serve 正驱动它（含 attach 中）：唯一合法的驱动者就是它。
    if let Some(serve) = live_serve(reg, target.native_session) {
        return Ok(Some(serve));
    }
    // 2. 有 TUI 在跑：只能经它自己的服务投递。找不到它的端口就放弃，绝不再起第二个驱动者。
    let tui_pid = match presence {
        Presence::Managed(pid) => pid,
        Presence::Elsewhere(pid) => Some(pid),
        Presence::Gone => None,
    };
    if presence != Presence::Gone {
        let endpoint = tui_pid.and_then(|pid| {
            reg.endpoints
                .iter()
                .find(|e| e.kind == Kind::Tui && e.pid == pid)
                .cloned()
        });
        return match endpoint {
            Some(e) if Client::of(&e).healthy(Duration::from_secs(3)) => Ok(Some(e)),
            Some(_) => Err("lead/opencode_http_unreachable".into()),
            None => Ok(None),
        };
    }
    // 3. 没人在跑：起 serve（O2）。
    start_serve(reg, target).map(Some)
}

fn start_serve(reg: &mut Registry, target: &NativeTarget<'_>) -> Result<Endpoint, String> {
    let agent = meowo_agent::by_id(target.provider).ok_or("lead/provider_unknown")?;
    let exe = agent
        .launch_argv()
        .into_iter()
        .next()
        .ok_or("lead/opencode_cli_missing")?;
    let cwd = target
        .cwd
        .filter(|dir| Path::new(dir).is_dir())
        .ok_or("lead/session_cwd_missing")?;
    // 与这个会话原本的账号/代理/中转同一套环境：账号隔离变量决定读哪份会话库与凭据。
    let store = crate::open_store(target.db)?;
    let profile = store
        .session_profile(target.session_id)
        .map_err(|e| e.to_string())?;
    let env = crate::terminal::launch_env_for_exact_profile(agent, profile.as_deref());
    let port = free_port()?;
    let password = crate::remote::generate_token()?;
    let mut command = std::process::Command::new(&exe);
    command
        .args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .current_dir(cwd)
        .envs(env)
        .env(PASSWORD_ENV, &password)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：后台服务不弹黑框。
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "lead/opencode_serve_spawn_failed")?;
    let pid = child.id();
    let client = Client::new(port, &password);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if client.healthy(Duration::from_secs(1)) {
            break;
        }
        if child.try_wait().ok().flatten().is_some() || Instant::now() >= deadline {
            kill_tree(pid);
            let _ = child.wait();
            return Err("lead/opencode_serve_not_ready".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let endpoint = Endpoint {
        kind: Kind::Serve,
        pid,
        port,
        password,
        native_session: Some(target.native_session.to_owned()),
        cwd: Some(cwd.to_owned()),
        attach_pid: None,
        started_ms: now_ms(),
    };
    reg.endpoints.push(endpoint.clone());
    // 先落盘再投递：投递途中 Meowo 退出，重启后仍能按文件找回并回收这个 serve。
    reg.save(&registry_path(target.db))?;
    // Child 不在这里 wait：交给回收线程按「空闲 + 无 attach」结束；std 的 Child drop 不杀进程。
    drop(child);
    Ok(endpoint)
}

/// 最后一条用户消息里的 agent / model / variant——用户在 TUI 里接着发话时 TUI 会带的就是这些。
fn last_user_choice(messages: &Value) -> Value {
    let mut choice = json!({});
    let Some(last) = messages.as_array().and_then(|items| {
        items
            .iter()
            .rev()
            .find(|m| m["info"]["role"].as_str() == Some("user"))
    }) else {
        return choice;
    };
    let info = &last["info"];
    if let Some(agent) = info["agent"].as_str().filter(|a| !a.is_empty()) {
        choice["agent"] = json!(agent);
    }
    let model = &info["model"];
    if model["providerID"].is_string() && model["modelID"].is_string() {
        choice["model"] = json!({"providerID": model["providerID"], "modelID": model["modelID"]});
    }
    if let Some(variant) = info["variant"].as_str().filter(|v| !v.is_empty()) {
        choice["variant"] = json!(variant);
    }
    choice
}

fn prompt_body(message: &str, message_id: &str, part_id: &str, choice: &Value) -> Value {
    let mut body = json!({
        "messageID": message_id,
        "parts": [{"id": part_id, "type": "text", "text": message}],
    });
    for key in ["agent", "model", "variant"] {
        if let Some(value) = choice.get(key) {
            body[key] = value.clone();
        }
    }
    body
}

fn post_prompt(
    endpoint: &Endpoint,
    target: &NativeTarget<'_>,
    message_id: &str,
    part_id: &str,
) -> Result<(), String> {
    let client = Client::of(endpoint);
    let native = target.native_session;
    if !crate::session_command::is_safe_id(native) {
        return Err("lead/owner_invalid".into());
    }
    // 会话所在目录决定服务端用哪个项目实例；TUI 自己的请求也带这个目录。先确认会话存在。
    let first = target.cwd.map(query);
    let scoped = |path: String, directory: &Option<String>| match directory {
        Some(dir) if path.contains('?') => format!("{path}&directory={dir}"),
        Some(dir) => format!("{path}?directory={dir}"),
        None => path,
    };
    let info = client.get(
        &scoped(format!("/session/{native}"), &first),
        Duration::from_secs(5),
    )?;
    if info["id"].as_str() != Some(native) {
        return Err("lead/native_session_missing".into());
    }
    let directory = info["directory"].as_str().map(query).or(first);
    let messages = client
        .get(
            &scoped(format!("/session/{native}/message?limit=50"), &directory),
            Duration::from_secs(10),
        )
        .unwrap_or(Value::Null);
    let body = prompt_body(
        target.message,
        message_id,
        part_id,
        &last_user_choice(&messages),
    );
    let status = client.post(
        &scoped(format!("/session/{native}/prompt_async"), &directory),
        &body,
        Duration::from_secs(15),
    )?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("lead/opencode_http_status_{status}"))
    }
}

// ---------------------------------------------------------------------------------------------
// serve 回收

static REAPING: Mutex<Vec<u32>> = Mutex::new(Vec::new());
const REAP_IDLE: Duration = Duration::from_secs(6);

/// 会话空闲且没有 attach 客户端时结束 serve。等审批/提问时会话不是空闲态，serve 留着等用户打开作答——
/// 不替用户答审批。
fn spawn_reaper(db: PathBuf, pid: u32) {
    // 单元测试里的 serve 是替身，pid 是编的；真回收会去结束一个无关进程。探针测真进程时显式调 reap。
    if cfg!(test) {
        return;
    }
    {
        let Ok(mut reaping) = REAPING.lock() else {
            return;
        };
        if reaping.contains(&pid) {
            return;
        }
        reaping.push(pid);
    }
    let spawned = std::thread::Builder::new()
        .name("lead-opencode-serve".into())
        .spawn(move || {
            reap(&db, pid);
            if let Ok(mut reaping) = REAPING.lock() {
                reaping.retain(|p| *p != pid);
            }
        });
    if spawned.is_err() {
        if let Ok(mut reaping) = REAPING.lock() {
            reaping.retain(|p| *p != pid);
        }
    }
}

pub(super) fn reap(db: &Path, pid: u32) {
    let path = registry_path(db);
    let mut idle_since: Option<Instant> = None;
    // 投递返回 204 到会话进入 busy 之间有短暂空窗，先等一下再开始判空闲。
    std::thread::sleep(Duration::from_secs(2));
    loop {
        let endpoint = {
            let _guard = LOCK.lock();
            Registry::load(&path)
                .endpoints
                .into_iter()
                .find(|e| e.kind == Kind::Serve && e.pid == pid)
        };
        let Some(endpoint) = endpoint else { return };
        let client = Client::of(&endpoint);
        let busy = if !client.healthy(Duration::from_secs(2)) {
            None
        } else if endpoint.attach_pid.is_some_and(pid_alive) {
            Some(true)
        } else {
            // 状态查不到时按忙处理：宁可多留一会儿 serve，也不在回合中途把它结束。
            Some(
                client
                    .get("/session/status", Duration::from_secs(3))
                    .map(|status| {
                        endpoint
                            .native_session
                            .as_deref()
                            .is_some_and(|native| status.get(native).is_some())
                    })
                    .unwrap_or(true),
            )
        };
        match busy {
            Some(true) => idle_since = None,
            Some(false) => {
                if idle_since.get_or_insert_with(Instant::now).elapsed() >= REAP_IDLE {
                    stop_serve(&path, pid, true);
                    return;
                }
            }
            // 健康检查失败：进程已经没了（或被外力结束），只清条目。此时 pid 可能已被系统复用，
            // 绝不能再按它结束进程。
            None => {
                stop_serve(&path, pid, false);
                return;
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn stop_serve(path: &Path, pid: u32, kill: bool) {
    let _guard = LOCK.lock();
    let mut reg = Registry::load(path);
    let before = reg.endpoints.len();
    reg.endpoints
        .retain(|e| !(e.kind == Kind::Serve && e.pid == pid));
    if reg.endpoints.len() != before {
        if kill {
            kill_tree(pid);
        }
        let _ = reg.save(path);
    }
}

/// Meowo 启动时接回上次留下的 serve：活着就重新挂回收线程，死了就清条目。
pub(crate) fn adopt(db: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("lead-opencode-adopt".into())
        .spawn(move || {
            let pids: Vec<u32> = {
                let _guard = LOCK.lock();
                let path = registry_path(&db);
                let mut reg = Registry::load(&path);
                let before = reg.endpoints.len();
                reg.endpoints
                    .retain(|e| e.kind == Kind::Serve || pid_alive(e.pid));
                if reg.endpoints.len() != before {
                    let _ = reg.save(&path);
                }
                reg.endpoints
                    .iter()
                    .filter(|e| e.kind == Kind::Serve)
                    .map(|e| e.pid)
                    .collect()
            };
            for pid in pids {
                spawn_reaper(db.clone(), pid);
            }
        });
}

// ---------------------------------------------------------------------------------------------
// 面板状态

pub(crate) fn status(db: &Path, task: &Path, gate_id: &str, native: &str) -> super::NativeStatus {
    let _guard = LOCK.lock();
    let reg = Registry::load(&registry_path(db));
    let key = delivery_key(task, gate_id, native);
    match reg.deliveries.get(&key) {
        None => super::NativeStatus {
            state: "saved",
            reason: None,
            can_retry: true,
        },
        Some(d) => match d.state.as_str() {
            "accepted" => super::NativeStatus {
                state: "accepted",
                reason: None,
                can_retry: false,
            },
            "unsupported" => super::NativeStatus {
                state: "native_manual",
                reason: None,
                can_retry: true,
            },
            _ => super::NativeStatus {
                state: "failed",
                reason: d.reason.clone(),
                can_retry: true,
            },
        },
    }
}

#[cfg(test)]
mod probe;
#[cfg(test)]
mod tests;
