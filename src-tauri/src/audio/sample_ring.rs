//! 无锁 SPSC 采样环形缓冲，供频谱分析与 AAudio 独占输出使用。
//!
//! 音频线程侧只做一次原子写入，无锁、无分配、无阻塞。
//! 解码线程可能在背压处等很久，缓冲腾出空间后会灌进上一首样本，故环上带写入世代。

use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// 全局写入世代。新环继承当前值，因此设备切换重建环之后原有解码线程仍可继续写入；
/// 只有 [`SampleRing::invalidate`]（停止 / 切歌 / 流断开）会递增它，让所有持有旧世代的
/// 生产者（含正卡在背压等待里的）同时失效。
static NEXT_WRITE_EPOCH: AtomicU64 = AtomicU64::new(1);

/// 申请一个新的写入世代：作废当前所有持有旧世代的生产者
pub(crate) fn bump_write_epoch() -> u64 {
    NEXT_WRITE_EPOCH.fetch_add(1, Ordering::SeqCst) + 1
}

/// 当前写入世代（新建的环继承它）
pub(crate) fn current_write_epoch() -> u64 {
    NEXT_WRITE_EPOCH.load(Ordering::SeqCst)
}

/// 测试用串行锁：全局世代是进程级的，任何"调用 invalidate"或"断言新环继承世代"的用例
/// （本模块与 WASAPI 侧的 ring 用例）都必须先拿这把锁，否则并行跑会互相干扰。
#[cfg(test)]
pub(crate) static EPOCH_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 单生产者单消费者无锁环形缓冲(容量向上取整到 2 的幂)。
///
/// `written` 只由生产者写;`consumed` 允许双方前进但永不后退(见 `fetch_max` 注释)。
pub struct SampleRing {
    /// 采样以 f32 位模式存放,避免为 f32 引入额外的同步包装
    slots: Box<[AtomicU32]>,
    /// 生产者已写入的采样总数
    written: AtomicUsize,
    /// 消费者已取走的采样总数
    consumed: AtomicUsize,
    /// 容量掩码(容量恒为 2 的幂)
    mask: usize,
    /// 当前写入世代，供 [`Self::push_slice_checked`] 判定生产者是否已被作废
    epoch: AtomicU64,
}

impl SampleRing {
    #[must_use]
    pub fn new(min_capacity: usize) -> Self {
        let capacity = min_capacity.max(2).next_power_of_two();
        let mut slots = Vec::with_capacity(capacity);
        slots.resize_with(capacity, || AtomicU32::new(0));
        Self {
            slots: slots.into_boxed_slice(),
            written: AtomicUsize::new(0),
            consumed: AtomicUsize::new(0),
            mask: capacity - 1,
            epoch: AtomicU64::new(current_write_epoch()),
        }
    }

    /// 容量(采样数)
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// 生产者:写入一个采样。缓冲满时丢弃该采样并返回 `false`。
    ///
    /// 不等待、不覆盖未读数据：音频回调必须在确定时间内返回，偶发丢采样远好于阻塞。
    #[inline]
    pub fn push(&self, sample: f32) -> bool {
        let w = self.written.load(Ordering::Relaxed);
        let c = self.consumed.load(Ordering::Acquire);
        if w.wrapping_sub(c) >= self.slots.len() {
            return false;
        }
        self.slots[w & self.mask].store(sample.to_bits(), Ordering::Relaxed);
        self.written.store(w.wrapping_add(1), Ordering::Release);
        true
    }

    /// 生产者:批量写入,返回实际写入的采样数，缓冲满时截断且不覆盖未读数据。
    ///
    /// 按"环的当前世代"写入，仅供生命周期与环绑定的生产者（测试）使用；
    /// 会被切歌/停止作废的 AAudio 解码线程一律走 [`Self::push_slice_checked`]。
    #[cfg(test)]
    pub fn push_slice(&self, samples: &[f32]) -> usize {
        self.push_slice_checked(samples, self.write_epoch())
            .unwrap_or(0)
    }

