//! Rust 后端与 TypeScript 前端之间的 Tauri IPC DTO。

use serde::{Deserialize, Serialize};

/// 一条待办。`status` 用字面量而非枚举：来源是各家 agent 的自由文本状态，
/// 归一化后仍可能出现本版本不认识的值，前端按未知处理即可，不该让整份反序列化失败。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct TodoDto {
    pub content: String,
    /// `pending` / `in_progress` / `completed`。
    pub status: String,
    /// 「上一任务」残留标记（用户开新回合时置位；transcript 重建路径没有此概念，恒 false）。
    /// true 的行不计入当前进度，前端渲染为弱化的「上一任务」区。
    pub stale: bool,
}

/// 一次子任务委派的展示信息。真正的子任务时间线不在这里——它住在 provider 的侧车流里，
/// 由 `get_subagent_transcript` 按 `ToolUse.id` 在用户展开时才读取。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentRef {
    /// 委派时写的一句话任务描述（claude/kimi 的 `description` 参数）。
    pub description: String,
    /// 子 agent 类型（`subagent_type`，如 general-purpose / explore）。
    pub agent_type: Option<String>,
    /// 这次调用派出几个子任务。kimi 的 `AgentSwarm` 一次可以派出十几个，
    /// 展开前就把规模显示出来；普通单发委派为 1。
    pub count: u32,
}

/// 编辑类工具回执里的一段统一 diff（claude 的 `toolUseResult.structuredPatch` 原样搬运）。
/// `lines` 每行首字符是 ` `/`-`/`+`；起始行号让对话页像终端一样标真实行号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<String>,
}

/// 一次委派的结局统计。挂在**主链的工具结果**上，于是折叠状态下就能显示进度——
/// 不必先展开（展开要读侧车流，那是按需 I/O）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentOutcome {
    pub running: u32,
    pub completed: u32,
    pub failed: u32,
    /// 后台委派的任务 id（claude 启动回执里的 `agentId`）。后台子任务的真结局可能不走
    /// task-notification，而是主 agent 用 `TaskOutput` 拉取——那条回执挂在 TaskOutput 自己的
    /// 调用上，只有这个 id 能把它归回原委派。前端按「同 id 首见者为委派本体」路由。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// 一个子任务的完整时间线。一次委派可能对应多条（kimi 的 `AgentSwarm`），
/// 故 `get_subagent_transcript` 返回的是列表而不是单份 items。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentRun {
    /// 分支标签（kimi 的 `agent-3`）。单发委派没有可显示的分支名时为 None。
    pub label: Option<String>,
    /// 归一化状态：`running` / `completed` / `failed`。None = 该 provider 没有留下状态
    /// 信号（claude 的 meta.json 只记身份不记结果）。
    pub status: Option<String>,
    pub items: Vec<ChatItem>,
}

/// 一次委派里**单条分支**的侧车实测状态（kimi `AgentSwarm` 的 `agent-N`）。
/// 面板把多发委派展开成逐分支一行后，每行的状态从各自的侧车流读，不再共用聚合结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentBranchProbeDto {
    /// 分支标签（kimi 的 `agent-3`）。单发委派没有可显示的分支名时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 归一化状态：`running` / `completed` / `failed`。该分支没留下状态信号时保持
    /// `running`——不能反过来断言它已结束（与聚合口径同一条规则）。
    pub status: String,
    /// 该分支侧车最后一次落盘的时刻（ISO-8601）；仅已结束的分支携带。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// 一次**未结**委派的侧车实测状态。折叠状态下的结局统计只来自主链回执，而并行委派的
/// 回执必须等同一步里的工具**全部**跑完才一起写盘——整批跑完之前，先收工的子任务在
/// 主链上毫无痕迹（实拍：4 个 explore 里 1 个已完成，进度面板仍是 0/4，四行耗时一律
/// 按「委派时刻→现在」算）。而侧车流自己带着终结标记，这里据此实测补齐。
///
/// 按需读取（用户展开进度面板时），不进历史轮询热路径——同 `SubagentRun`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct SubagentProbeDto {
    /// 归一化状态：`running` / `completed` / `failed`。
    pub status: String,
    /// 侧车最后一次落盘的时刻（ISO-8601）。面板据此把耗时从「委派→现在」纠正成
    /// 真实执行时长；还在跑时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// 逐分支实测状态，与侧车流的定位顺序一致（与面板展开的分支行按序号对应）。
    /// 旧前端没有此字段时按 `#[serde(default)]` 拿到空列表，退回聚合 `status` 的口径。
    #[serde(default)]
    pub branches: Vec<SubagentBranchProbeDto>,
}

/// 工具调用的结构化详情：对话页「像终端那样」展开看——写了什么、改了什么、跑了什么
/// （用户实拍：终端里 Write 一行「Wrote 225 lines to …」+ 正文预览 + 「+262 lines」，
/// 对话页只有一个路径摘要）。只有插件认得的工具才填，认不得的留 None，前端退回
/// 摘要 + 回执。正文有上限（见各插件），超限置 truncated，前端据此标「已截断」而不是
/// 默默少一截。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolDetail {
    /// 整文件写入。`lines` 是**完整**行数（content 可能已截断，行数不受影响）。
    Write {
        path: String,
        content: String,
        lines: u32,
        truncated: bool,
    },
    /// 局部替换：old → new。
    Edit {
        path: String,
        old: String,
        new: String,
        replace_all: bool,
        truncated: bool,
    },
    /// 终端命令。
    Command {
        command: String,
        description: Option<String>,
    },
    /// 读文件，可带行范围。
    Read {
        path: String,
        offset: Option<u32>,
        limit: Option<u32>,
    },
    /// 搜索（Grep / Glob）。
    Search {
        pattern: String,
        path: Option<String>,
        glob: Option<String>,
    },
}

