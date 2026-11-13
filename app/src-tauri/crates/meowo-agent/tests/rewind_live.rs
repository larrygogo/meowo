//! 用**本机真实 claude 会话**核对回退分叉的判定（ChatLineMark::Link）。数据不存在时自动跳过。
//!
//! 存在的理由：分叉判据是「用户提示的 parentUuid 跳回更早的节点、且其后出现过用户提示」。
//! 合成用例证明不了真实 transcript 里没有别的结构也长这样（附件、hook、压缩、排队消息都
//! 带 parentUuid）——误判的代价是把正常对话从对话页上抹掉，必须拿真实数据兜底。

use meowo_agent::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
use meowo_agent::{ChatItem, TranscriptSpec};
use std::path::PathBuf;
use std::time::Instant;

fn claude_transcripts() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(home.join(".claude/projects")) else {
        return out;
    };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        out.extend(
            files
                .flatten()
                .map(|f| f.path())
                .filter(|p| p.extension().is_some_and(|e| e == "jsonl")),
        );
    }
    out
}

/// 整读每份真实 transcript：判出分叉的逐份打印被收回的用户提示，并量最大那份的耗时。
/// 人工核对用（--nocapture）；断言只兜「整读不会把全部用户消息收光」。
#[test]
#[ignore = "读本机真实 transcript；手动核对用"]
fn real_transcripts_fork_retractions_are_rewinds() {
    let files = claude_transcripts();
    let mut forked = 0;
    let mut slowest = (0u128, PathBuf::new());
    for path in &files {
        let users = |items: &[ChatItem]| {
            items
                .iter()
                .filter_map(|item| match item {
                    ChatItem::UserText { text, .. } => {
                        Some(text.chars().take(30).collect::<String>())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let started = Instant::now();
        let delta = meowo_agent::read_chat_delta_paged(&CLAUDE_TRANSCRIPT, path, 0, None, 0);
        let took = started.elapsed().as_millis();
        if took > slowest.0 {
            slowest = (took, path.clone());
        }
        if !delta.undone {
            continue;
        }
        forked += 1;
        // 逐行朴素解析（不判分叉）作对照，差集就是被收回的用户提示。
        let body = std::fs::read_to_string(path).unwrap_or_default();
        let naive: Vec<String> = body
            .lines()
            .flat_map(|line| {
                CLAUDE_TRANSCRIPT
                    .parse_transcript_line(line)
                    .into_iter()
                    .map(ChatItem::from)
            })
            .filter_map(|item| match item {
                ChatItem::UserText { text, .. } => Some(text.chars().take(30).collect::<String>()),
                _ => None,
            })
            .collect();
        let kept = users(&delta.items);
        let mut gone = naive.clone();
        for text in &kept {
            if let Some(i) = gone.iter().position(|g| g == text) {
                gone.remove(i);
            }
        }
        eprintln!(
            "[fork] {}\n       保留 {} 条，收回 {:?}",
            path.display(),
            kept.len(),
            gone
        );
        assert!(!kept.is_empty(), "整读把用户消息收光了：{}", path.display());
    }
    // 判分叉的额外开销：每行多一次轻量反序列化（只取 uuid/parentUuid/isSidechain）。
    let body = std::fs::read_to_string(&slowest.1).unwrap_or_default();
    let started = Instant::now();
    let links = body
        .lines()
        .filter_map(|line| CLAUDE_TRANSCRIPT.chat_line_mark(line))
        .count();
    eprintln!("[fork] {} 份里判出分叉 {forked} 份；最慢整读 {}ms {}，其中判分叉的节点解析 {}ms（{links} 个节点，{} 字节）",
        files.len(), slowest.0, slowest.1.display(), started.elapsed().as_millis(), body.len());
}

/// codex 回溯（thread_rolled_back）在真实 rollout 上收回的用户消息，人工核对用。
#[test]
#[ignore = "读本机真实 rollout；手动核对用"]
fn real_codex_rollbacks_retract_the_right_turns() {
    use meowo_agent::plugins::codex::telemetry::CODEX_TRANSCRIPT;
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return;
    };
    let mut stack = vec![home.join(".codex/sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            if !body.contains("\"thread_rolled_back\"") {
                continue;
            }
            let users = |items: Vec<ChatItem>| -> Vec<String> {
                items
                    .into_iter()
                    .filter_map(|item| match item {
                        ChatItem::UserText { text, .. } => Some(text.chars().take(30).collect()),
                        _ => None,
                    })
                    .collect()
            };
            let naive = users(
                body.lines()
                    .flat_map(|line| {
                        CODEX_TRANSCRIPT
                            .parse_transcript_line(line)
                            .into_iter()
                            .map(ChatItem::from)
                    })
                    .collect(),
            );
            let kept = users(
                meowo_agent::read_chat_delta_paged(&CODEX_TRANSCRIPT, &path, 0, None, 0).items,
            );
            let mut gone = naive.clone();
            for text in &kept {
                if let Some(i) = gone.iter().position(|g| g == text) {
                    gone.remove(i);
                }
            }
            eprintln!(
                "[codex] {}\n        保留 {} 条，收回 {gone:?}",
                path.display(),
                kept.len()
            );
        }
    }
}

/// codex 分页历史的续写段（回溯后另起的 `…_<段id>.jsonl`）：从根段路径出发解析，应读到
/// 「根段前缀 + 续写段」，被回溯掉的那轮不在其中。人工核对用。
#[test]
#[ignore = "读本机真实 rollout；手动核对用"]
fn real_codex_continuation_segments_compose() {
    use meowo_agent::plugins::codex::telemetry::CODEX_TRANSCRIPT;
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
    else {
        return;
    };
    let mut stack = vec![home.join(".codex/sessions")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            // 根段文件名：rollout-<19 位时间>-<线程id>.jsonl；有同线程续写段的才看。
            let Some(tid) = name
                .strip_prefix("rollout-")
                .and_then(|n| n.get(20..))
                .and_then(|n| n.strip_suffix(".jsonl"))
            else {
                continue;
            };
            if tid.contains('_')
                || !std::fs::read_dir(&dir).unwrap().flatten().any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .contains(&format!("-{tid}_"))
                })
            {
                continue;
            }
            let resolved = CODEX_TRANSCRIPT
                .resolve_transcript_path(Some(&path.to_string_lossy()), None, tid)
                .unwrap();
            let users: Vec<String> =
                meowo_agent::read_chat_delta_paged(&CODEX_TRANSCRIPT, &resolved, 0, None, 0)
                    .items
                    .into_iter()
                    .filter_map(|item| match item {
                        ChatItem::UserText { text, .. } => Some(text.chars().take(30).collect()),
                        _ => None,
                    })
                    .collect();
            eprintln!(
                "[segment] {name}\n          → {}\n          用户消息 {users:?}",
                resolved.display()
            );
        }
    }
}
