//! SPSC 环形帧缓冲
//!
//! 音频回调线程（单读者）与解码线程（单写者）之间唯一的数据通道。
//!
//! - `head` 只由读者推进，`tail` 只由写者推进，均为单调原子计数，全程无锁
//! - `generation` 换代计数用于 seek：写者换代前先 +1，**永不回退 tail**；
//!   读者发现换代后自行把 head 跳到当前写点，丢弃旧代残留。
//!   单写者的"写前换代" + 读者的"读后校验"，从根源上避免撕裂读

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::base::subsystem::audio::common::StereoFrame;

/// 读者与写者共享的环形缓冲（容量为 2 的幂）
pub(crate) struct SharedRing {
    /// 帧数据槽位。并发访问的合法性由 head/tail 原子序与单写单读协议保证（见下）
    buf: UnsafeCell<Box<[StereoFrame]>>,
    capacity: usize,
    mask: usize,
    /// 读者游标（单调递增，仅读者写）
    head: AtomicUsize,
    /// 写者游标（单调递增，仅写者写）
    tail: AtomicUsize,
    /// 换代计数：seek 时 +1
    generation: AtomicU64,
    /// 解码端已到达流末尾（之后不会再有新数据）
    eof: AtomicBool,
    /// 读者侧统计：缓冲耗尽导致的欠载次数（诊断用）
    underruns: AtomicU64,
}

// SAFETY：`buf` 槽位的并发访问由 SPSC 协议保证互斥——
//   - 写者（解码线程，全进程唯一）只写 [tail, tail+n)，该区间 ⊆ 空闲区 [head, head+capacity)
//   - 读者（音频回调，全进程唯一）只读 [head, head+n)，该区间 ⊆ 已写区 [.., tail)
//   两区间永不相交；Release（写者发布 tail、读者发布 head）与对方的 Acquire 观测
//   建立 happens-before，保证数据可见。
// "每一侧唯一"由类型布局静态保证：写者句柄只交给 worker 线程，读者只存在于音频回调
// （StreamReader 仅被 MusicPlayer 在持 Inner 锁时调用）。
unsafe impl Send for SharedRing {}
unsafe impl Sync for SharedRing {}

impl SharedRing {
    /// 以指定帧容量创建（向上取 2 的幂，最小 16）
    ///
    /// 环是纯传输管道：采样率/时长等元数据由持有方（容器）自行保存。
    pub(crate) fn with_capacity(capacity_frames: usize) -> Self {
        let capacity = capacity_frames.max(16).next_power_of_two();
        Self {
            buf: UnsafeCell::new(vec![StereoFrame::SILENT; capacity].into_boxed_slice()),
            capacity,
            mask: capacity - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            generation: AtomicU64::new(0),
            eof: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
        }
    }

    // ───────────────────────── 读者侧（音频线程） ─────────────────────────

