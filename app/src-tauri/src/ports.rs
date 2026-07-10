//! `meowo_agent::ports` 的宿主实现：把插件层声明的外部能力接到真实世界。
//!
//! 插件层只认 trait，故 HTTP 栈（ureq）与 macOS `security` 子进程都止步于此文件——插件的单测
//! 注入假实现即可，不需要真网络、也不需要 `#[cfg(target_os)]`。

use meowo_agent::ports::{Body, HttpError, HttpPort, HttpRequest, KeychainPort};

// ═══ HTTP ═══

pub struct UreqHttp;

impl HttpPort for UreqHttp {
    fn send(&self, req: &HttpRequest) -> Result<String, HttpError> {
        let mut r = match req.method {
            "POST" => ureq::post(req.url),
            _ => ureq::get(req.url),
        }
        .timeout(req.timeout);
        for (k, v) in req.headers {
            r = r.set(k, v);
        }
        let resp = match &req.body {
            Body::Empty => r.call(),
            Body::Json(v) => r.send_json(v.clone()),
            Body::Form(pairs) => r.send_form(pairs),
        };
        match resp {
            Ok(resp) => resp.into_string().map_err(|e| HttpError::Transport(e.to_string())),
            // 4xx/5xx：ureq 归为 Error::Status，响应体交给调用方判读（OAuth 错误码等）。
            Err(ureq::Error::Status(code, resp)) => {
                Err(HttpError::Status(code, resp.into_string().unwrap_or_default()))
            }
            Err(e) => Err(HttpError::Transport(e.to_string())),
        }
    }

    /// 流式下载。agent 的二进制是 240–260 MB——边读边写，绝不先缓冲进内存。
    fn download(
        &self,
        url: &str,
        dest: &std::path::Path,
        timeout: std::time::Duration,
        on_progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64, HttpError> {
        use std::io::{Read, Write};

        let resp = ureq::get(url).timeout(timeout).call().map_err(|e| match e {
            ureq::Error::Status(code, r) => HttpError::Status(code, r.into_string().unwrap_or_default()),
            e => HttpError::Transport(e.to_string()),
        })?;
        // Content-Length 可能缺失（chunked）；缺了就画不了进度条，但下载照常。
        let total: Option<u64> = resp.header("Content-Length").and_then(|s| s.parse().ok());

        let mut file = std::fs::File::create(dest).map_err(|e| HttpError::Transport(e.to_string()))?;
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut written: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| HttpError::Transport(e.to_string()))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| HttpError::Transport(e.to_string()))?;
            written += n as u64;
            on_progress(written, total);
        }
        file.flush().map_err(|e| HttpError::Transport(e.to_string()))?;
        Ok(written)
    }
}

// ═══ 系统密钥链 ═══

/// macOS 登录 Keychain（经 `security` 子进程）。非 macOS 编译成 [`meowo_agent::NoKeychain`] 的等价物。
pub struct SystemKeychain;

#[cfg(target_os = "macos")]
impl KeychainPort for SystemKeychain {
    fn available(&self) -> bool {
        true
    }

    fn read_password(&self, service: &str) -> Option<String> {
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", service, "-w"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        let s = s.trim_end_matches(['\r', '\n']).to_string();
        (!s.is_empty()).then_some(s)
    }

    fn read_account(&self, service: &str) -> Option<String> {
        // `-g`：属性打到 stdout、密码打到 stderr，这里只取属性。
        let out = std::process::Command::new("security")
            .args(["find-generic-password", "-s", service, "-g"])
            .output()
            .ok()?;
        parse_keychain_account(&String::from_utf8_lossy(&out.stdout))
    }

    /// `-U`：条目存在则更新。password 经 argv 传入，仅同用户进程可见，与本仓既有 shell-out 一致。
    fn write_password(&self, service: &str, account: &str, password: &str) -> Result<(), String> {
        let status = std::process::Command::new("security")
            .args(["add-generic-password", "-U", "-s", service, "-a", account, "-w", password])
            .status()
            .map_err(|e| format!("写回 Keychain 失败：{e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("写回 Keychain 失败（security add-generic-password 非零退出）".into())
        }
    }
}

/// 非 macOS：无密钥链，凭据走文件。全部方法取 trait 默认实现（`available()` 恒 false）。
#[cfg(not(target_os = "macos"))]
impl KeychainPort for SystemKeychain {}

/// 从 `security find-generic-password -g` 的属性输出里抠出 account（`"acct"<blob>=...`）。
/// 形如 `"acct"<blob>="root"`，或 non-UTF8 时 `0x726F6F74  "root"`（hex + 可读串）→ 取引号内。
/// 纯函数便于单测；仅 macOS 调用，其它平台放行 dead_code。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_keychain_account(attrs: &str) -> Option<String> {
    let key = "\"acct\"<blob>=";
    for line in attrs.lines() {
        let Some(idx) = line.find(key) else { continue };
        let rest = line[idx + key.len()..].trim();
        if rest == "<NULL>" {
            return None;
        }
        let after = &rest[rest.find('"')? + 1..];
        let v = &after[..after.find('"')?];
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

// ═══ 注入点 ═══

static HTTP: UreqHttp = UreqHttp;
static KEYCHAIN: SystemKeychain = SystemKeychain;

/// 本进程的一组端口。所有 agent 能力调用都从这里取。
pub fn ports() -> meowo_agent::Ports<'static> {
    meowo_agent::Ports { http: &HTTP, keychain: &KEYCHAIN }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_keychain_account_extracts_acct() {
        let attrs = "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\n    \"acct\"<blob>=\"root\"\n    \"svce\"<blob>=\"Claude Code-credentials\"\n";
        assert_eq!(super::parse_keychain_account(attrs).as_deref(), Some("root"));
        // non-UTF8 时 security 打成 hex + 可读串，取引号内。
        let hexed = "    \"acct\"<blob>=0x726F6F74  \"root\"\n";
        assert_eq!(super::parse_keychain_account(hexed).as_deref(), Some("root"));
        // 没有 acct 行 / NULL → None。
        assert_eq!(super::parse_keychain_account("nothing here"), None);
        assert_eq!(super::parse_keychain_account("    \"acct\"<blob>=<NULL>\n"), None);
    }
}
