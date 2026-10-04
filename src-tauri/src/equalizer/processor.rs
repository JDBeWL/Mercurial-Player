//! 10 段参数均衡器的双二阶系数、滤波状态与 eq.json 持久化。

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

pub const EQ_BAND_COUNT: usize = 10;
pub const EQ_FREQUENCIES: [f32; EQ_BAND_COUNT] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const EQ_Q_VALUES: [f32; EQ_BAND_COUNT] = [0.7, 0.7, 0.8, 0.9, 1.0, 1.0, 1.1, 1.2, 1.3, 1.4];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqPreset {
    pub name: String,
    pub gains: [f32; EQ_BAND_COUNT],
}

impl EqPreset {
    #[must_use]
    pub fn flat() -> Self {
        Self {
            name: "Flat".to_string(),
            gains: [0.0; EQ_BAND_COUNT],
        }
    }
    #[must_use]
    pub fn bass_boost() -> Self {
        Self {
            name: "Bass Boost".to_string(),
            gains: [4.0, 3.5, 2.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        }
    }
    #[must_use]
    pub fn treble_boost() -> Self {
        Self {
            name: "Treble Boost".to_string(),
            gains: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 2.5, 3.5, 4.0],
        }
    }
    #[must_use]
    pub fn vocal() -> Self {
        Self {
            name: "Vocal".to_string(),
            gains: [-1.5, -1.0, 0.0, 1.5, 2.5, 2.5, 2.0, 0.5, 0.0, -0.5],
        }
    }
    #[must_use]
    pub fn rock() -> Self {
        Self {
            name: "Rock".to_string(),
            gains: [3.5, 2.5, 1.5, 0.0, -0.5, 0.0, 1.5, 2.0, 2.5, 3.0],
        }
    }
    #[must_use]
    pub fn pop() -> Self {
        Self {
            name: "Pop".to_string(),
            gains: [-0.5, 0.0, 1.5, 2.0, 2.5, 2.0, 0.5, 0.0, -0.5, -1.0],
        }
    }
    #[must_use]
    pub fn jazz() -> Self {
        Self {
            name: "Jazz".to_string(),
            gains: [2.0, 1.5, 0.5, 1.0, -1.0, -1.0, 0.0, 1.5, 2.0, 2.5],
        }
    }
    #[must_use]
    pub fn classical() -> Self {
        Self {
            name: "Classical".to_string(),
            gains: [2.5, 2.0, 1.5, 0.5, -0.5, -0.5, 0.0, 1.5, 2.0, 2.5],
        }
    }
    #[must_use]
    pub fn electronic() -> Self {
        Self {
            name: "Electronic".to_string(),
            gains: [3.5, 3.0, 0.5, 0.0, -1.0, 1.0, 0.5, 2.0, 2.5, 3.5],
        }
    }
    #[must_use]
    pub fn acoustic() -> Self {
        Self {
            name: "Acoustic".to_string(),
            gains: [2.0, 1.5, 0.5, 0.5, 1.5, 1.5, 1.5, 2.0, 1.5, 0.5],
        }
    }
}

#[must_use]
pub fn get_all_presets() -> Vec<EqPreset> {
    vec![
        EqPreset::bass_boost(),
        EqPreset::treble_boost(),
        EqPreset::vocal(),
        EqPreset::rock(),
        EqPreset::pop(),
        EqPreset::jazz(),
        EqPreset::classical(),
        EqPreset::electronic(),
        EqPreset::acoustic(),
    ]
}

#[derive(Debug, Clone, Copy)]
pub struct BiquadCoefficients {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for BiquadCoefficients {
    fn default() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }
}

impl BiquadCoefficients {
    /// 根据频段序号设计该 band 的滤波器:首尾用 shelf,中间用 peaking。
    ///
    /// 两端用 shelf 是因为用户对 31Hz/16kHz 的直觉是"低音/高音整体多一点",
    /// peaking 的钟形响应只在中心隆起、两端回落,容易诱导用户过量提升。
    #[must_use]
    pub fn for_band(sample_rate: f32, band: usize, gain_db: f32) -> Self {
        let freq = EQ_FREQUENCIES[band];
        if band == 0 {
            Self::low_shelf(sample_rate, freq, gain_db)
        } else if band == EQ_BAND_COUNT - 1 {
            Self::high_shelf(sample_rate, freq, gain_db)
        } else {
            Self::peaking_eq(sample_rate, freq, gain_db, EQ_Q_VALUES[band])
        }
    }

