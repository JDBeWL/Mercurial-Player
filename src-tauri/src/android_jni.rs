//! Android JNI 通用封装
//!
//! 供 `android_saf`（Rust→Kotlin 调用）与 `android`（Kotlin→Rust 导出 + 通知同步）
//! 共用。`android_saf` 原先内联了 `with_jni` / `app_class`，阶段 3 的通知与
//! MediaSession 同步同样需要它们，因此提到本模块。
//!
//! 仅 Android 目标编译。
#![allow(unsafe_code)] // JNI 指针与 JavaVM 构造必须使用 unsafe，见各调用点 SAFETY 注释

use crate::error::AppError;

/// 在挂接到当前线程的 JNIEnv 内执行闭包，规避 JNIEnv 借用逃逸问题
pub fn with_jni<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce(&mut jni::JNIEnv) -> Result<T, AppError>,
{
    let ctx = ndk_context::android_context();
    // SAFETY: from_raw 与 attach 均按 JNI 规范使用 ndk-context 提供的有效指针
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| AppError::msg(format!("构造 JavaVM 失败: {e}")))?;
    let mut guard = vm
        .attach_current_thread()
        .map_err(|e| AppError::msg(format!("attach JVM 线程失败: {e}")))?;
    f(&mut guard)
}

/// 通过 Activity 的 ClassLoader 加载应用类
///
/// 不能直接用 `JNIEnv::find_class`：JNI 附加线程的 `FindClass` 只查系统
/// classloader，找不到 APK 内的应用类。需从 ndk_context 保存的 Activity
/// （全局引用，进程存活期有效）出发取应用 ClassLoader 再 `loadClass`。
pub fn app_class<'env>(
    env: &mut jni::JNIEnv<'env>,
    name: &str,
) -> Result<jni::objects::JClass<'env>, AppError> {
    use jni::objects::{JObject, JString, JValue};

    let ctx = ndk_context::android_context();
    // SAFETY: android.rs 初始化 ndk_context 时泄漏的 Activity 全局引用在进程存活期有效
    let activity_raw = ctx.context() as jni::sys::jobject;
    let activity = unsafe { JObject::from_raw(activity_raw) };

    let loader = env
        .call_method(
            &activity,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .map_err(|e| AppError::msg(format!("getClassLoader 失败: {e}")))?
        .l()
        .map_err(|_| AppError::msg("getClassLoader 返回类型不符"))?;

    let name_obj: JString = env
        .new_string(name)
        .map_err(|e| AppError::msg(format!("new_string 失败: {e}")))?;
    let name_local = JObject::from(name_obj);
    let cls = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name_local)],
        )
        .map_err(|e| AppError::msg(format!("loadClass({name}) 失败: {e}")))?
        .l()
        .map_err(|_| AppError::msg(format!("loadClass({name}) 返回类型不符")))?;
    Ok(jni::objects::JClass::from(cls))
}

/// 调用无参、`()Ljava/lang/String;` 签名的静态方法，取回返回的字符串
///
/// 供 AAudio 侧读取 Kotlin 组装好的设备 JSON（见 `audio::aaudio::device`）。
pub fn jni_call_static_string(class_name: &str, method: &str) -> Result<String, AppError> {
    use jni::objects::JObject;

    with_jni(|env| {
        let class = app_class(env, class_name)?;
        let ret = env
            .call_static_method(&class, method, "()Ljava/lang/String;", &[])
            .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
        let obj: JObject = ret
            .l()
            .map_err(|e| AppError::msg(format!("{method} 返回类型不符: {e}")))?;
        if obj.is_null() {
            return Ok(String::new());
        }
        let s: jni::objects::JString = obj.into();
        env.get_string(&s)
            .map(|v| v.to_string_lossy().to_string())
            .map_err(|e| AppError::msg(format!("{method} 读取字符串失败: {e}")))
    })
}

/// 调用任意类的 `(String) -> void` 静态方法
///
/// 用于把播放状态以 JSON 形式同步给 Kotlin（通知栏 / MediaSession）。
pub fn jni_call_void_string(class_name: &str, method: &str, arg: &str) -> Result<(), AppError> {
    use jni::objects::JValue;

    with_jni(|env| {
        let class = app_class(env, class_name)?;
        let js = env
            .new_string(arg)
            .map_err(|e| AppError::msg(format!("new_string 失败: {e}")))?;
        env.call_static_method(
            &class,
            method,
            "(Ljava/lang/String;)V",
            &[JValue::Object(&js)],
        )
        .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
        Ok(())
    })
}

/// 调用任意类的 `(float) -> void` 静态方法
///
/// 供「界面字号」使用（见 [`crate::app_font_scale`]）：倍率是浮点数，
/// 走 float 签名比让 Kotlin 再解析一遍字符串干净。
pub fn jni_call_void_float(class_name: &str, method: &str, value: f32) -> Result<(), AppError> {
    with_jni(|env| {
        let class = app_class(env, class_name)?;
        env.call_static_method(
            &class,
            method,
            "(F)V",
            &[jni::objects::JValue::Float(value)],
        )
        .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
        Ok(())
    })
}
