//! ONNX Runtime 后端：Windows 上走 DirectML，让模型跑在显卡上。
//!
//! session 必须跨曲目复用（创建约 0.9 秒），首次推理含一次 GPU kernel 编译，切歌前要预热。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use beat_this::{Model, Tensor};
use ort::ep::{CPU, DirectML};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Value;

use crate::error::AppError;

/// 推理跑在哪块硬件上，用于诊断和日志
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    /// 显卡（DirectML）
    Gpu,
    /// CPU（ORT 的 MLAS 内核）
    Cpu,
}

impl Device {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            // 与 `beat_model::backend_hint` 保持同一套小写取值：前者用于"还没分析过"的
            // 预估，后者是实际结果，前端按同一个字符串比对
            Self::Gpu => "directml",
            Self::Cpu => "onnxruntime-cpu",
        }
    }
}

/// 一个 ONNX 模型的推理会话
pub struct OrtModel {
    session: Session,
}

impl Model for OrtModel {
    fn run(&mut self, inputs: &[(&str, &Tensor)]) -> anyhow::Result<HashMap<String, Tensor>> {
        let (name, tensor) = inputs
            .first()
            .copied()
            .ok_or_else(|| anyhow!("节拍推理没有输入"))?;
        let value = Value::from_array((tensor.shape.clone(), tensor.data.clone()))?;
        let outputs = self.session.run(ort::inputs![name => value])?;

        let mut result = HashMap::new();
        for (out_name, out) in &outputs {
            let (shape, data) = out.try_extract_tensor::<f32>()?;
            result.insert(
                out_name.to_string(),
                Tensor {
                    shape: shape.iter().map(|&d| d as usize).collect(),
                    data: data.to_vec(),
                },
            );
        }
        Ok(result)
    }
}

/// 加载两个模型文件，优先用显卡。
///
/// 返回实际用上的设备——调用方据此打日志，也让"说好的 GPU 其实退回了 CPU"
/// 这种静默降级能被看见。
pub fn load_pair(
    mel_path: &Path,
    beat_path: &Path,
) -> Result<(OrtModel, OrtModel, Device), AppError> {
    // 先试显卡。DirectML 在虚拟机、远程桌面、老显卡上没有可用设备，
    // `with_execution_providers` 会返回 Err 而不是 panic，可以安全回退。
    match (load(mel_path, true), load(beat_path, true)) {
        (Ok(mel), Ok(beat)) => Ok((mel, beat, Device::Gpu)),
        (mel, beat) => {
            let reason = mel
                .err()
                .map(|e| e.to_string())
                .or_else(|| beat.err().map(|e| e.to_string()))
                .unwrap_or_default();
            log::warn!("DirectML 不可用，回退 CPU：{reason}");
            let mel = load(mel_path, false)?;
            let beat = load(beat_path, false)?;
            Ok((mel, beat, Device::Cpu))
        }
    }
}

fn load(path: &Path, gpu: bool) -> Result<OrtModel, AppError> {
    let builder = Session::builder()
        .map_err(|e| AppError::Audio(format!("ORT 会话创建失败: {e}")))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| AppError::Audio(format!("ORT 优化级别设置失败: {e}")))?
        // 0 = 交给 ORT 按核数决定；写死线程数在部分平台会慢数倍
        .with_intra_threads(0)
        .map_err(|e| AppError::Audio(format!("ORT 线程数设置失败: {e}")))?;

    let mut builder = if gpu {
        builder
            .with_execution_providers([DirectML::default().with_device_id(0).build()])
            .map_err(|e| AppError::Audio(format!("DirectML 注册失败: {e}")))?
    } else {
        builder
            .with_execution_providers([CPU::default().build()])
            .map_err(|e| AppError::Audio(format!("CPU 执行器注册失败: {e}")))?
    };

    let session = builder
        .commit_from_file(path)
        .map_err(|e| AppError::Audio(format!("ORT 加载模型失败: {e}")))?;
    Ok(OrtModel { session })
}

/// 运行库是否就位（只查文件，不加载、不占显存），用于预估耗时。
///
/// 必须把 `providers_shared` 一起算上：缺它时 GPU 会话建不起来，会静默回退 CPU。
#[must_use]
pub fn runtime_present() -> bool {
    runtime_dir().is_some_and(|dir| dir.join("onnxruntime_providers_shared.dll").exists())
}

/// 找出 onnxruntime 动态库并设置 `ORT_DYLIB_PATH`；返回 `None` 表示应退回 rten。
/// ort 加载失败是 panic，release 下 abort 接不住，所以先用 `libloading` 验证文件、映射与 `OrtGetApiBase`。
#[allow(unsafe_code)] // set_var 在 Rust 2024 起是 unsafe，理由见函数内 SAFETY 注释
pub fn prepare_runtime() -> Option<PathBuf> {
    let dylib = runtime_dir()?.join(dylib_name());
    if !dylib.exists() {
        log::debug!("未找到 {}，节拍分析使用 CPU 后端", dylib.display());
        return None;
    }
    // 已经设过就不覆盖：用户可能自己指定了别的版本
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return Some(dylib);
    }
    if let Err(e) = probe_dylib(&dylib) {
        log::warn!("{} 无法加载（{e}），节拍分析退回 CPU 后端", dylib.display());
        return None;
    }
    // SAFETY: 单线程初始化阶段调用；此后不再修改环境，符合 Rust 2024 对
    // set_var 的要求（该函数本身 unsafe 就是因为可能与并发读取竞争）。
    unsafe {
        std::env::set_var("ORT_DYLIB_PATH", &dylib);
    }
    Some(dylib)
}

#[cfg(windows)]
fn dylib_name() -> &'static str {
    "onnxruntime.dll"
}

#[cfg(not(windows))]
fn dylib_name() -> &'static str {
    "libonnxruntime.so"
}

/// 依次找：组件目录（生产）→ 可执行文件旁的 `runtime/`（旧布局）→
/// 源码树的 `resources/runtime/`（开发时）。
fn runtime_dir() -> Option<PathBuf> {
    if let Some(dir) = super::beat_model::component_dir() {
        let candidate = dir.join("runtime");
        if candidate.join(dylib_name()).exists() {
            return Some(candidate);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [dir.join("runtime"), dir.join("resources").join("runtime")] {
                if candidate.join(dylib_name()).exists() {
                    return Some(candidate);
                }
            }
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/runtime");
    if dev.join(dylib_name()).exists() {
        return Some(dev);
    }
    None
}

/// 真正把库映射进来并取一次符号。只有这一步成功，后面 ort 的 `expect` 才不会炸。
#[allow(unsafe_code)] // 加载动态库并查符号，本身定义就是 unsafe
fn probe_dylib(path: &Path) -> Result<(), String> {
    // SAFETY: 只是加载库并查符号，不调用任何函数；库句柄用完即弃，
    // 系统会因引用计数保留映射，后续 ort 自己加载时仍能拿到同一份。
    unsafe {
        let lib = libloading::Library::new(path).map_err(|e| e.to_string())?;
        lib.get::<unsafe extern "C" fn() -> *const std::ffi::c_void>(b"OrtGetApiBase")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
