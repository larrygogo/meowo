//! 五家回放测试：每家一份本机真实会话的摘录（`tests/fixtures/chat-replay/<家>.jsonl`，已脱敏），
//! 走与对话页相同的读取入口（[`read_chat_delta`]）解析成 ChatItem，与入库的快照
//! （`<家>.items.json`）逐字比对。
//!
//! 它拦的是这类回归：接一个新 CLI 版本、改一处解析或归类，五家里任何一家的时间线变了都会
//! 在这里显形。前端另有一份测试（`app/src/views/chat/replay.test.tsx`）读同一批快照，
//! 接着走归并与渲染，断言到用户看见的文案与状态——两头用同一份数据接起来。
//!
//! 样本怎么来的：从本机真实会话里只摘覆盖各类块所需的记录，保持原顺序；用户主目录的用户名
//! 换成 `user`，邮箱、内网地址、加密正文换成占位，过长的输出截短（标了「样本截短」）；
//! claude 摘录的 parentUuid 重新串成一条链（摘录必然断链）。除此之外不改内容，也不手编记录。
//! 每家缺哪类块、为什么缺，写在 [`replay_samples_cover_the_expected_blocks`] 的期望表旁边。
//!
//! 改了解析后更新快照：`MEOWO_UPDATE_REPLAY=1 cargo test -p meowo-agent --lib replay_`。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::transcript::{ChatItem, ToolKind, TranscriptSpec};

const PROVIDERS: [&str; 5] = ["claude", "kimi", "codex", "gemini", "opencode"];

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/chat-replay")
}

fn spec(agent: &str) -> &'static dyn TranscriptSpec {
    crate::registry::by_id(agent)
        .and_then(|plugin| plugin.telemetry())
        .and_then(|telemetry| telemetry.transcript())
        .unwrap_or_else(|| panic!("{agent} 没有 transcript 能力"))
}

/// 一家的样本走完整解析。
/// - claude / kimi / codex：样本就是 CLI 写的会话文件的摘录。
/// - gemini：样本是 CLI 的原始日志（带 `$set` 行），先经插件自己的物化（归并成稳定快照）再读，
///   与应用里的路径一致；物化到临时目录，不碰用户数据目录。
/// - opencode：对话在它的 SQLite 库里，样本是 Meowo 导出器对真实库的导出行（对话页读的
///   就是导出文件）；库 → 导出行这一步由 `plugins::opencode::store` 自己的测试覆盖。
fn replay(agent: &str) -> Vec<ChatItem> {
    let delta = crate::read_chat_delta(spec(agent), &sample_path(agent), 0, None);
    assert!(!delta.items.is_empty(), "{agent} 样本没解析出任何条目");
    delta.items
}

