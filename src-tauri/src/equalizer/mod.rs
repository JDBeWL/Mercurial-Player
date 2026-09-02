//! 均衡器模块
//!
//! 提供 10 段参数均衡器功能。

pub mod commands;
pub mod processor;

// 重新导出常用类型
pub use processor::{
    BiquadCoefficients, BiquadState, EQ_BAND_COUNT, EQ_FREQUENCIES, EQ_Q_VALUES, EqPreset,
    EqSettings, GlobalEqualizer, get_all_presets,
};
