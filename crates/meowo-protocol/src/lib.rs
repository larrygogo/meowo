//! Meowo 组件边界上的共享协议。
//!
//! 这里仅放跨 crate / 跨语言传输的数据形态与纯编解码逻辑，不依赖 Tauri、数据库、网络或
//! Agent 插件。领域模型在各自 crate 内保持独立，在边界处显式转换成这里的 DTO。

pub mod broker;
pub mod ipc;
