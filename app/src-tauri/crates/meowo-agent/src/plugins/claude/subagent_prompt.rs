//! 用户切到子代理视图直接发的话（Claude Code 输入框显示「Message @general-purpose…」时）。
//!
//! 2.1.322 实拍（用户 11-11 报「未确认送达」）：这句话**只**写进子代理侧车
//! `<session>/subagents/agent-<id>.jsonl`，形如 isMeta + origin.kind = "human" 的 user 行，
//! 正文被包成「The user sent a new message while you were working:\n<原话>\n\nThis is how…」。
//! 主链同一时刻（晚 4ms）只记一行 queue-operation enqueue：
//! `<task-notification><task-id>id</task-id><summary>Agent "描述" was resumed by the user</summary>…`，
//! 没有正文。对话窗只读主链，回显永远对不上，判成「未确认送达」。
//!
//! 这里按那行通知去对应侧车取回正文，作为一条标了去向的用户消息挂回主时间线。
//! 只在通知行上读侧车：通知一次用户续上一次，读的次数跟着用户发的次数走，不进每轮轮询。
//! 子代理**还在跑**时发给它的话主链是否留痕未取到样本，这条路径覆盖不到（见报告）。

use crate::transcript::{day_millis, SubagentOutcome, TranscriptEvent};
use std::io::BufRead;
use std::path::Path;

const INTERJECTION_HEAD: &str = "The user sent a new message while you were working:\n";
const INTERJECTION_TAIL: &str =
    "\n\nThis is how Claude Code surfaces messages the user sends mid-turn";
const RESUMED_HEAD: &str = "Agent \"";
const RESUMED_TAIL: &str = "\" was resumed by the user";
const WAITING_TAIL: &str = "\" is waiting on its own background work";
/// 侧车那行比主链通知早几毫秒落盘（实拍两例 4ms、5ms）。放宽到 5 秒，再早的是别的一次。
const RESUME_WINDOW_MS: u64 = 5_000;

/// CC 包给模型的插话 → 用户原话。只认 isMeta + origin.kind = "human" 且正文以固定句开头
/// （行首锚定，用户自己贴这段英文不受影响，见 transcript 引用污染）。
pub(super) fn unwrap_interjection(v: &serde_json::Value) -> Option<String> {
    if v.get("isMeta").and_then(|x| x.as_bool()) != Some(true)
        || v.pointer("/origin/kind").and_then(|x| x.as_str()) != Some("human")
    {
        return None;
    }
    let rest = v
        .pointer("/message/content")?
        .as_str()?
        .strip_prefix(INTERJECTION_HEAD)?;
    // 说明附在最后：按最后一次出现切，用户原话里碰巧有同样的句子也不会被截。
    let body = rest
        .rsplit_once(INTERJECTION_TAIL)
        .map_or(rest, |(body, _)| body)
        .trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// 主链上「子代理被用户续上」的排队通知 → 侧车里那句用户原话。
/// None = 不是这种通知，调用方照常解析；Some(空) = 是通知但取不到正文（侧车已清理等），
/// 通知本身不进时间线（与其他 task-notification 入队行一致）。
///
/// 另带一条「又在跑」的结局回执，按 task-id 归回原委派（同 SendMessage 续上，见 transcript 的
/// resumed_agent_id）：收工后被续上，徽标翻回运行中，这一段的完成通知到了再翻成完成。只在入队
/// 这一行给，送达行（queued_command）可能晚于完成通知落盘（见 subagent_resume_tests）。
pub(super) fn resumed_agent_prompt(
    main_transcript: &Path,
    line: &str,
) -> Option<Vec<TranscriptEvent>> {
    // 绝大多数行连这句都没有，先按子串挡掉，省一次 JSON 解析。
    if !line.contains("was resumed by the user") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("queue-operation")
        || v.get("operation").and_then(|x| x.as_str()) != Some("enqueue")
    {
        return None;
    }
    let content = v.get("content").and_then(|x| x.as_str())?;
    if !content.trim_start().starts_with("<task-notification>") {
        return None;
    }
    let agent = super::transcript::tag_text(content, "summary")?
        .strip_prefix(RESUMED_HEAD)?
        .strip_suffix(RESUMED_TAIL)?
        .to_string();
    let at = v.get("timestamp").and_then(|x| x.as_str())?;
    let task_id = super::transcript::tag_text(content, "task-id")?;
    if task_id.is_empty() {
        return Some(Vec::new());
    }
    let running = TranscriptEvent::ToolResult {
        id: format!("resumed:{task_id}:{at}"),
        timestamp: Some(at.to_string()),
        tool_call_id: Some(task_id.to_string()),
        text: format!("{RESUMED_HEAD}{agent}{RESUMED_TAIL}"),
        is_error: false,
        subagent: Some(SubagentOutcome {
            running: 1,
            completed: 0,
            failed: 0,
            background_running: false,
            task_id: Some(task_id.to_string()),
        }),
        patch: None,
    };
    // id 要拼进路径：只收纯 id，不让通知正文把读取引出 subagents 目录。
    if !task_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Some(vec![running]);
    }
    let sidecar = main_transcript
        .with_extension("")
        .join("subagents")
        .join(format!("agent-{task_id}.jsonl"));
    let prompt = find_prompt_before(&sidecar, at).map(|(id, timestamp, text)| {
        TranscriptEvent::SubagentPrompt {
            id,
            timestamp: Some(timestamp),
            text,
            agent,
        }
    });
    Some(prompt.into_iter().chain(std::iter::once(running)).collect())
}

