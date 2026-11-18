//! 令牌的存放处：系统凭据库。Windows 凭据管理器（CredWriteW / CredReadW / CredDeleteW，普通凭据，
//! 只有当前 Windows 用户读得到），macOS 登录钥匙串（/usr/bin/security）。别的平台存不了令牌，
//! 设置页给出说明，不能添加代码托管账号。

use super::Token;

/// 凭据库里的条目名（Windows 的 TargetName、macOS 的 service）。
#[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
const TARGET: &str = "Meowo/github.com";
/// macOS 钥匙串条目的 account。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const ACCOUNT: &str = "github.com";

/// `security -i` 从标准输入读的那一行：写入（-U 已有则更新）。令牌不进进程参数（`ps` 看不到）。
/// 各段都放在双引号里；Token::new 已把令牌限制在字母数字与 `_-.`，任一段含引号、反斜杠或换行就不拼。
/// 条目的 account 与标签由调用方指定（代码托管账号用）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn interactive_line(target: &str, account: &str, label: &str, token: &Token) -> Option<String> {
    let plain = |s: &str| !s.is_empty() && !s.contains(['"', '\\', '\n', '\r']);
    (plain(target) && plain(account) && plain(label) && plain(token.expose())).then(|| {
        format!("add-generic-password -U -s \"{target}\" -a \"{account}\" -l \"{label}\" -w \"{}\"\n", token.expose())
    })
}

pub(crate) trait Vault: Sync {
    fn supported(&self) -> bool;
    /// Ok(None) = 没存过。
    fn read(&self) -> Result<Option<Token>, String>;
    fn write(&self, token: &Token) -> Result<(), String>;
    /// 没有这一条也算成功。
    fn delete(&self) -> Result<(), String>;
}

pub(crate) struct SystemVault;

#[cfg(target_os = "windows")]
mod imp {
    use super::Token;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FILETIME};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn read(target: &str, _account: &str) -> Result<Option<Token>, String> {
        let target = wide(target);
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: target 以 0 结尾；成功时 cred 由系统分配，读完用 CredFree 释放。
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            return if error == ERROR_NOT_FOUND { Ok(None) } else { Err(format!("github/credential_failed: CredReadW {error}")) };
        }
        // SAFETY: 成功时 cred 非空，CredentialBlob 指向 CredentialBlobSize 字节。
        let bytes = unsafe {
            let c = &*cred;
            let blob = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(cred as *const core::ffi::c_void);
            blob
        };
        Ok(String::from_utf8(bytes).ok().and_then(|text| Token::secret(&text)))
    }

    pub(super) fn write(target: &str, account: &str, _label: &str, token: &Token) -> Result<(), String> {
        let mut target = wide(target);
        let mut user = wide(account);
        let mut blob = token.expose().as_bytes().to_vec();
        let cred = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            // 只在这台电脑上，不随漫游配置同步到别的机器。
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: 各指针在调用期间有效；系统复制数据，不持有它们。
        let ok = unsafe { CredWriteW(&cred, 0) };
        // 内存里那份明文尽快清掉。
        blob.iter_mut().for_each(|b| *b = 0);
        if ok == 0 {
            return Err(format!("github/credential_failed: CredWriteW {}", unsafe { GetLastError() }));
        }
        Ok(())
    }

    pub(super) fn delete(target: &str, _account: &str) -> Result<(), String> {
        let target = wide(target);
        // SAFETY: target 以 0 结尾。
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_NOT_FOUND {
                return Err(format!("github/credential_failed: CredDeleteW {error}"));
            }
        }
        Ok(())
    }
}

/// macOS：/usr/bin/security。读和删经命令行参数（只有条目名）；写入用 `security -i` 从标准输入传命令，
/// 令牌不出现在进程参数里。`-i` 的退出码不反映单条命令成败，写完读回核对。
#[cfg(target_os = "macos")]
mod imp {
    use super::{interactive_line, Token};
    use std::process::{Command, Stdio};

    const SECURITY: &str = "/usr/bin/security";
    /// security 找不到条目时的退出码（errSecItemNotFound）。
    const NOT_FOUND: i32 = 44;

    pub(super) fn read(target: &str, account: &str) -> Result<Option<Token>, String> {
        let out = Command::new(SECURITY)
            .args(["find-generic-password", "-s", target, "-a", account, "-w"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("github/credential_failed: {e}"))?;
        if out.status.code() == Some(NOT_FOUND) {
            return Ok(None);
        }
        if !out.status.success() {
            return Err(format!("github/credential_failed: security {}", out.status));
        }
        Ok(String::from_utf8(out.stdout).ok().and_then(|text| Token::secret(&text)))
    }

    pub(super) fn write(target: &str, account: &str, label: &str, token: &Token) -> Result<(), String> {
        use std::io::Write as _;
        let line = interactive_line(target, account, label, token).ok_or("github/credential_failed: bad token")?;
        let mut child = Command::new(SECURITY)
            .arg("-i")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("github/credential_failed: {e}"))?;
        // 写完就关掉标准输入：security -i 读到 EOF 退出。
        let wrote = child.stdin.take().map(|mut stdin| stdin.write_all(line.as_bytes()));
        let status = child.wait().map_err(|e| format!("github/credential_failed: {e}"))?;
        if !matches!(wrote, Some(Ok(()))) || !status.success() {
            return Err(format!("github/credential_failed: security -i {status}"));
        }
        match read(target, account)? {
            Some(stored) if stored == *token => Ok(()),
            _ => Err("github/credential_failed: keychain write not visible".into()),
        }
    }

