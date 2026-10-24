//! 应用内「界面字号」倍率（Android），桌面端 no-op
//!
//! WebView 把系统「字体大小」当 `textZoom` 初值乘到所有 CSS px 上，而固定像素布局的容器不随之长大，带下伸部的字母会被切掉。

use crate::error::AppError;

/// 设置应用内界面字号倍率（`1.0` = 设计稿原始大小）；上下限由 Kotlin 侧 `FontScaleBridge` 统一 clamp，这里不校验。
#[cfg(target_os = "android")]
pub fn set_app_font_scale(scale: f32) -> Result<(), AppError> {
    crate::android::java_bridge::jni_call_void_float(
        "com/jdbewl/mercurial_player/MainActivity",
        "setAppFontScale",
        scale,
    )
}

/// 桌面端 no-op：Windows/macOS/Linux 的 WebView 不受系统字号影响
#[cfg(not(target_os = "android"))]
pub fn set_app_font_scale(_scale: f32) -> Result<(), AppError> {
    Ok(())
}
