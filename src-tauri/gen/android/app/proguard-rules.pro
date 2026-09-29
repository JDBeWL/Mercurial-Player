# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# ============================================================================
# Rust(JNI) 按「字符串名 + 签名」回调 Kotlin —— 这些符号必须保留
# ============================================================================
# Rust 侧不是静态引用 Kotlin，而是通过 JNI 传字符串去查方法，见：
#   src-tauri/src/android_jni.rs    jni_call_static_string / jni_call_void_string
#   src-tauri/src/android_saf.rs    jni_call_string / jni_call_int / jni_call_static_noop
#   src-tauri/src/android.rs        MediaBridge.update
#   src-tauri/src/audio/aaudio/device.rs  AudioBridge.getOutputDevicesJson
#
# R8 编译期看不到这类引用，会把方法改名或整个裁掉，release 包运行时就会抛
#   NoSuchMethodError: no static method "L.../SafBridge;.getAppDataDir()Ljava/lang/String;"
# （本项目实测：release 包启动即崩在 SafBridge.getAppDataDir）。
# 因此凡是「Rust 按名字调 Kotlin」的类，整体保留。
#
# ⚠️ 以后在 Rust 里新增这种跨语言调用时，务必确认目标类已在此列表内。
-keep class com.jdbewl.mercurial_player.SafBridge { *; }
-keep class com.jdbewl.mercurial_player.AudioBridge { *; }
-keep class com.jdbewl.mercurial_player.MediaBridge { *; }
-keep class com.jdbewl.mercurial_player.MainActivity { *; }

# 说明：Kotlin 侧 `external fun`（Kotlin → Rust）的名字由 AGP 默认的
# proguard-android-optimize.txt（-keepclasseswithmembernames class * { native <methods>; }）
# 保护，无需在这里重复声明。
