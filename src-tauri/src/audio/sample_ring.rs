//! 音频线程 → 分析线程的无锁采样通道。
//!
//! 频谱的 FFT 与事件发送不能在音频回调线程上做,采样经此交出,
//! 音频线程侧只做一次原子写入,无锁、无分配、无阻塞。

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// 单生产者单消费者无锁环形缓冲(容量向上取整到 2 的幂)。
/// `written` 只由生产者写、`consumed` 只由消费者写,无需额外同步。
pub struct SampleRing {
    /// 采样以 f32 位模式存放,避免为 f32 引入额外的同步包装
    slots: Box<[AtomicU32]>,
    /// 生产者已写入的采样总数
    written: AtomicUsize,
    /// 消费者已取走的采样总数
    consumed: AtomicUsize,
    /// 容量掩码(容量恒为 2 的幂)
    mask: usize,
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
        }
    }

    /// 容量(采样数)
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.mask + 1
    }

    /// 生产者:写入一个采样。缓冲满时丢弃该采样并返回 `false`。
    ///
    /// 不等待、不覆盖未读数据
    /// 音频线程必须能在确定时间内返回
    /// 偶发丢采样远好于让音频回调阻塞
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
        self.consumed.store(c.wrapping_add(n), Ordering::Release);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::SampleRing;

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
}
