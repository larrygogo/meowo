//! 账号能力：通用的账号 / 用量类型 + [`AccountCap`] 能力槽。
//!
//! 各家的凭据格式、OAuth 刷新、用量 API schema 全在 `plugins/<id>/account.rs`；本模块只定义
//! 它们共同产出的形状。联网与密钥链经 [`crate::ports`] 注入，故本层不依赖 HTTP 栈、无平台 `cfg`。
//!
//! **缓存与限频不在这里。** `fetch_usage` 就是「去 API 拿一次」，读缓存 / 60s 限频 / 写回全部归
//! 宿主的编排层——此前三个 agent 各自在 `usage()` 里重复了同一套缓存逻辑。

use serde::{Deserialize, Serialize};

use crate::ports::Ports;
use crate::variant::Installation;

/// 用量泳道种类。serde snake_case 与前端/API 字段对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageKind {
    FiveHour,
    SevenDay,
    Opus,
    Weekly,
    /// 按模型限定的每周配额(claude usage API 的 `limits[]` 里 `weekly_scoped` 条目,
    /// 如 Fable/Opus 专属周限)。模型名在 [`UsageLane::label`],不再为每个模型加枚举。
    ModelWeekly,
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
            Self::ModelWeekly => "model_weekly",
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
            "model_weekly" => Self::ModelWeekly,
            "balance" => Self::Balance,
            _ => Self::Other,
        }
    }

    pub const ALL: &'static [UsageKind] = &[
        Self::FiveHour,
        Self::SevenDay,
        Self::Opus,
        Self::Weekly,
        Self::ModelWeekly,
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
    /// 泳道的附加名字。目前只有 [`UsageKind::ModelWeekly`] 用:被限定的模型名
    /// (API 的 `scope.model.display_name`,如 "Fable")。`default` 兼容旧缓存/旧 DTO。
    #[serde(default)]
    pub label: Option<String>,
}

/// 某 agent 的全部用量泳道。note 携带补充文字（如 extra_usage_enabled / 余额）。
///
/// `plan` 是个**账号侧**的字段，长在用量结构里是有原因的：有些 agent 的身份信息**只出现在用量
/// 响应里**——kimi 的 `/usages` 带 `user.membership.level`（会员等级），而它的凭据、JWT、本地文件
/// 里根本没有任何可读的身份（实测：只有内部 userId）。而 [`AccountCap::account`] 是纯本地、
/// 高频调用的（登录轮询 2s 一跑），不能为了一个等级去联网。
///
/// 所以让用量刷新时把它顺带捎回来，由宿主缓存、并合并进账号卡片。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderUsage {
    pub lanes: Vec<UsageLane>,
    pub note: Option<String>,
    /// 用量接口顺带返回的**套餐名**（kimi 的会员等级）。宿主缓存后合并进账号卡片。
    ///
    /// 缺省 `None`：另几家的套餐信息本就在账号侧（claude 的 `.claude.json` 有 `userRateLimitTier`），
    /// 不必绕这一圈。
    #[serde(default)]
    pub plan: Option<String>,
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

// 用量请求失败的分类码（照 Orca 的错误分类：401/403 过期、429 限流、5xx 服务端、连不上）。前端按码出
// 说得清原因的文案，宿主按码决定退避多久。之前一律成了「用量获取失败」，看不出该等还是该重新登录。
/// 凭据过期或被拒（本地 token 已过期，或接口回 401/403）。不替 CLI 刷新：刷新会轮换 refresh token，
/// CLI 手里那份就失效了（Orca 同样只读凭据）。CLI 下次运行会自己刷新，额度随之恢复。
pub const USAGE_EXPIRED: &str = "USAGE_EXPIRED";
/// 被限流（429）。后面可带 `:<秒>`，是接口 Retry-After 给的等待时长。
pub const USAGE_RATE_LIMITED: &str = "USAGE_RATE_LIMITED";
/// 服务端出错（5xx）。
pub const USAGE_SERVER: &str = "USAGE_SERVER";
/// 连不上（网络、代理、超时）。
pub const USAGE_NETWORK: &str = "USAGE_NETWORK";

