package com.jdbewl.mercurial_player

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import org.json.JSONArray
import org.json.JSONObject

/**
 * 输出设备探测（USB DAC 独占 / 位完美的设备信息来源）：AAudio 要 `AudioDeviceInfo.getId()`，
 * cpal 在安卓上给不了（原理见 Rust `audio/aaudio::device`），故设备信息一律经 [getOutputDevicesJson] 取。
 * JSON 字段与 Rust 侧的 `OutputDeviceInfo` 一一对应，改一边要同步另一边。
 */
object AudioBridge {
  private const val TAG = "AudioBridge"

  @Volatile
  private var appContext: Context? = null

  @Volatile
  private var audioManager: AudioManager? = null

  /** 已注册的设备回调；重复 init 时先注销，避免回调叠加 */
  @Volatile
  private var callback: AudioDeviceCallback? = null

  /**
   * 设备回调专用线程。回调里要同步经 JNI 调 Rust（查设备/拿锁/openStream），
   * 而 init() 在主线程调用，用注册线程的 Looper 会让插拔外设直接 ANR，故显式指定后台 Looper。
   * 线程随进程存活，[release] 由 [MainActivity.onDestroy] 在没有播放会话时才调用。
   */
  @Volatile
  private var callbackThread: HandlerThread? = null

  fun init(context: Context) {
    val ctx = context.applicationContext
    appContext = ctx
    val manager = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
    if (manager == null) {
      // 拿不到 AudioManager 只可能是系统服务异常，必须输出日志
      android.util.Log.e(TAG, "AUDIO_SERVICE 不可用，输出设备探测将一直返回空列表")
    }
    audioManager = manager
    registerCallback(manager)
  }

  /**
   * 注销设备回调并停掉专用线程。只应在没有播放会话时调用（见 [MainActivity.onDestroy]）：
   * 播放中注销会让 USB DAC 插拔失去快速路径，只剩 AAudio error 回调兜底，路由变化后不会
   * 自动重建流。
   */
  fun release() {
    callback?.let { cb ->
      audioManager?.let { am -> runCatching { am.unregisterAudioDeviceCallback(cb) } }
      callback = null
    }
    synchronized(this) {
      callbackThread?.let { thread ->
        // quitSafely：等在途回调跑完再退，避免刚插上 DAC 就释放时丢掉那次通知
        thread.quitSafely()
        callbackThread = null
      }
    }
  }

  private fun registerCallback(manager: AudioManager?) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
    val am = manager ?: return
    callback?.let { runCatching { am.unregisterAudioDeviceCallback(it) } }

    val cb =
      object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) {
          if (addedDevices.any { it.isUsbOutput() }) notifyRouteChanged()
        }

        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) {
          if (removedDevices.any { it.isUsbOutput() }) notifyRouteChanged()
        }
      }
    callback = cb
    val handler = Handler(callbackHandlerThread().looper)
    runCatching { am.registerAudioDeviceCallback(cb, handler) }
      .onFailure { e -> android.util.Log.w(TAG, "registerAudioDeviceCallback failed: ${e.message}") }
  }

  /** 取或按需创建设备回调线程；重复 init 时复用同一个 */
  private fun callbackHandlerThread(): HandlerThread {
    callbackThread?.let { return it }
    return synchronized(this) {
      callbackThread ?: HandlerThread("audio-device-callback").apply {
        start()
        callbackThread = this
      }
    }
  }

  private fun notifyRouteChanged() {
    // 交给 Rust 决定怎么处理（收流 / 暂停 / 通知前端），Kotlin 只负责"变了"这件事
    runCatching { MainActivity.nativeAudioRouteChanged() }
      .onFailure { e ->
        android.util.Log.w(TAG, "nativeAudioRouteChanged failed: ${e.message}")
      }
  }

  /** 由 Rust 调用（JNI）：返回全部输出设备的 JSON 数组；空串表示拿不到，Rust 侧按空列表处理 */
  @JvmStatic
  fun getOutputDevicesJson(): String {
    val manager = audioManager
    if (manager == null) {
      android.util.Log.e(TAG, "getOutputDevicesJson: AudioBridge 未初始化，按空设备列表返回") // 同上抛出异常
      return ""
    }
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return ""

    val array = JSONArray()
    runCatching {
      for (device in manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)) {
        if (!device.isSink) continue
        array.put(device.toJson())
      }
    }.onFailure { e ->
      android.util.Log.e(TAG, "getDevices failed: ${e.message}")
    }
    return array.toString()
  }

  private fun AudioDeviceInfo.isUsbOutput(): Boolean =
    isSink && type in USB_TYPES

  private fun AudioDeviceInfo.toJson(): JSONObject {
    val obj = JSONObject()
    obj.put("id", id)
    obj.put("name", productName.toString())
    obj.put("typeName", typeName())
    obj.put("isUsb", isUsbOutput())

    // 这三个列表为空 == 设备上报 UNSPECIFIED，Rust 侧会按设备默认值处理
    obj.put("sampleRates", jsonIntArray { sampleRates })
    obj.put("channelCounts", jsonIntArray { channelCounts })
    obj.put("encodings", jsonIntArray { encodings })
    return obj
  }

  /** 老设备上这些 getter 可能抛异常，一律按 UNSPECIFIED（空数组）处理 */
  private inline fun jsonIntArray(read: () -> IntArray): JSONArray =
    JSONArray().apply {
      val values =
        try {
          read()
        } catch (e: Throwable) {
          android.util.Log.w(TAG, "device capability query failed: ${e.message}")
          IntArray(0)
        }
      values.forEach { put(it) }
    }

  private fun AudioDeviceInfo.typeName(): String =
    when (type) {
      AudioDeviceInfo.TYPE_BUILTIN_SPEAKER -> "BUILTIN_SPEAKER"
      AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "BUILTIN_EARPIECE"
      AudioDeviceInfo.TYPE_WIRED_HEADSET -> "WIRED_HEADSET"
      AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "WIRED_HEADPHONES"
      AudioDeviceInfo.TYPE_BLUETOOTH_SCO -> "BLUETOOTH_SCO"
      AudioDeviceInfo.TYPE_BLUETOOTH_A2DP -> "BLUETOOTH_A2DP"
      AudioDeviceInfo.TYPE_USB_DEVICE -> "USB_DEVICE"
      AudioDeviceInfo.TYPE_USB_HEADSET -> "USB_HEADSET"
      AudioDeviceInfo.TYPE_USB_ACCESSORY -> "USB_ACCESSORY"
      AudioDeviceInfo.TYPE_HDMI -> "HDMI"
      else -> "TYPE_$type"
    }

  private val USB_TYPES =
    setOf(
      AudioDeviceInfo.TYPE_USB_DEVICE,
      AudioDeviceInfo.TYPE_USB_HEADSET,
      AudioDeviceInfo.TYPE_USB_ACCESSORY,
    )
}
