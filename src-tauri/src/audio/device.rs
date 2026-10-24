//! 音频设备管理：设备枚举、友好名解析与跨平台稳定设备标识。

use crate::error::AppError;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub supports_exclusive_mode: bool,
    pub is_exclusive_mode: bool,
    pub audio_mode_status: String,
}

/// 从 cpal 设备取友好名称。WASAPI 后端把 `DEVPKEY_Device_FriendlyName` 放在 `extended()[0]`，
/// `description().name()` 只给 `DeviceDesc`（如 "Speakers"）；wasapi crate 按 FriendlyName 匹配设备，必须一致。
pub fn get_device_friendly_name(device: &cpal::Device) -> Option<String> {
    let desc = device.description().ok()?;
    if let Some(friendly) = desc.extended().first() {
        Some(friendly.clone())
    } else {
        // 无 extended 时 name() 已是 FriendlyName（没有 DeviceDesc 可用），或两者相同
        Some(desc.name().to_string())
    }
}

/// 获取所有可用的音频输出设备。Android 特例：AAudio 后端枚举出的 OUTPUT 条目 `productName`
/// 常是同一手机型号无法区分，且音频路由本就由系统统一管理，故只上报默认设备。
pub fn get_all_audio_devices() -> Result<Vec<AudioDeviceInfo>, AppError> {
    let host = cpal::default_host();

    #[cfg(target_os = "android")]
    let devices: Vec<cpal::Device> = host.default_output_device().into_iter().collect();

    #[cfg(not(target_os = "android"))]
    let devices: Vec<cpal::Device> = host.output_devices().map_err(|e| e.to_string())?.collect();

    let default_device_name = host
        .default_output_device()
        .and_then(|d| get_device_friendly_name(&d));

    let mut device_infos: Vec<AudioDeviceInfo> = Vec::new();

    for device in devices {
        if let Some(name) = get_device_friendly_name(&device) {
            // 同名设备去重：切换按 name 匹配（见 `name_to_device_id`），前端拿 name 当 v-for key，重复会 key 冲突。
            if device_infos
                .iter()
                .any(|d: &AudioDeviceInfo| d.name == name)
            {
                log::warn!("Skipping duplicate audio device name: {name}");
                continue;
            }

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

// 设备标识：友好名随驱动/语言变化不可持久化，落盘用 cpal `DeviceId` 字符串（"host:id"，config.audio.preferredDeviceId）；
// 各平台原生稳定（Windows WASAPI endpoint ID、macOS CoreAudio DeviceUID、Linux ALSA PCM 名），Android 由系统接管不参与。
// 标识机器绑定，换机/重装驱动后失效则静默回退默认设备；同芯片公版方案可能撞 ID（已知问题）。

/// 统一走 cpal 枚举，保证 ID 与名称来自同一次枚举，避免跨枚举不匹配。
fn enumerate_output_devices() -> Vec<(String, String)> {
    let host = cpal::default_host();
    let Ok(devices) = host.output_devices() else {
        log::warn!("Failed to enumerate output devices");
        return Vec::new();
    };

    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let name = get_device_friendly_name(&device)?;
            Some((id.to_string(), name))
        })
        .collect()
}

/// `candidate` 形如 `host:id`；早期版本在 Windows 只落盘裸 endpoint ID（无 host 前缀），这里多剥一次前缀兼容旧配置。
fn device_id_matches(candidate: &str, stored: &str) -> bool {
    candidate == stored
        || candidate
            .split_once(':')
            .is_some_and(|(_, raw)| raw == stored)
}

/// 根据落盘标识解析当前友好名称(用于启动恢复时的日志与状态展示)。
pub fn device_id_to_name(device_id: &str) -> Option<String> {
    enumerate_output_devices()
        .into_iter()
        .find(|(id, _)| device_id_matches(id, device_id))
        .map(|(_, name)| name)
}

/// 根据友好名称解析落盘标识(切换成功后写入 config.audio.preferredDeviceId)。
pub fn name_to_device_id(device_name: &str) -> Option<String> {
    enumerate_output_devices()
        .into_iter()
        .find(|(_, name)| name == device_name)
        .map(|(id, _)| id)
}

/// 启动时把落盘标识解析回 cpal 设备：优先让 `device_by_id` 匹配，避免在重名设备上按名称选错；
/// 标识过期或设备已拔出时返回 None，调用方回退系统默认设备。
pub fn resolve_preferred_device(preferred_id: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if let Ok(id) = preferred_id.parse::<cpal::DeviceId>() {
        if let Some(device) = host.device_by_id(&id) {
            return Some(device);
        }
    }

    // 兼容早期无 host 前缀的落盘格式
    host.output_devices().ok()?.find(|device| {
        device
            .id()
            .is_ok_and(|id| device_id_matches(&id.to_string(), preferred_id))
    })
}

#[cfg(test)]
mod tests {
    use super::device_id_matches;

    #[test]
    fn test_device_id_matches_current_format() {
        assert!(device_id_matches(
            "wasapi:{0.0.0.00000000}.{abc-def}",
            "wasapi:{0.0.0.00000000}.{abc-def}"
        ));
        assert!(device_id_matches(
            "coreaudio:AppleHDAEngineOutput:1B,0,1,1:0",
            "coreaudio:AppleHDAEngineOutput:1B,0,1,1:0"
        ));
        assert!(device_id_matches(
            "alsa:hw:CARD=PCH,DEV=0",
            "alsa:hw:CARD=PCH,DEV=0"
        ));
    }

    #[test]
    fn test_device_id_matches_legacy_bare_id() {
        assert!(device_id_matches(
            "wasapi:{0.0.0.00000000}.{abc-def}",
            "{0.0.0.00000000}.{abc-def}"
        ));
        assert!(device_id_matches(
            "alsa:hw:CARD=PCH,DEV=0",
            "hw:CARD=PCH,DEV=0"
        ));
    }

    #[test]
    fn test_device_id_matches_rejects_mismatch() {
        assert!(!device_id_matches("wasapi:{a}", "wasapi:{b}"));
        assert!(!device_id_matches("wasapi:{a}", "coreaudio:{a}"));
        assert!(!device_id_matches(
            "alsa:hw:CARD=PCH,DEV=0",
            "hw:CARD=PCH,DEV=1"
        ));
    }
}
