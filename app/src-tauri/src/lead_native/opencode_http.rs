//! `ServerQueue(OpencodeHttp)`：OpenCode 原生 TUI 会话的关卡答复自动续跑（任务 20261113-lead-tui，B 路）。
//!
//! 路线（research-codex-opencode.md 实测，1.19.5 本模块真机复核）：
//! - 在线 O1：OpenCode 的 TUI 本身就是内置 HTTP 服务的客户端，但只有带 `--port` 才对外监听。
//!   Meowo 在托管终端里起 TUI 时追加 `--hostname 127.0.0.1 --port <空闲端口>`，并设
//!   `OPENCODE_SERVER_PASSWORD`（不设则服务无鉴权，同机任何进程都能驱动会话）。答复用
//!   `POST /session/:id/prompt_async` 送进同一会话：空闲即开新回合，运行中 TUI 显示 QUEUED 排队，
//!   输入框里的草稿不动。**不用** `/tui/append-prompt`、`/tui/submit-prompt`——它们改的就是输入框。
//! - TUI 不在时：不投递、不另起服务（leader 2026-11-13「不需要离线」：原先起 `opencode serve` 投递、
//!   打开会话时 `attach` 上去，已删）。答复只保存、记为「等会话打开」；用户在 Meowo 里重新打开会话时
//!   TUI 照常带端口启动，关卡巡检（每 10s）看到它在就经上面的 prompt_async 送达。
//! - 计费：注入请求与 TUI 自发请求对上游的请求头逐字一致（research 实测）；这里只对本机服务做
//!   Basic Auth，不给模型请求加任何自定义头。agent/model/variant 取自该会话最后一条用户消息，
//!   与用户在 TUI 里接着发一句话时 TUI 自己会带的值一致（不按默认 build agent 跑，免得 plan 模式下
//!   被注入的回合拿到写权限）。
//!
//! - 密码泄漏面（L2，未修）：1.19.5 服务端密码只从 `OPENCODE_SERVER_PASSWORD` 环境变量读
//!   （内嵌源码 `@opencode/ServerAuthConfig`：password 取自同名环境变量，没有文件或其他不继承的
//!   途径；`--password` 只是 attach/run 客户端参数，且会进可见命令行）。TUI 的 bash 工具与 `!`
//!   命令以 `{...process.env, ...shell.env 插件输出}` 起子进程，所以模型跑的 shell 会继承这个密码，
//!   `env`/`echo` 之类可能把它打进 transcript。影响限于本机：服务只听 127.0.0.1、密码每次启动
//!   重新生成，能读到它的进程本来就以用户身份跑在同一个模型会话里。可能的缓解是在 Lead 插件里
//!   加 `shell.env` hook 把它置空，未经真机验证不做。
//!
//! - 端口身份（做空 L4）：free_port 先绑再放，TUI 随后才绑同一个端口，中间可能被别的进程抢走——
//!   这时 TUI 起不来服务，连上的却是别人的服务。投递前核对身份：带本次密码必须 200、带诱饵密码必须
//!   401，两条都成立才认是自己的 TUI（与 Codex 用诱饵 token 核身份同一思路）。
//!
//! 端口与密码按 TUI 进程持久化在 `<数据目录>/lead-native/opencode-http.json`（Meowo 重启后仍能找回
//! 还开着的 TUI）；投递记录也在这里，保证巡检反复调用 deliver 时同一答复只送一次。

use super::{NativeOutcome, NativeTarget};
use meowo_agent::lead_install::{NativeContinue, ServerAuth, ServerProtocol};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// 沿 parentID 上溯的层数上限：子代理再派子代理也远到不了这么深，超过视为数据异常（含环）。
const MAX_PARENT_DEPTH: usize = 16;
/// 投递记录上限：只需覆盖「答复已送、模型尚未接手关卡」这段窗口，留够余量即可。
const MAX_DELIVERIES: usize = 200;
/// 失败后巡检自动重试的退避基数（手动重试不受限）。
const RETRY_BACKOFF_MS: i64 = 30_000;
const MAX_AUTO_ATTEMPTS: u32 = 5;
/// TUI 刚启动、内置服务还没开始监听的宽限期：这段时间连不上不算投递失败（不计次、不退避），
/// 下一次巡检再试——否则用户刚打开会话那几秒的巡检会把答复推进 30s 起步的退避。
const TUI_STARTUP_GRACE_MS: i64 = 30_000;

