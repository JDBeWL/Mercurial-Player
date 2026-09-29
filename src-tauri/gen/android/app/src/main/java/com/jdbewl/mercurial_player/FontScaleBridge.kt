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
 * 只乘 appScale、**不要除 fontScale**：系统字号就是 textZoom 的初值，不是叠在外的乘子（原理见 app_font_scale.rs）。
 */
object FontScaleBridge {
    private const val TAG = "FontScaleBridge"
    private const val PREFS = "app_font_scale"
    private const val KEY_SCALE = "scale"

    /** 与前端设置页滑块的上下限保持一致，改一处要改两处 */
    const val MIN_SCALE = 0.8f
    const val MAX_SCALE = 1.6f

    /** 只用于读写 SharedPreferences */
    private var appContext: Context? = null
    private var webRef: WeakReference<WebView>? = null
    private val main = Handler(Looper.getMainLooper())

    /** 用户选定的倍率（相对设计稿）。Rust 命令可能从任意线程调用，故 volatile */
    @Volatile
    private var appScale = 1f

    /** 由 [MainActivity.onWebViewCreate] 调用 */
    fun attach(activity: Activity, webView: WebView) {
        val context = activity.applicationContext
        appContext = context
        webRef = WeakReference(webView)
        appScale = prefs(context).getFloat(KEY_SCALE, 1f)
        apply()
    }

    /**
     * 由 Rust 命令 `set_app_font_scale` 经 [MainActivity.setAppFontScale] 调用。
     * Tauri 命令跑在异步运行时上，调用线程不保证是主线程，内部会切回主线程设置。
     */
    fun setScale(scale: Float) {
        val clamped = scale.coerceIn(MIN_SCALE, MAX_SCALE)
        val changed = clamped != appScale
        appScale = clamped
        appContext?.let { prefs(it).edit().putFloat(KEY_SCALE, clamped).apply() }
        if (changed) apply()
    }

    /** 系统字号变化后重新覆盖一次（WebView 重建后初值会跟着新 fontScale 走） */
    fun reapply() = apply()

    /** 当前应用内倍率，供排查用 */
    fun currentScale(): Float = appScale

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun apply() {
        val webView = webRef?.get() ?: return
        // 写 textZoom 有线程约束，统一切到主线程
        main.post {
            val scale = appScale
            // 只乘 appScale，不除 fontScale —— 原因见类注释
            val zoom = Math.round(100f * scale)
            runCatching { webView.settings.textZoom = zoom }
                .onFailure { Log.w(TAG, "setTextZoom($zoom) 失败", it) }
            Log.i(TAG, "界面字号 appScale=$scale → textZoom=$zoom")
        }
    }
}
