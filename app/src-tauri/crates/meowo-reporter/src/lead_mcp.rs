//! 原生 TUI 的 Lead 等待通道；身份只由托管 PTY broker 验证，工具参数不能选择会话。

use crate::lead;
use meowo_protocol::broker::{write_v2_handshake, BrokerRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const MAX_LINE: usize = 64 * 1024;
const MAX_FILE: usize = 64 * 1024;
const POLL: Duration = Duration::from_secs(2);
const CONTROL_POLL: Duration = Duration::from_millis(100);
const REVALIDATE: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Context {
    session_id: String,
    cwd: String,
    provider: String,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Arguments {
    task_slug: Option<String>,
    gate_id: Option<String>,
}

struct Wait {
    id: Value,
    context: Context,
    task: PathBuf,
    gate_id: String,
    deadline: Option<Instant>,
    native_thread_id: Option<String>,
    generation: u64,
}

type Auth<'a> = dyn Fn(Option<&str>, bool) -> Result<Context, String> + 'a;

fn context(thread: Option<&str>, catalog: bool) -> Result<Context, String> {
    let endpoint: SocketAddr = std::env::var("MEOWO_PTY_ENDPOINT")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|a: &SocketAddr| a.ip().is_loopback())
        .ok_or("lead/context_unbound")?;
    let token = std::env::var("MEOWO_PTY_TOKEN")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or("lead/context_unbound")?;
    let mut socket = TcpStream::connect_timeout(&endpoint, Duration::from_secs(3))
        .map_err(|_| "lead/context_unavailable")?;
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| "lead/context_unavailable")?;
    socket
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| "lead/context_unavailable")?;
    write_v2_handshake(
        &mut socket,
        &BrokerRequest::LeadContext {
            token,
            process_id: std::process::id(),
            native_thread_id: thread.map(str::to_owned),
            catalog,
        },
    )
    .map_err(|_| "lead/context_unavailable")?;
    let mut raw = Vec::new();
    socket
        .take((MAX_LINE + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| "lead/context_unavailable")?;
    if raw.len() > MAX_LINE {
        return Err("lead/context_invalid".into());
    }
    let value: Value = serde_json::from_slice(&raw).map_err(|_| "lead/context_invalid")?;
    if value.get("ok") != Some(&Value::Bool(true)) {
        // broker 错误只透传结构化 reason，不能把原始响应里的凭据写进模型上下文。
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .filter(|s| s.starts_with("lead/") && s.len() < 128)
            .unwrap_or("lead/context_denied")
            .to_owned());
    }
    let ctx: Context = serde_json::from_value(value).map_err(|_| "lead/context_invalid")?;
    if ctx.session_id.is_empty()
        || ctx.session_id.len() > 256
        || ctx.provider.is_empty()
        || !Path::new(&ctx.cwd).is_absolute()
    {
        return Err("lead/context_invalid".into());
    }
    Ok(ctx)
}

fn read_small(path: &Path, limit: usize) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= limit)
        .then(|| String::from_utf8(bytes).ok())
        .flatten()
}

fn gate(task: &Path) -> Option<meowo_protocol::ipc::LeadGateDto> {
    lead::parse_gate(&read_small(&task.join("gate.json"), MAX_FILE)?)
}

fn owned(task: &Path, lead_dir: &Path, session: &str) -> bool {
    // slug 本身挡不住目录链接；只允许项目 .lead 的直接真实子目录，不能越界读别的任务。
    task.canonicalize()
        .ok()
        .is_some_and(|p| p.parent() == Some(lead_dir))
        && read_small(&task.join(".owner"), 256).is_some_and(|s| s.trim() == session)
}

fn select_task(ctx: &Context, args: &Arguments) -> Result<(PathBuf, String), String> {
    if args
        .task_slug
        .as_ref()
        .is_some_and(|s| s.len() > 200 || !lead::slug_ok(s))
        || args
            .gate_id
            .as_ref()
            .is_some_and(|s| s.is_empty() || s.len() > 256)
    {
        return Err("lead/invalid_arguments".into());
    }
    let dir = lead::project_root(Path::new(&ctx.cwd))
        .join(".lead")
        .canonicalize()
        .map_err(|_| "lead/task_missing")?;
    let mut best: Option<(PathBuf, String, Option<std::time::SystemTime>)> = None;
    let entries = std::fs::read_dir(&dir).map_err(|_| "lead/task_missing")?;
    // 本地任务数量也设上限；意外目录洪泛时明确失败，不默默选择扫描到的一半。
    for (index, entry) in entries.enumerate() {
        if index >= 4096 {
            return Err("lead/task_limit".into());
        }
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Some(slug) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !lead::slug_ok(slug)
            || args.task_slug.as_ref().is_some_and(|s| s != slug)
            || !owned(&path, &dir, &ctx.session_id)
        {
            continue;
        }
        let Some(g) = gate(&path) else { continue };
        if args.gate_id.as_ref().is_some_and(|id| id != &g.id) {
            continue;
        }
        let modified = std::fs::metadata(path.join("progress.md"))
            .and_then(|m| m.modified())
            .ok();
        if best.as_ref().is_none_or(|b| modified > b.2) {
            best = Some((path, g.id, modified));
        }
    }
    best.map(|(path, id, _)| (path, id))
        .ok_or_else(|| "lead/owned_gate_missing".into())
}

