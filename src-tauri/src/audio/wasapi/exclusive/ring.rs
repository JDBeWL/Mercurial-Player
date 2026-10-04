//! WASAPI 独占模式的无锁 SPSC 采样环形缓冲与欠载计数：线程契约与不用锁的理由见 [`SpscSampleRing`]。
use std::time::Duration;

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::audio::sample_ring::{bump_write_epoch, current_write_epoch};

/// SPSC 环形缓冲容量(采样数)
///
/// 按最坏情况一次性预分配，覆盖立体声 <=384kHz、6声道 <=192kHz 等所有现实的独占模式格式。
/// 生产者的 2 秒水位门控（见 decode_push）远小于它，故无需设备初始化后再跨线程重设容量。
pub(super) const SPSC_RING_CAPACITY: usize = 384_000 * 2 * 4;

/// 无锁 SPSC(单生产者/单消费者)采样环形缓冲：渲染回调与解码线程不争互斥锁
/// （Windows 上 `std::sync::Mutex` 争用会陷入内核，渲染线程被抢占即产生 xrun/爆音）。
///
/// 线程契约:push/push_slice_checked 仅生产者；pop_slice 仅消费者；clear/len 任意线程；
/// invalidate 仅命令线程（递增全局世代，不在实时线程做）。
/// 内存序:head/tail 各自 Release store 配 Acquire load；计数器单调递增，以 `usize` 计不会回绕。
pub(super) struct SpscSampleRing {
    /// 内部可变性:生产者/消费者访问不相交区间（SAFETY 说明见各方法）
    buf: UnsafeCell<Box<[f32]>>,
    capacity: usize,
    /// 单调递增写入计数(生产者独占写)
    head: AtomicUsize,
    /// 单调递增读取计数(消费者独占写)
    tail: AtomicUsize,
    /// 当前写入世代，供 [`Self::push_slice_checked`] 判定生产者是否已被作废
    epoch: AtomicU64,
}

// SAFETY: SPSC 契约下生产者只写 [head, head+free) 区间、消费者只读
// [tail, head) 区间,两者不相交且各自唯一;跨线程共享安全。
#[allow(unsafe_code)] // 无锁 SPSC 需要受控的裸指针访问,SAFETY 说明见各处
unsafe impl Sync for SpscSampleRing {}

