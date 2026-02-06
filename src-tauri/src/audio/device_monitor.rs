//! 音频设备监听模块
//!
//! 监听音频设备的连接和断开事件，并在设备断开时自动切换到其他可用设备。

use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

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
        *self.current_device.lock().unwrap() = device_name;
    }

    /// 获取当前设备
    pub fn get_current_device(&self) -> String {
        self.current_device.lock().unwrap().clone()
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

    while is_running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_secs(1));

        let current_devices = get_device_names(&host);
        let current_device_name = current_device.lock().unwrap().clone();

        // 检查设备是否被移除
        if !current_devices.contains(&current_device_name) && previous_devices.contains(&current_device_name) {
            println!("Device removed: {}", current_device_name);
            
            // 发送设备移除事件
            let _ = app.emit("device-removed", DeviceChangeEvent {
                event_type: "device-removed".to_string(),
                device_name: Some(current_device_name.clone()),
            });

            // 尝试切换到其他可用设备
            if let Some(fallback_device) = find_fallback_device(&host, &current_device_name) {
                println!("Switching to fallback device: {}", fallback_device);
                
                // 发送设备切换请求
                let _ = app.emit("device-switch-required", DeviceChangeEvent {
                    event_type: "device-switch-required".to_string(),
                    device_name: Some(fallback_device.clone()),
                });
            } else {
                println!("No fallback device available");
                
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
                println!("Device added: {}", device_name);
                
                let _ = app.emit("device-added", DeviceChangeEvent {
                    event_type: "device-added".to_string(),
                    device_name: Some(device_name.clone()),
                });

                // 检查新添加的设备是否为系统默认设备
                if let Some(default_device) = host.default_output_device() {
                    if let Ok(default_name) = default_device.name() {
                        if default_name == *device_name {
                            println!("New device is system default, switching to: {}", device_name);
                            
                            // 发送自动切换到默认设备的事件
                            let _ = app.emit("device-default-changed", DeviceChangeEvent {
                                event_type: "device-default-changed".to_string(),
                                device_name: Some(device_name.clone()),
                            });
                        }
                    }
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
                .filter_map(|device| device.name().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// 查找备用设备
fn find_fallback_device(host: &cpal::Host, excluded_device: &str) -> Option<String> {
    // 首先尝试默认设备
    if let Some(default_device) = host.default_output_device() {
        if let Ok(name) = default_device.name() {
            if name != excluded_device {
                return Some(name);
            }
        }
    }

    // 如果默认设备不可用，选择第一个可用设备
    host.output_devices()
        .ok()?
        .filter_map(|device| device.name().ok())
        .find(|name| name != excluded_device)
}