    /// RBJ peaking EQ。
    ///
    /// 系数用 f64 计算后转 f32 存储:31Hz@44.1kHz 的归一化频率极低,
    /// f32 直接算会有可观的系数量化误差。
    #[must_use]
    pub fn peaking_eq(sample_rate: f32, frequency: f32, gain_db: f32, q: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        let sr = f64::from(sample_rate);
        let f0 = f64::from(frequency);
        let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
        let omega = std::f64::consts::TAU * f0 / sr;
        let (sin_omega, cos_omega) = omega.sin_cos();
        let alpha = sin_omega / (2.0 * f64::from(q));
        let b0 = 1.0 + alpha * a;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: (b0 / a0) as f32,
            b1: (-2.0 * cos_omega / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (-2.0 * cos_omega / a0) as f32,
            a2: (a2 / a0) as f32,
        }
    }

    /// RBJ low shelf(Q = 1/sqrt(2),等价于 shelf slope S = 1 的 Butterworth 特性)。
    #[must_use]
    pub fn low_shelf(sample_rate: f32, frequency: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        Self::shelf(sample_rate, frequency, gain_db, false)
    }

    /// RBJ high shelf(参数同 [`BiquadCoefficients::low_shelf`])。
    #[must_use]
    pub fn high_shelf(sample_rate: f32, frequency: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.001 {
            return Self::default();
        }
        Self::shelf(sample_rate, frequency, gain_db, true)
    }

    fn shelf(sample_rate: f32, frequency: f32, gain_db: f32, high: bool) -> Self {
        let sr = f64::from(sample_rate);
        let f0 = f64::from(frequency);
        let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
        let omega = std::f64::consts::TAU * f0 / sr;
        let (sin_omega, cos_omega) = omega.sin_cos();
        // Q = 1/sqrt(2),即 shelf slope S = 1:alpha = sin_omega / sqrt(2)
        let alpha = sin_omega / std::f64::consts::SQRT_2;
        let term = 2.0 * a.sqrt() * alpha;
        let (a_plus, a_minus) = (a + 1.0, a - 1.0);

        let (b0, b1, b2, a0, a1, a2) = if high {
            (
                a * (a_plus + a_minus * cos_omega + term),
                -2.0 * a * (a_minus + a_plus * cos_omega),
                a * (a_plus + a_minus * cos_omega - term),
                a_plus - a_minus * cos_omega + term,
                2.0 * (a_minus - a_plus * cos_omega),
                a_plus - a_minus * cos_omega - term,
            )
        } else {
            (
                a * (a_plus - a_minus * cos_omega + term),
                2.0 * a * (a_minus - a_plus * cos_omega),
                a * (a_plus - a_minus * cos_omega - term),
                a_plus + a_minus * cos_omega + term,
                -2.0 * (a_minus + a_plus * cos_omega),
                a_plus + a_minus * cos_omega - term,
            )
        };
        Self {
            b0: (b0 / a0) as f32,
            b1: (b1 / a0) as f32,
            b2: (b2 / a0) as f32,
            a1: (a1 / a0) as f32,
            a2: (a2 / a0) as f32,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadState {
    pub fn process(&mut self, input: f32, coeffs: &BiquadCoefficients) -> f32 {
        let output = coeffs.b0 * input + coeffs.b1 * self.x1 + coeffs.b2 * self.x2
            - coeffs.a1 * self.y1
            - coeffs.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EqSettings {
    pub enabled: bool,
    pub gains: [f32; EQ_BAND_COUNT],
    pub preamp: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gains: [0.0; EQ_BAND_COUNT],
            preamp: 0.0,
        }
    }
}

impl EqSettings {
    /// 有效 preamp(dB) = 用户 preamp - 最大提升量。
    ///
    /// 均衡器提升某频段后,该频段的峰值可能超过 0dBFS 触发 soft clip 全曲失真。
    /// 自动把整体电平拉低最大提升量即可保证峰值不超(削减 band 只会降低电平,
    /// 不需要补偿)。与专业均衡器(Equalizer APO / VLC auto gain)行为一致。
    #[must_use]
    pub fn effective_preamp_db(&self) -> f32 {
        self.preamp - self.gains.iter().copied().fold(0.0_f32, f32::max)
    }
}

// EQ 设置单独落盘到 data/eq.json,不进 config.json:调节是高频低延迟操作,拖拽滑块时
// set_eq_* 命令会连续触发,且它独立于配置的导出/导入。为避免每次变更都同步写盘阻塞
// 命令线程,这里维护一个后台线程:变更只投递脏标记,线程合并积压后写一次最新快照。
enum PersistMsg {
    Ping,
    Stop,
}

/// 后台持久化器:持有一份 settings 句柄,收信后读取最新快照落盘。
struct EqPersister {
    tx: std::sync::mpsc::Sender<PersistMsg>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl EqPersister {
    /// 在专用线程上运行持久化循环;`settings` 与 GlobalEqualizer 共享同一份 Arc。
    fn spawn(settings: Arc<RwLock<EqSettings>>, path: String) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PersistMsg>();
        let thread = std::thread::Builder::new()
            .name("eq-persister".to_string())
            .spawn(move || {
                let mut stop = false;
                let mut dirty = false;
                while let Ok(msg) = rx.recv() {
                    match msg {
                        PersistMsg::Ping => {
                            dirty = true;
                            // 合并拖拽期间积压的脏标记;注意不能把 Stop 一并吞掉,
                            // 否则 Drop 时的 join 会永远等不到退出信号
                            loop {
                                match rx.try_recv() {
                                    Ok(PersistMsg::Ping) => dirty = true,
                                    Ok(PersistMsg::Stop) => {
                                        stop = true;
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                        }
                        PersistMsg::Stop => stop = true,
                    }
                    if dirty {
                        // 先取快照再释放读锁,避免持锁做文件 I/O
                        let snapshot = lock_or_log!(settings.read()).clone();
                        write_eq_settings_file(&path, &snapshot);
                        dirty = false;
                    }
                    if stop {
                        break;
                    }
                }
            })
            .expect("spawn eq persister thread");
        Self {
            tx,
            thread: Some(thread),
        }
    }

    fn mark_dirty(&self) {
        // 发送失败说明 eq-persister 线程已退出：之后的 EQ 改动都不会再落盘，必须留痕
        if self.tx.send(PersistMsg::Ping).is_err() {
            log::error!("eq-persister 线程已停止，EQ 设置无法落盘");
        }
    }
}

impl Drop for EqPersister {
    fn drop(&mut self) {
        // 发送 Stop 并等待线程退出(线程会先处理完已排队的 Ping 再做最后落盘)
        let _ = self.tx.send(PersistMsg::Stop);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("eq-persister 线程异常退出，最后一次 EQ 落盘可能未完成");
            }
        }
    }
}

/// 把 EQ 快照原子写入 `<data>/eq.json`(先写 .tmp 再 rename)
fn write_eq_settings_file(path: &str, settings: &EqSettings) {
    use std::io::Write;
    let Ok(content) = serde_json::to_string_pretty(settings) else {
        return;
    };
    let tmp_path = format!("{path}.tmp");
    let result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)
    })();
    if let Err(e) = result {
        log::warn!("Failed to persist equalizer settings to {path}: {e}");
        let _ = std::fs::remove_file(&tmp_path);
    }
}

/// 从 eq.json 读取 EQ 快照;文件缺失/损坏时回退默认值(不会影响启动)
fn read_eq_settings_file(path: &str) -> Option<EqSettings> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<EqSettings>(&text) {
        Ok(settings) => Some(settings),
        Err(e) => {
            log::warn!("Failed to parse equalizer settings file {path}: {e}");
            None
        }
    }
}

