//! 音频引擎主入口
//!
//! 设备层见 [`device`]（cpal 胶水，平台差异唯一收敛点）；
//! 混音器/流式/录音等其余部分全部是平台中立纯逻辑。

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};

use self::common::{AudioError, AudioUserCallback, StereoFrame};
use self::device::DeviceStream;

#[inline]
fn atomic_f32_store(v: f32) -> u32 {
    v.to_bits()
}
#[inline]
fn atomic_f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}

pub mod common;
pub mod device;
pub mod record;
pub mod sfx;
mod music;
mod sound_data;
mod ring;
pub mod decoder;
pub mod stream;

#[cfg(test)]
mod test_support;

pub use sfx::{AudioEffect, ChannelState, FadeState, FadeType, SfxChannel};
pub use music::MusicPlayer;
pub use record::AudioRecorder;
pub use stream::MusicStream;
pub use sound_data::SoundData;

use self::music::MusicSource;

/// 不透明分组句柄，通过 `AudioMixer::create_group()` 创建
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupHandle(pub(crate) u32);

struct Inner {
    sfx_channels: Vec<SfxChannel>,
    music: MusicPlayer,
    scratch: Vec<StereoFrame>,
    reserved: usize,
    /// 组总线音量（稠密索引 = group_id - 1），混音时在锁内查表
    group_volumes: Vec<f32>,
}

impl Inner {
    fn new(num_channels: u32) -> Self {
        Self {
            sfx_channels: (0..num_channels).map(|_| SfxChannel::new()).collect(),
            music: MusicPlayer::new(),
            scratch: Vec::with_capacity(2048),
            reserved: 0,
            group_volumes: Vec::new(),
        }
    }

    /// 组总线增益（未知组 / 未分组返回 1.0）
    ///
    /// 独立函数形式：mix 循环中 scratch 处于借用状态，避免 &self 冲突
    fn group_gain(group_volumes: &[f32], group: Option<GroupHandle>) -> f32 {
        match group {
            Some(g) => group_volumes.get(g.0 as usize - 1).copied().unwrap_or(1.0),
            None => 1.0,
        }
    }

    fn mix(&mut self, output: &mut [StereoFrame], master_vol: f32, sfx_vol: f32, music_vol: f32) {
        output.fill(StereoFrame::SILENT);
        if self.scratch.len() < output.len() {
            self.scratch
                .extend(std::iter::repeat(StereoFrame::SILENT).take(output.len() - self.scratch.len()));
        }
        let group_volumes = &self.group_volumes;
        let scratch = &mut self.scratch[..output.len()];

        for ch in &mut self.sfx_channels {
            if ch.state != ChannelState::Playing {
                continue;
            }
            scratch.fill(StereoFrame::SILENT);
            let written = ch.read_frames(scratch);
            if written == 0 {
                continue;
            }
            let g = master_vol * sfx_vol
                * Self::group_gain(group_volumes, ch.group)
                * ch.volume
                * ch.fade_gain();
            if g <= 0.0 {
                continue;
            }
            for i in 0..written {
                if ch.pan >= 0.0 {
                    output[i].left += scratch[i].left * g * (1.0 - ch.pan);
                    output[i].right += scratch[i].right * g;
                } else {
                    output[i].left += scratch[i].left * g;
                    output[i].right += scratch[i].right * g * (1.0 + ch.pan);
                }
            }
        }

        if self.music.state == ChannelState::Playing {
            scratch.fill(StereoFrame::SILENT);
            let written = self.music.read_frames(scratch);
            if written > 0 {
                let g = master_vol * music_vol * self.music.fade_gain();
                if g > 0.0 {
                    for i in 0..written {
                        output[i].left += scratch[i].left * g;
                        output[i].right += scratch[i].right * g;
                    }
                }
            }
        }

        for out in output.iter_mut() {
            out.left = out.left / (1.0 + out.left.abs());
            out.right = out.right / (1.0 + out.right.abs());
        }
    }
}

struct AudioMixerCallback {
    inner: Arc<Mutex<Inner>>,
    master_volume: Arc<AtomicU32>,
    sfx_volume: Arc<AtomicU32>,
    music_volume: Arc<AtomicU32>,
}

impl AudioUserCallback for AudioMixerCallback {
    fn on_frames(&mut self, frames: &mut [StereoFrame]) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let master = atomic_f32_load(self.master_volume.load(Ordering::Relaxed));
        let sfx = atomic_f32_load(self.sfx_volume.load(Ordering::Relaxed));
        let music = atomic_f32_load(self.music_volume.load(Ordering::Relaxed));
        inner.mix(frames, master, sfx, music);
    }
}