#[allow(unsafe_code)]
impl SpscSampleRing {
    #[must_use]
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            buf: UnsafeCell::new(vec![0.0; capacity].into_boxed_slice()),
            capacity,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            epoch: AtomicU64::new(current_write_epoch()),
        }
    }

    /// 生产者:按"环的当前世代"写入，返回实际写入数(缓冲满时截断)。仅供测试；
    /// 生产路径一律走 [`Self::push_slice_checked`] 显式带上世代，避免绕过作废校验。
    #[cfg(test)]
    pub(super) fn push_slice(&self, samples: &[f32]) -> usize {
        self.push_slice_checked(samples, self.write_epoch())
            .unwrap_or(0)
    }

    /// 生产者:带写入世代校验的写入（挡住切歌 / 停止时已被作废的生产者）；
    /// 提交前复验世代，过期则不推进 `head`，这批采样对消费者永不可见。
    ///
    /// - `Some(n)`：写入 n 个（n 可为 0，表示缓冲已满，可等待后重试）
    /// - `None`：世代已被 [`Self::invalidate`] 作废，必须停止写入
    pub(super) fn push_slice_checked(&self, samples: &[f32], epoch: u64) -> Option<usize> {
        if self.epoch.load(Ordering::Acquire) != epoch {
            return None;
        }
        let head = self.head.load(Ordering::Relaxed);
        let n = self.write_region(head, samples);
        if n == 0 {
            return Some(0);
        }
        if self.epoch.load(Ordering::Acquire) != epoch {
            return None;
        }
        self.head.store(head + n, Ordering::Release);
        Some(n)
    }

    /// 把 `samples` 按剩余容量截断后写进 `[head, head+n)`（不推进 `head`，发布由调用方负责）
    fn write_region(&self, head: usize, samples: &[f32]) -> usize {
        let tail = self.tail.load(Ordering::Acquire);
        let used = head - tail;
        let free = self.capacity - used;
        let n = free.min(samples.len());
        if n == 0 {
            return 0;
        }
        let start = head % self.capacity;
        let first = (self.capacity - start).min(n);
        // SAFETY: SPSC 契约下本方法是 [head, head+n) 区间的唯一写入者，生产者线程唯一；
        // 该区间尚未经 head 的 Release store 发布，消费者不会读取。
        let buf = unsafe { &mut *self.buf.get() };
        buf[start..start + first].copy_from_slice(&samples[..first]);
        if n > first {
            buf[..n - first].copy_from_slice(&samples[first..n]);
        }
        n
    }

    /// 本环当前的写入世代。生产者开始推送前只取一次：每轮重取会拿到"永远新鲜"的世代，等于没有校验。
    #[must_use]
    pub(super) fn write_epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// 作废旧世代生产者并清空缓冲（切歌 / 停止，线程契约见 [`SpscSampleRing`]）。
    /// 先递增世代再清空：反过来的话，被作废的生产者正好能在清空后把上一首的样本写进来。
    pub(super) fn invalidate(&self) {
        self.epoch.store(bump_write_epoch(), Ordering::SeqCst);
        self.clear();
    }

    /// 消费者:读出采样到 out，不足部分填 0(欠载)；返回实际读出数，欠载数 = `out.len() - 返回值`。
    pub(super) fn pop_slice(&self, out: &mut [f32]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed); // 消费者独占,无需同步
        let n = (head - tail).min(out.len());
        if n > 0 {
            let start = tail % self.capacity;
            let first = (self.capacity - start).min(n);
            // SAFETY: head 已通过 Acquire load 观察到，[tail, tail+n) 的写入均已发布；
            // free 计算排除了该区间，消费者推进 tail 前生产者不会复写，且消费者线程唯一。
            let buf = unsafe { &*self.buf.get() };
            out[..first].copy_from_slice(&buf[start..start + first]);
            if n > first {
                out[first..n].copy_from_slice(&buf[..n - first]);
            }
            self.tail.store(tail + n, Ordering::Release);
        }
        if n < out.len() {
            out[n..].fill(0.0);
        }
        n
    }

    /// 当前缓冲采样数(近似值,供水位检查)
    #[must_use]
    pub(super) fn len(&self) -> usize {
        // 必须先 tail 后 head：head 只增、tail 只增，读到 tail 之后读到的 head
        // 必然 >= 它。反过来的话，消费者/第三方线程可能拿到偏小的旧 head 而回绕
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        head.saturating_sub(tail)
    }

    /// 清空缓冲(tail 快进到 head)
    pub(super) fn clear(&self) {
        let head = self.head.load(Ordering::Acquire);
        self.tail.store(head, Ordering::Release);
    }
}

/// 欠载统计与节流上报
///
/// 实时线程高频打日志本身会加剧延迟恶化，故只在欠载占比过半且距上次上报 >=5s 时输出累计值。
pub(super) struct UnderrunLogger {
    last_log: std::time::Instant,
    total: u64,
}

impl UnderrunLogger {
    pub(super) fn new() -> Self {
        Self {
            last_log: std::time::Instant::now(),
            total: 0,
        }
    }

    pub(super) fn record(&mut self, underrun: usize, samples_needed: usize) {
        self.total = self.total.saturating_add(underrun as u64);
        if underrun > samples_needed / 2 && self.last_log.elapsed() >= Duration::from_secs(5) {
            log::warn!(
                "WASAPI buffer underrun: {underrun}/{samples_needed} samples (累计 {})",
                self.total
            );
            self.last_log = std::time::Instant::now();
        }
    }
}