pub struct GlobalEqualizer {
    settings: Arc<RwLock<EqSettings>>,
    persister: Option<EqPersister>,
}

impl GlobalEqualizer {
    /// 纯内存均衡器(无落盘)。用于测试与不指定数据目录的场景。
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: Arc::new(RwLock::new(EqSettings::default())),
            persister: None,
        }
    }

    /// 创建均衡器并绑定 `<data>/eq.json` 持久化。
    ///
    /// 启动时若文件有效则恢复上次设置,之后每次变更经后台线程合并落盘。
    #[must_use]
    pub fn with_persistence(config_dir: &str) -> Self {
        let path = std::path::Path::new(config_dir)
            .join("eq.json")
            .to_string_lossy()
            .to_string();

        let initial = match read_eq_settings_file(&path) {
            Some(settings) => {
                log::info!("Loaded equalizer settings from: {path}");
                settings
            }
            None => EqSettings::default(),
        };
        let settings = Arc::new(RwLock::new(initial));
        let persister = EqPersister::spawn(Arc::clone(&settings), path);
        Self {
            settings,
            persister: Some(persister),
        }
    }

    #[must_use]
    pub fn get_settings_handle(&self) -> Arc<RwLock<EqSettings>> {
        Arc::clone(&self.settings)
    }

    #[must_use]
    pub fn get_settings(&self) -> EqSettings {
        lock_or_log!(self.settings.read()).clone()
    }

    pub fn set_settings(&self, settings: EqSettings) {
        *lock_or_log!(self.settings.write()) = settings;
        self.persist_now();
    }

    pub fn set_enabled(&self, enabled: bool) {
        lock_or_log!(self.settings.write()).enabled = enabled;
        self.persist_now();
    }

    pub fn set_gains(&self, gains: [f32; EQ_BAND_COUNT]) {
        lock_or_log!(self.settings.write()).gains = gains;
        self.persist_now();
    }

    pub fn set_band_gain(&self, band: usize, gain: f32) {
        if band < EQ_BAND_COUNT {
            lock_or_log!(self.settings.write()).gains[band] = gain.clamp(-8.0, 8.0);
            self.persist_now();
        }
    }

    pub fn set_preamp(&self, preamp: f32) {
        lock_or_log!(self.settings.write()).preamp = preamp.clamp(-8.0, 8.0);
        self.persist_now();
    }

    /// 标记一次变更:通知后台线程尽快把最新快照写盘(有合并)
    fn persist_now(&self) {
        if let Some(persister) = &self.persister {
            persister.mark_dirty();
        }
    }
}

