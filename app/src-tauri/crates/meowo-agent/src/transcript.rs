//! Transcript 抽象（provider 无关）：数据类型 + 解析器 trait + 增量缓存。
//!
//! 「怎么定位 transcript、怎么解析它」是 agent 的能力，故 trait 住在插件层而非 DB 层——
//! 此前这套代码寄生在 `meowo-store` 里，让「读一个 JSONL 文件」平白拖上了 rusqlite 依赖，
//! 也让 claude 专属的 `~/.claude/projects` 路径布局伪装成了通用的 store API。
//!
//! 具体格式由各 agent 插件实现；claude 同时解析标题，codex/kimi 仅提供结构化对话，标题仍走首条 prompt。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// GUI 对话窗口消费的 provider 无关消息单元。终端 ANSI 只负责还原终端，结构化对话始终来自
/// agent 自己的 transcript，避免把光标移动、spinner 和重绘误当正文。
pub use meowo_protocol::ipc::{
    ChatItem, DiffHunk, CompactionDetail, SubagentBranchProbeDto, SubagentOutcome, SubagentProbeDto, SubagentRef, SubagentRun,
    ToolDetail,
};

/// 一条待读取的子任务侧车流。
pub struct SubagentStream {
    /// 分支标签（kimi 的 `agent-3`）；单发委派可为 None。
    pub label: Option<String>,
    /// 归一化状态 `running` / `completed` / `failed`；provider 未留下信号时为 None。
    pub status: Option<String>,
    /// 侧车最后一次落盘的时刻（ISO-8601）；provider 不留时间信号时为 None。
    /// 只有已结束的流才有意义——还在跑的流「最后一次落盘」随时会变。
    pub finished_at: Option<String>,
    pub path: PathBuf,
}

/// Provider 私有日志解析后的领域事件。它刻意不依赖 GUI/IPC 的序列化形状：插件只描述
/// “发生了什么”，边界适配器再决定当前前端契约如何表达。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptEvent {
    UserMessage {
        id: String,
        timestamp: Option<String>,
        text: String,
        /// 回合进行中由 CLI 插入（不是一轮的开头）。只有能从日志结构上认出来的
        /// provider 才置 true（claude 的 queued_command），其余一律 false。
        interjected: bool,
    },
    AssistantMessage {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    AssistantChunk {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    /// 回合级错误（API Error 等系统插入文案）。与 AssistantMessage 分开：这不是模型说的
    /// 话，前端按错误气泡渲染（label 与看板卡片的 error_label 同源）。
    TurnError {
        id: String,
        timestamp: Option<String>,
        label: String,
        text: String,
    },
    Reasoning {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ReasoningChunk {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ToolCall {
        id: String,
        timestamp: Option<String>,
        name: String,
        summary: String,
        /// 该调用是一次子任务委派时的展示信息（见 [`SubagentSpec::detect_call`]）。
        subagent: Option<SubagentRef>,
        /// 结构化详情（写入正文 / 编辑前后 / 命令…），插件认不得的工具为 None。
        detail: Option<ToolDetail>,
        /// ExitPlanMode 的完整计划文档（Kimi 在 display.plan 中携带）；普通工具为 None。
        plan: Option<String>,
    },
    ToolResult {
        id: String,
        timestamp: Option<String>,
        tool_call_id: Option<String>,
        text: String,
        is_error: bool,
        /// 该结果是子任务委派的回执时的结局统计（见 [`SubagentSpec::detect_result`]）。
        subagent: Option<SubagentOutcome>,
        /// 编辑类工具的统一 diff（见 [`DiffHunk`]）；拿不到为 None。
        patch: Option<Vec<DiffHunk>>,
    },
    Metadata {
        id: String,
        timestamp: Option<String>,
        kind: String,
        compaction: Option<CompactionDetail>,
    },
    /// CLI 收下了一条用户消息、排进了自己的队列（它正忙，还没开始处理）。不是对话内容，
    /// 只是送达回执：前端拿它确认「这条已经进了 CLI」，不必等它落成 user 行再猜。
    /// 只有日志里记了入队动作的 provider 才产出（claude 的 queue-operation enqueue）。
    PromptQueued {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
}

impl From<TranscriptEvent> for ChatItem {
    fn from(event: TranscriptEvent) -> Self {
        match event {
            TranscriptEvent::UserMessage {
                id,
                timestamp,
                text,
                interjected,
            } => Self::UserText {
                id,
                timestamp,
                text,
                interjected,
            },
            TranscriptEvent::AssistantMessage {
                id,
                timestamp,
                text,
            } => Self::AssistantText {
                id,
                timestamp,
                text,
            },
            TranscriptEvent::AssistantChunk {
                id,
                timestamp,
                text,
            } => Self::AssistantDelta {
                id,
                timestamp,
                text,
            },
            TranscriptEvent::TurnError {
                id,
                timestamp,
                label,
                text,
            } => Self::TurnError {
                id,
                timestamp,
                label,
                text,
            },
            TranscriptEvent::Reasoning {
                id,
                timestamp,
                text,
            } => Self::Reasoning {
                id,
                timestamp,
                text,
            },
            TranscriptEvent::ReasoningChunk {
                id,
                timestamp,
                text,
            } => Self::ReasoningDelta {
                id,
                timestamp,
                text,
            },
            TranscriptEvent::ToolCall {
                id,
                timestamp,
                name,
                summary,
                subagent,
                detail,
                plan,
            } => Self::ToolUse {
                id,
                timestamp,
                name,
                summary,
                subagent,
                detail,
                plan,
            },
            TranscriptEvent::ToolResult {
                id,
                timestamp,
                tool_call_id,
                text,
                is_error,
                subagent,
                patch,
            } => Self::ToolResult {
                id,
                timestamp,
                tool_use_id: tool_call_id,
                text,
                is_error,
                subagent,
                patch,
            },
            TranscriptEvent::Metadata {
                id,
                timestamp,
                kind,
                compaction,
            } => Self::Meta {
                id,
                timestamp,
                kind,
                compaction,
            },
            TranscriptEvent::PromptQueued {
                id,
                timestamp,
                text,
            } => Self::PromptQueued {
                id,
                timestamp,
                text,
            },
        }
    }
}

/// 一次增量读取结果。`offset` 只推进到最后一个完整换行，写到一半的 JSON 留给下一轮。
/// `start` 是本批 items 起始所对应的字节位置——首屏尾部加载时 > 0（前面还有历史未展示），
/// 供前端判断是否给出「加载更早」入口，并作为下一次向上翻页的上界。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChatDelta {
    pub items: Vec<ChatItem>,
    /// 本批记录里的最近计划；在首屏裁剪前提取，避免计划随较早历史一起被丢掉。
    pub plan: Option<String>,
    /// 本批完整记录中各维度最后一次出现的模式值；普通增量可能为空。
    pub agent_modes: Vec<AgentMode>,
    pub start: u64,
    pub offset: u64,
    pub reset: bool,
    /// 本批记录里有撤回（见 [`ChatLineMark::Undo`] / [`ChatLineMark::Link`]）：此时必为整读，
    /// items 为空也可信。
    pub undone: bool,
    /// 本次读到的文件 mtime，调用方原样带回下一轮做「等长重写」检测。
    /// 光比长度不够：CLI 压缩/重写 transcript 后字节数可能与上次**完全相同**，
    /// 只看 len 会认为无变化而静默漏掉整段新内容。
    #[serde(skip)]
    pub mtime: Option<std::time::SystemTime>,
}

/// 撤回语义的行标记。CLI 撤回不改写 transcript，两种落法：
/// - 追加一条「撤掉最后 N 轮」的记录（kimi `context.undo {count}`，给只会顺序读的读者留的
///   兼容行）。读取层顺序回放：遇到 `Undo(n)` 就把倒数第 n 个 `TurnStart` 那行起的消息收回。
/// - 什么都不写，下一条消息从更早的节点长出新分支（claude `/rewind` / 双击 Esc：transcript
///   是 parentUuid 树，2.1.314 实测回退本身零写入）。读取层见 `Link`：用户提示的 parent 不是
///   上一个节点、而指向更早的节点且其后出现过用户提示，就把那个节点之后的消息全部收回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatLineMark {
    /// 一轮的起点（可被撤回的用户输入；插话、后台通知不算）。
    TurnStart,
    /// 撤掉最近 n 轮。
    Undo(usize),
    /// 主链上的一个节点：自身 id 与父 id。只在该行产出用户提示时才用来判分叉。
    Link { id: String, parent: Option<String> },
}

/// Provider transcript 中观测到的一个独立模式维度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentMode {
    pub dimension: String,
    pub value: String,
}

impl AgentMode {
    pub fn new(dimension: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            dimension: dimension.into(),
            value: value.into(),
        }
    }
}

/// 检测到的回合错误：短中文标签 + 原始文案 + 去重指纹。指纹取**标签**而非出错消息的
/// uuid——自动重试会连发多条同类错误（每条新 uuid），按 uuid 去重会通知轰炸
///（缘由详见 claude 插件 to_info 的注释）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnError {
    pub label: String,
    pub raw: String,
    pub fingerprint: String,
}

/// 单次扫 transcript 的产物：标题、错误与上下文已用量。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptInfo {
    pub title: Option<String>,
    pub error: Option<TurnError>,
    /// 最近一条带 usage 的 assistant 回合的「上下文已用 token 数」
    /// = input + cache_creation + cache_read + output。无 usage 时为 None。
    pub context_tokens: Option<u64>,
    /// 上下文已用百分比（相对 200k 标准窗口，封顶 100）。
    pub context_pct: Option<u8>,
    /// 最近一条 assistant 正文的轻推预览（合并空白、截断）——供卡片 hover 速览，
    /// 不切终端就能判断该会话在问什么/说了什么。无正文回合（纯 tool_use）时为 None。
    pub preview: Option<String>,
    /// 还在跑的后台子任务数（claude：已发启动回执、尚无任何结局信号——task-notification
    /// （含排队形态）/ TaskOutput 拉取都算结局）。主回合 Stop 后它们仍在独立干活，
    /// 状态判定据此把「等待中」翻成「运行中」。不支持后台委派的 agent 恒 0。
    pub busy_subagents: u32,
}

/// 增量解析单元：逐行 fold、按需产出 TranscriptInfo。
/// Send：TranscriptCache 经 Arc<Mutex<>> 在 Tauri 主线程与后台轮询线程间共享。
pub trait TranscriptParser: Send {
    fn fold_line(&mut self, line: &str);
    fn to_info(&self) -> TranscriptInfo;
}

