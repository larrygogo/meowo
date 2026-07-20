//! 接线：把 meowo-reporter 的 hooks 幂等挂到各 agent 的配置文件里。
//!
//! 通用编排（读配置 → 格式适配器合并 hooks → 写前改写 → 备份 → 原子写 → 落盘后副作用）在此；
//! agent 专属的两步副作用经 [`WiringCap`] 能力槽声明——claude 要把 `statusLine` 包成先写库的脚本，
//! codex 要往 `config.toml` 写 trusted_hash 预信任。不声明该能力的 agent（kimi）走纯合并路径。
//!
//! 宿主只需提供两样东西：sidecar 的 meowo-reporter 路径、meowo 自己的数据目录。见 [`WiringContext`]。

use std::path::Path;

use crate::config::{EnsureOutcome, MissingConfig, RepairReason};
use crate::variant::Installation;

/// 接线时宿主提供的上下文。插件因此不需要知道 `db_path()` 之类的 app 知识。
pub struct WiringContext<'a> {
    /// 与当前 app 同版本的 reporter（app 同目录的 sidecar）。存在时必须优先于配置里的旧路径，
    /// 否则 GUI 已升级而 hooks 仍调用安装目录中的旧 reporter，新协议（如 GUI 审批）会静默失效。
    pub fallback_reporter: Option<&'a str>,
    /// meowo 自己的数据目录（`~/.meowo`）。claude 的 statusLine 包装脚本落在这里。
    pub meowo_dir: &'a Path,
}

/// agent 专属的接线副作用。两个方法都有默认实现——只覆写自己需要的那个。
pub trait WiringCap: Sync {
    /// 落盘**之前**对配置文本的改写（claude 的 statusLine——它与 hooks 同住 settings.json，
    /// 无法靠 `after_write` 表达）。入参是 hooks 合并后的文本与已解析的 reporter 路径。
    ///
    /// **约定：无改动时必须原样返回入参文本。** 返回一个语义等价但重新序列化过的字符串，会让
    /// 下面的幂等判定误判为「有改动」，于是每次启动都重写一遍用户配置。
    fn amend(
        &self,
        _inst: &Installation,
        text: &str,
        _ctx: &WiringContext,
        _reporter: &str,
    ) -> Result<String, RepairReason> {
        Ok(text.to_string())
    }

    /// 配置落盘**之后**的副作用（codex 的 trusted_hash 预信任）。入参是**实际写出的**配置文本。
    /// 返回 `Some(reason)` 会让整个接线报失败——只有当该步骤失败真的导致 hooks 不生效时才这么做。
    fn after_write(&self, _inst: &Installation, _written: &str) -> Option<RepairReason> {
        None
    }
}

/// 备份一次：`<文件名>.cckb-bak` 不存在时 copy。保留最初的用户原始配置。
///
/// copy 失败（权限/磁盘满/杀软拦截）必须上抛，不许吞：「写前必备份」的意思是
/// **备份不了就不写**——吞掉错误照常落盘，连用户原始配置的最后一次留档机会也赔进去。
pub fn backup_once(path: &Path) -> std::io::Result<()> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    let bak = path.with_file_name(format!("{name}.cckb-bak"));
    if bak.exists() {
        return Ok(());
    }
    std::fs::copy(path, &bak).map(|_| ())
}

