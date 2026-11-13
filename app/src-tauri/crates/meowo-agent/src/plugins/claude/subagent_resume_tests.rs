//! 「子代理被用户续上」的通知不是完成信号（UI 优化收尾 #2）。
//!
//! 实拍形制（本机 transcript，脱敏）：用户在子代理视图里给已收工的子代理发话，主链先记一行
//! queue-operation enqueue，正文 `<summary>Agent "描述" was resumed by the user</summary>`，
//! 没有 `<status>`；主 agent 回合中途吸收时再落一行 attachment/queued_command（同一段正文）。
//! 以前 task_notification_result 把它当成一次完成，子任务徽标提前显示完成。
//!
//! 送达那行可能比子代理这一段的完成通知还晚落盘（本机实拍顺序：enqueue 续上 → enqueue 完成 →
//! user 完成 → queued_command 续上），所以只有入队那一刻算「又在跑」，送达行只吞掉不计。

use crate::plugins::claude::transcript::{analyze_transcript, CLAUDE_TRANSCRIPT};
use crate::transcript::read_chat_delta;

const LAUNCH: &str = r#"{"type":"user","uuid":"l1","timestamp":"2026-11-10T10:00:00.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_bg","content":[{"type":"text","text":"Async agent launched successfully.\nagentId: a6ca15 (internal ID - do not mention to user.)"}]}]}}"#;
const DONE_USER: &str = r#"{"type":"user","uuid":"n1","timestamp":"2026-11-10T10:12:49.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" finished</summary>\n</task-notification>"}}"#;
const RESUME_ENQUEUE: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-10T12:46:41.439Z","sessionId":"s","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<summary>Agent \"做空验证\" was resumed by the user</summary>\n<note>The user resumed this agent from its transcript view; it is running again. Do not treat its work as cancelled or relaunch it; its result arrives in a later task-notification for this task-id.</note>\n</task-notification>"}"#;
const RESUME_QUEUED: &str = r#"{"parentUuid":"p","isSidechain":false,"attachment":{"type":"queued_command","prompt":"<task-notification>\n<task-id>a6ca15</task-id>\n<summary>Agent \"做空验证\" was resumed by the user</summary>\n<note>The user resumed this agent from its transcript view; it is running again. Do not treat its work as cancelled or relaunch it; its result arrives in a later task-notification for this task-id.</note>\n</task-notification>","commandMode":"task-notification","origin":{"kind":"task-notification","producer":"session-task"},"timestamp":"2026-11-10T12:46:41.439Z"},"type":"attachment","uuid":"q1","timestamp":"2026-11-10T12:46:41.439Z"}"#;
const DONE_AGAIN_ENQUEUE: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-10T12:50:22.041Z","sessionId":"s","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" finished</summary>\n</task-notification>"}"#;
const DONE_AGAIN_USER: &str = r#"{"type":"user","uuid":"n2","timestamp":"2026-11-10T12:50:22.063Z","message":{"role":"user","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" finished</summary>\n</task-notification>"}}"#;