/// 把 assistant 正文清洗成卡片预览：合并所有空白为单空格、按**字符**截断到 ~180。
/// 单次遍历完成「折叠空白 + 计数截断」，命中上限即提前返回——大消息不再整条 collapse/分配。
///
/// 住在共享层而不是某个插件里：卡片预览的形制（长度、空白折叠）是**看板的**约定，
/// 不是哪一家 agent 的；kimi 的解析器也要产出同形状的预览，让它反向依赖 claude 插件
/// 就把「加 agent 只动 plugins/」的纪律破了。
pub(crate) fn preview_text(s: &str) -> Option<String> {
    const MAX: usize = 180;
    let mut out = String::new();
    let mut count = 0usize; // out 中的字符数
    let mut pending_space = false; // 词间是否有待补的单空格（行首/行尾不补）
    for ch in s.chars() {
        if ch.is_whitespace() {
            if count > 0 {
                pending_space = true;
            }
            continue;
        }
        // 写入该非空白字符（连同可能的前导空格）前先判断是否会超限。
        let need = if pending_space { 2 } else { 1 };
        if count + need > MAX {
            out.push('…');
            return Some(out);
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        out.push(ch);
        count += 1;
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 只提供 GUI 对话、不参与标题/错误/上下文分析的 transcript 共用空解析器。
pub(crate) struct ChatOnlyParser;

impl TranscriptParser for ChatOnlyParser {
    fn fold_line(&mut self, _line: &str) {}

    fn to_info(&self) -> TranscriptInfo {
        TranscriptInfo::default()
    }
}

/// 子任务（subagent）能力：agent 支持把工作委派给子 agent，并把子 agent 的过程记在
/// **主 transcript 之外的侧车流**里时声明它。
///
/// 为什么单列一个槽而不是在解析里加分支：两家的布局与关联方式毫无共同点——
/// claude 是 `<session>/subagents/agent-<id>.{jsonl,meta.json}`，靠 meta 里的 `toolUseId`
/// 做外键；kimi 是 `agents/agent-N/wire.jsonl`，得从主链 `tool.result` 的**输出正文**里
/// 抠出 `agent_id`。把这些塞进共享路径必然长成一排 `if provider ==`。
///
/// 侧车流按需读取（用户展开时），不进 [`read_chat_delta`] 的增量热路径：子任务往往几十条、
/// 一个会话几十个，跟着 650ms 轮询一起读会让长会话首开与稳态轮询都付出无谓代价。
pub trait SubagentSpec: Sync {
    /// 主链上这条工具调用是不是一次子任务委派？是则返回展示信息。
    fn detect_call(
        &self,
        tool_name: &str,
        input: Option<&serde_json::Value>,
    ) -> Option<SubagentRef>;

    /// 定位该次委派的侧车流。**返回列表**：一次调用未必只派一个子任务——kimi 的
    /// `AgentSwarm` 一次能派出十几个（fan-out 一批 items，或 resume 一批既有 agent）。
    /// `main_transcript` = 主 transcript 路径，`tool_use_id` = 主链那条工具调用的 id。
    /// 空列表 = 子任务尚未落盘、记录已清理，或这条根本不是委派。
    fn locate_streams(&self, main_transcript: &Path, tool_use_id: &str) -> Vec<SubagentStream>;

    /// 解析侧车流的一行。两家的侧车流都与主流同格式，但可能有额外前提
    /// （claude 的侧车行全部带 `isSidechain`，主流解析会主动丢弃它们）。
    fn parse_stream_line(&self, line: &str) -> Vec<TranscriptEvent>;

    /// 主链上这条**工具结果**是不是某次委派的回执？是则给出各分支的结局统计。
    ///
    /// 状态就写在主 transcript 里，解析时顺手取到即可——因此折叠状态下也能显示进度，
    /// 不必先展开（展开要读侧车流，那是按需 I/O，不该为了一个徽标付出）。
    /// 默认 None：该 provider 没有在主链留下可靠的结局信号。
    fn detect_result(&self, _output: &str) -> Option<SubagentOutcome> {
        None
    }
}

/// 某 agent 的 transcript 规格：定位文件 + 解析标题 + 产出增量解析器。
/// Sync：以 &'static dyn 共享。
pub trait TranscriptSpec: Sync {
    /// 新建一个该 agent 的增量解析器（供 TranscriptCache 在新建/重置条目时调用）。
    fn new_parser(&self) -> Box<dyn TranscriptParser>;
    /// 定位 transcript 文件（hook 路径 → cwd+id 重建 → 全局查找）。
    fn resolve_transcript_path(
        &self,
        transcript_path: Option<&str>,
        cwd: Option<&str>,
        session_id: &str,
    ) -> Option<PathBuf>;
    /// 高频轮询（看板/侧栏每 ~2s 一次刷新）专用的定位：允许实现对「找不到」做短期负缓存，
    /// 代价是文件后来出现时最多晚一个 TTL 才被发现。默认等同 [`Self::resolve_transcript_path`]
    /// （不缓存）。一次性动作（恢复、接续、交接）必须走原方法——它们要的是此刻的真相。
    fn resolve_transcript_path_polled(&self, cwd: Option<&str>, session_id: &str) -> Option<PathBuf> {
        self.resolve_transcript_path(None, cwd, session_id)
    }
    /// 解析会话标题（读不到/无标题返回 None）。
    fn resolve_title(
        &self,
        transcript_path: Option<&str>,
        cwd: Option<&str>,
        session_id: &str,
    ) -> Option<String>;

    /// 解析会话的真实工作目录——resume 必须在正确的项目目录下运行才找得到会话。
    ///
    /// 默认实现原样返回 DB 记录的 cwd。能从 transcript 内容读出权威 cwd 的 agent（claude）覆写它，
    /// 以纠正失真的 DB 记录（会话早于 hook 接线、SessionStart 丢失、项目目录事后被移动）。
    ///
    /// 此前这是 `meowo_store::title::resolve_cwd`：一个读 `~/.claude/projects` 的 claude 专属函数，
    /// 却被 app 当通用 API 对所有 agent 调用——非 claude 会话靠「全局找不到就回退 DB cwd」的巧合
    /// 拿到正确结果。现在这个回退就是默认实现本身。
    fn resolve_cwd(&self, cwd: Option<&str>, _session_id: &str) -> Option<String> {
        default_resolve_cwd(cwd)
    }

    /// 把一条完整 transcript JSONL 记录转成领域事件。默认不支持对话窗口。
    fn parse_transcript_line(&self, _line: &str) -> Vec<TranscriptEvent> {
        Vec::new()
    }

    /// 带 transcript 文件路径上下文的行解析：行内引用落在文件旁的资源时（kimi 把用户
    /// 粘贴的图片记成 blobref，正文在 wire.jsonl 旁的 `blobs/` 目录）需要覆写它。
    /// 默认忽略路径、退回 [`TranscriptSpec::parse_transcript_line`]。
    fn parse_transcript_line_in(&self, _path: &Path, line: &str) -> Vec<TranscriptEvent> {
        self.parse_transcript_line(line)
    }

    /// 从一条完整 transcript 记录中提取零到多个模式维度更新。默认不支持。
    fn agent_modes_from_line(&self, _line: &str) -> Vec<AgentMode> {
        Vec::new()
    }

    /// 这一行对「撤回」的意义（见 [`ChatLineMark`]）。默认没有撤回语义。
    fn chat_line_mark(&self, _line: &str) -> Option<ChatLineMark> {
        None
    }

    /// 从文件头开始读取时使用的初始模式。只声明协议有确定默认值的维度；后续记录会覆盖它。
    fn default_agent_modes(&self) -> Vec<AgentMode> {
        Vec::new()
    }

    /// 从 Agent 写入 transcript 的 runtime 能力清单发现斜杠命令。默认没有此类元数据；
    /// 实现必须只返回当前会话明确声明的项目，不能根据版本或名称猜测。
    fn supports_runtime_slash_commands(&self) -> bool {
        false
    }

    /// None 表示权威清单尚未出现；Some（包括空 Vec）表示已经观测到本会话的完整清单。
    fn runtime_slash_commands(&self, _path: &Path) -> Option<Vec<crate::chat_ui::SlashCommand>> {
        None
    }

    /// 这份 transcript 是否声明「会话已转到另一个 id 继续」，是则返回新会话 id。
    /// agent 自己写下的接续标记是换绑 PTY 的强证据：同一个终端换了会话 id、认领方又是
    /// 另一个进程时，靠它把「agent 主动接续」和「嵌套 agent 误认领」分开。默认没有此标记。
    fn continued_in(&self, _path: &Path) -> Option<String> {
        None
    }

    /// 此 transcript 是否提供结构化对话。与解析入口分开声明，避免用假数据探测能力。
    fn supports_chat(&self) -> bool {
        false
    }

    /// 长会话首屏 / 「加载更早」的尾读声明（见 [`read_chat_delta_paged`]）。None（默认）= 不支持，
    /// 照旧整段解析。声明了的 agent 只解析文件尾部够凑一屏的那段，窗口之前的内容只做子串预筛。
    fn tail_read(&self) -> Option<TailRead> {
        None
    }

    /// 是否需要主看板后台增量分析标题/错误/预览。仅聊天用的日志应返回 false，避免无意义热读大文件。
    fn supports_analysis(&self) -> bool {
        true
    }

    /// 子任务能力。None = 该 agent 没有子任务概念（codex 当前如此：工具只有 exec/wait，
    /// `task_started` 是回合级事件而非委派），或其子任务过程不落在可读的侧车流里。
    fn subagents(&self) -> Option<&'static dyn SubagentSpec> {
        None
    }
}

/// transcript 缓存失效判据，聊天与看板分析两条读取路径共用。
///
/// 两种情况必须丢弃 offset 从头重读，否则会静默漏掉整段内容：
/// - **截断/重建**：`len < offset`，文件比我们读过的还短。
/// - **等长重写**：`len == offset` 但 mtime 变了。压缩/改写后长度恰好不变时，
///   光比长度看不出任何变化，只有 mtime 会动。
///
/// `prev_mtime` 为 None 时不判等长重写：没有上一次的观测就无从谈「变了」，
/// 拿「有 mtime」去比「没 mtime」会把首次读取误判成重写，白白全量重读一次。
pub(crate) fn transcript_reset(
    len: u64,
    offset: u64,
    mtime: Option<std::time::SystemTime>,
    prev_mtime: Option<std::time::SystemTime>,
) -> bool {
    len < offset || (len == offset && prev_mtime.is_some() && mtime != prev_mtime)
}

/// 单个子任务侧车的读取上限。子任务本身就是「大工作拆出去」，个别会跑出很大的流；
/// 展开是一次同步 IPC，不能让一次点击拖住窗口。超限时读尾部——子任务的结论在末尾。
const SUBAGENT_STREAM_LIMIT: u64 = 4 * 1024 * 1024;

/// 读一条侧车流的全部消息。超限时读尾部——子任务的结论在末尾。
fn read_stream(subagents: &dyn SubagentSpec, path: &Path) -> Option<Vec<ChatItem>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let truncated = len > SUBAGENT_STREAM_LIMIT;
    if truncated {
        file.seek(SeekFrom::Start(len - SUBAGENT_STREAM_LIMIT))
            .ok()?;
    }
    // JSONL 行可能含非法 UTF-8（截断的多字节），lossy 读避免整份作废。
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    // 超限时从中间切入，首行多半是半条 JSON，丢掉。
    if truncated {
        lines.next();
    }
    Some(
        lines
            .flat_map(|line| subagents.parse_stream_line(line))
            .map(ChatItem::from)
            .collect(),
    )
}

/// 读取一次子任务委派的完整时间线。**按需调用**（用户展开时），不进增量热路径。
///
/// 与 [`read_chat_delta`] 的区别是刻意的：那条路径服务 650ms 轮询，必须增量且只读一个流；
/// 这里读的是已经写完的侧车流，整读一次即可，不需要 offset 记账。
///
/// 返回多条：一次委派未必只派一个子任务（kimi 的 `AgentSwarm`）。空列表表示该 agent
/// 没有子任务能力，或这条调用找不到任何侧车流（尚未落盘、已被清理、或根本不是委派）。
pub fn read_subagent_chat(
    spec: &dyn TranscriptSpec,
    main_transcript: &Path,
    tool_use_id: &str,
) -> Vec<SubagentRun> {
    let Some(subagents) = spec.subagents() else {
        return Vec::new();
    };
    subagents
        .locate_streams(main_transcript, tool_use_id)
        .into_iter()
        .filter_map(|stream| {
            let mut items = read_stream(subagents, &stream.path)?;
            items.retain(|item| !is_inherited_launch(item, tool_use_id));
            Some(SubagentRun {
                label: stream.label,
                status: stream.status,
                items,
            })
        })
        .collect()
}

/// 侧车流里「派出这个子任务」那一截，不属于子任务自己的工作，展示时去掉。
///
/// claude 的 fork 子任务继承父会话上下文（实拍 2.1.317，侧车头 4 行）：
/// `fork-context-ref` 引用行，之后是父会话里**派出它的那条 Agent 调用**（id 与主链完全
/// 相同），再是回执「Fork started — that fork is you.」，以及同一行里一大段
/// `<fork-boilerplate>` 说明。前端时间线见到 Agent 调用就挂子任务块、展开又按同一个
/// id 拉回同一份侧车——自己套自己，无限嵌套（用户实拍反馈）。
///
/// 按 id 判而不按文案：任何 provider 的侧车里出现「派出自己」的调用与回执都是父链回声。
fn is_inherited_launch(item: &ChatItem, launch_id: &str) -> bool {
    match item {
        ChatItem::ToolUse { id, .. } => id == launch_id,
        ChatItem::ToolResult { tool_use_id, .. } => tool_use_id.as_deref() == Some(launch_id),
        ChatItem::UserText { text, .. } => text.trim_start().starts_with("<fork-boilerplate>"),
        _ => false,
    }
}

/// 实测一次**未结**委派此刻的状态——主链回执还没落盘时的唯一来源。
///
/// 并行委派的回执要等同一步里的工具全部跑完才一起写盘，于是整批跑完之前，先收工的
/// 子任务在主链上毫无痕迹，折叠徽标与进度面板只能一律显示「在跑」。侧车流自己带着
/// 终结标记（kimi 的 `step.end`），[`SubagentSpec::locate_streams`] 已经把它读了出来，
/// 这里只负责把一次委派的多条流（`AgentSwarm`）聚合成一个状态。
///
/// 与 [`read_subagent_chat`] 同样是**按需** I/O（面板展开时），不进 650ms 轮询热路径。
///
/// 返回 None = 这条调用定位不到侧车流，或该 provider 不留状态信号（claude 的 meta.json
/// 只记身份不记结果）——此时不能反过来断言「它结束了」，交由调用方维持原判。
pub fn probe_subagent_state(
    spec: &dyn TranscriptSpec,
    main_transcript: &Path,
    tool_use_id: &str,
) -> Option<SubagentProbeDto> {
    let subagents = spec.subagents()?;
    let streams = subagents.locate_streams(main_transcript, tool_use_id);
    if streams.is_empty() {
        return None;
    }
    let mut running = false;
    let mut failed = false;
    let mut known = false;
    let mut finished_at: Option<String> = None;
    let mut branches: Vec<SubagentBranchProbeDto> = Vec::with_capacity(streams.len());
    for stream in &streams {
        match stream.status.as_deref() {
            Some("running") => {
                known = true;
                running = true;
            }
            Some("failed") => {
                known = true;
                failed = true;
            }
            Some("completed") => known = true,
            // 没留下信号的流不能算「已结束」——一批里只要有一条说不准，整次委派就还没完。
            _ => running = true,
        }
        // 一次委派的多条流各自收工，整体结束于最后一条。
        if let Some(ts) = &stream.finished_at {
            if finished_at.as_deref().is_none_or(|seen| seen < ts.as_str()) {
                finished_at = Some(ts.clone());
            }
        }
        // 逐分支状态与上面的聚合同一份判据：有信号的照归一值下发；无信号的分支标
        // `running`（而非跳过）——面板分支行按序号取 branches[i]，跳过会让序号错位、
        // 把后一条流的状态贴到前一行上；标 running 则维持「不能断言结束」的原语义。
        let (branch_running, branch_status) = match stream.status.as_deref() {
            Some("running") => (true, "running"),
            Some("failed") => (false, "failed"),
            Some("completed") => (false, "completed"),
            _ => (true, "running"),
        };
        branches.push(SubagentBranchProbeDto {
            label: stream.label.clone(),
            status: branch_status.to_string(),
            finished_at: stream
                .finished_at
                .clone()
                .filter(|_| !branch_running),
        });
    }
    if !known {
        return None;
    }
    let status = if running {
        "running"
    } else if failed {
        "failed"
    } else {
        "completed"
    };
    Some(SubagentProbeDto {
        status: status.to_string(),
        // 还在跑就没有「结束时刻」可言：侧车尾部的时间只是最近一次落盘。
        finished_at: finished_at.filter(|_| !running),
        branches,
    })
}

/// 从 `offset` 起只解析新增的完整 JSONL 行。文件被截断/重建时自动从头开始并标记 reset，前端据此
/// 清空旧消息。读取失败原样返回 offset，短暂的文件锁不会让界面丢历史。
pub fn read_chat_delta(
    spec: &dyn TranscriptSpec,
    path: &Path,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
) -> ChatDelta {
    read_chat_delta_paged(spec, path, offset, prev_mtime, 0)
}

/// [`read_chat_delta`] 的分页版：`page_size > 0` 且本次是整读（base==0）时，把结果裁成
/// 最后一屏——只留末尾 `page_size` 条消息，裁剪按整行进行（边界行不腰斩，宁可多留），
/// `start` 随之指向保留窗口的首行字节。`start > 0` 即「前面还有更早历史」，把它作为
/// [`read_chat_window`] 的 `end` 就能向上翻页，不必再整文件重读。
pub fn read_chat_delta_paged(
    spec: &dyn TranscriptSpec,
    path: &Path,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
    page_size: usize,
) -> ChatDelta {
    // 长会话首开：能尾读就只解析尾部一屏（结果与下方整读逐字段一致，判不准时它自己退回整读）。
    if let Some(delta) = tail_first_page(spec, path, offset, prev_mtime, page_size, TAIL_FIRST_BLOCK) {
        return delta;
    }
    // 文件 IO 与失效判据复用 read_transcript_delta（与看板分析同一份）。
    // NoChange 不早退：空文件首读（offset=0）也要走到下方 base==0 分支下发默认模式。
    let (reset, buf, mtime) = match read_transcript_delta(path, offset, prev_mtime) {
        DeltaOutcome::Unreadable => {
            return ChatDelta {
                items: Vec::new(),
                plan: None,
                agent_modes: Vec::new(),
                start: offset,
                offset,
                reset: false,
                undone: false,
                mtime: prev_mtime,
            };
        }
        DeltaOutcome::NoChange(mtime) => (false, Vec::new(), mtime),
        DeltaOutcome::Data { reset, buf, mtime } => (reset, buf, mtime),
    };
    let base = if reset { 0 } else { offset };
    let consumed = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let region = parse_chat_region(spec, path, base, &buf[..consumed]);
    // 增量批次里出现撤回：被撤的消息多半早已下发，增量收不回来——改为从文件头整读，
    // 按 reset 下发让前端清空重灌。undone 告诉前端「整读结果为空也是真的」（撤掉了唯一一轮）。
    if region.undone && base > 0 {
        let mut full = read_chat_delta_paged(spec, path, 0, None, page_size);
        full.reset = true;
        full.undone = true;
        return full;
    }
    let ChatRegion { mut lines, agent_modes, undone, .. } = region;
    let plan = latest_plan(&lines);
    // 只有整读（首屏/reset 重读）才裁首屏：增量赶上来的大批次裁了会丢消息。
    let start = if base == 0 {
        tail_window_start(&mut lines, base, page_size)
    } else {
        base
    };
    let items = lines.into_iter().flat_map(|(_, items)| items).collect();
    ChatDelta {
        items,
        plan,
        agent_modes,
        start,
        offset: base + consumed as u64,
        reset,
        undone,
        mtime,
    }
}

/// 一次向上翻页的结果，见 [`read_chat_window`]。
#[derive(Debug, Clone, PartialEq)]
pub struct ChatWindow {
    pub items: Vec<ChatItem>,
    /// 本屏首行的字节偏移：>0 时前面还有更早历史，可作为下一屏的 `end` 继续上翻。
    pub start: u64,
    pub has_more: bool,
}

/// 「加载更早」的向上翻页：读 `[0, end)` 区间的最后一屏（`page_size` 条，整行裁剪）。
/// `end` 来自上一屏的 `ChatDelta::start` / `ChatWindow::start`——当前已加载窗口的首行字节，
/// 故每屏只需解析「还没展示过的那段」，长会话反复上翻不再是 O(整文件) × 页数。
/// 文件读不到返回 None（前端保留「加载更早」入口供重试）。
pub fn read_chat_window(
    spec: &dyn TranscriptSpec,
    path: &Path,
    end: u64,
    page_size: usize,
) -> Option<ChatWindow> {
    read_chat_window_in(spec, path, end, page_size, TAIL_FIRST_BLOCK)
}

/// [`read_chat_window`] 本体；`first_block` 是尾读初始窗口（测试用极小值压块边界）。
fn read_chat_window_in(
    spec: &dyn TranscriptSpec,
    path: &Path,
    end: u64,
    page_size: usize,
    first_block: u64,
) -> Option<ChatWindow> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    // 与首屏同一套尾读：只解析 `end` 之前够凑一屏的那段（判不准时它自己扩到文件头整段解析）。
    if spec.tail_read().is_some() && page_size > 0 {
        let end = end.min(file.metadata().ok()?.len());
        let TailOutcome::Window { mut lines, start, .. } =
            tail_region(spec, path, &mut file, end, page_size, first_block)
        else {
            return None;
        };
        let start = tail_window_start(&mut lines, start, page_size);
        return Some(ChatWindow {
            items: lines.into_iter().flat_map(|(_, items)| items).collect(),
            start,
            has_more: start > 0,
        });
    }
    let mut buf = Vec::new();
    file.take(end).read_to_end(&mut buf).ok()?;
    let consumed = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut lines = parse_chat_region(spec, path, 0, &buf[..consumed]).lines;
    let start = tail_window_start(&mut lines, 0, page_size);
    let items = lines.into_iter().flat_map(|(_, items)| items).collect();
    Some(ChatWindow {
        items,
        start,
        has_more: start > 0,
    })
}

