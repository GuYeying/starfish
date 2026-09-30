//! 视频音轨泵——mixer 流式声部的视频侧驱动
//!
//! 数据链：`mp4_demux::AudioDemuxer` 拉原始 AAC 样本 → symphonia 逐包解码
//! →（采样率 ≠ 混音域时）流式线性插值重采样 → `StreamVoice::push_interleaved`。
//! 驱动点在 [`Video::update`](super::Video::update)（共用核心不假设线程存在，
//! 桌面无线程、Web 无线程同一份代码）。
//!
//! 同步纪律（v1 = 视频时钟）：泵以视频主时钟为推帧目标——`pushed`（已解码
//! 的音轨位置）落后于时钟才继续解码；环形缓冲背压满即停推，剩余数据留在
//! `pending` 下帧优先冲销，**绝不丢弃**（丢样会爆音）。首帧对齐：泵就绪时
//! 直接从当前时钟起推（中途打开不回放历史），"同帧起播近似同步"由此成立。
//!
//! 容错：个别帧解码失败跳过（连续超限才禁用泵）——坏音轨不该拖死画面。

use std::io::Cursor;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions, CODEC_TYPE_AAC};
use symphonia::core::formats::Packet;

use super::mp4_demux::AudioDemuxer;
use super::VideoError;
use crate::base::audio::resample::StreamResampler;
use crate::base::audio::StreamVoice;

/// 连续解码失败上限（超过即禁用泵：整体坏流，逐帧跳过已无意义）
const MAX_CONSECUTIVE_ERRORS: u32 = 32;

/// 解码 + 重采样 + 推帧核心（数据就绪后才构造：native 同步装配 /
/// web fetch 完成后装配，见 [`AudioPump`]）
struct PumpCore {
    demux: AudioDemuxer<Cursor<Vec<u8>>>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    /// 源采样率 ≠ 声部采样率时的重采样器（None = 直通）
    resampler: Option<StreamResampler>,
    src_rate: u32,
    src_channels: usize,
    /// 已解码到的音轨位置（源时钟；与视频主时钟比较的量）
    pushed: Duration,
    /// 首帧对齐：第一次泵时从当前视频时钟起（跳过打开前的历史）
    started: bool,
    eos: bool,
    /// 背压未收完的声部采样率交错采样（下帧优先重试；以采样计非帧）
    pending: Vec<f32>,
    consecutive_errors: u32,
}

impl PumpCore {
    /// 从字节装配（demux + 解码器 + 重采样器）
    fn from_bytes(bytes: Vec<u8>, voice_rate: u32) -> Result<Self, VideoError> {
        let demux = AudioDemuxer::new(Cursor::new(bytes))?;
        let info = demux.info();

        // symphonia AAC 解码器：无 ASC 路径——采样率/声道经 CodecParameters
        // 声明，样本即裸 AAC GA 帧（symphonia 解码入口不解析 ADTS 头）。
        // 0.5 的 CodecParameters 为公开字段（无 builder setter）
        let mut params = CodecParameters::new();
        params.codec = CODEC_TYPE_AAC;
        params.sample_rate = Some(info.sample_rate);
        params.channels = Some(channels_to_flag(info.channels));

        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| VideoError::Backend(format!("AAC 解码器创建失败: {e}")))?;

        let resampler = (info.sample_rate != voice_rate)
            .then(|| StreamResampler::new(info.sample_rate, voice_rate));