/// Provider 日志经插件解析后交给聊天归一化层的稳定消息单元。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatItem {
    UserText {
        id: String,
        timestamp: Option<String>,
        text: String,
        /// 回合进行中被 CLI 插入的话（claude queued_command）。前端据此标「回合中插入」、
        /// 结算插话回执。false 不下发，旧前端/旧缓存缺字段按 false。
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[cfg_attr(test, ts(type = "boolean", optional))]
        interjected: bool,
    },
    AssistantText {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    AssistantDelta {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    /// 回合级错误（API Error 等，CC 以 model=`<synthetic>` 写盘的系统插入文案）。
    /// 独立变体而非复用 AssistantText：前端要渲染成错误气泡，不能与模型正文同皮——
    /// 「API Error: Overloaded」被当成模型的话平铺在对话里，用户读不出这是故障。
    TurnError {
        id: String,
        timestamp: Option<String>,
        /// 短中文标签（与看板卡片的 error_label 同源，见 claude 插件 classify_error）。
        label: String,
        text: String,
    },
    Reasoning {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ReasoningDelta {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
    ToolUse {
        id: String,
        timestamp: Option<String>,
        name: String,
        summary: String,
        /// Some = 这条是子任务委派（claude/kimi 的 `Agent` 工具）。委派出去的工作记在主
        /// transcript 之外的侧车流里，前端据此渲染成可展开条目，展开时才按需拉取。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent: Option<SubagentRef>,
        /// 结构化详情（见 [`ToolDetail`]）；插件认不得的工具为 None。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ToolDetail>,
        /// ExitPlanMode 的完整计划文档（Kimi 在 display.plan 中携带）；普通工具为 None。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<String>,
    },
    ToolResult {
        id: String,
        timestamp: Option<String>,
        tool_use_id: Option<String>,
        text: String,
        is_error: bool,
        /// 这条结果是某次子任务委派的回执时，带上各分支的结局统计。前端按
        /// `tool_use_id` 配到对应的委派上，于是折叠状态下也能显示状态。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subagent: Option<SubagentOutcome>,
        /// 编辑类工具的统一 diff（带真实行号与上下文）；插件拿不到时为 None，
        /// 前端退回按 ToolDetail 的旧段/新段两块展示。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patch: Option<Vec<DiffHunk>>,
    },
    Meta {
        id: String,
        timestamp: Option<String>,
        kind: String,
        /// 分隔条的附加事实（目前只有压缩：触发方式、压缩前 token、摘要正文）。
        /// provider 拿不到时为 None，前端照旧只显示一句「上下文已压缩」。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compaction: Option<CompactionDetail>,
    },
    /// 送达回执：CLI 正忙，把这条用户消息收进了自己的队列（对照 Kimi 的 prompt:accepted）。
    /// 不进对话时间线，前端只拿它推进发出消息的状态（发送中 → 已排队 → 已读到）。
    PromptQueued {
        id: String,
        timestamp: Option<String>,
        text: String,
    },
}

/// 压缩分隔条的细节（对照 Kimi 的「已自动压缩上下文（before → after tokens）· 查看摘要」）。
/// 压缩后用户最想知道的是「它还记得什么」：摘要正文就是答案，此前被整条丢弃。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
pub struct CompactionDetail {
    /// "auto"（上下文将满时 CLI 自己压的）/ "manual"（用户 /compact）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// 压缩前的上下文 token 数。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "number | null", optional))]
    pub pre_tokens: Option<u64>,
    /// 压缩摘要正文（喂回模型的那份上下文）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum PendingReviewKind {
    Approval,
    Question,
    Plan,
    /// Lead 关卡等 leader 拍板。
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "snake_case")]
pub enum LoginOutcome {
    Success,
    Cancelled,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LoginDoneEvent {
    pub operation_id: String,
    pub provider: String,
    pub outcome: LoginOutcome,
}

impl PendingReviewKind {
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "approval" => Some(Self::Approval),
            "question" => Some(Self::Question),
            "plan" => Some(Self::Plan),
            "gate" => Some(Self::Gate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct AgentModeDto {
    pub dimension: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    /// CLI 侧会话 id：前端以它为键的动作（重命名 rename_session、置顶 STAR_KEY）
    /// 从对话窗标题菜单发起时需要，数字主键替代不了。
    pub cc_session_id: String,
    pub title: String,
    pub status: String,
    pub provider: String,
    pub cwd: Option<String>,
    pub supported: bool,
    pub items: Vec<ChatItem>,
    /// 最近的完整计划。items 只是首屏尾部窗口，计划可能已经被裁出窗口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub plan: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub reset: bool,
    /// 这次 reset 源于 CLI 撤回了最近几轮（transcript 里出现撤回记录）。前端平时不采信
    /// 「reset 但 items 为空」（文件重写途中的瞬态），撤掉唯一一轮时整读结果确实为空，
    /// 靠它放行。false 不下发。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(type = "boolean", optional))]
    pub undone: bool,
    pub pending_review: Option<PendingReviewKind>,
    pub model: Option<String>,
    /// 当前思考程度（statusline 的 `effort.level`，如 "high"）。模型不支持思考程度时
    /// CLI 不报，这里为 None，前端据此不显示档位控件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort: Option<String>,
    pub agent_modes: Vec<AgentModeDto>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub context_pct: Option<i64>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub context_window: Option<i64>,
    pub current_activity: Option<String>,
    /// 会话进程是否仍被认为存活（与看板 `session_connected` 同口径：pid 在进程表里，
    /// 或距最近事件不足宽限期）。status 是 hook 驱动的离散快照，进程死后 reaper 收尾前
    /// DB 里可能残留 running——前端展示运行态必须以此校正，否则出现「假运行中」。
    pub connected: bool,
    /// 托管 PTY 的实时屏幕状态（"working" | "idle" | "blocked"，非托管为 None），与侧栏/看板
    /// `LiveItem.screen_state` 同源。对话页的运行态必须同样经它校正：gemini 的 SessionStart
    /// 就把 DB 置成 running、首个 BeforeAgent 之前没有任何事件纠正它，只看 DB 的对话页会把
    /// 空闲会话当成「运行中」，发消息按了排队键 Tab 而非回车——正文停在输入框里发不出去
    /// （0.6.11 实机，gemini 0.64）。
    pub screen_state: Option<String>,
    /// 最近一轮以错误收场（transcript 分析口径，与侧栏/贴纸的 `LiveItem.errored` 同源）。
    /// 不做 transcript 分析的 agent（codex/kimi）恒为 false。
    pub errored: bool,
    /// 本 GUI 进程正托管着该会话的 PTY。决定「结束会话」入口的可见性：只有自己托管的
    /// 进程才能从 GUI 结束；外部终端里跑的会话（connected 但非托管）不该亮这个入口。
    pub pty_managed: bool,
    /// 「结束会话」入口的口径：托管 PTY，或会话进程仍活着（ConPTY kill 静默无效、broker
    /// 已收掉 PTY 记录的孤儿会话，stop_managed_terminal 会按 pid 杀树兜底）。后台会话
    /// （agent 守护进程托管）恒 false——杀了也会被 supervisor 拉回。pty_managed 的语义
    /// 不变，这里只是把它和「进程存活」并成入口门控。
    pub endable: bool,
    /// 由 agent 自己的后台守护进程托管（claude FleetView 的后台会话），与看板
    /// `LiveItem.background` 同源。这类会话既不在用户的终端里，也接管不了（杀进程会被
    /// supervisor 拉回来），输入框的引导文案必须换一套——让用户回终端的 FleetView。
    pub background: bool,
    /// 已归档（与看板 `LiveItem.archived` 同一列）。对话窗标题栏据此在「归档 / 取消归档」
    /// 之间切换：归档只改看板可见性，不动会话本身——归档态的会话照样能继续对话。
    pub archived: bool,
    /// Agent 自己维护的待办清单（快照式待办工具经 hook 落库）。空 = 该会话没有清单，
    /// 或该 agent 的待办是增量事件而非快照（当前版本的 Claude Code 即如此）。
    pub todos: Vec<TodoDto>,
    pub has_more: bool,
    /// 当前已加载窗口首行在 transcript 里的字节偏移：「加载更早」向上翻页的上界
    /// （作为 get_chat_history 的 before 参数带回）。只在整读（首读/reset）与翻页
    /// 响应里有意义，增量轮询响应恒为 0，前端不采信。
    #[cfg_attr(test, ts(type = "number"))]
    pub earliest: u64,
    /// hook 驱动的最近往来（UserPromptSubmit / Stop 落库），与 transcript 解析无关。
    /// items 为空（transcript 未落盘/未定位）或该 agent 不提供结构化 transcript 时，
    /// 前端用它们渲染临时时间线——「会话已在工作」不该显示成一片空白。
    pub last_user_text: Option<String>,
    pub last_ai_text: Option<String>,
    /// 跨 provider 接续链：本会话接替的上一段会话 id。Some 时对话页时间线头部
    /// 渲染「由上一段会话接续」的提示。
    #[cfg_attr(test, ts(type = "number | null"))]
    pub predecessor_id: Option<i64>,
    /// 本会话已被哪个后继接替。Some 时对话页渲染「已切换至…」横幅并禁发——
    /// 向被接替的会话续话会让接续链分叉。
    #[cfg_attr(test, ts(type = "number | null"))]
    pub superseded_by: Option<i64>,
}

/// `copy_session_to_provider` 的返回：新会话的临时负 id（前端拿它走既有的 binding
/// 轮询换真 id）+ 交接文件路径（注入 prompt 时引用；注入失败落回输入框当草稿）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct SwitchStartedDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub temp_id: i64,
    pub handoff_path: String,
}

/// `managed_terminal_pending_launch` 的返回：临时负 id 认领前的 provider/cwd。对话页靠它
/// 在认领前就拿到该家的启动识别规则——claude 的目录信任页出现在 hook 认领之前(未信任
/// 目录不跑 hook),不提前拿,信任页永远认不出、对话页一直停在「会话正在启动…」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PendingLaunchDto {
    pub provider: String,
    pub cwd: Option<String>,
}

/// 接续链上的一段会话（`get_session_lineage` 的行，按时间升序）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LineageEntryDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub id: i64,
    pub provider: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub started_at: i64,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub ended_at: Option<i64>,
    /// statusline 快照里的模型展示名；provider 不支持或首帧未到时缺失。
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PendingApprovalDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub request_id: String,
    pub provider: String,
    pub tool_name: String,
    pub description: Option<String>,
    pub input: String,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub permission_suggestions: Vec<serde_json::Value>,
    /// 题面卡可否在对话窗内直接作答：broker 仍在 approvals 表里持有该 request_id
    /// （PreToolUse 挂起中）时为 true。挂起结算/超时后轮询把它翻回 false，前端卡片
    /// 随之降级为纯展示。审批卡不使用该字段（恒 false）。
    #[serde(default)]
    pub answerable: bool,
}

/// 一次轮询同时取回审批与 AskUserQuestion 题面。此前是两条独立的 400ms 轮询——
/// 目标高度相关（都是「会话在等用户吗」），拆开只是白付一倍 IPC。push 事件仍是
/// 主路径，这条轮询是冷启动兜底（emit 不排队，对话窗晚开时只有轮询能补回题面）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PendingInteractionDto {
    pub approval: Option<PendingApprovalDto>,
    pub question: Option<PendingApprovalDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PtyOutputEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    /// 自 PTY 启动以来，本帧首字节的绝对偏移。
    #[cfg_attr(test, ts(type = "number"))]
    pub offset: u64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PtyExitEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub code: Option<u32>,
    /// true = 升级链末档的强制收尾：kill 对卡死的进程静默无效、等不到真实退出，
    /// UI 先把会话摘除，进程本身可能仍躺在系统里（zombie）。前端据此区分文案，
    /// 不把「可能还活着」说成「已结束」。serde(default) 兼容旧事件源。
    #[serde(default)]
    pub forced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ManagedTerminalSnapshotDto {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub active: bool,
    /// 画面来源是本 GUI 托管的 PTY（而非后台会话旁路）。前端拿它判「终端可写」：
    /// 旁路快照的 active 只代表旁观连接活着，不代表能输入——曾拿 active 当判据，
    /// 恢复会话被旁路活性短路，托管 PTY 根本没起。
    pub managed: bool,
    pub data: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub start_offset: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub end_offset: u64,
    pub exited: bool,
    pub exit_code: Option<u32>,
    /// 已退出会话的输出尾部可读文本：CLI 拒绝启动时（resume 冲突、认证失败…）原因只
    /// 打在 PTY 输出里。秒退探测只盖启动后第 1 秒，那之后退出的失败靠它把原因带给
    /// 对话页（否则只有一句「退出码 X」，用户得自己去终端页翻）。仅 exited 时有值。
    #[serde(default)]
    pub exit_tail: Option<String>,
    /// PTY 当前生效的行列数（0 = 未知：无活跃 PTY / 尚未设置尺寸）。前端在终端视图
    /// **隐藏**时用它把 xterm 网格钉到 PTY 真实尺寸——隐藏态宿主是屏外停靠盒，按盒子
    /// fit 出来的网格与 PTY 脱节，隐藏期到达的帧会按错误宽度换行/错行叠画（实拍花屏）。
    pub cols: u16,
    pub rows: u16,
    /// PTY 输出流里**此刻处于开启态**的 DEC 私有模式（备用屏 1049、鼠标上报 1000-1006、
    /// 括号粘贴 2004 等，见 pty.rs `ModeTracker::TRACKED`）。前端 reset 后回放时先按它
    /// 补写 `CSI ? n h` 作为基线：这些开关只在 TUI 启动时发一次，1MiB backlog 很快把
    /// 它们淘汰，重开窗口/重对齐的回放起点在其后——不补的话 xterm 退回主屏、关掉鼠标
    /// 上报，而 TUI 仍按全屏 + 鼠标模式画（实拍：两条滚动条、滚轮滚的不是 TUI 的内容）。
    /// 空 = 无活跃 PTY / 旁路快照 / 已退出定格。
    #[serde(default)]
    pub modes: Vec<u16>,
    /// 此刻有外部同步终端（attach 客户端）在线。双视图同写同一 PTY 时对话页据此提示
    /// 「输入可能交错」（T-14）；实时增删另有 `pty-external-viewers` 事件推送，这里
    /// 承担的是重开窗口/重对齐时的初始值。
    #[serde(default)]
    pub external_viewers: bool,
    /// 已退出的 CLI 是意外退出（退出码非 0、不是用户结束、不在重启窗口内，判定见 app 的
    /// exit_report::is_unexpected_exit）。对话页据此把「会话已结束」换成「Agent 意外退出
    ///（退出码 N）」并附 exit_tail 的最后几行（二期 #7）。仅 exited 时可能为 true。
    #[serde(default)]
    pub exit_unexpected: bool,
}

/// 某会话的托管 PTY 被**就地换了一个新进程**（`pty-restarted`）。
///
/// 只在**不是由持有画面的那扇窗发起**的重启时发：切换账号后把运行中的会话重启到新账号，
/// 命令来自设置窗，而画面在对话窗/终端页——它们无从知道进程换了代。
/// 对话窗自己发起的重启（sendText / 切模式 / 接管 / 假死横幅的「结束并恢复」）在原地
/// 调 rearm，不走这条事件，免得同一次重启复位两遍、白闪一次。
///
/// 前端收到即把输出偏移归零并重拉快照：新 PTY 从 0 重新计数，沿用旧偏移会让所有新输出
/// 被判成「已写过」而丢弃——画面定格在旧进程的最后一帧，打字也没有回显（实拍症状）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PtyRestartedEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
}

/// 外部同步终端（attach 客户端）在线状态变化事件（`pty-external-viewers`）。
/// 对话窗订阅它即时刷新「输入可能交错」提示——快照轮询在桌面端不是常开通道，
/// 订阅表增删的那一刻必须主动推（pty.rs handle_attach 的两处订阅表写点）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ExternalViewersEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    pub online: bool,
}

