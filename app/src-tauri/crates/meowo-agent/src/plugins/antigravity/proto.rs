//! agy 对话库里 protobuf blob 的最小结构体。
//!
//! 字段编号全部取自 agy.exe 内嵌的官方描述符（Go `rawDesc`，1.5.17/1.5.18 同一套），不是从样本里
//! 猜的：调研时把 389 个 `.proto` 抠成 FileDescriptorSet，按字段名解码本机 6 个对话库的全部 blob，
//! 未知字段为 0（research-transcript.md §1~§3）。
//!
//! 只声明对话页要用的字段。prost 遇到未声明的字段直接跳过，所以 agy 加字段不会让解析失败；每条
//! 用户消息附带的 3~4KB `user_config`、每回合最后一次模型调用附带的约 115KB 系统提示也因此不会被
//! 解码成对象。官方定义里 `step` 是 oneof，这里拆成各自的可选字段：oneof 只是「至多出现一个」的
//! 约定，线格式上就是普通字段，拆开写不影响解码，还省掉一层枚举。

use prost::Message;

/// `google.protobuf.Timestamp`。
#[derive(Clone, PartialEq, Message)]
pub struct Timestamp {
    #[prost(int64, tag = "1")]
    pub seconds: i64,
    #[prost(int32, tag = "2")]
    pub nanos: i32,
}

impl Timestamp {
    pub fn millis(&self) -> i64 {
        self.seconds * 1000 + i64::from(self.nanos) / 1_000_000
    }
}

/// `gemini_coder.Step`：`steps.step_payload` 列的整段内容。同表的 metadata / error_details /
/// permissions 列只是它的副本，实测会漏写（permissions 有值时那一列是 NULL），所以只读这一列。
#[derive(Clone, PartialEq, Message)]
pub struct Step {
    /// `exa.cortex_pb.CortexStepType`，见 [`step_type`]。
    #[prost(int32, tag = "1")]
    pub r#type: i32,
    /// `exa.cortex_pb.CortexStepStatus`，见 [`status`]。会原地更新。
    #[prost(int32, tag = "4")]
    pub status: i32,
    #[prost(message, optional, tag = "5")]
    pub metadata: Option<StepMetadata>,
    #[prost(message, optional, tag = "19")]
    pub user_input: Option<UserInput>,
    #[prost(message, optional, tag = "20")]
    pub planner_response: Option<PlannerResponse>,
    #[prost(message, optional, tag = "24")]
    pub error_message: Option<ErrorMessage>,
    #[prost(message, optional, tag = "30")]
    pub checkpoint: Option<Checkpoint>,
    /// 步骤级错误（工具失败、用户拒绝审批）。
    #[prost(message, optional, tag = "31")]
    pub error: Option<ErrorDetails>,
    #[prost(message, optional, tag = "114")]
    pub system_message: Option<SystemMessage>,
    /// 所有内置工具（run_command、read_file、write_file、ask_question…）的执行与结果都是这一种。
    #[prost(message, optional, tag = "140")]
    pub generic: Option<Generic>,
    #[prost(message, optional, tag = "143")]
    pub invoke_subagent: Option<InvokeSubagent>,
}

/// `exa.cortex_pb.CortexStepType` 里对话页认的几个值（完整 140 个见 research-transcript-work/enums-step.txt）。
pub mod step_type {
    pub const USER_INPUT: i32 = 14;
    pub const PLANNER_RESPONSE: i32 = 15;
    pub const ERROR_MESSAGE: i32 = 17;
    pub const CHECKPOINT: i32 = 23;
    pub const SYSTEM_MESSAGE: i32 = 101;
    pub const INVOKE_SUBAGENT: i32 = 127;
    pub const GENERIC: i32 = 132;
}

/// `exa.cortex_pb.CortexStepStatus`。
pub mod status {
    pub const UNSPECIFIED: i32 = 0;
    pub const PENDING: i32 = 1;
    pub const RUNNING: i32 = 2;
    pub const DONE: i32 = 3;
    pub const CLEARED: i32 = 5;
    pub const CANCELED: i32 = 6;
    pub const ERROR: i32 = 7;
    pub const GENERATING: i32 = 8;
    pub const WAITING: i32 = 9;
    pub const QUEUED: i32 = 11;
    pub const INTERRUPTED: i32 = 12;

    /// 不会再变的状态。回复先以 GENERATING 插入、生成完才改成 DONE；工具 RUNNING/PENDING →
    /// WAITING（等审批）→ DONE/ERROR（research-transcript.md §4 的只读轮询时序）。
    ///
    /// 按「进行中」的六个值反着判：agy 将来新增的状态默认当定稿导出。反过来（新状态当进行中）
    /// 会让导出永远停在那一行、对话页从此不再更新，代价更大。
    pub fn settled(status: i32) -> bool {
        !matches!(status, UNSPECIFIED | PENDING | RUNNING | GENERATING | WAITING | QUEUED)
    }
}

