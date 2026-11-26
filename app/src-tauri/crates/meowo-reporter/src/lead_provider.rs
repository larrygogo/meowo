//! 原生 hook 的身份/工具/输出适配。资料：Codex 官方 /docs/hooks；Kimi Code hooks；
//! Antigravity CLI 内置 hooks 文档与真机探针（负载先经插件的 normalize_hook_payload 改成规范形状）；
//! OpenCode v1.19.3 packages/plugin/src/index.ts。
//! MCP 工具参数不能提供会话身份；未知事件、工具、失败形状均放行且不认领。

use crate::lead::{self, Output, TurnEvidence};
use crate::lead_waiter;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    Codex,
    Kimi,
    Antigravity,
    OpenCode,
}
impl Provider {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "codex" => Some(Self::Codex),
            "kimi" => Some(Self::Kimi),
            "antigravity" => Some(Self::Antigravity),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Kimi => "kimi",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
        }
    }
}

struct Native {
    data: Value,
    event: String,
    prompt: Option<String>,
    turn: Option<String>,
}

fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|text| !text.is_empty())
}
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(
            parts
                .iter()
                .filter_map(|part| string(part, "text"))
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => None,
    }
}

fn normalize(provider: Provider, raw: &Value) -> Option<Native> {
    if provider == Provider::Kimi
        && raw.get("client_type").and_then(Value::as_str) != Some("kimi_code_cli")
    {
        return None;
    }
    let event = string(raw, "hook_event_name")?;
    let (session, cwd, prompt) = if provider == Provider::OpenCode {
        match event {
            "tool.execute.after" | "experimental.chat.system.transform" => (
                string(raw.get("input")?, "sessionID")?,
                string(raw, "cwd")?,
                None,
            ),
            "chat.message" => (
                string(raw.get("input")?, "sessionID")?,
                string(raw, "cwd")?,
                raw.pointer("/output/parts").and_then(text),
            ),
            "session.created" => {
                if raw
                    .pointer("/event/properties/info/parentID")
                    .is_some_and(|parent| !parent.is_null())
                {
                    return None;
                }
                (
                    raw.pointer("/event/properties/info/id")?.as_str()?,
                    raw.pointer("/event/properties/info/directory")?.as_str()?,
                    None,
                )
            }
            _ => return None,
        }
    } else {
        let supported = match provider {
            Provider::Codex => matches!(
                event,
                "SessionStart" | "UserPromptSubmit" | "PostToolUse" | "Stop"
            ),
            Provider::Kimi => matches!(
                event,
                "SessionStart"
                    | "UserPromptSubmit"
                    | "PreToolUse"
                    | "PostToolUse"
                    | "Stop"
                    | "PostCompact"
            ),
            // 规范名：PreInvocation（每回合第一次）已改写成 UserPromptSubmit，见 plugins::antigravity::hook。
            Provider::Antigravity => matches!(
                event,
                "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop"
            ),
            Provider::OpenCode => false,
        };
        if !supported
            || raw.get("agent_id").is_some()
            || raw
                .get("agentId")
                .and_then(Value::as_str)
                .is_some_and(|id| id != "main")
        {
            return None;
        }
        (
            string(raw, "session_id")?,
            string(raw, "cwd")?,
            raw.get("prompt").and_then(text),
        )
    };
    if session.is_empty()
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        || cwd.is_empty()
    {
        return None;
    }
    Some(Native {
        data: json!({"session_id":session,"cwd":cwd,"source":raw.get("source"),"transcript_path":raw.get("transcript_path"),"stop_hook_active":raw.get("stop_hook_active").and_then(Value::as_bool).unwrap_or(false)}),
        event: event.into(),
        prompt,
        turn: raw
            .get("turn_id")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn failed(response: &Value) -> bool {
    response.get("isError").and_then(Value::as_bool) == Some(true)
        || response.get("is_error").and_then(Value::as_bool) == Some(true)
        || response
            .get("error")
            .is_some_and(|error| !error.is_null() && error != &Value::Bool(false))
        || ["exit_code", "exitCode"].into_iter().any(|key| {
            response
                .get(key)
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
        })
        || response
            .get("data")
            .filter(|data| data.is_object())
            .is_some_and(failed)
}

fn write_call(provider: Provider, native: &Native, raw: &Value) -> Option<Value> {
    let expected = match provider {
        Provider::Codex | Provider::Kimi | Provider::Antigravity => "PostToolUse",
        Provider::OpenCode => "tool.execute.after",
    };
    if native.event != expected {
        return None;
    }
    let (name, input, response) = if provider == Provider::OpenCode {
        (
            string(raw.get("input")?, "tool")?,
            raw.pointer("/input/args")?,
            raw.get("output")?,
        )
    } else {
        (
            string(raw, "tool_name")?,
            raw.get("tool_input")?,
            raw.get("tool_response").unwrap_or(&Value::Null),
        )
    };
    if provider == Provider::OpenCode && response.get("output").and_then(Value::as_str).is_none() {
        return None;
    }
    if failed(response) {
        return None;
    }
    let (canonical, args) = match (provider, name) {
        (Provider::Codex, "apply_patch") => {
            // Codex Bash 的 PostToolUse 也覆盖非零退出；补丁只接受明确成功的原生输出。
            let success = response
                .as_str()
                .is_some_and(|output| output.starts_with("Success. Updated the following files:"));
            if !success {
                return None;
            }
            let patch = string(input, "command")?;
            let path = patch
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("*** Add File: ")
                        .or_else(|| line.strip_prefix("*** Update File: "))
                })
                .find(|path| path.replace('\\', "/").contains(".lead/"))?;
            ("Write", json!({"file_path":path}))
        }
        (Provider::Kimi, "Write" | "Edit") => (
            name,
            json!({"file_path":string(input,"path").or_else(|| string(input,"file_path"))?}),
        ),
        // agy 的参数是 PascalCase（真机库：write_file 的 Path / Content）。
        (Provider::Antigravity, "write_file" | "edit_file") => (
            "Write",
            json!({"file_path":string(input,"Path").or_else(|| string(input,"TargetFile"))?}),
        ),
        (Provider::OpenCode, "write" | "edit") => {
            ("Write", json!({"file_path":string(input,"filePath")?}))
        }
        (Provider::Antigravity, "run_command") => {
            ("Bash", json!({"command":string(input,"CommandLine")?}))
        }
        (Provider::Kimi, "Bash")
        | (Provider::OpenCode, "bash")
        | (Provider::Codex, "Bash") => {
            if provider == Provider::Codex
                && !["exit_code", "exitCode"]
                    .into_iter()
                    .any(|key| response.get(key).and_then(Value::as_i64) == Some(0))
            {
                return None;
            }
            if provider == Provider::OpenCode
                && response.pointer("/metadata/exit").and_then(Value::as_i64) != Some(0)
            {
                return None;
            }
            ("Bash", json!({"command":string(input,"command")?}))
        }
        _ => return None,
    };
    Some(json!({"name":canonical,"input":args}))
}

#[derive(Default, Serialize, Deserialize)]
struct TurnState {
    session: String,
    turn: Option<String>,
    user: String,
    calls: Vec<Value>,
    blocked: bool,
    /// 本回合已经为哪个关卡要求过启动等待器（agy 的 Stop continue 不限次数，续跑那轮的
    /// continuationActive 也不可靠——真机探针仍是 false——只问一次）。
    #[serde(default)]
    waiter_asked: Option<String>,
}
fn state_path(task: &Path, provider: Provider) -> PathBuf {
    task.join(format!(".provider-turn-{}.json", provider.id()))
}
fn read_state(task: &Path, provider: Provider, session: &str) -> TurnState {
    fs::read(state_path(task, provider))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<TurnState>(&bytes).ok())
        .filter(|state| state.session == session)
        .unwrap_or_else(|| TurnState {
            session: session.into(),
            ..TurnState::default()
        })
}
fn save_state(task: &Path, provider: Provider, state: &TurnState) -> Option<()> {
    meowo_agent::fsutil::write_atomic_secure(
        &state_path(task, provider),
        &serde_json::to_string(state).ok()?,
    )
    .ok()
}

