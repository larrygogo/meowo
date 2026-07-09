//! provider 自动接线：启动时对检测到已安装的 AI CLI 幂等挂上 meowo-reporter hooks。
//! 组织仿 account/（trait + 静态注册表）。合并逻辑保持纯函数（不依赖 Tauri/app 状态），
//! 为后续 `meowo-reporter setup` 子命令跨 crate 迁移铺路。
pub mod claude;
pub mod codex;
pub mod kimi;

/// 接线失败原因住在 `meowo-agent`（与 hooks 格式适配器同层，供 reporter/app 共用）。
pub use meowo_agent::config::RepairReason;

/// codex 尚未迁入插件层，暂在此保留其命令认领规则（`"<exe>" --provider codex` 形态）。
/// 迁入后由 `Variant.hooks.command.claim` 取代（见 rollout 计划 Phase B）。
pub(crate) fn claim_provider_cmd(cmd: &str, provider: &str) -> Option<String> {
    const CODEX_SHAPE: meowo_agent::CommandSpec =
        meowo_agent::CommandSpec { quote_exe: true, with_provider: true };
    CODEX_SHAPE.claim(cmd, provider)
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

