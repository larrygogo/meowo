//! 终端页签里手打 `claude` 跟随 Meowo 当前选中的 Claude 账号。照 Orca（stablyai/orca c1bd89d）的
//! `claude-shell-function.ts`：2026-11-27 leader 决定多账号完全照 Orca。
//!
//! 做法：Meowo 把当前账号的数据目录写进一个指针文件（默认账号写空），终端启动时注入一个 `claude`
//! 函数，**每次运行都重新读指针**——切换账号后已开的终端不用重开就跟着走。注入方式按 shell：
//! - PowerShell：照常读用户的 `$PROFILE`，再点源 `claude.ps1`（`-NoExit -Command`）；
//! - bash：`--rcfile` 指向 Meowo 的启动文件，它先按登录 shell 的顺序读用户配置，再定义函数；
//! - zsh：`ZDOTDIR` 指向 Meowo 的包装目录，每个启动文件先读用户的同名文件，`.zshrc` 末尾定义函数
//!   并把 `ZDOTDIR` 还给用户（之后的 `.zlogin` 直接读用户的）。
//!
//! 其余 shell（fish 等）不注入，照旧。

use std::path::PathBuf;

use portable_pty::CommandBuilder;

const CLAUDE_PS1: &str = include_str!("shell_claude/claude.ps1");
const CLAUDE_SH: &str = include_str!("shell_claude/claude.sh");

const BASH_RC: &str = r#"# Meowo 终端页签的 bash 启动文件：按登录 shell 的顺序读用户配置，再定义 claude 函数。
[ -f /etc/profile ] && . /etc/profile
if [ -f "$HOME/.bash_profile" ]; then . "$HOME/.bash_profile"
elif [ -f "$HOME/.bash_login" ]; then . "$HOME/.bash_login"
elif [ -f "$HOME/.profile" ]; then . "$HOME/.profile"
fi
. "$MEOWO_SHELL_DIR/claude.sh"
"#;

/// zsh 包装目录里的 `.zshenv` / `.zprofile`：读用户的同名文件，读完把 ZDOTDIR 指回包装目录，
/// 让 zsh 接着读我们的下一个文件。用户文件里自己改了 ZDOTDIR 的，记下来当用户的目录。
fn zsh_passthrough(file: &str) -> String {
    format!(
        r#"# Meowo 终端页签：先读用户自己的 {file}。
__meowo_zdotdir="$ZDOTDIR"; ZDOTDIR="${{MEOWO_USER_ZDOTDIR:-$HOME}}"
[ -f "$ZDOTDIR/{file}" ] && . "$ZDOTDIR/{file}"
MEOWO_USER_ZDOTDIR="$ZDOTDIR"; ZDOTDIR="$__meowo_zdotdir"; unset __meowo_zdotdir
"#
    )
}

const ZSH_RC: &str = r#"# Meowo 终端页签：读用户自己的 .zshrc，定义 claude 函数，再把 ZDOTDIR 还给用户。
ZDOTDIR="${MEOWO_USER_ZDOTDIR:-$HOME}"
[ -f "$ZDOTDIR/.zshrc" ] && . "$ZDOTDIR/.zshrc"
. "$MEOWO_SHELL_DIR/claude.sh"
"#;

fn shell_dir() -> PathBuf {
    crate::setup::meowo_dir().join("shell")
}

/// 指针文件：内容是当前 Claude 账号的数据目录，默认账号为空（Meowo 的默认账号不注入任何变量）。
pub(crate) fn pointer_path() -> PathBuf {
    shell_dir().join("claude-profile")
}

fn write_if_changed(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|old| old == body) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    meowo_agent::fsutil::write_atomic(path, body)
}

/// 按当前设置刷新指针。切换、删除、并回账号后调（见 `profile::notify_active_profile_changed`），
/// 开终端前也调一次兜底。
pub(crate) fn write_pointer() {
    let claude = meowo_agent::id::CLAUDE.as_str();
    let body = crate::profile::active_id(claude)
        .and_then(|id| crate::profile::data_dir(claude, Some(&id)))
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Err(e) = write_if_changed(&pointer_path(), &body) {
        eprintln!("Meowo 终端 claude 指针写入失败：{e}");
    }
}

