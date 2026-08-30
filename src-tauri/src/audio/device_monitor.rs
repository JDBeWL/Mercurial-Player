//! 音频设备监听模块
//!
//! 监听音频设备的连接和断开事件、设备断开时自动切换到其他可用设备，
//! 以及系统默认输出设备变化（包括无插拔时用户在系统设置中主动切换）时跟随切换。
//!
//! 所有平台统一使用 cpal 轮询实现。Windows 平台的 IMMNotificationClient 事件驱动
//! 实现保留在 `windows_impl` 模块中（通过 `IMM_NOTIFICATION` 常量启用），
//! 但由于 COM 回调触发和 previous_default 状态同步存在运行时可靠性问题，
//! 默认使用轮询模式以保证功能稳定。

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

// ============================================================================
// 基于 cpal 的轮询实现（所有平台共用）
// ============================================================================

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
        thread::sleep(Duration::from_millis(500));

        let current_devices = get_device_names(&host);
        let current_device_name = match current_device.lock() {
            Ok(device) => device.clone(),
            Err(err) => {
                // 锁失败时不要更新 previous_devices,直接 continue 保留旧基准,
                // 否则会用本次的设备列表覆盖基准,导致下次循环时设备变更漏检
                log::error!("Failed to read current device in monitor loop: {err}");
                continue;
            }
        };

        // 检查设备是否被移除
        if !current_devices.contains(&current_device_name)
            && previous_devices.contains(&current_device_name)
        {
            log::info!("Device removed: {current_device_name}");

            // 发送设备移除事件
            let _ = app.emit(
                "device-removed",
                DeviceChangeEvent {
                    event_type: "device-removed".to_string(),
                    device_name: Some(current_device_name.clone()),
                },
            );

            // 尝试切换到其他可用设备
            if let Some(fallback_device) = find_fallback_device(&host, &current_device_name) {
                log::warn!("Switching to fallback device: {fallback_device}");

                // 发送设备切换请求
                let _ = app.emit(
                    "device-switch-required",
                    DeviceChangeEvent {
                        event_type: "device-switch-required".to_string(),
                        device_name: Some(fallback_device.clone()),
                    },
                );
            } else {
                log::error!("No fallback device available");

                // 发送无可用设备事件
                let _ = app.emit(
                    "no-device-available",
                    DeviceChangeEvent {
                        event_type: "no-device-available".to_string(),
                        device_name: None,
                    },
                );
            }
        }

        // 检查新设备添加
        for device_name in &current_devices {
            if !previous_devices.contains(device_name) {
                log::info!("Device added: {device_name}");

                let _ = app.emit(
                    "device-added",
                    DeviceChangeEvent {
                        event_type: "device-added".to_string(),
                        device_name: Some(device_name.clone()),
                    },
                );
            }
        }

        // 检查系统默认设备变化（无插拔时用户在系统设置中主动切换、
        // 或新设备插入导致默认设备自动变更，都会走到这里）
        let current_default = get_default_device_name(&host);
        if current_default != previous_default {
            let old_default = previous_default.take();
            previous_default.clone_from(&current_default);

            // 仅当应用当前使用的设备是旧默认设备（即正在跟随系统默认输出）时，
            // 才跟随系统切换到新默认设备，避免打断用户手动指定的设备
            if let Some(new_default) = current_default {
                let is_following_default =
                    old_default.as_deref() == Some(current_device_name.as_str());
                if is_following_default && new_default != current_device_name {
                    log::info!("System default device changed to: {new_default}, following");

                    let _ = app.emit(
                        "device-default-changed",
                        DeviceChangeEvent {
                            event_type: "device-default-changed".to_string(),
                            device_name: Some(new_default),
                        },
                    );
                }
            }
        }

        previous_devices = current_devices;
    }
}

// ============================================================================
// 辅助函数（双平台共用）
// ============================================================================

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

// ============================================================================
// Windows 平台：基于 IMMNotificationClient 的事件驱动实现（已禁用）
// ============================================================================
// 保留此模块供未来调试。默认使用轮询模式，因为 IMMNotificationClient 的
// COM 回调触发和 previous_default 状态同步在运行时存在可靠性问题。
// 如需启用，在 Cargo.toml 添加 `imm-notification` feature。
// ============================================================================

#[cfg(all(target_os = "windows", feature = "imm-notification"))]
mod windows_impl {
    #![allow(unsafe_code)]
    #![allow(clippy::ref_as_ptr)]