/// `exa.cortex_pb.CortexStepMetadata`。
#[derive(Clone, PartialEq, Message)]
pub struct StepMetadata {
    #[prost(message, optional, tag = "1")]
    pub created_at: Option<Timestamp>,
    /// `CortexStepSource`：2 MODEL、3 USER_IMPLICIT、4 USER_EXPLICIT、5 SYSTEM、6 SYSTEM_SDK。
    #[prost(int32, tag = "3")]
    pub source: i32,
    /// GENERIC 行的工具名与参数；`id` 等于前一条回复里 `tool_calls[].id`。
    #[prost(message, optional, tag = "4")]
    pub tool_call: Option<ChatToolCall>,
    #[prost(message, optional, tag = "8")]
    pub completed_at: Option<Timestamp>,
    #[prost(message, optional, tag = "9")]
    pub model_usage: Option<ModelUsage>,
    /// 一次用户输入触发的整轮共用同一个 id。
    #[prost(string, tag = "12")]
    pub execution_id: String,
}

/// `exa.codeium_common_pb.ChatToolCall`。
#[derive(Clone, PartialEq, Message)]
pub struct ChatToolCall {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, tag = "3")]
    pub arguments_json: String,
}

/// `exa.codeium_common_pb.ModelUsageStats`。
#[derive(Clone, PartialEq, Message)]
pub struct ModelUsage {
    #[prost(uint64, tag = "2")]
    pub input_tokens: u64,
    #[prost(uint64, tag = "3")]
    pub output_tokens: u64,
    #[prost(uint64, tag = "4")]
    pub cache_write_tokens: u64,
    #[prost(uint64, tag = "5")]
    pub cache_read_tokens: u64,
}

/// `exa.cortex_pb.CortexErrorDetails`。
#[derive(Clone, PartialEq, Message)]
pub struct ErrorDetails {
    #[prost(string, tag = "1")]
    pub user_error_message: String,
    #[prost(string, tag = "2")]
    pub short_error: String,
    #[prost(string, tag = "3")]
    pub full_error: String,
    #[prost(uint32, tag = "7")]
    pub error_code: u32,
    #[prost(string, tag = "9")]
    pub model_error_message: String,
}

/// `exa.cortex_pb.CortexStepUserInput`。
#[derive(Clone, PartialEq, Message)]
pub struct UserInput {
    /// 用户原话。斜杠命令也原样存（如 `/plan line1\nline2`）。
    #[prost(string, tag = "2")]
    pub user_response: String,
    #[prost(message, repeated, tag = "3")]
    pub items: Vec<TextOrScopeItem>,
    #[prost(message, repeated, tag = "5")]
    pub images: Vec<ImageData>,
    #[prost(bool, tag = "6")]
    pub is_queued_message: bool,
}

/// `exa.codeium_common_pb.TextOrScopeItem`（oneof chunk 里只认文本）。
#[derive(Clone, PartialEq, Message)]
pub struct TextOrScopeItem {
    #[prost(string, tag = "1")]
    pub text: String,
}

/// `exa.codeium_common_pb.ImageData`。
#[derive(Clone, PartialEq, Message)]
pub struct ImageData {
    #[prost(string, tag = "1")]
    pub base64_data: String,
    #[prost(string, tag = "2")]
    pub mime_type: String,
    #[prost(string, tag = "4")]
    pub uri: String,
}

/// `exa.cortex_pb.CortexStepPlannerResponse`。
#[derive(Clone, PartialEq, Message)]
pub struct PlannerResponse {
    #[prost(string, tag = "1")]
    pub response: String,
    /// agy 给的是思考摘要，不是原文。
    #[prost(string, tag = "3")]
    pub thinking: String,
    #[prost(message, repeated, tag = "7")]
    pub tool_calls: Vec<ChatToolCall>,
    /// 优先用它；为空时退回 `response`（样本里两者相同）。
    #[prost(string, tag = "8")]
    pub modified_response: String,
}

/// `exa.cortex_pb.CortexStepGeneric`。
#[derive(Clone, PartialEq, Message)]
pub struct Generic {
    /// 参数的字符串化副本（值都成了字符串："5000"、"false"），只在 metadata.tool_call 缺席时兜底。
    #[prost(map = "string, string", tag = "1")]
    pub args: std::collections::HashMap<String, String>,
    #[prost(message, optional, tag = "2")]
    pub result: Option<GenericResult>,
}

/// `exa.cortex_pb.GenericStepResult`。
#[derive(Clone, PartialEq, Message)]
pub struct GenericResult {
    /// 给模型看的文本，开头固定两行 `Created At:` / `Completed At:`。
    #[prost(string, tag = "1")]
    pub result: String,
    /// agy 自己界面的渲染信息（标题、终端块、退出码）。
    #[prost(message, optional, tag = "3")]
    pub step_render_info: Option<RenderInfo>,
    /// 完整输出落在 `brain/<对话 id>/.system_generated/steps/<idx>/output.txt`。
    #[prost(string, tag = "7")]
    pub full_output_uri: String,
}