fn result(id: Value, text: &str, error: bool) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":text}],"isError":error}})
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

#[derive(Default)]
struct Server {
    active: Option<Wait>,
    generation: u64,
}

impl Server {
    fn request(&mut self, request: Value, auth: &Auth<'_>) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if id.is_none() {
            if method == "notifications/cancelled" || method == "$/cancelRequest" {
                let target = request
                    .pointer("/params/requestId")
                    .or_else(|| request.pointer("/params/id"));
                if self.active.as_ref().is_some_and(|w| Some(&w.id) == target) {
                    let wait = self.active.take().unwrap();
                    return Some(rpc_error(wait.id, -32800, "lead/wait_cancelled"));
                }
            }
            return None;
        }
        let id = id.unwrap();
        if !(id.is_string() || id.is_number()) {
            return Some(rpc_error(Value::Null, -32600, "lead/invalid_request"));
        }
        let empty = || json!({"jsonrpc":"2.0","id":id,"result":{}});
        match method {
            "initialize" => Some(json!({"jsonrpc":"2.0","id":id,"result":{
                "protocolVersion":"2024-11-05","capabilities":{"tools":{}},
                "serverInfo":{"name":"meowo-lead","version":env!("CARGO_PKG_VERSION")}}})),
            "ping" => Some(empty()),
            "tools/list" => {
                let tools = if auth(None, true).is_ok() {
                    json!([{
                        "name":"lead_wait",
                        "description":"Wait for the leader's answer to your own Lead gate. Read the returned gate.json, record the decision, then mark it consumed. Delivery alone is not consumption. Cancellation leaves the answer on disk.",
                        "inputSchema":{"type":"object","properties":{"taskSlug":{"type":"string"},"gateId":{"type":"string"}},"additionalProperties":false}
                    }])
                } else {
                    json!([])
                };
                Some(json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools}}))
            }
            "tools/call" => {
                if let Some(wait) = &self.active {
                    // 相同 id 的重发不另开等待，也不产生两份同 id 回执；不同 id 明确报告冲突。
                    if wait.id == id {
                        return None;
                    }
                    return Some(result(id, "lead/wait_busy", true));
                }
                if request.pointer("/params/name").and_then(Value::as_str) != Some("lead_wait") {
                    return Some(result(id, "lead/unknown_tool", true));
                }
                let thread = request
                    .pointer("/params/_meta/threadId")
                    .and_then(Value::as_str);
                if thread.is_some_and(|s| s.is_empty() || s.len() > 256) {
                    return Some(result(id, "lead/context_invalid", true));
                }
                let ctx = match auth(thread, false) {
                    Ok(c) => c,
                    Err(e) => return Some(result(id, &e, true)),
                };
                let args = request
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let args: Arguments = match serde_json::from_value(args) {
                    Ok(a) => a,
                    Err(_) => return Some(result(id, "lead/invalid_arguments", true)),
                };
                match select_task(&ctx, &args) {
                    Ok((task, gate_id)) => {
                        self.generation = self.generation.wrapping_add(1);
                        self.active = Some(Wait {
                            id,
                            context: ctx,
                            task,
                            gate_id,
                            deadline: None,
                            native_thread_id: thread.map(str::to_owned),
                            generation: self.generation,
                        });
                        self.tick()
                    }
                    Err(e) => Some(result(id, &e, true)),
                }
            }
            _ => Some(rpc_error(id, -32601, "lead/method_not_found")),
        }
    }

    fn tick(&mut self) -> Option<Value> {
        let wait = self.active.as_ref()?;
        let dir = lead::project_root(Path::new(&wait.context.cwd))
            .join(".lead")
            .canonicalize()
            .ok();
        let outcome = if wait.deadline.is_some_and(|d| Instant::now() >= d) {
            Some(("lead/wait_timeout".to_owned(), true))
        } else if !dir
            .as_ref()
            .is_some_and(|d| owned(&wait.task, d, &wait.context.session_id))
        {
            Some(("lead/task_owner_changed".to_owned(), true))
        } else {
            match gate(&wait.task) {
                None => Some(("lead/gate_missing".to_owned(), true)),
                Some(g) if g.id != wait.gate_id => Some(("lead/gate_changed".to_owned(), true)),
                Some(g) if g.status == "consumed" => Some(("lead/gate_consumed".to_owned(), false)),
                Some(g) if g.status == "answered" && g.answer.is_some() => {
                    Some((lead::answer_message(&wait.task, &g), false))
                }
                Some(_) => None,
            }
        };
        outcome.map(|(text, error)| result(self.active.take().unwrap().id, &text, error))
    }

    fn validated(&mut self, generation: u64, checked: Result<Context, String>) -> Option<Value> {
        let wait = self.active.as_ref()?;
        if wait.generation != generation {
            return None;
        }
        let error = match checked {
            Ok(ctx)
                if ctx.session_id == wait.context.session_id
                    && ctx.cwd == wait.context.cwd
                    && ctx.provider == wait.context.provider =>
            {
                return None
            }
            Ok(_) => "lead/context_changed".to_owned(),
            Err(error) => error,
        };
        Some(result(self.active.take().unwrap().id, &error, true))
    }
}