impl Default for GlobalEqualizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_peaking_eq_zero_gain_returns_default() {
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 0.0, 1.0);
        let default = BiquadCoefficients::default();
        assert!(approx_eq(coeffs.b0, default.b0));
        assert!(approx_eq(coeffs.b1, default.b1));
        assert!(approx_eq(coeffs.b2, default.b2));
        assert!(approx_eq(coeffs.a1, default.a1));
        assert!(approx_eq(coeffs.a2, default.a2));
    }

    #[test]
    fn test_peaking_eq_near_zero_gain_returns_default() {
        // |gain_db| < 0.001 也应返回 default (恒等滤波器)
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 0.0005, 1.0);
        let default = BiquadCoefficients::default();
        assert!(approx_eq(coeffs.b0, default.b0));
    }

    #[test]
    fn test_peaking_eq_nonzero_gain_not_identity() {
        let coeffs = BiquadCoefficients::peaking_eq(48000.0, 1000.0, 6.0, 1.0);
        assert!(
            !approx_eq(coeffs.b0, 1.0),
            "b0 should differ from 1.0 with non-zero gain"
        );
        assert!(
            !approx_eq(coeffs.b1, 0.0),
            "b1 should differ from 0.0 with non-zero gain"
        );
        // peaking EQ 归一化后 a1 == b1
        assert!(approx_eq(coeffs.a1, coeffs.b1));
    }

    #[test]
    fn test_biquad_process_with_identity_coeffs() {
        let mut state = BiquadState::default();
        let coeffs = BiquadCoefficients::default();
        for &x in &[0.5_f32, -0.3, 0.8, -0.1, 0.0, 1.0] {
            let y = state.process(x, &coeffs);
            assert!(approx_eq(y, x), "identity filter output {y} != input {x}");
        }
    }

    #[test]
    fn test_biquad_reset_clears_state() {
        let mut state = BiquadState::default();
        let coeffs = BiquadCoefficients {
            b0: 0.5,
            b1: 0.5,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        };
        // 处理若干样本填充状态
        let _ = state.process(1.0, &coeffs);
        let _ = state.process(0.5, &coeffs);
        // reset 后用恒等系数处理,输出应等于输入
        state.reset();
        let identity = BiquadCoefficients::default();
        let y = state.process(0.7, &identity);
        assert!(
            approx_eq(y, 0.7),
            "after reset, identity output {y} != input 0.7"
        );
    }

    #[test]
    fn test_effective_preamp_compensates_max_boost() {
        let mut settings = EqSettings::default();
        assert!(
            approx_eq(settings.effective_preamp_db(), 0.0),
            "flat EQ needs no compensation"
        );
        settings.gains[5] = 4.0;
        settings.gains[2] = 2.0;
        settings.preamp = -1.0;
        assert!(
            approx_eq(settings.effective_preamp_db(), -5.0),
            "effective preamp should subtract the max boost (4dB), got {}",
            settings.effective_preamp_db()
        );
        // 纯削减不需要补偿
        settings.gains[5] = 0.0;
        settings.gains[2] = -3.0;
        assert!(
            approx_eq(settings.effective_preamp_db(), -1.0),
            "cut-only EQ should not be compensated"
        );
    }

    #[test]
    fn test_eq_preset_flat_is_all_zero() {
        let preset = EqPreset::flat();
        assert_eq!(preset.name, "Flat");
        assert_eq!(preset.gains.len(), EQ_BAND_COUNT);
        for &g in &preset.gains {
            assert!(g.abs() < 1e-5, "flat preset gain should be 0");
        }
    }

    #[test]
    fn test_eq_preset_gains_length() {
        let presets = [
            EqPreset::flat(),
            EqPreset::bass_boost(),
            EqPreset::treble_boost(),
            EqPreset::vocal(),
            EqPreset::rock(),
            EqPreset::pop(),
            EqPreset::jazz(),
            EqPreset::classical(),
            EqPreset::electronic(),
            EqPreset::acoustic(),
        ];
        for preset in &presets {
            assert_eq!(
                preset.gains.len(),
                EQ_BAND_COUNT,
                "preset {} gains length wrong",
                preset.name
            );
        }
    }

    #[test]
    fn test_get_all_presets_count() {
        let presets = get_all_presets();
        assert_eq!(presets.len(), 9, "get_all_presets should return 9 presets");
        for preset in &presets {
            assert_eq!(preset.gains.len(), EQ_BAND_COUNT);
        }
    }

    #[test]
    fn test_global_equalizer_defaults() {
        let eq = GlobalEqualizer::new();
        let settings = eq.get_settings();
        assert!(!settings.enabled, "default enabled should be false");
        assert!(settings.preamp.abs() < 1e-5, "default preamp should be 0");
        assert_eq!(settings.gains.len(), EQ_BAND_COUNT);
        for &g in &settings.gains {
            assert!(g.abs() < 1e-5, "default gains should be 0");
        }
    }

    #[test]
    fn test_global_equalizer_set_band_gain_clamps_high() {
        let eq = GlobalEqualizer::new();
        eq.set_band_gain(0, 100.0);
        assert!(
            approx_eq(eq.get_settings().gains[0], 8.0),
            "gain over +8 should clamp to 8"
        );
    }

    #[test]
    fn test_global_equalizer_set_band_gain_clamps_low() {
        let eq = GlobalEqualizer::new();
        eq.set_band_gain(0, -100.0);
        assert!(
            approx_eq(eq.get_settings().gains[0], -8.0),
            "gain below -8 should clamp to -8"
        );
    }

    #[test]
    fn test_global_equalizer_set_band_gain_out_of_range_ignored() {
        let eq = GlobalEqualizer::new();
        eq.set_band_gain(EQ_BAND_COUNT + 5, 5.0);
        for &g in &eq.get_settings().gains {
            assert!(g.abs() < 1e-5, "out-of-range band gain should be ignored");
        }
    }

    #[test]
    fn test_global_equalizer_preamp_clamps() {
        let eq = GlobalEqualizer::new();
        eq.set_preamp(100.0);
        assert!(approx_eq(eq.get_settings().preamp, 8.0));
        eq.set_preamp(-100.0);
        assert!(approx_eq(eq.get_settings().preamp, -8.0));
    }

    #[test]
    fn test_global_equalizer_set_gains_and_enabled_roundtrip() {
        let eq = GlobalEqualizer::new();
        eq.set_enabled(true);
        eq.set_gains([1.0; EQ_BAND_COUNT]);
        let settings = eq.get_settings();
        assert!(settings.enabled);
        assert!(settings.gains.iter().all(|&g| approx_eq(g, 1.0)));
        eq.set_settings(EqSettings::default());
        assert!(!eq.get_settings().enabled);
    }

    #[test]
    fn test_eq_persistence_roundtrip() {
        // 验证 with_persistence:写入 eq.json 后重建实例能恢复设置
        let dir = std::env::temp_dir().join(format!("eq-persist-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let dir_str = dir.to_str().expect("temp path utf-8");
        let path = dir.join("eq.json");

        let mut gains = [0.0f32; EQ_BAND_COUNT];
        gains[2] = 3.5;
        gains[9] = -2.0;
        {
            let eq = GlobalEqualizer::with_persistence(dir_str);
            eq.set_enabled(true);
            eq.set_gains(gains);
            eq.set_preamp(1.5);
            // 离开作用域时 EqPersister Drop -> Stop + join,队列中最后一次 Ping 已写盘
        }

        let eq = GlobalEqualizer::with_persistence(dir_str);
        let settings = eq.get_settings();
        assert!(settings.enabled, "enabled should be restored");
        assert!(approx_eq(settings.gains[2], 3.5));
        assert!(approx_eq(settings.gains[9], -2.0));
        assert!(approx_eq(settings.preamp, 1.5));
        drop(eq);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(dir.join("eq.json.tmp"));
        let _ = std::fs::remove_dir(&dir);
    }
}
