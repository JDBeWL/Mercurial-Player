//! 共享模式输出：当前曲目与过渡中的旧曲目各持一个 rodio `Player`，共用同一个 mixer。
//!
//! rodio 的单个 `Player` 内部是串行队列，无法让两条音源同时出声；智能过渡需要新旧曲目
//! 重叠播放，因此在共享模式下多建一个 `Player` 直接接到同一个 `Mixer`。所有播放控制
//! （播放/暂停/停止/音量）都必须同时作用于两条链路。

use rodio::mixer::Mixer;
use rodio::{Player, Source};
use std::mem;

/// 共享输出：`current` 是 UI 与位置事件对应的曲目，`outgoing` 只在交叉淡化期间存在
pub struct SharedOutput {
    mixer: Mixer,
    sample_rate: u32,
    channels: u16,
    current: Player,
    outgoing: Option<Player>,
}

impl SharedOutput {
    #[must_use]
    pub fn new(current: Player, mixer: Mixer, sample_rate: u32, channels: u16) -> Self {
        Self {
            mixer,
            sample_rate,
            channels,
            current,
            outgoing: None,
        }
    }

    #[must_use]
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }

    /// 把音源追加到当前曲目队列（与 `Player::append` 语义一致）
    pub fn append<S>(&self, source: S)
    where
        S: Source<Item = f32> + Send + 'static,
    {
        self.current.append(source);
    }

    /// 开始交叉淡化：新曲目换成一个独立的 `Player` 并立即出声，旧曲目转入 `outgoing`。
    pub fn begin_transition<S>(&mut self, source: S)
    where
        S: Source<Item = f32> + Send + 'static,
    {
        let volume = self.current.volume();
        let paused = self.current.is_paused();

        let incoming = Player::connect_new(&self.mixer);
        incoming.set_volume(volume);
        // 先落暂停位再挂音源：暂停态下新曲不应在交叉开始前漏出一小段声音
        if paused {
            incoming.pause();
        }
        incoming.append(source);
        if !paused {
            incoming.play();
        }

        self.outgoing = Some(mem::replace(&mut self.current, incoming));
    }

    /// 交叉淡化完成：旧曲目的增益已经到 0，直接丢弃即可（`Player` 的 Drop 会停止它）
    pub fn finish_transition(&mut self) {
        self.outgoing = None;
    }

    /// 取消/清理过渡：停止并丢弃旧曲目
    pub fn cancel_transition(&mut self) {
        if let Some(outgoing) = self.outgoing.take() {
            outgoing.stop();
        }
    }

    #[must_use]
    pub fn is_transitioning(&self) -> bool {
        self.outgoing.is_some()
    }

    pub fn play(&self) {
        self.current.play();
        if let Some(outgoing) = &self.outgoing {
            outgoing.play();
        }
    }

    pub fn pause(&self) {
        self.current.pause();
        if let Some(outgoing) = &self.outgoing {
            outgoing.pause();
        }
    }

    pub fn stop(&self) {
        self.current.stop();
        if let Some(outgoing) = &self.outgoing {
            outgoing.stop();
        }
    }

    pub fn clear(&self) {
        self.current.clear();
        if let Some(outgoing) = &self.outgoing {
            outgoing.clear();
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.current.set_volume(volume);
        if let Some(outgoing) = &self.outgoing {
            outgoing.set_volume(volume);
        }
    }

    #[must_use]
    pub fn volume(&self) -> f32 {
        self.current.volume()
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.current.is_paused()
    }

    #[must_use]
    pub fn empty(&self) -> bool {
        self.current.empty()
    }
}
