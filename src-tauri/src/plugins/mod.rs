//! 插件系统：发现/读取内置与用户插件目录，命令层见 `manager` 与 `commands`。

pub mod commands;
pub mod manager;

pub use commands::*;
