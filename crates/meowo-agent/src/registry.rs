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

static KIMI: crate::plugins::kimi::Kimi = crate::plugins::kimi::Kimi;

/// 已迁入插件层的 agent。claude/codex 仍走 meowo-app 内的旧路径，迁完后补进来。
static ALL: &[&dyn AgentPlugin] = &[&KIMI];

pub fn all() -> &'static [&'static dyn AgentPlugin] {
    ALL
}

/// 按身份串取插件（`"kimi"` 等，与 DB / 前端 provider key 同值）。未迁入的 agent 返回 None。
pub fn by_id(id: &str) -> Option<&'static dyn AgentPlugin> {
    ALL.iter().copied().find(|p| p.id().as_str() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_id_matches_declared_id() {
        assert_eq!(by_id("kimi").map(|p| p.id().as_str()), Some("kimi"));
        assert!(by_id("claude").is_none(), "claude 尚未迁入插件层");
        assert!(by_id("nope").is_none());
    }

    #[test]
    fn every_plugin_declares_at_least_one_variant() {
        for p in all() {
            assert!(!p.variants().is_empty(), "{} 无变体", p.id());
        }
    }
}