fn write(name: &str, lines: &[&str]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("claude-resume-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let main = dir.join("s.jsonl");
    std::fs::write(
        &main,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
    .unwrap();
    main
}

/// 委派 toolu_bg 的最终结局 (running, completed, failed)：与前端 collectSubagentReceipts 同一规则
/// （同 task_id 首见的回执是委派本体，后到的归回它，后到者覆盖）。
fn outcome_of(main: &std::path::Path) -> (u64, u64, u64) {
    let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, main, 0, None);
    let mut owners = std::collections::HashMap::<String, String>::new();
    let mut outcomes = std::collections::HashMap::<String, serde_json::Value>::new();
    for item in &delta.items {
        let v = serde_json::to_value(item).unwrap();
        let (Some(tool), Some(sub)) = (
            v["tool_use_id"].as_str(),
            v.get("subagent").filter(|s| !s.is_null()),
        ) else {
            continue;
        };
        let mut owner = tool.to_string();
        if let Some(task) = sub["task_id"].as_str() {
            owner = owners.entry(task.to_string()).or_insert(owner).clone();
        }
        outcomes.insert(owner, sub.clone());
    }
    let sub = &outcomes["toolu_bg"];
    let n = |k: &str| sub[k].as_u64().unwrap_or(0);
    (n("running"), n("completed"), n("failed"))
}

/// 回归：送达形态（queued_command）的续上通知以前被记成一次完成。续上不是完成，委派仍在跑。
#[test]
fn queued_resume_notification_is_not_completion() {
    let main = write("queued", &[LAUNCH, RESUME_QUEUED]);
    assert_eq!(outcome_of(&main), (1, 0, 0));
    // 通知原文也不能以用户消息冒出来。
    let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
    assert!(
        delta
            .items
            .iter()
            .all(|i| serde_json::to_value(i).unwrap()["type"] != "user_text"),
        "{:?}",
        delta.items
    );
}

/// 收工后被续上：入队那一刻翻回运行中，这一段的完成通知到了再翻成完成。
#[test]
fn resume_after_finish_runs_again_until_next_completion() {
    let main = write("rerun", &[LAUNCH, DONE_USER, RESUME_ENQUEUE]);
    assert_eq!(outcome_of(&main), (1, 0, 0));
    assert_eq!(analyze_transcript(main.to_str().unwrap()).busy_subagents, 1);
    // 本机实拍顺序：送达行比这一段的完成通知还晚。不能被它翻回运行中、永远挂着。
    let main = write(
        "late",
        &[
            LAUNCH,
            DONE_USER,
            RESUME_ENQUEUE,
            DONE_AGAIN_ENQUEUE,
            DONE_AGAIN_USER,
            RESUME_QUEUED,
        ],
    );
    assert_eq!(outcome_of(&main), (0, 1, 0));
    assert_eq!(analyze_transcript(main.to_str().unwrap()).busy_subagents, 0);
}

/// 用户自己贴这句英文（无 task-notification 外壳）照常是用户消息（transcript 引用污染）。
#[test]
fn quoted_resume_phrase_stays_user_text() {
    let quoted = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"Agent \"x\" was resumed by the user 是什么意思"}}"#;
    let main = write("quoted", &[quoted]);
    let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
    assert_eq!(
        serde_json::to_value(&delta.items[0]).unwrap()["type"],
        "user_text"
    );
}

// ═══ 「还在等自己的后台活儿」不是收尾（做空第三轮 #7）═══
// 本机实拍（2.1.322，脱敏）：子代理自己派了后台任务就停下时，主链收到 status=completed 的通知，
// summary 是「Agent "…" is waiting on its own background work」，note 写明结果可能只是中间态、
// 同一 task-id 之后还会再通知。真正收尾的是后来那条「… finished」（实拍晚约 8 分钟）。

const WAITING_NOTE: &str = "This agent stopped with background work of its own still running. It may resume on its own when that work completes or reports, and the same task-id notifies again if it does; the result below may be interim.";

fn waiting_user(uuid: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","message":{{"role":"user","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" is waiting on its own background work</summary>\n<note>{WAITING_NOTE}</note>\n<result>中间结论</result>\n</task-notification>"}}}}"#
    )
}

fn waiting_enqueue(ts: &str) -> String {
    format!(
        r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"{ts}","sessionId":"s","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" is waiting on its own background work</summary>\n<note>{WAITING_NOTE}</note>\n</task-notification>"}}"#
    )
}

fn waiting_queued(ts: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"attachment":{{"type":"queued_command","prompt":"<task-notification>\n<task-id>a6ca15</task-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" is waiting on its own background work</summary>\n<note>{WAITING_NOTE}</note>\n</task-notification>","commandMode":"task-notification","origin":{{"kind":"task-notification"}},"timestamp":"{ts}"}},"type":"attachment","uuid":"wq","timestamp":"{ts}"}}"#
    )
}

/// 委派 toolu_bg 最终结局上有没有「后台仍在运行」标（与 outcome_of 同一套路由）。
fn background_of(main: &std::path::Path) -> bool {
    let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, main, 0, None);
    let mut owners = std::collections::HashMap::<String, String>::new();
    let mut last = std::collections::HashMap::<String, bool>::new();
    for item in &delta.items {
        let v = serde_json::to_value(item).unwrap();
        let (Some(tool), Some(sub)) = (
            v["tool_use_id"].as_str(),
            v.get("subagent").filter(|s| !s.is_null()),
        ) else {
            continue;
        };
        let mut owner = tool.to_string();
        if let Some(task) = sub["task_id"].as_str() {
            owner = owners.entry(task.to_string()).or_insert(owner).clone();
        }
        last.insert(owner, sub["background_running"].as_bool().unwrap_or(false));
    }
    last.get("toolu_bg").copied().unwrap_or(false)
}

