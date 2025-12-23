//! 音频设备管理模块
//!
//! 提供音频设备的检测、切换和管理功能。

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::StreamConfig;
use serde::Serialize;

/// 表示音频设备信息
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub supports_exclusive_mode: bool,
    pub is_exclusive_mode: bool,
    pub audio_mode_status: String,
}

/// 获取所有可用的音频输出设备
pub fn get_all_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let host = cpal::default_host();
    let default_device_name = host.default_output_device().and_then(|d| d.name().ok());

    let devices = host.output_devices().map_err(|e| e.to_string())?;
    let mut device_infos: Vec<AudioDeviceInfo> = Vec::new();

    for device in devices {
        if let Ok(name) = device.name() {
            let is_default = default_device_name.as_ref().is_some_and(|d_name| *d_name == name);
            let supports_exclusive_mode = check_wasapi_exclusive_support(&name);

            device_infos.push(AudioDeviceInfo {
                name,
                is_default,
                supports_exclusive_mode,
                is_exclusive_mode: false,
                audio_mode_status: "standard".to_string(),
            });
        }
    }

    Ok(device_infos)
}

fn check_wasapi_exclusive_support(device_name: &str) -> bool {
    super::wasapi::check_device_exclusive_support(Some(device_name)).unwrap_or_else(|e| {
        println!("Failed to check exclusive mode support for {device_name}: {e}");
        false
    })
}

/// 检测设备是否支持独占模式（使用 cpal）
#[allow(dead_code)]
pub fn check_exclusive_mode_support(device: &cpal::Device) -> bool {
    let config = match device.default_output_config() {
        Ok(config) => config,
        Err(_) => return false,
    };

    let mut stream_config: StreamConfig = config.into();
    stream_config.buffer_size = cpal::BufferSize::Fixed(256);

    device
        .build_output_stream(
            &stream_config,
            |_data: &mut [f32], _: &cpal::OutputCallbackInfo| {},
            |_err| {},
            None,
        )
        .is_ok()
}
