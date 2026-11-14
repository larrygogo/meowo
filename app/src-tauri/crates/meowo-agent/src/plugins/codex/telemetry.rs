//! codex（OpenAI Codex CLI）会话解析。codex 的 hooks 与 claude 同款：Stop hook 直带
//! `last_assistant_message`（故最近 AI 正文走 hook payload，不在此读），标题靠首条 prompt 命名
//! （rollout 首条 user 文本被 AGENTS.md/指令包裹，不适合当标题）。唯一需从会话文件补的是【模型】
//! ——Stop hook 不携带模型，需读 rollout 的 `turn_context.model`。
//!
//! rollout：`{CODEX_HOME 或 ~/.codex}/sessions/<YYYY>/<MM>/<DD>/rollout-<ISO>-<session_uuid>.jsonl`，
//! 每行一个事件 `{type, payload}`。首行 `type=session_meta`；其后 `type=turn_context` 的
//! `payload.model` 即模型（如 "gpt-5.5"），通常在文件靠前（首回合）。

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::transcript::ChatItem;
use super::exec_script::{output_parts, parse_patch, script_calls, OutputPart};
use crate::transcript::{
    clip_text, ToolDetail, ToolFacts, ToolKind, TranscriptEvent, TranscriptInfo, TranscriptParser,
    TranscriptSpec, TurnError,
};

fn chat_id(prefix: &str, line: &str) -> String {
    let mut hash = crate::codec::FNV1A_OFFSET;
    crate::codec::fnv1a(&mut hash, line.as_bytes());
    format!("codex-{prefix}-{hash:016x}")
}

fn content_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                // 0.148 的 Reasoning.summary_text 是裸字符串数组，消息 content 是对象数组。
                part.as_str().map(str::to_string).or_else(|| {
                    part.get("text")
                        .or_else(|| part.get("input_text"))
                        .or_else(|| part.get("output_text"))
                        .and_then(|text| text.as_str())
                        .map(str::to_string)
                })
            })
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

