//! 应用品牌改名后的安装包迁移。
//!
//! macOS 的 Tauri updater 会把新包内容原地覆盖到“当前正在运行的 `.app` 路径”，不会采用更新包
//! 内部的新目录名。因此从旧版升级时，内部已经是 Meowo，Finder 里却仍会留下
//! `/Applications/cc-kanban.app`。这里在新版第一次启动时把外层 bundle 原子改名并从新路径重启。

use std::path::{Path, PathBuf};

const LEGACY_BUNDLE_NAME: &str = "cc-kanban.app";
const CURRENT_BUNDLE_NAME: &str = "Meowo.app";

#[derive(Debug, PartialEq, Eq)]
enum RenameOutcome {
    NotLegacy,
    Renamed { old: PathBuf, new: PathBuf },
    Skipped(String),
}

/// 从 `…/<name>.app/Contents/MacOS/<exe>` 严格定位外层 bundle；开发态 target/debug 不匹配。
fn legacy_bundle_paths(executable: &Path) -> Option<(PathBuf, PathBuf)> {
    let macos = executable.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let bundle = contents.parent()?;
    if bundle.file_name()?.to_str()? != LEGACY_BUNDLE_NAME {
        return None;
    }
    let target = bundle.parent()?.join(CURRENT_BUNDLE_NAME);
    Some((bundle.to_path_buf(), target))
}

fn rename_legacy_bundle(executable: &Path) -> RenameOutcome {
    let Some((old, new)) = legacy_bundle_paths(executable) else {
        return RenameOutcome::NotLegacy;
    };
    // 两份 app 都存在时绝不猜哪份该覆盖；保留现场，让用户可见地处理冲突。
    if new.exists() {
        return RenameOutcome::Skipped(format!(
            "目标 {} 已存在，保留旧 bundle，不自动覆盖",
            new.display()
        ));
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => RenameOutcome::Renamed { old, new },
        Err(error) => RenameOutcome::Skipped(format!(
            "无法把 {} 重命名为 {}：{error}",
            old.display(),
            new.display()
        )),
    }
}

/// 若当前正从旧名字的 macOS bundle 启动，改名后通过 LaunchServices 从新路径拉起。
///
/// 返回 true 表示新实例已经交给 `open`，调用方应立即结束当前启动，避免同一版本跑两份。
#[cfg(target_os = "macos")]
pub(crate) fn migrate_legacy_bundle_and_relaunch() -> bool {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("[app-migration] 无法读取当前程序路径：{error}");
            return false;
        }
    };
    match rename_legacy_bundle(&executable) {
        RenameOutcome::NotLegacy => false,
        RenameOutcome::Skipped(reason) => {
            eprintln!("[app-migration] {reason}");
            false
        }
        RenameOutcome::Renamed { old, new } => {
            let launched = std::process::Command::new("/usr/bin/open")
                .arg("-n")
                .arg(&new)
                .status()
                .is_ok_and(|status| status.success());
            if launched {
                eprintln!(
                    "[app-migration] 已把 {} 迁移为 {}，从新路径重启",
                    old.display(),
                    new.display()
                );
                return true;
            }

            // 新实例没拉起来就恢复旧路径；否则后续插件仍按启动时的旧 current_exe 初始化，会因
            // 路径消失而令本次启动失败，用户反而两个名字都打不开。
            match std::fs::rename(&new, &old) {
                Ok(()) => eprintln!("[app-migration] 从新路径重启失败，已恢复 {}", old.display()),
                Err(error) => eprintln!(
                    "[app-migration] 从新路径重启失败，且无法恢复 {}：{error}",
                    old.display()
                ),
            }
            false
        }
    }
}

/// 旧版若开启过“登录时启动”，会留下指向旧 bundle 的 `cc-kanban.plist`。新路径启动成功后，
/// 先创建 Meowo 的 LaunchAgent，再删除旧项；创建失败时保留旧项，避免静默丢掉用户设置。
#[cfg(target_os = "macos")]
pub(crate) fn migrate_legacy_autostart(app: &tauri::AppHandle) {
    use tauri_plugin_autostart::ManagerExt;

    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let old = PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join("cc-kanban.plist");
    if !old.exists() {
        return;
    }
    if let Err(error) = app.autolaunch().enable() {
        eprintln!("[app-migration] 刷新登录自启路径失败，保留旧项：{error}");
        return;
    }
    if let Err(error) = std::fs::remove_file(&old) {
        eprintln!(
            "[app-migration] 新登录自启项已建立，但清理 {} 失败：{error}",
            old.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "meowo-app-bundle-{name}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn only_recognizes_the_exact_legacy_macos_bundle_shape() {
        let old = Path::new("/Applications/cc-kanban.app/Contents/MacOS/cc-kanban");
        assert_eq!(
            legacy_bundle_paths(old),
            Some((
                PathBuf::from("/Applications/cc-kanban.app"),
                PathBuf::from("/Applications/Meowo.app")
            ))
        );
        assert!(
            legacy_bundle_paths(Path::new("/Applications/Meowo.app/Contents/MacOS/Meowo"))
                .is_none()
        );
        assert!(legacy_bundle_paths(Path::new("target/debug/meowo-app")).is_none());
    }

    #[test]
    fn renames_bundle_without_touching_its_contents() {
        let root = temp_dir("rename");
        let executable = root
            .join(LEGACY_BUNDLE_NAME)
            .join("Contents")
            .join("MacOS")
            .join("Meowo");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"binary").unwrap();

        let expected = root.join(CURRENT_BUNDLE_NAME);
        assert_eq!(
            rename_legacy_bundle(&executable),
            RenameOutcome::Renamed {
                old: root.join(LEGACY_BUNDLE_NAME),
                new: expected.clone()
            }
        );
        assert_eq!(
            std::fs::read(expected.join("Contents/MacOS/Meowo")).unwrap(),
            b"binary"
        );
        assert!(!root.join(LEGACY_BUNDLE_NAME).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn never_overwrites_an_existing_meowo_bundle() {
        let root = temp_dir("conflict");
        let executable = root
            .join(LEGACY_BUNDLE_NAME)
            .join("Contents")
            .join("MacOS")
            .join("Meowo");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::create_dir(root.join(CURRENT_BUNDLE_NAME)).unwrap();

        assert!(matches!(
            rename_legacy_bundle(&executable),
            RenameOutcome::Skipped(_)
        ));
        assert!(root.join(LEGACY_BUNDLE_NAME).exists());
        assert!(root.join(CURRENT_BUNDLE_NAME).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
