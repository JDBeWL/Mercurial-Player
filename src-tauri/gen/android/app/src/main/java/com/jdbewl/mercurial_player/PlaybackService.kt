package com.jdbewl.mercurial_player

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.media.app.NotificationCompat.MediaStyle
import androidx.media.session.MediaButtonReceiver
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * 后台播放前台服务：只保活进程 + 托管 MediaSession/通知，不持有任何音频句柄（解码与输出都在 Rust 侧）。
 * 播放中以 `mediaPlayback` 类型常驻前台；暂停时保留通知但退出前台，用户仍可从通知恢复；
 * 无曲目时由 [MediaBridge] 调 [stop] 停止服务并释放 MediaSession。
 */
class PlaybackService : Service() {
  companion object {
    private const val TAG = "PlaybackService"
    private const val CHANNEL_ID = "mercurial_playback"
    private const val NOTIFICATION_ID = 0x5A1D
    private const val REQ_CODE = 0x5A1D

    const val ACTION_PLAY = "com.jdbewl.mercurial_player.ACTION_PLAY"
    const val ACTION_PAUSE = "com.jdbewl.mercurial_player.ACTION_PAUSE"
    const val ACTION_NEXT = "com.jdbewl.mercurial_player.ACTION_NEXT"
    const val ACTION_PREVIOUS = "com.jdbewl.mercurial_player.ACTION_PREVIOUS"
    const val ACTION_STOP = "com.jdbewl.mercurial_player.ACTION_STOP"

    @Volatile
    private var running = false

    /** 启动（或更新）服务。必须在 Activity 处于前台时首次调用，见 Android 12+ 限制。 */
    fun ensureStarted(
      context: Context,
      title: String,
      artist: String,
      album: String,
      durationMs: Long,
      positionMs: Long,
      playing: Boolean,
      coverPath: String = "",
    ) {
      val intent =
        Intent(context, PlaybackService::class.java).apply {
          putExtra("title", title)
          putExtra("artist", artist)
          putExtra("album", album)
          putExtra("durationMs", durationMs)
          putExtra("positionMs", positionMs)
          putExtra("playing", playing)
          putExtra("coverPath", coverPath)
        }
      if (running) {
        // 服务已在跑：仍走 onStartCommand 更新通知与 MediaSession
        runCatching { ContextCompat.startForegroundService(context, intent) }
          .onFailure { e -> android.util.Log.w(TAG, "startForegroundService 被拒绝: ${e.message}") }
        return
      }
      runCatching {
        ContextCompat.startForegroundService(context, intent)
      }.onFailure { e ->
        android.util.Log.w(TAG, "startForegroundService 被拒绝: ${e.message}")
      }
    }

    fun stop(context: Context) {
      runCatching { context.stopService(Intent(context, PlaybackService::class.java)) }
    }
  }

  private lateinit var notificationManager: NotificationManager
  private lateinit var audioManager: AudioManager
  private var mediaSession: MediaSessionCompat? = null
  private var focusRequest: AudioFocusRequest? = null

  /** 是否**真正持有**音频焦点（requestAudioFocus 返回值）。focusRequest 非空只表示请求对象已构建 */
  private var focusGranted = false
  private val mainHandler = Handler(Looper.getMainLooper())

  /**
   * 媒体命令串行后台执行器。dispatchToRust 的 JNI 调用是同步的：切歌/seek 会打开文件、
   * 建解码器并预填充缓冲，慢存储或慢文档提供者上可能秒级；主线程（MediaSession 回调 /
   * 广播接收器 / 焦点回调）只负责提交命令，避免控制迟滞与 ANR。串行也保证命令按到达顺序执行。
   */
  private val commandExecutor: ExecutorService =
    Executors.newSingleThreadExecutor { r -> Thread(r, "media-command") }

  /** 通知封面读取/解码执行器（content:// 要经 provider，可能很慢，不能占主线程） */
  private val coverExecutor: ExecutorService =
    Executors.newSingleThreadExecutor { r -> Thread(r, "cover-load") }

  /** 当前封面位图及其来源路径：只缓存"当前这一首"，避免位图累积 */
  private var coverBitmap: Bitmap? = null
  private var coverBitmapPath: String? = null

  /** 正在后台加载的封面路径，避免同一路径重复提交 */
  private var coverLoadingPath: String? = null

  private var title = ""
  private var artist = ""
  private var album = ""
  private var durationMs = 0L
  private var positionMs = 0L
  private var playing = false
  private var hasTrack = false
  private var coverPath = ""

