package com.jdbewl.mercurial_player

import android.app.Activity
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.webkit.WebView
import java.lang.ref.WeakReference

/**
 * 应用内「界面字号」桥：把 `WebSettings.textZoom` 写成 `100 × appScale`，从而覆盖系统字号。
 * 只乘 appScale、**不要除 fontScale**：系统字号就是 textZoom 的初值，不是叠在外的乘子（原理见 android/font_scale.rs）。
 */
object FontScaleBridge {
    private const val TAG = "FontScaleBridge"
    private const val PREFS = "app_font_scale"
    private const val KEY_SCALE = "scale"

    /**
     * 与前端设置页滑块的上下限保持一致，由 `tests/utils/fontScaleBoundsMirror.test.ts` 守住。
     */
    const val MIN_SCALE = 0.8f
    const val MAX_SCALE = 1.6f

    /** 只用于读写 SharedPreferences。由 [attachContext] 注入，早于 [attach] */
    private var appContext: Context? = null

    @Volatile
    private var webRef: WeakReference<WebView>? = null

    private val main = Handler(Looper.getMainLooper())

    /** 用户选定的倍率（相对设计稿）。Rust 命令可能从任意线程调用，故 volatile */
    @Volatile
    private var appScale = 1f

    /**
     * 注入 Application Context（由 [MainActivity.onCreate] 调用）。必须与 [attach] 分开且更早：
     * Rust 的 `set_app_font_scale` 可能在 WebView 建好前来（启动恢复倍率），若此时拿不到
     * Context 就不落盘，用户刚设的倍率重启后丢失。
     */
    fun attachContext(context: Context) {
        appContext = context.applicationContext
    }

    /** 由 [MainActivity.onWebViewCreate] 调用 */
    fun attach(activity: Activity, webView: WebView) {
        appContext = activity.applicationContext
        webRef = WeakReference(webView)
        appScale = prefs(activity.applicationContext).getFloat(KEY_SCALE, 1f)
        applyScale()
    }

    /** 由 [MainActivity.onDestroy] 调用：WebView 已销毁，不能再对它写 textZoom */
    fun detach() {
        webRef = null
    }

    /**
     * 由 Rust 命令 `set_app_font_scale` 经 [MainActivity.setAppFontScale] 调用。
     * Tauri 命令跑在异步运行时上，调用线程不保证是主线程，内部会切回主线程设置。
     */
    fun setScale(scale: Float) {
        val clamped = scale.coerceIn(MIN_SCALE, MAX_SCALE)
        val changed = clamped != appScale
        appScale = clamped
        // commit 而非 apply：这是用户显式选择的关键设置，而 apply 是异步落盘；
        // 紧接着用户就可能被系统选择器/权限对话框打断并发进程被杀，异步写会丢
        appContext?.let { prefs(it).edit().putFloat(KEY_SCALE, clamped).commit() }
        if (changed) applyScale()
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    /**
     * 把当前倍率写到 WebView 的 textZoom。三个入口共用：WebView 创建（[attach]）、用户改倍率
     * （[setScale]）、系统字号变化（[MainActivity.onConfigurationChanged]）。
     */
    fun applyScale() {
        val webView = webRef?.get() ?: return
        // 写 textZoom 有线程约束，统一切到主线程
        main.post {
            val scale = appScale
            // 只乘 appScale，不除 fontScale —— 原因见类注释
            val zoom = Math.round(100f * scale)
            runCatching {
                val settings = webView.settings
                if (settings.textZoom == zoom) settings.textZoom = 100
                settings.textZoom = zoom
            }.onFailure { Log.w(TAG, "setTextZoom($zoom) 失败", it) }
            Log.i(TAG, "界面字号 appScale=$scale → textZoom=$zoom")
        }
    }
}
