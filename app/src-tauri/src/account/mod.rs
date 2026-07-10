//! ProviderAccount 抽象层：通用账号/用量泳道类型、trait、注册表、provider 分键缓存。
//! Claude impl 见 claude.rs；后续新 provider 各自新建文件并在 ALL 注册。
//!
//! 设计模式镜像 meowo-reporter/src/agent.rs：trait + ALL 静态注册表 + for_provider + enum↔registry 配对单测。

mod claude;
mod codex;
mod kimi;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ═══ 共享 I/O 工具（pub(super) 供 claude.rs 使用） ═══

pub(super) fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok().map(PathBuf::from)
}

pub(super) fn usage_cache_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MEOWO_DB") {
        return Some(PathBuf::from(p).with_file_name("usage-cache.json"));
    }
    home_dir().map(|h| h.join(".meowo").join("usage-cache.json"))
}

pub(super) fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

pub(super) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ═══ 1. 通用类型 ═══

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

// 供前端 serde 字段匹配/未来 lane 筛选用；as_str/from_str/ALL 已在单测 roundtrip 覆盖，
// 但 dead_code lint 不计入 #[cfg(test)] 内的调用，故此处显式豁免。
#[allow(dead_code)]
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

/// 某 provider 的全部用量泳道。note 携带补充文字（如 extra_usage_enabled）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderUsage {
    pub lanes: Vec<UsageLane>,
    pub note: Option<String>,
}

/// 通用账号信息（字段可选，部分 provider 不提供所有字段）。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Account {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub organization: Option<String>,
    pub plan: Option<String>,
    /// 登录来源标签（如 "oauth" / "api-key"），provider 自定义填写。
    pub login_label: Option<String>,
}

/// get_accounts 命令返回的单条 provider 载荷。
#[derive(Debug, Clone, Serialize)]
pub struct ProviderAccountPayload {
    pub provider: String,
    pub account: Option<Account>,
    pub usage: Option<ProviderUsage>,
    pub usage_supported: bool,
}

// ═══ 2. ProviderAccount trait ═══

/// Provider 账号抽象：account 信息 + 用量泳道 + 是否支持用量查询。
/// Sync 约束保证静态注册表（&'static dyn ProviderAccount）可跨线程共享。
pub trait ProviderAccount: Sync {
    fn id(&self) -> meowo_agent::AgentId;
    fn account(&self) -> Option<Account>;
    /// force=false：仅读缓存，不联网；force=true：可触发网络刷新（含 60s 限频）。
    fn usage(&self, force: bool) -> Option<ProviderUsage>;
    /// 该 provider 是否支持用量查询。false 时 refresh_usage_v2 返回 USAGE_UNSUPPORTED。
    fn usage_supported(&self) -> bool {
        true
    }
}

// ═══ 3. 注册表 ═══

static CLAUDE_PA: claude::ClaudeProviderAccount = claude::ClaudeProviderAccount;
static KIMI_PA: kimi::KimiProviderAccount = kimi::KimiProviderAccount;
static CODEX_PA: codex::CodexProviderAccount = codex::CodexProviderAccount;
static ALL_PA: &[&dyn ProviderAccount] = &[&CLAUDE_PA, &KIMI_PA, &CODEX_PA];

/// 按 agent 身份取 ProviderAccount 实现；遍历 ALL 注册表。入参恒来自插件注册表，故必然命中；
/// find 失败时回退 claude 兜底（配对测试守住这点）。
pub fn for_agent(id: meowo_agent::AgentId) -> &'static dyn ProviderAccount {
    ALL_PA.iter().copied().find(|a| a.id() == id).unwrap_or(&CLAUDE_PA)
}

/// 所有已注册 provider（供 get_accounts 遍历）。
pub fn all() -> &'static [&'static dyn ProviderAccount] {
    ALL_PA
}

// ═══ 4. Provider 分键缓存 ═══
//
// 当前格式：
//   {"providers": {"claude": ProviderUsage, ...}, "fetched_at_map": {"claude": ms, ...}, ...}
// 旧扁平格式（仅容错读取，仅 claude；写入方已全部移除，保留一个版本周期后可删）：
//   {"usage": Usage, "fetched_at": ms}

/// 读某 provider 的缓存用量。
/// 先试新格式 providers.{key}，再对 claude 兼容旧扁平 usage 字段。
pub fn read_cached_usage(k: meowo_agent::AgentId) -> Option<ProviderUsage> {
    let v = read_json(&usage_cache_path()?)?;
    // 新格式
    if let Some(providers) = v.get("providers") {
        if let Some(raw) = providers.get(k.as_str()) {
            if let Ok(pu) = serde_json::from_value::<ProviderUsage>(raw.clone()) {
                return Some(pu);
            }
        }
    }
    // 旧扁平格式容错（仅 claude）
    if k == meowo_agent::id::CLAUDE {
        if let Some(old_raw) = v.get("usage") {
            let old: claude::Usage = serde_json::from_value(old_raw.clone()).ok()?;
            return Some(claude::map_to_provider_usage(&old));
        }
    }
    None
}

