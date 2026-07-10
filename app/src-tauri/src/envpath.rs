//! Windows 持久 PATH 的判定与追加。
//!
//! 为什么需要它：agent 的官方安装器**不保证**把自己的 bin 目录写进用户 PATH。claude 的
//! `claude.exe install` 只在 stdout 打一行「请自己去系统属性里加 PATH」，然后 exit 0；
//! meowo 后台跑它、只看退出码，于是一律报「安装成功」，用户直到手敲 `claude` 才发现打不开
//! （kimi 的安装器则自己写了 PATH，故 `~/.kimi-code/bin` 在而 `~/.local/bin` 不在）。
//!
//! meowo 自身不受影响——启动 agent 走的是 `Installation::launch_argv()` 固化的绝对路径，
//! 刻意绕开 PATH，反而把这个坑掩盖得很彻底。
//!
//! **不能用进程 PATH（`std::env::var("PATH")`）判定**：那是 meowo-app 启动那一刻的快照，
//! 装完之后即便 PATH 已被写好，本进程也看不见——会稳定假阴性。故一律读注册表的持久值。

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ};
use winreg::{RegKey, RegValue};

/// 用户级环境变量所在键。
const USER_ENV: &str = "Environment";
/// 机器级环境变量所在键（只读参与判定：目录已在系统 PATH 时无需再往用户 PATH 加）。
const MACHINE_ENV: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// 展开 `%NAME%`。未定义的变量**原样保留**（与 Windows 自身行为一致，避免把 `%FOO%` 吃成空串
/// 后让两个不同目录比较相等）。大小写不敏感——Windows 环境变量名不区分大小写。
pub(crate) fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            // `%%` 或 `%NAME%`：取中间的名字去查环境变量。
            Some(end) => {
                let name = &after[..end];
                match std::env::vars().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                    Some((_, v)) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            // 落单的 `%`：原样输出，收工。
            None => {
                out.push('%');
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 规范化一个 PATH 条目用于比较：展开变量 → `/` 归一为 `\` → 去尾部分隔符 → 小写。
/// 不做 canonicalize：目录可能尚不存在（装之前就想判定），且解析符号链接会引入 IO 失败面。
pub(crate) fn normalize(entry: &str) -> String {
    let expanded = expand_env(entry.trim());
    let slashed = expanded.replace('/', "\\");
    let trimmed = slashed.trim_end_matches('\\');
    trimmed.to_lowercase()
}

/// PATH 串里是否已包含该目录（空条目跳过——`a;;b` 里的空项不是「当前目录」的意思，别误判）。
pub(crate) fn path_contains(path_value: &str, dir: &str) -> bool {
    let target = normalize(dir);
    if target.is_empty() {
        return false;
    }
    path_value
        .split(';')
        .filter(|e| !e.trim().is_empty())
        .any(|e| normalize(e) == target)
}

/// 把目录追加到 PATH 串尾部。调用方须先用 [`path_contains`] 确认不重复。
pub(crate) fn append_dir(old: &str, dir: &str) -> String {
    let base = old.trim_end_matches(';');
    if base.is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    }
}

/// 读某个环境键下的 `Path`（缺键/缺值都返回 None，不是错误：全新用户可能就没有用户级 Path）。
fn read_path(root: winreg::HKEY, subkey: &str) -> Option<String> {
    RegKey::predef(root)
        .open_subkey_with_flags(subkey, KEY_READ)
        .ok()?
        .get_value::<String, _>("Path")
        .ok()
}

/// 该目录是否已在**持久** PATH（用户级或机器级任一即可）上。
pub(crate) fn dir_on_persistent_path(dir: &str) -> bool {
    let user = read_path(HKEY_CURRENT_USER, USER_ENV).unwrap_or_default();
    let machine = read_path(HKEY_LOCAL_MACHINE, MACHINE_ENV).unwrap_or_default();
    path_contains(&user, dir) || path_contains(&machine, dir)
}

/// UTF-16LE + 结尾 NUL，注册表字符串值的字节表示。
fn utf16_bytes(s: &str) -> Vec<u8> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .flat_map(|u| u.to_le_bytes())
        .collect()
}

/// 把目录追加进**用户级** PATH，并广播 WM_SETTINGCHANGE 让新进程立刻看到。
///
/// 只碰 HKCU。绝不读 `$env:PATH` 再回写用户 PATH——那会把机器级条目复制进用户级，
/// 是这类脚本的经典事故。已存在则幂等返回 Ok。
///
/// 保留原值的类型（`REG_EXPAND_SZ` 常见，含 `%USERPROFILE%` 之类）：写成 `REG_SZ` 会让
/// 其余条目里的变量失去展开语义。原本没有该值时按 `REG_EXPAND_SZ` 新建。
pub(crate) fn add_dir_to_user_path(dir: &str) -> Result<(), String> {
    if dir_on_persistent_path(dir) {
        return Ok(()); // 幂等：已在 PATH 上（可能是机器级），不重复追加
    }
    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(USER_ENV, KEY_READ | KEY_SET_VALUE)
        .map_err(|e| format!("打开注册表 HKCU\\{USER_ENV} 失败：{e}"))?;

    // 原始值决定新值的类型；读不到就当空串 + REG_EXPAND_SZ。
    let (old, vtype) = match env.get_raw_value("Path") {
        Ok(raw) => {
            let text = env.get_value::<String, _>("Path").map_err(|e| format!("读取 Path 失败：{e}"))?;
            (text, raw.vtype)
        }
        Err(_) => (String::new(), REG_EXPAND_SZ),
    };

    let new = append_dir(&old, dir);
    env.set_raw_value(
        "Path",
        &RegValue { bytes: utf16_bytes(&new), vtype },
    )
    .map_err(|e| format!("写入 Path 失败：{e}"))?;

    broadcast_env_change();
    Ok(())
}

/// 通知所有顶层窗口环境变量已变（新开的终端/资源管理器据此重读）。
/// 已在运行的终端不会更新——它们的 PATH 是自己启动时的快照，只能重开。
fn broadcast_env_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let param: Vec<u16> = OsStr::new("Environment").encode_wide().chain(std::iter::once(0)).collect();
    // SMTO_ABORTIFHUNG + 5s 超时：某个卡死的顶层窗口不该拖垮我们。失败无所谓——
    // 值已经落盘，新开的终端照样能读到。
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_keeps_undefined_vars_verbatim() {
        std::env::set_var("MEOWO_TEST_EXPAND", "C:\\x");
        assert_eq!(expand_env("%MEOWO_TEST_EXPAND%\\bin"), "C:\\x\\bin");
        // 大小写不敏感
        assert_eq!(expand_env("%meowo_test_expand%"), "C:\\x");
        // 未定义 → 原样保留，绝不吃成空串（否则 `%A%\bin` 与 `%B%\bin` 会误判相等）
        assert_eq!(expand_env("%MEOWO_NOPE%\\bin"), "%MEOWO_NOPE%\\bin");
        // 落单的 %
        assert_eq!(expand_env("C:\\100%"), "C:\\100%");
        std::env::remove_var("MEOWO_TEST_EXPAND");
    }

    #[test]
    fn normalize_is_case_and_separator_insensitive() {
        assert_eq!(normalize("C:/Users/x/.local/bin/"), "c:\\users\\x\\.local\\bin");
        assert_eq!(normalize("  C:\\Users\\X\\.local\\bin\\\\  "), "c:\\users\\x\\.local\\bin");
    }

    #[test]
    fn path_contains_matches_regardless_of_form() {
        let p = r"C:\Windows;C:\Users\x\.local\bin\;C:\Program Files\CMake\bin";
        assert!(path_contains(p, "c:/users/x/.local/bin"));
        assert!(!path_contains(p, r"C:\Users\x\.cargo\bin"));
    }

    /// `a;;b` 的空项不该被当成任何目录——否则空目录名会与任意条目误匹配。
    #[test]
    fn path_contains_skips_empty_entries() {
        assert!(!path_contains("C:\\a;;C:\\b", ""));
        assert!(path_contains("C:\\a;;C:\\b", "C:\\b"));
    }

    #[test]
    fn append_dir_handles_trailing_semicolons_and_empty() {
        assert_eq!(append_dir("C:\\a;C:\\b", "C:\\c"), "C:\\a;C:\\b;C:\\c");
        assert_eq!(append_dir("C:\\a;", "C:\\c"), "C:\\a;C:\\c");
        assert_eq!(append_dir("", "C:\\c"), "C:\\c");
    }

    /// 真机自检：本模块的判定必须与 Windows 自身对 PATH 的解析一致。
    /// 用一个绝不可能存在的目录，确保 dir_on_persistent_path 不会假阳性。
    #[test]
    fn nonexistent_dir_is_not_on_path() {
        assert!(!dir_on_persistent_path(r"C:\meowo\definitely\not\on\path"));
    }
}