pub(crate) fn reserve_owner(task: &Path, session: &str) -> bool {
    let owner = task.join(".owner");
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&owner)
    {
        Ok(mut file) => {
            if file
                .write_all(session.as_bytes())
                .and_then(|_| file.sync_all())
                .is_ok()
            {
                true
            } else {
                let _ = fs::remove_file(owner);
                false
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::read_to_string(owner)
                .ok()
                .is_some_and(|owner| owner.trim() == session)
        }
        Err(_) => false,
    }
}

fn original_prompt(provider: Provider, native: &Native) -> Option<String> {
    let agent = meowo_agent::by_id(provider.id())?;
    let spec = agent.telemetry()?.transcript()?;
    let path = spec.resolve_transcript_path(
        native.data.get("transcript_path").and_then(Value::as_str),
        native.data.get("cwd").and_then(Value::as_str),
        native.data["session_id"].as_str()?,
    )?;
    let mut file = fs::File::open(&path).ok()?;
    let size = file.metadata().ok()?.len();
    let from = size.saturating_sub(4 * 1024 * 1024);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let body = if from > 0 {
        text.split_once('\n')?.1
    } else {
        &text
    };
    body.lines()
        .rev()
        .flat_map(|line| spec.parse_transcript_line_in(&path, line))
        .find_map(|event| match event {
            meowo_agent::transcript::TranscriptEvent::UserMessage { text, .. } => Some(text),
            _ => None,
        })
}