pub struct AudioMixer {
    inner: Arc<Mutex<Inner>>,
    master_volume: Arc<AtomicU32>,
    sfx_volume: Arc<AtomicU32>,
    music_volume: Arc<AtomicU32>,
    /// cpal 设备流（Drop 即停止并释放设备）
    _stream: DeviceStream,
    pub output_sample_rate: u32,
    group_id_counter: u32,
}

/// 采样率域适配：源采样率 ≠ 混音域（设备真实采样率）时一次性线性插值重采样
///
/// cpal 无 SDL 式设备边界转换，混音域由设备决定；采样率适配职责移到数据侧，
/// 播放期零转换（与流式路径同款算法，见 [`SoundData::resample`]）。
fn resample_to_domain(sound: Arc<SoundData>, domain_rate: u32) -> Arc<SoundData> {
    if sound.sample_rate == domain_rate {
        sound
    } else {
        Arc::new((*sound).resample(domain_rate))
    }
}

impl AudioMixer {
    /// 创建混音器并打开默认播放设备（f32 立体声，构造即播放）
    ///
    /// 混音域 = 设备真实采样率，可用 [`output_sample_rate`](Self::output_sample_rate) 查询。
    /// 采样率不一致的音源由 [`load_sound`](Self::load_sound) / `play_*` 侧
    /// 一次性重采样；流式音乐（`music_load_file`）本来就按混音域重采样。
    pub fn new(num_channels: u32) -> Result<Self, AudioError> {
        let inner = Arc::new(Mutex::new(Inner::new(num_channels.max(1))));
        let master_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));
        let sfx_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));
        let music_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));

        let cb = AudioMixerCallback {
            inner: inner.clone(),
            master_volume: master_volume.clone(),
            sfx_volume: sfx_volume.clone(),
            music_volume: music_volume.clone(),
        };

        let stream = device::open_output_stream(cb)?;
        let output_sample_rate = stream.spec.sample_rate;

        Ok(Self {
            inner,
            master_volume,
            sfx_volume,
            music_volume,
            _stream: stream,
            output_sample_rate,
            group_id_counter: 1,
        })
    }

    // ── SFX API ──

    pub fn play(&mut self, sound: Arc<SoundData>) -> Result<Option<usize>, AudioError> {
        self.play_with(sound, 0, 0.0)
    }

    pub fn play_on(&mut self, sound: Arc<SoundData>, channel: usize) -> Result<(), AudioError> {
        let sound = resample_to_domain(sound, self.output_sample_rate);
        let mut inner = self.inner.lock().unwrap();
        let ch = inner
            .sfx_channels
            .get_mut(channel)
            .ok_or(AudioError::custom("channel out of range"))?;
        *ch = SfxChannel::with_sound(sound, 0, 0.0);
        Ok(())
    }

    pub fn play_with(
        &mut self,
        sound: Arc<SoundData>,
        loops: i32,
        fade_in_ms: f32,
    ) -> Result<Option<usize>, AudioError> {
        self.play_in_group(None, sound, loops, fade_in_ms)
    }

    pub fn play_in_group(
        &mut self,
        group: Option<GroupHandle>,
        sound: Arc<SoundData>,
        loops: i32,
        fade_in_ms: f32,
    ) -> Result<Option<usize>, AudioError> {
        let sound = resample_to_domain(sound, self.output_sample_rate);
        let mut inner = self.inner.lock().unwrap();
        let reserved = inner.reserved;
        let idx = if let Some(g) = group {
            inner
                .sfx_channels
                .iter()
                .position(|ch| ch.group == Some(g) && ch.state == ChannelState::Stopped)
        } else {
            inner
                .sfx_channels
                .iter()
                .enumerate()
                .skip(reserved)
                .find(|(_, ch)| ch.state == ChannelState::Stopped)
                .map(|(i, _)| i)
        };
        match idx {
            Some(i) => {
                inner.sfx_channels[i] = SfxChannel::with_sound(sound, loops, fade_in_ms);
                Ok(Some(i))
            }
            None => Ok(None),
        }
    }

    pub fn create_group(&mut self) -> GroupHandle {
        let id = self.group_id_counter;
        self.group_id_counter += 1;
        // 注册组总线音量槽位（默认 1.0）
        self.inner.lock().unwrap().group_volumes.push(1.0);
        GroupHandle(id)
    }

    /// 设置组总线音量（0.0 ~ 1.0）
    pub fn set_group_volume(&mut self, group: GroupHandle, volume: f32) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(v) = inner.group_volumes.get_mut(group.0 as usize - 1) {
            *v = volume.clamp(0.0, 1.0);
        }
    }

    /// 读取组总线音量（组不存在返回 None）
    pub fn group_volume(&self, group: GroupHandle) -> Option<f32> {
        self.inner
            .lock()
            .ok()?
            .group_volumes
            .get(group.0 as usize - 1)
            .copied()
    }

    pub fn remove_channel_group(&mut self, channel: usize) -> Result<(), AudioError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .sfx_channels
            .get_mut(channel)
            .ok_or(AudioError::custom("channel out of range"))?;
        inner.sfx_channels[channel].group = None;
        Ok(())
    }

    pub fn stop(&mut self, channel: usize) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.stop();
        }
    }

    pub fn stop_all(&mut self) {
        for ch in &mut self.inner.lock().unwrap().sfx_channels {
            ch.stop();
        }
    }

    pub fn stop_group(&mut self, group: GroupHandle) {
        for ch in &mut self.inner.lock().unwrap().sfx_channels {
            if ch.group == Some(group) {
                ch.stop();
            }
        }
    }

    pub fn pause(&mut self, channel: usize) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.state = ChannelState::Paused;
        }
    }

    pub fn resume(&mut self, channel: usize) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            if ch.state == ChannelState::Paused {
                ch.state = ChannelState::Playing;
            }
        }
    }

    pub fn pause_all(&mut self) {
        for ch in &mut self.inner.lock().unwrap().sfx_channels {
            if ch.state == ChannelState::Playing {
                ch.state = ChannelState::Paused;
            }
        }
    }

    pub fn resume_all(&mut self) {
        for ch in &mut self.inner.lock().unwrap().sfx_channels {
            if ch.state == ChannelState::Paused {
                ch.state = ChannelState::Playing;
            }
        }
    }

    pub fn set_channel_volume(&mut self, channel: usize, volume: f32) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.volume = volume.clamp(0.0, 1.0);
        }
    }

    pub fn set_channel_pan(&mut self, channel: usize, pan: f32) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.pan = pan.clamp(-1.0, 1.0);
        }
    }

    pub fn set_channel_group(
        &mut self,
        channel: usize,
        group: GroupHandle,
    ) -> Result<(), AudioError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .sfx_channels
            .get_mut(channel)
            .ok_or(AudioError::custom("channel out of range"))?;
        inner.sfx_channels[channel].group = Some(group);
        Ok(())
    }

    pub fn set_channel_group_range(
        &mut self,
        from: usize,
        to: usize,
        group: Option<GroupHandle>,
    ) -> Result<(), AudioError> {
        let mut inner = self.inner.lock().unwrap();
        for i in from..=to {
            inner
                .sfx_channels
                .get_mut(i)
                .ok_or(AudioError::custom("channel out of range"))?;
        }
        for ch in inner.sfx_channels[from..=to].iter_mut() {
            ch.group = group;
        }
        Ok(())
    }

    pub fn channel_fade_in(&mut self, channel: usize, ms: u32) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.fade = Some(FadeState::new_fade_in(
                ms,
                ch.data.as_ref().map(|d| d.sample_rate).unwrap_or(44100),
            ));
        }
    }

    pub fn channel_fade_out(&mut self, channel: usize, ms: u32) {
        if let Some(ch) = self.inner.lock().unwrap().sfx_channels.get_mut(channel) {
            ch.fade = Some(FadeState::new_fade_out(
                ms,
                ch.data.as_ref().map(|d| d.sample_rate).unwrap_or(44100),
            ));
        }
    }

    pub fn fade_out_group(&mut self, group: GroupHandle, ms: u32) {
        let mut inner = self.inner.lock().unwrap();
        for ch in &mut inner.sfx_channels {
            if ch.group == Some(group) {
                let sr = ch.data.as_ref().map(|d| d.sample_rate).unwrap_or(44100);
                ch.fade = Some(FadeState::new_fade_out(ms, sr));
            }
        }
    }

    pub fn group_count(&self, group: GroupHandle) -> usize {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .sfx_channels
                    .iter()
                    .filter(|ch| ch.group == Some(group))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn group_busy(&self, group: GroupHandle) -> bool {
        self.inner
            .lock()
            .map(|inner| {
                inner
                    .sfx_channels
                    .iter()
                    .any(|ch| ch.group == Some(group) && ch.state == ChannelState::Playing)
            })
            .unwrap_or(false)
    }

    pub fn group_available(&self, group: GroupHandle) -> Option<usize> {
        self.inner.lock().ok().and_then(|inner| {
            inner
                .sfx_channels
                .iter()
                .position(|ch| ch.group == Some(group) && ch.state == ChannelState::Stopped)
        })
    }

    pub fn is_channel_busy(&self, channel: usize) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| {
                inner
                    .sfx_channels
                    .get(channel)
                    .map(|ch| ch.state == ChannelState::Playing)
            })
            .unwrap_or(false)
    }

    pub fn get_busy(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner
            .sfx_channels
            .iter()
            .any(|ch| ch.state == ChannelState::Playing)
            || inner.music.state == ChannelState::Playing
    }

    pub fn set_reserved(&mut self, n: usize) {
        self.inner.lock().unwrap().reserved = n;
    }

    pub fn find_free_channel(&self) -> Option<usize> {
        let inner = self.inner.lock().ok()?;
        inner
            .sfx_channels
            .iter()
            .position(|ch| ch.state == ChannelState::Stopped)
    }

    pub fn get_channel_sound(&self, channel: usize) -> Option<Arc<SoundData>> {
        self.inner
            .lock()
            .ok()?
            .sfx_channels
            .get(channel)?
            .get_sound()
    }

    pub fn num_channels(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.sfx_channels.len())
            .unwrap_or(0)
    }

    pub fn sfx_add_effect(
        &mut self,
        channel: usize,
        effect: Box<dyn AudioEffect>,
    ) -> Result<(), AudioError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .sfx_channels
            .get_mut(channel)
            .ok_or(AudioError::custom("channel out of range"))?
            .effects
            .push(effect);
        Ok(())
    }

    // ── Music API ──
    //
    // 长音频走流式（MusicStream，边解码边播），整段内存数据（SoundData）保留兼容。
    // 所有会替换当前源的操作都在锁外 join 退役的解码线程。

    /// 加载流式音乐文件（边解码边播，适合 BGM / 环境音 / 语音）
    pub fn music_load_file(&mut self, path: &str) -> Result<(), AudioError> {
        let stream = MusicStream::from_file(path, self.output_sample_rate)?;
        self.inner
            .lock()
            .unwrap()
            .music
            .load(MusicSource::Stream(stream));
        self.join_retired();
        Ok(())
    }

    /// 加载整段音频数据（兼容旧接口；短音频可整段解码后交给 music）
    pub fn music_load(&mut self, data: Arc<SoundData>) {
        self.inner
            .lock()
            .unwrap()
            .music
            .load(MusicSource::Buffer { data, cursor: 0 });
        self.join_retired();
    }

    /// 排队下一个流式音乐文件（立即预起解码线程，灌满缓冲即 park）
    pub fn music_queue_file(&mut self, path: &str) -> Result<(), AudioError> {
        let stream = MusicStream::from_file(path, self.output_sample_rate)?;
        self.inner
            .lock()
            .unwrap()
            .music
            .queue(MusicSource::Stream(stream));
        Ok(())
    }

    /// 排队下一首（整段内存数据）
    pub fn music_queue(&mut self, data: Arc<SoundData>) {
        self.inner
            .lock()
            .unwrap()
            .music
            .queue(MusicSource::Buffer { data, cursor: 0 });
    }

    /// 无线程环境（Web）：每帧推进流式解码（预算：帧数）
    ///
    /// 桌面（有线程）无需调用——解码线程自动维持缓冲。
    pub fn pump_streams(&mut self, budget_frames: usize) {
        self.inner.lock().unwrap().music.pump(budget_frames);
    }

    /// 加载短音效文件为可复用的 SoundData（对位 pygame.mixer.Sound）
    ///
    /// 解码后一次性重采样到混音域（设备真实采样率），播放期零转换。
    pub fn load_sound(&self, path: &str) -> Result<Arc<SoundData>, AudioError> {
        let data = SoundData::from_file(path)?;
        Ok(resample_to_domain(Arc::new(data), self.output_sample_rate))
    }

    /// 控制线程锁外 join 已退役的解码线程
    ///
    /// JoinHandle 绝不放在与音频回调共享的锁内 join——这是流式线程安全的底线。
    fn join_retired(&mut self) {
        let handles = self
            .inner
            .lock()
            .map(|mut inner| inner.music.drain_retired())
            .unwrap_or_default();
        for h in handles {
            let _ = h.join();
        }
    }

    pub fn music_play(&mut self, loops: i32) {
        self.inner.lock().unwrap().music.play(loops);
    }
    pub fn music_stop(&mut self) {
        self.inner.lock().unwrap().music.stop();
    }
    pub fn music_pause(&mut self) {
        self.inner.lock().unwrap().music.pause();
    }
    pub fn music_resume(&mut self) {
        self.inner.lock().unwrap().music.resume();
    }
    pub fn music_fade_out(&mut self, ms: u32) {
        self.inner.lock().unwrap().music.fade_out(ms);
    }
    pub fn music_fade_in(&mut self, ms: u32) {
        self.inner.lock().unwrap().music.fade_in(ms);
    }
    pub fn music_add_effect(&mut self, effect: Box<dyn AudioEffect>) {
        self.inner.lock().unwrap().music.add_effect(effect);
    }

    pub fn music_set_volume(&self, volume: f32) {
        self.music_volume
            .store(atomic_f32_store(volume.clamp(0.0, 1.0)), Ordering::Relaxed);
    }
    pub fn music_get_volume(&self) -> f32 {
        atomic_f32_load(self.music_volume.load(Ordering::Relaxed))
    }
    pub fn music_seek(&mut self, seconds: f32) {
        self.inner.lock().unwrap().music.seek(seconds);
    }
    pub fn music_position(&self) -> f32 {
        self.inner
            .lock()
            .map(|inner| inner.music.position())
            .unwrap_or(0.0)
    }
    pub fn music_duration(&self) -> f32 {
        self.inner
            .lock()
            .map(|inner| inner.music.duration())
            .unwrap_or(0.0)
    }
    pub fn music_is_playing(&self) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.music.state == ChannelState::Playing)
            .unwrap_or(false)
    }
    pub fn music_rewind(&mut self) {
        self.inner.lock().unwrap().music.rewind();
    }

    // ── 全局音量 ──

    pub fn set_master_volume(&self, volume: f32) {
        self.master_volume
            .store(atomic_f32_store(volume.clamp(0.0, 1.0)), Ordering::Relaxed);
    }
    pub fn master_volume(&self) -> f32 {
        atomic_f32_load(self.master_volume.load(Ordering::Relaxed))
    }
    pub fn set_sfx_volume(&self, volume: f32) {
        self.sfx_volume
            .store(atomic_f32_store(volume.clamp(0.0, 1.0)), Ordering::Relaxed);
    }
    pub fn sfx_volume(&self) -> f32 {
        atomic_f32_load(self.sfx_volume.load(Ordering::Relaxed))
    }
}