/// 把用量请求的 HTTP 失败归成上面的分类码；归不进的原样给出说明。
pub fn classify_usage_http_error(e: &crate::ports::HttpError) -> String {
    use crate::ports::HttpError;
    match e {
        HttpError::Status(401 | 403, _) => USAGE_EXPIRED.to_string(),
        HttpError::Status(429, body) => match crate::ports::retry_after_secs(body) {
            Some(secs) => format!("{USAGE_RATE_LIMITED}:{secs}"),
            None => USAGE_RATE_LIMITED.to_string(),
        },
        HttpError::Status(500..=599, _) => USAGE_SERVER.to_string(),
        HttpError::Transport(_) => USAGE_NETWORK.to_string(),
        other => format!("请求用量失败：{other}"),
    }
}

/// API Key 登录能力：把用户提供的 key 落成该 agent 自己认得的持久配置。
///
/// 通用能力，目前没有插件声明。给「交互式登录走不通、又没有输入 key 的登录子命令」的 CLI 用：
/// 交互式登录（拉终端走 OAuth）帮不上忙，必须由宿主替用户把 key 写到 CLI 认的位置。最初为已移除的
/// gemini 插件引入（Google 停掉了个人版 Gemini Code Assist 的 OAuth，key 只能靠环境变量或
/// `~/.gemini/.env`）。
///
/// 与 [`crate::RelayCap`] 不是一回事：中转改的是**端点**（附带才要 key），且只对 meowo 拉起的
/// 会话生效；这里写的是 agent 自己的配置，用户在任何终端裸跑 CLI 同样生效。
pub trait ApiKeyLoginCap: Sync {
    /// 校验并保存 key。**幂等**：重复保存即覆盖。实现负责把 key 写到该实况（`inst.data_dir`）下
    /// CLI 自己会读的位置，并确保 CLI 下次启动就走 API-key 认证、不再去碰 OAuth。
    fn save_api_key(&self, inst: &Installation, key: &str) -> Result<(), String>;

    /// 清除已保存的 key（登出的一部分）。**幂等**：没配过也返回 Ok。
    /// 只清 key 本身，不碰配置里的其它内容。
    fn clear_api_key(&self, inst: &Installation) -> Result<(), String>;
}

/// 账号能力：读账号信息 + 拉实时用量。不声明此能力的 agent，其账号卡片显示为未登录。
///
/// # 三个方法都接 [`Installation`]
///
/// 因为「哪个账号」这件事，**完全由传进来的那份实况决定**：默认账号给的是 agent 自己的目录，
/// 某个 profile（多账号）给的是那个 profile 的私有目录，凭据路径也随之落在它里面。
///
/// 这里曾经是无参的，各实现自己去 `registry::installation(ID)` 取默认实况——于是多账号根本
/// 读不出登录态：无论问的是哪个 profile，答的永远是默认账号那一个。
pub trait AccountCap: Sync {
    /// 读该实况的登录态与账号信息。**只读本地**（凭据文件 / 密钥链），不联网——调用方会在轮询
    /// 登录状态时高频调用它。
    fn account(&self, inst: &Installation, ports: &Ports) -> Option<Account>;

    /// 联网拉一次实时用量（含按需刷新 token）。
    ///
    /// **不读缓存、不做限频、不写回**——那些归宿主编排层。返回 `Err` 时其内容会被当作错误码/文案
    /// 回传前端；读不到官方 OAuth 凭据时应返回 [`USAGE_UNSUPPORTED`]。
    fn fetch_usage(&self, inst: &Installation, ports: &Ports) -> Result<ProviderUsage, String>;

    /// 该 agent 当前是否支持用量查询（如 claude 在第三方登录下不支持）。默认支持。
    fn usage_supported(&self, _inst: &Installation, _ports: &Ports) -> bool {
        true
    }
}