/// screen_state「运行中 → 空闲/阻塞」降级转变的直达事件（`board-urgent`，T-15 高优通道）。
/// 「agent 停下来等人」是看板角标最该即时反映的转变，而后端合流（300ms）+ 前端节流
/// （400ms）叠在检测节拍与防抖之后，最坏拖到 ~1.7s。后端检测到降级时绕过 board-changed
/// 合流直发本事件，前端看板随之绕过刷新节流立即重拉。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct BoardUrgentEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    /// 转变后的发布状态（`idle` / `blocked`）。
    pub state: String,
}

/// 灵动岛提醒（`island-alert`，只发给主窗口）。与系统通知同源：watch 的通知判定（指纹去重、
/// 启动首轮播种、「正看着就不打扰」）走完、决定要提醒时同时发它，但不受「系统通知」开关
/// 影响——关了系统通知的人照样要岛上弹一下。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct IslandAlertEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
    /// `waiting`（跑完等你）/ `approval` / `question` / `plan` / `blocked`（屏幕上挂着交互）/ `error`。
    pub kind: String,
    /// 会话展示标题。
    pub title: String,
    pub project: String,
    /// 补充说明：出错时是错误标签，其余为空。
    pub detail: Option<String>,
}

/// 对话页 transcript 变更推送（`chat-transcript`，C-14）。后端 watch 到该会话的
/// transcript 文件写入时直发（不走 board 合流），对话窗据此提前拉增量——外部终端
/// 会话没有 pty-output，这条是它的实时通道；定时轮询降级为最终兜底。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ChatTranscriptEvent {
    #[cfg_attr(test, ts(type = "number"))]
    pub session_id: i64,
}