    pub(super) fn delete(target: &str, account: &str) -> Result<(), String> {
        let status = Command::new(SECURITY)
            .args(["delete-generic-password", "-s", target, "-a", account])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("github/credential_failed: {e}"))?;
        if status.success() || status.code() == Some(NOT_FOUND) { Ok(()) } else { Err(format!("github/credential_failed: security {status}")) }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl Vault for SystemVault {
    fn supported(&self) -> bool {
        true
    }
    fn read(&self) -> Result<Option<Token>, String> {
        imp::read(TARGET, ACCOUNT)
    }
    fn write(&self, token: &Token) -> Result<(), String> {
        imp::write(TARGET, ACCOUNT, "Meowo GitHub", token)
    }
    fn delete(&self) -> Result<(), String> {
        imp::delete(TARGET, ACCOUNT)
    }
}

/// 代码托管账号的一条凭据：条目名 `Meowo/<平台>/<主机>/<账号>`（旧版 GitLab 设备码登录另存的 `#refresh` 只在迁移时删），
/// macOS 钥匙串的 account 是 `<账号>@<主机>`。
pub(crate) struct KeyVault {
    pub(crate) target: String,
    pub(crate) account: String,
    pub(crate) label: String,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl Vault for KeyVault {
    fn supported(&self) -> bool {
        true
    }
    fn read(&self) -> Result<Option<Token>, String> {
        imp::read(&self.target, &self.account)
    }
    fn write(&self, token: &Token) -> Result<(), String> {
        imp::write(&self.target, &self.account, &self.label, token)
    }
    fn delete(&self) -> Result<(), String> {
        imp::delete(&self.target, &self.account)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Vault for KeyVault {
    fn supported(&self) -> bool {
        false
    }
    fn read(&self) -> Result<Option<Token>, String> {
        Ok(None)
    }
    fn write(&self, _token: &Token) -> Result<(), String> {
        Err("github/credential_unsupported".into())
    }
    fn delete(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
impl Vault for SystemVault {
    fn supported(&self) -> bool {
        false
    }
    fn read(&self) -> Result<Option<Token>, String> {
        Ok(None)
    }
    fn write(&self, _token: &Token) -> Result<(), String> {
        Err("github/credential_unsupported".into())
    }
    fn delete(&self) -> Result<(), String> {
        Ok(())
    }
}

/// 真凭据库的往返（Windows 本机可跑；用一个测试专用的条目名，跑完删掉，不碰 Meowo 自己的那一条）。
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn windows_凭据管理器_写读删往返() {
        let target = format!("Meowo/github.com-test-{}", std::process::id());
        let token = Token::new("gho_ROUNDTRIP123").unwrap();
        assert_eq!(imp::read(&target, ACCOUNT).unwrap(), None);
        imp::write(&target, ACCOUNT, "Meowo GitHub", &token).unwrap();
        assert_eq!(imp::read(&target, ACCOUNT).unwrap(), Some(token));
        imp::delete(&target, ACCOUNT).unwrap();
        assert_eq!(imp::read(&target, ACCOUNT).unwrap(), None);
        // 删一条不存在的也算成功（退出登录可重复点）。
        imp::delete(&target, ACCOUNT).unwrap();
    }

    #[test]
    fn windows_凭据管理器_代码托管账号_按平台主机账号分条() {
        let pid = std::process::id();
        let a = KeyVault { target: format!("Meowo/gitlab/gitlab.com/test-{pid}"), account: "me@gitlab.com".into(), label: "Meowo GitLab".into() };
        let b = KeyVault { target: format!("Meowo/gitea/codeberg.org/test-{pid}"), account: "me@codeberg.org".into(), label: "Meowo Gitea".into() };
        // Atlassian API token 带 `=`：宽字符集的令牌也能原样存取。
        let ta = Token::secret("glpat-AbC_123").unwrap();
        let tb = Token::secret("ATATT3xFfGF0abc-_=12AB34CD").unwrap();
        a.write(&ta).unwrap();
        b.write(&tb).unwrap();
        assert_eq!((a.read().unwrap(), b.read().unwrap()), (Some(ta), Some(tb)));
        a.delete().unwrap();
        assert_eq!(a.read().unwrap(), None);
        assert!(b.read().unwrap().is_some(), "删一条不动另一条");
        b.delete().unwrap();
    }
}
#[cfg(test)]
mod line_tests {
    use super::*;

    /// macOS 写钥匙串的那一行（各平台都能跑这个纯函数）：令牌在标准输入里、带引号，条目名与 account 对。
    #[test]
    fn security_交互写入行_令牌不进参数且拼不出别的命令() {
        let token = Token::new("gho_ABC_123").unwrap();
        assert_eq!(
            interactive_line(TARGET, ACCOUNT, "Meowo GitHub", &token).unwrap(),
            "add-generic-password -U -s \"Meowo/github.com\" -a \"github.com\" -l \"Meowo GitHub\" -w \"gho_ABC_123\"\n"
        );
        assert!(interactive_line("bad\"target", ACCOUNT, "Meowo GitHub", &token).is_none());
        assert!(interactive_line("line\nbreak", ACCOUNT, "Meowo GitHub", &token).is_none());
    }
}