package com.jdbewl.mercurial_player

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

class MainActivity : TauriActivity() {
  // Settings.vue 用 history 条目实现"详情页 → 列表页 → 关闭设置 → 再返回才退出"。
  override val handleBackNavigation: Boolean = true

  companion object {
    private const val TAG = "MainActivity"

    /** 界面级偏好 */
    private const val PREFS_UI = "ui_prefs"
    private const val KEY_NOTIFICATION_GUIDED = "notification_permission_guided"

    /** 主线程之外的 native 调用串行执行器 */
    private val backgroundCalls: ExecutorService =
      Executors.newSingleThreadExecutor { r -> Thread(r, "activity-native-call") }

    init {
      System.loadLibrary("mercurial_player_lib")
    }

    // singleTask 生命周期内只有一个 Activity 实例，静态方法据此取窗口
    @Volatile
    private var current: MainActivity? = null

    /**
     * 最近一次下发的系统栏隐藏状态（hideStatusBars to hideNavigationBars）会让 Rust 的
     * `set_system_ui_hidden` 在 Activity 尚未创建时也会调用，以前静默 return 却让命令层收到
     * "假成功"，现在记在进程内，等窗口可用时重放。
     */
    @Volatile
    private var pendingSystemUi: Pair<Boolean, Boolean>? = null
    // 初始化 Rust 侧 cpal AAudio 依赖的 ndk_context（JavaVM + Context）。
    // 必须传 Application Context：该全局引用会活到进程退出，Activity 却会随进程保活被重建；
    // 且 ndk-context 0.1.1 不允许重复初始化（二次调用直接断言崩溃），Rust 侧另有一道 Once 保护
    @JvmStatic external fun initNdkContext(context: Context)

    // 由 Rust 侧通过 JNI（call_static_method）调用，转发给 SafBridge 调起系统目录选择器
    @JvmStatic
    fun safRequestPick() {
      // Rust 的调用线程不是主线程，而 Activity Result API 的 launch 必须在主线程
      Handler(Looper.getMainLooper()).post { SafBridge.requestPick() }
    }

    /**
     * 通知栏 / MediaSession / 耳机线控 / 音频焦点 / 拔耳机 的统一入口。
     * native 实现见 src-tauri/src/android/entry.rs 的 nativeMediaAction。
     */
    @JvmStatic external fun nativeMediaAction(action: String, positionMs: Long)

    /**
     * USB DAC 插拔通知：由 [AudioBridge] 的 `AudioDeviceCallback` 调用。
     * native 实现见 src-tauri/src/android/entry.rs 的 nativeAudioRouteChanged。
     */
    @JvmStatic external fun nativeAudioRouteChanged()

    /**
     * 报告应用前后台，用于关掉后台期间的频谱计算与事件推送。由 [onStart] / [onStop] 调用。
     * native 实现见 src-tauri/src/android/entry.rs 的 nativeSetForeground。
     */
    @JvmStatic external fun nativeSetForeground(foreground: Boolean)

    /**
     * 设置应用内界面字号倍率。由 Rust 命令 `set_app_font_scale` 经 JNI 调用
     * （见 src-tauri/src/android/font_scale.rs），转发给 [FontScaleBridge]。
     */
    @JvmStatic
    fun setAppFontScale(scale: Float) {
      FontScaleBridge.setScale(scale)
    }

    /** 隐藏/恢复系统栏。由 Rust 命令 `set_system_ui_hidden` 经 JNI 调用 */
    @JvmStatic
    fun setSystemUiHidden(hideStatusBars: Boolean, hideNavigationBars: Boolean) {
      pendingSystemUi = hideStatusBars to hideNavigationBars
      val activity = current
      if (activity == null) {
        // 不在这里假装成功：状态已记下，onResume 会把窗口可用的那一刻补上
        Log.w(TAG, "setSystemUiHidden: 当前没有 Activity，已记录待应用状态")
        return
      }
      activity.runOnUiThread {
        applySystemUiHidden(activity, hideStatusBars, hideNavigationBars)
      }
    }

    /** 把系统栏的显隐落到窗口上。必须在主线程、且 window.decorView 已存在时调用 */
    private fun applySystemUiHidden(
      activity: MainActivity,
      hideStatusBars: Boolean,
      hideNavigationBars: Boolean,
    ) {
      val controller = WindowInsetsControllerCompat(activity.window, activity.window.decorView)
      // 隐藏后从边缘滑动可临时唤出，不会把用户锁死
      controller.systemBarsBehavior =
        WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
      if (hideStatusBars) {
        controller.hide(WindowInsetsCompat.Type.statusBars())
      } else {
        controller.show(WindowInsetsCompat.Type.statusBars())
      }
      if (hideNavigationBars) {
        controller.hide(WindowInsetsCompat.Type.navigationBars())
      } else {
        controller.show(WindowInsetsCompat.Type.navigationBars())
      }
    }
  }

  /** 最近一次已知的深/浅色模式，用于判断 uiMode 是否真的变了（见 [onConfigurationChanged]） */
  private var lastUiMode = Configuration.UI_MODE_NIGHT_UNDEFINED

