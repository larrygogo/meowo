//! provider 自动接线：启动时对检测到已安装的 AI CLI 幂等挂上 meowo-reporter hooks。
//! 组织仿 account/（trait + 静态注册表）。合并逻辑保持纯函数（不依赖 Tauri/app 状态），
//! 为后续 `meowo-reporter setup` 子命令跨 crate 迁移铺路。
pub mod claude;
pub mod codex;
pub mod kimi;

/// Provider 接线抽象。Sync：以 &'static dyn 静态注册表共享。
pub trait ProviderSetup: Sync {
    fn key(&self) -> meowo_store::ProviderKey;
    /// 数据目录存在即视为已安装（各自尊重 env 覆盖）。不存在 → apply_all 跳过。
    fn detect(&self) -> bool;
    /// 幂等接线。全程 best-effort：读不到/解析失败/找不到 reporter 均静默返回，绝不 panic。
    fn apply(&self);
}

static CLAUDE_SETUP: claude::ClaudeSetup = claude::ClaudeSetup;
static CODEX_SETUP: codex::CodexSetup = codex::CodexSetup;
static KIMI_SETUP: kimi::KimiSetup = kimi::KimiSetup;
static ALL_SETUP: &[&dyn ProviderSetup] = &[&CLAUDE_SETUP, &CODEX_SETUP, &KIMI_SETUP];

/// 启动后台线程入口：逐 provider 独立 best-effort，一家失败不影响他家。
pub fn apply_all() {
    for s in ALL_SETUP {
        if s.detect() {
            s.apply();
        }
    }
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

/// 解析 hook command 为（可执行路径, 余参）。首 token 支持带双引号或裸路径。
pub(crate) fn parse_hook_command(cmd: &str) -> Option<(String, Vec<String>)> {
    let c = cmd.trim();
    let (path, rest) = if let Some(r) = c.strip_prefix('"') {
        let end = r.find('"')?;
        (r[..end].to_string(), r[end + 1..].trim())
    } else {
        match c.split_once(char::is_whitespace) {
            Some((p, r)) => (p.to_string(), r.trim()),
            None => (c.to_string(), ""),
        }
    };
    let args = rest.split_whitespace().map(str::to_string).collect();
    Some((path, args))
}

/// 严格认领带 provider 参数的命令（codex/kimi 形态）：可执行文件名恰为 meowo-reporter[.exe]
/// 且余参恰为 ["--provider", provider]。返回可执行路径。不裸 contains，不误伤用户 hook。
pub(crate) fn claim_provider_cmd(cmd: &str, provider: &str) -> Option<String> {
    let (path, args) = parse_hook_command(cmd)?;
    let name = std::path::Path::new(&path).file_name()?.to_str()?;
    let is_reporter =
        name.eq_ignore_ascii_case("meowo-reporter") || name.eq_ignore_ascii_case("meowo-reporter.exe");
    (is_reporter && args == ["--provider", provider]).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_provider_cmd_strict() {
        // 认领：带引号/裸路径两种形态。
        assert_eq!(
            claim_provider_cmd("\"C:/x/meowo-reporter.exe\" --provider codex", "codex").as_deref(),
            Some("C:/x/meowo-reporter.exe")
        );
        assert_eq!(
            claim_provider_cmd("C:/x/meowo-reporter.exe --provider kimi", "kimi").as_deref(),
            Some("C:/x/meowo-reporter.exe")
        );
        // 拒绝：provider 不符 / 无参数 / 多余参数 / 别的可执行 / 子串陷阱。
        assert!(claim_provider_cmd("C:/x/meowo-reporter.exe --provider codex", "kimi").is_none());
        assert!(claim_provider_cmd("\"C:/x/meowo-reporter.exe\"", "codex").is_none());
        assert!(claim_provider_cmd("C:/x/meowo-reporter.exe --provider codex --v", "codex").is_none());
        assert!(claim_provider_cmd("node meowo-reporter-notify.js --provider codex", "codex").is_none());
    }
}
