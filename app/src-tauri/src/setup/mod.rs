//! hooks 自动接线的**编排层**：启动时对检测到已安装的 agent 幂等挂上 meowo-reporter hooks。
//!
//! 合并逻辑在 `meowo_agent::config` 的格式适配器，通用编排在 `meowo_agent::wiring`，agent 专属的
//! 副作用（claude 的 statusLine、codex 的 trusted_hash）在各自的 `plugins/<id>/setup.rs`。
//!
//! 本模块只提供两样宿主才知道的事实：sidecar 的 meowo-reporter 在哪，meowo 的数据目录在哪。

pub use meowo_agent::config::RepairReason;

use meowo_agent::wiring::WiringContext;
use meowo_agent::AgentId;

/// app 可执行同目录的 meowo-reporter（打包态 sidecar 与 app 放一起）。
fn sibling_reporter() -> Option<String> {
    let bin = if cfg!(windows) { "meowo-reporter.exe" } else { "meowo-reporter" };
    let exe = std::env::current_exe().ok()?;
    let sib = exe.with_file_name(bin);
    sib.exists().then(|| sib.to_string_lossy().into_owned())
}

/// meowo 自己的数据目录（`~/.meowo`，board.db 与 statusline.sh 的所在）。
fn meowo_dir() -> std::path::PathBuf {
    crate::db_path().parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// 接线一个 agent。`reporter` 由调用方预解析，避免 apply_all 里逐个重复查 sidecar。
fn wire(plugin: &dyn meowo_agent::AgentPlugin, reporter: Option<&str>) -> Option<RepairReason> {
    let dir = meowo_dir();
    let ctx = WiringContext { fallback_reporter: reporter, meowo_dir: &dir };
    plugin.wire(&ctx)
}

/// 启动后台线程入口：逐 agent 独立 best-effort，一家失败不影响他家。
/// 未配置过的 agent（数据目录不存在＝没装）跳过，绝不凭空创建它的配置。
pub fn apply_all() {
    let reporter = sibling_reporter();
    for p in meowo_agent::all() {
        if p.is_configured() {
            let _ = wire(*p, reporter.as_deref());
        }
    }
}

/// 对指定 agent 强制执行一次接线（不管是否 configured）。用于用户手动点击「修复连接」。
/// 返回 `None` = 已接线/已是目标状态；`Some(reason)` = 未能接线及原因（供前端提示）。
pub fn apply_provider(id: AgentId) -> Option<RepairReason> {
    let Some(p) = meowo_agent::by_id(id.as_str()) else {
        eprintln!("Meowo repair[{id}]: 无对应插件，跳过");
        return Some(RepairReason::NotDetected);
    };
    wire(p, sibling_reporter().as_deref())
}