/// 工作区里有改动的一个文件（git status --porcelain 的一行归一化结果）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct GitChangedFileDto {
    /// 相对仓库根的路径（正斜杠形式，与 porcelain 输出一致）。
    pub path: String,
    /// 归一化状态字母：M/A/D/T 取自 porcelain 的 X 或 Y；未跟踪（??）记为 "U"。
    pub status: String,
    /// 相对 HEAD 的增删行数（`git diff --numstat`；未跟踪文件按行数全记为新增）。
    /// 二进制、仓库还没有提交、未跟踪文件过大或过多时数不出来，为 None，前端不显示。
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
    /// 暂存区相对 HEAD 的状态字母（porcelain 的 X：M/A/D/T）；没暂存为 None。
    /// 「更改」面板按 VS Code 源代码管理分「暂存的更改」「更改」两组（用户要求 2026-11-09），
    /// `status` 合一的字母分不出来。「已提交未合并」清单里恒为 None。
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub index_status: Option<String>,
    /// 工作区相对暂存区的状态字母（porcelain 的 Y：M/D/T；未跟踪记 "U"）；没有工作区改动为 None。
    /// 部分暂存（MM）的文件两个字段都有值，两组里都出现。
    /// 两个字段在 TS 里是可选的：手机页可能先于安装版更新，旧后端不下发，前端按「全是未暂存」处理。
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub worktree_status: Option<String>,
    /// 折叠的未跟踪目录：`path` 以 `/` 结尾，这里是其中未跟踪文件的个数；普通文件为 None。
    /// 没进 .gitignore 的构建目录能有上万个文件（用户实测 15612 个），逐个下发会让面板卡死，
    /// 多于 50 个的目录只给一条，点开再按需列（见 git.rs UNTRACKED_FOLD_MIN）。
    /// TS 里可选：旧后端不下发，前端按普通文件处理。
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub untracked_files: Option<u32>,
}

