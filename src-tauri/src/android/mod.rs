//! Android 平台专属代码：与 Kotlin 侧的互相调用、SAF 存储桥、界面字号与系统栏。
//!
//! - `entry`：Kotlin → Rust 的 JNI 导出（ndk_context 初始化、通知栏/线控媒体动作）与全局 AppHandle
//! - `java_bridge`：Rust → Kotlin 的 JNI 调用封装（其余四个模块都基于它）
//! - `saf`：Storage Access Framework 的 fd 桥与目录授权
//! - `font_scale`：应用内界面字号倍率
//! - `system_ui`：状态栏 / 导航栏显隐
//!
//! `saf`、`font_scale`、`system_ui` 在非 Android 目标上编译为 no-op 存根，
//! 调用点因此不需要 cfg 分支；`entry` 与 `java_bridge` 只在 Android 目标编译。

pub mod font_scale;
pub mod saf;
pub mod system_ui;

#[cfg(target_os = "android")]
mod entry;
#[cfg(target_os = "android")]
pub mod java_bridge;

#[cfg(target_os = "android")]
pub use entry::{notify_media_session, set_app_handle};
