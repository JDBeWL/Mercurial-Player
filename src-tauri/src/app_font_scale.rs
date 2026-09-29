//! 应用内「界面字号」倍率（Android）
//!
//! # 为什么需要它
//!
//! Android 的「字体大小」设置会被 WebView 当成字号倍率，乘到**所有 CSS px 字号**上。
//! 真机实测（Redmi Note 11T Pro，`font_scale=1.17`）：CSS 里写的 `16px` 实际按
//! `18.56px` 排版、`14px` 按 `16.24px` 排版 —— 把系统字号调回 `1.0` 后二者立刻变回
//! `16px` / `14px`（探针实测，是这条结论的决定性证据，见 `.workbuddy/memory` 日志）。
//!
//! 而本项目是**固定像素布局的桌面级 UI**（`index.html` 的 viewport 注释也这么写）：
//! 字号被放大 17% 之后，容器盒子并不会跟着长 —— 于是 `max-height: 1.4em` 这类按原始
//! 字号算出来的高度就不够了，`g` / `y` 这些带下伸部的字母会被切掉下半截。
//! 与其到处补窟窿，不如把界面字号收归应用自己控制。
//!
//! # 做法
//!
//! `WebSettings.textZoom` 就是字号倍率的**唯一来源**，它的初值跟着系统「字体大小」走
//! （实测 `fontScale=1.17` 时初值为 116）。所以应用自己显式写
//! `textZoom = 100 × 应用内倍率` 就已经把系统设置整体覆盖掉，「通用设置 → 界面字号」
//! 成为唯一来源；系统字号被改变时（WebView 重建 / `onConfigurationChanged`）再覆盖一次。
//!
//! ⚠️ 别当成"两个因子相乘"：第一版按 `textZoom = 100 × appScale / fontScale` 写，
//! 真机上 16px 被排成 13.6px（= 16 × 0.85）而不是 16px —— 说明 fontScale 不是叠在
//! textZoom 之外的乘子，它**就是** textZoom 的初值。
//!
//! 本模块只做 Rust → Kotlin 的转发；桌面端是 no-op —— Windows/macOS/Linux 的 WebView
//! 不走这套机制，界面字号维持设计稿原样，改动前行为零差异。

use crate::error::AppError;

/// 设置应用内界面字号倍率（`1.0` = 设计稿原始大小）
///
/// 前端在启动时与用户改动时各推一次。倍率的上下限由 Kotlin 侧统一 clamp
/// （见 `FontScaleBridge.MIN_SCALE` / `MAX_SCALE`），这里不做校验。
#[cfg(target_os = "android")]
pub fn set_app_font_scale(scale: f32) -> Result<(), AppError> {
    crate::android_jni::jni_call_void_float(
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
