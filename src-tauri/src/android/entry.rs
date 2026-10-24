//! Android 平台初始化辅助
//!
//! tao/wry 不初始化 `ndk_context`，cpal 的 AAudio 后端依赖它；`MainActivity` 必须传 Application Context。

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jlong};
use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, Once, OnceLock};

/// 全局 AppHandle：Kotlin 侧（通知栏 / MediaSession / 耳机线控）经 JNI 反向驱动播放时没有命令上下文
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn set_app_handle(app: &tauri::AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
}

pub fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

/// `ndk-context` 只能初始化一次，重复初始化会 `assert!(previous.is_none())` panic（release 的 `panic = "abort"` 直接杀进程）；
/// `singleTask` 只保证同一时刻一个 Activity，进程被前台服务保活时 Activity 重开会再走一遍 `onCreate`。
static NDK_CONTEXT_INIT: Once = Once::new();

/// Kotlin `MainActivity.initNdkContext(context)` 的原生实现；`context` 必须是 Application Context（裸指针要活到进程退出，持有 Activity 会泄漏并触发重复初始化）。
///
/// # Safety
/// JNI 调用传入的 `context` 必须是有效且长期存活的句柄。
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
    // 转全局引用并泄漏：裸指针指向的 Context 必须在进程存活期内不被 GC 回收
    let context_global = match env.new_global_ref(&context) {
        Ok(g) => g,
        Err(e) => {
            log::error!("initNdkContext: failed to create global ref: {e}");
            return;
        }
    };
    let vm_ptr = vm.get_java_vm_pointer().cast::<c_void>();
    let context_ptr = context_global.as_raw().cast::<c_void>();
    std::mem::forget(context_global);
    // `call_once` 让并发调用只初始化一次；竞争失败者会多泄漏一个全局引用（无害），但不会二次触发断言
    NDK_CONTEXT_INIT.call_once(|| {
        // SAFETY: vm/context 均为系统提供的合法指针，且 call_once 保证进程内仅执行一次
        unsafe {
            ndk_context::initialize_android_context(vm_ptr, context_ptr);
        }
        log::info!("ndk_context initialized for Android (application context)");
    });
}

/// Kotlin `MainActivity.nativeMediaAction(action, positionMs)` 的原生实现。
/// 通知栏、MediaSession 回调、线控与音频焦点变化都经这里下发到 `crate::audio::queue::media_control`。
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
    // 只有 seek 需要位置，0 合法（外部控制器常调 `onSeekTo(0)`），负值非法；其余动作不携带位置
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

/// 上一次推给 MediaSession 的 `(曲目路径, 封面路径)`：播放/暂停/seek 不换曲，缓存省掉重取。
/// 取封面要打开 SAF fd，这趟 JNI 往返绝不能发生在 MediaSession 回调线程上（主 looper，慢 provider 直接 ANR）。
static LAST_COVER: Mutex<Option<(String, String)>> = Mutex::new(None);

/// 正在后台解析封面的曲目路径，用于去重：同一首曲目只提交一次解析
static COVER_RESOLVING: Mutex<Option<String>> = Mutex::new(None);

/// 一次状态同步的廉价字段（不含封面路径解析，那一步见 `cover_path_for`）
struct MediaSnapshot {
    playing: bool,
    has_track: bool,
    title: String,
    artist: String,
    album: String,
    duration_ms: i64,
    path: Option<String>,
}

/// 采集当前播放状态。返回 `None` 表示队列锁中毒（原因已写日志）。
fn collect_media_state(state: &crate::AppState) -> Option<MediaSnapshot> {
    let current = match state.player.queue.lock() {
        Ok(queue) => queue.current().cloned(),
        Err(e) => {
            log::warn!("notify_media_session: 队列锁中毒: {e}");
            return None;
        }
    };
    // 刻意在释放队列锁之后才问 is_output_playing：持锁期间调用会给锁序多添一条隐式依赖
    let playing = crate::audio::commands::is_output_playing(state);
    let has_track = current.is_some();
    let (title, artist, album, duration_ms, path) = match current {
        Some(t) => (
            t.title.clone().unwrap_or_else(|| "未知曲目".to_string()),
            t.artist.clone().unwrap_or_default(),
            t.album.clone().unwrap_or_default(),
            (t.duration.unwrap_or(0.0) * 1000.0) as i64,
            Some(t.path),
        ),
        None => (String::new(), String::new(), String::new(), 0, None),
    };
    Some(MediaSnapshot {
        playing,
        has_track,
        title,
        artist,
        album,
        duration_ms,
        path,
    })
}

