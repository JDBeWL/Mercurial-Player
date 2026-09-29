//! 均衡器模块：10 段参数均衡，处理链内嵌在解码路径里。

pub mod commands;
pub mod processor;

// 重新导出常用类型
pub use processor::{
    BiquadCoefficients, BiquadState, EQ_BAND_COUNT, EQ_FREQUENCIES, EQ_Q_VALUES, EqPreset,
    EqSettings, GlobalEqualizer, get_all_presets,
};