/// 解析一段完整 JSONL 字节（`base` = 该段在文件中的起始字节），逐行产出
/// （行起始字节， 该行消息）并顺手折叠本段的模式记录（整读口径；尾读窗口见 [`tail_region`]）。
fn parse_chat_region(spec: &dyn TranscriptSpec, path: &Path, base: u64, buf: &[u8]) -> ChatRegion {
    let parsed = parse_lines(spec, path, base, buf);
    let folded = fold_lines(spec, path, base, &parsed, false);
    folded.into_region(parsed)
}

/// 一行的逐行解析结果。这三样只取决于这一行本身（纯函数），与前后文无关；
/// 跨行的状态（撤回、双写判重、分叉）由 [`fold_lines`] 另算。拆开是为了尾读扩窗时
/// 复用已解析的尾段：JSON 解析是大头，跨行折叠很便宜。
struct ParsedLine {
    start: u64,
    modes: Vec<AgentMode>,
    mark: Option<ChatLineMark>,
    items: Vec<ChatItem>,
}

/// 逐行解析一段完整 JSONL 字节（`base` = 该段在文件中的起始字节）。行界在**原始字节**上切：
/// `from_utf8_lossy` 的替换字符会改变字节数，拿它算偏移会对不上文件。
fn parse_lines(spec: &dyn TranscriptSpec, path: &Path, base: u64, buf: &[u8]) -> Vec<ParsedLine> {
    let mut out = Vec::new();
    let mut pos = base;
    for chunk in buf.split_inclusive(|b| *b == b'\n') {
        let start = pos;
        pos += chunk.len() as u64;
        let line = String::from_utf8_lossy(chunk);
        let line = line.trim_end_matches(['\r', '\n']);
        let modes = spec.agent_modes_from_line(line);
        let mark = spec.chat_line_mark(line);
        // 撤回行本身不产出消息（折叠时直接跳过），不必解析。
        let items = if matches!(mark, Some(ChatLineMark::Undo(_))) {
            Vec::new()
        } else {
            spec.parse_transcript_line_in(path, line).into_iter().map(ChatItem::from).collect()
        };
        out.push(ParsedLine { start, modes, mark, items });
    }
    out
}

/// [`fold_lines`] 的结果：每条已解析消息留不留（按行顺序平铺），`offsets[i]..offsets[i+1]`
/// 是第 i 行的消息区间。只记去留不搬数据，尾读扩窗时同一批解析结果可以反复折叠。
struct Folded {
    kept: Vec<bool>,
    offsets: Vec<usize>,
    agent_modes: Vec<AgentMode>,
    undone: bool,
    ambiguous: bool,
}

impl Folded {
    fn total(&self) -> usize {
        self.kept.iter().filter(|kept| **kept).count()
    }

    fn into_region(self, parsed: Vec<ParsedLine>) -> ChatRegion {
        let lines = parsed
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                let kept = &self.kept[self.offsets[i]..self.offsets[i + 1]];
                let items = line
                    .items
                    .into_iter()
                    .zip(kept)
                    .filter(|(_, kept)| **kept)
                    .map(|(item, _)| item)
                    .collect();
                (line.start, items)
            })
            .collect();
        ChatRegion { lines, agent_modes: self.agent_modes, undone: self.undone }
    }
}

/// 按文件顺序折叠逐行结果：折叠模式记录、回放撤回与分叉、消掉双写的用户消息。
/// `base` = 第一行在文件中的起始字节（0 时从默认模式起折）。
///
/// `tail = true`：这一段是尾读窗口（[`tail_region`]），调用方要拿它冒充「整读的尾部」。
/// 此时凡是结果可能取决于 `base` 之前内容的地方都不自作主张，而是置 `ambiguous`
/// 让调用方扩大窗口重来：撤回的轮数超出窗口、用户提示的分叉点不在窗口内、
/// 窗口里第一条消息就是用户消息（双写判重要看前一条）。
fn fold_lines(spec: &dyn TranscriptSpec, path: &Path, base: u64, parsed: &[ParsedLine], tail: bool) -> Folded {
    let mut kept: Vec<bool> = Vec::new();
    let mut offsets: Vec<usize> = Vec::with_capacity(parsed.len() + 1);
    // 本段里各轮起点所在的行下标（撤回回放用），以及本段有没有撤回记录。
    let mut turn_starts: Vec<usize> = Vec::new();
    let mut undone = false;
    let mut agent_modes = if base == 0 {
        spec.default_agent_modes()
    } else {
        Vec::new()
    };
    // 上一条产出的消息若是用户消息，记下它（双写判重用）；中间隔了任何别的消息就清空。
    let mut last_user: Option<(&str, Option<&str>)> = None;
    // 分叉判定（见 ChatLineMark::Link）：节点 id → (所在行下标, 截至该行的用户提示数)；上一个
    // 节点 id（增量批次的第一个节点之前，按需从 base 之前回读）；本段已见的用户提示数。
    let mut link_at: std::collections::HashMap<&str, (usize, usize)> = std::collections::HashMap::new();
    let mut last_link: Option<&str> = None;
    let mut prompts = 0usize;
    // 尾读专用：结果可能依赖窗口之前的内容（见函数注释）；窗口前最后一个主链节点（懒取一次）；
    // 「还没产出过任何消息」：此时 last_user 的真值取决于窗口前一行，窗口内无从得知。
    let mut ambiguous = false;
    let mut seed_link: Option<Option<String>> = None;
    let mut user_state_unknown = tail;
    for (index, line) in parsed.iter().enumerate() {
        let line_at = kept.len();
        offsets.push(line_at);
        merge_modes(&mut agent_modes, line.modes.clone());
        let mut link = None;
        match &line.mark {
            Some(ChatLineMark::Link { id, parent }) => link = Some((id.as_str(), parent.as_deref())),
            Some(ChatLineMark::TurnStart) => turn_starts.push(index),
            Some(ChatLineMark::Undo(count)) => {
                undone = true;
                // 窗口里不够 n 轮：整读会收回窗口之前的轮次，起点在窗口外，这里复现不了。
                ambiguous |= tail && turn_starts.len() < *count;
                user_state_unknown = false;
                // 本段里不够 n 轮（增量批次只含末尾几行）时整段收回；调用方见 undone
                // 会改为从文件头重读，那时起点都在段内，回放是准的。
                let cut = turn_starts.len().saturating_sub(*count);
                let from = turn_starts.get(cut).copied().unwrap_or(0);
                kept[offsets[from]..].fill(false);
                turn_starts.truncate(cut);
                last_user = None;
                continue;
            }
            None => {}
        }
        let mut prompt = false;
        for item in &line.items {
            if let ChatItem::UserText { text, timestamp, .. } = item {
                // 窗口里的第一条消息就是用户消息：整读时它可能与窗口前一条双写判重被吞。
                ambiguous |= user_state_unknown;
                let key = text.trim();
                if last_user.is_some_and(|(prev, prev_ts)| {
                    prev == key && within_double_write(prev_ts, timestamp.as_deref())
                }) {
                    kept.push(false);
                    continue;
                }
                last_user = Some((key, timestamp.as_deref()));
                prompt = true;
            } else {
                last_user = None;
            }
            user_state_unknown = false;
            kept.push(true);
        }
        if let Some((id, parent)) = link {
            if prompt {
                if let Some(parent) = parent.filter(|parent| Some(*parent) != last_link) {
                    match link_at.get(parent) {
                        // 分叉点之后出现过用户提示 = 那之后的整段被放弃了（回退后重发）。
                        // 没出现过的（只隔了附件、系统行）不算：那是同一轮里的正常跳接。
                        Some(&(at, seen)) if prompts > seen => {
                            kept[offsets[at + 1]..line_at].fill(false);
                            undone = true;
                            last_user = None;
                        }
                        Some(_) => {}
                        // 尾读窗口：parent 恰是窗口前最后一个节点 = 正常接续（整读时它就是
                        // last_link，根本不进这个分支）；否则分叉点可能在窗口外，整读会把窗口
                        // 前后一并收回，这里复现不了，交给调用方扩窗。
                        None if tail => {
                            let continues = last_link.is_none()
                                && seed_link
                                    .get_or_insert_with(|| last_link_before(spec, path, base))
                                    .as_deref()
                                    == Some(parent);
                            ambiguous |= !continues;
                        }
                        // 分叉点在本段之前（增量批次）：本段收不回已下发的消息，标 undone 让
                        // 调用方从文件头整读。批次首个节点之前的「上一个节点」按需回读；
                        // 回读不到（窗口外/不可读）就不判，宁可漏收不误收。
                        None if base > 0 => {
                            let previous = match last_link {
                                Some(previous) => Some(previous.to_string()),
                                None => last_link_before(spec, path, base),
                            };
                            if previous.is_some_and(|previous| previous != parent) {
                                undone = true;
                            }
                        }
                        None => {}
                    }
                }
                prompts += 1;
            }
            link_at.insert(id, (index, prompts));
            last_link = Some(id);
        }
    }
    offsets.push(kept.len());
    Folded { kept, offsets, agent_modes, undone, ambiguous }
}

/// `base` 之前最后一个主链节点的 id（增量批次判分叉用：批次里第一条用户提示的 parent 该是它）。
/// 只往回看一个窗口，找不到返回 None（调用方按「不知道」处理，不判分叉）。
fn last_link_before(spec: &dyn TranscriptSpec, path: &Path, base: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    const WINDOW: u64 = 512 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let start = base.saturating_sub(WINDOW);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = vec![0; (base - start) as usize];
    file.read_exact(&mut buf).ok()?;
    let body = if start == 0 {
        &buf[..]
    } else {
        // 窗口首行多半是半截，跳过。
        &buf[buf.iter().position(|b| *b == b'\n').map_or(buf.len(), |i| i + 1)..]
    };
    body.split(|b| *b == b'\n').rev().find_map(|chunk| {
        let line = String::from_utf8_lossy(chunk);
        match spec.chat_line_mark(line.trim_end_matches('\r')) {
            Some(ChatLineMark::Link { id, .. }) => Some(id),
            _ => None,
        }
    })
}

/// [`parse_chat_region`] 的结果：逐行（行起始字节，该行消息）、本段折叠出的模式、本段是否含撤回。
struct ChatRegion {
    lines: Vec<(u64, Vec<ChatItem>)>,
    agent_modes: Vec<AgentMode>,
    undone: bool,
}

