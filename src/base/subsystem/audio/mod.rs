use std::ffi::{c_void, CStr};

use sdl3::{
    AudioSubsystem as SdlAudioSubsystem, Error, Sdl, audio::{AudioCallback, AudioDevice, AudioDeviceID, AudioFormatNum, AudioRecordingCallback, AudioSpec, AudioStreamOwner, AudioStreamWithCallback}, get_error, libc::free, sys::audio::{
        self as sys_audio, SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, SDL_AUDIO_DEVICE_DEFAULT_RECORDING, SDL_AudioDeviceID, SDL_AudioSpec
    }
};

pub mod common;
pub mod playback;
pub mod recording;



/// 音频子系统外层封装（与 EventSubsystem 风格完全统一）
#[derive(Debug, Clone)]
pub struct AudioSubsystem {
    inner: SdlAudioSubsystem,
}

impl AudioSubsystem {
    pub fn new(sdl:&Sdl) -> Self {
        let inner = sdl.audio().expect("AudioSubsystem init failed.");
        Self { inner }
    }

    pub fn inner(&self)->&SdlAudioSubsystem{
        &self.inner
    }

    // -------------------------------------------------------------------------
    // 枚举音频设备 ID
    // -------------------------------------------------------------------------
    /// 枚举所有播放设备 ID
    pub fn audio_playback_device_ids(&self) -> Result<Vec<AudioDeviceID>, Error> {
        self.audio_device_ids(|num| unsafe { sys_audio::SDL_GetAudioPlaybackDevices(num) })
    }

    /// 枚举所有录音设备 ID
    pub fn audio_recording_device_ids(&self) -> Result<Vec<AudioDeviceID>, Error> {
        self.audio_device_ids(|num| unsafe { sys_audio::SDL_GetAudioRecordingDevices(num) })
    }

    /// 通用设备 ID 枚举内部实现
    fn audio_device_ids<F>(&self, get_devices: F) -> Result<Vec<AudioDeviceID>, Error>
    where
        F: FnOnce(&mut i32) -> *mut SDL_AudioDeviceID,
    {
        let mut count = 0;
        let devices_ptr = get_devices(&mut count);

        if devices_ptr.is_null() {
            return Err(get_error());
        }

        let mut list = Vec::with_capacity(count as usize);
        for i in 0..count {
            let dev_id = unsafe { *devices_ptr.offset(i as isize) };
            list.push(AudioDeviceID::Device(dev_id));
        }

        // 释放 SDL 分配的堆内存
        unsafe { free(devices_ptr as *mut c_void) };
        Ok(list)
    }

    // -------------------------------------------------------------------------
    // 打开默认设备
    // -------------------------------------------------------------------------
    /// 打开默认播放设备
    pub fn open_playback_device(&self, spec: &AudioSpec) -> Result<AudioDevice, Error> {
        self.open_device(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK, spec)
    }

    /// 打开默认录音设备
    pub fn open_recording_device(&self, spec: &AudioSpec) -> Result<AudioDevice, Error> {
        self.open_device(SDL_AUDIO_DEVICE_DEFAULT_RECORDING, spec)
    }

    /// 获取默认播放设备实例（仅创建实例，不打开流）
    pub fn default_playback_device(&self) -> AudioDevice {
        AudioDevice::new(
            AudioDeviceID::Device(SDL_AUDIO_DEVICE_DEFAULT_PLAYBACK),
            self.inner.clone(),
        )
    }

    /// 获取默认录音设备实例（仅创建实例，不打开流）
    pub fn default_recording_device(&self) -> AudioDevice {
        AudioDevice::new(
            AudioDeviceID::Device(SDL_AUDIO_DEVICE_DEFAULT_RECORDING),
            self.inner.clone(),
        )
    }

    /// 按设备 ID 打开音频设备（内部通用实现）
    fn open_device(
        &self,
        device_id: SDL_AudioDeviceID,
        spec: &AudioSpec,
    ) -> Result<AudioDevice, Error> {
        let sdl_spec: SDL_AudioSpec = spec.clone().into();
        let dev_handle = unsafe { sys_audio::SDL_OpenAudioDevice(device_id, &sdl_spec) };

        if dev_handle == 0 {
            Err(get_error())
        } else {
            Ok(AudioDevice::new(AudioDeviceID::Device(dev_handle), self.inner.clone()))
        }
    }

    // -------------------------------------------------------------------------
    // 带回调的音频流（泛型回调）
    // -------------------------------------------------------------------------
    /// 在指定播放设备上创建带回调的音频流
    pub fn open_playback_stream_with_callback<CB, Channel>(
        &self,
        device: &AudioDevice,
        spec: &AudioSpec,
        callback: CB,
    ) -> Result<AudioStreamWithCallback<CB>, Error>
    where
        CB: AudioCallback<Channel>,
        Channel: AudioFormatNum + 'static,
    {
        device.open_playback_stream_with_callback(spec, callback)
    }

    /// 打开默认播放设备 + 带回调音频流
    pub fn open_playback_stream<CB, Channel>(
        &self,
        spec: &AudioSpec,
        callback: CB,
    ) -> Result<AudioStreamWithCallback<CB>, Error>
    where
        CB: AudioCallback<Channel>,
        Channel: AudioFormatNum + 'static,
    {
        let device = AudioDevice::open_playback(&self.inner, None, spec)?;
        device.open_playback_stream_with_callback(spec, callback)
    }

    /// 打开默认录音设备 + 带回调音频流
    pub fn open_recording_stream<CB, Channel>(
        &self,
        spec: &AudioSpec,
        callback: CB,
    ) -> Result<AudioStreamWithCallback<CB>, Error>
    where
        CB: AudioRecordingCallback<Channel>,
        Channel: AudioFormatNum + 'static,
    {
        let device = AudioDevice::open_recording(&self.inner, None, spec)?;
        device.open_recording_stream_with_callback(spec, callback)
    }

    // -------------------------------------------------------------------------
    // 驱动 & 设备名称查询
    // -------------------------------------------------------------------------
    /// 获取当前使用的音频驱动名
    pub fn current_audio_driver(&self) -> Result<&str, std::str::Utf8Error> {
        unsafe {
            let ptr = sys_audio::SDL_GetCurrentAudioDriver();
            debug_assert!(!ptr.is_null());
            CStr::from_ptr(ptr).to_str()
        }
    }

    /// 根据索引获取播放设备名称
    pub fn audio_playback_device_name(&self, index: u32) -> Result<String, Error> {
        self.inner.audio_playback_device_name(index)
    }

    /// 根据索引获取录音设备名称
    pub fn audio_recording_device_name(&self, index: u32) -> Result<String, Error> {
        self.inner.audio_recording_device_name(index)
    }

    // -------------------------------------------------------------------------
    // 音频格式转换流 AudioStream
    // -------------------------------------------------------------------------

    /// 创建播放专用音频转换流
    pub fn new_playback_stream(
        &self,
        app_spec: &AudioSpec,
        device_spec: Option<&AudioSpec>,
    ) -> Result<AudioStreamOwner, Error> {
        self.inner.new_playback_stream(app_spec, device_spec)
    }

    /// 创建录音专用音频转换流
    pub fn new_recording_stream(
        &self,
        device_spec: Option<&AudioSpec>,
        app_spec: &AudioSpec,
    ) -> Result<AudioStreamOwner, Error> {
        self.inner.new_recording_stream(device_spec, app_spec)
    }

}