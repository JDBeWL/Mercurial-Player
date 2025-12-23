//! EQ 均衡器相关的 Tauri 命令

use super::processor::{get_all_presets, EqPreset, EqSettings, EQ_BAND_COUNT, EQ_FREQUENCIES};
use crate::AppState;
use tauri::{command, State};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EqBandInfo {
    pub index: usize,
    pub frequency: f32,
    pub label: String,
}

#[command]
pub fn get_eq_bands() -> Vec<EqBandInfo> {
    EQ_FREQUENCIES
        .iter()
        .enumerate()
        .map(|(i, &freq)| EqBandInfo {
            index: i,
            frequency: freq,
            label: format_frequency(freq),
        })
        .collect()
}

fn format_frequency(freq: f32) -> String {
    if freq >= 1000.0 {
        format!("{}k", (freq / 1000.0) as u32)
    } else {
        format!("{}", freq as u32)
    }
}

#[command]
pub fn get_eq_settings(state: State<AppState>) -> EqSettings {
    state.equalizer.get_settings()
}

#[command]
pub fn set_eq_enabled(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.equalizer.set_enabled(enabled);
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_enabled(enabled);
    }
    Ok(())
}

#[command]
pub fn set_eq_gains(state: State<AppState>, gains: Vec<f32>) -> Result<(), String> {
    if gains.len() != EQ_BAND_COUNT {
        return Err(format!("Expected {EQ_BAND_COUNT} gains, got {}", gains.len()));
    }

    let mut gains_array = [0.0f32; EQ_BAND_COUNT];
    for (i, &gain) in gains.iter().enumerate() {
        gains_array[i] = gain.clamp(-8.0, 8.0);
    }

    state.equalizer.set_gains(gains_array);
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_gains(gains_array);
    }
    Ok(())
}

#[command]
pub fn set_eq_band_gain(state: State<AppState>, band: usize, gain: f32) -> Result<(), String> {
    if band >= EQ_BAND_COUNT {
        return Err(format!("Invalid band index: {band}"));
    }

    let clamped_gain = gain.clamp(-8.0, 8.0);
    state.equalizer.set_band_gain(band, clamped_gain);
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_band_gain(band, clamped_gain);
    }
    Ok(())
}

#[command]
pub fn set_eq_preamp(state: State<AppState>, preamp: f32) -> Result<(), String> {
    let clamped_preamp = preamp.clamp(-8.0, 8.0);
    state.equalizer.set_preamp(clamped_preamp);
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_preamp(clamped_preamp);
    }
    Ok(())
}

#[command]
pub fn get_eq_presets() -> Vec<EqPreset> {
    get_all_presets()
}

#[command]
pub fn apply_eq_preset(state: State<AppState>, preset_name: String) -> Result<(), String> {
    let presets = get_all_presets();
    let preset = presets
        .iter()
        .find(|p| p.name == preset_name)
        .ok_or_else(|| format!("Preset not found: {preset_name}"))?;

    state.equalizer.set_gains(preset.gains);
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_gains(preset.gains);
    }
    Ok(())
}

#[command]
pub fn reset_eq(state: State<AppState>) -> Result<(), String> {
    let default_settings = EqSettings::default();
    state.equalizer.set_settings(default_settings.clone());
    if let Ok(mut eq) = state.player.equalizer.lock() {
        eq.set_gains(default_settings.gains);
        eq.set_preamp(default_settings.preamp);
        eq.set_enabled(default_settings.enabled);
    }
    Ok(())
}