    /// 生产者:带写入世代校验的批量写入（挡住切歌 / 停止时已被作废的生产者）。
    ///
    /// - `Some(n)`：写入 n 个（n 可为 0，表示环已满，可等待后重试）
    /// - `None`：世代已被 [`Self::invalidate`] 作废，必须停止写入
    ///
    /// 提交前复验世代：过期则不推进 `written`，这批采样对消费者永不可见。
    pub fn push_slice_checked(&self, samples: &[f32], epoch: u64) -> Option<usize> {
        if samples.is_empty() {
            return Some(0);
        }
        if self.epoch.load(Ordering::Acquire) != epoch {
            return None;
        }
        let w = self.written.load(Ordering::Relaxed);
        let n = self.write_slots(w, samples);
        if n == 0 {
            return Some(0);
        }
        if self.epoch.load(Ordering::Acquire) != epoch {
            return None;
        }
        self.written.store(w.wrapping_add(n), Ordering::Release);
        Some(n)
    }

    /// 把 `samples` 按剩余容量截断后写进槽位（不推进 `written`，发布由调用方负责）
    fn write_slots(&self, w: usize, samples: &[f32]) -> usize {
        let c = self.consumed.load(Ordering::Acquire);
        let free = self.slots.len() - w.wrapping_sub(c);
        let n = free.min(samples.len());
        if n == 0 {
            return 0;
        }
        for (i, sample) in samples[..n].iter().enumerate() {
            self.slots[w.wrapping_add(i) & self.mask].store(sample.to_bits(), Ordering::Relaxed);
        }
        n
    }

    /// 丢弃尚未被取走的全部采样(切歌 / seek 用)
    pub fn clear(&self) {
        // fetch_max 而非 store：并发 drain 时不会把 consumed 退回旧值，否则清空后会重播切歌前的样本
        let w = self.written.load(Ordering::Acquire);
        self.consumed.fetch_max(w, Ordering::Release);
    }

    /// 本环当前的写入世代。生产者在开始推送前只取一次，之后每次写入都带上它。
    #[must_use]
    pub fn write_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// 作废所有持有旧世代的生产者，并丢弃未读数据（停止 / 切歌 / 流断开时调用）。
    ///
    /// 先递增世代再清空：反过来的话被作废的生产者正好能在清空后灌进上一首的样本，
    /// 而这正是解码线程从背压等待中被唤醒时最容易走到的路径。
    pub fn invalidate(&self) {
        self.epoch.store(bump_write_epoch(), Ordering::SeqCst);
        self.clear();
    }

    /// 尚未被取走的采样数
    #[must_use]
    pub fn len(&self) -> usize {
        let w = self.written.load(Ordering::Acquire);
        let c = self.consumed.load(Ordering::Acquire);
        w.wrapping_sub(c)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 消费者:取走至多 `max_n` 个采样追加到 `out`,返回实际取到的数量。
    pub fn drain_into(&self, out: &mut Vec<f32>, max_n: usize) -> usize {
        if max_n == 0 {
            return 0;
        }
        let c = self.consumed.load(Ordering::Relaxed);
        let w = self.written.load(Ordering::Acquire);
        let n = w.wrapping_sub(c).min(max_n);
        if n == 0 {
            return 0;
        }
        // 先扩容再按迭代器写入,避免逐样本 push 反复走容量检查
        let start = out.len();
        out.resize(start + n, 0.0);
        for (i, dst) in out[start..].iter_mut().enumerate() {
            let slot = c.wrapping_add(i) & self.mask;
            *dst = f32::from_bits(self.slots[slot].load(Ordering::Relaxed));
        }
        self.consumed
            .fetch_max(c.wrapping_add(n), Ordering::Release);
        n
    }
}

/// 独占模式环形缓冲的目标容量（采样数）：取 `max(目标秒数, 水位门控 + 余量)`。
///
/// 后者是硬约束：[`crate::audio::EXCLUSIVE_BUFFER_WATERMARK_SECS`] 只有在环没满时才拦得住生产者，
/// 环比门控还小则门控形同虚设，`push_samples` 的背压等待会变成主节奏点。
/// 调用方还会把结果向上取整到 2 的幂（写指针用掩码取模），口径由测试守住。
#[must_use]
pub fn exclusive_ring_capacity(sample_rate: u32, channels: u16, seconds: f32) -> usize {
    // 门控之上的余量（秒）：覆盖一次回调块与解码抖动，保证"门控先生效"而不是"环先满"
    const MARGIN_SECS: f32 = 0.5;

    let per_sec = sample_rate as usize * channels.max(1) as usize;
    // 秒数先化成整数毫秒再做整数乘除：容量要拿去分配内存，f32 直接参与会在 192kHz*8ch 这类大值上带舍入误差
    let samples_for_secs = |secs: f32| -> usize {
        let ms = (f64::from(secs) * 1000.0).round().max(0.0) as u64;
        (per_sec as u64 * ms / 1000) as usize
    };
    let watermark_secs = crate::audio::EXCLUSIVE_BUFFER_WATERMARK_SECS as f32;
    samples_for_secs(seconds)
        .max(samples_for_secs(watermark_secs + MARGIN_SECS))
        .max(2)
}

#[cfg(test)]
mod tests {
    use super::{EPOCH_TEST_LOCK, SampleRing, exclusive_ring_capacity};