/// [`TranscriptSpec::tail_read`] 的声明：窗口之前的内容只做字节子串预筛，以下两类行
/// 必须能靠子串认出来，否则尾读会漏掉它们对结果的影响。
#[derive(Debug, Clone, Copy)]
pub struct TailRead {
    /// 可能产出模式（`agent_modes_from_line` 非空）的行必含其一。
    pub mode_markers: &'static [&'static str],
    /// 可能产出计划（`ToolUse.plan`）的行必含其一；空 = 该 agent 从不产出计划。
    pub plan_markers: &'static [&'static str],
}

/// 尾读初始窗口。claude 长会话平均一行 ~4KB，1MB 通常够凑满 200 条。
const TAIL_FIRST_BLOCK: u64 = 1 << 20;

/// [`tail_region`] 的结果。`Window` 的逐行结果与整读的同段逐字一致：要么窗口从文件头起
/// （`start == 0`，本身就是整读），要么窗口内消息数 **严格多于** 一屏且无窗口外依赖
/// （等于一屏时整读会因「总数 ≤ 一屏」不裁剪，start 口径不同）。
enum TailOutcome {
    Window {
        lines: Vec<(u64, Vec<ChatItem>)>,
        agent_modes: Vec<AgentMode>,
        undone: bool,
        start: u64,
        end: u64,
    },
    Unreadable,
}

/// 读 `[0, end)` 里最后一屏所需的那段：从尾部按块往前，直到窗口里的消息够一屏、且没有任何
/// 依赖窗口外内容的迹象（[`fold_lines`] 的 `ambiguous`）。窗口起点对齐到行首，
/// 终点对齐到 `end` 之前最后一个换行——与整读的 `consumed` 同口径。
///
/// 扩窗：消息不够时按已见密度估算还差多少字节（留 1.5 倍余量，至少翻倍），判不准时 ×4；
/// 扩到文件头就在这里直接整段解析（`tail=false`），不让调用方再读一遍。真实数据里「末尾 200 条
/// 横跨 18MB」（大工具结果）和「整份不到 200 条」都常见，固定倍数会在它们身上反复白解析。
fn tail_region(
    spec: &dyn TranscriptSpec,
    path: &Path,
    file: &mut std::fs::File,
    end: u64,
    page_size: usize,
    first_block: u64,
) -> TailOutcome {
    use std::io::{Read, Seek, SeekFrom};
    // 文件不比首块大多少时直接整读：先解析尾部一块、条数不够再扩到文件头，等于解析两遍。
    // 做空审查实测 1–4MB 的真实会话因此比改前慢约 2 倍（2.36MB：11.2→27.4ms），本机约五分之一
    // 的会话落在这个区间。按首块的倍数算而不是绝对字节：测试用极小首块压块边界，照样走尾读。
    const WHOLE_FILE_FACTOR: u64 = 8;
    let mut block = first_block.max(1);
    if end <= block.saturating_mul(WHOLE_FILE_FACTOR) {
        block = end;
    }
    // 已解析的窗口 `[parsed_start, window_end)` 与它的逐行结果。扩窗只读、只解析新增的前缀，
    // 拼在已解析的尾段前面：逐行解析是纯函数，跨行状态每轮按新窗口从头折叠。
    // 二期实测（release，真实 21MB 会话「加载更早」）：每轮整窗重解析时尾部被解析三遍，
    // 比整读慢 37%（67.7→92.9ms）。
    let mut parsed: Vec<ParsedLine> = Vec::new();
    let mut parsed_start: Option<u64> = None;
    let mut window_end = 0u64;
    loop {
        let raw = end.saturating_sub(block);
        // 已有窗口时 `parsed_start` 是行首，[raw, parsed_start) 以换行收尾，全是新内容。
        let read_end = parsed_start.unwrap_or(end).max(raw);
        let mut buf = vec![0; (read_end - raw) as usize];
        if file.seek(SeekFrom::Start(raw)).is_err() || file.read_exact(&mut buf).is_err() {
            return TailOutcome::Unreadable;
        }
        if raw == 0 {
            // 从文件头起：前缀整段解析，接上已解析的尾段，按整读口径（tail=false）折叠。
            let prefix_end = match parsed_start {
                Some(_) => buf.len(),
                None => memchr::memrchr(b'\n', &buf).map_or(0, |i| i + 1),
            };
            let mut lines = parse_lines(spec, path, 0, &buf[..prefix_end]);
            lines.append(&mut parsed);
            let region = fold_lines(spec, path, 0, &lines, false).into_region(lines);
            return TailOutcome::Window {
                lines: region.lines,
                agent_modes: region.agent_modes,
                undone: region.undone,
                start: 0,
                end: if parsed_start.is_some() { window_end } else { prefix_end as u64 },
            };
        }
        // 首个换行之前是半截行，跳过；最后一个换行之后是没写完的行，不碰。
        let body_at = memchr::memchr(b'\n', &buf).map(|i| i + 1);
        match parsed_start {
            Some(_) => {
                // 新增前缀里没有完整行（一整行横跨了新读的这段）：窗口不变，接着扩。
                if let Some(body_at) = body_at {
                    let mut lines = parse_lines(spec, path, raw + body_at as u64, &buf[body_at..]);
                    if !lines.is_empty() {
                        lines.append(&mut parsed);
                        parsed = lines;
                        parsed_start = Some(raw + body_at as u64);
                    }
                }
            }
            None => {
                let consumed = body_at
                    .and_then(|at| memchr::memrchr(b'\n', &buf[at..]).map(|i| i + 1))
                    .unwrap_or(0);
                let (Some(body_at), true) = (body_at, consumed > 0) else {
                    block = block.saturating_mul(4);
                    continue;
                };
                let start = raw + body_at as u64;
                parsed = parse_lines(spec, path, start, &buf[body_at..body_at + consumed]);
                parsed_start = Some(start);
                window_end = start + consumed as u64;
            }
        }
        let start = parsed_start.unwrap_or(end);
        let folded = fold_lines(spec, path, start, &parsed, true);
        let total = folded.total();
        if !folded.ambiguous && total > page_size {
            let region = folded.into_region(parsed);
            return TailOutcome::Window {
                lines: region.lines,
                agent_modes: region.agent_modes,
                undone: region.undone,
                start,
                end: window_end,
            };
        }
        block = if folded.ambiguous || total == 0 {
            block.saturating_mul(4)
        } else {
            let span = end - start;
            let estimate = span.saturating_mul(page_size as u64 + 1) / total as u64;
            (estimate.saturating_mul(3) / 2).max(block.saturating_mul(2))
        };
        // 不再「过半就整读」：扩窗只解析新增前缀，多扩一轮不白解析，按估算扩更省。
        // 二期实测（release，真实 21MB 会话「加载更早」，末尾 200 条横跨 7.5MB）：
        // 过半整读要解析整段 19.3MB。
    }
}

/// 顺序扫 `[0, end)`（`end` 在行首），对含任一 `markers` 的整行调用 `visit`（按文件顺序）。
/// 只做字节子串查找，命中才切行；跨块的行拼整后再判。读失败返回 false。
fn scan_marked_lines(
    file: &mut std::fs::File,
    end: u64,
    markers: &[&str],
    mut visit: impl FnMut(&[u8]),
) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    const CHUNK: usize = 4 << 20;
    if markers.is_empty() || end == 0 {
        return true;
    }
    let finders: Vec<_> = markers.iter().map(|m| memchr::memmem::Finder::new(m.as_bytes())).collect();
    let marked = |line: &[u8]| finders.iter().any(|f| f.find(line).is_some());
    if file.seek(SeekFrom::Start(0)).is_err() {
        return false;
    }
    let mut buf = vec![0; CHUNK];
    let mut carry: Vec<u8> = Vec::new();
    let mut pos = 0u64;
    let mut starts: Vec<usize> = Vec::new();
    while pos < end {
        let want = CHUNK.min((end - pos) as usize);
        if file.read_exact(&mut buf[..want]).is_err() {
            return false;
        }
        pos += want as u64;
        let mut chunk = &buf[..want];
        // 上一块留下的半行：补到本块第一个换行为止再判。
        if !carry.is_empty() {
            match memchr::memchr(b'\n', chunk) {
                None => {
                    carry.extend_from_slice(chunk);
                    continue;
                }
                Some(nl) => {
                    carry.extend_from_slice(&chunk[..nl]);
                    if marked(&carry) {
                        visit(&carry);
                    }
                    carry.clear();
                    chunk = &chunk[nl + 1..];
                }
            }
        }
        let body_len = memchr::memrchr(b'\n', chunk).map_or(0, |i| i + 1);
        let (body, rest) = chunk.split_at(body_len);
        // 命中位置 → 行首，去重排序后按文件顺序回调（多个标记可能命中同一行）。
        starts.clear();
        for finder in &finders {
            for hit in finder.find_iter(body) {
                starts.push(memchr::memrchr(b'\n', &body[..hit]).map_or(0, |i| i + 1));
            }
        }
        starts.sort_unstable();
        starts.dedup();
        for &line_start in &starts {
            let line_end = memchr::memchr(b'\n', &body[line_start..]).map_or(body.len(), |i| line_start + i);
            visit(&body[line_start..line_end]);
        }
        carry.extend_from_slice(rest);
    }
    if !carry.is_empty() && marked(&carry) {
        visit(&carry);
    }
    true
}

/// 首屏尾读（[`read_chat_delta_paged`] 的快路径）。只接管「整读 + 裁首屏」：offset=0 的首开
/// 或失效重读。返回 None = 不适用或判不准，调用方走整读——所以这里任何拿不准的情形都退回去。
///
/// 与整读逐字段一致的依据：
/// - items / start：窗口逐行结果与整读同段一致（[`tail_region`]），且窗口消息数多于一屏，
///   整读的裁剪点必然落在窗口里；窗口扩到文件头时本身就是整读。
/// - agent_modes：默认值 → 窗口前所有含模式标记的行按序折叠 → 叠上窗口内的折叠结果。
///   按维度覆盖、新维度按首次出现顺序追加，分两段做与一口气顺序折叠等价。
/// - plan：窗口内有就是它（最靠后）；没有而窗口前出现过计划标记，退回整读。
/// - undone：窗口不从文件头起时只反映窗口内。整读时它为真只影响「reset 且 items 为空」的
///   前端判定（ChatWindow.tsx emptyReset），而这种窗口的 items 恒多于一屏，观察不到差别。
fn tail_first_page(
    spec: &dyn TranscriptSpec,
    path: &Path,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
    page_size: usize,
    first_block: u64,
) -> Option<ChatDelta> {
    tail_first_page_window(spec, path, offset, prev_mtime, page_size, first_block).map(|(delta, _)| delta)
}