/// `exa.cortex_pb.StepRenderInfo`。
#[derive(Clone, PartialEq, Message)]
pub struct RenderInfo {
    #[prost(string, tag = "1")]
    pub title: String,
    #[prost(bool, tag = "4")]
    pub hidden: bool,
    /// 「Ran」「Analyzed」「Created」之类的动词。
    #[prost(string, tag = "5")]
    pub title_prefix: String,
    #[prost(message, repeated, tag = "9")]
    pub title_parts: Vec<TitlePart>,
    #[prost(message, repeated, tag = "10")]
    pub content_blocks: Vec<ContentBlock>,
}

/// `StepRenderInfo.TitlePart`（oneof part 里只认文本）。
#[derive(Clone, PartialEq, Message)]
pub struct TitlePart {
    #[prost(string, tag = "1")]
    pub text: String,
}

/// `StepRenderInfo.ContentBlock`。
#[derive(Clone, PartialEq, Message)]
pub struct ContentBlock {
    #[prost(string, tag = "1")]
    pub markdown: String,
    #[prost(message, optional, tag = "3")]
    pub terminal: Option<TerminalBlock>,
}

/// `StepRenderInfo.TerminalBlock`。
#[derive(Clone, PartialEq, Message)]
pub struct TerminalBlock {
    #[prost(string, tag = "1")]
    pub command: String,
    #[prost(int32, optional, tag = "3")]
    pub exit_code: Option<i32>,
    #[prost(string, tag = "4")]
    pub output: String,
}

/// `exa.cortex_pb.CortexStepSystemMessage`。
#[derive(Clone, PartialEq, Message)]
pub struct SystemMessage {
    #[prost(string, tag = "1")]
    pub message: String,
    #[prost(message, optional, tag = "2")]
    pub render_info: Option<RenderInfo>,
    #[prost(string, tag = "3")]
    pub event_type: String,
}

/// `exa.cortex_pb.CortexStepErrorMessage`。
#[derive(Clone, PartialEq, Message)]
pub struct ErrorMessage {
    #[prost(message, optional, tag = "3")]
    pub error: Option<ErrorDetails>,
    #[prost(bool, tag = "5")]
    pub should_show_user: bool,
}

/// `exa.cortex_pb.CortexStepCheckpoint`（上下文压缩摘要）。
#[derive(Clone, PartialEq, Message)]
pub struct Checkpoint {
    #[prost(string, tag = "4")]
    pub user_intent: String,
    #[prost(string, tag = "5")]
    pub session_summary: String,
}

/// `exa.cortex_pb.CortexStepInvokeSubagent`。子代理是单独的对话库。
#[derive(Clone, PartialEq, Message)]
pub struct InvokeSubagent {
    #[prost(string, tag = "1")]
    pub subagent_name: String,
    #[prost(string, tag = "2")]
    pub prompt: String,
    #[prost(string, tag = "5")]
    pub conversation_id: String,
}

/// `exa.cortex_pb.CortexStepGeneratorMetadata`：`gen_metadata.data`，每次模型调用一行。
#[derive(Clone, PartialEq, Message)]
pub struct GeneratorMetadata {
    #[prost(message, optional, tag = "1")]
    pub chat_model: Option<ChatModel>,
    /// 本次调用产出的 step idx（如 `[5, 6]` = 回复 5 与工具 6）。
    #[prost(uint32, repeated, tag = "2")]
    pub step_indices: Vec<u32>,
}

/// `exa.cortex_pb.ChatModelMetadata`。字段 1、2 是整段系统提示与历史，不声明、不解码。
#[derive(Clone, PartialEq, Message)]
pub struct ChatModel {
    #[prost(message, optional, tag = "4")]
    pub usage: Option<ModelUsage>,
    #[prost(message, optional, tag = "9")]
    pub chat_start_metadata: Option<ChatStart>,
    /// 如 "gemini-4-neon"。
    #[prost(string, tag = "19")]
    pub response_model: String,
    /// 如 "Gemini 4 Neon (High)"。
    #[prost(string, tag = "21")]
    pub model_display_name: String,
}

/// `exa.cortex_pb.ChatStartMetadata`。
#[derive(Clone, PartialEq, Message)]
pub struct ChatStart {
    #[prost(message, optional, tag = "10")]
    pub context_window_metadata: Option<ContextWindow>,
}

/// `exa.cortex_pb.ContextWindowMetadata`：请求发出时的估算值。
#[derive(Clone, PartialEq, Message)]
pub struct ContextWindow {
    #[prost(int32, tag = "1")]
    pub estimated_tokens_used: i32,
    #[prost(int32, optional, tag = "4")]
    pub max_context_tokens: Option<i32>,
}

/// `exa.cortex_pb.ExecutorMetadata`：`executor_metadata.data`，每回合结束后写一行。
#[derive(Clone, PartialEq, Message)]
pub struct ExecutorMetadata {
    #[prost(int32, tag = "1")]
    pub termination_reason: i32,
    #[prost(int32, tag = "3")]
    pub last_step_idx: i32,
    #[prost(string, tag = "9")]
    pub execution_id: String,
    #[prost(string, optional, tag = "12")]
    pub execution_error: Option<String>,
}

pub fn decode<M: Message + Default>(bytes: &[u8]) -> Option<M> {
    M::decode(bytes).ok()
}
