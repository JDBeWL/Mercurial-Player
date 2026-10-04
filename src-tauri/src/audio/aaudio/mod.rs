//! Android AAudio 独占（位完美）输出：USB DAC 插上时绕过系统混音与重采样，按原生采样率直出。
//!
//! [`player`] 的接口与 `WasapiExclusivePlayback` 对齐，命令层与解码推送线程因此完全复用；
//! [`device`] 经 Kotlin `AudioManager` 查询设备信息，[`ffi`] 是 libaaudio 的 FFI 声明。

pub mod device;
pub mod ffi;
pub mod player;

pub use device::OutputDeviceInfo;
pub use player::AaudioExclusivePlayer;

use crate::error::AppError;
use serde::Serialize;

/// 给设置页的输出路由快照
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRouteInfo {
    /// 当前是否插着 USB 音频设备
    pub usb_connected: bool,
    /// USB 设备名（未连接时为空）
    pub usb_device_name: String,
    /// 独占模式是否已启用（配置项）
    pub exclusive_enabled: bool,
    /// 独占是否真的生效（开流后由 AAudio 回报；共享模式回退时为 false）
    pub exclusive_active: bool,
    /// 当前输出采样率（未开流时为 0）
    pub sample_rate: u32,
    pub channels: u16,
    /// 全部输出设备的摘要，便于排障
    pub devices: Vec<OutputDeviceInfo>,
}

/// 汇总当前输出路由，供设置页展示
pub fn audio_route_info(
    exclusive_enabled: bool,
    player: Option<&AaudioExclusivePlayer>,
) -> AudioRouteInfo {
    let devices = device::query_output_devices().unwrap_or_default();
    let usb = devices.iter().find(|d| d.is_usb).cloned();
    AudioRouteInfo {
        usb_connected: usb.is_some(),
        usb_device_name: usb.map_or_else(String::new, |d| d.name),
        exclusive_enabled,
        exclusive_active: player.is_some_and(AaudioExclusivePlayer::is_exclusive_confirmed),
        sample_rate: player.map_or(0, AaudioExclusivePlayer::sample_rate),
        channels: player.map_or(0, AaudioExclusivePlayer::channels),
        devices,
    }
}

/// 配置里"启用 USB DAC 独占"是否真的可用：得有 USB 设备才行
pub fn usb_dac_available() -> Result<bool, AppError> {
    Ok(device::find_usb_output_device()?.is_some())
}

/// USB DAC 热插拔：由 Kotlin 的 `AudioDeviceCallback` 经 JNI 调用。拔掉时作废独占流、关标志并
/// 暂停；插上时建流后置标志。
///
/// AAudio 的流死绑创建时的 device id，设备不在了就必须丢掉这条流
/// （[`AaudioExclusivePlayer::release_stream`]），否则拿旧 id 开流时独占与共享回退都会失败。
pub fn on_audio_route_changed(app: &tauri::AppHandle) {
    use tauri::Emitter;
    use tauri::Manager;

    let state = app.state::<crate::AppState>();

    // 设备查询走 JNI，可能失败。失败时什么都不做：宁可漏一次状态更新，
    // 也不能凭"这次没查到设备"就把用户正在放的音暂停掉。
    let devices = match device::query_output_devices() {
        Ok(devices) => devices,
        Err(e) => {
            log::warn!("on_audio_route_changed: 查询输出设备失败，跳过本次处理: {e}");
            return;
        }
    };
    let usb = devices.iter().any(|d| d.is_usb);
    let want_exclusive = state
        .config_manager
        .load_config()
        .map(|c| c.audio.usb_dac_exclusive)
        .unwrap_or(false)
        && usb;

    // 锁序遵循 audio/mod.rs 顶部的约定：exclusive_mode 先于 wasapi_player。下面的临界区会跨
    // `initialize()`（内含 JNI 设备查询与 openStream），但这条路径只在设备插拔时走，不与音频
    // 回调争锁。
    let Ok(mut exclusive) = state.player.output.exclusive_mode.lock() else {
        log::warn!("on_audio_route_changed: exclusive_mode 锁中毒");
        return;
    };

    // 字段沿用 `wasapi_player`：它在 Android 上装的是 AAudio 独占播放器
    // （`PlatformPlayer` 按平台取不同类型，字段只此一份，避免两条并行状态）
    let mut player_guard = match state.player.output.wasapi_player.lock() {
        Ok(guard) => guard,
        Err(e) => {
            log::warn!("on_audio_route_changed: 独占播放器锁中毒: {e}");
            return;
        }
    };

    if want_exclusive {
        // 现有流绑定的设备还在列表里吗？（插了第二个 USB 音频设备时不该打断正在播的声）
        let bound_is_gone = player_guard
            .as_ref()
            .and_then(AaudioExclusivePlayer::current_device)
            .is_some_and(|bound| !devices.iter().any(|d| d.id == bound.id));
        if bound_is_gone {
            log::info!("原 USB 音频设备已不在输出列表，作废独占流（下次播放按新设备重建）");
            if let Some(player) = player_guard.as_ref() {
                player.release_stream();
            }
        }

        // 没有流就现在建一条：应用启动时没插 DAC（播放器字段是 None），或流刚被上面作废。
        // 现在就建而不是等下次播放，设置页才能立刻反映"独占已生效"。
        if player_guard
            .as_ref()
            .is_none_or(|p| p.current_device().is_none())
        {
            let player = player_guard.take().unwrap_or_default();
            match player.initialize(None) {
                Ok((rate, channels, name)) => {
                    log::info!("USB DAC 独占就绪: {name} @ {rate}Hz, {channels} ch");
                }
                // 建流失败不致命：`ensure_format` 会在下次播放时再试一次
                Err(e) => {
                    log::warn!("USB DAC 已接入，但独占流未建成（下次播放会重试）: {e}");
                }
            }
            *player_guard = Some(player);
        }

        // 必须按"有没有真的开出一条流"判定，而不是 `player_guard.is_some()`：initialize 失败时
        // 这里存的是一条没有流的 default 播放器，按 is_some 会让设置页显示"独占已生效"、播放却
        // 走独占路径拿不到流（无声），共享输出也已被让位。
        *exclusive = player_guard
            .as_ref()
            .is_some_and(|p| p.current_device().is_some());
        if !*exclusive {
            log::warn!("独占未生效（无可用流），本次继续走共享输出");
        }
        drop(player_guard);
        drop(exclusive);
        let _ = app.emit(
            "audio-route-changed",
            serde_json::json!({ "usbConnected": usb }),
        );
        return;
    }

    // 不再具备独占条件：作废独占流并暂停，避免解码线程在一条死流上空转
    if let Some(player) = player_guard.as_ref() {
        let _ = player.stop();
        let _ = player.clear_buffer();
        // 设备没了，流本身也已失效，必须一起丢掉（见函数文档）
        player.release_stream();
    }
    drop(player_guard);
    *exclusive = false;
    drop(exclusive);

    if let Err(e) = crate::audio::queue::media_control(app, &state, "pause", None) {
        log::warn!("独占流失效后暂停失败，前端状态可能仍停留在播放: {e}");
    }
    // 共享模式的输出是启动时按当时的默认设备开的，同样不会自愈。
    // 趁已经暂停把它按当前默认设备重建，否则拔掉 DAC 后共享播放会一直没声音。
    if let Err(e) = crate::audio::rebuild_shared_sink(&state) {
        log::warn!("重建共享输出失败（下次播放可能仍指向旧设备）: {e}");
    }
    let _ = app.emit(
        "audio-route-changed",
        serde_json::json!({ "usbConnected": usb, "exclusive": false }),
    );
}
