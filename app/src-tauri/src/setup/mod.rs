//! provider 自动接线：启动时对检测到已安装的 AI CLI 幂等挂上 meowo-reporter hooks。
//! 组织仿 account/（trait + 静态注册表）。合并逻辑保持纯函数（不依赖 Tauri/app 状态），
//! 为后续 `meowo-reporter setup` 子命令跨 crate 迁移铺路。
pub mod claude;
pub mod codex;
pub mod kimi;

/// 接线失败原因住在 `meowo-agent`（与 hooks 格式适配器同层，供 reporter/app 共用）。
pub use meowo_agent::config::RepairReason;

use meowo_agent::{EnsureOutcome, Installation, MissingConfig};

/// 配置落盘之后的 agent 专属副作用（codex 的 trusted_hash 预信任）。入参是**实际写出的**配置文本。
/// 返回 `Some(reason)` 会让整个接线报失败——只有当该步骤失败真的导致 hooks 不生效时才这么做。
pub(crate) type AfterWrite = fn(&Installation, &str) -> Option<RepairReason>;

/// 落盘**之前**对配置文本的 agent 专属改写（claude 的 statusLine——它与 hooks 同住 settings.json，
/// 无法靠 `AfterWrite` 表达）。入参是 hooks 合并后的文本与已解析的 reporter 路径。
///
/// 约定：**无改动时必须原样返回入参文本**。返回一个语义等价但重新序列化过的字符串会让下面的
/// 幂等判定误判为「有改动」，于是每次启动都重写一遍用户配置。
pub(crate) type Amend = fn(&Installation, &str, &str) -> Result<String, RepairReason>;

/// 三家 agent 共用的接线编排：
/// 读配置 → 格式适配器合并 hooks → `amend` 写前改写 → 备份 → 原子写 → `after` 副作用。
/// 三个「绝不」在此集中兑现：解析失败绝不写、写前必备份、一律原子写。
pub(crate) fn wire_hooks(
    inst: &Installation,
    agent_id: &str,
    amend: Option<Amend>,
    after: Option<AfterWrite>,
) -> Option<RepairReason> {
    let path = inst.config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match inst.hooks.missing {
            MissingConfig::CreateFrom(seed) => seed.to_string(),
            MissingConfig::Fail(reason) => {
                eprintln!("Meowo repair[{agent_id}]: {} 不存在（{reason:?}），跳过", path.display());
                return Some(reason);
            }
        },
        // 文件存在但读不了（权限、非 UTF-8 编码如 UTF-16）：绝不当「不存在」处理，
        // 否则会拿初始模板覆盖用户文件。
        Err(e) => {
            eprintln!("Meowo repair[{agent_id}]: {} 读取失败（{e}），跳过", path.display());
            return Some(RepairReason::ConfigUnreadable);
        }
    };

    // reporter 路径：复用配置里已认领的当前 meowo-reporter → 否则 app 同目录的 sidecar。
    // 历史 cc-reporter 路径不算数（claimed_reporter 已排除）：把它当目标写回去 hooks 仍然失效。
    // 已认领的路径还须**当前仍存在**：app 换了目录后 hooks 里残留的旧路径若被当成目标写回去，
    // hooks 会静默失效（而 sidecar 明明就在手边）。
    let claimed = inst.hooks.claimed_reporter(&text, agent_id).filter(|p| std::path::Path::new(p).exists());
    let Some(reporter) = claimed.or_else(sibling_reporter) else {
        eprintln!("Meowo repair[{agent_id}]: 找不到 meowo-reporter 二进制（既有 hooks 无有效 meowo 路径且 app 同目录无 sidecar），无法接线");
        return Some(RepairReason::ReporterNotFound);
    };

    let merged = match inst.hooks.ensure_hooks(&text, &reporter, agent_id) {
        EnsureOutcome::Changed(next) => next,
        // 尚不能就此收工：hooks 已是目标态，statusLine 之类的 amend 目标可能仍需改动。
        EnsureOutcome::Unchanged => text.clone(),
        EnsureOutcome::Abandon(reason) => {
            eprintln!("Meowo repair[{agent_id}]: {} 形态无法安全改写（{reason:?}），放弃（绝不写坏）", path.display());
            return Some(reason);
        }
    };

    let next = match amend {
        Some(f) => match f(inst, &merged, &reporter) {
            Ok(t) => t,
            Err(reason) => {
                eprintln!("Meowo repair[{agent_id}]: {} 写前改写失败（{reason:?}），放弃", path.display());
                return Some(reason);
            }
        },
        None => merged,
    };

    // 幂等判定放在 amend **之后**、与最初读到的文本比对：hooks 与 statusLine 任一需改动就要落盘。
    // （曾经只看 hooks 的合并结果，于是 hooks 已就位、statusLine 待接时被误报「已是目标状态」。）
    let written = if next == text {
        eprintln!("Meowo repair[{agent_id}]: {} 已是目标状态，无需改动", path.display());
        text
    } else {
        if path.exists() {
            backup_once(&path);
        }
        if let Err(e) = crate::fsutil::write_atomic(&path, &next) {
            eprintln!("Meowo repair[{agent_id}]: {} 写入失败（{e}）", path.display());
            return Some(RepairReason::WriteFailed);
        }
        eprintln!("Meowo repair[{agent_id}]: 已写入 {}（变体 {}）", path.display(), inst.variant_tag);
        next
    };

    after.and_then(|f| f(inst, &written))
}

