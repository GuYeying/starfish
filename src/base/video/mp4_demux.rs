//! MP4(H.264) 解复用共享层——android / web 后端共用的纯 Rust 解复用
//!
//! 平台硬解框架（MediaCodec / WebCodecs）都吃裸 H.264 码流，不经各自系统
//! 解容器：本层用 `mp4` crate（纯 Rust）拉出视频轨样本，统一转 Annex-B
//! （00 00 00 01 起始码），并携带 SPS/PPS（Annex-B 帧，关键帧前置）。
//!
//! Windows 上 MF/VT 自带解复用，不参与本模块——但 `test` cfg 使其在
//! `cargo test` 时可用真文件（`resources/videos/sample-5s.mp4`）做集成测试。

#![cfg(any(target_os = "android", target_arch = "wasm32", test))]

use std::io::{Read, Seek};
use std::time::Duration;

use super::VideoError;

/// 恒定帧率兜底（拿不到 frame_rate 时按 30fps 步进）
const FALLBACK_FPS: f64 = 30.0;

pub(crate) struct Demuxer<R: Read + Seek> {
    reader: mp4::Mp4Reader<R>,
    track_id: u32,
    sample_count: u32,
    next_id: u32,
    timescale: u32,
    /// 帧步进（cfr 假设：1 / frame_rate；pts 兜底）
    frame_step: Duration,
    width: u32,
    height: u32,
    /// 参数集（Annex-B 帧：00 00 00 01 + NAL）
    sps_annexb: Vec<u8>,
    pps_annexb: Vec<u8>,
    params_sent: bool,
    /// WebCodecs 用（从 SPS 提取 profile/compat/level，如 "avc1.640028"）
    pub(crate) codec_string: String,
}

