//! 音频设备管理模块
//!
//! 提供音频设备的检测、切换和管理功能。

use crate::error::AppError;
use cpal::traits::{DeviceTrait, HostTrait};
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

/// 从 cpal 设备获取友好名称。
///
/// cpal 0.17 WASAPI 后端的 `description().name()` 返回的是 `DEVPKEY_Device_DeviceDesc`
/// （如 "Speakers"），而不是 `DEVPKEY_Device_FriendlyName`（如 "Speakers (Realtek High Definition Audio)"）。
/// FriendlyName 在 DeviceDesc 和 FriendlyName 不同时会被放到 `extended()[0]` 中。
///
/// wasapi crate 的 `get_friendlyname()` 使用的是 FriendlyName，所以我们需要优先使用 FriendlyName
/// 来保持与 wasapi crate 的一致性，同时也能显示完整的设备名称给用户。
pub fn get_device_friendly_name(device: &cpal::Device) -> Option<String> {
    let desc = device.description().ok()?;
    // cpal 0.17 WASAPI 后端：当 DeviceDesc 存在且与 FriendlyName 不同时，
    // FriendlyName 被放到 extended[0]。我们优先使用它。
    if let Some(friendly) = desc.extended().first() {
        Some(friendly.clone())
    } else {
        // 没有 extended 信息意味着：
        // 1. name() 就是 FriendlyName（因为没有 DeviceDesc 可用）
        // 2. 或者 DeviceDesc == FriendlyName（两者相同，不需要 extended）
        Some(desc.name().to_string())
    }
}

/// 获取所有可用的音频输出设备
pub fn get_all_audio_devices() -> Result<Vec<AudioDeviceInfo>, AppError> {
    let host = cpal::default_host();
    let default_device_name = host
        .default_output_device()
        .and_then(|d| get_device_friendly_name(&d));

    let devices = host.output_devices().map_err(|e| e.to_string())?;
    let mut device_infos: Vec<AudioDeviceInfo> = Vec::new();

    for device in devices {
        if let Some(name) = get_device_friendly_name(&device) {
            let is_default = default_device_name
                .as_ref()
                .is_some_and(|d_name| *d_name == name);
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
    #[cfg(windows)]
    {
        super::wasapi::check_device_exclusive_support(Some(device_name)).unwrap_or_else(|e| {
            println!("Failed to check exclusive mode support for {device_name}: {e}");
            false
        })
    }
    #[cfg(not(windows))]
    {
        let _ = device_name;
        false
    }
}
