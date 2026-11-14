//! Kimi 2.6 的「这一下是不是子代理干的」判定（任务 .lead/20261113-lead-tui，第二轮真机 D 的产品问题 1）。
//!
//! 为什么要读 wire：kimi 2.6.0 的 hook 载荷里**没有** agent 身份。kimi.exe 内嵌源码
//! `agentExternalHooksService.runPreToolUse` 只带 `sessionId`（主会话与子代理共用同一个）、
//! `toolName/toolInput/toolCallId` 与 `sessionTitle`；`scopeContext.agentId` 只用在 UI 事件里，不进载荷
//! （2026-11-13 读本机 2.6.0 二进制）。所以 reporter 原来「带 agent_id 的一律忽略」对 kimi 形同虚设：
//! 子代理写挂起关卡时照样被要求启动等待器，它就自己在后台跑了，答复通知进了子代理的 loop（run5 D）。
//!
//! kimi 自己的落盘里有身份：会话目录 `agents/<agentId>/wire.jsonl` 一个 agent 一份
//! （`agents/main` 是主 agent，源码 context_recovery 文案原文 "one file per agent"）。
//! - 主 wire 记子代理生命周期（`subagent.spawned` 带 `runInBackground`，`subagent.completed/failed/
//!   cancelled` 收尾）和主 agent 的回合起止（`turn.prompt` / `turn.ended`），据此判断 PreToolUse 时
//!   写关卡的是谁：前台子代理在跑时主 agent 正卡在 Agent 工具里，写关卡的只能是子代理；只有后台子代理、
//!   而主 agent 回合已结束时同理。
//! - 每个 agent 的后台任务记在它自己的 wire（`task.started` 是 AgentEvent2，带 `agentId` 与 `info.pid`）：
//!   等待器沿自己的父进程链去对 pid，就知道是谁启动了自己。
//!
//! 读不到 / 格式对不上一律返回「不知道」，调用方按原行为放行：宁可漏判，不误伤主 agent。

use serde_json::Value;
use std::path::{Path, PathBuf};

/// 每个 wire 只读末尾这么多：要找的都是刚写下的记录，而 wire 可能有几十 MB。
const TAIL_BYTES: u64 = 512 * 1024;

/// 会话目录下的 `agents/`（主 wire 的上两级）。
fn agents_dir(session: &str) -> Option<PathBuf> {
    let spec = meowo_agent::by_id("kimi")?.telemetry()?.transcript()?;
    let main = spec.resolve_transcript_path(None, None, session)?;
    Some(main.parent()?.parent()?.to_path_buf())
}

/// 读文件末尾（从完整行开始）。
fn tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let from = size.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Some(if from > 0 {
        text.split_once('\n').map(|(_, rest)| rest.to_owned())?
    } else {
        text
    })
}

/// PreToolUse 时写挂起关卡的是不是子代理（读主 wire）。
pub fn writer_is_subagent(session: &str) -> bool {
    agents_dir(session)
        .and_then(|dir| tail(&dir.join("main").join("wire.jsonl")))
        .is_some_and(|text| subagent_is_writing(&text))
}

/// 主 wire 末尾 → 此刻能写工具调用的是不是只剩子代理。
///
/// - 有前台子代理在跑：主 agent 卡在 Agent 工具里（kimi 前台子代理是同步等待的），不会发工具调用；
/// - 只有后台子代理在跑、且主 agent 没有进行中的回合：同理。
///
/// 已知不覆盖：主 agent 同一步里并发发出 Agent 调用和写关卡（极少见），会被当成子代理拦下，
/// 模型看到原因后重写即可，代价是一次重试。
pub fn subagent_is_writing(main_wire: &str) -> bool {
    // subagentId → 是否在前台（主 agent 在等它）。被通知唤醒的旧子代理（completed 之后又 started）
    // 不再挡着主 agent，按后台算。
    let mut running: Vec<(String, bool)> = Vec::new();
    let mut finished: Vec<String> = Vec::new();
    let mut main_busy = false;
    for line in main_wire.lines() {
        // 先按子串粗筛，免得逐行解析几十万字的 wire。
        if !(line.contains("\"subagent.") || line.contains("\"turn.")) {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = event["subagentId"].as_str().unwrap_or_default().to_owned();
        match event["type"].as_str().unwrap_or_default() {
            "subagent.spawned" => {
                // 只管主 agent 派出的；子代理再派的孙代理不改变主 agent 的状态。
                let parent = event["parentAgentId"].as_str().unwrap_or("main");
                if parent == "main" {
                    let foreground = !event["runInBackground"].as_bool().unwrap_or(false);
                    running.retain(|(r, _)| *r != id);
                    running.push((id, foreground));
                }
            }
            "subagent.started" => {
                if finished.contains(&id) && !running.iter().any(|(r, _)| *r == id) {
                    running.push((id, false));
                }
            }
            "subagent.completed" | "subagent.failed" | "subagent.cancelled" => {
                running.retain(|(r, _)| *r != id);
                finished.push(id);
            }
            "turn.prompt" if event["agentId"].as_str().unwrap_or("main") == "main" => {
                main_busy = true;
            }
            "turn.ended" if event["agentId"].as_str().unwrap_or("main") == "main" => {
                main_busy = false;
                // 主回合结束时它等着的前台子代理必然已收尾（没记到也不再算前台）。
                for entry in &mut running {
                    entry.1 = false;
                }
            }
            _ => {}
        }
    }
    running.iter().any(|(_, foreground)| *foreground) || (!running.is_empty() && !main_busy)
}

/// 本进程的父进程链（含自己），最多 16 层。
pub fn ancestors() -> Vec<u32> {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System};
    let sys =
        System::new_with_specifics(RefreshKind::new().with_processes(ProcessRefreshKind::new()));
    let mut chain = vec![std::process::id()];
    let Ok(mut cur) = sysinfo::get_current_pid() else {
        return chain;
    };
    for _ in 0..16 {
        let Some(parent) = sys.process(cur).and_then(|p| p.parent()) else {
            break;
        };
        chain.push(parent.as_u32());
        cur = parent;
    }
    chain
}

