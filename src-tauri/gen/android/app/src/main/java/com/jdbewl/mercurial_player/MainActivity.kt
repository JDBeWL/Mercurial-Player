package com.jdbewl.mercurial_player

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  // TauriActivity 把它覆盖成 false，于是返回键直接 finish() 掉 Activity：在应用里按返回
  // 会一步跳回桌面。打开后 wry 会装 OnBackPressedCallback，返回先走 webview.goBack()，
  // 前端（Settings.vue）用 history 条目实现"详情页 → 列表页 → 关闭设置 → 再返回才退出"。
  override val handleBackNavigation: Boolean = true

  companion object {
    init {
      System.loadLibrary("mercurial_player_lib")
    }

    // singleTask 生命周期内只有一个 Activity 实例，静态方法据此取窗口
    @Volatile
    private var current: MainActivity? = null
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
      val activity = current ?: return
      activity.runOnUiThread {
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
  }

  /** Android 13+ 通知运行时权限：没有它通知栏完全不可见 */
  private val requestNotificationPermission =
    registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
      android.util.Log.i("MainActivity", "POST_NOTIFICATIONS granted=$granted")
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    current = this
    // Application Context 而非 this：ndk_context 的引用是进程级的，不跟 Activity 生命周期
    initNdkContext(applicationContext)
    SafBridge.init(this)
    MediaBridge.init(this)
    AudioBridge.init(this)
    super.onCreate(savedInstanceState)
    askNotificationPermissionIfNeeded()
  }

  override fun onDestroy() {
    if (current === this) current = null
    super.onDestroy()
  }

  /**
   * 可见性上报用 onStart/onStop 而不是 onResume/onPause：弹系统对话框只会 pause，
   * 界面仍在前面，频谱该继续算。
   */
  override fun onStart() {
    super.onStart()
    runCatching { nativeSetForeground(true) }
      .onFailure { e -> android.util.Log.w("MainActivity", "setForeground(true) failed: ${e.message}") }
  }

  override fun onStop() {
    runCatching { nativeSetForeground(false) }
      .onFailure { e -> android.util.Log.w("MainActivity", "setForeground(false) failed: ${e.message}") }
    super.onStop()
  }

  override fun onResume() {
    super.onResume()
    // 后台期间 WebView 的 JS 会被冻结（实测：心跳停止、事件积压），
    // 回到前台时让 Rust 把真实播放状态重新推给前端
    runCatching { nativeMediaAction("sync", 0L) }
      .onFailure { e ->
        android.util.Log.w("MainActivity", "sync on resume failed: ${e.message}")
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
   * 配置变化时重新覆盖一次 textZoom：WebView 的 textZoom 初值跟着系统的「字体大小」走，
   * 不覆盖的话应用内倍率会跟着系统设置漂移。
   */
  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    FontScaleBridge.reapply()
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
}