impl<R: Read + Seek> Demuxer<R> {
    pub fn new(mut reader: R) -> Result<Self, VideoError> {
        let size = reader
            .seek(std::io::SeekFrom::End(0))
            .map_err(|e| VideoError::Backend(format!("mp4 seek 失败: {e}")))?;
        reader
            .seek(std::io::SeekFrom::Start(0))
            .map_err(|e| VideoError::Backend(format!("mp4 seek 失败: {e}")))?;
        let mp4_reader = mp4::Mp4Reader::read_header(reader, size)
            .map_err(|e| VideoError::Backend(format!("mp4 解析失败: {e}")))?;

        // 找 H.264 视频轨
        let mut found: Option<(u32, &mp4::Mp4Track)> = None;
        for (id, track) in mp4_reader.tracks() {
            if matches!(track.track_type(), Ok(mp4::TrackType::Video))
                && matches!(track.media_type(), Ok(mp4::MediaType::H264))
            {
                found = Some((*id, track));
                break;
            }
        }
        let Some((track_id, track)) = found else {
            return Err(VideoError::Backend("mp4 中无 H.264 视频轨".into()));
        };

        // 先提取全部轨道数据（track 借用 reader，须在 reader 移动前结束）
        let sample_count = track.sample_count();
        let timescale = track.timescale();
        let width = track.width() as u32;
        let height = track.height() as u32;
        let fps = track.frame_rate();
        let frame_step =
            Duration::from_secs_f64(if fps > 0.0 { 1.0 / fps } else { 1.0 / FALLBACK_FPS });

        // 参数集：avcC → SPS/PPS NAL（补起始码）
        let avc1 = track
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .ok_or_else(|| VideoError::Backend("视频轨缺少 avcC 配置".into()))?;
        let sps = avc1
            .avcc
            .sequence_parameter_sets
            .first()
            .map(|n| n.bytes.as_slice())
            .ok_or_else(|| VideoError::Backend("avcC SPS 缺失".into()))?;
        let pps = avc1
            .avcc
            .picture_parameter_sets
            .first()
            .map(|n| n.bytes.as_slice())
            .ok_or_else(|| VideoError::Backend("avcC PPS 缺失".into()))?;
        let with_start_code = |nal: &[u8]| -> Vec<u8> {
            let mut v = Vec::with_capacity(nal.len() + 4);
            v.extend_from_slice(&[0, 0, 0, 1]);
            v.extend_from_slice(nal);
            v
        };
        let sps_annexb = with_start_code(sps);
        let pps_annexb = with_start_code(pps);

        // codec string：SPS NAL = [头, profile_idc, constraint, level_idc, …]
        let codec_string = if sps.len() >= 4 {
            format!("avc1.{:02x}{:02x}{:02x}", sps[1], sps[2], sps[3])
        } else {
            "avc1.42E01E".into() // constrained baseline 兜底
        };

        Ok(Self {
            reader: mp4_reader,
            track_id,
            sample_count,
            next_id: 1, // mp4 crate 样本编号从 1 起
            timescale,
            frame_step,
            width,
            height,
            sps_annexb,
            pps_annexb,
            params_sent: false,
            codec_string,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// SPS（Annex-B 帧，含起始码）——MediaCodec csd-0 注入用
    pub(crate) fn sps_annexb(&self) -> &[u8] {
        &self.sps_annexb
    }

    /// PPS（Annex-B 帧，含起始码）——MediaCodec csd-1 注入用
    pub(crate) fn pps_annexb(&self) -> &[u8] {
        &self.pps_annexb
    }

    /// 下一帧样本：`Ok(None)` = 流结束；`(码流, pts, 是否关键帧)`。
    /// 输出 Annex-B；关键帧前置 SPS/PPS（首样本必为关键帧，参数集随之送达）。
    pub fn next_sample(&mut self) -> Result<Option<(Vec<u8>, Duration, bool)>, VideoError> {
        if self.next_id > self.sample_count {
            return Ok(None);
        }
        let Some(sample) = self
            .reader
            .read_sample(self.track_id, self.next_id)
            .map_err(|e| VideoError::Backend(format!("mp4 读样本失败: {e}")))?
        else {
            return Ok(None);
        };
        self.next_id += 1;

        let mut pts = Duration::from_secs_f64(sample.start_time as f64 / self.timescale as f64);
        let mut data = if sample.is_sync {
            self.params_sent = true;
            let mut v = Vec::with_capacity(
                self.sps_annexb.len() + self.pps_annexb.len() + sample.bytes.len() + 8,
            );
            v.extend_from_slice(&self.sps_annexb);
            v.extend_from_slice(&self.pps_annexb);
            v.extend_from_slice(avcc_to_annexb(sample.bytes.as_ref()).as_slice());
            v
        } else {
            avcc_to_annexb(sample.bytes.as_ref())
        };

        // 个别流首样本时间戳异常（负/超大），兜底为帧步进序列
        if !self.params_sent && pts > Duration::from_secs(1) {
            pts = self.frame_step;
        }

        Ok(Some((std::mem::take(&mut data), pts, sample.is_sync)))
    }

    /// 非关键帧的 pts 兜底（时间戳单调用）
    pub fn frame_step(&self) -> Duration {
        self.frame_step
    }
}

/// AVCC（4 字节长度前缀 NAL 串）→ Annex-B（00 00 00 01 起始码）
fn avcc_to_annexb(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0;
    while i + 4 <= data.len() {
        let n = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if n == 0 || i + n > data.len() {
            break; // 容错：长度异常即止（损坏样本交解码器报错）
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[i..i + n]);
        i += n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SAMPLE: &str = "resources/videos/sample-5s.mp4";

    #[test]
    fn demux_real_sample_file() {
        let bytes = std::fs::read(SAMPLE).expect("示例视频存在");
        let mut demux = Demuxer::new(Cursor::new(bytes)).expect("解复用成功");
        let (w, h) = demux.size();
        assert_eq!((w, h), (1920, 1080));
        assert!(demux.codec_string.starts_with("avc1."));

        // 首样本：关键帧前置参数集，起始码开头，pts ≈ 0
        let (data0, pts0, key0) = demux.next_sample().expect("读样本").expect("有首帧");
        assert!(key0, "首样本应为关键帧");
        assert!(pts0 < Duration::from_millis(100));
        assert!(data0.starts_with(&[0, 0, 0, 1]));
        // 参数集在帧头（至少 2 个起始码：SPS/PPS，其后 IDR NAL）
        assert!(data0.len() > 32);

        // 整轨拉完：pts 单调不减、全部 Annex-B 形态
        let mut count = 1;
        let mut last_pts = pts0;
        while let Some(Some((data, pts, _))) = Some(demux.next_sample().expect("读样本")) {
            assert!(pts >= last_pts, "pts 必须单调不减");
            last_pts = pts;
            assert!(data.starts_with(&[0, 0, 0, 1]));
            count += 1;
        }
        // 5s 视频应有可观帧数（30fps → ~150）
        assert!(count > 60, "帧数异常: {count}");
    }

    #[test]
    fn avcc_to_annexb_splits_length_prefixed_nals() {
        let avcc = [0, 0, 0, 2, 0xAA, 0xBB, 0, 0, 0, 1, 0xCC];
        let out = avcc_to_annexb(&avcc);
        assert_eq!(out, vec![0, 0, 0, 1, 0xAA, 0xBB, 0, 0, 0, 1, 0xCC]);
    }

    #[test]
    fn avcc_to_annexb_stops_on_corrupt_length() {
        // 长度声明越界 → 截断不 panic
        let avcc = [0, 0, 0, 99, 0xAA];
        assert!(avcc_to_annexb(&avcc).is_empty());
    }
}