/// 等待器是谁启动的：Some(true) = 子代理，Some(false) = 主 agent，None = 认不出（wire 还没落盘、
/// 格式变了、不是 kimi）。
pub fn launched_by_subagent(session: &str, chain: &[u32]) -> Option<bool> {
    let dir = agents_dir(session)?;
    let wires: Vec<(String, String)> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let text = tail(&entry.path().join("wire.jsonl"))?;
            Some((entry.file_name().to_string_lossy().into_owned(), text))
        })
        .collect();
    launcher_in(&wires, chain)
}

/// 在各 agent 的 wire 里找「pid 落在我父进程链上」的 `task.started`，返回它是不是子代理的。
/// 目录名与记录里的 agentId 任一说不是 main 就算子代理。
pub fn launcher_in(wires: &[(String, String)], chain: &[u32]) -> Option<bool> {
    let mut found = None;
    for (agent, text) in wires {
        for line in text.lines() {
            if !line.contains("\"task.started\"") {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let Some(pid) = event.pointer("/info/pid").and_then(Value::as_u64) else {
                continue;
            };
            if chain.contains(&(pid as u32)) {
                let id = event["agentId"].as_str().unwrap_or(agent);
                found = Some(agent != "main" || id != "main");
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 形状取自第二轮真机 run1 的主 wire（kimi 2.6.0）：前台 coder 子代理在跑 = 写关卡的是子代理。
    #[test]
    fn foreground_subagent_running_means_the_writer_is_the_subagent() {
        let base = vec![
            json!({"type":"turn.prompt","agentId":"main","origin":{"kind":"user"},"turnId":3}),
            json!({"type":"subagent.spawned","subagentId":"agent-0","subagentName":"coder","parentToolCallId":"tool_x","parentAgentId":"main","callerAgentId":"main","runInBackground":false}),
            json!({"type":"subagent.started","subagentId":"agent-0"}),
        ];
        assert!(subagent_is_writing(&wire(&base)));
        let mut done = base.clone();
        done.push(json!({"type":"subagent.completed","subagentId":"agent-0","resultSummary":"ok"}));
        assert!(!subagent_is_writing(&wire(&done)), "子代理已收尾，写的是主 agent");
        // 没有子代理的普通回合。
        assert!(!subagent_is_writing(&wire(&base[..1])));
        assert!(!subagent_is_writing(""));
    }

    /// 后台子代理：主 agent 回合进行中 = 可能是主 agent 写的（放行）；主回合已结束 = 只能是子代理。
    #[test]
    fn background_subagent_counts_only_when_main_is_idle() {
        let mut events = vec![
            json!({"type":"turn.prompt","agentId":"main","turnId":1}),
            json!({"type":"subagent.spawned","subagentId":"agent-1","parentAgentId":"main","runInBackground":true}),
            json!({"type":"subagent.started","subagentId":"agent-1"}),
        ];
        assert!(!subagent_is_writing(&wire(&events)));
        events.push(json!({"type":"turn.ended","agentId":"main","turnId":1,"reason":"completed"}));
        assert!(subagent_is_writing(&wire(&events)));
        // 子代理被后台任务通知重新唤醒（completed 之后又 started）：不挡主 agent，按后台算。
        let resumed = wire(&[
            json!({"type":"subagent.spawned","subagentId":"agent-0","parentAgentId":"main","runInBackground":false}),
            json!({"type":"subagent.completed","subagentId":"agent-0"}),
            json!({"type":"turn.ended","agentId":"main"}),
            json!({"type":"subagent.started","subagentId":"agent-0"}),
            json!({"type":"turn.prompt","agentId":"main"}),
        ]);
        assert!(!subagent_is_writing(&resumed), "主 agent 在回合里");
        // 孙代理（子代理派的）不改变判定。
        let nested = wire(&[
            json!({"type":"turn.prompt","agentId":"main"}),
            json!({"type":"subagent.spawned","subagentId":"agent-9","parentAgentId":"agent-0","runInBackground":false}),
        ]);
        assert!(!subagent_is_writing(&nested));
    }

    /// 等待器按 pid 对上 `task.started`：在子代理 wire 里 = 子代理启动的；在主 wire 里 = 主 agent。
    #[test]
    fn launcher_is_found_by_pid_in_the_agent_wires() {
        let main = wire(&[
            json!({"type":"task.started","agentId":"main","info":{"taskId":"bash-1","pid":111,"command":"x lead wait"}}),
        ]);
        let sub = wire(&[
            json!({"type":"task.started","agentId":"agent-0","info":{"taskId":"bash-2","pid":222,"command":"x lead wait"}}),
        ]);
        let wires = vec![("main".to_owned(), main), ("agent-0".to_owned(), sub)];
        assert_eq!(launcher_in(&wires, &[9, 111, 5]), Some(false));
        assert_eq!(launcher_in(&wires, &[9, 222, 5]), Some(true));
        assert_eq!(launcher_in(&wires, &[9, 333]), None, "认不出就不下结论");
        assert_eq!(launcher_in(&[], &[1]), None);
    }
}
