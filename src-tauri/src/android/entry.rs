//! Android 平台初始化辅助
//!
//! Tauri 的 Android 运行时栈（tao/wry）不初始化 `ndk_context`，而 cpal 的 AAudio 后端依赖它；
//! 不初始化时 cpal 首次访问音频设备会 panic "android context was not initialized"。
//! `initNdkContext` 由 MainActivity 调起，传 Application Context，并保证进程内只初始化一次。

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jlong};
use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};

/// 全局 AppHandle
/// Kotlin 侧（通知栏按钮 / MediaSession / 耳机线控）经 JNI 反向驱动播放时没有命令上下文可用。
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// 在 `setup` 中保存 AppHandle
pub fn set_app_handle(app: &tauri::AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
}

/// 取出全局 AppHandle（未初始化时返回 None）
pub fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// 进程级一次性初始化标志。
///
/// `singleTask` 只保证同一时刻一个 Activity 实例：进程被前台服务保活时，Activity 关闭后
/// 重新打开会在同一进程里再走一遍 `onCreate` → `initNdkContext`。而 ndk-context 0.1.1 的
/// `initialize_android_context` 在重复调用时 `assert!(previous.is_none())` 直接 panic，
/// release 构建（`panic = "abort"`）下即整进程终止，因此必须在 Rust 侧保证只初始化一次。
static NDK_CONTEXT_INIT: Once = Once::new();

/// Kotlin `MainActivity.initNdkContext(context)` 的原生实现。
///
/// Kotlin 侧传的是 **Application Context**：ndk_context 保存的裸指针会一直活到进程退出，
/// 不能持有随时可能被重建的 Activity（否则既泄漏已销毁的 Activity，重建后又触发重复初始化）。
///
/// # Safety
/// 由 Java/Kotlin 通过 JNI 调用：`context` 必须为有效句柄，
/// `initialize_android_context` 的参数必须为指向存活对象的合法指针。
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(unsafe_code)] // JNI 指针操作必须使用 unsafe
#[allow(clippy::missing_const_for_fn)]
pub extern "system" fn Java_com_jdbewl_mercurial_1player_MainActivity_initNdkContext(
    env: JNIEnv,
    _class: JClass,
    context: JObject,
) {
    // Activity 重建时直接跳过：JavaVM 与 Application Context 在进程内都不会变
    if NDK_CONTEXT_INIT.is_completed() {
        log::info!("initNdkContext: 已初始化（Activity 重建），跳过");
        return;
    }
    let Ok(vm) = env.get_java_vm() else {
        log::error!("initNdkContext: failed to obtain JavaVM");
        return;
    };
    // 转换为全局引用并泄漏：ndk_context 保存的是裸指针，必须保证指向的
    // Context 对象在进程存活期间不被 GC 回收
    let context_global = match env.new_global_ref(&context) {
        Ok(g) => g,
        Err(e) => {
            log::error!("initNdkContext: failed to create global ref: {e}");
            return;
        }
    };
    let vm_ptr = vm.get_java_vm_pointer().cast::<c_void>();
    let context_ptr = context_global.as_raw().cast::<c_void>();
    std::mem::forget(context_global); // 有意泄漏，保持引用有效直至进程退出
    // call_once 而非裸调用：即使 onCreate 之外还有调用点并发进来，也只有一次真正执行初始化；
    // 竞争失败者多泄漏一个全局引用（本身即有意泄漏，无害），但绝不会二次触发断言
    NDK_CONTEXT_INIT.call_once(|| {
        // SAFETY: vm/context 均为系统提供的合法指针，且 call_once 保证进程内仅执行一次
        unsafe {
            ndk_context::initialize_android_context(vm_ptr, context_ptr);
        }
        log::info!("ndk_context initialized for Android (application context)");
    });
}

