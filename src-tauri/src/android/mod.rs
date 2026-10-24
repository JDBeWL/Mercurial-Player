//! Android 平台专属代码
//!
//! 与 Kotlin 侧的 JNI 互调、SAF 存储桥、界面字号与系统栏；非 Android 目标下 `saf` / `font_scale` / `system_ui` 为 no-op 存根，调用点无需 cfg 分支。

pub mod font_scale;
pub mod saf;
pub mod system_ui;

#[cfg(target_os = "android")]
mod entry;
#[cfg(target_os = "android")]
pub mod java_bridge;

#[cfg(target_os = "android")]
pub use entry::{notify_media_session, set_app_handle};
