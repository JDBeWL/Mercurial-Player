//! WASAPI 独占模式检测和支持
//!
//! 这个模块提供 WASAPI 独占模式的检测功能。

#![allow(dead_code)]

use wasapi::{
    AudioClient, DeviceEnumerator, Direction, 
    SampleType, ShareMode, WaveFormat,
};

/// WASAPI 独占模式播放器占位符
pub struct WasapiExclusivePlayer {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_exclusive: bool,
}

impl WasapiExclusivePlayer {
    /// 创建新的 WASAPI 独占模式播放器（占位符）
    pub fn new(device_name: Option<&str>) -> Result<Self, String> {
        // 初始化 COM
        let _ = wasapi::initialize_mta();

        let (actual_device_name, sample_rate, channels) = if let Some(name) = device_name {
            let (sr, ch) = check_device_format(Some(name))?;
            (name.to_string(), sr, ch)
        } else {
            let (sr, ch) = check_device_format(None)?;
            ("Default Device".to_string(), sr, ch)
        };

        println!(
            "WASAPI Exclusive Mode available: {} @ {}Hz, {} channels",
            actual_device_name, sample_rate, channels
        );

        Ok(Self {
            device_name: actual_device_name,
            sample_rate,
            channels,
            is_exclusive: true,
        })
    }

    pub fn get_device_name(&self) -> &str {
        &self.device_name
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channels(&self) -> u16 {
        self.channels
    }
}

/// 检查设备格式
fn check_device_format(device_name: Option<&str>) -> Result<(u32, u16), String> {
    let enumerator = DeviceEnumerator::new()
        .map_err(|e| format!("Failed to create device enumerator: {:?}", e))?;

    let device = if let Some(name) = device_name {
        // 查找指定名称的设备
        let collection = enumerator
            .get_device_collection(&Direction::Render)
            .map_err(|e| format!("Failed to get device collection: {:?}", e))?;
        
        let mut found_device = None;
        for device in collection.into_iter().flatten() {
            if let Ok(friendly_name) = device.get_friendlyname() {
                if friendly_name == name {
                    found_device = Some(device);
                    break;
                }
            }
        }
        found_device.ok_or_else(|| format!("Device not found: {}", name))?
    } else {
        // 获取默认设备
        enumerator
            .get_default_device(&Direction::Render)
            .map_err(|e| format!("Failed to get default device: {:?}", e))?
    };

    let audio_client = device
        .get_iaudioclient()
        .map_err(|e| format!("Failed to get audio client: {:?}", e))?;

    get_exclusive_format(&audio_client)
}

/// 获取独占模式支持的格式
fn get_exclusive_format(audio_client: &AudioClient) -> Result<(u32, u16), String> {
    let formats_to_try: [(usize, usize, usize); 7] = [
        (48000, 2, 32),
        (44100, 2, 32),
        (96000, 2, 32),
        (48000, 2, 24),
        (44100, 2, 24),
        (48000, 2, 16),
        (44100, 2, 16),
    ];

    for (sample_rate, channels, bits) in formats_to_try {
        let sample_type = if bits == 32 {
            SampleType::Float
        } else {
            SampleType::Int
        };

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
#[allow(dead_code)]
pub fn get_exclusive_capable_devices() -> Result<Vec<String>, String> {
    let _ = wasapi::initialize_mta();

    let enumerator = DeviceEnumerator::new()
        .map_err(|e| format!("Failed to create device enumerator: {:?}", e))?;

    let collection = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(|e| format!("Failed to get device collection: {:?}", e))?;

    let mut capable_devices = Vec::new();

    for device in collection.into_iter().flatten() {
        if let Ok(name) = device.get_friendlyname() {
            if let Ok(audio_client) = device.get_iaudioclient() {
                if get_exclusive_format(&audio_client).is_ok() {
                    capable_devices.push(name);
                }
            }
        }
    }

    Ok(capable_devices)
}