/// 「子代理还在等自己的后台活儿」的通知 → 它的 task-id。锚定方式同 [`resumed_task_id`]。
///
/// 2.1.322 实拍：`<status>completed</status>` + `<summary>Agent "…" is waiting on its own background work</summary>`，
/// note 写「the result below may be interim」、同一 task-id 之后还会再通知；真正收尾的「… finished」晚约 8 分钟，
/// 也有再没通知的（本机一例）。徽标显示「等待后台」，后台计数照常减（见 settle_from_notification）。
pub(super) fn waiting_task_id(content: &str) -> Option<&str> {
    if !content.trim_start().starts_with("<task-notification>") {
        return None;
    }
    super::transcript::tag_text(content, "summary")?
        .strip_prefix(RESUMED_HEAD)?
        .strip_suffix(WAITING_TAIL)?;
    super::transcript::tag_text(content, "task-id").filter(|id| !id.is_empty())
}

/// 「子代理被用户续上」的通知（入队 content / 送达 prompt / user 行正文）→ 它的 task-id。
/// 行首锚定外壳，summary 首尾逐字对上才算（用户自己贴这句英文不受影响，见 transcript 引用污染）。
/// 这种通知没有 `<status>`，不是结局：调用方不能把它当成一次完成。
pub(super) fn resumed_task_id(content: &str) -> Option<&str> {
    if !content.trim_start().starts_with("<task-notification>") {
        return None;
    }
    super::transcript::tag_text(content, "summary")?
        .strip_prefix(RESUMED_HEAD)?
        .strip_suffix(RESUMED_TAIL)?;
    super::transcript::tag_text(content, "task-id").filter(|id| !id.is_empty())
}

/// 侧车里紧挨在 `at` 之前（[`RESUME_WINDOW_MS`] 内）的那句用户插话：(uuid, 时间, 原话)。
fn find_prompt_before(sidecar: &Path, at: &str) -> Option<(String, String, String)> {
    let anchor = day_millis(at)?;
    let file = std::fs::File::open(sidecar).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut buf = Vec::new();
    let mut found = None;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        // 侧车动辄几 MB，只有带这句的行才值得解析。
        if !line.contains("The user sent a new message while you were working") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim_end()) else {
            continue;
        };
        if v.get("type").and_then(|x| x.as_str()) != Some("user") {
            continue;
        }
        let Some(ts) = v.get("timestamp").and_then(|x| x.as_str()) else {
            continue;
        };
        // 同一天、不晚于通知（留 1 秒时钟抖动）、早得不多于窗口。跨零点的极端情况不认。
        let close = ts.get(..10) == at.get(..10)
            && day_millis(ts).is_some_and(|t| {
                t <= anchor + 1_000 && anchor.saturating_sub(t) <= RESUME_WINDOW_MS
            });
        if !close {
            continue;
        }
        let Some(text) = unwrap_interjection(&v) else {
            continue;
        };
        let id = v
            .get("uuid")
            .and_then(|x| x.as_str())
            .unwrap_or(ts)
            .to_string();
        // 窗口里有多条时取最后一条：离通知最近的那条才是触发续上的。
        found = Some((id, ts.to_string(), text));
    }
    found
}

#[cfg(test)]
mod tests {
    use crate::plugins::claude::transcript::{CLAUDE_SUBAGENTS, CLAUDE_TRANSCRIPT};
    use crate::transcript::{read_chat_delta, ChatItem, SubagentSpec};