pub(crate) fn claim_target(data: &Value) -> Option<PathBuf> {
    let input = data.get("tool_input")?;
    let text = string(input, "file_path").or_else(|| string(input, "command"))?;
    lead::find_task_refs(&text.replace('\\', "/"))
        .into_iter()
        .find_map(|reference| {
            let mut path = PathBuf::from(lead::windows_path(&reference));
            if !path.is_absolute() {
                path = Path::new(data.get("cwd")?.as_str()?).join(path);
            }
            let task = path.canonicalize().ok()?;
            (task.is_dir() && task.parent()?.file_name()? == ".lead").then_some(task)
        })
        // 任务名只在变量里、cd 进 `.lead` 的写法：与 Claude 同一套认法（只对写入类调用）
        .or_else(|| lead::ownership::claim_target(data))
}

/// 显式 Provider 参数只来自已安装的 hook 命令；未知 provider 不回退成 Claude。
pub fn run(provider: &str, mode: &str, raw: &str, registry: &Path) -> Output {
    if provider == "claude" {
        return lead::run(mode, raw, registry);
    }
    if !matches!(mode, "session" | "claim" | "stop" | "gate") {
        return Output::default();
    }
    if mode == "claim" && !raw.contains(".lead") {
        return Output::default();
    }
    // PreToolUse 每次工具调用都会跑：不碰关卡文件的直接放行。
    if mode == "gate" && !raw.contains("gate.json") {
        return Output::default();
    }
    std::panic::catch_unwind(|| run_inner(provider, mode, raw, registry)).unwrap_or_default()
}

