//! 跨 crate 共享的文件系统小工具。

/// 原子写文件（pid 后缀临时文件 + rename）：读端裸读不会见到半截文件；pid 后缀防多进程
/// 同时写同一路径时临时文件互相覆盖（吸取 kimi 凭据写回的实现）。rename 失败时 best-effort
/// 清理临时文件。settings.json / usage-cache.json / 各 agent 凭据写回统一走这里，
/// 消除四份各自漂移的 tmp+rename 拷贝。
///
/// 刻意**不**做成端口：它是纯 `std`，测试拿临时目录就能覆盖，注入只会平添间接层。
/// 端口留给真正需要隔离的外部世界——HTTP 与系统密钥链，见 [`crate::ports`]。
pub fn write_atomic(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn write_atomic_replaces_content_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("meowo-fsutil-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.json");
        super::write_atomic(&p, "{\"a\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"a\":1}");
        super::write_atomic(&p, "{\"a\":2}").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{\"a\":2}");
        // 临时文件不残留。
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tmp."))
            .collect();
        assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
