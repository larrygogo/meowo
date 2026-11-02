//! 「在编辑器中打开」项目目录（对话窗标题栏的分体按钮，对照 Kimi Code 标题栏的「用 VS Code 打开」）。
//!
//! 文件面板的「打开方式」按文件关联走（openwith.rs）；目录没有关联可查，这里按已知编辑器的
//! 安装位置探测。系统文件管理器那一项前端恒有（走 open_project_dir），不在此表。
//! 探测与拉起都是文件 IO / 进程 spawn，照 Command 线程纪律放 spawn_blocking。

use meowo_protocol::ipc::FileOpenerDto;
use std::path::{Path, PathBuf};

/// (id, 展示名)。顺序即菜单顺序。
const FOLDER_EDITORS: [(&str, &str); 3] = [
    ("vscode", "VS Code"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
];

/// 编辑器的启动载体：Windows 是 Electron 主程序（GUI 子系统，直接带目录参数拉起不弹控制台；
/// 不走 PATH 上的 code.cmd——那会先起一个 cmd 窗口）；macOS 是 .app（`open -a`）；其余查 PATH。
#[cfg(target_os = "windows")]
fn locate(id: &str) -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let program = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let candidates: Vec<PathBuf> = match id {
        // 用户级安装（默认）与系统级安装两处。
        "vscode" => [
            local
                .as_ref()
                .map(|base| base.join(r"Programs\Microsoft VS Code\Code.exe")),
            program
                .as_ref()
                .map(|base| base.join(r"Microsoft VS Code\Code.exe")),
        ]
        .into_iter()
        .flatten()
        .collect(),
        "cursor" => local
            .iter()
            .map(|base| base.join(r"Programs\cursor\Cursor.exe"))
            .collect(),
        "windsurf" => local
            .iter()
            .map(|base| base.join(r"Programs\Windsurf\Windsurf.exe"))
            .collect(),
        _ => Vec::new(),
    };
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(target_os = "macos")]
fn locate(id: &str) -> Option<PathBuf> {
    let app = match id {
        "vscode" => "/Applications/Visual Studio Code.app",
        "cursor" => "/Applications/Cursor.app",
        "windsurf" => "/Applications/Windsurf.app",
        _ => return None,
    };
    let path = PathBuf::from(app);
    path.is_dir().then_some(path)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn locate(id: &str) -> Option<PathBuf> {
    let exe = match id {
        "vscode" => "code",
        "cursor" => "cursor",
        "windsurf" => "windsurf",
        _ => return None,
    };
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe))
        .find(|path| path.is_file())
}

fn icon_for(path: &Path) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        crate::openwith::exe_icon_data_url(path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        None
    }
}

/// 本机装了哪些能打开目录的编辑器（带图标，Windows 从 exe 里取）。
#[tauri::command]
pub(crate) async fn list_folder_editors() -> Result<Vec<FileOpenerDto>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        FOLDER_EDITORS
            .iter()
            .filter_map(|(id, name)| {
                let path = locate(id)?;
                Some(FileOpenerDto {
                    id: (*id).to_string(),
                    name: (*name).to_string(),
                    icon: icon_for(&path),
                })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// 用指定编辑器打开会话目录。目录作为独立 argv 传入、不经 shell，无注入面；拉起即返回。
#[tauri::command]
pub(crate) async fn open_folder_in(cwd: String, editor: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_folder_blocking(&cwd, &editor))
        .await
        .map_err(|e| e.to_string())?
}

fn open_folder_blocking(cwd: &str, editor: &str) -> Result<(), String> {
    let dir = cwd.trim();
    if dir.is_empty() || !Path::new(dir).is_dir() {
        return Err("editor/dir_missing".into());
    }
    let app = locate(editor).ok_or_else(|| "editor/not_found".to_string())?;
    #[cfg(target_os = "windows")]
    {
        // 正斜杠路径交给 Electron 也认，但工作区标题会显示成 C:/…，与资源管理器不一致。
        std::process::Command::new(&app)
            .arg(dir.replace('/', "\\"))
            .spawn()
            .map_err(|e| format!("editor/spawn_failed：{e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        crate::fsutil::spawn_detached(
            std::process::Command::new("open")
                .arg("-a")
                .arg(&app)
                .arg(dir),
        )
        .map_err(|e| format!("editor/spawn_failed：{e}"))?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        crate::fsutil::spawn_detached(std::process::Command::new(&app).arg(dir))
            .map_err(|e| format!("editor/spawn_failed：{e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 目录不存在或编辑器不认识时给结构化原因() {
        assert_eq!(
            open_folder_blocking("C:/definitely/not/here/xyz", "vscode").unwrap_err(),
            "editor/dir_missing"
        );
        let here = std::env::current_dir().unwrap();
        assert_eq!(
            open_folder_blocking(&here.to_string_lossy(), "notepad-plus-plus").unwrap_err(),
            "editor/not_found"
        );
    }
}