enum Input {
    Request(Value),
    Invalid,
    Oversized,
    Eof,
}

fn read_input(mut input: impl BufRead, send: mpsc::SyncSender<Input>) {
    loop {
        // take 限制单行分配；超长行直接断线，不在攻击者无限输入上持续排空。
        let mut line = String::new();
        match input
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_line(&mut line)
        {
            Ok(0) | Err(_) => {
                let _ = send.send(Input::Eof);
                return;
            }
            Ok(_) if line.len() > MAX_LINE => {
                let _ = send.send(Input::Oversized);
                return;
            }
            Ok(_) => {
                if line.trim().is_empty() {
                    continue;
                }
                let message = serde_json::from_str(&line)
                    .map(Input::Request)
                    .unwrap_or(Input::Invalid);
                if send.send(message).is_err() {
                    return;
                }
            }
        }
    }
}

fn serve(input: Receiver<Input>, mut output: impl Write, auth: &Auth<'_>) -> std::io::Result<()> {
    let mut server = Server::default();
    let (validated_send, validated_receive) = mpsc::sync_channel(1);
    let mut validating = false;
    let mut next_validation = Instant::now() + REVALIDATE;
    let mut next_gate = Instant::now() + POLL;
    loop {
        let response = match input.recv_timeout(CONTROL_POLL) {
            Ok(Input::Request(request)) => server.request(request, auth),
            Ok(Input::Invalid) => Some(rpc_error(Value::Null, -32700, "lead/invalid_json")),
            Ok(Input::Oversized) => {
                writeln!(
                    output,
                    "{}",
                    rpc_error(Value::Null, -32600, "lead/request_too_large")
                )?;
                output.flush()?;
                return Ok(());
            }
            Ok(Input::Eof) | Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => None,
        };
        let validated = validated_receive
            .try_recv()
            .ok()
            .and_then(|(generation, checked)| {
                validating = false;
                next_validation = Instant::now() + REVALIDATE;
                server.validated(generation, checked)
            });
        // broker 校验有有限网络等待，放独立线程；等待期间 ping/cancel 仍由主循环立即处理。
        if !validating && Instant::now() >= next_validation {
            if let Some(wait) = &server.active {
                validating = true;
                let sender = validated_send.clone();
                let generation = wait.generation;
                let native_thread_id = wait.native_thread_id.clone();
                std::thread::spawn(move || {
                    let _ = sender.send((generation, context(native_thread_id.as_deref(), false)));
                });
            } else {
                next_validation = Instant::now() + REVALIDATE;
            }
        }
        let gate = if Instant::now() >= next_gate {
            next_gate = Instant::now() + POLL;
            server.tick()
        } else {
            None
        };
        for response in response.into_iter().chain(validated).chain(gate) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
}

/// 单独命令不接受 session 参数；EOF/管道断开立刻结束，磁盘答案从不因交付被改写。
pub fn run() {
    let (send, receive) = mpsc::sync_channel(16);
    std::thread::spawn(move || read_input(std::io::stdin().lock(), send));
    let _ = serve(receive, std::io::stdout().lock(), &context);
}

#[cfg(test)]
mod tests;