/// 对话页「Diff」按钮的可见性与弹层文件列表的数据源。
/// `git_available`/`is_repo` 任一为 false 时前端不显示入口，其余字段无意义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct GitDiffSummaryDto {
    /// git 可执行文件能否启动（未安装/被杀软拦下时为 false）。
    pub git_available: bool,
    /// cwd 是否在某个 git 工作树内。
    pub is_repo: bool,
    /// 当前分支名；detached HEAD 或查询失败时为 None。
    pub branch: Option<String>,
    /// 相对上游分支领先 / 落后的提交数（对照 Kimi 改动面板的「领先远程 / 落后远程」：agent 提交
    /// 之后要不要推送，一眼可见）。按本地记录的远程分支算，不自动 fetch。没有上游时为 None。
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub files: Vec<GitChangedFileDto>,
    /// 上游分支（如 `origin/feat`、远端名带斜杠时 `team/fork/feat`），拉取来源，ahead/behind 按它算。远端与分支名
    /// 读 branch.<b>.remote / merge 配置原样拼，不从 `远端/分支` 字符串里拆。没设上游、上游已被删（gone）、上游
    /// 是远端默认分支而本地不是它（推送不会推到那里）时为 None。本地计数、不 fetch。旧后端不下发。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub upstream: Option<String>,
    /// 还没推上去的提交数：推送目标就是上游时等于 ahead；否则（没上游、pushDefault 指向别的远端）是推送远端上
    /// 还没有的提交数（新建的 worktree 分支第一次推之前也要看得到「有几个没推」）。没有远端、detached、没算
    /// （非选中的仓库且推送目标不是上游）时为 None，前端不显示推送按钮。旧后端不下发。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unpushed: Option<u32>,
    /// 「推送」实际会推到哪（如 `origin/feat`），与 forge 的推送同一个判定（meowo-app git_target::resolve）：
    /// 远端按 branch.<b>.pushRemote → remote.pushDefault → 上游远端 → origin；上游是远端默认分支（典型：
    /// `git worktree add -b feat <路径> origin/main`）时改推同名分支并设上游，绝不推进默认分支；上游是别的功能
    /// 分支（fix 跟踪 origin/bugfix-123）时推到它。没有远端、detached 时为 None。旧后端不下发。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub push_target: Option<String>,
}

/// 「更改」面板里的一个仓库（用户要求 2026-11-09：会话开在父目录、或 agent 新开 worktree 去改、
/// 改完又提交时，面板都要看得到）。工作目录所在仓库排第一。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct RepoChangesDto {
    /// 仓库（或 worktree）根目录，正斜杠。单文件 diff 以它为 cwd。
    pub root: String,
    /// 根目录名，分组标题用。
    pub name: String,
    /// 是不是会话工作目录所在的仓库。
    pub is_cwd_repo: bool,
    /// 未提交的改动（与单仓库时的 git_diff_summary 同一份）。
    pub summary: GitDiffSummaryDto,
    /// 「已提交未合并」的基准提交（merge-base(HEAD, 默认分支) 的 SHA）。当前就在默认分支上、
    /// 或找不到默认分支时为 None，committed 为空。
    pub base: Option<String>,
    /// 默认分支名（如 `main`），展示用。
    pub base_label: Option<String>,
    /// 本分支相对基准已提交的改动（`git diff base HEAD`）。
    pub committed: Vec<GitChangedFileDto>,
    /// 这份是不是完整的：仓库多时只给选中的那个算增删行数和「本分支已提交」，其余只跑一次
    /// `git status`（文件清单有、行数为空、committed 为空），点到它时再拉（用户要求 2026-11-09：
    /// 先显示目录，具体内容点击后异步）。旧后端不下发，按完整处理。
    #[serde(default = "detailed_default")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub detailed: bool,
}

fn detailed_default() -> bool {
    true
}

