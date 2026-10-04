package com.jdbewl.mercurial_player

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject

/**
 * Rust → Kotlin 的播放状态同步入口：Rust 侧 `android/entry.rs::notify_media_session` 经 JNI 调 [update]。
 * 调用线程可能是分析线程或解码线程，因此统一 post 到主线程再操作 MediaSession/通知。
 * 这里只做「状态同步 + 起停服务」，播放控制与音频句柄都留在 Rust 侧。
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
    val state =
      runCatching { JSONObject(payload) }.getOrElse { e ->
        // 静默返回的话，Rust 侧改字段名/发错内容会表现为"通知栏完全不更新"，
        // 而应用内一切正常，无法排查问题
        android.util.Log.e(TAG, "播放状态 JSON 解析失败，本次同步被丢弃: ${e.message}")
        return
      }
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
          // 防御性设计
          if (PlaybackService.isRunning()) PlaybackService.stop(context)
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
