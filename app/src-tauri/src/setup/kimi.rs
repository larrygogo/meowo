//! kimi 自动接线。**合并逻辑已迁入 `meowo_agent::config::ConfigFormat::KimiToml`**（含旧 Python 版
//! `hooks = []` 空内联数组的无损替换），本模块只剩 I/O 编排：解析实况 → 读配置 → 交给格式适配器
//! → 备份 + 原子写。目录由变体表决定（新版 `~/.kimi-code` 优先，旧版 `~/.kimi` 兼容）。

use meowo_agent::{EnsureOutcome, RepairReason};

/// kimi 的 ProviderSetup。config.toml 由 `kimi login` 生成，缺失 → 视为未完成登录，跳过不创建。
pub struct KimiSetup;

impl super::ProviderSetup for KimiSetup {
    fn key(&self) -> meowo_store::ProviderKey {
        meowo_store::ProviderKey::Kimi
    }
    fn detect(&self) -> bool {
        meowo_reporter::kimi::kimi_share_dir().is_some_and(|d| d.is_dir())
    }
    fn apply(&self) -> Option<RepairReason> {
        let Some(inst) = meowo_reporter::kimi::kimi_install() else {
            eprintln!("Meowo repair[kimi]: 解析不到 kimi 安装实况，跳过");
            return Some(RepairReason::NotDetected);
        };
        let cfg = inst.config_path();
        // config.toml 由 `kimi login` 生成——未登录时它不存在，接线无处可写。这不是错误，
        // 而是「需要先登录」，据此给前端精准提示而非泛化的失败文案。
        let Ok(text) = std::fs::read_to_string(&cfg) else {
            eprintln!("Meowo repair[kimi]: config.toml 读取失败（缺失或不可读，需先完成 kimi login），跳过");
            return Some(RepairReason::NeedLogin);
        };

        // reporter 路径：复用配置里已认领的当前 meowo-reporter → 否则 app 同目录的 sidecar。
        // 历史 cc-reporter 路径不算数（claimed_reporter 已排除）：把它当目标写回去 hooks 仍然失效。
        let Some(reporter) = inst.hooks.claimed_reporter(&text, "kimi").or_else(super::sibling_reporter) else {
            eprintln!("Meowo repair[kimi]: 找不到 meowo-reporter 二进制（既有 hooks 无有效 meowo 路径且 app 同目录无 sidecar），无法接线");
            return Some(RepairReason::ReporterNotFound);
        };

        match inst.hooks.ensure_hooks(&text, &reporter, "kimi") {
            EnsureOutcome::Changed(next) => {
                super::backup_once(&cfg);
                match crate::fsutil::write_atomic(&cfg, &next) {
                    Ok(_) => {
                        eprintln!("Meowo repair[kimi]: 已写入 hooks 到 {}（变体 {}）", cfg.display(), inst.variant_tag);
                        None
                    }
                    Err(e) => {
                        eprintln!("Meowo repair[kimi]: config.toml 写入失败（{e}）");
                        Some(RepairReason::WriteFailed)
                    }
                }
            }
            EnsureOutcome::Unchanged => {
                eprintln!("Meowo repair[kimi]: config.toml 已是目标状态，无需改动");
                None
            }
            EnsureOutcome::Abandon(reason) => {
                eprintln!("Meowo repair[kimi]: config.toml 形态无法安全改写（{reason:?}），放弃（绝不写坏）");
                Some(reason)
            }
        }
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