/// Provider 接线抽象。Sync：以 &'static dyn 静态注册表共享。
pub trait ProviderSetup: Sync {
    fn key(&self) -> meowo_store::ProviderKey;
    /// 数据目录存在即视为已安装（各自尊重 env 覆盖）。不存在 → apply_all 跳过。
    fn detect(&self) -> bool;
    /// 幂等接线。全程 best-effort：绝不 panic。返回 `None` = 成功/已是目标状态；
    /// `Some(reason)` = 无法接线（供「修复连接」把原因回传前端）。
    fn apply(&self) -> Option<RepairReason>;
}

static CLAUDE_SETUP: claude::ClaudeSetup = claude::ClaudeSetup;
static CODEX_SETUP: codex::CodexSetup = codex::CodexSetup;
static KIMI_SETUP: kimi::KimiSetup = kimi::KimiSetup;
static ALL_SETUP: &[&dyn ProviderSetup] = &[&CLAUDE_SETUP, &CODEX_SETUP, &KIMI_SETUP];

/// 启动后台线程入口：逐 provider 独立 best-effort，一家失败不影响他家。
pub fn apply_all() {
    for s in ALL_SETUP {
        if s.detect() {
            let _ = s.apply();
        }
    }
}

/// 对指定 provider 强制执行一次接线（不管 detect 结果）。用于用户手动点击「修复连接」。
/// 返回是否成功 apply（true = 数据目录存在并已尝试写入；false = 未安装/找不到 reporter）。
/// 返回 `None` = 已接线/已是目标状态；`Some(reason)` = 未能接线及原因（供前端提示）。
pub fn apply_provider(key: meowo_store::ProviderKey) -> Option<RepairReason> {
    let Some(s) = ALL_SETUP.iter().find(|s| s.key() == key) else {
        eprintln!("Meowo repair[{key:?}]: 无对应 ProviderSetup，跳过");
        return Some(RepairReason::NotDetected);
    };
    if !s.detect() {
        eprintln!("Meowo repair[{key:?}]: detect()=false（数据目录不存在，视为未安装），跳过接线");
        return Some(RepairReason::NotDetected);
    }
    s.apply()
}

/// app 可执行同目录的 meowo-reporter（打包态 sidecar 与 app 放一起）。
pub(crate) fn sibling_reporter() -> Option<String> {
    let bin = if cfg!(windows) { "meowo-reporter.exe" } else { "meowo-reporter" };
    let exe = std::env::current_exe().ok()?;
    let sib = exe.with_file_name(bin);
    sib.exists().then(|| sib.to_string_lossy().into_owned())
}

/// 备份一次：`<文件名>.cckb-bak` 不存在时 copy。保留最初的用户原始配置。
pub(crate) fn backup_once(path: &std::path::Path) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let bak = path.with_file_name(format!("{name}.cckb-bak"));
    if !bak.exists() {
        let _ = std::fs::copy(path, &bak);
    }
}

