//! 音频设备监听模块
//!
//! 监听音频设备的连接和断开事件、设备断开时自动切换到其他可用设备，
//! 以及系统默认输出设备变化（包括无插拔时用户在系统设置中主动切换）时跟随切换。

use cpal::traits::HostTrait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use super::device::get_device_friendly_name;

/// 设备变更事件
#[derive(Debug, serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceChangeEvent {
    pub event_type: String, // "device-removed" 或 "device-added"
    pub device_name: Option<String>,
}

/// 设备监听器
pub struct DeviceMonitor {
    is_running: Arc<AtomicBool>,
    current_device: Arc<Mutex<String>>,
    monitor_thread: Option<thread::JoinHandle<()>>,
}

impl DeviceMonitor {
    /// 创建新的设备监听器
    pub fn new(initial_device: String) -> Self {
        Self {
            is_running: Arc::new(AtomicBool::new(false)),
            current_device: Arc::new(Mutex::new(initial_device)),
            monitor_thread: None,
        }
    }

    /// 启动设备监听
    pub fn start(&mut self, app_handle: AppHandle) {
        if self.is_running.load(Ordering::SeqCst) {
            return;
        }

        self.is_running.store(true, Ordering::SeqCst);
        let is_running = Arc::clone(&self.is_running);
        let current_device = Arc::clone(&self.current_device);

        let monitor_thread = thread::spawn(move || {
            monitor_device_changes(app_handle, is_running, current_device);
        });

        self.monitor_thread = Some(monitor_thread);
    }

    /// 停止设备监听
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(thread) = self.monitor_thread.take() {
            let _ = thread.join();
        }
    }

    /// 更新当前设备
    pub fn update_current_device(&self, device_name: String) {
        match self.current_device.lock() {
            Ok(mut current_device) => *current_device = device_name,
            Err(err) => log::error!("Failed to update current device: {err}"),
        }
    }

    /// 获取当前设备
    pub fn get_current_device(&self) -> String {
        match self.current_device.lock() {
            Ok(current_device) => current_device.clone(),
            Err(err) => {
                log::error!("Failed to get current device: {err}");
                String::new()
            }
        }
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 监听设备变更的主循环
fn monitor_device_changes(
    app: AppHandle,
    is_running: Arc<AtomicBool>,
    current_device: Arc<Mutex<String>>,
) {
    let host = cpal::default_host();
    let mut previous_devices = get_device_names(&host);
    let mut previous_default = get_default_device_name(&host);

    while is_running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));

        let current_devices = get_device_names(&host);
        let current_device_name = match current_device.lock() {
            Ok(device) => device.clone(),
            Err(err) => {
                log::error!("Failed to read current device in monitor loop: {err}");
                previous_devices = current_devices;
                continue;
            }
        };

        // 检查设备是否被移除
        if !current_devices.contains(&current_device_name) && previous_devices.contains(&current_device_name) {
            log::info!("Device removed: {current_device_name}");
            
            // 发送设备移除事件
            let _ = app.emit("device-removed", DeviceChangeEvent {
                event_type: "device-removed".to_string(),
                device_name: Some(current_device_name.clone()),
            });

            // 尝试切换到其他可用设备
            if let Some(fallback_device) = find_fallback_device(&host, &current_device_name) {
                log::warn!("Switching to fallback device: {fallback_device}");
                
                // 发送设备切换请求
                let _ = app.emit("device-switch-required", DeviceChangeEvent {
                    event_type: "device-switch-required".to_string(),
                    device_name: Some(fallback_device.clone()),
                });
            } else {
                log::error!("No fallback device available");
                
                // 发送无可用设备事件
                let _ = app.emit("no-device-available", DeviceChangeEvent {
                    event_type: "no-device-available".to_string(),
                    device_name: None,
                });
            }
        }

        // 检查新设备添加
        for device_name in &current_devices {
            if !previous_devices.contains(device_name) {
                log::info!("Device added: {device_name}");
                
                let _ = app.emit("device-added", DeviceChangeEvent {
                    event_type: "device-added".to_string(),
                    device_name: Some(device_name.clone()),
                });
            }
        }

        // 检查系统默认设备变化（无插拔时用户在系统设置中主动切换、
        // 或新设备插入导致默认设备自动变更，都会走到这里）
        let current_default = get_default_device_name(&host);
        if current_default != previous_default {
            let old_default = previous_default.take();
            previous_default = current_default.clone();

            // 仅当应用当前使用的设备是旧默认设备（即正在跟随系统默认输出）时，
            // 才跟随系统切换到新默认设备，避免打断用户手动指定的设备
            if let Some(new_default) = current_default {
                let is_following_default = old_default.as_deref() == Some(current_device_name.as_str());
                if is_following_default && new_default != current_device_name {
                    log::info!("System default device changed to: {new_default}, following");

                    let _ = app.emit("device-default-changed", DeviceChangeEvent {
                        event_type: "device-default-changed".to_string(),
                        device_name: Some(new_default),
                    });
                }
            }
        }

        previous_devices = current_devices;
    }
}

/// 获取所有设备名称
fn get_device_names(host: &cpal::Host) -> Vec<String> {
    host.output_devices()
        .ok()
        .map(|devices| {
            devices
                .filter_map(|device| get_device_friendly_name(&device))
                .collect()
        })
        .unwrap_or_default()
}

/// 获取系统默认输出设备名称
fn get_default_device_name(host: &cpal::Host) -> Option<String> {
    host.default_output_device()
        .and_then(|device| get_device_friendly_name(&device))
}

/// 查找备用设备
fn find_fallback_device(host: &cpal::Host, excluded_device: &str) -> Option<String> {
    // 首先尝试默认设备
    if let Some(default_device) = host.default_output_device() {
        if let Some(name) = get_device_friendly_name(&default_device) {
            if name != excluded_device {
                return Some(name);
            }
        }
    }

    // 如果默认设备不可用，选择第一个可用设备
    host.output_devices()
        .ok()?
        .filter_map(|device| get_device_friendly_name(&device))
        .find(|name| name != excluded_device)
}
