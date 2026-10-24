//! 均衡器
//!
//! 10 段参数均衡的系数、设置与预设；逐采样滤波的 `EqProcessor` 在 `crate::audio::eq_processor`。

pub mod commands;
pub mod processor;

pub use processor::{
    BiquadCoefficients, BiquadState, EQ_BAND_COUNT, EQ_FREQUENCIES, EQ_Q_VALUES, EqPreset,
    EqSettings, GlobalEqualizer, get_all_presets,
};
