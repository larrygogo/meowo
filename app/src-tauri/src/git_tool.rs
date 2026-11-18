//! 设置 › 代码托管 › Git：显示 Meowo 实际调用的 git 程序（路径、版本），读写全局提交身份
//! （`git config --global user.name / user.email`）。leader 2026-11-18：「把 git 工具和 git 平台都放在这里」，
//! 只做这两项，不做别的 git 配置。
//!
//! Meowo 起 git 一律是 `Command::new("git")`（git.rs、forge.rs、worktree.rs）；这里按标准库同样的查找顺序
//! 找出那一个，显示给用户。找不到时设置页说明影响（提交、推送、改动列表、PR 都用不了）并给下载链接。
//! 命令都是 async + spawn_blocking（app/src-tauri/CLAUDE.md 线程纪律：起子进程不进主线程）。

use meowo_protocol::ipc::GitToolDto;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 一次 git 调用的结果。spawn 失败（找不到程序）是 Err(原话)。
#[derive(Debug, Clone, Default)]
pub(crate) struct Out {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// 跑 git 的入口（测试换成假的或带 GIT_CONFIG_GLOBAL 的真 git）。
pub(crate) type Run<'a> = &'a dyn Fn(&[&str]) -> Result<Out, String>;

fn real_run(args: &[&str]) -> Result<Out, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).stdin(Stdio::null());
    // Windows：GUI 进程 spawn 控制台子进程会闪现 conhost 黑窗，压掉（同 git.rs）。
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    Ok(Out {
        code: out.status.code(),
        stdout: crate::fsutil::decode_text_bytes(&out.stdout)
            .trim()
            .to_string(),
        stderr: crate::fsutil::decode_text_bytes(&out.stderr)
            .trim()
            .to_string(),
    })
}

/// `Command::new("git")` 实际会起的那个程序，按 Rust 标准库的查找顺序（std `sys/pal/windows/process.rs` 的
/// `resolve_exe`，Rust 1.58 起）：
/// - Windows：名字没带扩展名只补 `.exe`（不看 PATHEXT，`git.cmd` / `git.bat` 不会被起）；依次找本程序所在目录、
///   系统目录（System32）、16 位系统目录（System）、Windows 目录，最后是 PATH。不找当前目录。
/// - 其他系统：execvp 的规矩，只在 PATH 里按顺序找。
///
/// dirs_before_path：PATH 之前先找的目录（Windows 上是上面四个，其他系统为空）。
pub(crate) fn resolve_program(
    name: &str,
    dirs_before_path: &[PathBuf],
    path_var: &str,
    windows: bool,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let file = if windows && Path::new(name).extension().is_none() {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    let sep = if windows { ';' } else { ':' };
    let from_path = path_var
        .split(sep)
        .map(|d| d.trim().trim_matches('"'))
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    dirs_before_path
        .iter()
        .cloned()
        .chain(from_path)
        .map(|dir| dir.join(&file))
        .find(|p| is_file(p))
}

fn real_path() -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    let mut before = Vec::new();
    if cfg!(windows) {
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            before.push(dir);
        }
        if let Some(root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
            before.extend([root.join("System32"), root.join("System"), root.clone()]);
        }
    }
    resolve_program("git", &before, &path, cfg!(windows), &|p| p.is_file())
}

/// `git version 2.47.1.windows.1` → `2.47.1.windows.1`。
pub(crate) fn parse_version(stdout: &str) -> Option<String> {
    let v = stdout
        .lines()
        .next()?
        .trim()
        .strip_prefix("git version ")?
        .trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// 读一项全局配置：退出码 1 = 没设（None），其他失败也按没设（界面显示为空，保存时会报原话）。
fn global(run: Run, key: &str) -> Option<String> {
    let out = run(&["config", "--global", "--get", key]).ok()?;
    (out.code == Some(0))
        .then_some(out.stdout)
        .filter(|v| !v.is_empty())
}

pub(crate) fn status_with(run: Run, path: Option<PathBuf>) -> GitToolDto {
    let mut dto = GitToolDto {
        path: path.map(|p| p.to_string_lossy().into_owned()),
        ..Default::default()
    };
    match run(&["--version"]) {
        Ok(out) if out.code == Some(0) => {
            dto.found = true;
            dto.version = parse_version(&out.stdout);
        }
        Ok(out) => {
            dto.error = Some(if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            });
            return dto;
        }
        Err(e) => {
            dto.error = Some(e);
            return dto;
        }
    }
    dto.user_name = global(run, "user.name");
    dto.user_email = global(run, "user.email");
    dto
}

