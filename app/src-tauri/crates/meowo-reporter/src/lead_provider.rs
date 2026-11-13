//! 原生 hook 的身份/工具/输出适配。资料：Codex 官方 /docs/hooks；Kimi Code hooks；
//! Gemini hooks/reference；OpenCode v1.19.3 packages/plugin/src/index.ts。
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
    Gemini,
    OpenCode,
}
impl Provider {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "codex" => Some(Self::Codex),
            "kimi" => Some(Self::Kimi),
            "gemini" => Some(Self::Gemini),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Kimi => "kimi",
            Self::Gemini => "gemini",
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
            Provider::Gemini => matches!(
                event,
                "SessionStart"
                    | "BeforeAgent"
                    | "BeforeModel"
                    | "BeforeTool"
                    | "AfterTool"
                    | "AfterAgent"
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
        Provider::Codex | Provider::Kimi => "PostToolUse",
        Provider::Gemini => "AfterTool",
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
    if provider == Provider::Gemini
        && (!response.is_object() || response.get("llmContent").is_none())
    {
        return None;
    }
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
        (Provider::Gemini, "write_file" | "replace") => {
            ("Write", json!({"file_path":string(input,"file_path")?}))
        }
        (Provider::OpenCode, "write" | "edit") => {
            ("Write", json!({"file_path":string(input,"filePath")?}))
        }
        (Provider::Kimi, "Bash")
        | (Provider::Gemini, "run_shell_command")
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
            if provider == Provider::Gemini {
                let output = response.get("llmContent").and_then(Value::as_str)?;
                if input.get("is_background").and_then(Value::as_bool) == Some(true)
                    || response.pointer("/data/pid").is_some()
                    || [
                        "Command was cancelled",
                        "Command was automatically cancelled",
                        "Command moved to background",
                        "Signal:",
                    ]
                    .iter()
                    .any(|marker| output.contains(marker))
                {
                    return None;
                }
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
    /// 本回合已经为哪个关卡要求过启动等待器（gemini 的 AfterAgent deny 不限次数，只问一次）。
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
}

/// 显式 Provider 参数只来自已安装的 hook 命令；未知 provider 不回退成 Claude。
pub fn run(provider: &str, mode: &str, raw: &str, registry: &Path) -> Output {
    if provider == "claude" {
        return lead::run(mode, raw, registry);
    }
    if !matches!(mode, "session" | "claim" | "stop" | "gate") {
        return Output::default();
    }
    if mode == "claim"
        && !raw.contains(".lead")
        && !raw.contains("lead wait --provider gemini")
        && !raw.contains(GEMINI_AGENT_TOOL)
    {
        return Output::default();
    }
    // PreToolUse / BeforeTool 每次工具调用都会跑：不碰关卡文件、也不是派子代理的直接放行。
    if mode == "gate" && !raw.contains("gate.json") && !raw.contains(GEMINI_AGENT_TOOL) {
        return Output::default();
    }
    std::panic::catch_unwind(|| run_inner(provider, mode, raw, registry)).unwrap_or_default()
}

fn run_inner(provider: &str, mode: &str, raw: &str, registry: &Path) -> Output {
    let Some(provider) = Provider::parse(provider) else {
        return Output::default();
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
    if provider == Provider::Gemini && matches!(native.event.as_str(), "BeforeAgent" | "AfterAgent") {
        // 主回合开始/结束时不可能有子代理在跑（gemini 子代理在主回合的 invoke_agent 调用里同步执行）：
        // 清掉可能残留的标记（invoke_agent 被取消时不一定有 AfterTool）。这两个事件只由主客户端触发
        // （0.68 GeminiClient.fireBeforeAgentHookSafe / fireAfterAgentHookSafe）。
        subagents_clear(registry, &session);
    }
    if mode == "gate" {
        return match (provider, native.event.as_str()) {
            (Provider::Kimi, "PreToolUse") => {
                kimi_gate_guard(&native, &raw, &session, registry).unwrap_or_default()
            }
            (Provider::Gemini, "BeforeTool") => {
                gemini_gate_guard(&native, &raw, &session, registry).unwrap_or_default()
            }
            _ => Output::default(),
        };
    }
    if mode == "claim" {
        if provider == Provider::Gemini {
            if string(&raw, "tool_name") == Some(GEMINI_AGENT_TOOL) {
                subagent_leave(registry, &session, &raw);
                return Output::default();
            }
            if let Some(out) = gemini_waiter_started(&raw, &session) {
                return out;
            }
        }
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
        let out = lead::run("claim", &native.data.to_string(), registry);
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
        let gate_written = touches_gate(&call);
        state.calls.push(call);
        if state.calls.len() > 64 {
            state.calls.remove(0);
        }
        let _ = save_state(&task, provider, &state);
        // 子代理写的关卡不给它起等待器（BeforeTool 本该已拦下，这里兜住没拦到的写法）。
        if provider == Provider::Gemini && gate_written && !subagents_running(registry, &session) {
            if let Some(tail) = gemini_waiter_tail(&task, &session) {
                return tail;
            }
        }
        return out;
    }
    let Some(task) = lead::task_of_session(&session, registry) else {
        return Output::default();
    };
    let mut state = read_state(&task, provider, &session);
    let prompt_event = matches!(
        native.event.as_str(),
        "UserPromptSubmit" | "BeforeAgent" | "chat.message"
    );
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
                    "SessionStart"
                        | "BeforeModel"
                        | "experimental.chat.system.transform"
                        | "session.created"
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
                Provider::Gemini if native.event == "BeforeModel" => {
                    let Some(mut request) =
                        raw.get("llm_request").cloned().filter(Value::is_object)
                    else {
                        return Output::default();
                    };
                    let Some(messages) = request.get_mut("messages").and_then(Value::as_array_mut)
                    else {
                        return Output::default();
                    };
                    // role 只能是 user：0.68 把 hook 消息原样转成 contents，Gemini API 对 system 角色回
                    // 400「Role 'system' is not supported」（2026-11-13 真模型实测，假模型测不出）。
                    // 已有的旧上下文就地替换、不删除：fromHookLLMRequest 按下标把带文本的消息对回原
                    // contents，删掉中间一条会让后面的消息全部错位。
                    let content = format!("[meowo-lead-context]\n{context}");
                    let existing = messages.iter_mut().find(|message| {
                        message["content"]
                            .as_str()
                            .is_some_and(|text| text.starts_with("[meowo-lead-context]\n"))
                    });
                    match existing {
                        Some(message) => *message = json!({"role":"user","content":content}),
                        None => messages.push(json!({"role":"user","content":content})),
                    }
                    json!({"hookSpecificOutput":{"llm_request":request}})
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
            if matches!(provider, Provider::Kimi | Provider::Gemini)
                && matches!(native.event.as_str(), "Stop" | "AfterAgent")
            {
                if let Some(out) = waiter_stop(provider, &task, &session, &mut state) {
                    return out;
                }
            }
            if provider == Provider::OpenCode
                || !matches!(native.event.as_str(), "Stop" | "AfterAgent")
                || state.user.is_empty()
            {
                return Output::default();
            }
            if state.turn.is_some() && native.turn.is_some() && state.turn != native.turn {
                return Output::default();
            }
            if provider == Provider::Gemini {
                if let Some(prompt) = native.prompt {
                    if prompt != state.user {
                        return Output::default();
                    }
                    state.user = prompt;
                }
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
                Provider::Gemini => Output {
                    stdout: Some(json!({"decision":"deny","reason":reason}).to_string()),
                    ..Output::default()
                },
                _ => out,
            }
        }
        _ => Output::default(),
    }
}

/// kimi：写挂起关卡之前，会话里必须已有等待器。
///
/// 为什么拦在 PreToolUse：kimi 2.6.0 的 Stop hook 实际从不触发（onDidFinishStep 里检查
/// hasPendingRequests()，而回合进行中 loop.state.status 不是 idle，恒为真；假模型实测普通回合也不调用），
/// PostToolUse 又是 fire-and-forget、不读输出。PreToolUse 是同步、可拦截、原因会交给模型的唯一入口：
/// 拦下写 gate.json（status 为 pending）的调用，原因里给出原样等待命令，模型后台启动它后重写关卡。
/// 等待器先启动时会等下一个挂起关卡出现再绑定（lead_waiter）。只拦本会话的任务，别人的任务不管。
fn kimi_gate_guard(native: &Native, raw: &Value, session: &str, registry: &Path) -> Option<Output> {
    kimi_gate_guard_with(native, raw, session, registry, crate::lead_kimi::writer_is_subagent)
}

/// 子代理写挂起关卡：直接拒绝并说明。不给等待命令——子代理拿到命令会自己在后台跑，答复就进了
/// 子代理（第二轮真机 run5 D）。kimi 2.6 的载荷分不出子代理，判定见 [`crate::lead_kimi`]。
const SUBAGENT_GATE_DENIED: &str = "[lead] 关卡只由主会话发：子代理不要写挂起的 gate.json，也不要启动关卡等待命令。把需要 leader 决定的问题写进你交回主会话的结果里，由主会话去发关卡。";

fn kimi_gate_guard_with(
    native: &Native,
    raw: &Value,
    session: &str,
    registry: &Path,
    writer_is_subagent: impl Fn(&str) -> bool,
) -> Option<Output> {
    let name = string(raw, "tool_name")?;
    let input = raw.get("tool_input")?;
    let (target, body) = match name {
        "Write" => (
            string(input, "path").or_else(|| string(input, "file_path"))?,
            string(input, "content").unwrap_or_default().to_owned(),
        ),
        "Edit" => (
            string(input, "path").or_else(|| string(input, "file_path"))?,
            input.to_string(),
        ),
        "Bash" => {
            let command = string(input, "command")?;
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
        return Some(Output {
            stderr: Some(SUBAGENT_GATE_DENIED.into()),
            exit: 2,
            stdout: None,
        });
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
    let rounds = if recently_asked { 50 } else { 1 };
    for round in 0..rounds {
        if lead_waiter::any_live_waiter(&task) {
            return None;
        }
        if round + 1 < rounds {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = fs::write(&asked, session);
    let reporter = lead_waiter::reporter_path()?;
    let command = lead_waiter::command("kimi", &reporter, session)?;
    Some(Output {
        stderr: Some(format!(
            "[lead] 写关卡前，先用 Bash 工具在后台启动 Meowo 的关卡等待命令（run_in_background=true，disable_timeout=true，description 写「等待 leader 答复」），命令：`{command}`。启动后再重新写这个 gate.json，然后结束本轮，不要轮询。leader 答复后它会结束、你会收到后台任务完成的通知，届时读它的输出和 gate.json，按 lead skill 处理答复。"
        )),
        exit: 2,
        stdout: None,
    })
}

/// gemini 0.68 派子代理的唯一工具（agent-tool.js `AGENT_TOOL_NAME = "invoke_agent"`，Kind.Agent）。
const GEMINI_AGENT_TOOL: &str = "invoke_agent";

/// gemini：主 agent 的 invoke_agent 在跑期间，写挂起关卡的只能是子代理——拦下。
///
/// 为什么靠「invoke_agent 起止」认子代理：0.68 子代理（LocalAgentExecutor）与主 agent 共用同一个
/// config 与 HookSystem，hook 载荷（createBaseInput：session_id / transcript_path / cwd）完全相同，
/// 没有 agent 身份；子代理的工具表剔除了 Kind.Agent，不会再派孙代理；主 agent 调 invoke_agent 时
/// 自己卡在这次调用里，不会并行写关卡。BeforeTool 在审批之前同步执行（scheduler evaluateBeforeToolHook），
/// `decision: deny` 会把原因作为工具错误交给模型。
fn gemini_gate_guard(
    native: &Native,
    raw: &Value,
    session: &str,
    registry: &Path,
) -> Option<Output> {
    let name = string(raw, "tool_name")?;
    let input = raw.get("tool_input")?;
    if name == GEMINI_AGENT_TOOL {
        subagent_enter(registry, session, raw);
        return None;
    }
    if !subagents_running(registry, session) {
        return None;
    }
    let (target, body) = match name {
        "write_file" => (
            string(input, "file_path")?,
            string(input, "content").unwrap_or_default().to_owned(),
        ),
        "replace" => (string(input, "file_path")?, input.to_string()),
        "run_shell_command" => {
            let command = string(input, "command")?;
            if !lead::shell_writes(command) {
                return None;
            }
            (command, command.to_owned())
        }
        _ => return None,
    };
    if !target.replace('\\', "/").contains("gate.json") || !body.contains("pending") {
        return None;
    }
    let mut data = native.data.clone();
    data["tool_input"] = if name == "run_shell_command" {
        json!({"command": target})
    } else {
        json!({"file_path": target})
    };
    claim_target(&data)?;
    Some(Output {
        stdout: Some(json!({"decision":"deny","reason":SUBAGENT_GATE_DENIED}).to_string()),
        ..Output::default()
    })
}

/// 子代理在跑的标记：`<registry 目录>/lead-subagents/<session>/<调用指纹>`，一次 invoke_agent 一个文件
/// （BeforeTool 建、AfterTool 删）。并行派多个子代理时各自独立，不用计数器、不怕并发 hook 互踩。
/// 载荷里没有调用 id，指纹取参数（agent_name + prompt）的哈希；同参数的两次并行调用会共用一个，
/// 先结束的那个会提前撤标记，只是少拦，不会误拦主 agent。
fn subagents_dir(registry: &Path, session: &str) -> Option<PathBuf> {
    let safe = !session.is_empty()
        && session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    safe.then(|| {
        registry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("lead-subagents")
            .join(session)
    })
}

fn call_fingerprint(raw: &Value) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.get("tool_input")
        .map(Value::to_string)
        .unwrap_or_default()
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn subagent_enter(registry: &Path, session: &str, raw: &Value) {
    if let Some(dir) = subagents_dir(registry, session) {
        let _ = fs::create_dir_all(&dir);
        let _ = fs::write(dir.join(call_fingerprint(raw)), "");
    }
}

fn subagent_leave(registry: &Path, session: &str, raw: &Value) {
    if let Some(dir) = subagents_dir(registry, session) {
        let _ = fs::remove_file(dir.join(call_fingerprint(raw)));
    }
}

fn subagents_clear(registry: &Path, session: &str) {
    if let Some(dir) = subagents_dir(registry, session) {
        let _ = fs::remove_dir_all(dir);
    }
}

fn subagents_running(registry: &Path, session: &str) -> bool {
    subagents_dir(registry, session)
        .and_then(|dir| fs::read_dir(dir).ok())
        .is_some_and(|mut entries| entries.next().is_some())
}

fn touches_gate(call: &Value) -> bool {
    let input = &call["input"];
    string(input, "file_path")
        .or_else(|| string(input, "command"))
        .is_some_and(|text| text.replace('\\', "/").contains("gate.json"))
}

/// 这次关卡要不要（再）启动等待器：挂起或已作答未送达，且没有活着的等待器。
fn waiter_wanted(task: &Path, session: &str) -> bool {
    lead_waiter::needs_waiter(task, session).is_some()
        || lead_waiter::undelivered_answer(task, session)
            .is_some_and(|gate| lead_waiter::live_waiter(task, &gate.id).is_none())
}

/// Gemini：写 gate.json 的工具调用之后，用 AfterTool 的尾调用让 CLI 自己在后台启动等待器，
/// 不依赖模型照 skill 去做（0.68 源码：tailToolCallRequest 替换原调用的结果，仍走审批策略）。
fn gemini_waiter_tail(task: &Path, session: &str) -> Option<Output> {
    if !waiter_wanted(task, session) {
        return None;
    }
    let reporter = lead_waiter::reporter_path()?;
    let command = lead_waiter::command("gemini", &reporter, session)?;
    Some(Output {
        stdout: Some(
            json!({"hookSpecificOutput":{"hookEventName":"AfterTool","tailToolCallRequest":{"name":"run_shell_command","args":{"command":command,"is_background":true,"description":"Meowo: wait for the leader's gate answer"}}}})
                .to_string(),
        ),
        ..Output::default()
    })
}

/// 尾调用会替换写 gate.json 那次调用的结果，模型只看到「命令已转到后台」。在等待器自己的
/// AfterTool 上补一句说明，免得模型以为写入失败去重写关卡、或开始轮询。
fn gemini_waiter_started(raw: &Value, session: &str) -> Option<Output> {
    if raw.get("hook_event_name").and_then(Value::as_str) != Some("AfterTool")
        || raw.get("tool_name").and_then(Value::as_str) != Some("run_shell_command")
    {
        return None;
    }
    let command = raw.pointer("/tool_input/command").and_then(Value::as_str)?;
    if !command.ends_with(&format!(" lead wait --provider gemini --session {session}")) {
        return None;
    }
    // 不用 [lead] 开头：真模型把它当成了 leader 的答复，直接消费了关卡（2026-11-13 实测）。
    let context = "[Meowo] 这是 Meowo 自动启动的关卡等待器（还没有答复），gate.json 已写好。现在直接结束本轮：不要重写 gate.json，不要轮询或读它的日志。leader 答复后它会结束，答复会自动送回本会话，届时按 lead skill 处理。";
    Some(Output {
        stdout: Some(
            json!({"hookSpecificOutput":{"hookEventName":"AfterTool","additionalContext":context}})
                .to_string(),
        ),
        ..Output::default()
    })
}

/// 回合结束时的关卡兜底（kimi Stop / gemini AfterAgent）：
/// - 答复已到、还没送达：直接把答复交给模型接着做（同一回合内，零等待）；
/// - 关卡挂起、会话里没有等待器：要求模型在后台启动等待器。kimi 只能靠这里（它的 PostToolUse
///   是 fire-and-forget，不读输出），且 CLI 每回合只认一次 Stop 拦截；gemini 是尾调用被拒或
///   用 shell 写关卡时的兜底，自己记一次，避免 AfterAgent 无限续跑。
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
        if provider == Provider::Gemini {
            if state.waiter_asked.as_deref() == Some(gate.id.as_str()) {
                return None;
            }
            state.waiter_asked = Some(gate.id.clone());
            save_state(task, provider, state)?;
        }
        let reporter = lead_waiter::reporter_path()?;
        let command = lead_waiter::command(provider.id(), &reporter, session)?;
        let how = match provider {
            Provider::Kimi => "用 Bash 工具在后台启动它（run_in_background=true，disable_timeout=true，description 写「等待 leader 答复」）",
            _ => "用 run_shell_command 在后台启动它（is_background=true）",
        };
        format!(
            "[lead] 关卡 {} 在等 leader 答复，会话里还没有等待器。{how}，命令：`{command}`。启动后直接结束本轮，不要轮询；它结束时你会收到后台任务完成的通知，届时读它的输出和任务目录里的 gate.json，按 lead skill 处理答复。",
            gate.id
        )
    };
    Some(match provider {
        Provider::Kimi => Output {
            stderr: Some(reason),
            exit: 2,
            stdout: None,
        },
        _ => Output {
            stdout: Some(json!({"decision":"deny","reason":reason}).to_string()),
            ..Output::default()
        },
    })
}

#[cfg(test)]
mod tests;
