//! 仅 Windows：hook 时把一个唯一 token 写进【本会话标签】的 Windows Terminal 标题。
//!
//! meowo-reporter 是被 agent(claude/codex/kimi) 在它自己的标签/ConPTY 里 spawn 的 hook 子进程，默认
//! 继承父进程的控制台。即使 agent 把 hook 的 stdout 重定向到管道抓 payload，子进程仍可用
//! CreateFileW("CONOUT$") 拿到【本标签 active screen buffer】句柄（绕过 stdout 重定向），往里写一条
//! OSC 2 标题序列 → conhost 解析更新标题 → 经 output pipe 转发给 WT → WT 改该标签标题。
//!
//! 用途：codex/kimi 这类不把任务标题写进标签的 CLI，其标签默认只显目录名；同窗口同目录两个会话标签
//! 同名时 UIA 无法区分（WT 不把 tab→进程映射暴露出来）。让 meowo-reporter 把【session_id 末 8 位】这个
//! 全局唯一 token 写进标签，meowo-app 即可按 token 精确匹配到正确标签（见 app 侧 focus_terminal_tab）。

/// 取 session_id 末 8 位十六进制作为 token（去掉非十六进制字符后取尾 8 个；不足则全取）。
/// claude/codex 的 UUID、kimi 的 `session_<uuid>` 都能得到稳定唯一的短码。纯函数，meowo-app 也复用它
/// 算出同一 token 做匹配。
pub fn short_sid(session_id: &str) -> String {
    let hex: String = session_id.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let n = hex.len();
    hex[n.saturating_sub(8)..].to_string()
}

/// 把 OSC 2 标题序列写进本标签的 ConPTY，设其标题为 `title`。失败（无 console / 非 WT / 被别的 PTY
/// 包裹）一律静默放弃——meowo-app 侧匹配不到该 token 时会自然回退到窗口级定位。
#[cfg(target_os = "windows")]
pub fn set_tab_title(title: &str) {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::WriteConsoleW;

    const GENERIC_WRITE: u32 = 0x4000_0000;

    // 去掉所有控制字符（含裸 ESC），防 OSC 注入（终端 CVE）。
    let clean: String = title.chars().filter(|c| !c.is_control()).collect();
    if clean.is_empty() {
        return;
    }
    // OSC 2: ESC ] 2 ; <title> BEL —— 只设 window/tab title。
    let seq = format!("\u{1b}]2;{clean}\u{7}");
    let wide: Vec<u16> = seq.encode_utf16().collect();
    let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();

    unsafe {
        // CONOUT$：即使 stdout 被 hook 管道重定向，仍拿到本标签 active screen buffer 句柄。
        // 必须 FILE_SHARE_READ|WRITE + OPEN_EXISTING。
        let h = CreateFileW(
            conout.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE || h.is_null() {
            return;
        }
        let mut written: u32 = 0;
        // 控制台句柄用 WriteConsoleW（走 conhost VT 解析）；UTF-16 免 codepage 问题。
        let _ = WriteConsoleW(
            h,
            wide.as_ptr().cast(),
            wide.len() as u32,
            &mut written,
            std::ptr::null(),
        );
        CloseHandle(h);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn set_tab_title(_title: &str) {}

#[cfg(test)]
mod tests {
    use super::short_sid;

    #[test]
    fn short_sid_takes_last_8_hex() {
        assert_eq!(short_sid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"), "34567890");
        assert_eq!(
            short_sid("session_00000000-0000-0000-0000-0000000000ab"),
            "000000ab"
        );
        assert_eq!(short_sid("xyz"), ""); // 无十六进制
        assert_eq!(short_sid("abc"), "abc"); // 不足 8 位
    }
}