/// 注册表与投递记录的进程内互斥：手机作答（answer_and_deliver）与关卡巡检（reconcile_task）可能
/// 同时对同一关卡调用 deliver，不串行化就会各送一次答复。
static LOCK: Mutex<()> = Mutex::new(());

/// 本机服务鉴权取自声明了 OpencodeHttp 机制的插件（meowo-agent 的 LeadCap::server_auth），
/// 宿主共享代码不写 provider 身份串（lib.rs HOST_SOURCES 守卫）。
fn server_auth() -> Option<ServerAuth> {
    meowo_agent::all()
        .iter()
        .filter_map(|agent| agent.lead())
        .find(|lead| {
            lead.native_continue()
                == Some(NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp))
        })
        .and_then(|lead| lead.server_auth())
}

/// 托管终端里带 `--port` 的原生 TUI。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Endpoint {
    pid: u32,
    port: u16,
    password: String,
    started_ms: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Delivery {
    /// accepted = 服务已接收（204）；failed = 送达失败可重试；unsupported = 本次做不到（面板提示手动继续）；
    /// awaiting = TUI 不在，等会话打开后由巡检送达（不计投递次数）。
    state: String,
    message_id: String,
    part_id: String,
    attempts: u32,
    at_ms: i64,
    #[serde(default)]
    reason: Option<String>,
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
        Self {
            base: format!("http://127.0.0.1:{port}"),
            auth: Self::basic(password),
        }
    }
    fn basic(password: &str) -> String {
        use base64::Engine;
        // 取不到鉴权声明（插件被移除）时用户名留空：服务回 401，按投递失败处理，不会误连。
        let username = server_auth().map_or("", |auth| auth.username);
        let token =
            base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
        format!("Basic {token}")
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
    /// 核对这个端口上是不是我们起的 TUI（做空 L4）：pid 会被系统复用，端口会被别人抢。带本次密码
    /// 必须健康 200，带诱饵密码必须 401——不设鉴权、来者不拒的服务（或别人的服务）过不了第二条。
    fn verify_identity(&self, timeout: Duration) -> Result<(), String> {
        let ok = self.get("/global/health", timeout)?;
        if ok.get("healthy").and_then(Value::as_bool) != Some(true) {
            return Err("lead/opencode_http_identity".into());
        }
        let decoy = crate::remote::generate_token()?;
        let other = Client {
            base: self.base.clone(),
            auth: Client::basic(&decoy),
        };
        match other.get("/global/health", timeout) {
            Err(reason) if reason == "lead/opencode_http_unauthorized" => Ok(()),
            Err(reason) if reason == "lead/opencode_http_unreachable" => Err(reason),
            _ => Err("lead/opencode_http_identity".into()),
        }
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

// ---------------------------------------------------------------------------------------------
// 启动：托管终端里起 opencode 时由 pty.rs 调用

/// 一次托管终端启动要在原 argv 后追加的参数与环境。
pub(crate) struct LaunchPlan {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    endpoint: Endpoint,
    db: PathBuf,
}

/// 只对声明了 OpencodeHttp 机制、且本会话启用了 Lead 的启动生效；其余原样返回 None，启动行为不变。
pub(crate) fn launch_plan(
    provider: &str,
    argv: &[String],
    env: &[(String, String)],
) -> Option<LaunchPlan> {
    if super::mechanism(provider) != Some(NativeContinue::ServerQueue(ServerProtocol::OpencodeHttp))
        || !crate::lead_runtime::enabled(provider, env)
        || argv.is_empty()
    {
        return None;
    }
    plan_for(&crate::db_path())
}

fn plan_for(db: &Path) -> Option<LaunchPlan> {
    let password_env = server_auth()?.password_env;
    let port = free_port().ok()?;
    let password = crate::remote::generate_token().ok()?;
    Some(LaunchPlan {
        args: vec![
            "--hostname".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
        ],
        env: vec![(password_env.into(), password.clone())],
        db: db.to_path_buf(),
        endpoint: Endpoint {
            pid: 0,
            port,
            password,
            started_ms: now_ms(),
        },
    })
}

impl LaunchPlan {
    /// spawn 成功后登记进程。失败只丢失自动续跑能力（面板退回手动继续），不影响终端本身。
    pub(crate) fn launched(self, child_pid: Option<u32>) {
        let Some(pid) = child_pid else { return };
        let _ = record_launch(&self, pid);
    }
}

fn record_launch(plan: &LaunchPlan, pid: u32) -> Result<(), String> {
    let _guard = LOCK.lock();
    let path = registry_path(&plan.db);
    let mut reg = Registry::load(&path);
    // 顺手清掉已退出的 TUI 条目：TUI 退出时服务随之消失，留着只会让文件里的旧密码越积越多。
    reg.endpoints.retain(|e| e.pid != pid && pid_alive(e.pid));
    reg.endpoints.push(Endpoint {
        pid,
        started_ms: now_ms(),
        ..plan.endpoint.clone()
    });
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

    let record = |state: &str, reason: Option<String>, attempts: u32| Delivery {
        state: state.into(),
        message_id: message_id.clone(),
        part_id: part_id.clone(),
        attempts,
        at_ms: now_ms(),
        reason,
    };
    let endpoint = match resolve_endpoint(&reg, presence) {
        Route::Tui(endpoint) => endpoint,
        Route::NoPort => {
            // 进程活着但没开端口（Lead 安装前启动的 TUI、外部终端里的 opencode）：不另起服务抢会话，
            // 如实告诉面板在会话里发一句话继续。
            reg.deliveries
                .insert(key, record("unsupported", None, attempts));
            reg.save(&path)?;
            return Ok(NativeOutcome::Unsupported);
        }
        Route::Awaiting => {
            // TUI 不在（或刚起、服务还没监听）：只保存，不计次，等会话打开后的巡检送达。
            reg.deliveries
                .insert(key, record("awaiting", None, attempts - 1));
            reg.save(&path)?;
            return Ok(NativeOutcome::Saved);
        }
        Route::Failed(reason) => {
            reg.deliveries
                .insert(key, record("failed", Some(reason), attempts));
            reg.save(&path)?;
            return Ok(NativeOutcome::Delivered);
        }
    };
    let entry = match post_prompt(&endpoint, target, &message_id, &part_id) {
        Ok(_) => record("accepted", None, attempts),
        Err(reason) => record("failed", Some(reason), attempts),
    };
    reg.deliveries.insert(key, entry);
    reg.save(&path)?;
    Ok(NativeOutcome::Delivered)
}

/// 这一刻投递该走哪条路。
#[derive(Debug)]
enum Route {
    /// 托管终端里带端口的 TUI，已核过身份。
    Tui(Endpoint),
    /// 会话有进程在跑，但不是 Meowo 带端口起的：够不着，不能投递。
    NoPort,
    /// TUI 不在（或刚起还没监听）：等会话打开。
    Awaiting,
    /// 应能投递但失败（可重试）。
    Failed(String),
}

fn resolve_endpoint(reg: &Registry, presence: Presence) -> Route {
    let pid = match presence {
        Presence::Managed(pid) => pid,
        Presence::Elsewhere(pid) => Some(pid),
        Presence::Gone => return Route::Awaiting,
    };
    let Some(endpoint) = pid.and_then(|pid| reg.endpoints.iter().find(|e| e.pid == pid).cloned())
    else {
        return Route::NoPort;
    };
    match Client::of(&endpoint).verify_identity(Duration::from_secs(3)) {
        Ok(()) => Route::Tui(endpoint),
        Err(reason)
            if reason == "lead/opencode_http_unreachable"
                && now_ms() - endpoint.started_ms < TUI_STARTUP_GRACE_MS =>
        {
            Route::Awaiting
        }
        Err(reason) => Route::Failed(reason),
    }
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

fn scoped(path: String, directory: &Option<String>) -> String {
    match directory {
        Some(dir) if path.contains('?') => format!("{path}&directory={dir}"),
        Some(dir) => format!("{path}?directory={dir}"),
        None => path,
    }
}

/// 沿 parentID 找到主会话（H3）。.owner 由 Lead 插件在 `tool.execute.after` 里按工具调用所在会话
/// 记录——子代理（task 工具派出的子会话，1.19.5 Session schema 的 parentID 非空）改了 .lead 时
/// 记下的就是子会话。答复若投进子会话，TUI 主视图看不到、主 agent 也不会接着干；所以一律投给
/// 最上层的主会话。父链断了（父会话被删）或成环就失败并说明，不猜。返回（主会话 id，主会话 info）。
fn root_session(
    client: &Client,
    native: &str,
    directory: &Option<String>,
) -> Result<(String, Value), String> {
    let mut current = native.to_owned();
    for _ in 0..MAX_PARENT_DEPTH {
        if !crate::session_command::is_safe_id(&current) {
            return Err("lead/opencode_parent_unresolved".into());
        }
        let info = client
            .get(
                &scoped(format!("/session/{current}"), directory),
                Duration::from_secs(5),
            )
            .map_err(|reason| {
                // 子会话在而父会话不在：不是「会话不存在」，是父链断了。
                if current != native && reason == "lead/native_session_missing" {
                    "lead/opencode_parent_unresolved".to_owned()
                } else {
                    reason
                }
            })?;
        if info["id"].as_str() != Some(current.as_str()) {
            return Err("lead/native_session_missing".into());
        }
        match info["parentID"].as_str().filter(|p| !p.is_empty()) {
            Some(parent) => current = parent.to_owned(),
            None => return Ok((current, info)),
        }
    }
    Err("lead/opencode_parent_unresolved".into())
}

/// 投递答复，返回实际投到的主会话 id。
///
/// 只用三个端点：`GET /session/:id`（含父链）、`GET /session/:id/message`、
/// `POST /session/:id/prompt_async`。按 1.19.5 内嵌源码核对过的两条不变量（单测钉住只用这三个）：
/// - 不碰输入框：改 TUI 输入框的是 `/tui/append-prompt`、`/tui/submit-prompt`；prompt_async 走
///   SessionPrompt.prompt → 建用户消息 → `SessionRunState.ensureRunning`，与 TUI 渲染层无关，草稿不动。
/// - 不答审批：待批的 permission 只有 `Permission.reply`（`/permission/:id/reply`、
///   `/session/:id/permissions/:pid`）或实例销毁会结束，SessionPrompt.prompt 不调用它。会话卡在
///   审批时 runner 仍在跑，`ensureRunning` 只是把新消息排在后面（TUI 显示 QUEUED），等用户自己
///   批完才轮到它。所以投递前不必查 permission 列表。
fn post_prompt(
    endpoint: &Endpoint,
    target: &NativeTarget<'_>,
    message_id: &str,
    part_id: &str,
) -> Result<String, String> {
    let client = Client::of(endpoint);
    if !crate::session_command::is_safe_id(target.native_session) {
        return Err("lead/owner_invalid".into());
    }
    // 会话所在目录决定服务端用哪个项目实例；TUI 自己的请求也带这个目录。先确认会话存在。
    let first = target.cwd.map(query);
    let (native, info) = root_session(&client, target.native_session, &first)?;
    let native = native.as_str();
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
        Ok(native.to_owned())
    } else {
        Err(format!("lead/opencode_http_status_{status}"))
    }
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
            // TUI 不在：打开会话后巡检自动送达，没有要人点的重试。
            "awaiting" => super::NativeStatus {
                state: "native_on_open",
                reason: None,
                can_retry: false,
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