/// 把一次快照推给 Kotlin（通知栏 / MediaSession）
/// 把一次快照推给 Kotlin（通知栏 / MediaSession）
fn push_media_state(snapshot: &MediaSnapshot, cover_path: &str) {
    let payload = serde_json::json!({
        "playing": snapshot.playing,
        "hasTrack": snapshot.has_track,
        "title": snapshot.title,
        "artist": snapshot.artist,
        "album": snapshot.album,
        "durationMs": snapshot.duration_ms,
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

/// 封面路径：命中 [`LAST_COVER`] 直接返回；未命中先返回空串、后台解析完再补推
fn cover_path_for(app: &tauri::AppHandle, path: Option<&str>) -> String {
    let Some(path) = path else {
        return String::new();
    };
    // 空串表示"这首没封面"，封面缓存会按体积自动淘汰
    let remembered = lock_or_log!(LAST_COVER.lock())
        .as_ref()
        .filter(|(last, cover)| {
            last.as_str() == path && (cover.is_empty() || Path::new(cover).exists())
        })
        .map(|(_, cover)| cover.clone());
    if let Some(cover) = remembered {
        return cover;
    }
    spawn_cover_resolve(app.clone(), path.to_string());
    String::new()
}

/// 在阻塞线程池里解析封面，成功后写回 [`LAST_COVER`] 并补推一次状态；同一首只会有一个在途解析。
///
/// 写回前确认当前曲目仍是它，否则快速切歌时上一首的封面会盖掉新曲。
fn spawn_cover_resolve(app: tauri::AppHandle, path: String) {
    {
        let mut in_flight = lock_or_log!(COVER_RESOLVING.lock());
        if in_flight.as_deref() == Some(path.as_str()) {
            return;
        }
        *in_flight = Some(path.clone());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = crate::media::commands::get_track_cover_path(path.clone())
            .ok()
            .flatten()
            .unwrap_or_default();
        *lock_or_log!(COVER_RESOLVING.lock()) = None;
        *lock_or_log!(LAST_COVER.lock()) = Some((path.clone(), resolved.clone()));

        use tauri::Manager;
        let state = app.state::<crate::AppState>();
        let Some(snapshot) = collect_media_state(&state) else {
            return;
        };
        if snapshot.path.as_deref() == Some(path.as_str()) {
            push_media_state(&snapshot, &resolved);
        }
    });
}

/// 把播放状态同步给 Kotlin 通知栏 / MediaSession，只在状态变化时低频调用（切歌/播放/seek）。
///
/// 进度交给 `PlaybackState.setState(state, position, speed, updateTime)` 由系统自行推算，不逐帧回调。
pub fn notify_media_session(app: &tauri::AppHandle, state: &crate::AppState) {
    let Some(snapshot) = collect_media_state(state) else {
        return;
    };
    let cover_path = cover_path_for(app, snapshot.path.as_deref());
    push_media_state(&snapshot, &cover_path);
}

/// Kotlin `MainActivity.nativeAudioRouteChanged()` 的原生实现。
///
/// 由 `AudioBridge` 注册的 `AudioDeviceCallback` 在 USB 音频设备插拔时调用，处理见
/// [`crate::audio::aaudio::on_audio_route_changed`]：拔掉收流并暂停，插上当场建好独占流。
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

/// Kotlin `nativeSetForeground(foreground)` 的原生实现。
/// 可见性只能由 Activity 生命周期写入：退到后台时 wry 已冻结 JS，前端补不了。
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