/// 做空第二轮定：结论已交就按完成算，次行提示「后台仍在运行」；不算进行中——本机实拍 9 次里 4 次
/// 之后再无 finished，挂进行中（或「等待后台」）会永远占着面板。后台计数照常减。
#[test]
fn waiting_on_own_background_work_is_done_with_background_note() {
    let (user, enqueue, queued) = (
        waiting_user("w1", "2026-11-10T10:05:00.000Z"),
        waiting_enqueue("2026-11-10T10:05:00.000Z"),
        waiting_queued("2026-11-10T10:05:00.000Z"),
    );
    // 送达形态（user 行 / queued_command）：完成 + 后台仍在运行，后台计数 0；再无 finished 时就停在这里。
    for (name, line) in [("user", &user), ("queued", &queued)] {
        let main = write(&format!("waiting-{name}"), &[LAUNCH, line]);
        assert_eq!(outcome_of(&main), (0, 1, 0), "{name} 形态按完成算");
        assert!(background_of(&main), "{name} 形态带「后台仍在运行」");
        assert_eq!(
            analyze_transcript(main.to_str().unwrap()).busy_subagents,
            0,
            "{name} 形态照常减后台计数"
        );
    }
    // 只有入队、再无送达也再无 finished：会话不卡在「后台在跑」。
    let main = write("waiting-enqueue-only", &[LAUNCH, &enqueue]);
    assert_eq!(analyze_transcript(main.to_str().unwrap()).busy_subagents, 0);
    // 随后 finished 到了：完成，次行消失。
    let main = write("waiting-then-done", &[LAUNCH, &enqueue, &user, DONE_USER]);
    assert_eq!(outcome_of(&main), (0, 1, 0));
    assert!(!background_of(&main));
    assert_eq!(analyze_transcript(main.to_str().unwrap()).busy_subagents, 0);
}

// ═══ SendMessage 续上后的完成通知归回原委派（做空第三轮 #8）═══
// 本机实拍（xbot 会话，脱敏）：SendMessage 续上收工的子代理后，那一段的完成通知 <tool-use-id> 指向
// SendMessage 那次调用，不是最初的委派。以前有 tool-use-id 就不带 task_id，结局记到 SendMessage 名下，
// 原委派被 SendMessage 回执翻回运行中后就一直挂着（实拍 02:44 挂到 04:39）。

const SEND_RESUME: &str = r#"{"type":"user","uuid":"sm","timestamp":"2026-11-10T11:00:00.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_sm","content":[{"type":"text","text":"{\"success\":true,\"message\":\"Resuming agent a6ca15\",\"resumedAgentId\":\"a6ca15\"}"}]}]}}"#;
const DONE_VIA_SEND: &str = r#"{"type":"user","uuid":"n3","timestamp":"2026-11-10T11:20:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>a6ca15</task-id>\n<tool-use-id>toolu_sm</tool-use-id>\n<status>completed</status>\n<summary>Agent \"做空验证\" finished</summary>\n</task-notification>"}}"#;

#[test]
fn completion_after_send_message_resume_settles_original_delegation() {
    let main = write("send-resume", &[LAUNCH, DONE_USER, SEND_RESUME]);
    assert_eq!(outcome_of(&main), (1, 0, 0), "SendMessage 续上后原委派在跑");
    let main = write(
        "send-resume-done",
        &[LAUNCH, DONE_USER, SEND_RESUME, DONE_VIA_SEND],
    );
    assert_eq!(outcome_of(&main), (0, 1, 0), "完成通知要归回原委派");
    assert_eq!(analyze_transcript(main.to_str().unwrap()).busy_subagents, 0);
}
