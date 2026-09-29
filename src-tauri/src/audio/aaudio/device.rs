//! Android 输出设备能力查询
//! `AAudioStreamBuilder_setDeviceId` 要的是 `AudioDeviceInfo.getId()` 这个系统设备 id，
//! cpal 枚举出的名字（多半是同一个手机型号）拿不到它，也拿不到采样率/位深，故一律经 Kotlin 取。

use crate::error::AppError;
use serde::{Deserialize, Serialize};

/// Kotlin `AudioBridge.getOutputDevicesJson()` 返回的单条记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDeviceInfo {
    /// 系统设备 id，直接喂给 `AAudioStreamBuilder_setDeviceId`
    pub id: i32,
    pub name: String,
    #[serde(rename = "typeName")]
    pub type_name: String,
    /// 是否为 USB 音频设备（USB_DEVICE / USB_HEADSET / USB_ACCESSORY 由 Kotlin 判定）
    pub is_usb: bool,
    /// 设备支持的采样率（升序），空数组表示未上报（按 UNSPECIFIED 处理）
    #[serde(default)]
    pub sample_rates: Vec<u32>,
    /// 设备支持的声道数
    #[serde(default)]
    pub channel_counts: Vec<u16>,
    /// `android.media.AudioFormat` 的 ENCODING_* 数值（PCM_16BIT=2 / PCM_FLOAT=4 /
    /// PCM_24BIT_PACKED=21 / PCM_32BIT=22）
    #[serde(default)]
    pub encodings: Vec<i32>,
}

/// 读取当前全部输出设备
pub fn query_output_devices() -> Result<Vec<OutputDeviceInfo>, AppError> {
    let json = crate::android_jni::jni_call_static_string(
        "com/jdbewl/mercurial_player/AudioBridge",
        "getOutputDevicesJson",
    )?;
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json).map_err(|e| AppError::msg(format!("解析输出设备 JSON 失败: {e}")))
}

/// 找出 USB 音频输出设备（USB DAC）。
///
/// 有多个时取第一个：同一时刻通常只接一个，且 `getDevices` 的顺序稳定。
pub fn find_usb_output_device() -> Result<Option<OutputDeviceInfo>, AppError> {
    Ok(query_output_devices()?.into_iter().find(|d| d.is_usb))
}

/// 在设备支持的采样率里挑一个最接近 `preferred` 的：位完美的关键就是别让系统重采样。
/// 设备没上报采样率列表（UNSPECIFIED）时返回 None，调用方走设备默认值。
pub fn pick_sample_rate(device: &OutputDeviceInfo, preferred: u32) -> Option<u32> {
    if device.sample_rates.is_empty() {
        return None;
    }
    if device.sample_rates.contains(&preferred) {
        return Some(preferred);
    }
    // 退而求其次：同样本族里最接近的一个（44.1k 用不到就落 48k，反之亦然）
    device
        .sample_rates
        .iter()
        .min_by_key(|sr| sr.abs_diff(preferred))
        .copied()
}

/// 设备支持的声道数与期望声道数的交集：取不超过期望值的最大支持值
pub fn pick_channel_count(device: &OutputDeviceInfo, preferred: u16) -> Option<u16> {
    if device.channel_counts.is_empty() {
        return None;
    }
    device
        .channel_counts
        .iter()
        .copied()
        .filter(|c| *c <= preferred)
        .max()
        .or_else(|| device.channel_counts.iter().copied().min())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(rates: Vec<u32>, chans: Vec<u16>) -> OutputDeviceInfo {
        OutputDeviceInfo {
            id: 7,
            name: "USB Audio".to_string(),
            type_name: "USB_DEVICE".to_string(),
            is_usb: true,
            sample_rates: rates,
            channel_counts: chans,
            encodings: vec![2, 4],
        }
    }

    #[test]
    fn picks_the_exact_native_rate() {
        let d = device(vec![44100, 48000, 96000], vec![2]);
        assert_eq!(pick_sample_rate(&d, 96000), Some(96000));
        assert_eq!(pick_sample_rate(&d, 44100), Some(44100));
    }

    #[test]
    fn falls_back_to_the_closest_rate() {
        let d = device(vec![48000, 96000], vec![2]);
        assert_eq!(pick_sample_rate(&d, 44100), Some(48000));
    }

    #[test]
    fn unspecified_rates_are_left_to_the_device() {
        let d = device(vec![], vec![]);
        assert_eq!(pick_sample_rate(&d, 44100), None);
        assert_eq!(pick_channel_count(&d, 2), None);
    }

    #[test]
    fn channel_count_never_exceeds_the_source() {
        let d = device(vec![48000], vec![2, 4, 6]);
        assert_eq!(pick_channel_count(&d, 2), Some(2));
        assert_eq!(pick_channel_count(&d, 6), Some(6));
    }

    #[test]
    fn device_only_wider_than_source_takes_the_minimum() {
        let d = device(vec![48000], vec![4, 8]);
        assert_eq!(pick_channel_count(&d, 2), Some(4));
    }
}
