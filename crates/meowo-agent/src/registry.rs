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

/// 按身份串取插件（`"claude"` / `"kimi"` / `"codex"`，与 DB / 前端 provider key 同值）。
pub fn by_id(id: &str) -> Option<&'static dyn AgentPlugin> {
    ALL.iter().copied().find(|p| p.id().as_str() == id)
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

    #[test]
    fn every_plugin_declares_at_least_one_variant() {
        for p in all() {
            assert!(!p.variants().is_empty(), "{} 无变体", p.id());
        }
    }
}
