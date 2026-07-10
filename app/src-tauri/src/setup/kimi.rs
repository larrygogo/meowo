//! kimi 自动接线。**合并逻辑已迁入 `meowo_agent::config::ConfigFormat::KimiToml`**（含旧 Python 版
//! `hooks = []` 空内联数组的无损替换），本模块只剩 I/O 编排：解析实况 → 读配置 → 交给格式适配器
//! → 备份 + 原子写。目录由变体表决定（新版 `~/.kimi-code` 优先，旧版 `~/.kimi` 兼容）。

use meowo_agent::RepairReason;

/// kimi 的 ProviderSetup。config.toml 由 `kimi login` 生成，缺失 → 视为未完成登录，跳过不创建。
pub struct KimiSetup;

impl super::ProviderSetup for KimiSetup {
    fn id(&self) -> meowo_agent::AgentId {
        meowo_agent::id::KIMI
    }
    fn detect(&self) -> bool {
        meowo_reporter::kimi::kimi_install().is_some_and(|i| i.is_configured())
    }
    fn apply(&self) -> Option<RepairReason> {
        let Some(inst) = meowo_reporter::kimi::kimi_install() else {
            eprintln!("Meowo repair[kimi]: 解析不到 kimi 安装实况，跳过");
            return Some(RepairReason::NotDetected);
        };
        // config.toml 由 `kimi login` 生成——不存在即「需先登录」（变体表里声明为
        // MissingConfig::Fail(NeedLogin)），据此给前端精准提示而非泛化的失败文案。
        // 接线无写前改写、无副作用，故 amend / after_write 均为 None。
        super::wire_hooks(&inst, "kimi", None, None)
    }
}

#[cfg(test)]
mod tests {
    /// dry-run：KIMI_SHARE_DIR=<真实 kimi 数据目录的副本> 时跑 KimiSetup::apply，核对副本产物。
    /// 用法：复制 ~/.kimi-code（或 ~/.kimi）到临时目录，
    ///       KIMI_SHARE_DIR=<副本> cargo test -p meowo-app dryrun_kimi -- --ignored --nocapture
    ///
    /// 只打印结构性摘要，**绝不 dump 配置原文**——真实 config.toml 含 `[providers]` 下的 api_key。
    #[test]
    #[ignore]
    fn dryrun_kimi() {
        use crate::setup::ProviderSetup;
        let reason = super::KimiSetup.apply();
        let inst = meowo_reporter::kimi::kimi_install().expect("应解析出实况");
        let text = std::fs::read_to_string(inst.config_path()).unwrap_or_default();
        let doc: toml_edit::DocumentMut = text.parse().expect("产物应为合法 TOML");
        let hooks = doc.get("hooks").and_then(|h| h.as_array_of_tables()).map(|a| a.len()).unwrap_or(0);
        eprintln!("变体={} 配置={}", inst.variant_tag, inst.config_path().display());
        eprintln!("apply reason={reason:?}  [[hooks]] 条数={hooks}");
        eprintln!("SessionStart 已接线={}", inst.hooks.has_reporter(&text, "kimi"));
        eprintln!("顶层键={:?}", doc.as_table().iter().map(|(k, _)| k).collect::<Vec<_>>());
    }
}
