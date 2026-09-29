package com.jdbewl.mercurial_player

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

/**
 * Rust → Kotlin 的播放状态同步入口（阶段 3.3 的 Rust→Kotlin 方向）。
 *
 * Rust 侧 `android.rs::notify_media_session` 通过 JNI 调用 [update]，调用线程
 * 可能是分析线程或解码线程，因此统一 post 到主线程再操作 MediaSession/通知。
 *
 * 注意：这里只做「状态同步 + 起停服务」，真正的播放控制仍然在 Rust 侧，
 * Kotlin 不持有任何音频句柄（见 MOBILE_ANDROID_PLAN.md 阶段 3 的修正说明）。
 */
object MediaBridge {
  private const val TAG = "MediaBridge"

  @Volatile
  private var appContext: Context? = null
  private val mainHandler = Handler(Looper.getMainLooper())

  fun init(context: Context) {
    appContext = context.applicationContext
  }

  /** 由 Rust 调用：播放状态变化（切歌 / 播放/暂停 / seek）时同步一次 */
  @JvmStatic
  fun update(payload: String) {
    val context = appContext ?: return
    val state = runCatching { JSONObject(payload) }.getOrElse { return }
    mainHandler.post {
      runCatching {
        val hasTrack = state.optBoolean("hasTrack", false)
        val playing = state.optBoolean("playing", false)
        val title = state.optString("title")
        val artist = state.optString("artist")
        val album = state.optString("album")
        val durationMs = state.optLong("durationMs", 0L)
        val positionMs = state.optLong("positionMs", 0L)
        val coverPath = state.optString("coverPath", "")

        if (!hasTrack && !playing) {
          PlaybackService.stop(context)
          return@post
        }

        PlaybackService.ensureStarted(
          context = context,
          title = title,
          artist = artist,
          album = album,
          durationMs = durationMs,
          positionMs = positionMs,
          playing = playing,
          coverPath = coverPath,
        )
      }.onFailure { e ->
        android.util.Log.e(TAG, "update failed: ${e.message}")
      }
    }
  }
}
