//! 流式线性插值重采样——[`SoundData::resample`](crate::base::audio::SoundData::resample)
//! 同款算法的流式版
//!
//! 与一次性版的差异只在状态管理：采样位置跨块延续，块边界用"上一块末帧"
//! 作插值左邻（右邻永远在当前块内），末尾 [`flush`](StreamResampler::flush)
//! 复制末帧补齐。音质契约与整段版一致（线性插值）。
//!
//! 输入为**交错立体声** f32（单声道源由调用方先复制成 L=R——输出反正要进
//! `StreamVoice` 的立体声环）。典型客户：视频音轨泵（AAC 44100/48000 →
//! 设备混音域）。

/// 源采样率 → 目标采样率的流式重采样器
pub struct StreamResampler {
    /// 源采样率 / 目标采样率（< 1 = 升采样）
    ratio: f64,
    /// 下一输出帧对应的源流位置（帧，含小数；源流绝对坐标）
    next_out: f64,
    /// 已接收的源帧总数
    total_in: u64,
    /// 上一块的末帧（跨块插值的左邻）
    prev: [f32; 2],
    /// `prev` 是否有效（首块前为假：流开头无左邻可借）
    has_prev: bool,
}

impl StreamResampler {
    /// 创建重采样器（采样率相同也按同一路径工作——直通等价）
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            ratio: src_rate as f64 / dst_rate.max(1) as f64,
            next_out: 0.0,
            total_in: 0,
            prev: [0.0; 2],
            has_prev: false,
        }
    }

    /// 喂入一段源采样率交错立体声块，追加目标采样率输出到 `out`
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        let chunk_end = self.total_in + frames as u64;

        // 可插值范围：右邻 ≤ 块末帧，即 next_out < chunk_end - 1。
        // 块界左邻 = prev（上一块末帧，绝对下标 total_in-1）。
        let limit = chunk_end as f64 - 1.0;
        while self.next_out < limit {
            let p = self.next_out;
            let idx = p as u64;
            let frac = (p - idx as f64) as f32;
            let left = if self.has_prev && idx < self.total_in {
                self.prev
            } else {
                frame_at(input, (idx - self.total_in) as usize)
            };
            let right = frame_at(input, (idx + 1 - self.total_in) as usize);
            out.push(left[0] + (right[0] - left[0]) * frac);
            out.push(left[1] + (right[1] - left[1]) * frac);
            self.next_out += self.ratio;
        }

        self.prev = frame_at(input, frames - 1);
        self.has_prev = true;
        self.total_in = chunk_end;
    }

    /// 流结束：复制末帧补齐剩余输出（把 `next_out` 推进到流末）
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if !self.has_prev {
            return;
        }
        let end = self.total_in as f64;
        while self.next_out < end {
            out.extend_from_slice(&self.prev);
            self.next_out += self.ratio;
        }
    }
}

/// 块内第 `i` 帧的 (L, R)
fn frame_at(input: &[f32], i: usize) -> [f32; 2] {
    [input[i * 2], input[i * 2 + 1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 恒定值信号：任意采样率下输出恒等
    #[test]
    fn constant_signal_passes_through() {
        let mut rs = StreamResampler::new(44100, 48000);
        let mut out = Vec::new();
        rs.push(&[0.5; 4410], &mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|&v| (v - 0.5).abs() < 1e-6));
    }

    /// 分块喂入与整段一次喂入结果一致（状态跨块正确）
    ///
    /// 奇数采样长度的块会破坏帧对齐（`input.len()/2` 截尾）——块长必须
    /// 为 2 的倍数（完整帧），此为调用方契约
    #[test]
    fn chunked_matches_single_shot() {
        let src: Vec<f32> = (0..8820).map(|i| ((i as f32) * 0.01).sin()).collect();
        let mut whole = Vec::new();
        StreamResampler::new(44100, 48000).push(&src, &mut whole);

        let mut chunked = Vec::new();
        let mut rs = StreamResampler::new(44100, 48000);
        let mut i = 0;
        while i < src.len() {
            let end = (i + 440).min(src.len() & !1); // 偶数长度块：逼出跨块插值
            rs.push(&src[i..end], &mut chunked);
            i = end;
        }
        assert_eq!(whole.len(), chunked.len(), "分块与整段输出帧数必须一致");
        for (a, b) in whole.iter().zip(chunked.iter()) {
            assert!((a - b).abs() < 1e-6, "分块与整段输出值必须一致");
        }
    }

    /// 采样率相同 + flush：输出与输入逐帧一致（直通等价）
    #[test]
    fn same_rate_is_passthrough() {
        let src: Vec<f32> = (0..1000).map(|i| i as f32 * 0.001).collect();
        let mut out = Vec::new();
        let mut rs = StreamResampler::new(48000, 48000);
        rs.push(&src, &mut out);
        rs.flush(&mut out);
        assert_eq!(out.len(), src.len());
        for (a, b) in out.iter().zip(src.iter()) {
            assert_eq!(a, b);
        }
    }

    /// flush：末尾用复制帧补齐，总输出帧数 ≈ 输入时长 × 目标率
    #[test]
    fn flush_completes_tail() {
        let mut rs = StreamResampler::new(44100, 48000);
        let mut out = Vec::new();
        rs.push(&[0.25; 88200], &mut out); // 1s 源 = 44100 帧 = 88200 采样
        rs.flush(&mut out);
        let frames = out.len() / 2;
        // 1s @48000 = 48000 帧，线性插值边界容差 ±2 帧
        assert!(
            (frames as i64 - 48000).abs() <= 2,
            "输出帧数 {frames} 应≈48000"
        );
        assert!(out.iter().all(|&v| (v - 0.25).abs() < 1e-6));
    }
}
