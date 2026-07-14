//! 账号/用量的**编排层**：缓存、限频、注入端口。
//!
//! 各 agent 的凭据格式、OAuth 刷新、用量 API schema 全部住在 `meowo_agent::plugins::<id>::account`，
//! 由 `AgentPlugin::account()` 能力槽取用；本模块不认识任何具体 agent。
//!
//! 缓存与 60s 限频**刻意留在这里**：它们是「meowo 怎么管用量」的策略，不是「某个 agent 怎么查用量」
//! 的知识。此前三家各自在 `usage()` 里重复了同一套读缓存/判鲜/写回逻辑。

use meowo_agent::{AccountCap, AgentId, ProviderUsage};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use meowo_agent::UsageKind;
pub use meowo_agent::{Account, USAGE_UNSUPPORTED};

/// 用量缓存的新鲜期：期内的 force 刷新直接吃缓存，不打 API。
const USAGE_FRESH_MS: i64 = 60_000;

// ═══ 共享 I/O 工具 ═══

fn home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .map(PathBuf::from)
}

fn usage_cache_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MEOWO_DB") {
        return Some(PathBuf::from(p).with_file_name("usage-cache.json"));
    }
    home_dir().map(|h| h.join(".meowo").join("usage-cache.json"))
}

fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ═══ get_accounts 的返回载荷 ═══

/// get_accounts 命令返回的单条 agent 载荷。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderAccountPayload {
    pub provider: String,
    pub account: Option<Account>,
    pub usage: Option<ProviderUsage>,
    pub usage_supported: bool,
    pub relay_enabled: bool,
}

// ═══ 能力取用 ═══

/// 该 agent 的账号能力（未注册 / 无账号概念 → None）。
pub fn account_cap(id: AgentId) -> Option<&'static dyn AccountCap> {
    meowo_agent::by_id(id.as_str())?.account()
}

/// 读账号信息（只读本地，不联网）。
pub fn account_of(id: AgentId) -> Option<Account> {
    let p = crate::ports::HostPorts::for_agent(id);
    account_cap(id)?.account(&p.as_ports())
}

/// 该 agent 当前是否支持用量查询。
pub fn usage_supported(id: AgentId) -> bool {
    if crate::settings::load_settings().relay.enabled(id) {
        return false;
    }
    let p = crate::ports::HostPorts::for_agent(id);
    account_cap(id).is_some_and(|c| c.usage_supported(&p.as_ports()))
}

/// 取用量。
///
/// - `force == false`：只读缓存，绝不联网。
/// - `force == true`：缓存仍新鲜（< 60s）则复用，否则调 agent 的 `fetch_usage` 拉一次并写回。
///
/// 这段策略此前被三个 agent 各抄了一份；现在它只存在于这里，插件只负责「去 API 拿」。
pub fn usage_of(id: AgentId, force: bool) -> Option<ProviderUsage> {
    if crate::settings::load_settings().relay.enabled(id) {
        return None;
    }
    if !force {
        return read_cached_usage(id);
    }
    if cache_is_fresh(id, USAGE_FRESH_MS) {
        if let Some(cached) = read_cached_usage(id) {
            return Some(cached);
        }
    }
    // 端口按 agent 绑定：代理是 per-agent 的（claude 走代理、kimi 直连是常态）。
    let p = crate::ports::HostPorts::for_agent(id);
    match account_cap(id)?.fetch_usage(&p.as_ports()) {
        Ok(u) => {
            write_cached_usage(id, &u);
            Some(u)
        }
        Err(e) => {
            eprintln!("Meowo usage[{id}]: {e}");
            None
        }
    }
}

/// 所有注册了账号能力的 agent（供 get_accounts 遍历）。
pub fn all_with_account() -> impl Iterator<Item = &'static dyn meowo_agent::AgentPlugin> {
    meowo_agent::all()
        .iter()
        .copied()
        .filter(|p| p.account().is_some())
}

// ═══ 用量缓存（按 agent 分键） ═══
//
// 当前格式：
//   {"providers": {"claude": ProviderUsage, ...}, "fetched_at_map": {"claude": ms, ...}, ...}
// 旧扁平格式（仅容错读取，仅 claude；写入方已全部移除，保留一个版本周期后可删）：
//   {"usage": Usage, "fetched_at": ms}