/// 写前校验（「更改」面板提交时报 git/identity 的就地对话框也用它，forge.rs `set_identity_with`）：两项都要填（git 提交缺任何一项都会拒）；不能有换行（会写坏配置文件）；邮箱要像邮箱（有 @、没有空白
/// 与尖括号，git 会把尖括号当作身份的分隔符）。
pub(crate) fn validate(name: &str, email: &str) -> Result<(String, String), String> {
    let (name, email) = (name.trim(), email.trim());
    if name.is_empty() {
        return Err("git/name_empty".into());
    }
    if name.chars().any(|c| c.is_control())
        || name.contains(['<', '>'])
        || name.chars().count() > 200
    {
        return Err("git/name_invalid".into());
    }
    if email.is_empty() {
        return Err("git/email_empty".into());
    }
    let ok = email.chars().count() <= 254
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty());
    if !ok {
        return Err("git/email_invalid".into());
    }
    Ok((name.to_string(), email.to_string()))
}

/// 校验后写全局配置。git 拒了（配置文件被锁、没写权限）：`git/config_failed: <git 的原话>`。
pub(crate) fn set_with(run: Run, name: &str, email: &str) -> Result<(), String> {
    let (name, email) = validate(name, email)?;
    for (key, value) in [("user.name", name.as_str()), ("user.email", email.as_str())] {
        let out = run(&["config", "--global", key, value])
            .map_err(|e| format!("git/config_failed: {e}"))?;
        if out.code != Some(0) {
            let said = if out.stderr.is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            return Err(format!("git/config_failed: {said}"));
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn git_tool_status() -> Result<GitToolDto, String> {
    tauri::async_runtime::spawn_blocking(|| status_with(&real_run, real_path()))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn git_identity_set(name: String, email: String) -> Result<GitToolDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_with(&real_run, &name, &email)?;
        Ok(status_with(&real_run, real_path()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ok(stdout: &str) -> Result<Out, String> {
        Ok(Out {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        })
    }

    #[test]
    fn 版本号_从_git_version_里取() {
        assert_eq!(
            parse_version("git version 2.47.1.windows.1\n").as_deref(),
            Some("2.47.1.windows.1")
        );
        assert_eq!(
            parse_version("git version 2.39.5 (Apple Git-154)").as_deref(),
            Some("2.39.5 (Apple Git-154)")
        );
        assert_eq!(parse_version("usage: git"), None);
    }

    /// 与 `Command::new("git")` 一致：Windows 只认 git.exe（PATH 里排在前面的 git.cmd 不算），先找程序目录与系统目录
    /// 再找 PATH；其他系统只按 PATH 找不带扩展名的 git。
    #[test]
    fn 找的是_command_new_实际会起的那个_git() {
        let files = [
            "/a/git.cmd",
            "/b/git.exe",
            "/c/git",
            "/app/git.exe",
            "/win/System32/git.exe",
        ];
        let is = |p: &Path| files.iter().any(|f| Path::new(f) == p);
        let win = ["/a", "/b", "/c"].join(";");
        assert_eq!(
            resolve_program("git", &[], &win, true, &is),
            Some(PathBuf::from("/b/git.exe")),
            "不按 PATHEXT 认 git.cmd"
        );
        let before = [PathBuf::from("/app"), PathBuf::from("/win/System32")];
        assert_eq!(
            resolve_program("git", &before, &win, true, &is),
            Some(PathBuf::from("/app/git.exe")),
            "程序目录先于 PATH"
        );
        assert_eq!(
            resolve_program("git", &before[1..], &win, true, &is),
            Some(PathBuf::from("/win/System32/git.exe")),
            "系统目录先于 PATH"
        );
        assert_eq!(
            resolve_program("git", &[], "/a;\"/c\"", true, &is),
            None,
            "只有 git.cmd 和不带扩展名的 git：Command::new 起不来"
        );
        let unix = ["/a", "/c", "/b"].join(":");
        assert_eq!(
            resolve_program("git", &[], &unix, false, &is),
            Some(PathBuf::from("/c/git"))
        );
        assert_eq!(resolve_program("git", &[], "", false, &is), None);
    }

    /// 真的对照一次：找出来的路径要和 `Command::new("git")` 实际起的是同一个程序（版本输出一致）。
    #[test]
    fn 显示的路径与实际执行的是同一个_git() {
        let Some(path) = real_path() else { return };
        let version = |cmd: &mut Command| {
            cmd.arg("--version")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        assert_eq!(
            version(&mut Command::new(&path)),
            version(&mut Command::new("git")),
            "{}",
            path.display()
        );
    }

    #[test]
    fn 找不到_git_给原话_不再查身份() {
        let calls = RefCell::new(0);
        let run = |_: &[&str]| {
            *calls.borrow_mut() += 1;
            Err("program not found".to_string())
        };
        let dto = status_with(&run, None);
        assert!(!dto.found);
        assert_eq!(dto.error.as_deref(), Some("program not found"));
        assert_eq!(*calls.borrow(), 1, "找不到 git 就不再起进程读配置");
    }

    #[test]
    fn 读身份_没设的项为空() {
        let run = |args: &[&str]| match args {
            ["--version"] => ok("git version 2.47.1"),
            ["config", "--global", "--get", "user.name"] => ok("Larry"),
            _ => Ok(Out {
                code: Some(1),
                ..Default::default()
            }),
        };
        let dto = status_with(&run, Some(PathBuf::from("/usr/bin/git")));
        assert!(dto.found);
        assert_eq!(dto.path.as_deref(), Some("/usr/bin/git"));
        assert_eq!(dto.version.as_deref(), Some("2.47.1"));
        assert_eq!(dto.user_name.as_deref(), Some("Larry"));
        assert_eq!(dto.user_email, None);
    }

    #[test]
    fn 写前校验() {
        assert_eq!(
            validate(" Larry ", " a@b.c ").unwrap(),
            ("Larry".into(), "a@b.c".into())
        );
        assert_eq!(validate("", "a@b.c").unwrap_err(), "git/name_empty");
        assert_eq!(validate("a\nb", "a@b.c").unwrap_err(), "git/name_invalid");
        assert_eq!(validate("x <y>", "a@b.c").unwrap_err(), "git/name_invalid");
        assert_eq!(validate("x", "").unwrap_err(), "git/email_empty");
        for bad in ["ab.c", "@b.c", "a@", "a b@c", "<a@b>", "a@b\nc"] {
            assert_eq!(
                validate("x", bad).unwrap_err(),
                "git/email_invalid",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn 校验不过不起_git_写失败给原话() {
        let calls = RefCell::new(Vec::<String>::new());
        let run = |args: &[&str]| {
            calls.borrow_mut().push(args.join(" "));
            Ok(Out {
                code: Some(255),
                stderr:
                    "error: could not lock config file C:/Users/x/.gitconfig: Permission denied"
                        .into(),
                ..Default::default()
            })
        };
        assert_eq!(set_with(&run, "x", "bad").unwrap_err(), "git/email_invalid");
        assert!(calls.borrow().is_empty());
        assert_eq!(
            set_with(&run, "x", "a@b.c").unwrap_err(),
            "git/config_failed: error: could not lock config file C:/Users/x/.gitconfig: Permission denied"
        );
        assert_eq!(
            calls.borrow().as_slice(),
            ["config --global user.name x"],
            "第一项失败就停"
        );
    }

    /// 真 git：GIT_CONFIG_GLOBAL 指到临时文件（git ≥ 2.32），不碰用户的 ~/.gitconfig。写进去再读回来。
    #[test]
    fn 真_git_写进全局配置再读回() {
        let dir = std::env::temp_dir().join(format!("meowo-git-tool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("gitconfig");
        let _ = std::fs::remove_file(&file);
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .env("GIT_CONFIG_GLOBAL", &file)
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())?;
            Ok(Out {
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).trim().to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            })
        };
        let before = status_with(&run, None);
        assert!(before.found, "开发机上要有 git：{:?}", before.error);
        assert_eq!(
            (before.user_name.as_deref(), before.user_email.as_deref()),
            (None, None)
        );
        set_with(&run, "Meowo 测试", "test@example.com").unwrap();
        let after = status_with(&run, None);
        assert_eq!(after.user_name.as_deref(), Some("Meowo 测试"));
        assert_eq!(after.user_email.as_deref(), Some("test@example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
