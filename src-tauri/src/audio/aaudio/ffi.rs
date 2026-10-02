//! libaaudio 的原始 FFI 声明：只声明本项目用到的那部分 AAudio C API。
//! cpal 的 AAudio 后端不暴露共享模式与设备选择，要"USB DAC 直连"必须自己调 AAudio。
//! 符号来自 NDK 的 `libaaudio.so`（API 26 起），由文件末尾的 `#[link]` 引入，无需运行时 dlopen。

#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unsafe_code
)]

use std::os::raw::{c_char, c_int, c_void};

/// AAudio 的返回值：0 = OK，负数为 `AAUDIO_ERROR_*`
pub type aaudio_result_t = i32;
pub type aaudio_stream_state_t = i32;
pub type aaudio_direction_t = i32;
pub type aaudio_sharing_mode_t = i32;
pub type aaudio_format_t = i32;
pub type aaudio_performance_mode_t = i32;
pub type aaudio_data_callback_result_t = i32;

/// AAudio 的不透明句柄（用空枚举保证不可实例化）
pub enum AAudioStreamStruct {}
pub enum AAudioStreamBuilderStruct {}

pub type AAudioStream = *mut AAudioStreamStruct;
pub type AAudioStreamBuilder = *mut AAudioStreamBuilderStruct;

/// 数据回调：`(stream, userData, audioData, numFrames) -> 继续/停止`
pub type AAudioStream_dataCallback = Option<
    unsafe extern "C" fn(
        stream: *mut AAudioStreamStruct,
        userData: *mut c_void,
        audioData: *mut c_void,
        numFrames: i32,
    ) -> aaudio_data_callback_result_t,
>;

/// 错误回调：`(stream, userData, error)`
pub type AAudioStream_errorCallback = Option<
    unsafe extern "C" fn(stream: *mut AAudioStreamStruct, userData: *mut c_void, error: i32),
>;

// 方向
pub const AAUDIO_DIRECTION_OUTPUT: aaudio_direction_t = 0;

// 共享模式
pub const AAUDIO_SHARING_MODE_SHARED: aaudio_sharing_mode_t = 0;
/// 独占：直连设备、绕开 AudioFlinger 的混音与重采样（位完美的前提）
pub const AAUDIO_SHARING_MODE_EXCLUSIVE: aaudio_sharing_mode_t = 1;

// 采样格式
pub const AAUDIO_FORMAT_INVALID: aaudio_format_t = -1;
pub const AAUDIO_FORMAT_UNSPECIFIED: aaudio_format_t = 0;
pub const AAUDIO_FORMAT_PCM_I16: aaudio_format_t = 1;
pub const AAUDIO_FORMAT_PCM_FLOAT: aaudio_format_t = 2;
/// API 31+（本项目 minSdk 26，需运行时按 API 等级取舍）
pub const AAUDIO_FORMAT_PCM_I24_PACKED: aaudio_format_t = 3;
pub const AAUDIO_FORMAT_PCM_I32: aaudio_format_t = 4;

// 性能模式
pub const AAUDIO_PERFORMANCE_MODE_NONE: aaudio_performance_mode_t = 10;
pub const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: aaudio_performance_mode_t = 12;

// 数据回调返回值
pub const AAUDIO_CALLBACK_RESULT_CONTINUE: aaudio_data_callback_result_t = 0;
pub const AAUDIO_CALLBACK_RESULT_STOP: aaudio_data_callback_result_t = 1;

// 流状态
pub const AAUDIO_STREAM_STATE_UNINITIALIZED: aaudio_stream_state_t = 0;
pub const AAUDIO_STREAM_STATE_UNKNOWN: aaudio_stream_state_t = 1;
pub const AAUDIO_STREAM_STATE_OPEN: aaudio_stream_state_t = 2;
pub const AAUDIO_STREAM_STATE_STARTED: aaudio_stream_state_t = 4;
pub const AAUDIO_STREAM_STATE_PAUSED: aaudio_stream_state_t = 6;
pub const AAUDIO_STREAM_STATE_STOPPED: aaudio_stream_state_t = 10;
pub const AAUDIO_STREAM_STATE_DISCONNECTED: aaudio_stream_state_t = 13;

// 错误码（仅用于与文本互转，具体值以 NDK 头文件为准）
pub const AAUDIO_OK: aaudio_result_t = 0;