    use super::{DeviceChangeEvent, find_fallback_device, get_default_device_name};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};

    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, DEVICE_STATE_DISABLED, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
        EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl,
        MMDeviceEnumerator, eRender,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, STGM_READ,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::core::{PCWSTR, PWSTR};

    /// PKEY_Device_FriendlyName = {a45c254e-df08-4e93-bf1a-d1c97c2b3e08}, 14
    ///
    /// 该常量与 cpal 0.17 WASAPI 后端读取的 FriendlyName 属性完全相同，
    /// 因此通过 IPropertyStore 获取的设备名称会与 `device.rs` 中
    /// `get_device_friendly_name` 返回的名称一致。
    const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0xa45c254e_df08_4e93_bf1a_d1c97c2b3e08),
        pid: 14,
    };

    /// COM 回调对象：实现 IMMNotificationClient 接口接收系统音频设备变更通知。
    ///
    /// 字段必须全部 `Send + Sync`，因为 IMMNotificationClient 回调可能从
    /// 任意 RPC 线程触发（MTA 模式下）。
    #[windows::core::implement(IMMNotificationClient)]
    struct DeviceNotificationClient {
        app: AppHandle,
        current_device: Arc<Mutex<String>>,
        /// 上一次系统默认输出设备名称（用于检测"跟随系统默认"的切换）
        previous_default: Arc<Mutex<Option<String>>>,
        /// 缓存的 IMMDeviceEnumerator，用于在回调中根据 device id 查询设备属性
        enumerator: IMMDeviceEnumerator,
    }

    impl DeviceNotificationClient {
        /// 创建回调对象。`enumerator` 必须传入已创建的 IMMDeviceEnumerator 实例。
        fn new(
            app: AppHandle,
            current_device: Arc<Mutex<String>>,
            previous_default: Arc<Mutex<Option<String>>>,
            enumerator: IMMDeviceEnumerator,
        ) -> Self {
            Self {
                app,
                current_device,
                previous_default,
                enumerator,
            }
        }

        /// 根据设备 ID 获取设备的 FriendlyName。
        ///
        /// 返回值与 `device.rs` 中 `get_device_friendly_name` 的输出一致，
        /// 因为两者都读取 `PKEY_Device_FriendlyName` 属性。
        ///
        /// # Safety
        /// 调用者必须保证当前线程已通过 `CoInitializeEx` 初始化 COM。
        unsafe fn get_device_name_by_id(&self, device_id: PCWSTR) -> Option<String> {
            // SAFETY: 调用者保证当前线程已 CoInitializeEx
            let device = unsafe { self.enumerator.GetDevice(device_id).ok()? };

            // SAFETY: device 是有效的 IMMDevice，OpenPropertyStore 在同一线程调用
            let prop_store: IPropertyStore = unsafe { device.OpenPropertyStore(STGM_READ).ok()? };

            // SAFETY: key 是常量指针，PROPVARIANT 由 GetValue 写入
            let prop_variant = unsafe { prop_store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME).ok()? };

            // 将 PROPVARIANT 转换为字符串
            // SAFETY: prop_variant 已通过 GetValue 写入，PropVariantToStringAlloc 读取它
            let name_ptr: PWSTR =
                unsafe { PropVariantToStringAlloc(&raw const prop_variant).ok()? };

            // SAFETY: name_ptr 指向 CoTaskMemAlloc 分配的宽字符串，PCWSTR::to_string 复制到 Rust
            let name = unsafe { PCWSTR(name_ptr.0).to_string().ok() };

            // SAFETY: name_ptr 由 PropVariantToStringAlloc 通过 CoTaskMemAlloc 分配
            unsafe { CoTaskMemFree(Some(name_ptr.0 as *const _)) };

            name
        }

        /// 在设备移除/断开时寻找备用设备并发出对应事件。
        fn handle_current_device_removed(&self, removed_name: &str) {
            log::info!("Device removed: {removed_name}");

            let _ = self.app.emit(
                "device-removed",
                DeviceChangeEvent {
                    event_type: "device-removed".to_string(),
                    device_name: Some(removed_name.to_string()),
                },
            );

            // 尝试通过 cpal 寻找备用设备
            let host = cpal::default_host();
            if let Some(fallback_device) = find_fallback_device(&host, removed_name) {
                log::warn!("Switching to fallback device: {fallback_device}");
                let _ = self.app.emit(
                    "device-switch-required",
                    DeviceChangeEvent {
                        event_type: "device-switch-required".to_string(),
                        device_name: Some(fallback_device),
                    },
                );
            } else {
                log::error!("No fallback device available");
                let _ = self.app.emit(
                    "no-device-available",
                    DeviceChangeEvent {
                        event_type: "no-device-available".to_string(),
                        device_name: None,
                    },
                );
            }
        }
    }

    /// 实现 IMMNotificationClient 接口。注意：trait 必须实现到 `*_Impl` 类型上
    /// （由 `#[implement]` 宏生成），而不是原始类型，因为 `IUnknownImpl` 由宏
    /// 在 `*_Impl` 上实现，trait 的 supertrait 要求才能被满足。
    impl IMMNotificationClient_Impl for DeviceNotificationClient_Impl {
        /// 设备状态变化（启用/禁用/插拔）。
        ///
        /// 仅当当前设备变为不可用状态时触发移除逻辑；
        /// 设备从 Disabled 变为 Active 时不触发 device-added 事件，
        /// 因为 IMMDeviceEnumerator 在枚举设备列表时仍会包含 Disabled 设备。
        fn OnDeviceStateChanged(
            &self,
            pwstrdeviceid: &PCWSTR,
            dwnewstate: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            // SAFETY: COM 已在 monitor 线程初始化
            let device_name = unsafe { self.get_device_name_by_id(*pwstrdeviceid) };

            match dwnewstate {
                // 设备变为不可用：检查是否为当前设备
                DEVICE_STATE_UNPLUGGED | DEVICE_STATE_NOTPRESENT | DEVICE_STATE_DISABLED => {
                    if let Some(ref name) = device_name {
                        let current = self
                            .current_device
                            .lock()
                            .map(|guard| guard.clone())
                            .unwrap_or_default();
                        if *name == current {
                            self.handle_current_device_removed(name);
                        }
                    }
                }
                _ => {}
            }

            Ok(())
        }

        /// 设备被添加（首次插入）。
        fn OnDeviceAdded(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
            if let Some(name) = unsafe { self.get_device_name_by_id(*pwstrdeviceid) } {
                log::info!("Device added: {name}");
                let _ = self.app.emit(
                    "device-added",
                    DeviceChangeEvent {
                        event_type: "device-added".to_string(),
                        device_name: Some(name),
                    },
                );
            }
            Ok(())
        }

        /// 设备被移除（硬件拔出）。
        fn OnDeviceRemoved(&self, pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
            if let Some(name) = unsafe { self.get_device_name_by_id(*pwstrdeviceid) } {
                let current = self
                    .current_device
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();

                if name == current {
                    self.handle_current_device_removed(&name);
                } else {
                    log::info!("Device removed: {name}");
                    let _ = self.app.emit(
                        "device-removed",
                        DeviceChangeEvent {
                            event_type: "device-removed".to_string(),
                            device_name: Some(name),
                        },
                    );
                }
            }
            Ok(())
        }

        /// 系统默认设备变化。
        ///
        /// 仅当应用当前使用的设备是旧默认设备（即正在跟随系统默认输出）时，
        /// 才跟随系统切换到新默认设备，避免打断用户手动指定的设备。
        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            _role: ERole,
            pwstrdefaultdeviceid: &PCWSTR,
        ) -> windows::core::Result<()> {
            // 只关心输出设备（render）的默认变化
            if flow != eRender {
                return Ok(());
            }

            let new_default_name = unsafe { self.get_device_name_by_id(*pwstrdefaultdeviceid) };

            let new_default = match new_default_name {
                Some(name) => name,
                None => return Ok(()),
            };

            let old_default = self
                .previous_default
                .lock()
                .ok()
                .and_then(|mut guard| guard.take());

            // 更新缓存的默认设备
            if let Ok(mut guard) = self.previous_default.lock() {
                *guard = Some(new_default.clone());
            }

            let current_device_name = self
                .current_device
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default();

            // 仅当应用当前正在使用旧默认设备（即跟随系统默认）时才跟随切换
            let is_following_default = old_default.as_deref() == Some(current_device_name.as_str());
            if is_following_default && new_default != current_device_name {
                log::info!("System default device changed to: {new_default}, following");
                let _ = self.app.emit(
                    "device-default-changed",
                    DeviceChangeEvent {
                        event_type: "device-default-changed".to_string(),
                        device_name: Some(new_default),
                    },
                );
            }

            Ok(())
        }

        /// 设备属性变化（不处理，仅满足接口要求）。
        fn OnPropertyValueChanged(
            &self,
            _pwstrdeviceid: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }

    /// Windows 平台监听主入口。
    ///
    /// 在独立线程上初始化 COM（MTA），创建 `IMMDeviceEnumerator` 并注册
    /// `IMMNotificationClient` 回调，然后进入等待循环直到 `is_running` 变为 false。
    pub fn run_windows_monitor(
        app: AppHandle,
        is_running: Arc<AtomicBool>,
        current_device: Arc<Mutex<String>>,
    ) {
        // SAFETY: 本函数在专属的 device-monitor 线程上运行，所有 COM 调用都在该线程：
        // - CoInitializeEx(MTA) 在线程入口调用，CoUninitialize 在退出时配对
        // - IMMDeviceEnumerator 实例和回调注册都在同一线程完成
        unsafe {
            // 初始化 COM 库（多线程公寓 MTA）
            // MTA 模式下，IMMNotificationClient 回调可能从任意 RPC 线程触发，
            // DeviceNotificationClient 的字段（AppHandle / Arc<Mutex>）均 Send + Sync，线程安全。
            let com_initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if !com_initialized {
                log::error!("Device monitor: CoInitializeEx failed");
                return;
            }

            // 创建 IMMDeviceEnumerator 实例
            let enumerator: IMMDeviceEnumerator =
                match CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                    Ok(enumerator) => enumerator,
                    Err(e) => {
                        log::error!("Device monitor: CoCreateInstance failed: {e}");
                        CoUninitialize();
                        return;
                    }
                };

            // 初始化 previous_default 缓存（与轮询实现保持一致）
            let host = cpal::default_host();
            let previous_default = Arc::new(Mutex::new(get_default_device_name(&host)));

            // 创建回调对象
            let callback: IMMNotificationClient = DeviceNotificationClient::new(
                app,
                Arc::clone(&current_device),
                Arc::clone(&previous_default),
                enumerator.clone(),
            )
            .into();

            // 注册回调
            // SAFETY: enumerator 是有效的 IMMDeviceEnumerator，callback 是有效的 IMMNotificationClient
            if let Err(e) = enumerator.RegisterEndpointNotificationCallback(&callback) {
                log::error!("Device monitor: RegisterEndpointNotificationCallback failed: {e}");
                CoUninitialize();
                return;
            }

            log::info!("Device monitor: registered IMMNotificationClient callback");

            // 等待停止信号
            // MTA 模式下不需要消息泵，回调由 RPC 线程直接派发
            while is_running.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
            }

            // 注销回调
            // SAFETY: enumerator 和 callback 都有效，且 callback 之前已成功注册
            if let Err(e) = enumerator.UnregisterEndpointNotificationCallback(&callback) {
                log::warn!("Device monitor: UnregisterEndpointNotificationCallback failed: {e}");
            }

            // 释放 COM
            // SAFETY: 与 CoInitializeEx 配对，所有 COM 对象已释放
            CoUninitialize();

            log::info!("Device monitor thread exited");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::PKEY_DEVICE_FRIENDLY_NAME;

        /// 验证 PKEY_DEVICE_FRIENDLY_NAME 的 GUID 字节序是否正确。
        ///
        /// PKEY_Device_FriendlyName = {a45c254e-df08-4e93-bf1a-d1c97c2b3e08}, 14
        /// Data4 应为 [bf, 1a, d1, c9, 7c, 2b, 3e, 08]
        #[test]
        fn test_pkey_device_friendly_name_guid() {
            let guid = PKEY_DEVICE_FRIENDLY_NAME.fmtid;
            let expected_data4 = [0xbf, 0x1a, 0xd1, 0xc9, 0x7c, 0x2b, 0x3e, 0x08];

            assert_eq!(guid.data1, 0xa45c254e, "data1 mismatch");
            assert_eq!(guid.data2, 0xdf08, "data2 mismatch");
            assert_eq!(guid.data3, 0x4e93, "data3 mismatch");
            assert_eq!(
                guid.data4, expected_data4,
                "data4 byte order mismatch — GUID::from_u128 reverses Data4 on little-endian"
            );
            assert_eq!(PKEY_DEVICE_FRIENDLY_NAME.pid, 14, "pid mismatch");
        }
    }
}
