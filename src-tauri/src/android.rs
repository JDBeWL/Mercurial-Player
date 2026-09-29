//! Android 平台初始化辅助
//!
//! cpal 的 AAudio 后端（`host/aaudio/java_interface`）依赖 `ndk_context`
//! 提供的 JavaVM + Activity 指针；但 Tauri 的 Android 运行时栈（tao/wry）
//! 只会把 Activity 存进它们自己的模块，从不调用
//! [`ndk_context::initialize_android_context`]。因此这里提供一个 JNI 导出，
//! 由 Kotlin 侧 `MainActivity.onCreate` 调用，用真实的 Activity 指针完成初始化，
//! 否则 cpal 首次访问音频设备会 panic "android context was not initialized"。

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::jlong;
use std::ffi::c_void;
use std::sync::OnceLock;

/// 全局 AppHandle
///
/// Kotlin 侧（通知栏按钮 / MediaSession / 耳机线控）经 JNI 反向驱动播放时，
/// 没有 Tauri 命令上下文可用，需要一份进程级的 AppHandle 取回托管状态。
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 在 `setup` 中保存 AppHandle
pub fn set_app_handle(app: &tauri::AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
}

/// 取出全局 AppHandle（未初始化时返回 None）
pub fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// Kotlin `MainActivity.initNdkContext(activity)` 的原生实现。
///
/// # Safety
/// 由 Java/Kotlin 通过 JNI 调用：`activity` 必须为当前 Activity 的有效句柄，
/// `initialize_android_context` 的参数必须为指向存活对象的合法指针。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(unsafe_code)] // JNI 指针操作必须使用 unsafe
#[allow(clippy::missing_const_for_fn)]
pub extern "system" fn Java_com_jdbewl_mercurial_1player_MainActivity_initNdkContext(
    env: JNIEnv,
    _class: JClass,
    activity: JObject,
) {
    let Ok(vm) = env.get_java_vm() else {
        log::error!("initNdkContext: failed to obtain JavaVM");
        return;
    };
    // 转换为全局引用并泄漏：ndk_context 保存的是裸指针，必须保证指向的
    // Java 对象在进程存活期间不被 GC 回收
    let activity_global = match env.new_global_ref(&activity) {
        Ok(g) => g,
        Err(e) => {
            log::error!("initNdkContext: failed to create global ref: {e}");
            return;
        }
    };
    let vm_ptr = vm.get_java_vm_pointer().cast::<c_void>();
    let activity_ptr = activity_global.as_raw().cast::<c_void>();
    std::mem::forget(activity_global); // 有意泄漏，保持引用有效直至进程退出
    // SAFETY: vm/activity 均为系统提供的合法指针，且本次调用仅发生一次（Activity.onCreate）
    unsafe {
        ndk_context::initialize_android_context(vm_ptr, activity_ptr);
    }
    log::info!("ndk_context initialized for Android");
}
/// Kotlin `MainActivity.nativeMediaAction(action, positionMs)` 的原生实现
///
/// 通知栏按钮、MediaSession 回调、耳机线控（`ACTION_MEDIA_BUTTON`）、
/// 音频焦点丢失与 `ACTION_AUDIO_BECOMING_NOISY` 都经这里下发到 Rust 的
/// [`crate::audio::queue::media_control`]，与前端点击按钮走同一套播放逻辑。
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(clippy::missing_const_for_fn)]
pub extern "system" fn Java_com_jdbewl_mercurial_1player_MainActivity_nativeMediaAction(
    mut env: JNIEnv,
    _class: JClass,
    action: JString,
    position_ms: jlong,
) {
    let action: String = match env.get_string(&action) {
        Ok(s) => s.to_string_lossy().into_owned(),
        Err(e) => {
            log::error!("nativeMediaAction: 读取 action 失败: {e}");
            return;
        }
    };
    let Some(app) = app_handle() else {
        log::error!("nativeMediaAction: AppHandle 尚未初始化");
        return;
    };
    use tauri::Manager;
    let state = app.state::<crate::AppState>();
    let position = if position_ms > 0 {
        Some(position_ms as f32 / 1000.0)
    } else {
        None
    };
    if let Err(e) = crate::audio::queue::media_control(app, &state, &action, position) {
        log::error!("nativeMediaAction({action}) 失败: {e}");
    }
}

/// 把播放状态同步给 Kotlin 通知栏 / MediaSession
///
/// 低频调用（切歌、播放状态变化、seek）；播放进度交给
/// `PlaybackState.setState(state, position, speed, updateTime)` 由系统自行推算，
/// 不做逐帧 JNI 回调。
pub fn notify_media_session(_app: &tauri::AppHandle, state: &crate::AppState) {
    let queue = match state.player.queue.lock() {
        Ok(q) => q,
        Err(e) => {
            log::warn!("notify_media_session: 队列锁中毒: {e}");
            return;
        }
    };
    let (title, artist, album, duration_ms) = match queue.current() {
        Some(t) => (
            t.title.clone().unwrap_or_else(|| "未知曲目".to_string()),
            t.artist.clone().unwrap_or_default(),
            t.album.clone().unwrap_or_default(),
            (t.duration.unwrap_or(0.0) * 1000.0) as i64,
        ),
        None => (String::new(), String::new(), String::new(), 0),
    };
    let current_path = queue.current().map(|t| t.path.clone());
    let has_track = queue.current().is_some();
    drop(queue);

    let playing = state
        .player
        .output
        .sink
        .lock()
        .map(|sink| !sink.is_paused())
        .unwrap_or(false);

    // 通知封面：复用前端使用的封面缓存路径（可能为相对/沙箱路径）
    let cover_path = current_path
        .as_deref()
        .and_then(|p| crate::media::commands::get_track_cover_path(p.to_string()).ok())
        .flatten()
        .unwrap_or_default();

    let payload = serde_json::json!({
        "playing": playing,
        "hasTrack": has_track,
        "title": title,
        "artist": artist,
        "album": album,
        "durationMs": duration_ms,
        "positionMs": crate::audio::queue::last_position_ms(),
        "coverPath": cover_path,
    });

    if let Err(e) = crate::android_jni::jni_call_void_string(
        "com/jdbewl/mercurial_player/MediaBridge",
        "update",
        &payload.to_string(),
    ) {
        log::error!("同步播放状态到 MediaBridge 失败: {e}");
    }
}

/// Kotlin `MainActivity.nativeAudioRouteChanged()` 的原生实现
///
/// 由 `AudioBridge` 注册的 `AudioDeviceCallback` 在 USB 音频设备插拔时调用。
/// 真正的处理逻辑在 [`crate::audio::aaudio::on_audio_route_changed`]：拔掉会收流
/// 并暂停，插上只更新标志（开流发生在下一次播放，届时才能拿到曲目原生采样率）。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(clippy::missing_const_for_fn)]
pub extern "system" fn Java_com_jdbewl_mercurial_1player_MainActivity_nativeAudioRouteChanged(
    _env: JNIEnv,
    _class: JClass,
) {
    let Some(app) = app_handle() else {
        log::error!("nativeAudioRouteChanged: AppHandle 尚未初始化");
        return;
    };
    log::info!("nativeAudioRouteChanged: 输出路由变化");
    crate::audio::aaudio::on_audio_route_changed(app);
}