/// 给终端页签的 shell 挂上 claude 函数。`shell` 是 shell 程序名（pwsh / powershell / bash / zsh…）。
/// 失败只记日志：终端照样按原样开（登录 shell），只是手打 claude 不跟随账号。
pub(crate) fn prepare(command: &mut CommandBuilder, shell: &str) {
    if !try_prepare(command, shell) {
        default_args(command, shell);
    }
}

fn try_prepare(command: &mut CommandBuilder, shell: &str) -> bool {
    write_pointer();
    let dir = shell_dir();
    let mut files: Vec<(PathBuf, String)> = vec![
        (dir.join("claude.ps1"), CLAUDE_PS1.to_string()),
        (dir.join("claude.sh"), CLAUDE_SH.to_string()),
        (dir.join("bashrc"), BASH_RC.to_string()),
    ];
    let zdot = dir.join("zsh");
    files.push((zdot.join(".zshenv"), zsh_passthrough(".zshenv")));
    files.push((zdot.join(".zprofile"), zsh_passthrough(".zprofile")));
    files.push((zdot.join(".zshrc"), ZSH_RC.to_string()));
    for (path, body) in &files {
        if let Err(e) = write_if_changed(path, body) {
            eprintln!("Meowo 终端 claude 函数未注入（{}）：{e}", path.display());
            return false;
        }
    }

    command.env("MEOWO_SHELL_DIR", &dir);
    command.env("MEOWO_CLAUDE_PROFILE_POINTER", pointer_path());
    // 用户自己设过 CLAUDE_CONFIG_DIR（Meowo 从它的环境继承下来）：默认账号时还原成它。
    if let Some(own) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        command.env("MEOWO_CLAUDE_USER_CONFIG_DIR", own);
    }
    match shell {
        "pwsh" | "powershell" => {
            // Bypass 只对这个 shell 进程生效：Windows PowerShell 5.1 的系统默认策略 Restricted 不许点源脚本，
            // 每次开终端都报红字、函数也没挂上（红队 1 复现）。系统策略不改。
            command.args([
                "-ExecutionPolicy",
                "Bypass",
                "-NoExit",
                "-Command",
                ". (Join-Path $env:MEOWO_SHELL_DIR 'claude.ps1')",
            ]);
        }
        "bash" => {
            command.arg("--rcfile");
            command.arg(dir.join("bashrc"));
            command.arg("-i");
        }
        "zsh" => {
            if let Some(user) = std::env::var_os("ZDOTDIR").filter(|v| !v.is_empty()) {
                command.env("MEOWO_USER_ZDOTDIR", user);
            }
            command.env("ZDOTDIR", &zdot);
            command.arg("-l");
        }
        _ => default_args(command, shell),
    }
    true
}