/// [`tail_first_page`] 本体，额外返回尾读窗口起点（测试据此确认确实只解析了尾部）。
fn tail_first_page_window(
    spec: &dyn TranscriptSpec,
    path: &Path,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
    page_size: usize,
    first_block: u64,
) -> Option<(ChatDelta, u64)> {
    let declared = spec.tail_read()?;
    if page_size == 0 {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    let (len, mtime) = (meta.len(), meta.modified().ok());
    let reset = transcript_reset(len, offset, mtime, prev_mtime);
    if !reset && offset != 0 {
        return None;
    }
    let TailOutcome::Window { mut lines, agent_modes: window_modes, undone, start: window_start, end } =
        tail_region(spec, path, &mut file, len, page_size, first_block)
    else {
        return None;
    };
    let plan = latest_plan(&lines);
    let mut agent_modes = spec.default_agent_modes();
    let mut prefix_plan = false;
    let plan_finders: Vec<_> = declared
        .plan_markers
        .iter()
        .map(|m| memchr::memmem::Finder::new(m.as_bytes()))
        .collect();
    let mode_finders: Vec<_> = declared
        .mode_markers
        .iter()
        .map(|m| memchr::memmem::Finder::new(m.as_bytes()))
        .collect();
    // 窗口内已有计划时，窗口前的计划标记无关紧要，不必为它扫。
    let markers: Vec<&str> = declared
        .mode_markers
        .iter()
        .chain(if plan.is_none() { declared.plan_markers } else { &[] })
        .copied()
        .collect();
    let scanned = scan_marked_lines(&mut file, window_start, &markers, |raw| {
        if plan.is_none() && plan_finders.iter().any(|f| f.find(raw).is_some()) {
            prefix_plan = true;
        }
        if mode_finders.iter().any(|f| f.find(raw).is_some()) {
            let line = String::from_utf8_lossy(raw);
            merge_modes(&mut agent_modes, spec.agent_modes_from_line(line.trim_end_matches(['\r', '\n'])));
        }
    });
    if !scanned || prefix_plan {
        return None;
    }
    merge_modes(&mut agent_modes, window_modes);
    let start = tail_window_start(&mut lines, window_start, page_size);
    let delta = ChatDelta {
        items: lines.into_iter().flat_map(|(_, items)| items).collect(),
        plan,
        agent_modes,
        start,
        offset: end,
        reset,
        undone,
        mtime,
    };
    Some((delta, window_start))
}

/// 按维度覆盖、新维度追加（[`fold_lines`] 与尾读窗口前的折叠共用同一规则）。
fn merge_modes(into: &mut Vec<AgentMode>, modes: Vec<AgentMode>) {
    for mode in modes {
        if let Some(previous) = into.iter_mut().find(|previous| previous.dimension == mode.dimension) {
            *previous = mode;
        } else {
            into.push(mode);
        }
    }
}

/// 逐行结果里最靠后的非空计划（整读与尾读共用，保证同一口径）。
fn latest_plan(lines: &[(u64, Vec<ChatItem>)]) -> Option<String> {
    lines.iter().rev().find_map(|(_, items)| {
        items.iter().rev().find_map(|item| match item {
            ChatItem::ToolUse { plan: Some(plan), .. } if !plan.trim().is_empty() => Some(plan.clone()),
            _ => None,
        })
    })
}

/// kimi 每条用户消息双写（`turn.prompt` + `context.append_message`，真机全量核对：前者
/// 后面必紧跟后者，正文一致）。前端 reducer 的相邻判重只覆盖增量轮询；接续会话的前序段
/// 不经 reducer，于是整段用户消息成对双显（用户实拍）。在解析层统一消掉，所有读取路径
/// 一处生效。双写两行的时间戳实测最多差 1ms，用户真连发两条相同消息必然隔得更久，与
/// reducer 同一个 2s 容差；缺时间戳的不判重（宁可双显，不吞用户消息）。
fn within_double_write(prev: Option<&str>, next: Option<&str>) -> bool {
    match (prev, next) {
        (Some(a), Some(b)) => {
            a == b
                || (a.get(..10) == b.get(..10)
                    && matches!((day_millis(a), day_millis(b)), (Some(x), Some(y)) if x.abs_diff(y) <= 2_000))
        }
        _ => false,
    }
}

/// `YYYY-MM-DDTHH:MM:SS(.fff)Z` 的当日毫秒数（日期另比）。只认 UTC `Z` 结尾的形态——
/// 各插件落 ChatItem 时统一写这种；别的形态返回 None，调用方退回严格同戳。
fn day_millis(ts: &str) -> Option<u64> {
    let time = ts.strip_suffix('Z')?.get(11..)?;
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut parts = hms.split(':').map(|part| part.parse::<u64>().ok());
    let (h, m, sec) = (parts.next()??, parts.next()??, parts.next()??);
    let ms = format!("{frac:0<3}").get(..3)?.parse::<u64>().ok()?;
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// 尾部裁剪：只留最后 `page_size` 条消息，整行为单位（边界行的消息要么全留要么全走，
/// 半行裁断会让同一行的 items 永远翻不出来）。返回保留窗口的首行字节；未触发裁剪返回 base。
/// `page_size == 0` 表示不裁剪（整段保留，start = base）。
fn tail_window_start(lines: &mut Vec<(u64, Vec<ChatItem>)>, base: u64, page_size: usize) -> u64 {
    if page_size == 0 {
        return base;
    }
    let total: usize = lines.iter().map(|(_, items)| items.len()).sum();
    if total <= page_size {
        return base;
    }
    let mut keep = 0;
    let mut index = lines.len();
    while index > 0 && keep < page_size {
        index -= 1;
        keep += lines[index].1.len();
    }
    let start = lines[index].0;
    lines.drain(..index);
    start
}

/// 无 transcript 规格的 agent（codex/kimi）以及 `TranscriptSpec::resolve_cwd` 默认实现共用：
/// 直接采信 DB 记录的 cwd，空白视作没有。
pub fn default_resolve_cwd(cwd: Option<&str>) -> Option<String> {
    cwd.filter(|c| !c.trim().is_empty()).map(str::to_string)
}

/// 单条缓存：已解析到的字节偏移 + 上次解析时的 mtime + 累积解析器 + 最近使用刻度（淘汰用）。
struct CacheEntry {
    offset: u64,
    mtime: Option<std::time::SystemTime>,
    parser: Box<dyn TranscriptParser>,
    last_used: u64,
}

/// transcript 增量解析缓存：transcript 是只追加的 JSONL，没必要每轮把整文件重读重解析
/// （几十 MB → 数百 ms，多个会话叠加可达数秒，每 ~300ms 一次会打满 CPU、拖慢整窗）。
/// 这里按文件路径缓存「已解析到的字节偏移 + 累积状态」，每轮只读+解析新追加的完整行，
/// 把每次刷新从 O(整文件) 降到 O(新增字节) ≈ 接近 0。
#[derive(Default)]
pub struct TranscriptCache {
    entries: std::collections::HashMap<String, CacheEntry>,
    tick: u64, // 单调递增的访问刻度，供 LRU 淘汰
}

/// 缓存条目上限：超出时淘汰最久未访问的条目，防长期运行无界增长。
///
/// 上限必须显著大于一次列表补页可能触碰的 transcript 数（session_query 的扫描上限
/// 是每请求千行级）：条目被挤掉即偏移归零，下次访问整文件重读重解析——一次超上限的
/// 扫描会把**活跃会话**的条目全部轮转出去，之后每 ~300ms 的刷新都退化成 O(整文件)，
/// 恰是本缓存要防的场景。单条目只存偏移与累积摘要（标题/预览等短字符串），放大到
/// 1024 的内存代价可忽略。
const MAX_CACHE_ENTRIES: usize = 1024;

/// read_transcript_delta 的结果：analyze 与 analyze_shared 共用的文件 IO 段。
enum DeltaOutcome {
    /// 打开/metadata/seek/读取失败：沿用已累积状态（不要用 len=0 当真实长度误判截断）。
    Unreadable,
    /// 无新增字节：仅需刷新 mtime。
    NoChange(Option<std::time::SystemTime>),
    /// 读到了新增（或需从头重读）的字节。
    Data {
        reset: bool,
        buf: Vec<u8>,
        mtime: Option<std::time::SystemTime>,
    },
}

/// 从 offset/prev_mtime 快照出发读取 transcript 的增量字节。纯文件 IO、不触碰缓存，
/// 供 analyze（持锁调用）/ analyze_shared（锁外调用）/ [`read_chat_delta`]（对话窗）共用。
/// 失效判据见 [`transcript_reset`]。
fn read_transcript_delta(
    path: impl AsRef<Path>,
    offset: u64,
    prev_mtime: Option<std::time::SystemTime>,
) -> DeltaOutcome {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return DeltaOutcome::Unreadable;
    };
    let (len, mtime) = match f.metadata() {
        Ok(m) => (m.len(), m.modified().ok()),
        Err(_) => return DeltaOutcome::Unreadable,
    };
    let reset = transcript_reset(len, offset, mtime, prev_mtime);
    if !reset && len == offset {
        return DeltaOutcome::NoChange(mtime);
    }
    let base = if reset { 0 } else { offset };
    if f.seek(SeekFrom::Start(base)).is_err() {
        return DeltaOutcome::Unreadable;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return DeltaOutcome::Unreadable;
    }
    DeltaOutcome::Data { reset, buf, mtime }
}

impl TranscriptCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 增量解析 path：只处理上次偏移之后新追加的「完整行」（末尾未结束的半行留到下次）。
    /// 失效检测用 len + mtime 双重校验：len < 偏移（截断）或 len == 偏移但 mtime 变了
    /// （等长重写）→ 从头重解析。打开/读失败 → 返回当前累积结果。
    /// `spec` 决定新建/重置条目时用哪种 agent 的解析器。
    /// 与 analyze_shared 共用 snapshot/read_transcript_delta/commit 三段（单一事实源），
    /// 差别仅在本方法独占 &mut self、无并发窗口。
    pub fn analyze(&mut self, spec: &dyn TranscriptSpec, path: &str) -> TranscriptInfo {
        let (offset, prev_mtime) = self.snapshot(spec, path);
        match read_transcript_delta(path, offset, prev_mtime) {
            DeltaOutcome::Unreadable => self.current_info(path),
            DeltaOutcome::NoChange(mtime) => self.touch_mtime(path, mtime),
            DeltaOutcome::Data { reset, buf, mtime } if reset || offset == 0 => {
                let fresh = fold_fresh(spec, &buf);
                self.commit_fresh(path, offset, fresh, mtime)
            }
            DeltaOutcome::Data { buf, mtime, .. } => self.commit(path, offset, &buf, mtime),
        }
    }

    /// 与 `analyze` 等价，但供多线程经 `Mutex` 共享缓存时调用：文件 IO（open/metadata/读新增字节）
    /// 全部在锁外进行，只有「取快照」与「提交结果」两个短临界区持锁——避免大 transcript 首读
    /// （数 MB、数百 ms）期间把其它调用方（如 get_live_sessions）一并阻塞在缓存锁上。
    /// 两个线程并发分析同一文件时可能重复读取，但提交前校验偏移快照，只有一方生效，状态不会错乱。
    pub fn analyze_shared(
        cache: &std::sync::Mutex<TranscriptCache>,
        spec: &dyn TranscriptSpec,
        path: &str,
    ) -> TranscriptInfo {
        let lock = || cache.lock().unwrap_or_else(|e| e.into_inner());
        // 短临界区 1：确保条目存在，取（已解析偏移, 上次 mtime）快照。
        let (offset, prev_mtime) = lock().snapshot(spec, path);
        // 锁外做全部文件 IO。失败时与 analyze 同语义：返回当前累积结果。
        match read_transcript_delta(path, offset, prev_mtime) {
            DeltaOutcome::Unreadable => lock().current_info(path),
            DeltaOutcome::NoChange(mtime) => lock().touch_mtime(path, mtime),
            // 从头读（冷启动 / 截断重读）：整文件的 fold_line 是大头（12MB ≈ 20ms，几个大会话
            // 同时冷启动时曾在锁里排成一串），改在锁外用全新解析器折叠完，临界区里只做替换。
            // 提交前同样比对偏移快照：期间别人已提交过就丢弃自己的结果，不会重复计数。
            DeltaOutcome::Data { reset, buf, mtime } if reset || offset == 0 => {
                let fresh = fold_fresh(spec, &buf);
                lock().commit_fresh(path, offset, fresh, mtime)
            }
            // 短临界区 2：偏移仍与快照一致才提交；其它线程已推进则弃用本次读取、复用其结果。
            // 增量只含新追加的行（O(新增字节)），累积解析器不可克隆，留在锁内续折。
            DeltaOutcome::Data { buf, mtime, .. } => lock().commit(path, offset, &buf, mtime),
        }
    }

    /// [`fold_fresh`] 结果的提交：条目偏移仍等于快照（期间没人推进）才整体替换解析器与偏移，
    /// 否则弃用本次结果、返回已有累积状态。条目在锁外窗口被 LRU 淘汰时按新条目灌入
    /// （本结果从文件头读起，可安全重建）。
    fn commit_fresh(
        &mut self,
        path: &str,
        snap_offset: u64,
        (parser, consumed): (Box<dyn TranscriptParser>, u64),
        mtime: Option<std::time::SystemTime>,
    ) -> TranscriptInfo {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.entries.get_mut(path) {
            entry.last_used = tick;
            if entry.offset != snap_offset {
                return entry.parser.to_info();
            }
            entry.parser = parser;
            entry.offset = consumed;
            entry.mtime = mtime;
            return entry.parser.to_info();
        }
        let info = parser.to_info();
        self.entries.insert(
            path.to_string(),
            CacheEntry {
                offset: consumed,
                mtime,
                parser,
                last_used: tick,
            },
        );
        info
    }

    /// analyze / analyze_shared 临界区 1：确保条目存在（含 LRU 淘汰），返回（偏移, mtime）快照。不做文件 IO。
    fn snapshot(
        &mut self,
        spec: &dyn TranscriptSpec,
        path: &str,
    ) -> (u64, Option<std::time::SystemTime>) {
        self.tick += 1;
        if !self.entries.contains_key(path) && self.entries.len() >= MAX_CACHE_ENTRIES {
            if let Some(k) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&k);
            }
        }
        let tick = self.tick;
        let entry = self
            .entries
            .entry(path.to_string())
            .or_insert_with(|| CacheEntry {
                offset: 0,
                mtime: None,
                parser: spec.new_parser(),
                last_used: tick,
            });
        entry.last_used = tick;
        (entry.offset, entry.mtime)
    }

    /// 当前累积结果；条目不存在（锁外窗口内被 LRU 淘汰）时返回空结果。
    fn current_info(&mut self, path: &str) -> TranscriptInfo {
        self.entries
            .get(path)
            .map(|e| e.parser.to_info())
            .unwrap_or_default()
    }

    /// 无新增时刷新 mtime 并返回累积结果。
    fn touch_mtime(&mut self, path: &str, mtime: Option<std::time::SystemTime>) -> TranscriptInfo {
        match self.entries.get_mut(path) {
            Some(e) => {
                e.mtime = mtime;
                e.parser.to_info()
            }
            None => TranscriptInfo::default(),
        }
    }

    /// analyze / analyze_shared 临界区 2（增量）：把新追加的完整行续折进累积解析器。仅当条目偏移
    /// 仍等于快照偏移（期间无其它线程推进）时生效；否则弃用本次读取，直接返回已有结果。
    /// 从文件头读起的情形不走这里，见 [`Self::commit_fresh`]。
    fn commit(
        &mut self,
        path: &str,
        snap_offset: u64,
        buf: &[u8],
        mtime: Option<std::time::SystemTime>,
    ) -> TranscriptInfo {
        self.tick += 1;
        let tick = self.tick;
        // 条目在锁外窗口被 LRU 淘汰：增量读没有前文可接，丢弃，下轮从头重来。
        let Some(entry) = self.entries.get_mut(path) else {
            return TranscriptInfo::default();
        };
        entry.last_used = tick;
        if entry.offset != snap_offset {
            return entry.parser.to_info();
        }
        if let Some(nl) = buf.iter().rposition(|&b| b == b'\n') {
            entry.offset += (nl + 1) as u64;
            fold_complete_lines(entry.parser.as_mut(), &buf[..=nl]);
        }
        entry.mtime = mtime;
        entry.parser.to_info()
    }
}

/// 把一段以换行结尾的完整行逐行折进解析器（增量与从头两条提交路径同一份切行/转码口径）。
fn fold_complete_lines(parser: &mut dyn TranscriptParser, complete: &[u8]) {
    let chunk = String::from_utf8_lossy(complete);
    for line in chunk.lines() {
        parser.fold_line(line);
    }
}

