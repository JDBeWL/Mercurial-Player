//! Android 系统栏显隐（状态栏 / 导航栏）。
//!
//! tao 在 Android 上没实现 `set_fullscreen`（源码里直接 warn 后返回），
//! 所以只能自己经 JNI 交给 Kotlin 侧的 `WindowInsetsControllerCompat`。

use crate::error::AppError;

/// 隐藏或恢复系统栏；`true` 表示隐藏对应的一条系统栏
#[cfg(target_os = "android")]
pub fn set_system_ui_hidden(
    hide_status_bars: bool,
    hide_navigation_bars: bool,
) -> Result<(), AppError> {
    crate::android::java_bridge::jni_call_void_two_bools(
        "com/jdbewl/mercurial_player/MainActivity",
        "setSystemUiHidden",
        hide_status_bars,
        hide_navigation_bars,
    )
}

/// 桌面端 no-op：系统栏概念只属于 Android
#[cfg(not(target_os = "android"))]
pub fn set_system_ui_hidden(
    _hide_status_bars: bool,
    _hide_navigation_bars: bool,
) -> Result<(), AppError> {
    Ok(())
}