/// Kotlin `MainActivity.nativeMediaAction(action, positionMs)` 的原生实现
/// 通知栏按钮、MediaSession 回调、耳机线控、音频焦点丢失与 `ACTION_AUDIO_BECOMING_NOISY`
/// 都经这里下发到 [`crate::audio::queue::media_control`]，与前端点按钮走同一套逻辑。
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
    // 按动作解析参数：只有 seek 需要目标位置，0 是合法值（跳回曲首，外部控制器
    // 调 onSeekTo(0) 很常见），负值才非法；其余动作一律不携带位置
    let position = match action.as_str() {
        "seek" => {
            if position_ms < 0 {
                log::error!("nativeMediaAction: seek 位置非法({position_ms}ms)");
                return;
            }
            Some(position_ms as f32 / 1000.0)
        }
        _ => None,
    };
    if let Err(e) = crate::audio::queue::media_control(app, &state, &action, position) {
        log::error!("nativeMediaAction({action}) 失败: {e}");
    }
}

/// 上一次推给 MediaSession 的 `(曲目路径, 封面路径)`。
///
/// 播放/暂停/seek 都不换曲，封面不可能变，而这类事件远比切歌频繁。省掉重取的不只是
/// 标签解析：取封面要先比一次文件 mtime，而 content URI 的 mtime 要经 SAF 打开 fd，
/// 那趟 JNI 往返发生在主线程上（MediaSession 回调默认走主 looper）。
static LAST_COVER: Mutex<Option<(String, String)>> = Mutex::new(None);

/// 把播放状态同步给 Kotlin 通知栏 / MediaSession，只在状态变化时低频调用（切歌/播放/seek）。
/// 进度交给 `PlaybackState.setState(state, position, speed, updateTime)` 由系统自行推算，不逐帧回调。
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

    // 必须按实际输出模式判断：独占模式下共享 sink 一直是暂停的，只看它会让 MediaSession
    // 永远报告 PAUSED，于是框架把每次 PLAY_PAUSE 都翻成 onPlay，对已启动的流反复 requestStart
    let playing = crate::audio::commands::is_output_playing(state);

    // 通知封面：复用前端使用的封面缓存路径（可能为相对/沙箱路径）
    let cover_path = match current_path.as_deref() {
        None => String::new(),
        Some(path) => {
            // 空串表示"这首没封面"，同样是有效记忆；有封面的那条要确认文件还在，
            // 封面缓存会按体积自动淘汰，不能把已被清掉的路径推给通知栏
            let remembered = lock_or_log!(LAST_COVER.lock())
                .as_ref()
                .filter(|(last, cover)| {
                    last.as_str() == path && (cover.is_empty() || Path::new(cover).exists())
                })
                .map(|(_, cover)| cover.clone());
            if let Some(cover) = remembered {
                cover
            } else {
                let cover = crate::media::commands::get_track_cover_path(path.to_string())
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                *lock_or_log!(LAST_COVER.lock()) = Some((path.to_string(), cover.clone()));
                cover
            }
        }
    };

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

    if let Err(e) = crate::android::java_bridge::jni_call_void_string(
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
/// 处理逻辑在 [`crate::audio::aaudio::on_audio_route_changed`]：拔掉会收流并暂停，
/// 插上则当场建好独占流（设置页要立刻反映"独占已生效"）。
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

/// Kotlin `MainActivity.nativeSetForeground(foreground)` 的原生实现
///
/// 频谱门控里"应用是否可见"这一路必须由 Activity 生命周期来写：横屏看着波形退到后台时
/// 组件并不会卸载，而 wry 在 `onPause` 里就调了 `mWebView.onPause()` 冻结 JS —— 前端既
/// 来不及关，回到前台也不会主动补一句"我又可见了"。见 [`crate::audio::spectrum::SpectrumGate`]。
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(clippy::missing_const_for_fn)]
pub extern "system" fn Java_com_jdbewl_mercurial_1player_MainActivity_nativeSetForeground(
    _env: JNIEnv,
    _class: JClass,
    foreground: jboolean,
) {
    let Some(app) = app_handle() else {
        log::error!("nativeSetForeground: AppHandle 尚未初始化");
        return;
    };
    use tauri::Manager;
    app.state::<crate::AppState>()
        .player
        .visualization
        .spectrum_gate
        .set_foreground(foreground != 0);
}
