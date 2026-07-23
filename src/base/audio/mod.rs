//! 音频引擎主入口

use sdl3::audio::AudioStreamWithCallback;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Mutex,
};

use crate::base::subsystem::audio::{
    common::{AudioError, AudioUserCallback, StereoFrame},
    playback::PlaybackCallback,
    AudioSubsystem,
};

#[inline]
fn atomic_f32_store(v: f32) -> u32 {
    v.to_bits()
}
#[inline]
fn atomic_f32_load(v: u32) -> f32 {
    f32::from_bits(v)
}

pub mod sfx;
mod music;
mod track;
pub mod decoder;

pub use sfx::{AudioEffect, ChannelState, FadeState, FadeType, SfxChannel};
pub use music::MusicPlayer;
pub use track::SoundData;

/// 不透明分组句柄，通过 `AudioEngine::create_group()` 创建
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupHandle(pub(crate) u32);

struct Inner {
    sfx_channels: Vec<SfxChannel>,
    music: MusicPlayer,
    scratch: Vec<StereoFrame>,
    reserved: usize,
}

impl Inner {
    fn new(num_channels: u32) -> Self {
        Self {
            sfx_channels: (0..num_channels).map(|_| SfxChannel::new()).collect(),
            music: MusicPlayer::new(),
            scratch: Vec::with_capacity(2048),
            reserved: 0,
        }
    }

    fn mix(&mut self, output: &mut [StereoFrame], master_vol: f32, sfx_vol: f32, music_vol: f32) {
        output.fill(StereoFrame::SILENT);
        if self.scratch.len() < output.len() {
            self.scratch
                .extend(std::iter::repeat(StereoFrame::SILENT).take(output.len() - self.scratch.len()));
        }
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
            let g = master_vol * sfx_vol * ch.volume * ch.fade_gain();
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

struct AudioEngineCallback {
    inner: Arc<Mutex<Inner>>,
    master_volume: Arc<AtomicU32>,
    sfx_volume: Arc<AtomicU32>,
    music_volume: Arc<AtomicU32>,
}

impl AudioUserCallback for AudioEngineCallback {
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

pub struct AudioEngine {
    inner: Arc<Mutex<Inner>>,
    master_volume: Arc<AtomicU32>,
    sfx_volume: Arc<AtomicU32>,
    music_volume: Arc<AtomicU32>,
    _stream: AudioStreamWithCallback<PlaybackCallback>,
    pub output_sample_rate: u32,
    group_id_counter: u32,
}

impl AudioEngine {
    pub fn new(
        subsystem: &AudioSubsystem,
        sample_rate: u32,
        num_channels: u32,
    ) -> Result<Self, AudioError> {
        let spec = sdl3::audio::AudioSpec::new(
            Some(sample_rate as i32),
            Some(2),
            Some(sdl3::audio::AudioFormat::F32LE),
        );
        Self::new_with_spec(subsystem, &spec, num_channels)
    }

    pub fn new_with_spec(
        subsystem: &AudioSubsystem,
        spec: &sdl3::audio::AudioSpec,
        num_channels: u32,
    ) -> Result<Self, AudioError> {
        let inner = Arc::new(Mutex::new(Inner::new(num_channels.max(1))));
        let master_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));
        let sfx_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));
        let music_volume = Arc::new(AtomicU32::new(atomic_f32_store(1.0)));

        let cb = PlaybackCallback::new(AudioEngineCallback {
            inner: inner.clone(),
            master_volume: master_volume.clone(),
            sfx_volume: sfx_volume.clone(),
            music_volume: music_volume.clone(),
        });

        let device = subsystem.default_playback_device();
        let stream = subsystem
            .open_playback_stream_with_callback(&device, spec, cb)
            .map_err(|e: sdl3::Error| {
                AudioError::custom(format!("SDL stream open failed: {e}"))
            })?;
        stream.resume().map_err(|e: sdl3::Error| {
            AudioError::custom(format!("SDL stream resume failed: {e}"))
        })?;

        let output_sample_rate = spec.freq.unwrap_or(44100) as u32;
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
        GroupHandle(id)
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

    pub fn music_load(&mut self, data: Arc<SoundData>) {
        self.inner.lock().unwrap().music.load(data);
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
    pub fn music_queue(&mut self, data: Arc<SoundData>) {
        self.inner.lock().unwrap().music.queue(data);
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
