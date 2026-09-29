package com.jdbewl.mercurial_player

import android.app.Activity
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.webkit.WebView
import java.lang.ref.WeakReference

/**
 * 应用内「界面字号」桥。
 *
 * ## 为什么需要
 *
 * Android 的「字体大小」设置会被 WebView 当成字号倍率，乘到**所有 CSS px 字号**上。
 * 真机实测（Redmi Note 11T Pro，`font_scale=1.17`）：CSS 写的 16px 实际按 18.56px
 * 排版；把系统字号调回 1.0 后立刻变回 16px。而本应用是固定像素布局的桌面级 UI，
 * 字号被放大后容器盒子并不跟着长 —— 于是 `max-height: 1.4em` 这类按原始字号算出的
 * 高度就不够了，g / y 这些带下伸部的字母会被切掉下半截。
 *
 * ## 做法
 *
 * `WebSettings.textZoom` 就是字号倍率的**唯一来源**，它的初值跟着系统的「字体大小」
 * 走。所以应用自己显式写 `textZoom = 100 × 应用内倍率` 即可 —— 系统设置被整体覆盖，
 * 「通用设置 → 界面字号」成为唯一来源。
 *
 * ⚠️ 别把它想成"两个因子相乘"：本桥第一版按 `zoom = 100 × appScale / fontScale` 写，
 * 实测 device 上 16px 被排成 13.6px（= 16 × 0.85），而不是预期的 16px —— 说明 fontScale
 * 不是叠在 textZoom 之外的另一个乘子，而是**就是 textZoom 的初值**（fontScale=1.17 时
 * 初值 116）。所以这里只写 `100 × appScale`，不要再除 fontScale。
 *
 * 由 [MainActivity.onWebViewCreate] 注入 WebView，接着立即应用一次已保存的倍率
 * （发生在首次绘制之前，因此启动时不会看到字号跳变）。
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
     * 调用线程不保证是主线程（Tauri 命令跑在异步运行时上），内部会切回主线程设置。
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
            // 只乘 appScale，不除 fontScale —— 原因见类注释里那条实测记录
            val zoom = Math.round(100f * scale)
            runCatching { webView.settings.textZoom = zoom }
                .onFailure { Log.w(TAG, "setTextZoom($zoom) 失败", it) }
            Log.i(TAG, "界面字号 appScale=$scale → textZoom=$zoom")
        }
    }
}