/// codex 0.148+ 的统一逐项完成事件 `event_msg/item_completed`：老版本的
/// `event_msg/user_message`、`agent_message`、`agent_reasoning` 不再写入 rollout，
/// 用户与 AI 正文只剩这里（`response_item/message` 虽也存一份 assistant 正文，但
/// 与本事件同 id 重复，且其 user 形态裹着 environment_context，维持整体跳过）。
///
/// CommandExecution 与 FileChange 刻意不产出：它们是某次 `custom_tool_call`（exec 脚本）里
/// `exec_command` / `apply_patch` 的子事件，命令、补丁与输出已由主链承载（脚本在
/// [`tool_call_events`] 里解开），再发一遍就是同一件事上屏两次。本机 68 份 rollout 里
/// 每条 FileChange 都有一次脚本内的 apply_patch 对应（1573 次调用、656 条 FileChange）；
/// 反过来不成立——续写段带入的历史只有主链、没有这些子事件，所以主链才是完整的那份。
fn completed_item_events(
    payload: &serde_json::Value,
    timestamp: Option<String>,
    line: &str,
) -> Vec<TranscriptEvent> {
    let Some(item) = payload.get("item") else {
        return Vec::new();
    };
    let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
    // rollout 的 item.id 全局唯一；缺失时退回整行哈希。
    let id = |prefix: &str| {
        item.get("id")
            .and_then(|v| v.as_str())
            .map(|i| format!("codex-{prefix}-{i}"))
            .unwrap_or_else(|| chat_id(prefix, line))
    };
    match kind {
        "UserMessage" => {
            let text = item.get("content").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::UserMessage {
                    id: id("user"),
                    timestamp,
                    text,
                    interjected: false,
                    // 回合进行中用户又发的话，codex 不另起回合：这条记录带的 turn_id 与该回合
                    // 第一条用户消息相同（本机 98 份 rollout 里 15 条回合中途的用户消息都是，
                    // 例：01a13d51 那份的 1962、1966 行与回合 01a13e4c 同号）。把它交给前端判
                    // 回合边界。不标 interjected：codex 的回溯把插话也算一轮（见 chat_line_mark），
                    // 标了撤回会数错轮数。老版本的 `event_msg/user_message` 没有这个字段。
                    turn_id: payload
                        .get("turn_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
                .into_iter()
                .collect()
        }
        "AgentMessage" => {
            let text = item.get("content").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::AssistantMessage {
                    id: id("assistant"),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        "Reasoning" => {
            // 0.148 的思考多为 encrypted_content（summary_text/raw_content 皆空），
            // 只在真有可读摘要时上屏。
            let text = item
                .get("summary_text")
                .map(content_text)
                .filter(|t| !t.trim().is_empty())
                .or_else(|| item.get("raw_content").map(content_text))
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::Reasoning {
                    id: id("reasoning"),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        // 0.148 起压缩不再写 `event_msg/context_compacted`，改写 `compacted` +
        // `item_completed/ContextCompaction` + `response_item/compaction` 三行（本机 0.162.1
        // rollout 各 4/4/5 次，context_compacted 0 次；research-cli-drift X3，2026-11-12）。
        // 只认 ContextCompaction：`compacted` 在 0.144 与 context_compacted 成对出现（各 25 次），
        // 认它会让老会话每次压缩出两条分隔；`response_item/compaction` 还会出现在续写段开头
        // （ordinal 1，带入的旧压缩），不是这一刻发生的压缩。
        "ContextCompaction" => vec![TranscriptEvent::Metadata {
            id: id("compact"),
            timestamp,
            kind: "compact".into(),
            compaction: None,
        }],
        _ => Vec::new(),
    }
}

/// `turn_context.approval_policy` → 审批维度的模式值。
///
/// 0.162.1 及以前是字符串（`never` / `on-request` / `untrusted`）。0.163.0 起默认启动写成对象
/// `{"granular":{"sandbox_approval":false,"rules":false,"skill_approval":false,
/// "request_permissions":true,"mcp_elicitations":true}}`（本机 0.163.0 与 0.166.0 rollout 的
/// turn_context 共 11 份全是这个形态；codex.exe 里枚举为 UnlessTrusted/OnRequest/Granular/Never；
/// research-cli-drift X4，2026-11-12）。按 `as_str` 取会落空，按钮退回显示维度名。
/// 五个开关全开等于「按需审批」，全关等于「从不询问」，其余如实报 `granular`（分类审批）。
/// rollout 的审批策略 + 沙箱类型 → 权限档 id（codex 插件 launch_options 的 approval 栏）。
/// 宿主读会话当前设置时也用它换成同一套 id。
/// 沙箱类型在 rollout 里是 kebab（`workspace-write`），在 app-server 里是驼峰（`workspaceWrite`），都认。
pub fn permission_tier(
    approval: Option<&str>,
    sandbox: Option<&str>,
    reviewer: Option<&str>,
) -> Option<&'static str> {
    let sandbox: String = sandbox?
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    match sandbox.as_str() {
        "readonly" => Some("readOnly"),
        "dangerfullaccess" => Some("yolo"),
        "workspacewrite" if approval == Some("on-request") => Some(
            if matches!(reviewer, Some("auto_review" | "guardian_subagent")) {
                "approveForMe"
            } else {
                "ask"
            },
        ),
        _ => None,
    }
}

fn approval_mode(policy: &serde_json::Value) -> Option<&str> {
    if let Some(name) = policy.as_str() {
        return Some(name);
    }
    let flags = policy.get("granular")?.as_object()?;
    let values: Vec<bool> = flags
        .values()
        .filter_map(serde_json::Value::as_bool)
        .collect();
    Some(if !values.is_empty() && values.iter().all(|v| *v) {
        "on-request"
    } else if !values.is_empty() && values.iter().all(|v| !*v) {
        "never"
    } else {
        "granular"
    })
}

/// 回合收尾带的错误（`task_complete.error`）。时间线的错误卡与会话级「出错」状态
/// （侧栏、灵动岛、通知，见 [`CodexParser`]）都从这里取，两处不会各说各话。
fn turn_error(payload: &serde_json::Value) -> Option<TurnError> {
    let error = payload.get("error").filter(|e| !e.is_null())?;
    let info = error
        .get("codex_error_info")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let text = error
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or(info)
        .to_string();
    if text.trim().is_empty() {
        return None;
    }
    let label = if info == "unauthorized" {
        "需要重新登录"
    } else {
        "回合出错"
    };
    Some(TurnError {
        label: label.into(),
        raw: text,
        fingerprint: format!("codex:{label}"),
    })
}

/// 摘要上限（字符），与 claude / opencode 同值。
const SUMMARY_MAX_CHARS: usize = 800;

/// 命令行实参：字符串原样；数组形态（老版本 `["bash","-lc","…"]`、`["pwsh","-Command","…"]`）
/// 取真正的命令那一段，其余形态用空格拼起来。
fn command_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let parts: Vec<&str> = value.as_array()?.iter().filter_map(|v| v.as_str()).collect();
    match parts.as_slice() {
        [] => None,
        [_, flag, script] if matches!(*flag, "-lc" | "-c" | "-Command" | "/c") => {
            Some((*script).to_string())
        }
        parts => Some(parts.join(" ")),
    }
}

/// 一次工具调用 → 语义类别、摘要与结构化详情。`name` 是脚本里 `tools.<name>` 的名字，
/// 或直接调用（老版本、协作类工具）的函数名；`arg` 是它的实参。
/// 表里没有的工具为 Other：协作类的 wait_agent / list_agents / interrupt_agent / send_message
/// 不是「派出子任务」，此前因为名字里有 agent 被前端算进「已派出 N 个子任务」。
fn describe_tool(name: &str, arg: &serde_json::Value) -> (ToolKind, String, Option<ToolDetail>) {
    let text = |key: &str| arg.get(key).and_then(|v| v.as_str()).map(str::to_string);
    match name {
        "exec_command" | "shell_command" | "shell" | "local_shell" | "container.exec" => {
            let command = arg
                .get("cmd")
                .or_else(|| arg.get("command"))
                .and_then(command_text);
            match command {
                Some(command) => (
                    ToolKind::Run,
                    clip_text(&command, SUMMARY_MAX_CHARS),
                    Some(ToolDetail::Command {
                        command,
                        description: None,
                    }),
                ),
                None => (ToolKind::Run, String::new(), None),
            }
        }
        "apply_patch" => {
            let patch = arg
                .as_str()
                .map(str::to_string)
                .or_else(|| text("patch"))
                .or_else(|| text("input"))
                .unwrap_or_default();
            let files = parse_patch(&patch);
            let summary = clip_text(
                &files
                    .iter()
                    .map(|file| file.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                SUMMARY_MAX_CHARS,
            );
            // 只新建文件的补丁是「写入」，其余是「编辑」。
            let kind = if !files.is_empty() && files.iter().all(|file| file.op == "add") {
                ToolKind::Write
            } else {
                ToolKind::Edit
            };
            (
                kind,
                summary,
                (!files.is_empty()).then_some(ToolDetail::Patch { files }),
            )
        }
        "web__run" | "web_search" | "web.run" => {
            let list = |key: &str, field: &str| -> Vec<String> {
                arg.get(key)
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get(field).and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect()
            };
            // open 的 ref_id 可以是地址，也可以是上一次结果的引用号（turn6view0），后者不是地址。
            let urls: Vec<String> = list("open", "ref_id")
                .into_iter()
                .filter(|url| url.contains("://"))
                .collect();
            let mut queries = list("search_query", "q");
            queries.extend(text("query"));
            let total = urls.len() + queries.len();
            let url = urls.into_iter().next();
            let query = queries.into_iter().next();
            let summary = url.clone().or_else(|| query.clone()).unwrap_or_default();
            (
                ToolKind::Web,
                clip_text(&summary, SUMMARY_MAX_CHARS),
                (total > 0).then_some(ToolDetail::Web {
                    url,
                    query,
                    more: total.saturating_sub(1) as u32,
                }),
            )
        }
        "view_image" => match text("path") {
            Some(path) => (
                ToolKind::Read,
                path.clone(),
                Some(ToolDetail::Read {
                    path,
                    offset: None,
                    limit: None,
                }),
            ),
            None => (ToolKind::Read, String::new(), None),
        },
        "update_plan" => (
            ToolKind::Todo,
            clip_text(&text("explanation").unwrap_or_default(), SUMMARY_MAX_CHARS),
            None,
        ),
        // 派出子代理：任务正文（message）在 rollout 里是加密串，只有任务名可读。
        "spawn_agent" => (
            ToolKind::Agent,
            text("task_name")
                .or_else(|| text("agent_type"))
                .unwrap_or_default(),
            None,
        ),
        _ => (ToolKind::Other, other_summary(arg), None),
    }
}

/// 未归类工具的摘要：先取能认出对象的那个入参，再退回整包入参。协作类工具的 `message`
/// 是加密串（`gAAAA…`），显示出来只是一屏乱码，去掉。
fn other_summary(arg: &serde_json::Value) -> String {
    const KEYS: [&str; 7] = ["target", "task_name", "cell_id", "path", "name", "query", "url"];
    if let Some(text) = KEYS
        .iter()
        .find_map(|key| arg.get(key).and_then(|v| v.as_str()))
    {
        return clip_text(text, SUMMARY_MAX_CHARS);
    }
    match arg {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => clip_text(text.trim(), SUMMARY_MAX_CHARS),
        serde_json::Value::Object(map) => {
            let readable: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter(|(_, value)| !value.as_str().is_some_and(|s| s.starts_with("gAAAA")))
                // 输出预算、轮询间隔这类参数不说明「在干什么」。
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "yield_time_ms" | "max_output_tokens" | "max_tokens" | "timeout_ms"
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            if readable.is_empty() {
                String::new()
            } else {
                clip_text(
                    &serde_json::Value::Object(readable).to_string(),
                    SUMMARY_MAX_CHARS,
                )
            }
        }
        other => clip_text(&other.to_string(), SUMMARY_MAX_CHARS),
    }
}

/// `response_item/function_call`（直接调用）与 `custom_tool_call`（exec 脚本）→ 工具调用。
///
/// 脚本里有几个 `tools.<name>(…)` 就产出几条，id 是 `<call_id>#<序号>`，与回执一侧
/// [`tool_result_events`] 的分段约定一致（见 exec_script 模块头）。脚本里一个工具调用都
/// 认不出时（纯计算的脚本、认不得的写法）退回一条，摘要是脚本原文。
fn tool_call_events(
    payload: &serde_json::Value,
    scripted: bool,
    timestamp: Option<String>,
    line: &str,
) -> Vec<TranscriptEvent> {
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let call_id = payload
        .get("call_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| chat_id("tool", line));
    let raw = payload
        .get("arguments")
        .or_else(|| payload.get("input"))
        .map(content_text)
        .unwrap_or_default();
    let call = |id: String, name: &str, arg: &serde_json::Value| {
        let (kind, summary, detail) = describe_tool(name, arg);
        TranscriptEvent::ToolCall {
            id,
            timestamp: timestamp.clone(),
            name: name.to_string(),
            kind,
            summary,
            subagent: None,
            detail,
            plan: None,
        }
    };
    if !scripted {
        // 直接调用的实参是一段 JSON 文本。
        let arg = serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or(serde_json::Value::String(raw));
        return vec![call(call_id, name, &arg)];
    }
    if name != "exec" {
        // 老版本不走脚本的自定义工具（apply_patch 直接带补丁文本）。
        return vec![call(
            format!("{call_id}#0"),
            name,
            &serde_json::Value::String(raw),
        )];
    }
    let calls = script_calls(&raw);
    if calls.is_empty() {
        return vec![TranscriptEvent::ToolCall {
            id: format!("{call_id}#0"),
            timestamp,
            name: name.to_string(),
            kind: ToolKind::Other,
            summary: clip_text(raw.trim(), SUMMARY_MAX_CHARS),
            subagent: None,
            detail: None,
            plan: None,
        }];
    }
    calls
        .iter()
        .enumerate()
        .map(|(index, script_call)| {
            call(
                format!("{call_id}#{index}"),
                &script_call.name,
                &script_call.arg,
            )
        })
        .collect()
}

/// 工具回执。脚本回执按 `text()` 分段，第 i 段挂到 `<call_id>#<i>`；直接调用的回执并成一条。
/// 失败的来源有三个：行上的 `is_error`（新版恒无）、脚本状态头 `Script failed`、命令结果里
/// 的非零退出码——此前只看第一个，失败的命令从不标红（走查 A2）。
fn tool_result_events(
    payload: &serde_json::Value,
    scripted: bool,
    timestamp: Option<String>,
    line: &str,
) -> Vec<TranscriptEvent> {
    let call_id = payload
        .get("call_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let flagged = payload
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let parts = output_parts(payload.get("output").unwrap_or(&serde_json::Value::Null));
    let result = |id: String, tool_call_id: Option<String>, part: OutputPart| {
        TranscriptEvent::ToolResult {
            id,
            timestamp: timestamp.clone(),
            tool_call_id,
            text: part.text,
            is_error: part.is_error || flagged,
            // codex 没有带侧车流的子任务（见 mod.rs 的 subagents 说明）。
            subagent: None,
            patch: None,
            facts: ToolFacts {
                exit_code: part.exit_code,
                count: None,
                interrupted: part.interrupted,
            },
        }
    };
    let base = chat_id("result", line);
    if scripted {
        return parts
            .into_iter()
            .enumerate()
            .map(|(index, part)| {
                result(
                    if index == 0 {
                        base.clone()
                    } else {
                        format!("{base}-{index}")
                    },
                    call_id.as_ref().map(|id| format!("{id}#{index}")),
                    part,
                )
            })
            .collect();
    }
    let merged = OutputPart {
        text: parts
            .iter()
            .map(|part| part.text.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        exit_code: parts.iter().rev().find_map(|part| part.exit_code),
        is_error: parts.iter().any(|part| part.is_error),
        interrupted: parts.iter().any(|part| part.interrupted),
    };
    vec![result(base, call_id, merged)]
}

fn parse_transcript_events(line: &str) -> Vec<TranscriptEvent> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    let timestamp = value
        .get("timestamp")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let payload = value.get("payload").unwrap_or(&value);
    let outer = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let kind = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match (outer, kind) {
        ("event_msg", "user_message") => {
            let text = payload.get("message").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::UserMessage {
                    id: chat_id("user", line),
                    timestamp,
                    text,
                    interjected: false,
                    turn_id: None,
                })
                .into_iter()
                .collect()
        }
        ("event_msg", "agent_message") => {
            let text = payload.get("message").map(content_text).unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::AssistantMessage {
                    id: chat_id("assistant", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("event_msg", "agent_reasoning") => {
            let text = payload
                .get("text")
                .or_else(|| payload.get("message"))
                .map(content_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::Reasoning {
                    id: chat_id("reasoning", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("response_item", "reasoning") => {
            let text = payload
                .get("summary")
                .or_else(|| payload.get("content"))
                .or_else(|| payload.get("text"))
                .map(content_text)
                .unwrap_or_default();
            (!text.trim().is_empty())
                .then(|| TranscriptEvent::Reasoning {
                    id: chat_id("reasoning", line),
                    timestamp,
                    text,
                })
                .into_iter()
                .collect()
        }
        ("response_item", "function_call") | ("response_item", "custom_tool_call") => {
            tool_call_events(payload, kind == "custom_tool_call", timestamp, line)
        }
        ("response_item", "function_call_output")
        | ("response_item", "custom_tool_call_output") => {
            tool_result_events(payload, kind == "custom_tool_call_output", timestamp, line)
        }
        // 用户按 Esc 打断回合（本机 68 份 rollout 里 64 次）。画成与 claude 同款的中断分隔条。
        ("event_msg", "turn_aborted") => vec![TranscriptEvent::Metadata {
            id: chat_id("interrupted", line),
            timestamp,
            kind: "interrupted".into(),
            compaction: None,
        }],
        ("event_msg", "item_completed") => completed_item_events(payload, timestamp, line),
        // 回合收尾带错误（如 unauthorized：refresh token 被吊销）：终端里满屏红字，
        // 对话窗此前却一片空白——这是 rollout 里唯一的回合级错误信号。
        ("event_msg", "task_complete") => turn_error(payload)
            .map(|error| TranscriptEvent::TurnError {
                id: chat_id("error", line),
                timestamp,
                label: error.label,
                text: error.raw,
            })
            .into_iter()
            .collect(),
        ("event_msg", "context_compacted") => vec![TranscriptEvent::Metadata {
            id: chat_id("compact", line),
            timestamp,
            kind: "compact".into(),
            compaction: None,
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
fn parse_chat_items(line: &str) -> Vec<ChatItem> {
    parse_transcript_events(line)
        .into_iter()
        .map(ChatItem::from)
        .collect()
}

/// codex 的看板分析解析器：只产**会话级出错**。
///
/// 为什么要有它：回合出错（`task_complete.error`，如 refresh token 被吊销）在时间线里是
/// 一张红色错误卡，可侧栏状态、灵动岛与通知读的是分析口径——此前这里是空解析器，
/// 同一个会话对话里报错、侧栏却写着「等待输入」，也不弹出错通知（走查 A8）。
/// 错误的判据与时间线共用 [`turn_error`]。新回合开始（`task_started`）即清掉。
/// 标题、预览、上下文用量仍不从 rollout 取（标题走 session_index，见 resolve_title）。
#[derive(Default)]
struct CodexParser {
    error: Option<TurnError>,
}

impl TranscriptParser for CodexParser {
    fn fold_line(&mut self, line: &str) {
        // 绝大多数行两样都不是：先按字面量筛，免得每行整解析一次 JSON（rollout 动辄十几 MB）。
        if !line.contains("\"task_complete\"") && !line.contains("\"task_started\"") {
            return;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
            return;
        }
        let Some(payload) = value.get("payload") else {
            return;
        };
        match payload.get("type").and_then(|v| v.as_str()) {
            Some("task_started") => self.error = None,
            Some("task_complete") => self.error = turn_error(payload),
            _ => {}
        }
    }

    fn to_info(&self) -> TranscriptInfo {
        TranscriptInfo {
            error: self.error.clone(),
            ..TranscriptInfo::default()
        }
    }
}

/// 看板轮询用的 rollout 定位缓存（见 [`crate::transcript::PolledPaths`]）。[`find_rollout`] 要走一遍
/// sessions 目录树；命中的路径记 30 秒——回溯后续写段会换文件，过期后重新找当前段。
static POLLED_ROLLOUTS: crate::transcript::PolledPaths =
    crate::transcript::PolledPaths::new(std::time::Duration::from_secs(30));

pub struct CodexTranscript;
pub static CODEX_TRANSCRIPT: CodexTranscript = CodexTranscript;

impl TranscriptSpec for CodexTranscript {
    fn new_parser(&self) -> Box<dyn TranscriptParser> {
        Box::new(CodexParser::default())
    }

    /// 分析只要当前在写的那一段（出错信号在最新的回合里），不必拼接回溯前的历史。
    fn resolve_transcript_path_polled(
        &self,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<PathBuf> {
        POLLED_ROLLOUTS.get_or_resolve(session_id, || {
            find_rollout(session_id)
                .map(|known| super::lineage::current_rollout(&known, session_id))
        })
    }

    fn resolve_transcript_path(
        &self,
        transcript_path: Option<&str>,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<PathBuf> {
        // 回溯后续写段另起文件（分页历史，见 lineage.rs）：先找当前段，再按 history_base 拼。
        let known = transcript_path
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .or_else(|| find_rollout(session_id))?;
        let current = super::lineage::current_rollout(&known, session_id);
        Some(super::lineage::chat_path(&current, session_id))
    }

    /// codex 自己起的线程名：`<CODEX_HOME>/session_index.jsonl` 里该会话**最后一条**的
    /// `thread_name`（它按追加写，/rename 也是再追加一条）。实测是模型概括的短句，
    /// 如「Identify dominant color」，不是首句原文。
    fn resolve_title(
        &self,
        _transcript_path: Option<&str>,
        _cwd: Option<&str>,
        session_id: &str,
    ) -> Option<String> {
        std::iter::once(codex_home())
            .flatten()
            .chain(managed_codex_homes())
            .find_map(|home| thread_name_in(&home.join("session_index.jsonl"), session_id))
    }

    fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
        parse_transcript_events(line)
    }

    /// 回溯（TUI 双击 Esc 编辑上一条）不改写 rollout，追加 `event_msg/thread_rolled_back
    /// {num_turns}`（本机实拍 0.144），与 kimi 的 context.undo 同形，交给读取层回放。轮的边界
    /// 按 codex 源码 `is_user_turn_boundary`：每条非上下文注入的用户消息，插话也算一轮——即
    /// 我们产出 UserMessage 的那些行（老版本 `user_message`、0.148+ `item_completed/UserMessage`，
    /// 同一版本只写其一，本机 40 份 rollout 核对）。
    fn chat_line_mark(&self, line: &str) -> Option<crate::ChatLineMark> {
        if line.contains("\"thread_rolled_back\"") {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            let payload = value.get("payload")?;
            if value.get("type")?.as_str()? != "event_msg"
                || payload.get("type")?.as_str()? != "thread_rolled_back"
            {
                return None;
            }
            let turns = payload.get("num_turns")?.as_u64()?;
            return (turns > 0).then_some(crate::ChatLineMark::Undo(turns as usize));
        }
        if !line.contains("\"user_message\"") && !line.contains("\"UserMessage\"") {
            return None;
        }
        parse_transcript_events(line)
            .iter()
            .any(|event| matches!(event, TranscriptEvent::UserMessage { .. }))
            .then_some(crate::ChatLineMark::TurnStart)
    }

    fn agent_modes_from_line(&self, line: &str) -> Vec<crate::AgentMode> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            return Vec::new();
        };
        let outer = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&value);
        let mut modes = Vec::new();
        if outer == "turn_context" {
            if let Some(mode) = payload
                .pointer("/collaboration_mode/mode")
                .and_then(|v| v.as_str())
            {
                modes.push(crate::AgentMode::new("collaboration", mode));
            }
            // 审批策略与沙箱合成 TUI `/permissions` 的一档（0.167.2 四档：Read Only / Ask for approval /
            // Approve for me / Full Access，research-codex-opencode §9），与新建页、会话里的权限菜单
            // 同一套 id。对不上任何一档（granular、旧的 never+workspace-write）时退回审批策略原值。
            let approval = payload.get("approval_policy").and_then(approval_mode);
            let sandbox = payload
                .pointer("/sandbox_policy/type")
                .and_then(|v| v.as_str());
            let reviewer = payload
                .get("approvals_reviewer")
                .or_else(|| payload.get("approvalsReviewer"))
                .and_then(|v| v.as_str());
            if let Some(tier) = permission_tier(approval, sandbox, reviewer).or(approval) {
                modes.push(crate::AgentMode::new("approval", tier));
            }
        } else if outer == "event_msg"
            && payload.get("type").and_then(|v| v.as_str()) == Some("task_started")
        {
            if let Some(mode) = payload
                .get("collaboration_mode_kind")
                .and_then(|v| v.as_str())
            {
                modes.push(crate::AgentMode::new("collaboration", mode));
            }
        }
        modes
    }

    fn supports_chat(&self) -> bool {
        true
    }

    /// 长会话首开只解析尾部一屏。rollout 逐行独立解析，回溯（thread_rolled_back）由通用读取层
    /// 回放、判不准时自己扩窗；续写段已由 lineage 拼成一份文件。模式只来自 turn_context 与
    /// task_started 两种行（agent_modes_from_line），codex 从不产出计划。
    fn tail_read(&self) -> Option<crate::transcript::TailRead> {
        Some(crate::transcript::TailRead {
            mode_markers: &["turn_context", "task_started"],
            plan_markers: &[],
        })
    }

    /// 只为会话级出错（见 [`CodexParser`]）。
    fn supports_analysis(&self) -> bool {
        true
    }
}

/// codex 在本机的实况（数据目录/hooks 规格/凭据/启动 argv）。变体表见
/// `meowo_agent::plugins::codex`：数据目录 `CODEX_HOME` 优先，否则 `~/.codex`。
/// 检测/接线/状态/账号/会话读取全部经此一处解析路径。
pub fn codex_install() -> Option<crate::Installation> {
    crate::registry::installation(crate::id::CODEX)
}

/// codex 数据根。`codex_install()` 的便捷取值。
pub fn codex_home() -> Option<PathBuf> {
    codex_install().map(|i| i.data_dir)
}

/// codex 的启动前缀 argv（不含 `resume <id>`）：bun 全局 exe ／ `node <codex.js>` ／ 独立安装 exe。
/// 都没有则 None（调用方回退裸名 codex）。优先级与理由见变体表的 `LAUNCH`。
pub fn codex_launch_prefix() -> Option<Vec<String>> {
    codex_install()?.launch
}

/// 在 codex 的 `sessions/` 下按 session_id 找 rollout 文件（文件名内嵌 uuid，以 `<uuid>.jsonl`
/// 结尾）。递归 walk 年/月/日（限深，避免误入无关深目录）。仅作 transcript_path 缺失时的兜底。
///
/// 依次查默认数据目录与**全部受管 profile 目录**：session_id 全局唯一，首个命中即返回。
/// reporter 由 profile 里的 codex 派生时自带 `CODEX_HOME`，`codex_home()` 即命中；meowo-app
/// 没有该变量，profile 会话靠 `managed_codex_homes()` 兜底——漏了它，多账号下的 codex 会话
/// 上下文/切换引擎/跨账号搬迁会全部静默落空（kimi 侧 `managed_share_dirs` 与 claude 侧
/// `managed_projects_dirs` 是同一条纪律）。
fn find_rollout(session_id: &str) -> Option<PathBuf> {
    let suffix = format!("{session_id}.jsonl");
    std::iter::once(codex_home())
        .flatten()
        .chain(managed_codex_homes())
        .find_map(|home| walk_find(&home.join("sessions"), &suffix, 6))
}

/// 某份 `session_index.jsonl` 里该会话最后一次记下的线程名。空名不算。
fn thread_name_in(index: &Path, session_id: &str) -> Option<String> {
    let content = std::fs::read_to_string(index).ok()?;
    // 从尾往前找：追加写的索引里最新一条在最后，找到就停，不用把整份都解析一遍。
    content
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("id").and_then(|id| id.as_str()) == Some(session_id))
        .find_map(|v| {
            v.get("thread_name")
                .and_then(|n| n.as_str())
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_string)
        })
}

/// Meowo 管理的 codex profile 数据目录（每个 profile 根就是它的 `CODEX_HOME`，见
/// `plugins/codex` 的 `PROFILE` 声明）。
fn managed_codex_homes() -> Vec<PathBuf> {
    crate::managed_profile_dirs(crate::id::CODEX.as_str())
}

fn walk_find(dir: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            subdirs.push(p);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(suffix))
        {
            return Some(p);
        }
    }
    for d in subdirs {
        if let Some(f) = walk_find(&d, suffix, depth - 1) {
            return Some(f);
        }
    }
    None
}

/// 读文件前 max_lines 行为一个 String（模型在文件靠前，无需读全量）。
fn read_head_lines(path: &Path, max_lines: usize) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut out = String::new();
    for line in BufReader::new(f)
        .lines()
        .take(max_lines)
        .map_while(Result::ok)
    {
        out.push_str(&line);
        out.push('\n');
    }
    Some(out)
}

/// 取最后一轮的模型，避免 /model 切换后仍展示首轮（旧实现与旧单测曾固定取第一条）。
pub fn parse_model(content: &str) -> Option<String> {
    content.lines().rev().find_map(|line| {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        if value.get("type")?.as_str()? != "turn_context" {
            return None;
        }
        value
            .pointer("/payload/model")?
            .as_str()
            .filter(|model| !model.is_empty())
            .map(str::to_owned)
    })
}

/// 会话当前在写的 rollout 段（回溯后续写段另起文件，见 lineage.rs）。宿主读当前设置用
/// （lead_native::codex_app_server::read_rollout_settings，任务 20261114-tui-model-switch）。
pub fn current_rollout_path(session_id: &str) -> Option<PathBuf> {
    let known = find_rollout(session_id)?;
    Some(super::lineage::current_rollout(&known, session_id))
}

/// 取某 codex 会话的模型展示名：优先用 hook 给的 transcript_path，否则按 session_id 在 sessions 下找。
/// 最新段尾部优先，尾部没有 turn_context 才退首轮；有界读取避免每轮扫描长会话。
pub fn read_model(transcript_path: Option<&str>, session_id: &str) -> Option<String> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    let path = super::lineage::current_rollout(&path, session_id);
    read_tail(&path, 128 * 1024)
        .and_then(|tail| parse_model(&tail))
        .or_else(|| read_head_lines(&path, 200).and_then(|head| parse_model(&head)))
}

/// 从 rollout 文本取**最后一条 info 非 null** 的 token_count 的 (input_tokens, model_context_window)。
/// codex 会话开头的 token_count `info` 为 null（只有 rate_limits），跳过。used 取 last_token_usage.input_tokens
/// （最近一次请求的 context 输入量，已含 cached_input_tokens）。
pub fn parse_context(content: &str) -> Option<(i64, i64)> {
    let mut last: Option<(i64, i64)> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let payload = v.get("payload");
        if payload.and_then(|p| p.get("type")).and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.and_then(|p| p.get("info")).filter(|i| !i.is_null()) else {
            continue;
        };
        let used = info
            .get("last_token_usage")
            .and_then(|l| l.get("input_tokens"))
            .and_then(|x| x.as_i64());
        let window = info.get("model_context_window").and_then(|x| x.as_i64());
        if let (Some(u), Some(w)) = (used, window) {
            last = Some((u, w));
        }
    }
    last
}

/// 读文件尾部最多 max_bytes 字节为 lossy UTF-8（首个半截行交给 parse_context 跳过）。
/// `pub(super)`：account 的 token_count 尾部扫描复用同一份有界读——rollout 可达数十 MB，
/// 绝不能整个读进内存。
pub(super) fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(size.saturating_sub(max_bytes)))
        .ok()?;
    let mut buf = Vec::new();
    f.take(max_bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// codex 会话最近上下文占用：定位 rollout（hook 的 transcript_path 优先，否则按 id 找），
/// 尾部读取最后一条 token_count。定位/解析失败返回 None。
pub fn read_context(
    transcript_path: Option<&str>,
    session_id: &str,
) -> Option<crate::caps::ContextUsage> {
    let path = transcript_path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| find_rollout(session_id))?;
    // 最新的 token_count 在当前续写段末尾（回溯后根段不再增长，见 lineage.rs）。
    let path = super::lineage::current_rollout(&path, session_id);
    const TAIL_BYTES: u64 = 256 * 1024;
    let text = read_tail(&path, TAIL_BYTES)?;
    let (used, window) = parse_context(&text)?;
    if window <= 0 {
        return None;
    }
    let pct = (used * 100 / window).clamp(0, 100);
    Some(crate::caps::ContextUsage {
        used_pct: pct,
        window,
        // codex 的 token_count 事件不带模型名。
        model: None,
    })
}

// ═══ 能力槽 ═══

pub struct CodexTelemetry;
pub static TELEMETRY: CodexTelemetry = CodexTelemetry;

impl crate::caps::TelemetryCap for CodexTelemetry {
    /// codex 的 Stop hook 直带 AI 正文（同 claude）；模型 Stop 不带，从 rollout 的 turn_context 补。
    fn stop_outputs(&self, ctx: &crate::caps::HookContext) -> crate::caps::StopOutputs {
        crate::caps::StopOutputs {
            last_ai: ctx.last_assistant_message.map(str::to_string),
            model: read_model(ctx.transcript_path, ctx.session_id),
            effort: None,
        }
    }

    fn read_context(&self, ctx: &crate::caps::HookContext) -> Option<crate::caps::ContextUsage> {
        read_context(ctx.transcript_path, ctx.session_id)
    }

    fn resolves_transcript_title(&self) -> bool {
        true
    }

    fn transcript_title_generated(&self) -> bool {
        true
    }

    fn transcript(&self) -> Option<&'static dyn TranscriptSpec> {
        Some(&CODEX_TRANSCRIPT)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn thread_name_takes_the_last_entry_for_the_session() {
        let dir = std::env::temp_dir().join(format!("meowo-codex-idx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let index = dir.join("session_index.jsonl");
        std::fs::write(
            &index,
            concat!(
                r#"{"id":"a","thread_name":"First name","updated_at":"t1"}"#,
                "\n",
                r#"{"id":"b","thread_name":"Other","updated_at":"t2"}"#,
                "\n",
                r#"{"id":"a","thread_name":"Renamed","updated_at":"t3"}"#,
                "\n",
                r#"{"id":"c","thread_name":"  ","updated_at":"t4"}"#,
                "\n",
            ),
        )
        .unwrap();
        assert_eq!(
            super::thread_name_in(&index, "a").as_deref(),
            Some("Renamed")
        );
        assert_eq!(super::thread_name_in(&index, "b").as_deref(), Some("Other"));
        assert_eq!(super::thread_name_in(&index, "c"), None);
        assert_eq!(super::thread_name_in(&index, "missing"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
    use super::*;

    /// 压缩分隔：0.144.1 写 `event_msg/context_compacted`（与 `compacted` 成对），0.162.1 起写
    /// `compacted` + `item_completed/ContextCompaction` + `response_item/compaction`。两代各只出
    /// 一条分隔（行原文取自本机 rollout，长字段截短；research-cli-drift X3）。
    #[test]
    fn compaction_divider_old_and_new_rollouts() {
        let compacts = |lines: &[&str]| {
            lines
                .iter()
                .flat_map(|l| parse_transcript_events(l))
                .filter(
                    |e| matches!(e, TranscriptEvent::Metadata { kind, .. } if kind == "compact"),
                )
                .count()
        };
        // 0.144.1
        assert_eq!(
            compacts(&[
                r#"{"timestamp":"2026-07-13T07:00:00.000Z","type":"compacted","payload":{"message":"","replacement_history":[]}}"#,
                r#"{"timestamp":"2026-07-13T07:00:00.001Z","type":"event_msg","payload":{"type":"context_compacted"}}"#,
            ]),
            1
        );
        // 0.162.1（2026-10-15 rollout ordinal 1485/1490 与续写段 ordinal 1）
        assert_eq!(
            compacts(&[
                r#"{"timestamp":"2026-10-15T05:57:37.691Z","ordinal":1485,"type":"compacted","payload":{"message":"","replacement_history":[]}}"#,
                r#"{"timestamp":"2026-10-15T05:57:37.698Z","ordinal":1490,"type":"event_msg","payload":{"type":"item_completed","thread_id":"01a13d51-8612-7a62-adab-5bdfe4daf61f","turn_id":"01a13e20-9e5f-7433-98a0-2224340e0a8f","item":{"type":"ContextCompaction","id":"01a13e21-69bd-7160-91c2-008b5d13e015"},"started_at_ms":1792043739581,"completed_at_ms":1792043857698}}"#,
                r#"{"timestamp":"2026-10-15T06:05:57.748Z","ordinal":1,"type":"response_item","payload":{"type":"compaction","id":"cmp_0b0909f4","encrypted_content":"gAAAAABq0GtP"}}"#,
            ]),
            1
        );
    }

    /// 审批档：0.162.1 字符串、0.163.0/0.166.0 granular 对象（本机 rollout turn_context 原文）。
    #[test]
    fn approval_policy_string_and_granular_object() {
        let mode = |policy: &str| {
            let line = format!(
                r#"{{"type":"turn_context","payload":{{"approval_policy":{policy},"sandbox_policy":{{"type":"workspace-write"}}}}}}"#
            );
            CODEX_TRANSCRIPT
                .agent_modes_from_line(&line)
                .into_iter()
                .find(|m| m.dimension == "approval")
                .map(|m| m.value)
        };
        // never + workspace-write（旧 Full Auto）不是 0.167 的任何一档：退回审批策略原值。
        assert_eq!(mode(r#""never""#).as_deref(), Some("never"));
        assert_eq!(mode(r#""on-request""#).as_deref(), Some("ask"));
        assert_eq!(
            mode(r#"{"granular":{"sandbox_approval":false,"rules":false,"skill_approval":false,"request_permissions":true,"mcp_elicitations":true}}"#).as_deref(),
            Some("granular")
        );
        assert_eq!(
            mode(r#"{"granular":{"sandbox_approval":true,"rules":true,"skill_approval":true,"request_permissions":true,"mcp_elicitations":true}}"#).as_deref(),
            Some("ask")
        );
        assert_eq!(
            mode(r#"{"granular":{"sandbox_approval":false,"rules":false,"skill_approval":false,"request_permissions":false,"mcp_elicitations":false}}"#).as_deref(),
            Some("never")
        );
    }

    /// 回溯记录 `thread_rolled_back {num_turns}`（本机 0.144 实拍形态）：插话也算一轮（codex
    /// is_user_turn_boundary）；增量读到时整读重灌，被撤的那轮从对话里收回。
    #[test]
    fn thread_rollback_retracts_recent_turns() {
        use std::io::Write;
        let path =
            std::env::temp_dir().join(format!("meowo_codex_rollback_{}.jsonl", std::process::id()));
        let body = concat!(
            r#"{"timestamp":"2026-07-17T03:00:00.000Z","type":"event_msg","payload":{"type":"user_message","message":"先做甲"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-17T03:00:01.000Z","type":"event_msg","payload":{"type":"agent_message","message":"甲好了"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-17T03:00:02.000Z","type":"event_msg","payload":{"type":"user_message","message":"再做乙"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-17T03:00:03.000Z","type":"event_msg","payload":{"type":"user_message","message":"顺便看看"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-17T03:00:04.000Z","type":"event_msg","payload":{"type":"agent_message","message":"乙好了"}}"#,
            "\n",
        );
        std::fs::write(&path, body).unwrap();
        let first = crate::read_chat_delta(&CODEX_TRANSCRIPT, &path, 0, None);
        assert_eq!(first.items.len(), 5);
        // 插话也是一轮：撤 2 轮 = 从「再做乙」起全收回。
        std::fs::OpenOptions::new().append(true).open(&path).unwrap()
            .write_all(concat!(r#"{"timestamp":"2026-07-17T03:46:03.741Z","type":"event_msg","payload":{"type":"thread_rolled_back","num_turns":2}}"#, "\n").as_bytes())
            .unwrap();
        let next = crate::read_chat_delta(&CODEX_TRANSCRIPT, &path, first.offset, first.mtime);
        assert!(next.reset && next.undone);
        let texts: Vec<String> = next
            .items
            .iter()
            .filter_map(|item| match item {
                ChatItem::UserText { text, .. } | ChatItem::AssistantText { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["先做甲", "甲好了"]);
        std::fs::remove_file(path).ok();
    }

    /// rollout 查找必须覆盖**受管 profile 目录**：meowo-app 进程没有 `CODEX_HOME`，
    /// 只查默认目录的话，多账号下的 codex 会话上下文/切换引擎/跨账号搬迁会全部静默落空
    /// （claude 与 kimi 侧各自栽过同一跤，见它们的同名用例）。
    #[test]
    fn rollout_lookup_includes_managed_codex_profiles() {
        let sid = format!("01a01e42-0000-0000-0000-{:012}", std::process::id());
        let home = std::env::temp_dir().join(format!("codex_profile_home_{}", std::process::id()));
        let rollout = home
            .join(".meowo/profiles/codex/work/sessions/2026/08/20")
            .join(format!("rollout-2026-08-20T16-21-09-{sid}.jsonl"));
        std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
        std::fs::write(&rollout, "{}\n").unwrap();

        let _env = crate::env_guard();
        let old_home = std::env::var("USERPROFILE").ok();
        // meowo PTY 里跑测试会带着指向真库的 MEOWO_DB——数据根解析优先读它，不一并指进
        // 临时 home 的话，managed 目录会扫到真实 profile 而不是本用例造的。
        let old_db = std::env::var("MEOWO_DB").ok();
        // 同理：外层若带着 CODEX_HOME（本会话自身就可能带），默认目录会命中别处。
        let old_codex = std::env::var("CODEX_HOME").ok();
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("MEOWO_DB", home.join(".meowo").join("board.db"));
        std::env::remove_var("CODEX_HOME");
        let found = find_rollout(&sid);
        match old_home {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_db {
            Some(value) => std::env::set_var("MEOWO_DB", value),
            None => std::env::remove_var("MEOWO_DB"),
        }
        if let Some(value) = old_codex {
            std::env::set_var("CODEX_HOME", value);
        }
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(found.as_deref(), Some(rollout.as_path()));
    }

    #[test]
    fn extracts_collaboration_approval_and_sandbox_dimensions() {
        let modes = CODEX_TRANSCRIPT.agent_modes_from_line(
            r#"{"type":"turn_context","payload":{"approval_policy":"on-request","sandbox_policy":{"type":"workspace-write"},"collaboration_mode":{"mode":"plan"}}}"#,
        );
        assert_eq!(
            modes,
            vec![
                crate::AgentMode::new("collaboration", "plan"),
                crate::AgentMode::new("approval", "ask"),
            ]
        );
        let tier = |line: &str| {
            CODEX_TRANSCRIPT
                .agent_modes_from_line(line)
                .into_iter()
                .find(|m| m.dimension == "approval")
                .map(|m| m.value)
        };
        assert_eq!(
            tier(r#"{"type":"turn_context","payload":{"approval_policy":"on-request","sandbox_policy":{"type":"read-only"}}}"#).as_deref(),
            Some("readOnly")
        );
        assert_eq!(
            tier(r#"{"type":"turn_context","payload":{"approval_policy":"never","sandbox_policy":{"type":"danger-full-access"}}}"#).as_deref(),
            Some("yolo")
        );
        assert_eq!(
            tier(r#"{"type":"turn_context","payload":{"approval_policy":"on-request","approvals_reviewer":"auto_review","sandbox_policy":{"type":"workspace-write"}}}"#).as_deref(),
            Some("approveForMe")
        );
        assert_eq!(
            CODEX_TRANSCRIPT.agent_modes_from_line(
                r#"{"type":"event_msg","payload":{"type":"task_started","collaboration_mode_kind":"default"}}"#,
            ),
            vec![crate::AgentMode::new("collaboration", "default")]
        );
    }

    #[test]
    fn parse_context_takes_last_nonnull_token_count() {
        let rollout = r#"
{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":7.0}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":6766,"cached_input_tokens":4480},"model_context_window":258400}}}
"#;
        // 跳过 info=null 那条；取最后一条 info 非 null 的：input_tokens=6766, window=258400。
        assert_eq!(parse_context(rollout), Some((6766, 258400)));
    }

    #[test]
    fn parse_context_none_when_no_token_count() {
        assert_eq!(
            parse_context(r#"{"type":"turn_context","payload":{"model":"gpt-5.5"}}"#),
            None
        );
    }

    #[test]
    fn parse_model_takes_latest_turn_context() {
        let rollout = r#"
{"type":"session_meta","payload":{"id":"x","cwd":"/p","model_provider":"openai"}}
{"type":"turn_context","payload":{"model":"gpt-5.5","cwd":"/p","effort":"medium"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}
{"type":"turn_context","payload":{"model":"gpt-5.3-codex"}}
"#;
        assert_eq!(parse_model(rollout).as_deref(), Some("gpt-5.3-codex"));
    }

    #[test]
    fn read_model_uses_latest_turn_beyond_the_original_head_window() {
        let path = std::env::temp_dir().join(format!(
            "meowo-model-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut rollout = String::from(r#"{"type":"turn_context","payload":{"model":"first"}}"#);
        for _ in 0..300 {
            rollout.push_str("\n{\"type\":\"event_msg\",\"payload\":{}} ");
        }
        rollout.push_str(
            r#"
{"type":"turn_context","payload":{"model":"latest"}}
"#,
        );
        std::fs::write(&path, rollout).unwrap();
        let model = read_model(path.to_str(), "test-thread");
        std::fs::remove_file(path).unwrap();
        assert_eq!(model.as_deref(), Some("latest"));
    }

    #[test]
    fn parse_model_none_when_absent() {
        let rollout = r#"{"type":"session_meta","payload":{"id":"x"}}
{"type":"turn_context","payload":{"cwd":"/p"}}"#;
        assert_eq!(parse_model(rollout), None);
        assert_eq!(parse_model(""), None);
    }

    #[test]
    fn chat_parser_uses_clean_events_and_tool_records_without_duplicates() {
        let user = r#"{"timestamp":"t1","type":"event_msg","payload":{"type":"user_message","message":"修复测试"}}"#;
        let assistant = r#"{"timestamp":"t2","type":"event_msg","payload":{"type":"agent_message","message":"开始处理"}}"#;
        let tool = r#"{"timestamp":"t3","type":"response_item","payload":{"type":"function_call","name":"shell_command","arguments":"{\"command\":\"cargo test\"}","call_id":"c1"}}"#;
        let reasoning = r#"{"timestamp":"t2","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"先运行测试"}]}}"#;
        assert!(
            matches!(&parse_chat_items(user)[0], ChatItem::UserText { text, .. } if text == "修复测试")
        );
        assert!(
            matches!(&parse_chat_items(assistant)[0], ChatItem::AssistantText { text, .. } if text == "开始处理")
        );
        assert!(
            matches!(&parse_chat_items(tool)[0], ChatItem::ToolUse { name, summary, .. } if name == "shell_command" && summary.contains("cargo test"))
        );
        assert!(matches!(
            &parse_chat_items(reasoning)[0],
            ChatItem::Reasoning { text, .. } if text == "先运行测试"
        ));
        // 原始 response_item user message 常含指令包，与 event_msg.user_message 重复，必须跳过。
        assert!(parse_chat_items(
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[]}}"#
        )
        .is_empty());
    }

    /// codex 0.148 换了 rollout 形态：user/agent 正文只在 `event_msg/item_completed` 里
    ///（老的 `event_msg/user_message` 等不再写入），漏解析的直接后果是对话窗只剩 exec 组。
    /// 样本取自真机 rollout（0.148.0）。
    #[test]
    fn parses_codex_0148_item_completed_events() {
        let user = r#"{"timestamp":"t1","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","id":"u1","content":[{"type":"text","text":"我想做一个功能","text_elements":[]}]}}}"#;
        assert!(matches!(
            &parse_chat_items(user)[0],
            ChatItem::UserText { id, text, .. } if text == "我想做一个功能" && id == "codex-user-u1"
        ));

        let agent = r#"{"timestamp":"t2","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"m1","content":[{"type":"Text","text":"我先检查项目结构"}],"phase":"commentary"}}}"#;
        assert!(matches!(
            &parse_chat_items(agent)[0],
            ChatItem::AssistantText { text, .. } if text == "我先检查项目结构"
        ));

        // 0.148 的思考通常整段加密（summary_text/raw_content 皆空）→ 不产出。
        let encrypted = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":"r1","summary_text":[],"raw_content":[]}}}"#;
        assert!(parse_chat_items(encrypted).is_empty());
        // 真有可读摘要（裸字符串数组）时上屏。
        let readable = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":"r2","summary_text":["先运行测试"]}}}"#;
        assert!(matches!(
            &parse_chat_items(readable)[0],
            ChatItem::Reasoning { text, .. } if text == "先运行测试"
        ));

        // CommandExecution 是 exec custom_tool_call 的子事件，产出即同一命令上屏两次。
        let exec = r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"e1","command":["pwsh","-Command","ls"],"exit_code":0}}}"#;
        assert!(parse_chat_items(exec).is_empty());
    }

    /// FileChange 是脚本里 apply_patch 的子事件：补丁已由主链的调用带出，再产出就是
    /// 同一次修改显示两遍（走查 A2）。
    #[test]
    fn file_change_item_is_not_emitted() {
        let line = r##"{"timestamp":"t3","type":"event_msg","payload":{"type":"item_completed","item":{"type":"FileChange","id":"fc1","changes":{"/p/README.md":{"type":"add","content":"# 标题\n正文"}}}}}"##;
        assert!(parse_chat_items(line).is_empty());
    }

    /// 本机 0.167.2 实拍的形态：exec 脚本里包着一条命令，回执是命令结果的 JSON。
    /// 工具行要显示真实命令，非零退出要标红并带退出码。
    #[test]
    fn exec_script_is_unwrapped_into_the_real_command() {
        let call = r##"{"timestamp":"2026-11-12T09:06:13.439Z","type":"response_item","payload":{"type":"custom_tool_call","status":"completed","call_id":"call_29b2","name":"exec","input":"text(await tools.exec_command({cmd:\"cargo test -p meowo-agent\",workdir:\"C:/repo\",max_output_tokens:3000}));"}}"##;
        let items = parse_chat_items(call);
        match items.as_slice() {
            [ChatItem::ToolUse { id, name, kind, summary, detail, .. }] => {
                assert_eq!(id, "call_29b2#0");
                assert_eq!(name, "exec_command");
                assert_eq!(*kind, ToolKind::Run);
                assert_eq!(summary, "cargo test -p meowo-agent");
                assert!(matches!(detail, Some(ToolDetail::Command { command, .. }) if command == "cargo test -p meowo-agent"));
            }
            other => panic!("{other:?}"),
        }
        let output = r##"{"timestamp":"2026-11-12T09:06:18.684Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call_29b2","output":[{"type":"input_text","text":"Script completed\nWall time 5.2 seconds\nOutput:\n"},{"type":"input_text","text":"{\"chunk_id\":\"aad648\",\"wall_time_seconds\":1.8,\"exit_code\":101,\"original_token_count\":19,\"output\":\"error: test failed\\n\"}"}]}}"##;
        match parse_chat_items(output).as_slice() {
            [ChatItem::ToolResult { tool_use_id, text, is_error, exit_code, .. }] => {
                assert_eq!(tool_use_id.as_deref(), Some("call_29b2#0"));
                assert_eq!(text, "error: test failed\n");
                assert!(*is_error, "非零退出必须标成失败");
                assert_eq!(*exit_code, Some(101));
            }
            other => panic!("{other:?}"),
        }
    }

    /// 一段脚本连调补丁和命令：各出一行，补丁带逐文件的 diff 与增删行数；回执按 text() 分段对上号。
    #[test]
    fn exec_script_with_patch_and_command_yields_one_row_each() {
        let call = r##"{"timestamp":"t","type":"response_item","payload":{"type":"custom_tool_call","call_id":"c1","name":"exec","input":"text(await tools.apply_patch(\"*** Begin Patch\\n*** Update File: src/a.rs\\n@@\\n-old\\n+new\\n+more\\n*** End Patch\"));\ntext(await tools.exec_command({cmd:\"git diff --stat\"}));"}}"##;
        let items = parse_chat_items(call);
        assert_eq!(items.len(), 2);
        match &items[0] {
            ChatItem::ToolUse { id, name, kind, summary, detail: Some(ToolDetail::Patch { files }), .. } => {
                assert_eq!((id.as_str(), name.as_str(), *kind), ("c1#0", "apply_patch", ToolKind::Edit));
                assert_eq!(summary, "src/a.rs");
                assert_eq!((files[0].added, files[0].removed), (2, 1));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&items[1], ChatItem::ToolUse { id, kind: ToolKind::Run, .. } if id == "c1#1"));
        let output = r##"{"timestamp":"t","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c1","output":[{"type":"input_text","text":"Script completed\nWall time 0.4 seconds\nOutput:\n"},{"type":"input_text","text":"{}"},{"type":"input_text","text":"{\"chunk_id\":\"ea5d29\",\"exit_code\":0,\"output\":\" src/a.rs | 3 ++-\\n\"}"}]}}"##;
        let results = parse_chat_items(output);
        assert!(matches!(
            results.as_slice(),
            [
                ChatItem::ToolResult { tool_use_id: Some(first), is_error: false, .. },
                ChatItem::ToolResult { tool_use_id: Some(second), is_error: false, exit_code: Some(0), text, .. },
            ] if first == "c1#0" && second == "c1#1" && text.contains("src/a.rs")
        ));
    }

    /// 协作类工具不是「派出子任务」；派出子代理的摘要是任务名，不是加密的 message。
    #[test]
    fn collaboration_tools_are_classified_by_table() {
        let kind_of = |name: &str, args: &str| {
            let line = serde_json::json!({"type": "response_item", "payload": {
                "type": "function_call", "name": name, "arguments": args, "call_id": "c"}})
            .to_string();
            match parse_chat_items(&line).as_slice() {
                [ChatItem::ToolUse { kind, summary, .. }] => (*kind, summary.clone()),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(
            kind_of("spawn_agent", r#"{"task_name":"gemini_e2e","fork_turns":"all","message":"gAAAAABq9ddVY9O3"}"#),
            (ToolKind::Agent, "gemini_e2e".to_string())
        );
        assert_eq!(kind_of("wait_agent", r#"{"timeout_ms":10000}"#), (ToolKind::Other, String::new()));
        assert_eq!(kind_of("list_agents", "{}"), (ToolKind::Other, String::new()));
        assert_eq!(
            kind_of("send_message", r#"{"target":"/root","message":"gAAAAABq9YGatHbP"}"#),
            (ToolKind::Other, "/root".to_string())
        );
        assert_eq!(
            kind_of("wait", r#"{"cell_id":"g9:1","yield_time_ms":10000,"max_tokens":12000}"#),
            (ToolKind::Other, "g9:1".to_string())
        );
    }

    #[test]
    fn turn_aborted_becomes_an_interrupted_divider() {
        let line = r#"{"timestamp":"2026-11-12T15:16:16.944Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"01a1d032","reason":"interrupted","duration_ms":1521570}}"#;
        assert!(matches!(
            parse_chat_items(line).as_slice(),
            [ChatItem::Meta { kind, .. }] if kind == "interrupted"
        ));
    }

    /// 会话级「出错」与时间线的错误卡同源：回合带错收尾即置位，新回合开始即清掉。
    #[test]
    fn analysis_parser_reports_turn_error_until_next_turn() {
        let mut parser = CodexParser::default();
        parser.fold_line(r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#);
        assert!(parser.to_info().error.is_none());
        let failed = r#"{"timestamp":"t4","type":"event_msg","payload":{"type":"task_complete","last_agent_message":null,"error":{"message":"Your access token could not be refreshed.","codex_error_info":"unauthorized"}}}"#;
        parser.fold_line(failed);
        let error = parser.to_info().error.expect("回合带错收尾应置位");
        assert_eq!(error.label, "需要重新登录");
        // 时间线上同一行就是那张错误卡，标签一致。
        assert!(matches!(
            parse_chat_items(failed).as_slice(),
            [ChatItem::TurnError { label, .. }] if *label == error.label
        ));
        parser.fold_line(r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}"#);
        assert!(parser.to_info().error.is_none(), "新回合开始后不再报错");
        parser.fold_line(r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"}}"#);
        assert!(parser.to_info().error.is_none());
    }

    #[test]
    fn task_complete_error_becomes_turn_error() {
        let unauthorized = r#"{"timestamp":"t4","type":"event_msg","payload":{"type":"task_complete","last_agent_message":null,"error":{"message":"Your access token could not be refreshed.","codex_error_info":"unauthorized"}}}"#;
        assert!(matches!(
            &parse_chat_items(unauthorized)[0],
            ChatItem::TurnError { label, text, .. }
                if label == "需要重新登录" && text.contains("could not be refreshed")
        ));
        // 正常收尾（error=null）不产出。
        assert!(parse_chat_items(
            r#"{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done","error":null}}"#
        )
        .is_empty());
    }

    /// 回合中途送进去的话与回合开头那条写法相同，只有 turn_id 说明它们同属一个回合
    /// （本机 rollout 01a13d51 的 1962、1966 行，回合 01a13e4c；正文已换成占位）。
    #[test]
    fn user_message_carries_the_turn_id_from_the_record() {
        let line = r#"{"timestamp":"2026-10-15T06:49:36.271Z","ordinal":1965,"type":"event_msg","payload":{"type":"item_completed","thread_id":"01a13d51-8612-7a62-adab-5bdfe4daf61f","turn_id":"01a13e4c-2c55-7831-a8de-d0f6ddfb65d6","item":{"type":"UserMessage","id":"01a13e52-cd0e-7ae0-a8bc-4f7f2f45c10d","client_id":"6b982180-ed27-49ff-b20b-159a3d283a00","content":[{"type":"text","text":"clippy 也报错","text_elements":[]}]},"started_at_ms":1792046976271,"completed_at_ms":1792046976271}}"#;
        assert!(matches!(
            parse_chat_items(line).as_slice(),
            [ChatItem::UserText { turn_id: Some(turn), interjected: false, .. }]
                if turn == "01a13e4c-2c55-7831-a8de-d0f6ddfb65d6"
        ));
        // 老版本的 user_message 事件没有 turn_id。
        let old = r#"{"timestamp":"t1","type":"event_msg","payload":{"type":"user_message","message":"修复测试"}}"#;
        assert!(matches!(
            parse_chat_items(old).as_slice(),
            [ChatItem::UserText { turn_id: None, .. }]
        ));
    }
}
