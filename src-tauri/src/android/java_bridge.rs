//! Android JNI 通用封装：`with_jni` / `app_class` 与几个静态方法调用助手，
//! 供 `saf`/`font_scale`/`system_ui`（Rust→Kotlin）与本模块 `entry`（Kotlin→Rust）共用。
//! 仅 Android 目标编译。
#![allow(unsafe_code)] // JNI 指针与 JavaVM 构造必须使用 unsafe，见各调用点 SAFETY 注释

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use jni::objects::JObject;

use crate::error::AppError;

/// 单次 `with_jni` 里给局部引用帧的容量提示。不够时 ART 会自动扩，取宽一点是让带
/// `String[]` 参数的 SAF 调用不必扩容。
const LOCAL_FRAME_CAPACITY: i32 = 32;

/// 类名 → 类的全局引用。
///
/// 进程内不卸载类，且条目**只插不换不删**：这个不变量是 [app_class] 可以把裸句柄交出去
/// 并在锁外使用的前提——一旦被读到的 GlobalRef 若随后被 Drop，句柄就悬垂了。
static CLASS_CACHE: LazyLock<RwLock<HashMap<String, jni::objects::GlobalRef>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

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
///
/// 闭包返回的必须是 Rust 拥有的值（`String` / `i32` / `Vec`）：本函数为每次调用开一个
/// 局部引用帧并在返回时弹出，任何 Java 对象都不能跨出这个边界。
pub fn with_jni<T, F>(f: F) -> Result<T, AppError>
where
    F: FnOnce(&mut jni::JNIEnv) -> Result<T, AppError>,
{
    let ctx = ndk_context::android_context();
    // SAFETY: from_raw 按 JNI 规范使用 ndk-context 提供的有效 JavaVM 指针
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }
        .map_err(|e| AppError::msg(format!("构造 JavaVM 失败: {e}")))?;
    // 常驻挂接而不是 attach_current_thread：后者的 guard 在 drop 时真的会 detach，而 ART 的
    // DetachCurrentThread 要在全局锁下把线程移出 runtime 线程表。扫库跑在 rayon worker 上，
    // 每个曲目要付两三次，既拖慢单次调用又让"并行"扫描在这把锁上串行回去。worker 线程长命，
    // 挂住才划算；线程真的退出时 ART 会通过自己的 TLS 析构收尾。
    let mut env = vm
        .attach_current_thread_permanently()
        .map_err(|e| AppError::msg(format!("挂接 JVM 线程失败: {e}")))?;
    // 常驻之后局部引用不再随 detach 一并回收，必须自己开一帧：否则每次调用留下的
    // activity / loader / 类名串 / 返回值都会一路累积，ART 在 512 个引用附近直接 abort。
    env.push_local_frame(LOCAL_FRAME_CAPACITY)
        .map_err(|e| AppError::msg(format!("push_local_frame 失败: {e}")))?;
    let result = f(&mut env);
    if result.is_err() {
        // 异常原因写日志而不并进返回值：那条会原样透给前端 UI，可能带出本机信息
        if let Some(cause) = take_pending_exception(&mut env) {
            log::error!("JNI 调用失败,挂起异常已清除: {cause}");
        }
    }
    // SAFETY: 帧内创建的局部引用没有一个逃出闭包（返回的是 Rust 拥有的值），弹出时不欠外层
    // 对象，因此传 null。这里不把它 Err 化：本函数若在此早退，这一帧就永远留在栈上了。
    if let Err(e) = unsafe { env.pop_local_frame(&JObject::null()) } {
        log::warn!("pop_local_frame 失败: {e}");
    }
    result
}

/// 通过 Context 的 ClassLoader 加载应用类，并按类名缓存成全局引用
///
/// 不能用 `find_class`：附加线程的 FindClass 只查系统 classloader，找不到 APK 内的类。
/// `loadClass` 只认点号二进制动名，故在此把调用点写的 JNI 斜杠名转换过来。
///
/// 不缓存的话每次调用都要走一遍 `getClassLoader` → `new_string` → `loadClass`：这两个是
/// 真正的 Java 方法调用，而 `ClassLoader.loadClass` 内部按类名 `synchronized`，并行扫库时
/// 多个 worker 会串在同一个类上。方法 ID 仍每次由 `call_static_method` 现查——类已解析之后
/// 那只是 ART 的一次廉价查找，为它再加一层缓存不值得。
pub fn app_class<'env>(
    env: &mut jni::JNIEnv<'env>,
    name: &str,
) -> Result<jni::objects::JClass<'env>, AppError> {
    use jni::objects::{JClass, JString, JValue};

    // 读锁只用来取裸指针，不在持锁期间做任何 JNI 调用
    let cached = lock_or_log!(CLASS_CACHE.read())
        .get(name)
        .map(|global| global.as_raw());
    if let Some(raw) = cached {
        // SAFETY: 缓存的全局引用只插不换不删，句柄在进程存活期内一直有效；全局引用不属于
        // 任何局部帧，所以按 'env 使用不会随 with_jni 那一帧弹出而失效。
        return Ok(JClass::from(unsafe { JObject::from_raw(raw) }));
    }

    let ctx = ndk_context::android_context();
    // SAFETY: entry.rs 初始化 ndk_context 时泄漏的 Application Context 全局引用在进程存活期有效
    let context_raw = ctx.context() as jni::sys::jobject;
    let context = unsafe { JObject::from_raw(context_raw) };

    let loader = env
        .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
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
    let class = JClass::from(cls);

    let global = env
        .new_global_ref(&class)
        .map_err(|e| AppError::msg(format!("缓存类引用失败: {e}")))?;
    // 必须用 `or_insert` 而不是 `insert`：条目一旦被别的线程读到过，它的裸句柄就可能正在
    // 使用中，覆盖旧值会连带 Drop 掉那个 GlobalRef（即 DeleteGlobalRef），留下悬垂引用。
    // or_insert 在已有条目时丢弃的是我们刚建的、还没交给任何人的 global。
    lock_or_log!(CLASS_CACHE.write())
        .entry(name.to_string())
        .or_insert(global);
    Ok(class)
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
