//! 跨模块共享的文件系统小工具。
//!
//! 实现在 `meowo_agent::fsutil`——插件层写凭据、setup 写配置、account 写用量缓存都用同一份原子写，
//! 避免各自漂移的 tmp+rename 拷贝。本模块只做重导出，供 app 内既有调用点保持原路径。
pub(crate) use meowo_agent::fsutil::write_atomic;
