//! ProviderAccount 抽象层：通用账号/用量泳道类型、trait、注册表、provider 分键缓存。
//! Claude impl 见 claude.rs；后续新 provider 各自新建文件并在 ALL 注册。
//!
//! 设计模式镜像 cc-reporter/src/agent.rs：trait + ALL 静态注册表 + for_provider + enum↔registry 配对单测。

mod claude;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ═══ 共享 I/O 工具（pub(super) 供 claude.rs 使用） ═══

pub(super) fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok().map(PathBuf::from)
}

pub(super) fn usage_cache_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CC_KANBAN_DB") {
        return Some(PathBuf::from(p).with_file_name("usage-cache.json"));
    }
    home_dir().map(|h| h.join(".cc-kanban").join("usage-cache.json"))
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
    fn key(&self) -> cc_store::ProviderKey;
    fn account(&self) -> Option<Account>;
    /// force=false：仅读缓存，不联网；force=true：可触发网络刷新（含 60s 限频）。
    fn usage(&self, force: bool) -> Option<ProviderUsage>;
    /// 该 provider 是否支持用量查询。false 时 refresh_usage_v2 返回 USAGE_UNSUPPORTED。
    fn usage_supported(&self) -> bool {
        true
    }
}

// ═══ 3. Kimi / Codex 存根（暂不支持用量，满足 enum↔registry 配对约束） ═══

struct KimiProviderAccount;
impl ProviderAccount for KimiProviderAccount {
    fn key(&self) -> cc_store::ProviderKey {
        cc_store::ProviderKey::Kimi
    }
    fn account(&self) -> Option<Account> {
        None
    }
    fn usage(&self, _force: bool) -> Option<ProviderUsage> {
        None
    }
    fn usage_supported(&self) -> bool {
        false
    }
}

struct CodexProviderAccount;
impl ProviderAccount for CodexProviderAccount {
    fn key(&self) -> cc_store::ProviderKey {
        cc_store::ProviderKey::Codex
    }
    fn account(&self) -> Option<Account> {
        None
    }
    fn usage(&self, _force: bool) -> Option<ProviderUsage> {
        None
    }
    fn usage_supported(&self) -> bool {
        false
    }
}

// ═══ 4. 注册表 ═══

static CLAUDE_PA: claude::ClaudeProviderAccount = claude::ClaudeProviderAccount;
static KIMI_PA: KimiProviderAccount = KimiProviderAccount;
static CODEX_PA: CodexProviderAccount = CodexProviderAccount;
static ALL_PA: &[&dyn ProviderAccount] = &[&CLAUDE_PA, &KIMI_PA, &CODEX_PA];

/// 按 provider key 取 ProviderAccount 实现；遍历 ALL 注册表。未知回退 claude 兜底。
pub fn for_provider(k: cc_store::ProviderKey) -> &'static dyn ProviderAccount {
    ALL_PA.iter().copied().find(|a| a.key() == k).unwrap_or(&CLAUDE_PA)
}

/// 所有已注册 provider（供 get_accounts 遍历）。
pub fn all() -> &'static [&'static dyn ProviderAccount] {
    ALL_PA
}

// ═══ 5. Provider 分键缓存 ═══
//
// 新格式（本任务引入）：
//   {"providers": {"claude": ProviderUsage, ...}, "fetched_at_map": {"claude": ms, ...}, ...}
// 旧扁平格式（容错读取，仅 claude）：
//   {"usage": Usage, "fetched_at": ms}
//
// 新格式写入与旧格式字段共存，旧 get_account/refresh_usage 命令写旧格式不受影响。

/// 读某 provider 的缓存用量。
/// 先试新格式 providers.{key}，再对 claude 兼容旧扁平 usage 字段。
pub fn read_cached_usage(k: cc_store::ProviderKey) -> Option<ProviderUsage> {
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
    if k == cc_store::ProviderKey::Claude {
        if let Some(old_raw) = v.get("usage") {
            let old: claude::Usage = serde_json::from_value(old_raw.clone()).ok()?;
            return Some(claude::map_to_provider_usage(&old));
        }
    }
    None
}

/// 把某 provider 用量写入缓存（新格式，与旧扁平字段共存，不破坏旧命令读取）。
pub fn write_cached_usage(k: cc_store::ProviderKey, usage: &ProviderUsage) {
    let Some(p) = usage_cache_path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // 读现有文件合并，不覆盖旧 usage/fetched_at 字段（旧命令仍依赖它们）。
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
    if let Ok(s) = serde_json::to_string(&root) {
        let _ = std::fs::write(&p, s);
    }
}

/// 某 provider 的缓存是否在 fresh_ms 内。支持新 fetched_at_map 与旧 fetched_at（仅 claude）。
pub fn cache_is_fresh(k: cc_store::ProviderKey, fresh_ms: i64) -> bool {
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
    if k == cc_store::ProviderKey::Claude {
        if let Some(t) = v.get("fetched_at").and_then(|x| x.as_i64()) {
            return now_ms() - t < fresh_ms;
        }
    }
    false
}

// ═══ 6. 后向兼容 re-export（旧命令 get_account/refresh_usage 调用路径） ═══

pub use claude::{
    get_account_payload, refresh_usage_payload, AccountPayload, Usage, USAGE_UNSUPPORTED,
};

// ═══ 7. Tests ═══

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

    #[test]
    fn every_provider_key_has_provider_account_and_vice_versa() {
        // enum↔registry 单一事实源守护：ProviderKey 每个 variant 必有一个 ALL_PA 中的 ProviderAccount，
        // 反之亦然；二者数量相等。加新 provider 漏注册任一侧即在此处失败。
        for &k in cc_store::ProviderKey::ALL {
            assert!(
                ALL_PA.iter().any(|a| a.key() == k),
                "ProviderKey {k:?} 无对应 ProviderAccount 注册"
            );
        }
        for a in ALL_PA {
            assert!(
                cc_store::ProviderKey::ALL.contains(&a.key()),
                "ProviderAccount(key={:?}) 不在 ProviderKey::ALL",
                a.key()
            );
        }
        assert_eq!(
            ALL_PA.len(),
            cc_store::ProviderKey::ALL.len(),
            "ALL_PA 与 ProviderKey::ALL 数量不等"
        );
    }
}