/// 从文件头读到的字节在**锁外**折成一个全新解析器，返回（解析器, 已消费到的字节偏移）。
/// 末尾没换行的半行不消费，留给下一轮——与增量提交同口径。
fn fold_fresh(spec: &dyn TranscriptSpec, buf: &[u8]) -> (Box<dyn TranscriptParser>, u64) {
    let mut parser = spec.new_parser();
    let consumed = buf.iter().rposition(|&b| b == b'\n').map_or(0, |nl| nl + 1);
    fold_complete_lines(parser.as_mut(), &buf[..consumed]);
    (parser, consumed as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 缓存的增量/失效语义要用一个真实解析器才测得出来（断言标题随追加变化），故借 claude 的 spec。
    use crate::plugins::claude::transcript::{analyze_transcript, ClaudeTranscript};

    fn write_tmp(name: &str, content: &str) -> std::path::PathBuf {
        let p =
            std::env::temp_dir().join(format!("meowo_cache_{}_{}.jsonl", std::process::id(), name));
        std::fs::write(&p, content).unwrap();
        p
    }

    /// 测试用最小 spec：`m<N>:<tag>` 产出 N 条 Meta 消息（id 为 `<tag>-<i>`），
    /// `p:<正文>` 产出一条计划。可造「一行多条」的边界与首屏外的计划。
    struct StubSpec;

    impl TranscriptSpec for StubSpec {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            Box::new(ChatOnlyParser)
        }
        fn resolve_transcript_path(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: &str,
        ) -> Option<std::path::PathBuf> {
            None
        }
        fn resolve_title(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<String> {
            None
        }
        fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
            if let Some(plan) = line.strip_prefix("p:") {
                return vec![TranscriptEvent::ToolCall {
                    id: "plan".into(),
                    timestamp: None,
                    name: "ExitPlanMode".into(),
                    summary: String::new(),
                    subagent: None,
                    detail: None,
                    plan: Some(plan.into()),
                }];
            }
            let Some((n, tag)) = line
                .strip_prefix('m')
                .and_then(|rest| rest.split_once(':'))
            else {
                return Vec::new();
            };
            let n: usize = n.parse().unwrap_or(0);
            (0..n)
                .map(|i| TranscriptEvent::Metadata {
                    id: format!("{tag}-{i}"),
                    timestamp: None,
                    kind: "stub".into(),
                    compaction: None,
                })
                .collect()
        }
    }

    /// 每个元素是一条行：(该行的消息数, tag)。
    fn stub_lines(lines: &[(usize, &str)]) -> String {
        lines
            .iter()
            .map(|(n, tag)| format!("m{n}:{tag}\n"))
            .collect()
    }

    #[test]
    fn paged_history_keeps_latest_plan_outside_visible_items() {
        let path = write_tmp(
            "plan_outside_page",
            "p:# 旧计划\nm1:one\np:# 最新计划\nm1:two\nm1:three\n",
        );
        let delta = read_chat_delta_paged(&StubSpec, &path, 0, None, 1);
        assert_eq!(delta.items.len(), 1);
        assert!(delta.items.iter().all(|item| matches!(item, ChatItem::Meta { .. })));
        assert_eq!(delta.plan.as_deref(), Some("# 最新计划"));
        assert!(delta.start > 0);
        let _ = std::fs::remove_file(path);
    }

    /// 多发委派（kimi `AgentSwarm`）探测用最小 subagent spec：`locate_streams` 返回预制
    /// 分支流，与真实 provider 的落盘布局解耦——这里只验分支级状态的归一与下发口径，
    /// 定位环节由 subagent_live.rs 的真机测试负责。
    struct StubSubagents;

    impl SubagentSpec for StubSubagents {
        fn detect_call(&self, _: &str, _: Option<&serde_json::Value>) -> Option<SubagentRef> {
            None
        }
        fn locate_streams(&self, _: &Path, _: &str) -> Vec<SubagentStream> {
            vec![
                SubagentStream {
                    label: Some("agent-1".into()),
                    status: Some("completed".into()),
                    finished_at: Some("2026-08-28T08:00:00Z".into()),
                    path: PathBuf::new(),
                },
                SubagentStream {
                    label: Some("agent-2".into()),
                    status: Some("running".into()),
                    finished_at: None,
                    path: PathBuf::new(),
                },
                SubagentStream {
                    label: Some("agent-3".into()),
                    status: None,
                    finished_at: Some("2026-08-28T08:05:00Z".into()),
                    path: PathBuf::new(),
                },
            ]
        }
        fn parse_stream_line(&self, _: &str) -> Vec<TranscriptEvent> {
            Vec::new()
        }
    }

    static STUB_SUBAGENTS: StubSubagents = StubSubagents;

    struct SwarmSpec;

    impl TranscriptSpec for SwarmSpec {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            Box::new(ChatOnlyParser)
        }
        fn resolve_transcript_path(
            &self,
            _: Option<&str>,
            _: Option<&str>,
            _: &str,
        ) -> Option<std::path::PathBuf> {
            None
        }
        fn resolve_title(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<String> {
            None
        }
        fn subagents(&self) -> Option<&'static dyn SubagentSpec> {
            Some(&STUB_SUBAGENTS)
        }
    }

    /// 多发委派的实测要逐分支下发：面板把 `×N` 展开成一行一分支后，每行的状态只认
    /// 自己那条侧车流；聚合 `status`/`finished_at` 的行为保持不变。
    #[test]
    fn probe_reports_per_branch_state_for_swarm_delegations() {
        let p = write_tmp("probe_branches", "m1:x\n");
        let probe = probe_subagent_state(&SwarmSpec, &p, "call-1")
            .expect("有分支留下了状态信号，聚合应给出结论");
        // 聚合口径不变：还有分支在跑 → running，且没有整体「结束时刻」。
        assert_eq!(probe.status, "running");
        assert!(probe.finished_at.is_none());
        // 逐分支：顺序与 locate_streams 的定位一致（面板按序号对应分支行）；
        // completed 带结束时刻，running 不带。
        assert_eq!(probe.branches.len(), 3);
        assert_eq!(probe.branches[0].label.as_deref(), Some("agent-1"));
        assert_eq!(probe.branches[0].status, "completed");
        assert_eq!(
            probe.branches[0].finished_at.as_deref(),
            Some("2026-08-28T08:00:00Z")
        );
        assert_eq!(probe.branches[1].label.as_deref(), Some("agent-2"));
        assert_eq!(probe.branches[1].status, "running");
        assert!(probe.branches[1].finished_at.is_none());
        // 无信号的流不能断言结束：分支侧与聚合同一条规则——标 running，结束时刻扣下。
        assert_eq!(probe.branches[2].status, "running");
        assert!(probe.branches[2].finished_at.is_none());
        std::fs::remove_file(&p).ok();
    }

    fn meta_ids(items: &[ChatItem]) -> Vec<&str> {
        items
            .iter()
            .map(|item| match item {
                ChatItem::Meta { id, .. } => id.as_str(),
                other => panic!("stub 只产 Meta，拿到 {other:?}"),
            })
            .collect()
    }

    /// 首屏只留尾部一屏，start 指向保留窗口的首行字节（=「加载更早」的上界）。
    #[test]
    fn chat_first_page_trims_to_tail_and_reports_window_start() {
        let content = stub_lines(&[(1, "a"), (1, "b"), (1, "c"), (1, "d"), (1, "e")]);
        let p = write_tmp("chat_page_tail", &content);
        let page = read_chat_delta_paged(&StubSpec, &p, 0, None, 2);
        assert_eq!(meta_ids(&page.items), ["d-0", "e-0"], "只留尾部 2 条");
        assert_eq!(
            page.start as usize,
            "m1:a\nm1:b\nm1:c\n".len(),
            "start 必须对准保留窗口的首行"
        );
        assert_eq!(page.offset, content.len() as u64, "尾部记账不受首屏裁剪影响");
        std::fs::remove_file(&p).ok();
    }

    /// 裁剪按整行进行：边界行产出多条消息时全留（腰斩的那部分会永远翻不出来——
    /// 上一屏的上界是行首），宁可一屏多于 page_size。
    #[test]
    fn chat_first_page_trim_never_splits_a_line() {
        let content = stub_lines(&[(1, "a"), (3, "b")]);
        let p = write_tmp("chat_line_grain", &content);
        let page = read_chat_delta_paged(&StubSpec, &p, 0, None, 1);
        assert_eq!(meta_ids(&page.items), ["b-0", "b-1", "b-2"]);
        assert_eq!(page.start as usize, "m1:a\n".len());
        std::fs::remove_file(&p).ok();
    }

    /// 增量不裁剪：窗口关闭期间积压的大批次一次赶上来可能超过 page_size，
    /// 按首屏规则裁掉就永久丢消息。
    #[test]
    fn chat_delta_paged_never_trims_incremental_reads() {
        let p = write_tmp("chat_page_inc", &stub_lines(&[(1, "a"), (1, "b")]));
        let first = read_chat_delta_paged(&StubSpec, &p, 0, None, 10);
        assert_eq!(first.items.len(), 2);
        std::fs::write(
            &p,
            stub_lines(&[(1, "a"), (1, "b"), (1, "c"), (1, "d"), (1, "e"), (1, "f")]),
        )
        .unwrap();
        let delta = read_chat_delta_paged(&StubSpec, &p, first.offset, first.mtime, 2);
        assert!(!delta.reset);
        assert_eq!(delta.items.len(), 4, "增量批次不裁");
        assert_eq!(delta.start, first.offset);
        std::fs::remove_file(&p).ok();
    }

    /// 向上翻页：逐屏翻到文件头，各屏拼接与整读无重无漏（含多消息边界行）。
    #[test]
    fn chat_window_paginates_back_to_head_without_gaps() {
        let lines = [(1, "a"), (2, "b"), (1, "c"), (1, "d"), (3, "e"), (1, "f"), (1, "g")];
        let p = write_tmp("chat_page_walk", &stub_lines(&lines));
        let first = read_chat_delta_paged(&StubSpec, &p, 0, None, 4);
        assert!(first.start > 0, "10 条裁到一屏 4 条，前面必须还有");
        let mut pages = vec![first.items];
        let mut end = first.start;
        while end > 0 {
            let window = read_chat_window(&StubSpec, &p, end, 4).unwrap();
            assert_eq!(window.has_more, window.start > 0, "has_more 与 start 同口径");
            end = window.start;
            pages.push(window.items);
        }
        let walked: Vec<_> = pages.into_iter().rev().flatten().collect();
        let full = read_chat_delta(&StubSpec, &p, 0, None);
        assert_eq!(walked, full.items, "逐屏上翻拼接必须与整读一致");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn transcript_events_map_to_the_stable_ipc_contract() {
        let timestamp = Some("2026-07-19T10:00:00Z".to_string());
        assert_eq!(
            ChatItem::from(TranscriptEvent::AssistantChunk {
                id: "a".into(),
                timestamp: timestamp.clone(),
                text: "delta".into(),
            }),
            ChatItem::AssistantDelta {
                id: "a".into(),
                timestamp: timestamp.clone(),
                text: "delta".into(),
            }
        );
        assert_eq!(
            ChatItem::from(TranscriptEvent::ToolResult {
                id: "result".into(),
                timestamp: timestamp.clone(),
                tool_call_id: Some("call".into()),
                text: "ok".into(),
                is_error: false,
                subagent: None,
                patch: None,
            }),
            ChatItem::ToolResult {
                id: "result".into(),
                timestamp: timestamp.clone(),
                tool_use_id: Some("call".into()),
                text: "ok".into(),
                is_error: false,
                subagent: None,
                patch: None,
            }
        );
        assert_eq!(
            ChatItem::from(TranscriptEvent::Metadata {
                id: "compact".into(),
                timestamp,
                kind: "compact".into(),
                compaction: None,
            }),
            ChatItem::Meta {
                id: "compact".into(),
                timestamp: Some("2026-07-19T10:00:00Z".into()),
                kind: "compact".into(),
                compaction: None,
            }
        );
    }

    /// 失效判据的四种边界。两条读取路径共用它，跑偏一处就是整段内容静默丢失。
    #[test]
    fn transcript_reset_covers_truncation_and_same_length_rewrite() {
        use super::transcript_reset;
        let t1 = std::time::SystemTime::UNIX_EPOCH;
        let t2 = t1 + std::time::Duration::from_secs(1);
        // 正常追加：长度变长，不重读。
        assert!(!transcript_reset(200, 100, Some(t2), Some(t1)));
        // 截断/重建：文件比读过的还短。
        assert!(transcript_reset(50, 100, Some(t2), Some(t1)));
        // 等长重写：长度没变但 mtime 动了。
        assert!(transcript_reset(100, 100, Some(t2), Some(t1)));
        // 等长且 mtime 未变：真的没动过。
        assert!(!transcript_reset(100, 100, Some(t1), Some(t1)));
        // 首次读取（无历史 mtime）：无从判断「变了」，不得误判成重写。
        assert!(!transcript_reset(0, 0, Some(t1), None));
    }

    /// 等长重写必须触发 reset 重读。CLI 压缩/改写 transcript 后字节数可能与上次**完全相同**，
    /// 只比 offset 与文件长度会认为「无变化」，于是整段新内容被静默漏掉——对话窗口表现为
    /// 消息突然不再更新。mtime 是唯一能区分这种情况的信号。
    #[test]
    fn chat_delta_detects_same_length_rewrite() {
        let line_a = format!(
            "{}\n",
            r#"{"type":"user","message":{"role":"user","content":"AAA"}}"#
        );
        let p = write_tmp("chat_rewrite", &line_a);

        let first = read_chat_delta(&ClaudeTranscript, &p, 0, None);
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.offset, line_a.len() as u64);
        assert!(first.mtime.is_some());

        // 同样长度、不同内容地整体重写。mtime 必须比上一次新，否则这个测试本身无意义。
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let line_b = format!(
            "{}\n",
            r#"{"type":"user","message":{"role":"user","content":"BBB"}}"#
        );
        assert_eq!(line_a.len(), line_b.len(), "两行必须等长，否则测不到该分支");
        std::fs::write(&p, &line_b).unwrap();

        let second = read_chat_delta(&ClaudeTranscript, &p, first.offset, first.mtime);
        assert!(second.reset, "等长重写必须 reset，否则新内容会被漏掉");
        assert_eq!(second.items.len(), 1);
        assert_eq!(second.start, 0, "reset 时从头读");

        // 对照：mtime 也没变（真正无变化）时不该 reset，避免每轮白读整个文件。
        let third = read_chat_delta(&ClaudeTranscript, &p, second.offset, second.mtime);
        assert!(!third.reset);
        assert!(third.items.is_empty());
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn cache_incremental_matches_full_and_picks_up_appends() {
        use std::io::Write;
        let p = write_tmp(
            "cache_inc",
            concat!(
                r#"{"type":"ai-title","aiTitle":"标题A"}"#,
                "\n",
                r#"{"type":"assistant","uuid":"u1","message":{"role":"assistant","usage":{"input_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"hi"}]}}"#,
                "\n",
            ),
        );
        let mut cache = TranscriptCache::new();
        let i1 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
        assert_eq!(i1.title.as_deref(), Some("标题A"));
        assert_eq!(i1.context_tokens, Some(1000));

        // 追加新一轮（带更大 usage + 自定义标题），增量解析应读到。
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"标题B"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","uuid":"u2","message":{{"role":"assistant","usage":{{"input_tokens":40000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0}},"content":[{{"type":"tool_use","name":"Bash","input":{{}}}}]}}}}"#
        )
        .unwrap();
        drop(f);

        let i2 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
        // 与全量解析结果一致
        let full = analyze_transcript(p.to_str().unwrap());
        assert_eq!(i2.title.as_deref(), Some("标题B")); // custom 覆盖 ai
        assert_eq!(i2.context_tokens, Some(40000));
        assert_eq!(i2.title, full.title);
        assert_eq!(i2.context_tokens, full.context_tokens);

        // 再次调用、无新增 → 结果稳定。
        let i3 = cache.analyze(&ClaudeTranscript, p.to_str().unwrap());
        assert_eq!(i3.context_tokens, Some(40000));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn cache_detects_same_length_rewrite_by_mtime() {
        // 等长重写：len 不变但 mtime 变了 → 应从头重解析，而不是沿用旧状态。
        let line_a = r#"{"type":"ai-title","aiTitle":"AAAA"}"#;
        let line_b = r#"{"type":"ai-title","aiTitle":"BBBB"}"#;
        assert_eq!(line_a.len(), line_b.len());
        let p = write_tmp("cache_rewrite", &format!("{line_a}\n"));
        let mut cache = TranscriptCache::new();
        assert_eq!(
            cache
                .analyze(&ClaudeTranscript, p.to_str().unwrap())
                .title
                .as_deref(),
            Some("AAAA")
        );

        // 等长重写，循环到 mtime 确认变化为止（兼容粗粒度文件系统，NTFS/APFS 首轮即过）。
        let mtime0 = std::fs::metadata(&p).unwrap().modified().unwrap();
        for _ in 0..120 {
            std::thread::sleep(std::time::Duration::from_millis(25));
            std::fs::write(&p, format!("{line_b}\n")).unwrap();
            if std::fs::metadata(&p).unwrap().modified().unwrap() != mtime0 {
                break;
            }
        }
        assert_ne!(
            std::fs::metadata(&p).unwrap().modified().unwrap(),
            mtime0,
            "mtime 未变化，无法验证缓存失效"
        );
        assert_eq!(
            cache
                .analyze(&ClaudeTranscript, p.to_str().unwrap())
                .title
                .as_deref(),
            Some("BBBB")
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn analyze_shared_matches_analyze_on_append_truncate_and_missing() {
        // 锁外 IO 版必须与 analyze 语义一致：首读、追加增量、截断重读、文件缺失四种路径。
        use std::io::Write;
        use std::sync::Mutex;
        let spec = &ClaudeTranscript;
        let p = write_tmp(
            "cache_shared",
            concat!(r#"{"type":"ai-title","aiTitle":"标题A"}"#, "\n"),
        );
        let path = p.to_str().unwrap();
        let cache = Mutex::new(TranscriptCache::new());

        // 首读
        let i1 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i1.title.as_deref(), Some("标题A"));

        // 追加 → 增量读到
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, r#"{{"type":"custom-title","customTitle":"标题B"}}"#).unwrap();
        drop(f);
        let i2 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i2.title.as_deref(), Some("标题B"));

        // 无新增 → 结果稳定
        let i3 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i3.title.as_deref(), Some("标题B"));

        // 截断成更短内容 → 从头重解析
        std::fs::write(&p, concat!(r#"{"type":"ai-title","aiTitle":"C"}"#, "\n")).unwrap();
        let i4 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i4.title.as_deref(), Some("C"));

        // 文件消失 → 沿用已累积结果（与 analyze 一致）
        std::fs::remove_file(&p).ok();
        let i5 = TranscriptCache::analyze_shared(&cache, spec, path);
        assert_eq!(i5.title.as_deref(), Some("C"));
    }

    /// 计数解析器：标题 = 「已折叠行数:最后一行」。同一行被折两次（重复计数）或漏折都会让
    /// 计数与文件实际行数对不上——比真实解析器的「最后一条标题」更能暴露并发提交的错乱。
    struct CountSpec;
    struct CountParser(usize, String);

    impl TranscriptParser for CountParser {
        fn fold_line(&mut self, line: &str) {
            self.0 += 1;
            self.1 = line.to_string();
        }
        fn to_info(&self) -> TranscriptInfo {
            TranscriptInfo {
                title: Some(format!("{}:{}", self.0, self.1)),
                ..TranscriptInfo::default()
            }
        }
    }

    impl TranscriptSpec for CountSpec {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            Box::new(CountParser(0, String::new()))
        }
        fn resolve_transcript_path(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<PathBuf> {
            None
        }
        fn resolve_title(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<String> {
            None
        }
    }

    /// 锁外解析：多个线程同时对同一文件冷读 + 追加中的增量读，最终累积结果与单线程整读一致
    /// （不丢增量、不重复计数）；中途任何一次返回的计数都不超过文件实际完整行数。
    #[test]
    fn analyze_shared_concurrent_reads_match_single_thread() {
        use std::io::Write;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let p = write_tmp("cache_concurrent", "");
        let path = p.to_str().unwrap().to_string();
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
            for i in 0..3000 {
                writeln!(f, "line-{i}-{}", "z".repeat(i % 97)).unwrap();
            }
        }
        let written = AtomicUsize::new(3000);
        for round in 0..5 {
            let cache = std::sync::Mutex::new(TranscriptCache::new());
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    scope.spawn(|| {
                        for _ in 0..20 {
                            let before = written.load(Ordering::SeqCst);
                            let info = TranscriptCache::analyze_shared(&cache, &CountSpec, &path);
                            let count: usize = info.title.unwrap().split(':').next().unwrap().parse().unwrap();
                            // 写线程只增不减，故读后再取一次上界。
                            let after = written.load(Ordering::SeqCst);
                            assert!(count <= after, "计数 {count} 超过已写完整行 {after}（读前 {before}）");
                        }
                    });
                }
                scope.spawn(|| {
                    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
                    for i in 0..200 {
                        let n = written.load(Ordering::SeqCst);
                        // 先写半行再补完：读者可能恰好看到半行，必须留到下轮。
                        write!(f, "late-{round}-{i}").unwrap();
                        f.flush().unwrap();
                        // 先记账再补换行：任何时刻「完整行数 ≤ 计数器」，读者的上界断言才成立。
                        written.store(n + 1, Ordering::SeqCst);
                        writeln!(f, "-done").unwrap();
                        f.flush().unwrap();
                    }
                });
            });
            let shared = TranscriptCache::analyze_shared(&cache, &CountSpec, &path);
            let single = TranscriptCache::new().analyze(&CountSpec, &path);
            assert_eq!(shared, single, "round {round}");
            assert!(shared.title.unwrap().starts_with(&format!("{}:", written.load(Ordering::SeqCst))));
        }
        std::fs::remove_file(&p).ok();
    }

    // ── 尾读（首屏 / 加载更早）与整读的一致性 ──

    /// 对照组：把真实 spec 的一切原样转发，唯独不声明尾读——于是走的就是改动前的整读路径。
    struct NoTail<'a>(&'a dyn TranscriptSpec);

    impl TranscriptSpec for NoTail<'_> {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            self.0.new_parser()
        }
        fn resolve_transcript_path(&self, a: Option<&str>, b: Option<&str>, c: &str) -> Option<PathBuf> {
            self.0.resolve_transcript_path(a, b, c)
        }
        fn resolve_title(&self, a: Option<&str>, b: Option<&str>, c: &str) -> Option<String> {
            self.0.resolve_title(a, b, c)
        }
        fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
            self.0.parse_transcript_line(line)
        }
        fn parse_transcript_line_in(&self, path: &Path, line: &str) -> Vec<TranscriptEvent> {
            self.0.parse_transcript_line_in(path, line)
        }
        fn agent_modes_from_line(&self, line: &str) -> Vec<AgentMode> {
            self.0.agent_modes_from_line(line)
        }
        fn chat_line_mark(&self, line: &str) -> Option<ChatLineMark> {
            self.0.chat_line_mark(line)
        }
        fn default_agent_modes(&self) -> Vec<AgentMode> {
            self.0.default_agent_modes()
        }
        fn supports_chat(&self) -> bool {
            self.0.supports_chat()
        }
    }

    /// 尾读结果与整读逐字段比对（undone 除外，理由见 tail_first_page 注释）。返回尾读是否接管。
    /// 返回 true = 真的只解析了尾部（窗口不从文件头起）。
    fn tail_matches_full(spec: &dyn TranscriptSpec, path: &Path, page: usize, block: u64) -> bool {
        let full = read_chat_delta_paged(&NoTail(spec), path, 0, None, page);
        let Some((tail, window_start)) = tail_first_page_window(spec, path, 0, None, page, block) else {
            return false;
        };
        let at = format!("page {page} block {block}");
        assert_eq!(tail.items, full.items, "{at}");
        assert_eq!(tail.start, full.start, "{at}");
        assert_eq!(tail.plan, full.plan, "{at}");
        assert_eq!(tail.agent_modes, full.agent_modes, "{at}");
        assert_eq!(tail.offset, full.offset, "{at}");
        assert_eq!(tail.reset, full.reset, "{at}");
        assert_eq!(tail.mtime, full.mtime, "{at}");
        if window_start == 0 {
            // 扩到了文件头：本身就是整读，连 undone 也必须一致。
            assert_eq!(tail.undone, full.undone, "{at}");
        } else {
            assert!(tail.items.len() >= page, "{at}");
        }
        window_start > 0
    }

    /// 合成 claude transcript：`turns` 轮单链，每轮 提示/正文/工具/结果/收尾五行；
    /// `modes`=(轮次, 值) 在该轮前插 permission-mode 行；`forks`=(轮次, 指回的更早轮次) 让该轮
    /// 提示从更早的收尾节点长出新分支（/rewind 形态）；`dups` 的轮次把提示双写一遍。
    /// 每行带一段填充，凑出跨多个尾读块的体积。
    fn claude_session(turns: usize, modes: &[(usize, &str)], forks: &[(usize, usize)], dups: &[usize]) -> String {
        let pad = "p".repeat(300);
        let mut out = String::new();
        let mut prev: Option<String> = None;
        for t in 0..turns {
            let ts = format!("2026-11-10T{:02}:{:02}:{:02}.000Z", (t / 3600) % 24, (t / 60) % 60, t % 60);
            for (at, mode) in modes {
                if *at == t {
                    out.push_str(&format!(r#"{{"type":"permission-mode","permissionMode":"{mode}"}}"#));
                    out.push('\n');
                }
            }
            let parent = forks
                .iter()
                .find(|(at, _)| *at == t)
                .map(|(_, back)| format!("e{back}"))
                .or(prev.clone());
            let parent = parent.map_or("null".to_string(), |p| format!("\"{p}\""));
            out.push_str(&format!(r#"{{"parentUuid":{parent},"type":"user","message":{{"content":"提示 {t}"}},"uuid":"u{t}","timestamp":"{ts}","permissionMode":"x{t}"}}"#));
            out.push('\n');
            let mut last = format!("u{t}");
            if dups.contains(&t) {
                out.push_str(&format!(r#"{{"parentUuid":"{last}","type":"user","message":{{"content":"提示 {t}"}},"uuid":"d{t}","timestamp":"{ts}"}}"#));
                out.push('\n');
                last = format!("d{t}");
            }
            out.push_str(&format!(r#"{{"parentUuid":"{last}","type":"assistant","message":{{"id":"m{t}a","content":[{{"type":"text","text":"回答 {t} {pad}"}}]}},"uuid":"a{t}","timestamp":"{ts}"}}"#));
            out.push('\n');
            out.push_str(&format!(r#"{{"parentUuid":"a{t}","type":"assistant","message":{{"id":"m{t}b","content":[{{"type":"tool_use","id":"toolu_{t}","name":"Bash","input":{{"command":"ls {t}"}}}}]}},"uuid":"t{t}","timestamp":"{ts}"}}"#));
            out.push('\n');
            out.push_str(&format!(r#"{{"parentUuid":"t{t}","type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"toolu_{t}","content":"结果 {pad}"}}]}},"uuid":"r{t}","timestamp":"{ts}"}}"#));
            out.push('\n');
            out.push_str(&format!(r#"{{"parentUuid":"r{t}","type":"system","subtype":"turn_duration","uuid":"e{t}"}}"#));
            out.push('\n');
            prev = Some(format!("e{t}"));
        }
        out
    }

    const PAGES: [usize; 4] = [1, 7, 60, 200];
    const BLOCKS: [u64; 5] = [64, 777, 4096, 65536, TAIL_FIRST_BLOCK];

    /// 首屏：普通长会话（窗口前有模式切换、双写的提示、一行没写完的尾巴）尾读与整读一致，
    /// 且确实走了尾读。
    #[test]
    fn tail_first_page_matches_full_read_for_claude() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let mut content = claude_session(
            900,
            &[(0, "default"), (10, "plan"), (300, "acceptEdits")],
            &[],
            &[5, 450, 897],
        );
        content.push_str(r#"{"type":"user","message":{"content":"没写完"#);
        let path = write_tmp("tail_claude_plain", &content);
        let len = content.len() as u64;
        let mut taken = 0;
        let mut eligible = 0;
        for page in PAGES {
            for block in BLOCKS {
                let tail = tail_matches_full(&CLAUDE_TRANSCRIPT, &path, page, block);
                // 不比首块大 8 倍的文件按规则直接整读（tail_region 的 WHOLE_FILE_FACTOR），不该走尾读。
                if len <= block * 8 {
                    assert!(!tail, "文件只有首块的 {} 倍，应直接整读（page={page} block={block}）", len / block);
                } else {
                    eligible += 1;
                    taken += usize::from(tail);
                }
            }
        }
        assert!(eligible >= PAGES.len() * 2, "合成文件太小，测不到尾读：len={len}");
        assert!(taken * 5 >= eligible * 4, "尾读应接管绝大多数够大的组合，实际 {taken}/{eligible}");
        // 公共入口（默认块）与整读一致。
        let full = read_chat_delta_paged(&NoTail(&CLAUDE_TRANSCRIPT), &path, 0, None, 200);
        let public = read_chat_delta_paged(&CLAUDE_TRANSCRIPT, &path, 0, None, 200);
        assert_eq!((public.items, public.start, public.agent_modes), (full.items, full.start, full.agent_modes));
        // 小文件（不到一个初始块）直接整段解析，与整读一致；空文件给默认模式。
        let small = write_tmp("tail_claude_small", &claude_session(20, &[(3, "plan")], &[], &[]));
        assert!(!tail_matches_full(&CLAUDE_TRANSCRIPT, &small, 5, TAIL_FIRST_BLOCK));
        assert!(!tail_matches_full(&CLAUDE_TRANSCRIPT, &small, 500, 64));
        let empty = write_tmp("tail_claude_empty", "");
        assert!(!tail_matches_full(&CLAUDE_TRANSCRIPT, &empty, 5, 64));
        std::fs::remove_file(empty).ok();
        std::fs::remove_file(path).ok();
        std::fs::remove_file(small).ok();
    }

    /// /rewind 分叉：分叉点在窗口外（整读会把中间一大段收回）、在窗口边界附近、在窗口内，
    /// 尾读要么复现整读、要么自己退回整读——绝不给出不同的结果。
    #[test]
    fn tail_first_page_matches_full_read_across_rewind_forks() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let cases: [&[(usize, usize)]; 4] = [
            &[(890, 3)],            // 远处分叉：收回第 4~889 轮
            &[(700, 650)],          // 中段分叉
            &[(895, 893)],          // 窗口内分叉
            &[(400, 10), (898, 420)], // 两次分叉
        ];
        for forks in cases {
            let path = write_tmp("tail_claude_fork", &claude_session(900, &[(0, "default"), (500, "plan")], forks, &[]));
            for page in PAGES {
                for block in BLOCKS {
                    tail_matches_full(&CLAUDE_TRANSCRIPT, &path, page, block);
                }
            }
            std::fs::remove_file(path).ok();
        }
    }

    /// 加载更早：从首屏一路上翻到文件头，每一屏都与整读版 read_chat_window 一致，拼起来就是整读全部消息。
    #[test]
    fn tail_load_earlier_pages_concatenate_to_full_history() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let path = write_tmp("tail_claude_pages", &claude_session(300, &[(0, "default")], &[], &[100]));
        let all = read_chat_delta_paged(&NoTail(&CLAUDE_TRANSCRIPT), &path, 0, None, 0).items;
        for page in [60, 200] {
            for block in [64, 4096, TAIL_FIRST_BLOCK] {
                let first = tail_first_page(&CLAUDE_TRANSCRIPT, &path, 0, None, page, block)
                    .or_else(|| Some(read_chat_delta_paged(&NoTail(&CLAUDE_TRANSCRIPT), &path, 0, None, page)))
                    .unwrap();
                let mut items = first.items;
                let mut end = first.start;
                while end > 0 {
                    let window = read_chat_window_in(&CLAUDE_TRANSCRIPT, &path, end, page, block).unwrap();
                    let reference = read_chat_window(&NoTail(&CLAUDE_TRANSCRIPT), &path, end, page).unwrap();
                    assert_eq!(window, reference, "page {page} block {block} end {end}");
                    assert!(window.start < end);
                    items.splice(0..0, window.items);
                    end = window.start;
                }
                assert_eq!(items, all, "page {page} block {block}");
            }
        }
        std::fs::remove_file(path).ok();
    }

    /// 计数对照组：原样转发（含尾读声明），只数逐行解析被调了几次。
    struct CountParses<'a>(&'a dyn TranscriptSpec, std::sync::atomic::AtomicUsize);

    impl TranscriptSpec for CountParses<'_> {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            self.0.new_parser()
        }
        fn resolve_transcript_path(&self, a: Option<&str>, b: Option<&str>, c: &str) -> Option<PathBuf> {
            self.0.resolve_transcript_path(a, b, c)
        }
        fn resolve_title(&self, a: Option<&str>, b: Option<&str>, c: &str) -> Option<String> {
            self.0.resolve_title(a, b, c)
        }
        fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
            self.0.parse_transcript_line(line)
        }
        fn parse_transcript_line_in(&self, path: &Path, line: &str) -> Vec<TranscriptEvent> {
            self.1.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.0.parse_transcript_line_in(path, line)
        }
        fn agent_modes_from_line(&self, line: &str) -> Vec<AgentMode> {
            self.0.agent_modes_from_line(line)
        }
        fn chat_line_mark(&self, line: &str) -> Option<ChatLineMark> {
            self.0.chat_line_mark(line)
        }
        fn default_agent_modes(&self) -> Vec<AgentMode> {
            self.0.default_agent_modes()
        }
        fn supports_chat(&self) -> bool {
            self.0.supports_chat()
        }
        fn tail_read(&self) -> Option<TailRead> {
            self.0.tail_read()
        }
    }

    /// 扩窗不重复解析：尾读窗口一次次往前扩时，已经解析过的尾段直接复用，每行最多解析一次。
    /// 二期实测（release，真实 21MB 会话）：「加载更早」先解析 1MB、再 7MB、最后整段 19MB，
    /// 同一段尾部被解析三遍，比整读慢 37%（67.7→92.9ms）。
    #[test]
    fn tail_region_parses_each_line_at_most_once_while_widening() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let content = claude_session(300, &[(0, "default")], &[], &[100]);
        let path = write_tmp("tail_parse_once", &content);
        let lines = content.lines().count();
        for (page, block) in [(200, 64), (200, 4096), (60, 64), (1, 64)] {
            let counting = CountParses(&CLAUDE_TRANSCRIPT, Default::default());
            let first = tail_first_page(&counting, &path, 0, None, page, block).unwrap();
            let parsed = counting.1.swap(0, std::sync::atomic::Ordering::Relaxed);
            assert!(parsed <= lines, "首屏 page {page} block {block}：{parsed} 次解析 > {lines} 行");
            let mut end = first.start;
            while end > 0 {
                let window = read_chat_window_in(&counting, &path, end, page, block).unwrap();
                let parsed = counting.1.swap(0, std::sync::atomic::Ordering::Relaxed);
                let before = content.as_bytes()[..end as usize].iter().filter(|b| **b == b'\n').count();
                assert!(parsed <= before, "加载更早 page {page} block {block} end {end}：{parsed} 次解析 > {before} 行");
                end = window.start;
            }
        }
        std::fs::remove_file(path).ok();
    }

    /// 通用口径（撤回、双写判重、多维模式、计划）：用一个声明了尾读的最小 spec 随机造数，
    /// 尾读与整读逐字段一致。claude 本身没有撤回记录与计划，这些分支靠它覆盖。
    struct TailStub;

    impl TranscriptSpec for TailStub {
        fn new_parser(&self) -> Box<dyn TranscriptParser> {
            Box::new(ChatOnlyParser)
        }
        fn resolve_transcript_path(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<PathBuf> {
            None
        }
        fn resolve_title(&self, _: Option<&str>, _: Option<&str>, _: &str) -> Option<String> {
            None
        }
        fn supports_chat(&self) -> bool {
            true
        }
        fn tail_read(&self) -> Option<TailRead> {
            Some(TailRead {
                mode_markers: &["mode:"],
                plan_markers: &["plan:"],
            })
        }
        fn default_agent_modes(&self) -> Vec<AgentMode> {
            vec![AgentMode::new("base", "init")]
        }
        fn agent_modes_from_line(&self, line: &str) -> Vec<AgentMode> {
            line.strip_prefix("mode:")
                .and_then(|rest| rest.split_once('='))
                .map(|(dim, value)| vec![AgentMode::new(dim, value)])
                .unwrap_or_default()
        }
        fn chat_line_mark(&self, line: &str) -> Option<ChatLineMark> {
            if line.starts_with("t:") {
                return Some(ChatLineMark::TurnStart);
            }
            line.strip_prefix("x:").map(|n| ChatLineMark::Undo(n.parse().unwrap_or(1)))
        }
        fn parse_transcript_line(&self, line: &str) -> Vec<TranscriptEvent> {
            let user = |rest: &str| {
                let (text, ts) = rest.split_once('@').unwrap_or((rest, ""));
                vec![TranscriptEvent::UserMessage {
                    id: format!("u-{text}"),
                    timestamp: (!ts.is_empty()).then(|| ts.to_string()),
                    text: text.to_string(),
                    interjected: false,
                }]
            };
            if let Some(rest) = line.strip_prefix("t:").or_else(|| line.strip_prefix("u:")) {
                return user(rest);
            }
            if let Some(text) = line.strip_prefix("a:") {
                return vec![TranscriptEvent::AssistantMessage { id: format!("a-{text}"), timestamp: None, text: text.to_string() }];
            }
            if let Some(plan) = line.strip_prefix("plan:") {
                return vec![TranscriptEvent::ToolCall {
                    id: format!("plan-{plan}"),
                    timestamp: None,
                    name: "ExitPlanMode".into(),
                    summary: String::new(),
                    subagent: None,
                    detail: None,
                    plan: Some(plan.to_string()),
                }];
            }
            Vec::new()
        }
    }

    #[test]
    fn tail_first_page_matches_full_read_for_undo_dupes_modes_and_plans() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        let mut taken = 0;
        for seed in 0..60 {
            let mut content = String::new();
            let lines = 150 + next(250);
            for i in 0..lines {
                let line = match next(100) {
                    0..=17 => format!("t:q{}@2026-11-10T00:00:0{}.000Z", next(4), next(3)),
                    18..=27 => format!("u:q{}@2026-11-10T00:00:0{}.000Z", next(4), next(3)),
                    28..=60 => format!("a:r{i}"),
                    61..=64 => format!("x:{}", 1 + next(12)),
                    65..=70 => format!("mode:{}={}", ["base", "perm", "model"][next(3) as usize], next(5)),
                    71..=73 => format!("plan:v{i}"),
                    74..=76 => String::new(),
                    _ => format!("pad:{}", "z".repeat(next(120) as usize)),
                };
                content.push_str(&line);
                content.push('\n');
            }
            let path = write_tmp(&format!("tail_stub_{seed}"), &content);
            for page in [1, 3, 10, 40] {
                for block in [8, 33, 200, 1000] {
                    taken += usize::from(tail_matches_full(&TailStub, &path, page, block));
                }
                // 加载更早：每一屏与整读版一致。
                let mut end = read_chat_delta_paged(&NoTail(&TailStub), &path, 0, None, page).start;
                while end > 0 {
                    let window = read_chat_window_in(&TailStub, &path, end, page, 64).unwrap();
                    assert_eq!(window, read_chat_window(&NoTail(&TailStub), &path, end, page).unwrap(), "seed {seed} page {page} end {end}");
                    end = window.start;
                }
            }
            std::fs::remove_file(path).ok();
        }
        assert!(taken > 50, "尾读应在相当一部分随机样本上接管，实际 {taken}");
    }

    /// 真实数据核对（只读）：`MEOWO_TAIL_CHECK_DIR` 指向一个 claude projects 目录，逐个大于 1MB 的
    /// transcript 比对首屏与前 3 次「加载更早」的尾读/整读结果。手动跑：
    /// `MEOWO_TAIL_CHECK_DIR=~/.claude/projects cargo test -p meowo-agent --release --lib real_transcripts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn tail_matches_full_on_real_transcripts() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let Ok(root) = std::env::var("MEOWO_TAIL_CHECK_DIR") else {
            return;
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .flat_map(|e| std::fs::read_dir(e.path()).into_iter().flatten().flatten())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .filter(|p| p.metadata().is_ok_and(|m| m.len() > TAIL_FIRST_BLOCK))
            .collect();
        files.sort();
        let (mut checked, mut taken) = (0, 0);
        let (mut full_ms, mut tail_ms) = (0f64, 0f64);
        for path in &files {
            let at = std::time::Instant::now();
            let full = read_chat_delta_paged(&NoTail(&CLAUDE_TRANSCRIPT), path, 0, None, 200);
            let one_full = at.elapsed().as_secs_f64() * 1e3;
            let at = std::time::Instant::now();
            let tail = tail_first_page_window(&CLAUDE_TRANSCRIPT, path, 0, None, 200, TAIL_FIRST_BLOCK);
            let one_tail = at.elapsed().as_secs_f64() * 1e3;
            (full_ms, tail_ms) = (full_ms + one_full, tail_ms + one_tail);
            if one_full > 100.0 {
                let len = path.metadata().map_or(0, |m| m.len()) >> 20;
                println!("[real] {len} MB: full {one_full:.0} ms vs tail {one_tail:.0} ms");
            }
            checked += 1;
            // 会话可能正在被写：以整读为准时文件若变了就跳过这份。
            if let Some((tail, window_start)) = tail {
                if tail.offset != full.offset {
                    continue;
                }
                taken += usize::from(window_start > 0);
                assert_eq!(tail.items, full.items, "{}", path.display());
                assert_eq!(tail.start, full.start, "{}", path.display());
                assert_eq!(tail.agent_modes, full.agent_modes, "{}", path.display());
                assert_eq!(tail.plan, full.plan, "{}", path.display());
            }
            let mut end = full.start;
            for _ in 0..3 {
                if end == 0 {
                    break;
                }
                let window = read_chat_window(&CLAUDE_TRANSCRIPT, path, end, 200).unwrap();
                assert_eq!(window, read_chat_window(&NoTail(&CLAUDE_TRANSCRIPT), path, end, 200).unwrap(), "{}", path.display());
                end = window.start;
            }
        }
        println!("[real] files {checked}, tail taken {taken}, full {full_ms:.0} ms vs tail {tail_ms:.0} ms");
    }

    /// 尾读耗时基准（只读）：`MEOWO_TAIL_BENCH_DIR` 指向一个放 transcript 副本的目录，逐份比对
    /// 首屏与前 3 次「加载更早」的尾读/整读耗时（各 5 次取最好）。手动跑：
    /// `MEOWO_TAIL_BENCH_DIR=<dir> cargo test -p meowo-agent --release --lib bench_tail_dir -- --ignored --nocapture --test-threads=1`
    #[test]
    #[ignore]
    fn bench_tail_dir() {
        use crate::plugins::claude::transcript::CLAUDE_TRANSCRIPT;
        let Ok(root) = std::env::var("MEOWO_TAIL_BENCH_DIR") else {
            return;
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        let best = |f: &mut dyn FnMut()| {
            f();
            (0..5)
                .map(|_| {
                    let at = std::time::Instant::now();
                    f();
                    at.elapsed().as_secs_f64() * 1e3
                })
                .fold(f64::MAX, f64::min)
        };
        let no_tail = NoTail(&CLAUDE_TRANSCRIPT);
        for path in &files {
            let len = path.metadata().map_or(0, |m| m.len()) as f64 / 1048576.0;
            let first = read_chat_delta_paged(&CLAUDE_TRANSCRIPT, path, 0, None, 200);
            let full_first = best(&mut || {
                read_chat_delta_paged(&no_tail, path, 0, None, 200);
            });
            let tail_first = best(&mut || {
                read_chat_delta_paged(&CLAUDE_TRANSCRIPT, path, 0, None, 200);
            });
            println!(
                "[bench] {} {len:.1}MB first: full {full_first:.1} ms vs tail {tail_first:.1} ms",
                path.file_name().unwrap().to_string_lossy()
            );
            let mut end = first.start;
            for page in 1..=3 {
                if end == 0 {
                    break;
                }
                let full = best(&mut || {
                    read_chat_window(&no_tail, path, end, 200);
                });
                let tail = best(&mut || {
                    read_chat_window(&CLAUDE_TRANSCRIPT, path, end, 200);
                });
                let window = read_chat_window(&CLAUDE_TRANSCRIPT, path, end, 200).unwrap();
                println!(
                    "[bench]   earlier #{page} end={:.1}MB start={:.1}MB: full {full:.1} ms vs tail {tail:.1} ms",
                    end as f64 / 1048576.0,
                    window.start as f64 / 1048576.0
                );
                end = window.start;
            }
        }
    }
}