  /** 耳机/蓝牙断开 → 暂停 */
  private val noisyReceiver =
    object : BroadcastReceiver() {
      override fun onReceive(context: Context?, intent: Intent?) {
        if (intent?.action != AudioManager.ACTION_AUDIO_BECOMING_NOISY) return
        android.util.Log.i(TAG, "AUDIO_BECOMING_NOISY -> pause")
        dispatchToRust("pause", 0L)
      }
    }

  private val focusChangeListener =
    AudioManager.OnAudioFocusChangeListener { change ->
      when (change) {
        AudioManager.AUDIOFOCUS_LOSS,
        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT,
        -> {
          // 焦点已不在我们手里：下次播放必须重新申请，否则会一直以为还持有焦点
          focusGranted = false
          android.util.Log.i(TAG, "audio focus lost ($change) -> pause")
          dispatchToRust("pause", 0L)
        }
        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
          // 短暂可混音（如导航播报）：这里选择保持播放，由系统侧压低音量
          android.util.Log.i(TAG, "audio focus duck -> keep playing")
        }
      }
    }

  override fun onCreate() {
    super.onCreate()
    notificationManager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
    audioManager = getSystemService(AUDIO_SERVICE) as AudioManager
    createChannel()
    setupMediaSession()
    registerNoisyReceiver()
    running = true
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent == null && !hasTrack) {
      // 防御分支：空 Intent 且没有可恢复的会话（进程刚重建，Rust 侧播放器与全局 AppHandle
      // 都未初始化）时，建通知只会得到"未知曲目"通知与无效的媒体按钮，直接停掉
      android.util.Log.i(TAG, "空 Intent 且无有效会话，停止服务")
      stopSelf()
      return START_NOT_STICKY
    }
    if (intent != null) {
      // ACTION_MEDIA_BUTTON 由 MediaButtonReceiver 统一处理（耳机线控）
      if (intent.action == Intent.ACTION_MEDIA_BUTTON) {
        MediaButtonReceiver.handleIntent(mediaSession, intent)
      }
      when (intent.action) {
        ACTION_PLAY -> if (ensureFocusForPlayback()) dispatchToRust("play", 0L)
        ACTION_PAUSE -> dispatchToRust("pause", 0L)
        ACTION_NEXT -> dispatchToRust("next", 0L)
        ACTION_PREVIOUS -> dispatchToRust("previous", 0L)
        ACTION_STOP -> {
          dispatchToRust("stop", 0L)
          stopSelf()
          return START_NOT_STICKY
        }
      }
      title = intent.getStringExtra("title") ?: title
      artist = intent.getStringExtra("artist") ?: artist
      album = intent.getStringExtra("album") ?: album
      durationMs = intent.getLongExtra("durationMs", durationMs)
      positionMs = intent.getLongExtra("positionMs", 0L)
      playing = intent.getBooleanExtra("playing", playing)
      coverPath = intent.getStringExtra("coverPath") ?: coverPath
      hasTrack = true
    }

    val notification = buildNotification()
    // 每次 startForegroundService 都必须在 5 秒内 startForeground，暂停态也不例外；
    // 先入前台兑现契约，再按状态决定是否退出。
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      startForeground(
        NOTIFICATION_ID,
        notification,
        android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK,
      )
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
    if (playing) {
      requestAudioFocus()
    } else {
      // 暂停时保留通知但退出前台，用户仍可从通知恢复播放
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
        stopForeground(STOP_FOREGROUND_DETACH)
      } else {
        @Suppress("DEPRECATION")
        stopForeground(false)
      }
      notificationManager.notify(NOTIFICATION_ID, notification)
      abandonAudioFocus()
    }
    updateMediaSession()
    // 不用 START_STICKY：进程死亡后 Rust 侧的播放器与全局 AppHandle 都随进程消失，
    // 服务自身没有重建播放器的入口，粘性重启只会留下"未知曲目"通知与无效按键
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    abandonAudioFocus()
    runCatching { unregisterReceiver(noisyReceiver) }
    mediaSession?.release()
    mediaSession = null
    // shutdown（而非 shutdownNow）：已提交但未执行的命令要继续跑完，"停止播放"不能丢
    commandExecutor.shutdown()
    coverExecutor.shutdown()
    running = false
    super.onDestroy()
  }

  override fun onBind(intent: Intent?) = null

  private fun createChannel() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    if (notificationManager.getNotificationChannel(CHANNEL_ID) != null) return
    val channel =
      NotificationChannel(CHANNEL_ID, "播放控制", NotificationManager.IMPORTANCE_LOW).apply {
        description = "后台播放控制"
        setShowBadge(false)
      }
    notificationManager.createNotificationChannel(channel)
  }

  private fun setupMediaSession() {
    val session =
      MediaSessionCompat(this, "MercurialPlayer").apply {
        setCallback(
          object : MediaSessionCompat.Callback() {
            override fun onPlay() {
              if (ensureFocusForPlayback()) dispatchToRust("play", 0L)
            }

            override fun onPause() = dispatchToRust("pause", 0L)

            override fun onStop() {
              dispatchToRust("stop", 0L)
              stopSelf()
            }

            override fun onSkipToNext() = dispatchToRust("next", 0L)

            override fun onSkipToPrevious() = dispatchToRust("previous", 0L)

            override fun onSeekTo(pos: Long) = dispatchToRust("seek", pos)
          },
        )
        // 刻意不调用 setFlags(FLAG_HANDLES_MEDIA_BUTTONS | FLAG_HANDLES_TRANSPORT_CONTROLS)：
        // 两个 flag 自 API 21 起已废弃，媒体按钮与传输控制由框架自动路由到活跃的 MediaSession；
        // 本项目 minSdk=26，传了也只是 no-op 外加两条 deprecation 警告。
        isActive = true
      }
    mediaSession = session
  }

  private fun updateMediaSession() {
    val session = mediaSession ?: return
    session.setMetadata(
      MediaMetadataCompat.Builder()
        .putString(MediaMetadataCompat.METADATA_KEY_TITLE, title.ifEmpty { "未知曲目" })
        .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, artist)
        .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, album)
        .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
        .build(),
    )
    val state =
      if (playing) PlaybackStateCompat.STATE_PLAYING else PlaybackStateCompat.STATE_PAUSED
    val actions =
      PlaybackStateCompat.ACTION_PLAY or
        PlaybackStateCompat.ACTION_PAUSE or
        PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
        PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
        PlaybackStateCompat.ACTION_SEEK_TO or
        PlaybackStateCompat.ACTION_PLAY_PAUSE
    session.setPlaybackState(
      PlaybackStateCompat.Builder()
        .setActions(actions)
        .setState(state, positionMs, if (playing) 1.0f else 0.0f, SystemClock.elapsedRealtime())
        .build(),
    )
  }

  private fun buildNotification(): Notification {
    val contentIntent =
      PendingIntent.getActivity(
        this,
        REQ_CODE,
        packageManager.getLaunchIntentForPackage(packageName)?.apply {
          flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        },
        PendingIntent.FLAG_UPDATE_CURRENT or immutableFlag(),
      )

    val style =
      MediaStyle()
        .setMediaSession(mediaSession?.sessionToken)
        .setShowActionsInCompactView(0, 1, 2)

    val builder =
      NotificationCompat.Builder(this, CHANNEL_ID)
        .setSmallIcon(R.drawable.ic_stat_music)
        .setContentTitle(title.ifEmpty { "未知曲目" })
        .setContentText(artist.ifEmpty { album })
        .setSubText(album.ifEmpty { null })
        .setContentIntent(contentIntent)
        .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
        .setOnlyAlertOnce(true)
        .setStyle(style)
        .setOngoing(playing)

    addAction(builder, ACTION_PREVIOUS, "上一首", R.drawable.ic_action_previous)
    if (playing) {
      addAction(builder, ACTION_PAUSE, "暂停", R.drawable.ic_action_pause)
    } else {
      addAction(builder, ACTION_PLAY, "播放", R.drawable.ic_action_play)
    }
    addAction(builder, ACTION_NEXT, "下一首", R.drawable.ic_action_next)

    // 通知封面：命中缓存直接用；未命中提交后台加载，加载完再刷新通知（见 currentCoverBitmap）
    currentCoverBitmap(coverPath)?.let { builder.setLargeIcon(it) }
    return builder.build()
  }

  private fun addAction(
    builder: NotificationCompat.Builder,
    action: String,
    label: String,
    icon: Int,
  ) {
    val pi =
      PendingIntent.getService(
        this,
        action.hashCode(),
        Intent(this, PlaybackService::class.java).setAction(action),
        PendingIntent.FLAG_UPDATE_CURRENT or immutableFlag(),
      )
    builder.addAction(NotificationCompat.Action(icon, label, pi))
  }

  private fun immutableFlag(): Int =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) PendingIntent.FLAG_IMMUTABLE else 0

  /**
   * 取当前封面位图（主线程调用，只读缓存）。未命中时提交后台加载，完成后回主线程刷新通知；
   * 同一路径不会重复提交，解码失败也会"缓存"为空结果，避免每条状态同步都重试。
   */
  private fun currentCoverBitmap(path: String): Bitmap? {
    if (path.isBlank()) {
      coverBitmap = null
      coverBitmapPath = null
      return null
    }
    if (path == coverBitmapPath) return coverBitmap
    if (path != coverLoadingPath) {
      coverLoadingPath = path
      coverExecutor.execute {
        val bitmap = loadCoverBitmap(path)
        mainHandler.post {
          coverLoadingPath = null
          if (!running || coverPath != path) return@post
          coverBitmapPath = path
          coverBitmap = bitmap
          if (bitmap != null) {
            notificationManager.notify(NOTIFICATION_ID, buildNotification())
          }
        }
      }
    }
    return null
  }

  /**
   * 通知大图：封面路径由 Rust 侧 `get_track_cover_path` 给出（本地路径或 content://）。
   * Binder 事务上限 1MB，必须降采样到 ≤256px 再 `setLargeIcon`，否则抛 TransactionTooLargeException。
   * 只在 [coverExecutor] 线程调用：content:// 读取要走 provider，可能很慢。
   */
  private fun loadCoverBitmap(path: String): Bitmap? {
    if (path.isBlank()) return null
    val source =
      runCatching {
        when {
          path.startsWith("content://") -> {
            contentResolver.openInputStream(android.net.Uri.parse(path))?.use { input ->
              BitmapFactory.decodeStream(input)
            }
          }
          path.startsWith("/") -> {
            val file = java.io.File(path)
            if (!file.exists()) return null
            val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(file.absolutePath, opts)
            val scale = maxOf(1, maxOf(opts.outWidth, opts.outHeight) / 256)
            BitmapFactory.decodeFile(
              file.absolutePath,
              BitmapFactory.Options().apply { inSampleSize = scale },
            )
          }
          else -> null
        }
      }.getOrNull() ?: return null
    return scaleIfNeeded(source)
  }

  private fun scaleIfNeeded(bitmap: Bitmap): Bitmap {
    val max = maxOf(bitmap.width, bitmap.height)
    if (max <= 256) return bitmap
    val ratio = 256f / max
    return Bitmap.createScaledBitmap(
      bitmap,
      maxOf(1, (bitmap.width * ratio).toInt()),
      maxOf(1, (bitmap.height * ratio).toInt()),
      true,
    )
  }

  /**
   * 起播前的焦点闸门：确认拿到焦点才下发 play，把"未获焦点仍出声"挡在源头。
   * 申请失败时 [requestAudioFocus] 内部会下发 pause 兜底，覆盖 Rust 已先起播的路径。
   */
  private fun ensureFocusForPlayback(): Boolean {
    requestAudioFocus()
    return focusGranted
  }

  private fun requestAudioFocus() {
    // 已持有焦点才直接返回；focusRequest 非空不能当作"已获得"，否则被拒后永远无法重试
    if (focusGranted) return
    // focusRequest 只缓存请求对象（可复用重试），与是否持有焦点无关
    val granted =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        val request = focusRequest ?: buildFocusRequest().also { focusRequest = it }
        audioManager.requestAudioFocus(request) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
      } else {
        @Suppress("DEPRECATION")
        audioManager.requestAudioFocus(
          focusChangeListener,
          AudioManager.STREAM_MUSIC,
          AudioManager.AUDIOFOCUS_GAIN,
        ) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
      }
    focusGranted = granted
    if (!granted) {
      // 系统拒绝焦点：未获焦点还继续出声属于抢播，立即暂停输出；focusGranted 为 false，
      // 用户下次播放会重新申请（不再被"请求对象已存在"挡住）
      android.util.Log.w(TAG, "requestAudioFocus 被拒绝 -> 暂停输出")
      dispatchToRust("pause", 0L)
    }
  }

  private fun buildFocusRequest(): AudioFocusRequest =
    AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
      .setAudioAttributes(
        AudioAttributes.Builder()
          .setUsage(AudioAttributes.USAGE_MEDIA)
          .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
          .build(),
      )
      .setOnAudioFocusChangeListener(focusChangeListener, mainHandler)
      .build()

  private fun abandonAudioFocus() {
    focusGranted = false
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
      focusRequest = null
    } else {
      @Suppress("DEPRECATION")
      audioManager.abandonAudioFocus(focusChangeListener)
    }
  }

  private fun registerNoisyReceiver() {
    val filter = IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      registerReceiver(noisyReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
    } else {
      registerReceiver(noisyReceiver, filter)
    }
  }

  /**
   * 通过 JNI 下发到 Rust（native 实现见 src-tauri/src/android/entry.rs）。
   * 调用是同步的且可能很重（打开文件、建解码器、预填充缓冲、SAF fd 桥），
   * 因此提交到 [commandExecutor] 串行执行，主线程只负责提交，避免 ANR。
   */
  private fun dispatchToRust(action: String, positionMs: Long) {
    commandExecutor.execute {
      runCatching { MainActivity.nativeMediaAction(action, positionMs) }
        .onFailure { e -> android.util.Log.e(TAG, "nativeMediaAction($action) 失败: ${e.message}") }
    }
  }
}