#[link(name = "aaudio")]
unsafe extern "C" {
    // StreamBuilder
    pub fn AAudio_createStreamBuilder(builder: *mut AAudioStreamBuilder) -> aaudio_result_t;
    pub fn AAudioStreamBuilder_delete(builder: AAudioStreamBuilder) -> aaudio_result_t;
    pub fn AAudioStreamBuilder_setDeviceId(builder: AAudioStreamBuilder, deviceId: c_int);
    pub fn AAudioStreamBuilder_setDirection(builder: AAudioStreamBuilder, direction: c_int);
    pub fn AAudioStreamBuilder_setSharingMode(builder: AAudioStreamBuilder, sharingMode: c_int);
    pub fn AAudioStreamBuilder_setSampleRate(builder: AAudioStreamBuilder, sampleRate: c_int);
    pub fn AAudioStreamBuilder_setChannelCount(builder: AAudioStreamBuilder, channelCount: c_int);
    pub fn AAudioStreamBuilder_setFormat(builder: AAudioStreamBuilder, format: aaudio_format_t);
    pub fn AAudioStreamBuilder_setPerformanceMode(
        builder: AAudioStreamBuilder,
        mode: aaudio_performance_mode_t,
    );
    /// 单次数据回调交付的帧数。不设置时 AAudio 每 burst 回调一次；调大可降低回调线程
    /// 唤醒频率，代价是输出延迟变长。生效上限为 BufferCapacity。
    pub fn AAudioStreamBuilder_setFramesPerDataCallback(
        builder: AAudioStreamBuilder,
        numFrames: c_int,
    );
    /// 缓冲区容量上限。实际值会被 AAudio 向上取整到 burst 的整数倍，也可能按设备约束调整。
    pub fn AAudioStreamBuilder_setBufferCapacityInFrames(
        builder: AAudioStreamBuilder,
        numFrames: c_int,
    );
    pub fn AAudioStreamBuilder_setDataCallback(
        builder: AAudioStreamBuilder,
        callback: AAudioStream_dataCallback,
        userData: *mut c_void,
    );
    pub fn AAudioStreamBuilder_setErrorCallback(
        builder: AAudioStreamBuilder,
        callback: AAudioStream_errorCallback,
        userData: *mut c_void,
    );
    pub fn AAudioStreamBuilder_openStream(
        builder: AAudioStreamBuilder,
        stream: *mut AAudioStream,
    ) -> aaudio_result_t;

    // Stream
    pub fn AAudioStream_close(stream: AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_requestStart(stream: AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_requestPause(stream: AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_requestStop(stream: AAudioStream) -> aaudio_result_t;
    pub fn AAudioStream_getSampleRate(stream: AAudioStream) -> c_int;
    pub fn AAudioStream_getChannelCount(stream: AAudioStream) -> c_int;
    pub fn AAudioStream_getFormat(stream: AAudioStream) -> aaudio_format_t;
    pub fn AAudioStream_getDeviceId(stream: AAudioStream) -> c_int;
    pub fn AAudioStream_getSharingMode(stream: AAudioStream) -> aaudio_sharing_mode_t;
    pub fn AAudioStream_getState(stream: AAudioStream) -> aaudio_stream_state_t;
    pub fn AAudioStream_getBufferCapacityInFrames(stream: AAudioStream) -> c_int;
    pub fn AAudioStream_getFramesPerBurst(stream: AAudioStream) -> c_int;
    pub fn AAudioStream_setBufferSizeInFrames(
        stream: AAudioStream,
        numFrames: c_int,
    ) -> aaudio_result_t;

    /// 把错误码转成可读文本（返回的指针由实现静态持有，不要 free）
    pub fn AAudio_convertResultToText(returnCode: aaudio_result_t) -> *const c_char;
}

/// 把 AAudio 返回码转成可读文本（用于日志）。
///
/// # Safety
/// 只在成功返回且指针非空时读取 C 字符串；AAudio 返回的是静态字符串。
pub unsafe fn result_to_text(code: aaudio_result_t) -> String {
    // SAFETY: 该 C API 只把 returnCode 映射到静态字符串表, 任意 i32 输入都合法
    // (未知码返回兜底文本), 不触碰其它状态。
    let ptr = unsafe { AAudio_convertResultToText(code) };
    if ptr.is_null() {
        return format!("AAudio error {code}");
    }
    // SAFETY: AAudio_convertResultToText 返回的指针指向静态常量字符串
    let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
    cstr.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_mode_constants_match_ndk() {
        // SHARED 必须是 0、EXCLUSIVE 必须是 1（aaudio.h 的枚举顺序）
        assert_eq!(AAUDIO_SHARING_MODE_SHARED, 0);
        assert_eq!(AAUDIO_SHARING_MODE_EXCLUSIVE, 1);
    }

    #[test]
    fn format_constants_match_ndk() {
        assert_eq!(AAUDIO_FORMAT_PCM_I16, 1);
        assert_eq!(AAUDIO_FORMAT_PCM_FLOAT, 2);
        assert_eq!(AAUDIO_FORMAT_PCM_I24_PACKED, 3);
        assert_eq!(AAUDIO_FORMAT_PCM_I32, 4);
    }

    #[test]
    fn performance_mode_constants_match_ndk() {
        assert_eq!(AAUDIO_PERFORMANCE_MODE_NONE, 10);
        assert_eq!(AAUDIO_PERFORMANCE_MODE_LOW_LATENCY, 12);
    }
}
