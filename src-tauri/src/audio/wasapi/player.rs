//! WASAPI 独占模式检测和支持
//!
//! 这个模块提供 WASAPI 独占模式的检测功能。

#![allow(dead_code)]

use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, ShareMode, WaveFormat};

/// WASAPI 独占模式播放器占位符
pub struct WasapiExclusivePlayer {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_exclusive: bool,
}

impl WasapiExclusivePlayer {
    pub fn new(device_name: Option<&str>) -> Result<Self, String> {
        let _ = wasapi::initialize_mta();

        let (actual_device_name, sample_rate, channels) = if let Some(name) = device_name {
            let (sr, ch) = check_device_format(Some(name))?;
            (name.to_string(), sr, ch)
        } else {
            let (sr, ch) = check_device_format(None)?;
            ("Default Device".to_string(), sr, ch)
        };

        println!("WASAPI Exclusive Mode available: {actual_device_name} @ {sample_rate}Hz, {channels} channels");

        Ok(Self {
            device_name: actual_device_name,
            sample_rate,
            channels,
            is_exclusive: true,
        })
    }

    #[must_use]
    pub fn get_device_name(&self) -> &str {
        &self.device_name
    }

    #[must_use]
    pub const fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn get_channels(&self) -> u16 {
        self.channels
    }
}

fn check_device_format(device_name: Option<&str>) -> Result<(u32, u16), String> {
    let enumerator =
        DeviceEnumerator::new().map_err(|e| format!("Failed to create device enumerator: {e:?}"))?;

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

fn get_exclusive_format(audio_client: &AudioClient) -> Result<(u32, u16), String> {
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
        let sample_type = if bits == 32 { SampleType::Float } else { SampleType::Int };
        let wave_format = WaveFormat::new(bits, bits, &sample_type, sample_rate, channels, None);

        if audio_client.is_supported(&wave_format, &ShareMode::Exclusive).is_ok() {
            return Ok((sample_rate as u32, channels as u16));
        }
    }

    Err("No supported exclusive format found".to_string())
}

/// 检查设备是否支持独占模式
pub fn check_device_exclusive_support(device_name: Option<&str>) -> Result<bool, String> {
    let _ = wasapi::initialize_mta();
    Ok(check_device_format(device_name).is_ok())
}

/// 获取所有支持独占模式的设备
pub fn get_exclusive_capable_devices() -> Result<Vec<String>, String> {
    let _ = wasapi::initialize_mta();

    let enumerator =
        DeviceEnumerator::new().map_err(|e| format!("Failed to create device enumerator: {e:?}"))?;

    let collection = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(|e| format!("Failed to get device collection: {e:?}"))?;

    let capable_devices: Vec<String> = collection
        .into_iter()
        .flatten()
        .filter_map(|device| {
            let name = device.get_friendlyname().ok()?;
            let audio_client = device.get_iaudioclient().ok()?;
            get_exclusive_format(&audio_client).ok().map(|_| name)
        })
        .collect();

    Ok(capable_devices)
}
