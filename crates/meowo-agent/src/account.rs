//! 账号能力：通用的账号 / 用量类型 + [`AccountCap`] 能力槽。
//!
//! 各家的凭据格式、OAuth 刷新、用量 API schema 全在 `plugins/<id>/account.rs`；本模块只定义
//! 它们共同产出的形状。联网与密钥链经 [`crate::ports`] 注入，故本层不依赖 HTTP 栈、无平台 `cfg`。
//!
//! **缓存与限频不在这里。** `fetch_usage` 就是「去 API 拿一次」，读缓存 / 60s 限频 / 写回全部归
//! 宿主的编排层——此前三个 agent 各自在 `usage()` 里重复了同一套缓存逻辑。

use serde::{Deserialize, Serialize};

use crate::ports::Ports;

/// 用量泳道种类。serde snake_case 与前端/API 字段对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    FiveHour,
    SevenDay,
    Opus,
    Weekly,
    Balance,
    Other,
}

impl UsageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::SevenDay => "seven_day",
            Self::Opus => "opus",
            Self::Weekly => "weekly",
            Self::Balance => "balance",
            Self::Other => "other",
        }
    }

    /// 无副作用解析：未知 → Other。
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "five_hour" => Self::FiveHour,
            "seven_day" => Self::SevenDay,
            "opus" => Self::Opus,
            "weekly" => Self::Weekly,
            "balance" => Self::Balance,
            _ => Self::Other,
        }
    }

    pub const ALL: &'static [UsageKind] = &[
        Self::FiveHour,
        Self::SevenDay,
        Self::Opus,
        Self::Weekly,
        Self::Balance,
        Self::Other,
    ];
}

/// 单条用量泳道。used_pct=None 表示非百分比型（余额等），前端显示数值而不画进度条。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageLane {
    pub kind: UsageKind,
    /// 百分比（0‒100）；None = 非百分比型（余额）。
    pub used_pct: Option<f64>,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    /// 单位："percent" | "tokens" | "requests" | "usd"。
    pub unit: Option<String>,
    /// 重置时间（ISO 8601）。
    pub resets_at: Option<String>,
}

/// 某 agent 的全部用量泳道。note 携带补充文字（如 extra_usage_enabled / 余额）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderUsage {
    pub lanes: Vec<UsageLane>,
    pub note: Option<String>,
}

/// 通用账号信息（字段可选，部分 agent 不提供所有字段）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Account {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub organization: Option<String>,
    pub plan: Option<String>,
    /// 登录来源标签（如 "oauth" / "api-key"），各 agent 自定义填写。
    pub login_label: Option<String>,
}

/// 用量不可查的标记码：读不到可用的官方 OAuth 凭据（多为第三方/中转登录，或尚未在终端登录）。
/// 前端据此显示「当前登录方式不支持用量查询」而非通用报错。
pub const USAGE_UNSUPPORTED: &str = "USAGE_UNSUPPORTED";

/// 账号能力：读账号信息 + 拉实时用量。不声明此能力的 agent，其账号卡片显示为未登录。
pub trait AccountCap: Sync {
    /// 读本机登录态与账号信息。**只读本地**（凭据文件 / 密钥链），不联网——调用方会在轮询
    /// 登录状态时高频调用它。
    fn account(&self, ports: &Ports) -> Option<Account>;

    /// 联网拉一次实时用量（含按需刷新 token）。
    ///
    /// **不读缓存、不做限频、不写回**——那些归宿主编排层。返回 `Err` 时其内容会被当作错误码/文案
    /// 回传前端；读不到官方 OAuth 凭据时应返回 [`USAGE_UNSUPPORTED`]。
    fn fetch_usage(&self, ports: &Ports) -> Result<ProviderUsage, String>;

    /// 该 agent 当前是否支持用量查询（如 claude 在第三方登录下不支持）。默认支持。
    fn usage_supported(&self, _ports: &Ports) -> bool {
        true
    }
}
