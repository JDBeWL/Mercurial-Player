//! 均衡器模块：10 段参数均衡的系数、设置与预设。
//!
//! 播放链路里实际逐采样跑滤波的 `EqProcessor` 在 [`crate::audio::eq_processor`]：
//! 它要用音频侧的软削波查找表，放这里会让 equalizer 反向依赖 audio。

pub mod commands;
pub mod processor;

// 重新导出常用类型
pub use processor::{
    BiquadCoefficients, BiquadState, EQ_BAND_COUNT, EQ_FREQUENCIES, EQ_Q_VALUES, EqPreset,
    EqSettings, GlobalEqualizer, get_all_presets,
};
