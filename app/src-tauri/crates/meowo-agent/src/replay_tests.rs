//! 五家回放测试：每家若干份本机真实会话的摘录（`tests/fixtures/chat-replay/<家>.<会话>.jsonl`，
//! 已脱敏），走与对话页相同的读取入口（[`read_chat_delta`]）解析成 ChatItem，与入库的快照
//! （同名 `.items.json`）逐字比对。
//!
//! 它拦的是这类回归：接一个新 CLI 版本、改一处解析或归类，五家里任何一家的时间线变了都会
//! 在这里显形。前端另有一份测试（`app/src/views/chat/replay.test.tsx`）读同一批快照，
//! 接着走归并与渲染，断言到用户看见的文案与状态——两头用同一份数据接起来。
//!
//! 样本怎么来的：**每份取自同一场真实会话，记录保持原顺序**，只裁剪不拼接（文件名里的那段
//! 就是会话 id 的开头）。一场会话凑不齐要覆盖的块，就多放几份，各自是独立的用例。此前每家
//! 一份、从好几场会话里各摘几条拼成，时间戳来回跳，快照里出现「工作了 17181 分钟」（做空 ⑨）；
//! [`replay_samples_keep_the_original_order`] 现在钉死这一条。
//! 用户主目录的用户名换成 `user`，邮箱、内网地址、加密正文换成占位，过长的输出截短
//! （标了「样本截短」）；claude 摘录的 parentUuid 在每份里重新串成一条链（摘录必然断链）。
//! 除此之外不改内容，也不手编记录。每家缺哪类块、为什么缺，写在
//! [`replay_samples_cover_the_expected_blocks`] 的期望表旁边。
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

/// 一家的全部样本名（不带扩展名，如 `codex.01a1cea4`），按名字排序。
fn samples(agent: &str) -> Vec<String> {
    let prefix = format!("{agent}.");
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter_map(|name| name.strip_suffix(".jsonl").map(str::to_string))
        .filter(|name| name.starts_with(&prefix))
        .collect();
    names.sort();
    assert!(!names.is_empty(), "{agent} 没有样本");
    names
}

/// 一份样本走完整解析。
/// - claude / kimi / codex：样本就是 CLI 写的会话文件的摘录。
/// - gemini：样本是 CLI 的原始日志（带 `$set` 行），先经插件自己的物化（归并成稳定快照）再读，
///   与应用里的路径一致；物化到临时目录，不碰用户数据目录。
/// - opencode：对话在它的 SQLite 库里，样本是 Meowo 导出器对真实库的导出行（对话页读的
///   就是导出文件）；库 → 导出行这一步由 `plugins::opencode::store` 自己的测试覆盖。
fn replay(agent: &str, sample: &str) -> Vec<ChatItem> {
    let delta = crate::read_chat_delta(spec(agent), &sample_path(agent, sample), 0, None);
    assert!(!delta.items.is_empty(), "{sample} 样本没解析出任何条目");
    delta.items
}

/// 一家全部样本的条目，各份依次排开（只用于「覆盖了哪些块」这类不看顺序的断言）。
fn replay_all(agent: &str) -> Vec<ChatItem> {
    samples(agent)
        .iter()
        .flat_map(|sample| replay(agent, sample))
        .collect()
}

