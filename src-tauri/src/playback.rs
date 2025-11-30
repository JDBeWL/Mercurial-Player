//! 音频播放相关的 Tauri 命令
//!
//! 这个模块包含所有与音频播放控制相关的功能，包括播放、暂停、恢复、音量控制等

use super::audio_decoder::SymphoniaDecoder;
use super::AppState;
use rodio::Source;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;
use tauri::{command, State};

/// 表示当前音频播放器的状态
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackStatus {
    #[serde(rename = "isPlaying")]
    pub is_playing: bool,
    #[serde(rename = "positionSecs")]
    pub position_secs: f32,
    #[serde(rename = "volume")]
    pub volume: f32,
}

impl PlaybackStatus {
    #[must_use]
    #[allow(dead_code)]
    pub const fn new(is_playing: bool, position_secs: f32, volume: f32) -> Self {
        Self {
            is_playing,
            position_secs,
            volume,
        }
    }
}

/// 播放指定的音轨
#[command]
pub fn play_track(
    state: State<AppState>,
    path: String,
    position: Option<f32>,
) -> Result<(), String> {
    let player_state = &state.player;

    // 停止当前音轨并清除sink
    {
        let sink = player_state.sink.lock().unwrap();
        sink.stop();
        // 播放新音轨前恢复音量
        let target_volume = *player_state.target_volume.lock().unwrap();
        sink.set_volume(target_volume);
    }

    // 更新当前音轨信息
    *player_state.current_path.lock().unwrap() = Some(path.clone());
    *player_state.current_source.lock().unwrap() = None;

    let source: Box<dyn Source<Item = f32> + Send> = match SymphoniaDecoder::new(&path) {
        Ok(mut symphonia_decoder) => {
            if let Some(time) = position {
                if let Err(e) = symphonia_decoder.seek(Duration::from_secs_f32(time)) {
                    eprintln!("Symphonia seek failed: {}", e);
                }
            }
            println!("使用Symphonia解码器: {}", path);
            Box::new(crate::audio_decoder::SymphoniaSource::new(
                symphonia_decoder,
            ))
        }
        Err(e) => {
            println!("Symphonia解码失败，回退到rodio解码器: {}", e);
            let file = File::open(&path).map_err(|e| e.to_string())?;
            let reader = BufReader::new(file);
            let rodio_source = rodio::Decoder::new(reader).map_err(|e| e.to_string())?;
            Box::new(rodio_source.convert_samples::<f32>())
        }
    };

    let sink = player_state.sink.lock().unwrap();
    sink.append(source);
    sink.play();

    Ok(())
}

/// 暂停当前播放的音轨
#[command]
pub fn pause_track(state: State<AppState>) -> Result<(), String> {
    let player_state = &state.player;
    let sink = Arc::clone(&player_state.sink);
    sink.lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?
        .pause();
    Ok(())
}

/// 恢复播放暂停的音轨
#[command]
pub fn resume_track(state: State<AppState>) -> Result<(), String> {
    let sink = state
        .player
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    sink.play();
    Ok(())
}

/// 设置播放音量
#[command]
pub fn set_volume(state: State<AppState>, volume: f32) -> Result<(), String> {
    // 验证音量范围
    if !(0.0..=1.0).contains(&volume) {
        return Err("Volume must be between 0.0 and 1.0".to_string());
    }

    let player_state = &state.player;
    *player_state
        .target_volume
        .lock()
        .map_err(|e| format!("Failed to acquire lock on target volume: {}", e))? = volume;

    let sink = player_state
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    sink.set_volume(volume);
    Ok(())
}

/// 获取当前播放状态
#[command]
pub fn get_playback_status(state: State<AppState>) -> Result<PlaybackStatus, String> {
    let player_state = &state.player;
    let sink = player_state.sink.lock().unwrap();

    let is_playing = !sink.is_paused();
    let volume = *player_state.target_volume.lock().unwrap();

    // 计算播放位置（这里简化处理，实际实现可能需要更复杂的逻辑）
    let position_secs = 0.0; // 占位符，实际实现需要从解码器获取

    Ok(PlaybackStatus::new(is_playing, position_secs, volume))
}

/// 检查当前音轨是否播放完毕
#[command]
pub fn is_track_finished(state: State<AppState>) -> Result<bool, String> {
    let sink = state
        .player
        .sink
        .lock()
        .map_err(|e| format!("Failed to acquire lock on sink: {}", e))?;
    // 检查sink是否为空且当前没有暂停状态
    Ok(sink.empty() && !sink.is_paused())
}

/// 跳转到音轨的指定位置
#[command]
pub fn seek_track(state: State<AppState>, time: f32) -> Result<(), String> {
    let player_state = &state.player;
    let duration = Duration::from_secs_f32(time);

    if let Some(path) = player_state.current_path.lock().unwrap().clone() {
        // 统一使用Symphonia进行seek
        match SymphoniaDecoder::new(&path) {
            Ok(mut decoder) => {
                println!("使用Symphonia seek for: {}", path);
                match decoder.seek(duration) {
                    Ok(_) => {
                        let source: Box<dyn Source<Item = f32> + Send> =
                            Box::new(crate::audio_decoder::SymphoniaSource::new(decoder));

                        // 停止当前播放并替换为新源
                        {
                            let sink = player_state.sink.lock().unwrap();
                            sink.stop();
                            let target_volume = *player_state.target_volume.lock().unwrap();
                            sink.set_volume(target_volume);
                        }

                        let sink = player_state.sink.lock().unwrap();
                        sink.append(source);
                        sink.play();
                        Ok(())
                    }
                    Err(e) => Err(format!("Failed to seek track with Symphonia: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to create decoder for seeking: {}", e)),
        }
    } else {
        Err("No track currently loaded".to_string())
    }
}