/// write_cached_usage 的「读-合并-写」临界区锁：贴纸挂载/定时刷新会对多个 provider 同 tick 并发
/// refresh_usage（各自 spawn_blocking），不串行化时后写者会用旧快照覆盖先写者刚落盘的条目（丢失更新）。
static USAGE_CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 把某 provider 用量写入缓存（providers 分键合并写入，唯一写入方）。
pub fn write_cached_usage(k: meowo_agent::AgentId, usage: &ProviderUsage) {
    let Some(p) = usage_cache_path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _guard = USAGE_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 读现有文件合并：只更新本 provider 条目，其它 provider 的缓存原样保留。
    let mut root = read_json(&p).unwrap_or_else(|| serde_json::json!({}));
    let obj = match root.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    // providers map
    let providers = obj.entry("providers").or_insert_with(|| serde_json::json!({}));
    if let Some(pm) = providers.as_object_mut() {
        if let Ok(val) = serde_json::to_value(usage) {
            pm.insert(k.as_str().to_string(), val);
        }
    }
    // fetched_at_map（每个 provider 独立时间戳）
    let fat = obj.entry("fetched_at_map").or_insert_with(|| serde_json::json!({}));
    if let Some(fm) = fat.as_object_mut() {
        fm.insert(k.as_str().to_string(), serde_json::json!(now_ms()));
    }
    // 原子写：读端（get_accounts/cache_is_fresh）裸读本文件，直写可能被读到半截而解析失败、
    // 整份缓存瞬时作废。
    if let Ok(s) = serde_json::to_string(&root) {
        let _ = crate::fsutil::write_atomic(&p, &s);
    }
}

/// 某 provider 的缓存是否在 fresh_ms 内。支持新 fetched_at_map 与旧 fetched_at（仅 claude）。
pub fn cache_is_fresh(k: meowo_agent::AgentId, fresh_ms: i64) -> bool {
    let Some(v) = usage_cache_path().and_then(|p| read_json(&p)) else {
        return false;
    };
    // 新格式 fetched_at_map
    if let Some(t) = v
        .get("fetched_at_map")
        .and_then(|m| m.get(k.as_str()))
        .and_then(|x| x.as_i64())
    {
        return now_ms() - t < fresh_ms;
    }
    // 旧扁平格式（仅 claude）
    if k == meowo_agent::id::CLAUDE {
        if let Some(t) = v.get("fetched_at").and_then(|x| x.as_i64()) {
            return now_ms() - t < fresh_ms;
        }
    }
    false
}

// ═══ 5. re-export（refresh_usage 调用路径） ═══

pub use claude::USAGE_UNSUPPORTED;

// ═══ 6. Tests ═══

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_kind_as_str_roundtrip() {
        // as_str→from_str 全量 roundtrip：确保每个 variant 序列化后能正确还原。
        for &k in UsageKind::ALL {
            assert_eq!(
                UsageKind::from_str(k.as_str()),
                k,
                "UsageKind::{k:?} as_str/from_str roundtrip 失败"
            );
        }
    }

    #[test]
    fn usage_kind_unknown_maps_to_other() {
        assert_eq!(UsageKind::from_str("nonexistent_kind"), UsageKind::Other);
        assert_eq!(UsageKind::from_str(""), UsageKind::Other);
        assert_eq!(UsageKind::from_str("FIVE_HOUR"), UsageKind::Other); // 大小写不同 → Other
    }

    /// registry↔registry 单一事实源守护：插件注册表每个 agent 必有一个 ALL_PA 中的 ProviderAccount，
    /// 反之亦然；二者数量相等。加新 agent 漏注册任一侧即在此处失败。
    /// Phase 3 会把 ProviderAccount 折进插件的 `account()` 能力槽，届时本测试连同注册表一起消失。
    #[test]
    fn every_plugin_has_provider_account_and_vice_versa() {
        for p in meowo_agent::all() {
            assert!(
                ALL_PA.iter().any(|a| a.id() == p.id()),
                "插件 {} 无对应 ProviderAccount 注册",
                p.id()
            );
        }
        for a in ALL_PA {
            assert!(
                meowo_agent::by_id(a.id().as_str()).is_some(),
                "ProviderAccount({}) 未在插件注册表登记",
                a.id()
            );
        }
        assert_eq!(ALL_PA.len(), meowo_agent::all().len(), "ALL_PA 与插件注册表数量不等");
    }
}