/// 对话页实际读的那个文件：gemini 是物化后的快照，其余就是样本本身。
fn sample_path(agent: &str) -> PathBuf {
    let source = fixture_dir().join(format!("{agent}.jsonl"));
    assert!(source.is_file(), "缺样本 {}", source.display());
    if agent == "gemini" {
        // 物化只做一次：几个测试并行跑，各自清目录、各自物化会互相踩。
        static MATERIALIZED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        MATERIALIZED
            .get_or_init(|| {
                let dir = std::env::temp_dir()
                    .join(format!("meowo_replay_gemini_{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                crate::plugins::gemini::materialize_for_test(&source, &dir)
                    .expect("gemini 样本物化失败")
            })
            .clone()
    } else {
        source
    }
}

fn snapshot_text(items: &[ChatItem]) -> String {
    let mut text = serde_json::to_string_pretty(items).unwrap();
    text.push('\n');
    text
}

/// 时间线里出现了哪些要覆盖的块。
fn blocks(items: &[ChatItem]) -> BTreeSet<&'static str> {
    let mut seen = BTreeSet::new();
    for item in items {
        match item {
            ChatItem::ToolUse { kind, .. } => {
                seen.insert(match kind {
                    ToolKind::Read => "读",
                    ToolKind::Write => "写",
                    ToolKind::Edit => "改",
                    ToolKind::Run => "命令",
                    ToolKind::Search => "搜索",
                    ToolKind::Agent => "子任务",
                    ToolKind::Web => "网页",
                    ToolKind::Todo => "待办",
                    ToolKind::Other => "其他",
                });
            }
            ChatItem::ToolResult { is_error: true, .. } => {
                seen.insert("失败回执");
            }
            ChatItem::TurnError { .. } => {
                seen.insert("出错");
            }
            ChatItem::Meta { kind, .. } if kind == "interrupted" => {
                seen.insert("中断");
            }
            _ => {}
        }
    }
    seen
}

#[test]
fn replay_five_providers_match_snapshots() {
    let update = std::env::var_os("MEOWO_UPDATE_REPLAY").is_some();
    let mut drifted = Vec::new();
    for agent in PROVIDERS {
        let actual = snapshot_text(&replay(agent));
        let path = fixture_dir().join(format!("{agent}.items.json"));
        if update {
            std::fs::write(&path, &actual).unwrap();
            continue;
        }
        // 工作区可能是 CRLF（core.autocrlf），比对前统一换行。
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if expected != actual {
            drifted.push(agent);
        }
    }
    assert!(
        drifted.is_empty(),
        "这几家的回放结果与快照不一致：{drifted:?}。确认是有意的改动后，用 \
         MEOWO_UPDATE_REPLAY=1 cargo test -p meowo-agent --lib replay_ 更新快照，并同步更新前端的 \
         replay.test.tsx 快照。"
    );
}

/// 每家样本覆盖了哪些块，逐家钉死。少一类 = 样本或解析退化；多一类 = 该把说明更新了。
/// 缺的类别是本机真实数据里没有、不能手编的，原因逐条写在表里。
#[test]
fn replay_samples_cover_the_expected_blocks() {
    let expected: [(&str, &[&str]); 5] = [
        (
            "claude",
            &["读", "写", "改", "命令", "搜索", "子任务", "失败回执", "出错", "中断"],
        ),
        (
            "kimi",
            &["读", "写", "改", "命令", "搜索", "子任务", "失败回执", "出错", "中断"],
        ),
        // 缺「读」：codex 没有读文件工具，读文件就是跑命令。缺「搜索」同理。
        // 缺「出错」：本机 68 份 rollout 里没有一次 task_complete 带 error。
        (
            "codex",
            &["写", "改", "命令", "网页", "子任务", "其他", "失败回执", "中断"],
        ),
        // 缺「子任务」：本机 Gemini 会话没有派过子任务。
        (
            "gemini",
            &["读", "写", "改", "命令", "搜索", "其他", "失败回执", "出错", "中断"],
        ),
        // 缺「搜索」「子任务」：搜索只出现在一个私有项目的会话里（不能入库），task 一次都没有。
        (
            "opencode",
            &["读", "写", "改", "命令", "网页", "失败回执", "出错", "中断"],
        ),
    ];
    for (agent, want) in expected {
        let have = blocks(&replay(agent));
        let want: BTreeSet<&str> = want.iter().copied().collect();
        assert_eq!(have, want, "{agent} 样本覆盖的块变了");
    }
}

/// 回合出错时，会话级「出错」（侧栏状态点、灵动岛、通知读的分析口径）与时间线上的错误卡
/// 说的是同一件事：把真实样本喂到出错那一行为止，分析解析器报的错误标签就是错误卡的标签。
///
/// 五家都声明了分析能力——此前 codex 与 opencode 没有，回合出错只出现在时间线里，
/// 侧栏写着「等待输入」、也不弹通知（走查 A8）。codex 本机没有回合出错的真实记录，
/// 它的判据由 `plugins::codex::telemetry` 里按 rollout 记录形态写的单测覆盖。
#[test]
fn replay_session_error_matches_the_timeline_error_card() {
    for agent in PROVIDERS {
        assert!(
            spec(agent).supports_analysis(),
            "{agent} 没有分析能力：它的回合出错进不了侧栏、灵动岛和通知"
        );
    }
    for agent in ["claude", "kimi", "gemini", "opencode"] {
        let spec = spec(agent);
        let path = sample_path(agent);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let (at, label) = lines
            .iter()
            .enumerate()
            .find_map(|(index, line)| {
                spec.parse_transcript_line_in(&path, line)
                    .into_iter()
                    .find_map(|event| match event {
                        crate::transcript::TranscriptEvent::TurnError { label, .. } => Some(label),
                        _ => None,
                    })
                    .map(|label| (index, label))
            })
            .unwrap_or_else(|| panic!("{agent} 样本里没有回合出错的记录"));
        let mut parser = spec.new_parser();
        for line in &lines[..=at] {
            parser.fold_line(line);
        }
        let error = parser
            .to_info()
            .error
            .unwrap_or_else(|| panic!("{agent}：时间线有错误卡，会话级却没报出错"));
        assert_eq!(error.label, label, "{agent}：两处的错误标签不一致");
    }
}

/// 样本入库前做过脱敏；这里再拦一道，防止以后有人往里加未处理的记录。
#[test]
fn replay_samples_carry_no_personal_data() {
    for entry in std::fs::read_dir(fixture_dir()).unwrap().flatten() {
        let path = entry.path();
        let text = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let lowered = text.to_lowercase();
        for marker in ["@gmail.", "@hotmail.", "@outlook.", "@qq.com", "-gmail-com", "-hotmail-com"] {
            assert!(!lowered.contains(marker), "{name} 里有邮箱或账号名：{marker}");
        }
        // 用户主目录只允许占位用户名 user（Windows 的 Users 目录、Git Bash 的 /c/Users）。
        for (index, _) in lowered.match_indices("users") {
            let rest = &lowered[index + "users".len()..];
            let rest = rest.trim_start_matches(['\\', '/', '-']);
            let owner: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            assert!(
                owner.is_empty() || owner == "user" || owner == "me",
                "{name} 里有未替换的用户主目录：Users/{owner}"
            );
        }
        for marker in ["sk-ant-", "ghp_", "github_pat_", "bearer ey", "\"refresh_token\"", "\"access_token\""] {
            assert!(!lowered.contains(marker), "{name} 里有疑似凭据：{marker}");
        }
    }
}
