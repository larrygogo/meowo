//! 能力槽。meowo 提供全部能力位，agent 只声明自己有的那些；不声明的由框架降级。
//!
//! 这与「每个 agent 实现十几个方法、其中一半返回 `false`/`None`」的形态不同：`writes_tab_token()`
//! 返回 `false`、`transcript()` 返回 `None`、`usage_supported()` 返回 `false` 这类「我没有这个能力」
//! 的表达，统一成能力查询返回 `None`。codex 不支持重命名回写，就不实现那个方法；kimi 不读
//! transcript，就不提供 `TranscriptSpec`。
//!
//! 能力方法**不接** reporter 的 `HookEvent`——那个类型依赖 `meowo_store::TodoInput`，让插件层
//! 反向依赖 DB 层。改为传只含所需字段的 [`HookContext`]，能力看到的正是它需要的。

use crate::transcript::TranscriptSpec;

/// hook 事件里被 agent 能力用到的那几个字段。由调用方（reporter 的 dispatch）从 hook 负载构造。
#[derive(Debug, Default, Clone, Copy)]
pub struct HookContext<'a> {
    pub session_id: &'a str,
    /// hook 携带的 transcript 路径（codex 的 rollout / claude 的 jsonl）。缺失时能力自行兜底查找。
    pub transcript_path: Option<&'a str>,
    /// hook 携带的最近一条 AI 正文（claude / codex 带，kimi 不带）。
    pub last_assistant_message: Option<&'a str>,
}

/// Stop 时要落库的输出：最近一条 AI 正文 + 模型展示名。
#[derive(Debug, Default, PartialEq)]
pub struct StopOutputs {
    pub last_ai: Option<String>,
    pub model: Option<String>,
}

/// 会话上下文占用快照。
#[derive(Debug, Default, PartialEq)]
pub struct ContextUsage {
    /// 已用百分比（0–100，已 clamp）。
    pub used_pct: i64,
    /// 上下文窗口大小（token）。
    pub window: i64,
    /// 模型展示名（usage 记录里顺带能读到的 agent 才填,如 kimi）。None = 该通道不知道模型,
    /// 落库时不覆盖已有值。没有它,kimi 的模型要等第一次 Stop 才出现——新会话第一回合
    /// 跑得再久,卡片上也一直没有模型。
    pub model: Option<String>,
}

/// 一条待办的原始快照。`status` 保留 agent **自己写的词**（claude 是 `completed`、
/// kimi 是 `done`），归一化交给 DB 层的 `TodoStatus::from_str`——插件层不依赖 store，
/// 也不该替它决定枚举取值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSnapshot {
    pub content: String,
    pub status: String,
}

/// 会话遥测能力：从 hook 负载或该 agent 的会话文件里取出「正文 / 模型 / 上下文占用 / 标题」，
/// 以及把重命名写回 agent 自己的持久层。
///
/// 全部方法都有默认实现——一个 agent 只覆写它真正支持的那些。
pub trait TelemetryCap: Sync {
    /// Stop 时取最近 AI 正文 + 模型。claude 用 hook 携带的正文（模型走 statusline）；
    /// codex 正文走 hook、模型读 rollout；kimi 两者都从 wire.jsonl 一次读出。
    fn stop_outputs(&self, _ctx: &HookContext) -> StopOutputs {
        StopOutputs::default()
    }

    /// 从会话日志读最近一次上下文占用。claude 返回 None（走 statusline）。
    fn read_context(&self, _ctx: &HookContext) -> Option<ContextUsage> {
        None
    }

    /// 从会话日志读**当前的待办快照**。
    ///
    /// 与 hook 路径互补：hook 只在 meowo 在场时捕获得到，而会话日志是 agent 自己一直在写的。
    /// 有了它，「中途才启动 meowo」「hook 曾漏接」「早先解析有误」这几种情况都能按需重建，
    /// 不必干等 agent 下一次调用待办工具。None = 该 agent 的日志里读不到待办。
    fn read_todos(&self, _ctx: &HookContext) -> Option<Vec<TodoSnapshot>> {
        None
    }

    /// 该 agent 的 transcript 规格：提供「定位 + 标题解析 + 增量分析」。
    /// codex/kimi 的 spec 只供结构化对话；标题仍走首条 prompt、预览/模型走 stop_outputs。
    fn transcript(&self) -> Option<&'static dyn TranscriptSpec> {
        None
    }

    /// 是否由 transcript 解析标题。与 [`transcript`](Self::transcript) 刻意分开：可以有
    /// 「提供了 transcript 规格（供预览/上下文分析）但标题另有来源」的 agent。
    fn resolves_transcript_title(&self) -> bool {
        false
    }

    /// 把重命名同步到该 agent 自己的持久层，使它自身的会话列表/恢复列表也显示新名字。
    /// 返回是否成功落地（失败不阻断调用方更新 DB 标题）。默认不支持。
    fn write_rename(&self, _session_id: &str, _cwd: Option<&str>, _title: &str) -> bool {
        false
    }
}