    #[test]
    fn test_drain_empty_ring() {
        let ring = SampleRing::new(8);
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 4), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn test_push_then_drain_in_order() {
        let ring = SampleRing::new(8);
        for i in 0..5 {
            assert!(ring.push(i as f32));
        }
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 8), 5);
        assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_full_ring_drops_new_sample_without_blocking() {
        let ring = SampleRing::new(4);
        assert_eq!(ring.capacity(), 4);
        for i in 0..4 {
            assert!(ring.push(i as f32), "前 4 个应写入成功");
        }
        // 第 5 个超出容量:应被丢弃,而不是覆盖尚未读取的数据
        assert!(!ring.push(99.0));
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 4), 4);
        assert_eq!(out, vec![0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_wrap_around_keeps_data_intact() {
        let ring = SampleRing::new(4);
        // 写满 -> 取空 -> 再写,强制写指针回绕
        for round in 0..3 {
            for i in 0..4 {
                assert!(ring.push((round * 10 + i) as f32));
            }
            let mut out = Vec::new();
            assert_eq!(ring.drain_into(&mut out, 4), 4);
            let expect: Vec<f32> = (0..4).map(|i| (round * 10 + i) as f32).collect();
            assert_eq!(out, expect);
        }
    }

    #[test]
    fn test_partial_drain_neither_loses_nor_duplicates() {
        let ring = SampleRing::new(16);
        for i in 0..10 {
            assert!(ring.push(i as f32));
        }
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 3), 3);
        assert_eq!(out, vec![0.0, 1.0, 2.0]);
        out.clear();
        assert_eq!(ring.drain_into(&mut out, 100), 7);
        assert_eq!(out, vec![3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
    }

    #[test]
    fn test_capacity_rounds_up_to_power_of_two() {
        // 非 2 的幂容量向上取整(掩码取模依赖这一点)
        let ring = SampleRing::new(5);
        assert_eq!(ring.capacity(), 8);
        for i in 0..8 {
            assert!(ring.push(i as f32), "第 {i} 个应写入成功");
        }
        assert!(!ring.push(8.0), "超出容量应丢弃");
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 8), 8);
        assert_eq!(out, (0..8).map(|i| i as f32).collect::<Vec<f32>>());
    }

    #[test]
    fn test_drain_with_zero_max_is_noop() {
        let ring = SampleRing::new(4);
        assert!(ring.push(1.0));
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 0), 0);
        assert!(out.is_empty());
        // 未被取走的数据仍可完整读出
        assert_eq!(ring.drain_into(&mut out, 4), 1);
        assert_eq!(out, vec![1.0]);
    }

    /// 批量写入:写满时截断到剩余容量,不覆盖尚未被读走的数据
    #[test]
    fn test_push_slice_writes_sequentially_and_stops_when_full() {
        let ring = SampleRing::new(4);
        assert_eq!(ring.push_slice(&[1.0, 2.0, 3.0]), 3);
        // 只剩 1 个空位:批量写入应被截断到 1 个,而不是覆盖已有数据
        assert_eq!(ring.push_slice(&[4.0, 5.0]), 1);

        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 4), 4);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_len_reflects_pending_samples() {
        let ring = SampleRing::new(8);
        assert!(ring.is_empty());
        assert_eq!(ring.push_slice(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(ring.len(), 3);

        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 2), 2);
        assert_eq!(ring.len(), 1);
    }

    #[test]
    fn test_clear_drops_pending_samples() {
        let ring = SampleRing::new(8);
        assert_eq!(ring.push_slice(&[1.0, 2.0, 3.0]), 3);
        ring.clear();
        assert!(ring.is_empty());

        // 清空后写入的数据仍能被正常读出(指针没有被打乱)
        assert_eq!(ring.push_slice(&[9.0]), 1);
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 8), 1);
        assert_eq!(out, vec![9.0]);
    }

    /// 并发生产/消费:采样必须严格保序,不丢不重(Acquire/Release 配对出错时最易暴露)
    #[test]
    fn test_concurrent_producer_consumer_preserves_order() {
        use std::sync::Arc;

        const TOTAL: usize = 50_000;
        // 小容量(1 次只能装 64 个)迫使写指针频繁回绕
        let ring = Arc::new(SampleRing::new(64));

        let producer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                for i in 0..TOTAL {
                    // 满则让出 CPU 重试 —— 消费方(分析线程)之外的使用方式
                    while !ring.push(i as f32) {
                        std::thread::yield_now();
                    }
                }
            })
        };

        let mut received: Vec<f32> = Vec::with_capacity(TOTAL);
        let mut out: Vec<f32> = Vec::with_capacity(256);
        while received.len() < TOTAL {
            out.clear();
            if ring.drain_into(&mut out, 256) == 0 {
                std::thread::yield_now();
                continue;
            }
            received.extend_from_slice(&out);
        }
        producer.join().unwrap();

        assert_eq!(received, (0..TOTAL).map(|i| i as f32).collect::<Vec<f32>>());
    }

    /// 被作废的生产者（含从背压等待中醒来的那个）不得再写入——切歌串音正是从这里来
    #[test]
    fn test_stale_producer_cannot_write_after_invalidate() {
        let _guard = EPOCH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let ring = SampleRing::new(8);
        let epoch = ring.write_epoch();
        assert_eq!(ring.push_slice_checked(&[1.0, 2.0, 3.0], epoch), Some(3));

        // 切歌：作废生产者并清空
        ring.invalidate();
        assert!(ring.is_empty(), "invalidate 必须丢弃未读数据");
        assert_eq!(
            ring.push_slice_checked(&[9.0, 9.0], epoch),
            None,
            "过期世代必须被拒绝"
        );
        let mut out = Vec::new();
        assert_eq!(ring.drain_into(&mut out, 8), 0, "过期写入不得对消费者可见");

        // 新解码线程取到的是新世代，可以正常写
        let fresh = ring.write_epoch();
        assert_ne!(fresh, epoch);
        assert_eq!(ring.push_slice_checked(&[4.0], fresh), Some(1));
        assert_eq!(ring.drain_into(&mut out, 8), 1);
        assert_eq!(out, vec![4.0]);
    }

    /// 设备切换会重建环；新环继承当前世代，原解码线程因此不会被误判为过期
    #[test]
    fn test_ring_rebuild_keeps_existing_producer_valid() {
        let _guard = EPOCH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let old = SampleRing::new(8);
        let epoch = old.write_epoch();
        let rebuilt = SampleRing::new(8);
        assert_eq!(
            rebuilt.push_slice_checked(&[1.0, 2.0], epoch),
            Some(2),
            "重建后的环应继承同一世代，否则解码线程会在设备切换后静默停摆"
        );
    }

    /// 环满时 `push_slice_checked` 返回 `Some(0)`（可等待重试），而非当作过期
    #[test]
    fn test_checked_push_reports_full_as_zero_not_stale() {
        let ring = SampleRing::new(4);
        let epoch = ring.write_epoch();
        assert_eq!(
            ring.push_slice_checked(&[1.0, 2.0, 3.0, 4.0], epoch),
            Some(4)
        );
        assert_eq!(ring.push_slice_checked(&[5.0], epoch), Some(0));
    }

    /// 环容量必须严格大于水位门控，否则门控形同虚设（历史上正是这样埋出暂停卡死与切歌串音）
    #[test]
    fn test_exclusive_ring_capacity_exceeds_watermark() {
        let watermark = crate::audio::EXCLUSIVE_BUFFER_WATERMARK_SECS;
        for (sr, ch) in [
            (44_100u32, 2u16),
            (48_000, 2),
            (96_000, 2),
            (192_000, 2),
            (48_000, 1),
            (32_000, 2),
            (8_000, 1),
            (384_000, 8),
        ] {
            let ring = SampleRing::new(exclusive_ring_capacity(sr, ch, 1.5));
            let secs = ring.capacity() as f32 / (sr as f32 * ch as f32);
            assert!(
                secs > watermark as f32,
                "{sr}Hz/{ch}ch 实际容量 {secs:.2}s 未严格大于门控 {watermark}s"
            );
        }
    }
}