        Ok(Self {
            demux,
            decoder,
            resampler,
            src_rate: info.sample_rate,
            src_channels: info.channels as usize,
            pushed: Duration::ZERO,
            started: false,
            eos: false,
            pending: Vec::new(),
            consecutive_errors: 0,
        })
    }

    /// 拉取并解码下一帧，返回**源采样率**的交错立体声采样
    fn decode_next(&mut self) -> Result<Option<Vec<f32>>, VideoError> {
        let Some(raw) = self.demux.next_sample()? else {
            return Ok(None);
        };
        // Packet 轨号仅作标识（解码器不消费 pts/轨号），恒 1；ts/dur 不参与解码
        let packet = Packet::new_from_boxed_slice(1, 0, 0, raw.into_boxed_slice());

        let decoded = self
            .decoder
            .decode(&packet)
            .map_err(|e| VideoError::Backend(format!("AAC 解码失败: {e}")))?;

        let spec = *decoded.spec();
        let frames = decoded.frames();
        let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let chunk = sample_buf.samples().to_vec();

        // 立体声化：单声道复制 L=R（混音环只收交错立体声）
        let out = if self.src_channels == 1 {
            let mut stereo = Vec::with_capacity(chunk.len() * 2);
            for s in chunk {
                stereo.push(s);
                stereo.push(s);
            }
            stereo
        } else {
            chunk
        };
        Ok(Some(out))
    }
}

/// 声道数 → symphonia 声道位标（1 = 前左；2 = 前左|前右）
fn channels_to_flag(channels: u8) -> symphonia::core::audio::Channels {
    use symphonia::core::audio::Channels;
    match channels {
        1 => Channels::FRONT_LEFT,
        _ => Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
    }
}

/// 视频音轨泵（应用经 `open_with_audio` 间接持有；控制面直通声部句柄）
///
/// `core` 的存放形态按平台分叉（native `Option` / web `Rc<RefCell<Option>>`
/// 承接异步装配），推帧逻辑经 [`AudioPump::with_core`] 收敛为一份。
pub(crate) struct AudioPump {
    voice: StreamVoice,
    /// native：构造即就绪；`None` = 装配失败已禁用
    #[cfg(not(target_arch = "wasm32"))]
    core: Option<PumpCore>,
    /// web：fetch 完成后填充；`None` = 未就绪 / 装配失败已禁用
    #[cfg(target_arch = "wasm32")]
    core: std::rc::Rc<std::cell::RefCell<Option<PumpCore>>>,
}

impl AudioPump {
    /// 打开音轨泵（native：同步装配，失败即返回——调用方可感知坏资产）
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open(path: &str, voice: StreamVoice) -> Result<Self, VideoError> {
        let rate = voice.sample_rate();
        let bytes = std::fs::read(path)
            .map_err(|e| VideoError::Backend(format!("音轨源读取失败 {path}: {e}")))?;
        Ok(Self {
            voice,
            core: Some(PumpCore::from_bytes(bytes, rate)?),
        })
    }

    /// 打开音轨泵（web：异步 fetch 同一 URL，完成前泵惰性——视频加载照走）
    ///
    /// 与视频后端各自 fetch 是既定架构（桌面后端同样与音轨泵各开一次文件）；
    /// 同 URL 二次请求通常命中浏览器缓存。装配失败仅记录诊断、静音降级：
    /// web 无同步错误通道，不该让坏音轨毁掉画面。
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn open(path: &str, voice: StreamVoice) -> Self {
        let rate = voice.sample_rate();
        let core = std::rc::Rc::new(std::cell::RefCell::new(None));
        {
            let core = core.clone();
            let path = path.to_string();
            wasm_bindgen_futures::spawn_local(async move {
                match super::web::fetch_bytes(&path).await {
                    Ok(bytes) => match PumpCore::from_bytes(bytes, rate) {
                        Ok(c) => *core.borrow_mut() = Some(c),
                        Err(e) => crate::base::debug::console_log(&format!(
                            "[video] 音轨泵装配失败（静音降级）: {e}"
                        )),
                    },
                    Err(msg) => crate::base::debug::console_log(&format!(
                        "[video] 音轨源 fetch 失败（静音降级）: {msg}"
                    )),
                }
            });
        }
        Self { voice, core }
    }

    /// 声部句柄（Video 的 set_muted / set_audio_volume 直通）
    pub(crate) fn voice(&self) -> &StreamVoice {
        &self.voice
    }