/// 通用接线编排。三个「绝不」在此集中兑现：解析失败绝不写、写前必备份、一律原子写。
///
/// 返回 `None` = 成功/已是目标状态；`Some(reason)` = 无法接线（供「修复连接」把原因回传前端）。
pub fn wire_hooks(
    inst: &Installation,
    agent_id: &str,
    cap: Option<&dyn WiringCap>,
    ctx: &WiringContext,
) -> Option<RepairReason> {
    let path = inst.config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match inst.hooks.missing {
            MissingConfig::CreateFrom(seed) => seed.to_string(),
            MissingConfig::Fail(reason) => {
                eprintln!(
                    "Meowo repair[{agent_id}]: {} 不存在（{reason:?}），跳过",
                    path.display()
                );
                return Some(reason);
            }
        },
        // 文件存在但读不了（权限、非 UTF-8 编码如 UTF-16）：绝不当「不存在」处理，
        // 否则会拿初始模板覆盖用户文件。
        Err(e) => {
            eprintln!(
                "Meowo repair[{agent_id}]: {} 读取失败（{e}），跳过",
                path.display()
            );
            return Some(RepairReason::ConfigUnreadable);
        }
    };

    // reporter 路径：优先使用当前 app 随包 sidecar，只有宿主没有 sidecar 时才复用配置里的 reporter。
    // 仅按“文件名是 meowo-reporter 且路径存在”无法判断协议版本；此前优先 claimed，导致开发版
    // GUI/升级后的 app 仍调用安装目录中的旧 reporter，PermissionRequest 永远到不了新 broker。
    // 历史 cc-reporter 路径不算数（claimed_reporter 已排除）：把它当目标写回去 hooks 仍然失效。
    // 已认领的路径还须**当前仍存在**：app 换了目录后 hooks 里残留的旧路径若被当成目标写回去，
    // hooks 会静默失效（而 sidecar 明明就在手边）。
    let claimed = inst
        .hooks
        .claimed_reporter(&text, agent_id)
        .filter(|p| Path::new(p).exists());
    let bundled = ctx
        .fallback_reporter
        .filter(|path| Path::new(path).exists())
        .map(str::to_string);
    let Some(reporter) = prefer_bundled_reporter(claimed, bundled) else {
        eprintln!("Meowo repair[{agent_id}]: 找不到 meowo-reporter 二进制（既有 hooks 无有效 meowo 路径且 app 同目录无 sidecar），无法接线");
        return Some(RepairReason::ReporterNotFound);
    };

    let merged = match inst.hooks.ensure_hooks(&text, &reporter, agent_id) {
        EnsureOutcome::Changed(next) => next,
        // 尚不能就此收工：hooks 已是目标态，statusLine 之类的 amend 目标可能仍需改动。
        EnsureOutcome::Unchanged => text.clone(),
        EnsureOutcome::Abandon(reason) => {
            eprintln!(
                "Meowo repair[{agent_id}]: {} 形态无法安全改写（{reason:?}），放弃（绝不写坏）",
                path.display()
            );
            return Some(reason);
        }
    };

    let next = match cap {
        Some(c) => match c.amend(inst, &merged, ctx, &reporter) {
            Ok(t) => t,
            Err(reason) => {
                eprintln!(
                    "Meowo repair[{agent_id}]: {} 写前改写失败（{reason:?}），放弃",
                    path.display()
                );
                return Some(reason);
            }
        },
        None => merged,
    };

    // 幂等判定放在 amend **之后**、与最初读到的文本比对：hooks 与 statusLine 任一需改动就要落盘。
    // （曾经只看 hooks 的合并结果，于是 hooks 已就位、statusLine 待接时被误报「已是目标状态」。）
    let written = if next == text {
        eprintln!(
            "Meowo repair[{agent_id}]: {} 已是目标状态，无需改动",
            path.display()
        );
        text
    } else {
        if path.exists() {
            // 备份失败 → 放弃写入：「写前必备份」不许退化成「没备份照写」。
            if let Err(e) = backup_once(&path) {
                eprintln!(
                    "Meowo repair[{agent_id}]: {} 备份失败（{e}），放弃写入",
                    path.display()
                );
                return Some(RepairReason::WriteFailed);
            }
        }
        if let Err(e) = crate::fsutil::write_atomic(&path, &next) {
            eprintln!(
                "Meowo repair[{agent_id}]: {} 写入失败（{e}）",
                path.display()
            );
            return Some(RepairReason::WriteFailed);
        }
        eprintln!(
            "Meowo repair[{agent_id}]: 已写入 {}（变体 {}）",
            path.display(),
            inst.variant_tag
        );
        next
    };

    cap.and_then(|c| c.after_write(inst, &written))
}

fn prefer_bundled_reporter(claimed: Option<String>, bundled: Option<String>) -> Option<String> {
    bundled.or(claimed)
}

#[cfg(test)]
mod tests {
    use super::{backup_once, prefer_bundled_reporter};

    #[test]
    fn bundled_reporter_replaces_an_existing_but_stale_hook_binary() {
        assert_eq!(
            prefer_bundled_reporter(
                Some("C:/Users/me/AppData/Local/Meowo/meowo-reporter.exe".into()),
                Some("C:/workspace/target/debug/meowo-reporter.exe".into()),
            )
            .as_deref(),
            Some("C:/workspace/target/debug/meowo-reporter.exe")
        );
        assert_eq!(
            prefer_bundled_reporter(Some("/installed/meowo-reporter".into()), None).as_deref(),
            Some("/installed/meowo-reporter")
        );
    }

    /// 备份只在首次创建：第二次调用不得覆盖最初的用户原文（这正是 .cckb-bak 的意义）。
    #[test]
    fn backup_once_keeps_the_earliest_original() {
        let dir = std::env::temp_dir().join(format!("meowo-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("settings.json");

        std::fs::write(&cfg, "v1").unwrap();
        backup_once(&cfg).unwrap();
        std::fs::write(&cfg, "v2").unwrap();
        backup_once(&cfg).unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join("settings.json.cckb-bak")).unwrap(),
            "v1",
            "备份必须保留最初的用户原始配置"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 备份失败必须上抛（此前 `let _ = copy` 静默吞错，「写前必备份」成了空话）。
    /// `fs::copy` 对目录必然报错，用它制造一次逃不掉的备份失败。
    #[test]
    fn backup_failure_is_reported_not_swallowed() {
        let dir = std::env::temp_dir().join(format!("meowo-backup-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(backup_once(&dir).is_err(), "无法备份时必须返回 Err");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
