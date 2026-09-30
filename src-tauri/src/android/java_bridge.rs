//! Android JNI 通用封装：`with_jni` / `app_class` 与几个静态方法调用助手，
//! 供 `saf`/`font_scale`/`system_ui`（Rust→Kotlin）与本模块 `entry`（Kotlin→Rust）共用。
//! 仅 Android 目标编译。
#![allow(unsafe_code)] // JNI 指针与 JavaVM 构造必须使用 unsafe，见各调用点 SAFETY 注释

use crate::error::AppError;

/// 取出并清除当前线程挂起的 Java 异常，返回它的 `toString`。
///
/// jni 只告诉 Rust"抛了异常"，真正的原因留在 JVM 线程的挂起异常上：不取走它会一直挂着，
/// 让本线程之后每一次 JNI 调用都莫名失败。
fn take_pending_exception(env: &mut jni::JNIEnv) -> Option<String> {
    let Ok(throwable) = env.exception_occurred() else {
        return None;
    };
    let text = (|| {
        let value = env
            .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
            .ok()?;
        let obj = value.l().ok()?;
        let s: jni::objects::JString = obj.into();
        env.get_string(&s)
            .ok()
            .map(|v| v.to_string_lossy().into_owned())
    })();
    // 读不出文本也要清，否则异常仍挂在线程上
    if let Err(e) = env.exception_clear() {
        log::warn!("清除 Java 异常失败: {e}");
    }
    text
}

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
    let result = f(&mut guard);
    if result.is_err() {
        // 异常原因写日志而不并进返回值：那条会原样透给前端 UI，可能带出本机信息
        if let Some(cause) = take_pending_exception(&mut guard) {
            log::error!("JNI 调用失败,挂起异常已清除: {cause}");
        }
    }
    result
}

/// 通过 Activity 的 ClassLoader 加载应用类
/// 不能用 `find_class`：附加线程的 FindClass 只查系统 classloader，找不到 APK 内的类。
/// `loadClass` 只认点号二进制动名，故在此把调用点写的 JNI 斜杠名转换过来。
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
        .map_err(|e| AppError::msg(format!("getClassLoader 返回类型不符: {e}")))?;

    let binary_name = name.replace('/', ".");
    let name_obj: JString = env
        .new_string(&binary_name)
        .map_err(|e| AppError::msg(format!("new_string 失败: {e}")))?;
    let name_local = JObject::from(name_obj);
    let cls = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name_local)],
        )
        .map_err(|e| AppError::msg(format!("loadClass({binary_name}) 失败: {e}")))?
        .l()
        .map_err(|e| AppError::msg(format!("loadClass({binary_name}) 返回类型不符: {e}")))?;
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

/// 调用任意类的 `(float) -> void` 静态方法，供「界面字号」使用（见 [`crate::android::font_scale`]）。
/// 用 float 签名而不是让 Kotlin 再解析一遍字符串。
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

/// 调用任意类的 `(boolean, boolean) -> void` 静态方法
pub fn jni_call_void_two_bools(
    class_name: &str,
    method: &str,
    first: bool,
    second: bool,
) -> Result<(), AppError> {
    with_jni(|env| {
        let class = app_class(env, class_name)?;
        env.call_static_method(
            &class,
            method,
            "(ZZ)V",
            &[
                jni::objects::JValue::Bool(first as u8),
                jni::objects::JValue::Bool(second as u8),
            ],
        )
        .map_err(|e| AppError::msg(format!("调用 {method} 失败: {e}")))?;
        Ok(())
    })
}
