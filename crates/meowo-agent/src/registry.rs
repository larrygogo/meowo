//! 注册表：取代 `ProviderKey` 枚举做分支中枢。加/改 agent 只动 `plugins/`，不动这里的调用方。

use crate::{id::AgentId, variant::{Installation, Variant}};

/// 一个 agent 插件。核心只有「我是谁 + 我有哪些变体」；探测由默认实现按变体表逐个 probe。
pub trait AgentPlugin: Sync {
    fn id(&self) -> AgentId;
    fn display_name(&self) -> &'static str;

    /// 变体表，**按优先级排列**（新版在前）。首个变体同时充当「全新安装该装到哪」的默认。
    fn variants(&self) -> &'static [Variant];

    /// 本机实况：逐变体 probe，命中即返回；都不中 → None（＝未安装）。
    fn detect(&self) -> Option<Installation> {
        let home = crate::home_dir()?;
        self.variants().iter().find_map(|v| v.probe(self.id(), &home))
    }

    /// 未安装时的默认落点（首选变体的默认目录）。不保证目录存在。
    fn default_installation(&self) -> Option<Installation> {
        let home = crate::home_dir()?;
        let v = self.variants().first()?;
        let dir = v.data_dir.default_dir(&home)?;
        Some(v.installation_at(self.id(), dir, Some(&home)))
    }

    /// 探测到就用实况，否则退回默认落点。**路径解析的唯一入口**：读配置、找凭据、拼可执行都走它，
    /// 于是「kimi 的目录到底是哪个」只在此处回答一次。
    fn resolve(&self) -> Option<Installation> {
        self.detect().or_else(|| self.default_installation())
    }
}

static CLAUDE: crate::plugins::claude::Claude = crate::plugins::claude::Claude;
static KIMI: crate::plugins::kimi::Kimi = crate::plugins::kimi::Kimi;
static CODEX: crate::plugins::codex::Codex = crate::plugins::codex::Codex;

/// 全部 agent。三家均已迁入插件层——加 agent 只写 `plugins/<new>.rs` 再在此补一行。
static ALL: &[&dyn AgentPlugin] = &[&CLAUDE, &KIMI, &CODEX];

pub fn all() -> &'static [&'static dyn AgentPlugin] {
    ALL
}

/// 历史默认 agent。DB 里 `sessions.provider` 为 NULL/空的老会话即它，与 `meowo_store::DEFAULT_PROVIDER`
/// 及建表 SQL 的 `DEFAULT 'claude'` 同值（配对断言见 `meowo_reporter::agent` 的测试——那里同时依赖两个 crate）。
pub const DEFAULT_ID: AgentId = crate::id::CLAUDE;

/// 按身份串取插件（`"claude"` / `"kimi"` / `"codex"`，与 DB / 前端 provider key 同值）。
pub fn by_id(id: &str) -> Option<&'static dyn AgentPlugin> {
    ALL.iter().copied().find(|p| p.id().as_str() == id)
}

/// DB 列 / 命令行 `--provider` 的字符串 → 已注册插件。**身份解析的唯一入口。**
///
/// - `None` / 空串 → 默认插件（老会话没写过 provider 列）。
/// - 已注册的 id → 该插件。
/// - **未知 id → `None`**，绝不降级成默认。
///
/// 最后一条是刻意的：旧的 `ProviderKey::from_str` 把未知串静默解析成 `Claude`，于是一个由更新版
/// meowo 写入、本版本尚不认识的 provider，其会话会被当成 claude 来 resume / 读 transcript / 查用量
/// ——全都指向错误的 CLI。宁可让调用方拿到 `None` 后降级为「不提供 agent 专属能力」，也不冒名顶替。
pub fn resolve(provider: Option<&str>) -> Option<&'static dyn AgentPlugin> {
    match provider.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => by_id(id),
        None => by_id(DEFAULT_ID.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_id_matches_declared_id() {
        assert_eq!(by_id("claude").map(|p| p.id().as_str()), Some("claude"));
        assert_eq!(by_id("kimi").map(|p| p.id().as_str()), Some("kimi"));
        assert_eq!(by_id("codex").map(|p| p.id().as_str()), Some("codex"));
        assert!(by_id("nope").is_none());
    }

    /// 注册表与前端/DB 的 provider key 集合必须逐一对应——漏注册会让该 agent 的所有链路静默退化。
    #[test]
    fn registry_covers_every_provider_key() {
        let mut ids: Vec<&str> = all().iter().map(|p| p.id().as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["claude", "codex", "kimi"]);
    }

    /// 未知 provider 串**绝不**降级成默认插件——旧 `ProviderKey::from_str` 正是这么把未知 agent
    /// 的会话冒名成 claude 的。None/空串则走默认（老会话没写过 provider 列）。
    #[test]
    fn resolve_maps_unknown_to_none_and_empty_to_default() {
        assert_eq!(resolve(Some("kimi")).map(|p| p.id().as_str()), Some("kimi"));
        assert_eq!(resolve(None).map(|p| p.id().as_str()), Some("claude"));
        assert_eq!(resolve(Some("")).map(|p| p.id().as_str()), Some("claude"));
        assert_eq!(resolve(Some("  ")).map(|p| p.id().as_str()), Some("claude"));
        assert!(resolve(Some("gemini")).is_none());
        assert!(resolve(Some("nonsense")).is_none());
    }

    #[test]
    fn default_id_is_registered() {
        assert!(by_id(DEFAULT_ID.as_str()).is_some());
    }

    #[test]
    fn every_plugin_declares_at_least_one_variant() {
        for p in all() {
            assert!(!p.variants().is_empty(), "{} 无变体", p.id());
        }
    }
}
