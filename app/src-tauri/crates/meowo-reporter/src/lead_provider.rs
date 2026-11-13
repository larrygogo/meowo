//! 原生 hook 的身份/工具/输出适配。资料：Codex 官方 /docs/hooks；Kimi Code hooks；
//! Gemini hooks/reference；OpenCode v1.19.3 packages/plugin/src/index.ts。
//! MCP 工具参数不能提供会话身份；未知事件、工具、失败形状均放行且不认领。

use crate::lead::{self, Output, TurnEvidence};
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
                "SessionStart" | "UserPromptSubmit" | "PostToolUse" | "Stop" | "PostCompact"
            ),
            Provider::Gemini => matches!(
                event,
                "SessionStart" | "BeforeAgent" | "BeforeModel" | "AfterTool" | "AfterAgent"
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
    if !matches!(mode, "session" | "claim" | "stop") {
        return Output::default();
    }
    if mode == "claim" && !raw.contains(".lead") {
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
        let out = lead::run("claim", &native.data.to_string(), registry);
        let Some(task) = lead::task_of_session(&session, registry) else {
            return out;
        };
        let mut state = read_state(&task, provider, &session);
        if state.turn.is_some() && native.turn.is_some() && state.turn != native.turn {
            state = TurnState {
                session,
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
                    messages.retain(|message| {
                        !(message["role"] == "system"
                            && message["content"]
                                .as_str()
                                .is_some_and(|text| text.starts_with("[meowo-lead-context]\n")))
                    });
                    messages.push(json!({"role":"system","content":format!("[meowo-lead-context]\n{context}")}));
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

#[cfg(test)]
mod tests;