    /// 当前可读帧数
    pub(crate) fn available(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Relaxed);
        tail - head
    }

    /// 读者侧清空：丢弃全部未读数据（head 跳到当前写点，单调性保持）
    pub(crate) fn clear(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        self.head.store(tail, Ordering::Release);
    }

    /// 读走至多 `out.len()` 帧，返回实际帧数（绝不阻塞、绝不分配）
    ///
    /// 返回 0 且 [`finished`](Self::finished) 为假 = 暂时欠载。
    pub(crate) fn read(&self, out: &mut [StereoFrame]) -> usize {
        loop {
            let g0 = self.generation.load(Ordering::Acquire);
            let head = self.head.load(Ordering::Relaxed);
            let tail = self.tail.load(Ordering::Acquire);
            let avail = tail - head;
            let n = avail.min(out.len());
            self.copy_out(head, &mut out[..n]);

            // 读取期间发生了 seek（换代）：丢弃本次内容，读者自行跳到当前写点
            if self.generation.load(Ordering::Acquire) != g0 {
                self.head.store(tail, Ordering::Release);
                continue;
            }

            self.head.store(head + n, Ordering::Release);
            if n < out.len() && !self.eof.load(Ordering::Relaxed) {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
            return n;
        }
    }

    /// 整条流是否已播完（EOF 且缓冲已空）
    pub(crate) fn finished(&self) -> bool {
        self.eof.load(Ordering::Acquire)
            && self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Relaxed)
    }

    fn copy_out(&self, head: usize, out: &mut [StereoFrame]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        // SAFETY：[head, head+n) ⊆ [head, tail)，均为写者已发布的数据
        let buf = unsafe { &*self.buf.get() };
        let start = head & self.mask;
        let first = (self.capacity - start).min(n);
        out[..first].copy_from_slice(&buf[start..start + first]);
        if n > first {
            out[first..].copy_from_slice(&buf[..n - first]);
        }
    }

    // ───────────────────────── 写者侧（解码线程） ─────────────────────────

    /// 写入尽可能多的帧，返回实际写入数（缓冲满时小于入参长度）
    pub(crate) fn write(&self, frames: &[StereoFrame]) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let free = self.capacity - (tail - head);
        let n = free.min(frames.len());
        if n == 0 {
            return 0;
        }

        // SAFETY：[tail, tail+n) ⊆ 空闲区 [head, head+capacity)，读者不可见
        let buf = unsafe { &mut *self.buf.get() };
        let start = tail & self.mask;
        let first = (self.capacity - start).min(n);
        buf[start..start + first].copy_from_slice(&frames[..first]);
        if n > first {
            buf[..n - first].copy_from_slice(&frames[first..n]);
        }
        self.tail.store(tail + n, Ordering::Release);
        n
    }

    /// 当前空闲帧数（写者用于决定是否继续解码）
    pub(crate) fn free(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        self.capacity - (tail - head)
    }

    /// 标记解码结束（读者把剩余数据读完即视为整条流播完）
    pub(crate) fn set_eof(&self) {
        self.eof.store(true, Ordering::Release);
    }

    /// seek 前调用：换代。**必须先换代再写入任何新位置的数据**
    pub(crate) fn begin_generation(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn mk(v: f32) -> StereoFrame {
        StereoFrame { left: v, right: v }
    }

    #[test]
    fn write_read_roundtrip() {
        let ring = SharedRing::with_capacity(2048); // 容量 2048
        let frames: Vec<_> = (0..100).map(|i| mk(i as f32)).collect();
        assert_eq!(ring.write(&frames), 100);

        let mut out = vec![StereoFrame::SILENT; 100];
        assert_eq!(ring.read(&mut out), 100);
        for (i, f) in out.iter().enumerate() {
            assert_eq!(f.left, i as f32);
        }
        assert!(!ring.finished());
    }

    #[test]
    fn wraparound_keeps_order() {
        let ring = SharedRing::with_capacity(2048); // 容量 2048
        let w1: Vec<_> = (0..1000).map(|i| mk(i as f32)).collect();
        assert_eq!(ring.write(&w1), 1000);
        let mut out = vec![StereoFrame::SILENT; 1000];
        assert_eq!(ring.read(&mut out), 1000);

        // 第二次写入跨越环的物理边界（单调 tail=1000 → 2500 > 2048）
        let w2: Vec<_> = (0..1500).map(|i| mk(1000.0 + i as f32)).collect();
        assert_eq!(ring.write(&w2), 1500);
        let mut out2 = vec![StereoFrame::SILENT; 2000];
        assert_eq!(ring.read(&mut out2), 1500);
        for (i, f) in out2[..1500].iter().enumerate() {
            assert_eq!(f.left, 1000.0 + i as f32);
        }
    }

    #[test]
    fn eof_after_drain() {
        let ring = SharedRing::with_capacity(2048);
        let frames: Vec<_> = (0..10).map(|i| mk(i as f32)).collect();
        ring.write(&frames);
        ring.set_eof();
        // EOF 但数据未读完 → 未结束
        assert!(!ring.finished());
        let mut out = vec![StereoFrame::SILENT; 100];
        assert_eq!(ring.read(&mut out), 10);
        assert!(ring.finished());
    }

    #[test]
    fn seek_protocol_monotonic_and_valid() {
        let ring = SharedRing::with_capacity(2048);
        let frames: Vec<_> = (0..200).map(|i| mk(i as f32)).collect();
        ring.write(&frames); // 旧代数据
        ring.begin_generation(); // seek：换代
        ring.write(&frames); // 新代数据紧接写入

        // 读者按序读到全部数据（旧代残留 + 新代），无乱序无撕裂
        let mut out = vec![StereoFrame::SILENT; 1000];
        let n = ring.read(&mut out);
        assert_eq!(n, 400);
        for (i, f) in out[..200].iter().enumerate() {
            assert_eq!(f.left, i as f32);
        }
        for (i, f) in out[200..400].iter().enumerate() {
            assert_eq!(f.left, i as f32);
        }
    }

    /// 双线程压力传输：顺序与完整性必须严格保持
    #[test]
    fn spsc_stress_transfer() {
        let ring = Arc::new(SharedRing::with_capacity(2048));
        let total = 100_000usize;

        let w = ring.clone();
        let writer = std::thread::spawn(move || {
            let mut i = 0usize;
            while i < total {
                let chunk: Vec<_> = (i..(i + 64).min(total)).map(|k| mk(k as f32)).collect();
                let mut off = 0;
                while off < chunk.len() {
                    let n = w.write(&chunk[off..]);
                    off += n;
                    if off < chunk.len() {
                        std::hint::spin_loop();
                    }
                }
                i += chunk.len();
            }
            w.set_eof();
        });

        let mut got = 0usize;
        let mut expect = 0f32;
        let mut out = vec![StereoFrame::SILENT; 128];
        while got < total {
            let n = ring.read(&mut out);
            for f in &out[..n] {
                assert_eq!(f.left, expect, "顺序错误 @ {got}");
                expect += 1.0;
            }
            got += n;
        }
        writer.join().unwrap();
        assert!(ring.finished());
    }
}
