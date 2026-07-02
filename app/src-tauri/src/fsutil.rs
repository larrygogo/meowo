//! 跨模块共享的文件系统小工具。

/// 原子写文件（pid 后缀临时文件 + rename）：读端裸读不会见到半截文件；pid 后缀防多进程
/// 同时写同一路径时临时文件互相覆盖（吸取 kimi 凭据写回的实现）。rename 失败时 best-effort
/// 清理临时文件。settings.json / usage-cache.json / 各 provider 凭据写回统一走这里，
/// 消除四份各自漂移的 tmp+rename 拷贝。
pub(crate) fn write_atomic(path: &std::path::Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    std::fs::write(&tmp, body)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}