  /** Android 13+ 通知运行时权限：没有它通知栏完全不可见 */
  private val requestNotificationPermission =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
      Log.i(TAG, "POST_NOTIFICATIONS granted=$granted")
      if (granted) return@registerForActivityResult
      // 通知栏与后台播放控制会一直不可见，必须主动说明并给一条去设置的路径
      if (!shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS)) {
        showNotificationPermissionGuidance()
      }
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    current = this
    lastUiMode = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
    // Application Context 而非 this：ndk_context 的引用是进程级的，不跟 Activity 生命周期
    initNdkContext(applicationContext)
    // FontScaleBridge 的倍率要在 attach(webView) 之前就能落盘
    FontScaleBridge.attachContext(applicationContext)
    SafBridge.init(this)
    MediaBridge.init(this)
    AudioBridge.init(this)
    super.onCreate(savedInstanceState)
    askNotificationPermissionIfNeeded()
  }

  override fun onDestroy() {
    if (current === this) current = null
    // WebView 已随 Activity 销毁后须断掉引用，否则 FontScaleBridge 会一直握着
    // 一个已销毁的 WebView 防止下一次操作失败
    FontScaleBridge.detach()
    // 设备回调线程只在没有播放会话时才回收
    if (!PlaybackService.isRunning()) AudioBridge.release()
    super.onDestroy()
  }

  /**
   * 可见性上报用 onStart/onStop 而不是 onResume/onPause：弹系统对话框只会 pause，
   * 界面仍在前面，频谱该继续算。
   */
  override fun onStart() {
    super.onStart()
    safeNativeCall("setForeground(true)") { nativeSetForeground(true) }
  }

  override fun onStop() {
    safeNativeCall("setForeground(false)") { nativeSetForeground(false) }
    super.onStop()
  }

  override fun onResume() {
    super.onResume()
    // 系统栏期望状态的重放点：冷启动早期 Activity 还没建好时下发的隐藏请求记在
    // pendingSystemUi 里，等窗口可用（此刻）才真正落地
    current?.let { activity ->
      pendingSystemUi?.let { (hideStatus, hideNav) ->
        applySystemUiHidden(activity, hideStatus, hideNav)
      }
    }
    // 后台期间 WebView 的 JS 会被冻结
    // 因此与播放命令一样提交到后台串行执行，绝不占主线程
    submitBackgroundCall("sync") { nativeMediaAction("sync", 0L) }
  }

  /**
   * 把 native 调用挪出主线程。JNI 调用是同步的。
   * Activity 会被重建，执行器不应跟着重建或关闭。
   */
  private fun submitBackgroundCall(action: String, block: () -> Unit) {
    runCatching { backgroundCalls.execute { safeNativeCall(action, block) } }
      .onFailure { e -> Log.w(TAG, "后台调用提交失败($action): ${e.message}") }
  }

  /**
   * native 调用的统一兜底，只 try-catch `Exception`。
   * 当调用失败后必须抛出 error 级别的日志
   */
  private fun safeNativeCall(name: String, block: () -> Unit) {
    try {
      block()
    } catch (e: UnsatisfiedLinkError) {
      Log.e(TAG, "$name: native 库未加载或 JNI 符号缺失", e)
    } catch (e: Exception) {
      Log.w(TAG, "$name 失败: ${e.message}")
    }
  }

  /**
   * WryActivity 的钩子：WebView 创建时把引用交给 [FontScaleBridge]。
   * 这里就立刻应用一次已保存的界面字号 —— 早于首次绘制，启动时不会看到字号跳变。
   */
  override fun onWebViewCreate(webView: WebView) {
    FontScaleBridge.attach(this, webView)
  }

  /**
   * 配置变化时重新覆盖一次 textZoom：WebView 初值跟着系统「字体大小」走，不覆盖会漂移。
   */
  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    val uiMode = newConfig.uiMode and Configuration.UI_MODE_NIGHT_MASK
    if (uiMode != lastUiMode) {
      lastUiMode = uiMode
      // enableEdgeToEdge 会按当前主题重新推导系统栏样式（含 isAppearanceLightStatusBars），
      // 重复调用是幂等的
      enableEdgeToEdge()
    }
    FontScaleBridge.applyScale()
  }

  private fun askNotificationPermissionIfNeeded() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
    val granted =
      ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) ==
        PackageManager.PERMISSION_GRANTED
    if (!granted) {
      requestNotificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
  }

  /**
   * 通知权限被永久拒绝后的说明。只提示一次，
   * 避免每次冷启动都弹一个用户已经拒绝过的对话框。
   */
  private fun showNotificationPermissionGuidance() {
    if (isFinishing || isDestroyed) return
    val prefs = getSharedPreferences(PREFS_UI, MODE_PRIVATE)
    if (prefs.getBoolean(KEY_NOTIFICATION_GUIDED, false)) return
    prefs.edit().putBoolean(KEY_NOTIFICATION_GUIDED, true).apply()

    android.app.AlertDialog.Builder(this)
      .setTitle("开启通知权限")
      .setMessage(
        "没有通知权限时，后台播放的控制通知不会显示，锁屏控制与耳机线控也无法使用。" +
          "可以在系统设置里允许本应用发送通知。",
      )
      .setPositiveButton("去设置") { _, _ -> openAppNotificationSettings() }
      .setNegativeButton("以后再说", null)
      .show()
  }

  /** 跳到本应用的通知设置页；部分定制系统没有该页面时退回应用详情页 */
  private fun openAppNotificationSettings() {
    val intent =
      Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS)
        .putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, packageName)
    runCatching { startActivity(intent) }
      .onFailure {
        val fallback =
          Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
            .setData(Uri.fromParts("package", packageName, null))
        runCatching { startActivity(fallback) }
          .onFailure { e ->
            Log.w(TAG, "无法打开通知设置页: ${e.message}")
          }
      }
  }
}