/// 对话页实际读的那个文件：gemini 是物化后的快照，其余就是样本本身。
fn sample_path(agent: &str, sample: &str) -> PathBuf {
    let source = fixture_dir().join(format!("{sample}.jsonl"));
    assert!(source.is_file(), "缺样本 {}", source.display());
    if agent == "gemini" {
        // 每份样本只物化一次：几个测试并行跑，各自清目录、各自物化会互相踩。
        static MATERIALIZED: std::sync::Mutex<Vec<(String, PathBuf)>> =
            std::sync::Mutex::new(Vec::new());
        let mut done = MATERIALIZED.lock().unwrap();
        if let Some((_, path)) = done.iter().find(|(name, _)| name == sample) {
            return path.clone();
        }
        let dir =
            std::env::temp_dir().join(format!("meowo_replay_{sample}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = crate::plugins::gemini::materialize_for_test(&source, &dir)
            .expect("gemini 样本物化失败");
        done.push((sample.to_string(), path.clone()));
        path
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
            ChatItem::ToolResult {
                interrupted: true, ..
            } => {
                seen.insert("没跑完的调用");
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
    let mut expected_files = BTreeSet::new();
    for agent in PROVIDERS {
        for sample in samples(agent) {
            let actual = snapshot_text(&replay(agent, &sample));
            let path = fixture_dir().join(format!("{sample}.items.json"));
            expected_files.insert(format!("{sample}.items.json"));
            if update {
                std::fs::write(&path, &actual).unwrap();
                continue;
            }
            // 工作区可能是 CRLF（core.autocrlf），比对前统一换行。
            let expected = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            if expected != actual {
                drifted.push(sample);
            }
        }
    }
    assert!(
        drifted.is_empty(),
        "这几份样本的回放结果与快照不一致：{drifted:?}。确认是有意的改动后，用 \
         MEOWO_UPDATE_REPLAY=1 cargo test -p meowo-agent --lib replay_ 更新快照，并同步更新前端的 \
         replay.test.tsx 快照。"
    );
    // 样本删了、快照还留着：前端会接着读一份没有出处的快照。
    for entry in std::fs::read_dir(fixture_dir()).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.ends_with(".items.json") || expected_files.contains(&name),
            "{name} 没有对应的样本"
        );
    }
}

/// 每份样本取自同一场会话、保持原顺序：解析出的条目时间不回退。拼贴的样本（从几场会话各摘
/// 几条）时间来回跳，界面上算出的耗时是几天（做空 ⑨）。
///
/// 用户消息与其余条目分开比：回合进行中发出的话，落库时刻早于它前面那一步的收尾。opencode
/// 按消息 id 排，用户消息排在还在跑的那一步之后（`opencode.KbLGZo`：03:40:55 发出的话排在
/// 03:41:12 才跑完的 ping 后面），这是记录本来的顺序，不是拼贴。回执不参与比较：它的时间是
/// 调用结束的时刻，同一步里并行的两个调用，前一个的回执晚于后一个的开始是正常的。
#[test]
fn replay_samples_keep_the_original_order() {
    for agent in PROVIDERS {
        for sample in samples(agent) {
            let mut last_user = String::new();
            let mut last_other = String::new();
            for item in replay(agent, &sample) {
                if matches!(item, ChatItem::ToolResult { .. }) {
                    continue;
                }
                let is_user = matches!(item, ChatItem::UserText { .. });
                let Some(stamp) = serde_json::to_value(&item)
                    .ok()
                    .and_then(|value| value.get("timestamp")?.as_str().map(str::to_string))
                else {
                    continue;
                };
                let last = if is_user { &mut last_user } else { &mut last_other };
                assert!(
                    *last <= stamp,
                    "{sample}：条目时间回退（{last} 之后是 {stamp}），样本不是同一场会话的原顺序"
                );
                *last = stamp;
            }
        }
    }
}

/// 每家样本覆盖了哪些块，逐家钉死。少一类 = 样本或解析退化；多一类 = 该把说明更新了。
/// 缺的类别是本机真实数据里没有、不能手编的，原因逐条写在表里。
///
/// 「没跑完的调用」= 回执上带被打断标记的调用（验收 14 的核心场景），各家的真实来源：
/// - claude：会话在调用途中结束，CC 接续时补写的回执（本机 16 次）。
/// - kimi：样本里那次调用（杀掉了宿主进程，自己也没等到回执）在记录里就是没有回执，没有标记
///   可带；它由前端测试按「所在回合已被新回合越过」断言成已中断。本机 171 份主链只有这 1 次。
/// - codex：被 Esc 打断的命令回执写着 `aborted by user`；另有一场会话里 3 次调用到回合收尾、
///   到文件末尾都没有回执（`codex.01a13ed0`），同样由前端测试断言。
/// - gemini：没有。调用连同结果一起落盘（本机 44 次调用全部带结果）。
/// - opencode：被 Esc 打断的工具以 `Tool execution aborted` 收场（本机 3 次）。停在 running 的
///   工具本机有 4 次：3 次之后再无消息（导出器不导出），1 次在私有项目的会话里，不能入库。
#[test]
fn replay_samples_cover_the_expected_blocks() {
    let expected: [(&str, &[&str]); 5] = [
        (
            "claude",
            &[
                "读", "写", "改", "命令", "搜索", "子任务", "其他", "失败回执", "没跑完的调用", "出错",
                "中断",
            ],
        ),
        (
            "kimi",
            &["读", "写", "改", "命令", "搜索", "子任务", "失败回执", "出错", "中断"],
        ),
        // 缺「读」：codex 没有读文件工具，读文件就是跑命令。缺「搜索」同理。
        // 缺「出错」：本机 98 份 rollout 里没有一次 task_complete 带 error。
        (
            "codex",
            &[
                "写", "改", "命令", "网页", "子任务", "其他", "失败回执", "没跑完的调用", "中断",
            ],
        ),
        // 缺「子任务」：本机 Gemini 会话没有派过子任务。
        (
            "gemini",
            &["读", "写", "改", "命令", "搜索", "其他", "失败回执", "出错", "中断"],
        ),
        // 缺「搜索」「子任务」：搜索只出现在一个私有项目的会话里（不能入库），task 一次都没有。
        (
            "opencode",
            &[
                "读", "写", "改", "命令", "网页", "其他", "失败回执", "没跑完的调用", "出错", "中断",
            ],
        ),
    ];
    for (agent, want) in expected {
        let have = blocks(&replay_all(agent));
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
        let mut checked = 0;
        for sample in samples(agent) {
            let path = sample_path(agent, &sample);
            let text = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            let Some((at, label)) = lines.iter().enumerate().find_map(|(index, line)| {
                spec.parse_transcript_line_in(&path, line)
                    .into_iter()
                    .find_map(|event| match event {
                        crate::transcript::TranscriptEvent::TurnError { label, .. } => Some(label),
                        _ => None,
                    })
                    .map(|label| (index, label))
            }) else {
                continue;
            };
            let mut parser = spec.new_parser();
            for line in &lines[..=at] {
                parser.fold_line(line);
            }
            let error = parser
                .to_info()
                .error
                .unwrap_or_else(|| panic!("{sample}：时间线有错误卡，会话级却没报出错"));
            assert_eq!(error.label, label, "{sample}：两处的错误标签不一致");
            checked += 1;
        }
        assert!(checked > 0, "{agent} 样本里没有回合出错的记录");
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