/// 当前分支的 Pull Request（对话页标题栏，对照 Kimi 的「打开 Pull Request」与状态标记）。
/// 由 `gh pr view` 取得；没装 gh / 没登录 / 分支没有 PR 时命令返回 None，前端不显示。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDto {
    pub number: u32,
    /// `open` / `draft` / `merged` / `closed`。
    pub state: String,
    pub url: String,
    pub title: String,
    /// 以下字段由 `git_forge_status` 填（「更改」面板的 PR 与检查区块）；标题栏的 `git_pull_request`
    /// 不填。serde default：旧后端不下发时前端按「不知道」处理。
    /// 目标分支（合并确认框写明合进哪）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub head_ref: Option<String>,
    /// PR 头提交。合并时原样带回给 `gh_pr_merge --match-head-commit`：确认框打开之后又有人推了新提交，
    /// 合并就会被拒，不会把用户没看过的提交合进去。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub head_oid: Option<String>,
    /// GitHub 的 `mergeable`：`MERGEABLE` / `CONFLICTING` / `UNKNOWN`（还在算，按进行中处理）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mergeable: Option<String>,
    /// GitHub 的 `mergeStateStatus`：`CLEAN` / `BLOCKED` / `BEHIND` / `DIRTY` / `UNSTABLE` / `DRAFT` / `UNKNOWN` 等。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub merge_state: Option<String>,
    /// 每一项检查（CheckRun 与老式 StatusContext 都映射进来）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<PrCheckDto>>", optional))]
    pub checks: Vec<PrCheckDto>,
    /// 检查汇总：`none`（没有任何检查，允许合并）/ `pass` / `fail` / `pending`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub checks_summary: Option<String>,
    /// 已加入合并队列（本机发起合并时 GitHub 回的是入队而不是合并）。这时不能提示「已合并」。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(type = "boolean", optional))]
    pub queued: bool,
}

/// PR 的一项检查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PrCheckDto {
    pub name: String,
    /// CheckRun 所属的 workflow 名（StatusContext 没有）。
    pub workflow: Option<String>,
    /// `pass` / `fail` / `pending` / `skipped`。
    pub state: String,
    pub url: Option<String>,
}

/// 「更改」面板的 GitHub 区块（`git_forge_status`）：gh 能不能用、开 PR 的默认值、当前分支的 PR 与检查。
/// gh 不可用时只有 `compare_url`（前端开 PR 改为打开 GitHub 网页），不显示 CI、不能在应用里合并。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ForgeStatusDto {
    /// `ok` / `missing`（没装）/ `unauthenticated`（没登录）/ `not_github`（远端不是 GitHub）/ `no_remote`。
    pub gh: String,
    /// gh 太旧（`gh pr merge` 没有 `--match-head-commit`）：合并按钮不可用并说明。
    pub gh_too_old: bool,
    /// GitHub 仓库首页（`https://github.com/<owner>/<repo>`）。远端不是 GitHub 时为 None。
    pub repo_url: Option<String>,
    /// 当前分支对默认分支的 compare 网页（没装 gh 时「开 PR」打开它）。
    pub compare_url: Option<String>,
    /// 仓库允许的合并方式（`merge` / `squash` / `rebase`），gh repo view 读，缓存 1 小时。
    pub merge_methods: Vec<String>,
    /// 这个仓库上次用的合并方式（按 owner/repo 记在本机）。
    pub last_merge_method: Option<String>,
    pub pr: Option<PullRequestDto>,
    /// 还没有 PR 时，开 PR 表单的默认值。
    pub pr_defaults: Option<PrDefaultsDto>,
    /// 多个 GitHub 远端、又没用 `gh repo set-default` 定过时，PR 可以开在哪些远端的仓库（界面上让用户选，
    /// 选择按仓库记在 `~/.meowo/forge.json`）。只有一个 GitHub 远端或 gh 已定时为空。
    pub repo_choices: Vec<ForgeRepoChoiceDto>,
    /// PR 现在按哪个远端的仓库算（gh-resolved、用户选的、唯一的；没定时是暂按的 origin）。
    pub repo_remote: Option<String>,
    /// 多个 GitHub 远端还没定：查状态暂按 origin，开 PR / 合并返回 `gh/ambiguous_repo`，要先选。
    pub repo_needs_choice: bool,
}

/// PR 可以开在的一个仓库：远端名 + GitHub 的 owner/repo。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ForgeRepoChoiceDto {
    pub remote: String,
    pub slug: String,
}

/// 开 PR 的默认标题、正文与目标分支（本地 git 算，不联网）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PrDefaultsDto {
    /// 只有一个提交时取它的标题，否则用分支名。
    pub title: String,
    /// base..HEAD 的提交列表（每行 `- 标题`）。
    pub body: String,
    pub base: String,
}

/// 一次推送的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct PushResultDto {
    pub remote: String,
    pub branch: String,
    /// 这次顺手设了上游（原来没有上游，走的 `push -u`）。
    pub set_upstream: bool,
    /// 推上去的提交数（推之前算的「没推的提交」）。
    pub pushed: u32,
}

/// 后台 git 任务（手机端推送：手机 RPC 等不了 20–90 秒，先拿任务号再轮询）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct GitJobDto {
    /// `running` / `ok` / `failed`。
    pub state: String,
    pub push: Option<PushResultDto>,
    /// 失败原因（`git/push_auth` 之类，前端走 formatBackendError）。
    pub error: Option<String>,
}

/// 应用内合并的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct MergeResultDto {
    pub merged: bool,
    /// 进了合并队列（还没真正合并）。
    pub queued: bool,
}

