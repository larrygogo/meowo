//! kimi 插件。两个变体——这正是变体层存在的理由：
//!
//! | tag | 发行 | 数据目录 | hooks 格式 | 可执行 |
//! |---|---|---|---|---|
//! | `modern` | Node 版 **Kimi Code** | `~/.kimi-code` | `[[hooks]]`（默认无 hooks 键） | `<data>/bin/kimi` |
//! | `legacy` | 旧 Python 版 **kimi-cli** | `~/.kimi` | `[[hooks]]`（默认 `hooks = []` 空内联数组） | `~/.local/bin/kimi` 等 |
//!
//! 两者的 hook 配置格式与 hook stdin 载荷（session_id/cwd/hook_event_name）实测一致，故共用
//! [`ConfigFormat::KimiToml`]——差的只是目录与「空内联数组」这一形态，都已在声明里表达。

use crate::{
    auth::AuthScheme,
    config::ConfigFormat,
    id::{self, AgentId},
    registry::AgentPlugin,
    variant::{DataDirSpec, ExeSpec, Variant},
};

/// 来源：kimi-code 开源包 `packages/oauth/src/constants.ts`。
const AUTH_MODERN: AuthScheme = AuthScheme {
    credentials_rel: "credentials/kimi-code.json",
    token_url: "https://auth.kimi.com/api/oauth/token",
    client_id: "17e5f671-d194-4dfb-9706-5516cb48c098",
    default_base_url: "https://api.kimi.com/coding/v1",
};

/// 旧 Python 版的凭据布局与新版相同（实测 `~/.kimi/credentials/kimi-code.json` 字段一致）。
/// **client_id 未经证实**：若刷新 token 返回 `invalid_client`，就把这里换成旧版的值——
/// 变体层的意义正在于此，届时只改这一个 const，account 侧无需再动。
const AUTH_LEGACY: AuthScheme = AUTH_MODERN;

static VARIANTS: [Variant; 2] = [
    Variant {
        tag: "modern",
        data_dir: DataDirSpec { env: Some("KIMI_SHARE_DIR"), candidates: &[".kimi-code"] },
        config: ConfigFormat::KimiToml,
        auth: Some(&AUTH_MODERN),
        exe: ExeSpec { stem: "kimi", in_data: &["bin"], in_home: &[".kimi-code/bin"] },
    },
    Variant {
        tag: "legacy",
        data_dir: DataDirSpec { env: Some("KIMI_SHARE_DIR"), candidates: &[".kimi"] },
        config: ConfigFormat::KimiToml,
        auth: Some(&AUTH_LEGACY),
        // 旧版常经 uv/pipx 装到 ~/.local/bin，不在数据目录下。
        exe: ExeSpec { stem: "kimi", in_data: &["bin"], in_home: &[".kimi/bin", ".local/bin"] },
    },
];

pub struct Kimi;

impl AgentPlugin for Kimi {
    fn id(&self) -> AgentId {
        id::KIMI
    }
    fn display_name(&self) -> &'static str {
        "Kimi Code"
    }
    fn variants(&self) -> &'static [Variant] {
        &VARIANTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::Installation;
    use std::path::Path;

    /// 直接对变体表 probe（不碰真实 home，也不读 env）。
    fn probe_at(home: &Path) -> Option<Installation> {
        VARIANTS.iter().find_map(|v| {
            let dir = v.data_dir.candidates.iter().map(|c| home.join(c)).find(|p| p.is_dir())?;
            Some(v.installation_at(id::KIMI, dir, Some(home)))
        })
    }

    #[test]
    fn prefers_modern_then_legacy() {
        let home = std::env::temp_dir().join(format!("meowo-kimi-variants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);

        // 都不存在 → 未安装。
        std::fs::create_dir_all(&home).unwrap();
        assert!(probe_at(&home).is_none());

        // 只有旧版 → legacy 命中，配置路径落在 ~/.kimi/config.toml。
        std::fs::create_dir_all(home.join(".kimi")).unwrap();
        let legacy = probe_at(&home).expect("legacy 应命中");
        assert_eq!(legacy.variant_tag, "legacy");
        assert_eq!(legacy.config_path(), home.join(".kimi").join("config.toml"));
        assert_eq!(
            legacy.credentials_path(),
            Some(home.join(".kimi").join("credentials").join("kimi-code.json"))
        );

        // 新版出现 → 抢先。
        std::fs::create_dir_all(home.join(".kimi-code")).unwrap();
        let modern = probe_at(&home).expect("modern 应命中");
        assert_eq!(modern.variant_tag, "modern");
        assert_eq!(modern.data_dir, home.join(".kimi-code"));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn exe_falls_back_to_bare_name_when_not_found() {
        let home = std::env::temp_dir().join(format!("meowo-kimi-exe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".kimi")).unwrap();

        let inst = probe_at(&home).unwrap();
        assert_eq!(inst.exe, None);
        assert_eq!(inst.exe_command(), "kimi"); // 回退裸名走 PATH

        // 旧版真实落点：~/.local/bin/kimi[.exe]
        let local_bin = home.join(".local").join("bin");
        std::fs::create_dir_all(&local_bin).unwrap();
        let exe = local_bin.join(crate::exe_file_name("kimi"));
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(probe_at(&home).unwrap().exe.as_deref(), Some(exe.as_path()));

        let _ = std::fs::remove_dir_all(&home);
    }
}
