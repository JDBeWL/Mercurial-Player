//! WASAPI 独占模式检测和支持
//!
//! 这个模块提供 WASAPI 独占模式的检测功能。

use crate::error::AppError;
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, ShareMode, WaveFormat};

fn check_device_format(device_name: Option<&str>) -> Result<(u32, u16), AppError> {
    let enumerator = DeviceEnumerator::new()
        .map_err(|e| format!("Failed to create device enumerator: {e:?}"))?;

    let device = if let Some(name) = device_name {
        let collection = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|e| format!("Failed to get device collection: {e:?}"))?;

        collection
            .into_iter()
            .flatten()
            .find(|device| device.get_friendlyname().is_ok_and(|n| n == name))
            .ok_or_else(|| format!("Device not found: {name}"))?
    } else {
        enumerator
            .get_default_device(&Direction::Render)
            .map_err(|e| format!("Failed to get default device: {e:?}"))?
    };

    let audio_client = device
        .get_iaudioclient()
        .map_err(|e| format!("Failed to get audio client: {e:?}"))?;

    get_exclusive_format(&audio_client)
}

fn get_exclusive_format(audio_client: &AudioClient) -> Result<(u32, u16), AppError> {
    const FORMATS_TO_TRY: [(usize, usize, usize); 7] = [
        (48000, 2, 32),
        (44100, 2, 32),
        (96000, 2, 32),
        (48000, 2, 24),
        (44100, 2, 24),
        (48000, 2, 16),
        (44100, 2, 16),
    ];

    for (sample_rate, channels, bits) in FORMATS_TO_TRY {
        let sample_type = if bits == 32 {
            SampleType::Float
        } else {
            SampleType::Int
        };
        let wave_format = WaveFormat::new(bits, bits, &sample_type, sample_rate, channels, None);

        if audio_client
            .is_supported(&wave_format, &ShareMode::Exclusive)
            .is_ok()
        {
            return Ok((sample_rate as u32, channels as u16));
        }
    }

    Err("No supported exclusive format found".to_string().into())
}

/// 检查设备是否支持独占模式
pub fn check_device_exclusive_support(device_name: Option<&str>) -> Result<bool, AppError> {
    let _ = wasapi::initialize_mta();
    Ok(check_device_format(device_name).is_ok())
}