/// 会话所在 worktree 能不能清理（归档后的「删除 worktree」入口用）。判定全在桌面端做，
/// 手机端经远程桥拿到的是同一份结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatusDto {
    /// worktree 根目录（会话 cwd 可能是它的子目录）；不是 worktree 时为会话 cwd。
    pub path: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<u32>,
    pub eligible: bool,
    /// 不能清理的原因码（`not_worktree` / `dir_missing` / `gh_unavailable` / `no_pr` / `not_merged_local` /
    /// `pr_open` / `pr_closed` / `dirty` / `untracked` / `unpushed` / `submodule_dirty` /
    /// `submodule_unpushed` / `shared_session` / `detached` / `locked` / `not_repo`）。可清理时为 None。
    pub reason: Option<String>,
    /// 原因附带的数量（如没提交的文件数、共用目录的会话数）。
    pub count: Option<u32>,
    /// 可清理时：被 git 忽略、又不是构建产物的文件（`.env`、`.lead/` 等），删除前列给用户看。
    pub ignored: Vec<String>,
}

/// 占着 worktree 目录的进程（删除前列给用户确认）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreeProcessDto {
    pub pid: u32,
    pub name: String,
}

/// 一次删除的结果。`removed = false` 且 `blockers` 非空：有进程占用，什么都没动，
/// 等用户确认后带 `kill_processes = true` 再调。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRemovalDto {
    pub removed: bool,
    pub blockers: Vec<WorktreeProcessDto>,
    /// 本地分支是否一并删掉（worktree 删了但分支删失败时为 false，前端提示一句）。
    pub branch_deleted: bool,
}

/// 批量清理清单的一项：一个 worktree 目录，可能对应多条已归档会话。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct CleanableWorktreeDto {
    #[cfg_attr(test, ts(type = "number[]"))]
    pub session_ids: Vec<i64>,
    pub status: WorktreeStatusDto,
}

/// 单个文件的统一 diff 文本。未跟踪文件的 `diff` 是合成的伪 diff
/// （`--- /dev/null` / `+++ b/<path>` 头 + 每行加 `+` 前缀）；二进制文件只有
/// 一行占位 "(binary file)"。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiffDto {
    pub path: String,
    pub status: String,
    pub diff: String,
    /// diff 文本超过上限被截断时为 true（前端据此显示截断提示）。
    pub truncated: bool,
}

/// 「文件」页签目录树的一个条目（list_dir_entries 的返回元素）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    /// 条目名（不含路径）。
    pub name: String,
    /// 相对会话 cwd 的路径（正斜杠形式），展开/读取时回传给后端。
    pub rel_path: String,
    pub is_dir: bool,
}

/// Lead 工作流：会话所在项目当前任务的只读快照（lead_task_snapshot 的返回）。
///
/// 为什么不让前端直接 read_file_text：手机端远程桥刻意不放行任意文件读，这里由后端按约定
/// 只读 `.lead/active` 指向的任务目录（2026-11-09 用户在关卡 B 决定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LeadTaskDto {
    /// 任务目录名（`.lead/<slug>`）。
    pub slug: String,
    /// 阶段：interview / spec / design / implement / verify / done / blocked，解析不到为 unknown。
    pub phase: String,
    /// SPEC.md 原文（封顶，超出截断）。
    pub spec: String,
    pub checks: Vec<LeadCheckDto>,
    /// decisions.md 原文末尾。
    pub decisions: String,
    /// progress.md 原文末尾。
    pub progress: String,
    /// 正在挂起或已作答未消费的关卡；没有关卡或 gate.json 损坏为 None。
    pub gate: Option<LeadGateDto>,
}

/// 验收清单的一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LeadCheckDto {
    pub id: String,
    pub check: String,
    pub passes: bool,
    /// 豁免原因（leader 决定不做/不验）；有值时这一条不算未通过，面板单独显示。
    #[serde(default)]
    pub waived: Option<String>,
    /// 要 leader 亲自验收：只看条目上的 `by: "leader"` 字段，不看 check 文字；关卡 C 接受时自动勾上。
    #[serde(default)]
    pub leader: bool,
}

/// 关卡（`.lead/<task>/gate.json`）。状态 pending → answered（Meowo 写）→ consumed（agent 写）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LeadGateDto {
    pub id: String,
    /// A（确认规格）/ B（单向门拍板）/ C（看证据验收）/ D（需要授权或亲手操作）/ Q（访谈提问）。
    pub kind: String,
    pub title: String,
    /// markdown 摘要。
    pub summary: String,
    /// B、Q 由 agent 写选项；A、C、D 的选项固定（前端按 kind 渲染），这里可为空。
    pub options: Vec<LeadGateOptionDto>,
    /// pending / answered / consumed。
    pub status: String,
    /// 已作答时的答复。
    pub answer: Option<LeadGateAnswerDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LeadGateOptionDto {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub recommended: bool,
}

/// leader 的答复：选了哪个选项（A：confirm/revise；C：accept/reject；D：approve/done/deny；B、Q：选项 id），可附意见。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct LeadGateAnswerDto {
    pub choice: String,
    #[serde(default)]
    pub note: String,
}

/// 「文件」页签单文件文本内容（read_file_text 的返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct FileTextDto {
    /// 相对会话 cwd 的路径（正斜杠形式），原样回显请求参数。
    pub rel_path: String,
    /// 文件文本；binary 为 true 时为空串（前端显示二进制占位）。
    pub text: String,
    /// 文本超过上限被截断时为 true（前端据此显示截断提示）。
    pub truncated: bool,
    /// 前 8KB 嗅探到 NUL 时为 true（按二进制处理，不返回文本）。
    pub binary: bool,
}

/// 「文件」页签搜索里一个文件内的单行内容命中。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct SearchLineHitDto {
    /// 行号（1 起）。
    pub line: u32,
    /// 命中行文本：窗口对准首个命中处（左侧截掉时带 … 前缀）、超长截断。
    pub preview: String,
}

/// 「文件」页签搜索的一个条目（按文件/目录分组，search_project_files 的返回元素）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct SearchFileHitDto {
    /// 相对会话 cwd 的路径（正斜杠形式）。
    pub rel_path: String,
    /// 目录条目为 true（目录只有名称命中，lines 恒空）。
    pub is_dir: bool,
    /// 文件/目录名本身命中了关键字。
    pub name_match: bool,
    /// 展示用的内容命中行（每文件封顶若干条）。
    pub lines: Vec<SearchLineHitDto>,
    /// 该文件内容命中的**总**行数（可大于 lines.len()，前端角标显示全量）。
    pub total_line_matches: u32,
}

