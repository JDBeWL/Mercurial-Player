//! 独占设备初始化：枚举默认输出设备、协商采样率/声道/位深并建立 AudioClient。

use crate::error::AppError;
use std::thread;
use std::time::Duration;

pub(super) fn initialize_exclusive_device(
    device_name: Option<&str>,
) -> Result<(wasapi::AudioClient, (u32, u16, String, u16, bool)), AppError> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat};

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

    let device_name = device
        .get_friendlyname()
        .unwrap_or_else(|_| "Unknown".to_string());
    let mut audio_client = device
        .get_iaudioclient()
        .map_err(|e| format!("Failed to get audio client: {e:?}"))?;

    let default_format = audio_client
        .get_mixformat()
        .map_err(|e| format!("Failed to get mix format: {e:?}"))?;
    let default_sample_rate = default_format.get_samplespersec() as usize;
    let default_channels = default_format.get_nchannels() as usize;

    log::info!("Device default format: {default_sample_rate}Hz, {default_channels} channels");

    let sample_rates_to_try: [usize; 12] = [
        default_sample_rate,
        384_000,
        352_800,
        192_000,
        176_400,
        96_000,
        88_200,
        48_000,
        44_100,
        32_000,
        22_050,
        16_000,
    ];
    let bit_depths: [(usize, bool); 4] = [(32, true), (32, false), (24, false), (16, false)];
    let channels_to_try: [usize; 2] = [default_channels, 2];

    let mut found_format = None;

    'outer: for &sample_rate in &sample_rates_to_try {
        for &channels in &channels_to_try {
            for &(bits, is_float) in &bit_depths {
                let sample_type = if is_float {
                    SampleType::Float
                } else {
                    SampleType::Int
                };
                let wave_format =
                    WaveFormat::new(bits, bits, &sample_type, sample_rate, channels, None);

                if audio_client
                    .is_supported(&wave_format, &ShareMode::Exclusive)
                    .is_ok()
                {
                    found_format = Some((
                        wave_format,
                        sample_rate as u32,
                        channels as u16,
                        bits as u16,
                        is_float,
                    ));
                    break 'outer;
                }
            }
        }
    }

    let (wave_format, sample_rate, channels, bits, is_float) =
        found_format.ok_or_else(|| "No supported exclusive format found".to_string())?;

    let (_default_period, min_period) = audio_client
        .get_device_period()
        .map_err(|e| format!("Failed to get device period: {e:?}"))?;

    let stream_mode = StreamMode::EventsExclusive {
        period_hns: min_period,
    };

    // 尝试初始化独占模式，添加重试机制
    let mut last_error = None;
    for attempt in 1..=3 {
        match audio_client.initialize_client(&wave_format, &Direction::Render, &stream_mode) {
            Ok(()) => {
                log::info!(
                    "WASAPI Exclusive Mode initialized: {device_name} @ {sample_rate}Hz, {channels} channels, {bits} bits, float: {is_float}"
                );
                return Ok((
                    audio_client,
                    (sample_rate, channels, device_name, bits, is_float),
                ));
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < 3 {
                    log::warn!(
                        "Exclusive mode initialization attempt {attempt} failed, retrying..."
                    );
                    thread::sleep(Duration::from_millis(100 * attempt as u64));

                    // 重新获取audio client
                    audio_client = device
                        .get_iaudioclient()
                        .map_err(|e| format!("Failed to get audio client on retry: {e:?}"))?;
                }
            }
        }
    }

    Err(format!(
        "Failed to initialize exclusive mode after 3 attempts: {last_error:?}. The device may be in use by another application or does not support exclusive mode."
    )
    .into())
}