    /// 累计已推帧数（探针判读锚点：推泵活性）
    pub(crate) fn pushed_frames(&self) -> u64 {
        self.voice.pushed_frames()
    }

    /// 以视频主时钟为目标推进音轨（背压满即停推，不阻塞帧）
    pub(crate) fn pump(&mut self, clock: Duration) {
        self.with_core(|core, voice| {
            let Some(core) = core.as_mut() else {
                return;
            };
            if core.eos {
                return;
            }
            if !core.started {
                core.started = true;
                // 首帧对齐：直接从当前视频时钟起推（中途打开不回放历史）
                core.pushed = clock;
            }
            if clock <= core.pushed {
                return;
            }

            while core.pushed < clock {
                // 背压残留优先冲销（顺序保证：先推完旧数据才解码新数据）
                if !core.pending.is_empty() {
                    let accepted = voice.push_interleaved(&core.pending) * 2;
                    core.pending.drain(..accepted);
                    if !core.pending.is_empty() {
                        return; // 环满：停推，下帧再试
                    }
                    continue;
                }

                match core.decode_next() {
                    Ok(Some(chunk)) => {
                        core.consecutive_errors = 0;
                        // 解码游标按源时钟推进（chunk 为交错立体声：帧数 = len/2）
                        core.pushed += Duration::from_secs_f64(
                            chunk.len() as f64 / (2.0 * core.src_rate as f64),
                        );
                        // 采样率适配（源率 → 声部率）
                        let domain = match &mut core.resampler {
                            Some(rs) => {
                                let mut out = Vec::with_capacity(chunk.len());
                                rs.push(&chunk, &mut out);
                                out
                            }
                            None => chunk,
                        };
                        let accepted = voice.push_interleaved(&domain) * 2;
                        if accepted < domain.len() {
                            core.pending = domain[accepted..].to_vec();
                        }
                    }
                    Ok(None) => {
                        // 音轨自然结束：冲销残留后收工（ring 尾帧由混音侧排空）
                        let accepted = voice.push_interleaved(&core.pending) * 2;
                        core.pending.drain(..accepted);
                        if core.pending.is_empty() {
                            core.eos = true;
                        }
                        return; // 残留未清完则下帧继续冲销
                    }
                    Err(e) => {
                        // 跳过坏帧；连续超限才禁用泵（坏音轨不拖死画面）
                        core.consecutive_errors += 1;
                        if core.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                            crate::base::debug::console_log(&format!(
                                "[video] 音轨连续解码失败 {MAX_CONSECUTIVE_ERRORS} 帧，泵禁用: {e}"
                            ));
                            core.eos = true;
                            return;
                        }
                    }
                }
            }
        });
    }

    /// 平台核心槽的统一访问（native `Option` 直取 / web 借 RefCell）
    #[cfg(not(target_arch = "wasm32"))]
    fn with_core(&mut self, f: impl FnOnce(&mut Option<PumpCore>, &mut StreamVoice)) {
        f(&mut self.core, &mut self.voice);
    }

    #[cfg(target_arch = "wasm32")]
    fn with_core(&mut self, f: impl FnOnce(&mut Option<PumpCore>, &mut StreamVoice)) {
        let mut slot = self.core.borrow_mut();
        f(&mut slot, &mut self.voice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::audio::common::StereoFrame;
    use crate::base::audio::voice::StreamVoiceSlot;
    use std::sync::Arc;

    const SAMPLE: &str = "resources/videos/sample-5s.mp4";

    /// 无设备 StreamVoice（直构槽位：测试不依赖真实音频设备）
    fn test_voice() -> StreamVoice {
        StreamVoice {
            slot: Arc::new(StreamVoiceSlot::new(48_000)),
        }
    }

    /// AAC 解码链路：真文件的裸 AAC 样本必须解出非全零 PCM
    ///
    /// （交接风险点验证：symphonia 解码入口吃裸 GA 帧、CodecParameters 声明
    /// 采样率/声道——不解析 ADTS 头）
    #[test]
    fn aac_decode_produces_nonzero_pcm() {
        let bytes = std::fs::read(SAMPLE).expect("示例视频存在");
        let mut core = PumpCore::from_bytes(bytes, 48_000).expect("音轨泵核心装配");

        let mut total_samples = 0usize;
        let mut max_amp = 0f32;
        let mut nonzero = 0usize;
        loop {
            match core.decode_next().expect("解码") {
                Some(chunk) => {
                    total_samples += chunk.len();
                    for &v in &chunk {
                        let a = v.abs();
                        if a > max_amp {
                            max_amp = a;
                        }
                        if a > 0.0001 {
                            nonzero += 1;
                        }
                    }
                }
                None => break,
            }
        }
        // 全轨振幅（注意：资产开头有静音引导段，只测前几帧会误判全零）
        assert!(total_samples > 0, "应解出 PCM 采样");
        assert!(max_amp > 0.01, "解码 PCM 不应近全零: max={max_amp}");
        assert!(nonzero > 0, "解码 PCM 应含可闻段");
    }

    /// 推泵：模拟应用侧时钟逐帧推进——背压纪律（ring 容量封顶）+ 持续推进
    #[test]
    fn pump_pushes_into_voice_with_backpressure() {
        let mut voice = test_voice();
        let mut pump = AudioPump::open(SAMPLE, voice.clone()).expect("音轨泵打开");

        // 时钟从 0 逐帧推进 ~1s：ring 容量 16384 帧（≈340ms@48k）
        // → 背压先于时钟到达，推帧量必须封顶
        let mut clock = Duration::ZERO;
        for _ in 0..60 {
            clock += Duration::from_millis(16);
            pump.pump(clock);
        }
        let pushed = pump.pushed_frames();
        assert!(pushed > 0, "应有推帧");
        assert!(pushed <= 16384, "推帧量不得超过 ring 容量: {pushed}");

        // ring 应有可读数据（开头为静音引导段，不判振幅——非零判据在整轨测试）
        let mut out = vec![StereoFrame::SILENT; 4096];
        let n = voice.slot.ring.read(&mut out);
        assert!(n > 0, "ring 应有可读数据");

        // 模拟混音消费后，泵可持续推进（背压解锁）
        let before = pump.pushed_frames();
        voice.slot.ring.clear();
        for _ in 0..30 {
            clock += Duration::from_millis(16);
            pump.pump(clock);
        }
        assert!(pump.pushed_frames() > before, "消费后应继续解码推帧");
    }

    /// 整轨推完：时钟推进 + 反复排空 ring 直到 eos，总帧量 ≈ 音轨时长 × 声部
    /// 采样率，且全程含可闻段（非零 PCM 走通泵全链）
    #[test]
    fn pump_drains_full_track() {
        let mut voice = test_voice();
        let mut pump = AudioPump::open(SAMPLE, voice.clone()).expect("音轨泵打开");

        let mut total = 0usize;
        let mut max_amp = 0f32;
        let mut out = vec![StereoFrame::SILENT; 16384];
        let mut clock = Duration::ZERO;
        // 固定 500 帧迭代（8s 时钟 > 音轨 5.76s；eos 后 pump 为 no-op）
        for _ in 0..500 {
            clock += Duration::from_millis(16);
            pump.pump(clock);
            let n = voice.slot.ring.read(&mut out);
            total += n;
            for f in &out[..n] {
                max_amp = max_amp.max(f.left.abs()).max(f.right.abs());
            }
        }
        // 音轨 ≈5.76s @48000 ≈ 276_500 帧（±插值尾差与资产标称容差）
        assert!(total > 250_000, "整轨推帧量异常: {total}");
        assert!(max_amp > 0.01, "整轨推帧不应全零: max={max_amp}");
        assert!(
            pump.pushed_frames() >= total as u64,
            "推帧计数必须覆盖读走的帧"
        );
    }
}