    /// 2.1.322 实拍形制（脱敏）：发给已收工子代理的话只写进它的侧车；主链同一毫秒级时刻
    /// 只记一行「被用户续上」的排队通知，正文一个字都没有。
    const RESUME_ENQUEUE: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-11T04:38:44.145Z","sessionId":"s","content":"<task-notification>\n<task-id>a0f1</task-id>\n<summary>Agent \"拆分导入任务\" was resumed by the user</summary>\n<note>The user resumed this agent from its transcript view; it is running again.</note>\n</task-notification>"}"#;
    const SIDECAR_PROMPT: &str = r#"{"parentUuid":"p0","isSidechain":true,"agentId":"a0f1","type":"user","message":{"role":"user","content":"The user sent a new message while you were working:\n两边都已经通过了\n\nThis is how Claude Code surfaces messages the user sends mid-turn — within the running turn, often alongside the next tool result, rather than as a separate conversation turn. Address the message above as you continue this turn."},"isMeta":true,"uuid":"side-u1","timestamp":"2026-11-11T04:38:44.141Z","origin":{"kind":"human"}}"#;

    fn session(name: &str, sidecar: Option<&str>) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("claude-subprompt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let subagents = root.join("s/subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let main = root.join("s.jsonl");
        std::fs::write(&main, format!("{RESUME_ENQUEUE}\n")).unwrap();
        if let Some(body) = sidecar {
            std::fs::write(subagents.join("agent-a0f1.jsonl"), format!("{body}\n")).unwrap();
        }
        main
    }

    fn json(item: &ChatItem) -> serde_json::Value {
        serde_json::to_value(item).unwrap()
    }

    /// 回归（用户实拍 11-11）：这句话在主链上找不到，对话窗把回显判成「未确认送达」。
    /// 主链读出来要有这条用户消息，并标明发给了哪个子代理。
    #[test]
    fn resumed_subagent_prompt_surfaces_in_main_chat_with_target() {
        let main = session("main", Some(SIDECAR_PROMPT));
        let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
        let users: Vec<_> = delta
            .items
            .iter()
            .map(json)
            .filter(|v| v["type"] == "user_text")
            .collect();
        assert_eq!(users.len(), 1, "应有一条用户消息：{:?}", delta.items);
        assert_eq!(users[0]["text"], "两边都已经通过了");
        assert_eq!(users[0]["to_agent"], "拆分导入任务");
        assert_eq!(users[0]["timestamp"], "2026-11-11T04:38:44.141Z");
    }

    /// 侧车读不到（已清理 / 还没落盘）就什么也不出，不报错、不冒出通知原文。
    #[test]
    fn missing_sidecar_yields_nothing() {
        let main = session("missing", None);
        let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
        // 续上带的「又在跑」回执照出（见 subagent_resume_tests），用户消息一条都不能有。
        assert!(
            delta.items.iter().all(|i| json(i)["type"] != "user_text"),
            "{:?}",
            delta.items
        );
    }

    /// 时间对不上的侧车消息不认：那是更早一次发给它的话，已经由当时那条通知带出来了。
    #[test]
    fn sidecar_prompt_far_before_resume_is_not_claimed() {
        let old = SIDECAR_PROMPT.replace("04:38:44.141Z", "04:30:00.000Z");
        let main = session("stale", Some(&old));
        let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
        // 续上带的「又在跑」回执照出（见 subagent_resume_tests），用户消息一条都不能有。
        assert!(
            delta.items.iter().all(|i| json(i)["type"] != "user_text"),
            "{:?}",
            delta.items
        );
    }

    /// task-id 拼进路径前必须是纯 id：不能借通知正文跳出 subagents 目录。
    #[test]
    fn task_id_with_path_chars_is_rejected() {
        let main = session("traversal", Some(SIDECAR_PROMPT));
        let evil =
            RESUME_ENQUEUE.replace("<task-id>a0f1</task-id>", "<task-id>..\\\\a0f1</task-id>");
        std::fs::write(&main, format!("{evil}\n")).unwrap();
        let delta = read_chat_delta(&CLAUDE_TRANSCRIPT, &main, 0, None);
        // 续上带的「又在跑」回执照出（见 subagent_resume_tests），用户消息一条都不能有。
        assert!(
            delta.items.iter().all(|i| json(i)["type"] != "user_text"),
            "{:?}",
            delta.items
        );
    }

    /// 子代理面板里这条显示用户原话，不是 CC 套给模型的英文说明。
    #[test]
    fn sidecar_view_shows_user_words_not_wrapper() {
        let items: Vec<ChatItem> = CLAUDE_SUBAGENTS
            .parse_stream_line(SIDECAR_PROMPT)
            .into_iter()
            .map(ChatItem::from)
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(json(&items[0])["text"], "两边都已经通过了");
    }

    /// 用户自己在消息里贴这段英文（非 isMeta、无 origin）不能被改写（transcript 引用污染）。
    #[test]
    fn user_quoting_the_wrapper_is_left_alone() {
        let quoted = r#"{"type":"user","uuid":"u9","message":{"role":"user","content":"The user sent a new message while you were working:\nx"}}"#;
        let items: Vec<ChatItem> = CLAUDE_SUBAGENTS
            .parse_stream_line(quoted)
            .into_iter()
            .map(ChatItem::from)
            .collect();
        assert_eq!(
            json(&items[0])["text"],
            "The user sent a new message while you were working:\nx"
        );
    }
}

#[cfg(test)]
#[path = "subagent_resume_tests.rs"]
mod resume_tests;