impl Drop for AudioMixer {
    fn drop(&mut self) {
        // 收口解码线程：停掉当前源与队列中的所有流式 worker，
        // 在控制线程（此刻无锁竞争）join 完毕后，cpal 流随字段析构停止。
        let handles = self
            .inner
            .lock()
            .map(|mut inner| {
                inner.music.shutdown();
                inner.music.drain_retired()
            })
            .unwrap_or_default();
        for h in handles {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::common::StereoFrame as TestFrame;

    #[test]
    fn group_volume_attenuates_mix() {
        let mut inner = Inner::new(4);
        let samples = vec![0.1f32; 200];
        let data = Arc::new(SoundData::from_interleaved_f32(&samples, 44100));
        let mut ch = SfxChannel::with_sound(data, 0, 0.0);
        ch.group = Some(GroupHandle(1));
        inner.sfx_channels[0] = ch;
        inner.group_volumes.push(0.5); // 组 1 音量 0.5

        let mut out = vec![TestFrame::SILENT; 32];
        inner.mix(&mut out, 1.0, 1.0, 1.0);

        // 0.1（源）× 1（master）× 1（sfx）× 0.5（组）× 1（声道）× 1（淡变）= 0.05
        // 经软限幅 x/(1+|x|) → 0.05/1.05
        let expected = 0.05 / 1.05;
        assert!((out[0].left - expected).abs() < 1e-6);
        assert!((out[0].right - expected).abs() < 1e-6);
    }

    #[test]
    fn unknown_group_gain_is_one() {
        assert_eq!(Inner::group_gain(&[], Some(GroupHandle(3))), 1.0);
        assert_eq!(Inner::group_gain(&[], None), 1.0);
        let vols = vec![0.25f32];
        assert_eq!(Inner::group_gain(&vols, Some(GroupHandle(1))), 0.25);
    }
}
