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
///
/// Android 走的是特例：cpal 的 AAudio 后端会把系统里所有 OUTPUT 条目（扬声器、听筒、
/// 蓝牙、HDMI……）都枚举出来，而它们的 `productName` 常常都是同一个**手机型号**。
/// 结果就是设置页出现三行一模一样的名字，既无法区分，也没法操作——更何况音频路由
/// 本来就是系统统一管理的事（插上蓝牙/耳机自动切过去），App 里选设备既不符合平台
/// 惯例，强行指定也往往建不出流。因此这里只上报默认设备作为占位。
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
            // 同名设备去重：切换设备是按 name 匹配的（见 name_to_device_id），
            // 同名意味着其中只有第一个可被选中，其余项点了没反应；
            // 前端又拿 name 当 v-for 的 key，重复名字还会触发 key 冲突。
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

// ============================================================================
// 跨平台音频设备标识 (Device ID)
// ============================================================================
// 设备友好名会随驱动更新、系统语言而变化,不适合作为持久化标识。cpal 的
// `DeviceTrait::id()` 在各平台返回的都是原生稳定标识:
//   - Windows (WASAPI)  : IMMDevice::GetId() 的 endpoint ID
//                         {0.0.0.00000000}.{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}
//   - macOS (CoreAudio) : kAudioDevicePropertyDeviceUID,跨重启/重插保持稳定
//   - Linux (ALSA)      : PCM 名,如 hw:CARD=PCH,DEV=0
// 用户手动选择的设备以 `DeviceId` 的字符串形式("host:id")落盘
// (config.audio.preferredDeviceId),启动时再解析回具体设备。
//
// 注意:标识是机器绑定的,换机器或重装驱动后可能失效,此时静默回退到系统默认设备。
// 同样的芯片安装的驱动可能会被系统认为是同样设备，如CX31993的公版方案没有明确要求PID唯一，
// 可能会出现不同设备的标识相同的情况，这是已知的问题。

/// 枚举全部输出设备,返回 (device_id, friendly_name) 列表。
///
/// 统一走 cpal 枚举,保证 ID 与名称来自同一次枚举,避免跨枚举的 ID 与名称不匹配。
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

/// 判断枚举到的 ID 是否就是落盘的标识。
///
/// `candidate` 形如 `host:id`;早期版本在 Windows 上只落盘了裸 endpoint ID
/// (没有 host 前缀),这里额外剥掉前缀再比一次以兼容旧配置。
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

/// 启动时把落盘标识解析回 cpal 设备。
///
/// 优先让 cpal 用 `device_by_id` 自己匹配,避免在重名设备上按名称选错;
/// 标识过期或设备已拔出时返回 None,调用方回退到系统默认设备。
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
        // 当前落盘格式 host:id,WASAPI / CoreAudio / ALSA 三种形态都要能命中
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
        // 早期版本在 Windows 上只落盘裸 endpoint ID(无 host 前缀),需兼容
        assert!(device_id_matches(
            "wasapi:{0.0.0.00000000}.{abc-def}",
            "{0.0.0.00000000}.{abc-def}"
        ));
        // ID 里含冒号时只剥掉第一个冒号前的 host 段
        assert!(device_id_matches(
            "alsa:hw:CARD=PCH,DEV=0",
            "hw:CARD=PCH,DEV=0"
        ));
    }

    #[test]
    fn test_device_id_matches_rejects_mismatch() {
        assert!(!device_id_matches("wasapi:{a}", "wasapi:{b}"));
        assert!(!device_id_matches("wasapi:{a}", "coreaudio:{a}"));
        // 前缀剥离后仍不同
        assert!(!device_id_matches(
            "alsa:hw:CARD=PCH,DEV=0",
            "hw:CARD=PCH,DEV=1"
        ));
    }
}
