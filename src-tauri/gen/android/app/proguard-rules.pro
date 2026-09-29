# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# ============================================================================
# Rust(JNI) 按「字符串名 + 签名」回调 Kotlin —— 这些符号必须保留
# ============================================================================
# Rust 侧不静态引用 Kotlin，而是传字符串名去查方法（见 android_jni.rs / android_saf.rs /
# android.rs / audio/aaudio/device.rs）。R8 看不到这类引用，会改名或整个裁掉，
# release 包运行时抛 NoSuchMethodError。新增此类跨语言调用时同步加进来。
-keep class com.jdbewl.mercurial_player.SafBridge { *; }
-keep class com.jdbewl.mercurial_player.AudioBridge { *; }
-keep class com.jdbewl.mercurial_player.MediaBridge { *; }
-keep class com.jdbewl.mercurial_player.MainActivity { *; }

# Kotlin → Rust 的 external fun 由 proguard-android-optimize.txt 里的
# -keepclasseswithmembernames 兜住，不必重复声明。