#[cfg(test)]
mod spsc_ring_tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    /// 环形缓冲对样本是纯拷贝，相等是设计要求：按 f32 位模式比较，既避开 clippy::float_cmp 也不混淆 0.0/-0.0。
    fn assert_f32_slice_eq(actual: &[f32], expected: &[f32]) {
        assert_eq!(actual.len(), expected.len(), "长度不一致");
        for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(a.to_bits(), e.to_bits(), "index {i}: {a} != {e}");
        }
    }

    #[test]
    fn basic_push_pop() {
        let ring = SpscSampleRing::new(8);
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.push_slice(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(ring.len(), 3);

        let mut out = [0.0f32; 2];
        assert_eq!(ring.pop_slice(&mut out), 2);
        assert_f32_slice_eq(&out, &[1.0, 2.0]);

        // 欠载:只取到 1 个,其余填 0
        let mut out2 = [0.0f32; 5];
        assert_eq!(ring.pop_slice(&mut out2), 1);
        assert_f32_slice_eq(&out2, &[3.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(ring.len(), 0);
    }

    #[test]
    fn wraps_around() {
        let ring = SpscSampleRing::new(4);
        let mut out = [0.0f32; 3];
        ring.push_slice(&[1.0, 2.0, 3.0]);
        assert_eq!(ring.pop_slice(&mut out), 3);
        assert_f32_slice_eq(&out, &[1.0, 2.0, 3.0]);

        // head=3, 写入跨越缓冲区末尾
        assert_eq!(ring.push_slice(&[4.0, 5.0, 6.0]), 3);
        let mut out2 = [0.0f32; 3];
        assert_eq!(ring.pop_slice(&mut out2), 3);
        assert_f32_slice_eq(&out2, &[4.0, 5.0, 6.0]);
    }

    #[test]
    fn full_truncates() {
        let ring = SpscSampleRing::new(4);
        assert_eq!(ring.push_slice(&[1.0, 2.0, 3.0, 4.0, 5.0]), 4);
        assert_eq!(ring.len(), 4);
        assert_eq!(ring.push_slice(&[6.0]), 0);
    }

    #[test]
    fn clear_resets() {
        let ring = SpscSampleRing::new(8);
        ring.push_slice(&[1.0, 2.0, 3.0]);
        ring.clear();
        assert_eq!(ring.len(), 0);

        // clear 后可继续正常读写
        assert_eq!(ring.push_slice(&[7.0]), 1);
        let mut out = [0.0f32; 1];
        assert_eq!(ring.pop_slice(&mut out), 1);
        assert_f32_slice_eq(&out, &[7.0]);
    }

    /// 被作废的生产者不得再写入；用与 `sample_ring` 用例共享的锁串行化（全局世代是进程级的）。
    #[test]
    fn stale_epoch_rejected() {
        let _guard = crate::audio::sample_ring::EPOCH_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let ring = SpscSampleRing::new(8);
        let epoch = ring.write_epoch();
        assert_eq!(ring.push_slice_checked(&[1.0, 2.0], epoch), Some(2));

        ring.invalidate();
        assert_eq!(ring.len(), 0, "invalidate 必须丢弃未读数据");
        assert_eq!(
            ring.push_slice_checked(&[9.0], epoch),
            None,
            "过期世代必须被拒绝"
        );
        let mut out = [0.0f32; 2];
        assert_eq!(ring.pop_slice(&mut out), 0, "过期写入不得对消费者可见");

        // 新解码线程取到的是新世代，可以正常写
        let fresh = ring.write_epoch();
        assert_ne!(fresh, epoch);
        assert_eq!(ring.push_slice_checked(&[5.0], fresh), Some(1));
        let mut out2 = [0.0f32; 1];
        assert_eq!(ring.pop_slice(&mut out2), 1);
        assert_f32_slice_eq(&out2, &[5.0]);
    }

    /// 缓冲满时返回 `Some(0)`（可等待重试），而非当作过期
    #[test]
    fn full_reported_as_zero_not_stale() {
        let ring = SpscSampleRing::new(4);
        let epoch = ring.write_epoch();
        assert_eq!(
            ring.push_slice_checked(&[1.0, 2.0, 3.0, 4.0], epoch),
            Some(4)
        );
        assert_eq!(ring.push_slice_checked(&[5.0], epoch), Some(0));
    }

    /// 双线程压测:验证 SPSC 契约下数据不丢失、不乱序
    #[test]
    fn multithreaded_spsc() {
        const CHUNK: usize = 16;
        const CHUNKS: u32 = 20_000;
        let ring = Arc::new(SpscSampleRing::new(1024));

        let producer = {
            let ring = Arc::clone(&ring);
            thread::spawn(move || {
                for i in 0..CHUNKS {
                    let chunk = [(i % 100) as f32; CHUNK];
                    let mut written = 0;
                    while written < CHUNK {
                        written += ring.push_slice(&chunk[written..]);
                        std::hint::spin_loop();
                    }
                }
            })
        };
        let consumer = {
            let ring = Arc::clone(&ring);
            thread::spawn(move || {
                let mut out = [0.0f32; CHUNK];
                for i in 0..CHUNKS {
                    let mut read = 0;
                    while read < CHUNK {
                        read += ring.pop_slice(&mut out[read..]);
                        std::hint::spin_loop();
                    }
                    for &v in &out {
                        assert_eq!(v.to_bits(), ((i % 100) as f32).to_bits());
                    }
                }
            })
        };

        producer.join().expect("producer panicked");
        consumer.join().expect("consumer panicked");
        assert_eq!(ring.len(), 0);
    }
}