/// 读某 agent 的缓存用量。先试新格式 `providers.<id>`，再对 claude 兼容旧扁平 usage 字段。
pub fn read_cached_usage(id: AgentId) -> Option<ProviderUsage> {
    let v = read_json(&usage_cache_path()?)?;
    if let Some(providers) = v.get("providers") {
        if let Some(raw) = providers.get(id.as_str()) {
            if let Ok(pu) = serde_json::from_value::<ProviderUsage>(raw.clone()) {
                return Some(pu);
            }
        }
    }
    // 旧扁平格式容错（仅 claude）。
    if id == meowo_agent::id::CLAUDE {
        if let Some(old_raw) = v.get("usage") {
            let old: meowo_agent::plugins::claude::account::Usage =
                serde_json::from_value(old_raw.clone()).ok()?;
            return Some(meowo_agent::plugins::claude::account::map_to_provider_usage(&old));
        }
    }
    None
}

/// write_cached_usage 的「读-合并-写」临界区锁：贴纸挂载/定时刷新会对多个 agent 同 tick 并发
/// 刷新（各自 spawn_blocking），不串行化时后写者会用旧快照覆盖先写者刚落盘的条目（丢失更新）。
static USAGE_CACHE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 把某 agent 的用量写入缓存（providers 分键合并写入，唯一写入方）。
pub fn write_cached_usage(id: AgentId, usage: &ProviderUsage) {
    let Some(p) = usage_cache_path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _guard = USAGE_CACHE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 读现有文件合并：只更新本 agent 条目，其它 agent 的缓存原样保留。
    let mut root = read_json(&p).unwrap_or_else(|| serde_json::json!({}));
    let Some(obj) = root.as_object_mut() else {
        return;
    };

    let providers = obj
        .entry("providers")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(pm) = providers.as_object_mut() {
        if let Ok(val) = serde_json::to_value(usage) {
            pm.insert(id.as_str().to_string(), val);
        }
    }
    // fetched_at_map（每个 agent 独立时间戳）
    let fat = obj
        .entry("fetched_at_map")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(fm) = fat.as_object_mut() {
        fm.insert(id.as_str().to_string(), serde_json::json!(now_ms()));
    }
    // 原子写：读端（get_accounts/cache_is_fresh）裸读本文件，直写可能被读到半截而解析失败、
    // 整份缓存瞬时作废。
    if let Ok(s) = serde_json::to_string(&root) {
        let _ = meowo_agent::fsutil::write_atomic(&p, &s);
    }
}

/// 某 agent 的缓存是否在 fresh_ms 内。支持新 fetched_at_map 与旧 fetched_at（仅 claude）。
pub fn cache_is_fresh(id: AgentId, fresh_ms: i64) -> bool {
    let Some(v) = usage_cache_path().and_then(|p| read_json(&p)) else {
        return false;
    };
    if let Some(t) = v
        .get("fetched_at_map")
        .and_then(|m| m.get(id.as_str()))
        .and_then(|x| x.as_i64())
    {
        return now_ms() - t < fresh_ms;
    }
    // 旧扁平格式（仅 claude）
    if id == meowo_agent::id::CLAUDE {
        if let Some(t) = v.get("fetched_at").and_then(|x| x.as_i64()) {
            return now_ms() - t < fresh_ms;
        }
    }
    false
}

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
                "UsageKind::{k:?} roundtrip 失败"
            );
        }
    }

    #[test]
    fn usage_kind_unknown_maps_to_other() {
        assert_eq!(UsageKind::from_str("nonexistent_kind"), UsageKind::Other);
        assert_eq!(UsageKind::from_str(""), UsageKind::Other);
        assert_eq!(UsageKind::from_str("FIVE_HOUR"), UsageKind::Other); // 大小写不同 → Other
    }

    /// 三家目前都声明了账号能力；漏接能力槽会让该 agent 的账号卡片静默变成「未登录」。
    #[test]
    fn every_plugin_declares_account_capability() {
        for p in meowo_agent::all() {
            assert!(p.account().is_some(), "{} 未声明账号能力", p.id());
        }
        assert_eq!(all_with_account().count(), meowo_agent::all().len());
    }
}