/// 不注入时 shell 的参数（登录 shell 读全用户配置）。
fn default_args(command: &mut CommandBuilder, shell: &str) {
    if !matches!(shell, "pwsh" | "powershell") {
        command.arg("-l");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_passthrough_reads_the_users_file_and_points_back() {
        let s = zsh_passthrough(".zprofile");
        assert!(s.contains(r#"[ -f "$ZDOTDIR/.zprofile" ] && . "$ZDOTDIR/.zprofile""#), "{s}");
        assert!(s.contains(r#"ZDOTDIR="$__meowo_zdotdir""#), "{s}");
    }

    /// 真跑一遍 posix 函数（bash 与 zsh 各一次，机器上有才跑）：指针切换跟随、空指针还原用户的值、
    /// 用户自己设的值优先、坏指针拒跑。macOS CI 上跑；Windows 上已用 Git Bash 手测同一段。
    #[cfg(unix)]
    #[test]
    fn posix_function_follows_the_pointer() {
        let root = std::env::temp_dir().join(format!("meowo-shell-claude-sh-{}", std::process::id()));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("claude");
        std::fs::write(&fake, "#!/bin/sh
echo \"CFG=[${CLAUDE_CONFIG_DIR:-}]\"
").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("claude.sh"), CLAUDE_SH).unwrap();
        let pointer = root.join("pointer");
        let script = format!(
            r#"export PATH="{bin}:$PATH" MEOWO_CLAUDE_PROFILE_POINTER="{pointer}" MEOWO_CLAUDE_USER_CONFIG_DIR=/own CLAUDE_CONFIG_DIR=/own
. "{sh}"
printf /acct/a > "$MEOWO_CLAUDE_PROFILE_POINTER"; claude
printf /acct/b > "$MEOWO_CLAUDE_PROFILE_POINTER"; claude
printf "" > "$MEOWO_CLAUDE_PROFILE_POINTER"; claude
echo "after=[$CLAUDE_CONFIG_DIR]"
export CLAUDE_CONFIG_DIR=/mine; printf /acct/a > "$MEOWO_CLAUDE_PROFILE_POINTER"; claude
printf relative > "$MEOWO_CLAUDE_PROFILE_POINTER"; unset CLAUDE_CONFIG_DIR; claude; echo "exit=$?"
"#,
            bin = bin.display(),
            pointer = pointer.display(),
            sh = root.join("claude.sh").display(),
        );
        for shell in ["bash", "zsh"] {
            let Ok(out) = std::process::Command::new(shell).args(["-f", "-c", &script]).output() else { continue };
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(
                lines,
                vec!["CFG=[/acct/a]", "CFG=[/acct/b]", "CFG=[/own]", "after=[/own]", "CFG=[/mine]", "exit=1"],
                "{shell} stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 真跑一遍 PowerShell 函数：指针切换后同一个 shell 里的 claude 拿到的 CLAUDE_CONFIG_DIR 跟着变，
    /// 指针为空时还原用户自己的值；这个终端里用户自己设的值优先。用一个假的 claude.cmd 打印环境。
    #[cfg(windows)]
    #[test]
    fn powershell_function_follows_the_pointer() {
        let root = std::env::temp_dir().join(format!("meowo-shell-claude-{}", std::process::id()));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude.cmd"), "@echo CFG=[%CLAUDE_CONFIG_DIR%]\r\n").unwrap();
        std::fs::write(root.join("claude.ps1"), CLAUDE_PS1).unwrap();
        let pointer = root.join("pointer");
        let script = format!(
            r#"$env:Path = '{bin};' + $env:Path
$env:MEOWO_CLAUDE_PROFILE_POINTER = '{pointer}'
$env:MEOWO_CLAUDE_USER_CONFIG_DIR = 'C:\own'
$env:CLAUDE_CONFIG_DIR = 'C:\own'
. '{ps1}'
Set-Content -LiteralPath '{pointer}' -Value 'C:\acct\a' -NoNewline
claude
Set-Content -LiteralPath '{pointer}' -Value 'C:\acct\b' -NoNewline
claude
Set-Content -LiteralPath '{pointer}' -Value '' -NoNewline
claude
"after=[$env:CLAUDE_CONFIG_DIR]"
$env:CLAUDE_CONFIG_DIR = 'C:\mine'
Set-Content -LiteralPath '{pointer}' -Value 'C:\acct\a' -NoNewline
claude
"#,
            bin = bin.display(),
            pointer = pointer.display(),
            ps1 = root.join("claude.ps1").display(),
        );
        let exe = if crate::terminal::pwsh_available() { "pwsh" } else { "powershell" };
        let out = std::process::Command::new(exe)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).replace('\r', "");
        let _ = std::fs::remove_dir_all(&root);
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines,
            vec![
                "CFG=[C:\\acct\\a]",
                "CFG=[C:\\acct\\b]",
                "CFG=[C:\\own]",
                "after=[C:\\own]",
                "CFG=[C:\\mine]",
            ],
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