/// 「文件」页签搜索结果（名称命中条目在前、内容命中在后，各按路径排序）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct SearchResultDto {
    pub files: Vec<SearchFileHitDto>,
    /// 命中数/扫描量到达上限提前收兵时为 true（前端提示细化关键词）。
    pub truncated: bool,
}

/// 「文件」页签的图片预览（read_image_preview 的返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct ImagePreviewDto {
    /// `data:<mime>;base64,…` 形式的图片数据；too_large 时为 None。
    pub data_url: Option<String>,
    /// 超出预览上限（base64 过大会卡 IPC/渲染），前端显示「过大」占位。
    pub too_large: bool,
}

/// 检测到的本地打开方式（文件面板「打开」按钮/菜单用，list_file_openers 的返回元素）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(export, export_to = "../../../../src/generated/contracts/"))]
#[serde(rename_all = "camelCase")]
pub struct FileOpenerDto {
    /// 回传 open_path_with 的标识：Windows 是系统关联处理器的名字（exe 路径），
    /// 其余平台是探测表里的编辑器 id（"vscode"/"cursor"…）。
    pub id: String,
    /// 菜单展示名（"Visual Studio Code"…）。
    pub name: String,
    /// 应用图标 PNG data URL；拿不到（UWP 资源图标/非 Windows 平台）为 None，前端用通用图标兜底。
    pub icon: Option<String>,
}

impl From<crate::broker::ApprovalRequest> for PendingApprovalDto {
    fn from(request: crate::broker::ApprovalRequest) -> Self {
        Self {
            session_id: request.session_id,
            request_id: request.request_id,
            provider: request.provider,
            tool_name: request.tool_name,
            description: request.description,
            input: request.input,
            permission_suggestions: request.permission_suggestions,
            // 可作答与否取决于 broker 是否还持有请求（approvals 表），不是请求本身的属性；
            // 由 emit/轮询处按持有状态覆写。
            answerable: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_snapshot_uses_the_frontend_camel_case_contract() {
        let value = serde_json::to_value(ManagedTerminalSnapshotDto {
            session_id: 7,
            active: true,
            managed: true,
            data: "QUJD".into(),
            start_offset: 10,
            end_offset: 13,
            exited: false,
            exit_code: None,
            exit_tail: None,
            cols: 120,
            rows: 40,
            modes: vec![1049, 1006],
            external_viewers: false,
            exit_unexpected: true,
        })
        .unwrap();
        assert_eq!(value["sessionId"], 7);
        assert_eq!(value["startOffset"], 10);
        assert_eq!(value["endOffset"], 13);
        assert!(value.get("start_offset").is_none());
        assert_eq!(value["exitUnexpected"], true);
    }

    /// GUI 边界必须走 DTO 的理由，钉成测试：原始 `broker::ApprovalRequest` 在
    /// `permission_suggestions` 为空时把字段整个 skip 掉（reporter 线路的减负），而前端类型
    /// （ts-rs 从 DTO 生成）承诺该字段**恒在**——app 曾直接把 ApprovalRequest emit 给前端，
    /// codex 的审批（从不带 suggestions）一弹就让 ChatWindow 在 `.map` 上崩掉。
    #[test]
    fn dto_always_carries_permission_suggestions_even_when_empty() {
        let request = crate::broker::ApprovalRequest {
            session_id: 7,
            request_id: "req-1".into(),
            provider: "codex".into(),
            tool_name: "Bash".into(),
            description: None,
            input: "{}".into(),
            permission_suggestions: vec![],
            pre_tool_use: false,
        };
        // 原始线路结构：空列表 → 字段消失（这正是不能拿它喂前端的原因）。
        let raw = serde_json::to_value(&request).unwrap();
        assert!(raw.get("permissionSuggestions").is_none());

        // DTO：字段恒在，空时是 `[]` 而不是缺席。
        let dto_value = serde_json::to_value(PendingApprovalDto::from(request)).unwrap();
        assert_eq!(dto_value["permissionSuggestions"], serde_json::json!([]));
        assert_eq!(dto_value["sessionId"], 7);
        assert_eq!(dto_value["requestId"], "req-1");
    }

    /// interjected 只在为真时下发；缺字段（旧后端/旧缓存）按 false 读回。
    #[test]
    fn user_text_interjected_is_omitted_when_false() {
        let plain = ChatItem::UserText { id: "u".into(), timestamp: None, text: "hi".into(), interjected: false };
        assert!(serde_json::to_value(&plain).unwrap().get("interjected").is_none());
        let steered = ChatItem::UserText { id: "u".into(), timestamp: None, text: "hi".into(), interjected: true };
        assert_eq!(serde_json::to_value(&steered).unwrap()["interjected"], true);
        let old: ChatItem = serde_json::from_str(r#"{"type":"user_text","id":"u","timestamp":null,"text":"hi"}"#).unwrap();
        assert_eq!(old, plain);
    }

    #[test]
    fn chat_contract_keeps_tagged_items_and_rejects_unknown_review_kinds() {
        let item = ChatItem::ToolResult {
            id: "result-1".into(),
            timestamp: None,
            tool_use_id: Some("tool-1".into()),
            text: "ok".into(),
            is_error: false,
            subagent: None,
            patch: None,
        };
        let value = serde_json::to_value(item).unwrap();
        assert_eq!(value["type"], "tool_result");
        assert_eq!(value["tool_use_id"], "tool-1");
        // 非子任务的回执不该带这个键——旧前端与快照比对都按「缺席」理解。
        assert!(value.get("subagent").is_none());
        assert_eq!(
            PendingReviewKind::from_stored("question"),
            Some(PendingReviewKind::Question)
        );
        assert_eq!(PendingReviewKind::from_stored("future"), None);
    }
}