fn run_inner(provider: &str, mode: &str, raw: &str, registry: &Path) -> Output {
    let Some(provider) = Provider::parse(provider) else {
        return Output::default();
    };
    // agy 的负载是 camelCase、部分字段是 JSON 字符串，还会送来 App/IDE 的会话：先经插件改写成规范形状
    // （与普通上报同一个入口，reporter 不按 agent 身份解析）。
    let rewritten = meowo_agent::by_id(provider.id()).map(|plugin| plugin.normalize_hook_payload(raw));
    let raw = match &rewritten {
        Some(meowo_agent::HookPayload::Ignore) => return Output::default(),
        Some(meowo_agent::HookPayload::Rewrite(text)) => text.as_str(),
        _ => raw,
    };
    let Ok(raw) = serde_json::from_str::<Value>(raw) else {
        return Output::default();
    };
    let Some(mut native) = normalize(provider, &raw) else {
        return Output::default();
    };
    let session = native.data["session_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if mode == "gate" {
        return match (provider, native.event.as_str()) {
            (Provider::Kimi | Provider::Antigravity, "PreToolUse") => {
                gate_guard(provider, &native, &raw, &session, registry).unwrap_or_default()
            }
            _ => Output::default(),
        };
    }
    if mode == "claim" {
        let Some(call) = write_call(provider, &native, &raw) else {
            return Output::default();
        };
        native.data["tool_name"] = call["name"].clone();
        native.data["tool_input"] = call["input"].clone();
        let Some(task) = claim_target(&native.data) else {
            return Output::default();
        };
        if !reserve_owner(&task, &session) {
            return Output::default();
        }
        // claim 的提示是 Claude PostToolUse 的输出格式，其他 Provider 不认，丢掉。
        let out = Output { stdout: None, ..lead::run("claim", &native.data.to_string(), registry) };
        let Some(task) = lead::task_of_session(&session, registry) else {
            return out;
        };
        let mut state = read_state(&task, provider, &session);
        if state.turn.is_some() && native.turn.is_some() && state.turn != native.turn {
            state = TurnState {
                session: session.clone(),
                ..TurnState::default()
            };
        }
        if state.user.is_empty() {
            state.user = original_prompt(provider, &native).unwrap_or_default();
        }
        if native.turn.is_some() {
            state.turn = native.turn;
        }
        state.calls.push(call);
        if state.calls.len() > 64 {
            state.calls.remove(0);
        }
        let _ = save_state(&task, provider, &state);
        return out;
    }
    let Some(task) = lead::task_of_session(&session, registry) else {
        return Output::default();
    };
    let mut state = read_state(&task, provider, &session);
    let prompt_event = matches!(native.event.as_str(), "UserPromptSubmit" | "chat.message");
    if prompt_event {
        state = TurnState {
            session: session.clone(),
            turn: native.turn.clone(),
            user: native.prompt.clone().unwrap_or_default(),
            ..TurnState::default()
        };
        if save_state(&task, provider, &state).is_none() {
            let _ = fs::remove_file(state_path(&task, provider));
            return Output::default();
        }
    } else if native.event == "SessionStart" && native.data["source"] != "compact" {
        state = TurnState {
            session: session.clone(),
            ..TurnState::default()
        };
        if save_state(&task, provider, &state).is_none() {
            let _ = fs::remove_file(state_path(&task, provider));
            return Output::default();
        }
    }
    match mode {
        "session" => {
            if !(prompt_event
                || matches!(
                    native.event.as_str(),
                    "SessionStart" | "experimental.chat.system.transform" | "session.created"
                ))
            {
                return Output::default();
            }
            if provider == Provider::Kimi && !prompt_event {
                return Output::default();
            }
            let out = lead::run("session", &native.data.to_string(), registry);
            let Some(context) = out
                .stdout
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .and_then(|value| {
                    value
                        .pointer("/hookSpecificOutput/additionalContext")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
            else {
                return Output::default();
            };
            let value = match provider {
                Provider::Kimi => {
                    return Output {
                        stdout: Some(context),
                        ..Output::default()
                    }
                }
                Provider::OpenCode => json!({"additionalContext":context}),
                // agy：SessionStart / PreInvocation 的输出只认 injectSteps，系统消息会永久留在对话里
                // （hooks.md Note）。每回合第一次调用才注入一次（PreInvocation invocationNum == 0）。
                Provider::Antigravity => {
                    json!({"injectSteps":[{"systemMessage":{"systemMessage":context}}]})
                }
                _ => {
                    json!({"hookSpecificOutput":{"hookEventName":native.event,"additionalContext":context}})
                }
            };
            Output {
                stdout: Some(value.to_string()),
                ..Output::default()
            }
        }
        "stop" => {
            if matches!(provider, Provider::Kimi | Provider::Antigravity) && native.event == "Stop" {
                if let Some(out) = waiter_stop(provider, &task, &session, &mut state) {
                    return out;
                }
            }
            if provider == Provider::OpenCode
                || native.event != "Stop"
                || state.user.is_empty()
            {
                return Output::default();
            }
            if state.turn.is_some() && native.turn.is_some() && state.turn != native.turn {
                return Output::default();
            }
            if state.blocked {
                native.data["stop_hook_active"] = json!(true);
            }
            let out = lead::stop_with_evidence(
                &native.data,
                registry,
                &TurnEvidence {
                    user_text: state.user.clone(),
                    calls: state.calls.clone(),
                },
            );
            let Some(block) = out
                .stdout
                .as_ref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
            else {
                return out;
            };
            state.blocked = true;
            if save_state(&task, provider, &state).is_none() {
                return Output::default();
            }
            let reason = block["reason"].as_str().unwrap_or_default().to_string();
            match provider {
                Provider::Kimi => Output {
                    stderr: Some(reason),
                    exit: 2,
                    stdout: None,
                },
                // agy Stop：`continue` 阻止结束、reason 作为系统消息注入（真机探针 probe-hooks-1.md 第 7 条）。
                Provider::Antigravity => Output {
                    stdout: Some(json!({"decision":"continue","reason":reason}).to_string()),
                    ..Output::default()
                },
                _ => out,
            }
        }
        _ => Output::default(),
    }
}

/// kimi / antigravity：写挂起关卡之前，会话里必须已有等待器。
///
/// 为什么拦在 PreToolUse：kimi 2.6.0 的 Stop hook 实际从不触发（onDidFinishStep 里检查
/// hasPendingRequests()，而回合进行中 loop.state.status 不是 idle，恒为真；假模型实测普通回合也不调用），
/// PostToolUse 又是 fire-and-forget、不读输出。PreToolUse 是同步、可拦截、原因会交给模型的唯一入口：
/// 拦下写 gate.json（status 为 pending）的调用，原因里给出原样等待命令，模型后台启动它后重写关卡。
/// 等待器先启动时会等下一个挂起关卡出现再绑定（lead_waiter）。只拦本会话的任务，别人的任务不管。
///
/// agy 的 PreToolUse 同样同步、可拦（`{"decision":"deny","reason"}`，原因交给模型）；它的子代理是
/// 单独的对话，负载带父对话 id，已在负载改写时整个丢掉，走到这里的只会是主 agent。
fn gate_guard(provider: Provider, native: &Native, raw: &Value, session: &str, registry: &Path) -> Option<Output> {
    match provider {
        Provider::Kimi => gate_guard_with(provider, native, raw, session, registry, crate::lead_kimi::writer_is_subagent),
        _ => gate_guard_with(provider, native, raw, session, registry, |_| false),
    }
}

/// 拦下时的输出：kimi 认退出码 2 + stderr，agy 认 PreToolUse 的 deny 决定。
fn deny(provider: Provider, reason: String) -> Output {
    match provider {
        Provider::Antigravity => Output {
            stdout: Some(json!({"decision":"deny","reason":reason}).to_string()),
            ..Output::default()
        },
        _ => Output {
            stderr: Some(reason),
            exit: 2,
            stdout: None,
        },
    }
}

/// 怎么在后台启动等待器（说给模型听）。agy 的 run_command 没有后台开关：`WaitMsBeforeAsync` 到点
/// 还没结束的命令转到后台，结束时唤醒 agent（changelog「when a background task … finishes and wakes the agent」）。
fn start_how(provider: Provider) -> &'static str {
    match provider {
        Provider::Antigravity => "用 run_command 在后台启动它（WaitMsBeforeAsync 设为 500，命令会转到后台运行；不要设 RunPersistent）",
        _ => "用 Bash 工具在后台启动它（run_in_background=true，disable_timeout=true，description 写「等待 leader 答复」）",
    }
}

/// 子代理写挂起关卡：直接拒绝并说明。不给等待命令——子代理拿到命令会自己在后台跑，答复就进了
/// 子代理（第二轮真机 run5 D）。kimi 2.6 的载荷分不出子代理，判定见 [`crate::lead_kimi`]。
const SUBAGENT_GATE_DENIED: &str = "[lead] 关卡只由主会话发：子代理不要写挂起的 gate.json，也不要启动关卡等待命令。把需要 leader 决定的问题写进你交回主会话的结果里，由主会话去发关卡。";

fn gate_guard_with(
    provider: Provider,
    native: &Native,
    raw: &Value,
    session: &str,
    registry: &Path,
    writer_is_subagent: impl Fn(&str) -> bool,
) -> Option<Output> {
    let raw_name = string(raw, "tool_name")?;
    let input = raw.get("tool_input")?;
    // 各家工具名 → 统一的 Write / Edit / Bash（agy 是 write_file / edit_file / run_command，参数 PascalCase）。
    let name = match (provider, raw_name) {
        (Provider::Antigravity, "write_file") => "Write",
        (Provider::Antigravity, "edit_file") => "Edit",
        (Provider::Antigravity, "run_command") => "Bash",
        (Provider::Antigravity, _) => return None,
        (_, other) => other,
    };
    let path = || {
        string(input, "path")
            .or_else(|| string(input, "file_path"))
            .or_else(|| string(input, "Path"))
            .or_else(|| string(input, "TargetFile"))
    };
    let (target, body) = match name {
        "Write" => (
            path()?,
            string(input, "content")
                .or_else(|| string(input, "Content"))
                .unwrap_or_default()
                .to_owned(),
        ),
        "Edit" => (path()?, input.to_string()),
        "Bash" => {
            let command = string(input, "command").or_else(|| string(input, "CommandLine"))?;
            if !lead::shell_writes(command) {
                return None;
            }
            (command, command.to_owned())
        }
        _ => return None,
    };
    if !target.replace('\\', "/").contains("gate.json") || !body.contains("pending") {
        return None; // 只管写出挂起关卡；消费关卡（改成 consumed）照常放行
    }
    let mut data = native.data.clone();
    data["tool_name"] = json!(if name == "Bash" { "Bash" } else { "Write" });
    data["tool_input"] = if name == "Bash" {
        json!({"command": target})
    } else {
        json!({"file_path": target})
    };
    let task = claim_target(&data)?;
    // 先认子代理、再认领：子代理不能借写关卡把任务认到自己名下（同一个 session_id）。
    if writer_is_subagent(session) {
        return Some(deny(provider, SUBAGENT_GATE_DENIED.into()));
    }
    // 先认领：等待器按会话找任务，模型下一步就要启动它。
    if !reserve_owner(&task, session) {
        return None;
    }
    lead::run("claim", &data.to_string(), registry);
    let task = lead::task_of_session(session, registry)?;
    // 刚要求过启动等待器：它的进程可能还没起来登记（实测后台 Bash 起 reporter 要几百毫秒），
    // 这时再拦一次会让模型起第二个，旧的被接手退出、又白白唤醒一回合。等它几秒。
    let asked = task.join(".native-waiter-asked");
    let recently_asked = fs::metadata(&asked)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < std::time::Duration::from_secs(120));
    let draft_id = (name == "Write")
        .then(|| serde_json::from_str::<Value>(&body).ok())
        .flatten()
        .and_then(|draft| draft.get("id").and_then(Value::as_str).map(str::to_owned));
    if draft_id.as_deref().is_some_and(|id| !lead_waiter::valid_target_id(id)) {
        return Some(deny(
            provider,
            "[lead] 关卡 id 不能用于目标等待：请使用非空、不以 -- 开头且不含控制字符的新 id。草稿尚未写入。".into(),
        ));
    }
    let owner_pid = crate::proc::owner_pid(provider.id());
    let ready = || match draft_id.as_deref() {
        Some(id) => lead_waiter::target_waiter_ready(&task, id, session, owner_pid),
        None => lead_waiter::any_live_waiter(&task),
    };
    let rounds = if recently_asked { 50 } else { 1 };
    for round in 0..rounds {
        if ready() {
            return None;
        }
        if round + 1 < rounds {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = fs::write(&asked, session);
    let reporter = lead_waiter::reporter_path()?;
    let command = match draft_id.as_deref() {
        Some(id) => lead_waiter::command_for_gate(provider.id(), &reporter, session, id),
        None => lead_waiter::command(provider.id(), &reporter, session),
    }?;
    let next = if lead::publication::exists(&task) {
        "启动后重新写 gate.json 草稿，再按 lead skill 用刚查询的版本执行 publish；确认本次 pending 卡已发布后才能结束本轮。答复到达后用 status 核对权威版本，再通过 consume 确认接手，不修改草稿消费。"
    } else {
        "启动后再重新写这个 gate.json，然后结束本轮。答复到达后读 gate.json，按 lead skill 处理。"
    };
    let how = start_how(provider);
    Some(deny(
        provider,
        format!(
            "[lead] 写关卡前，先启动 Meowo 的关卡等待命令：{how}，命令：`{command}`。{next}不要轮询。leader 答复后它会结束、你会收到后台任务完成的通知。"
        ),
    ))
}

/// 回合结束时的关卡兜底（kimi / antigravity 的 Stop）：
/// - 答复已到、还没送达：直接把答复交给模型接着做（同一回合内，零等待）；
/// - 关卡挂起、会话里没有等待器：要求模型在后台启动等待器。kimi 只能靠这里（它的 PostToolUse
///   是 fire-and-forget，不读输出），且 CLI 每回合只认一次 Stop 拦截；agy 的 Stop continue 不限次数，
///   自己记一次，避免无限续跑。
fn waiter_stop(
    provider: Provider,
    task: &Path,
    session: &str,
    state: &mut TurnState,
) -> Option<Output> {
    let reason = if let Some(gate) = lead_waiter::undelivered_answer(task, session)
        .filter(|gate| lead_waiter::live_waiter(task, &gate.id).is_none())
    {
        lead_waiter::mark_delivered(task, &gate);
        lead::answer_message(task, &gate)
    } else {
        let gate = lead_waiter::needs_waiter(task, session)?;
        if provider == Provider::Antigravity {
            if state.waiter_asked.as_deref() == Some(gate.id.as_str()) {
                return None;
            }
            state.waiter_asked = Some(gate.id.clone());
            save_state(task, provider, state)?;
        }
        let reporter = lead_waiter::reporter_path()?;
        let command = lead_waiter::command(provider.id(), &reporter, session)?;
        let how = start_how(provider);
        format!(
            "[lead] 关卡 {} 在等 leader 答复，会话里还没有等待器。{how}，命令：`{command}`。启动后直接结束本轮，不要轮询；它结束时你会收到后台任务完成的通知，届时读它的输出，按 lead skill 查询权威关卡并处理答复。",
            gate.id
        )
    };
    Some(match provider {
        Provider::Antigravity => Output {
            stdout: Some(json!({"decision":"continue","reason":reason}).to_string()),
            ..Output::default()
        },
        _ => Output {
            stderr: Some(reason),
            exit: 2,
            stdout: None,
        },
    })
}

#[cfg(test)]
mod tests;
