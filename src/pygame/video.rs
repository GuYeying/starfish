//! pygame.video 对应物（**starfish 扩展域**：真 pygame 无 video 模块——
//! 六平台硬解视频播放，包装 [`crate::base::video`] 的平台分派播放器）。
//!
//! 帧消费：[`Video::texture`] / [`Video::texture_view`]（当前帧纹理直通；
//! Surface 桥/BufferProxy 桥由绑定层定）。驱动模型：**每帧调 [`Video::update`]
//! 推进解码与上传**（dt = 帧间隔），随后正常 present。
//!
//! 音轨：[`VideoSystem::open`] 为纯视频；带音轨的 `open_with_audio` 需
//! mixer StreamVoice（经桥层开放，后续批次）。
//!
//! 硬解唯一策略：平台无硬件 H.264 解码器时 `open` 返回
//! `VideoError::NoHardwareDecoder`，不落软解兜底（批次 G 契约）。

use std::sync::Arc;
use std::time::Duration;

use crate::base::video::{Video as BaseVideo, VideoError};
use crate::pygame::render::gpu;

/// 视频系统（GPU 句柄持有人；`set_mode` 后可用）
pub struct VideoSystem {
    inner: crate::base::video::VideoModule,
}

impl VideoSystem {
    /// 从 pygame 渲染槽取 GPU 句柄构建（进程一次即可，可复用开多视频）
    pub fn new() -> Self {
        let gpu = gpu();
        Self {
            inner: crate::base::video::VideoModule::new(
                gpu.device.clone(),
                gpu.queue.clone(),
            ),
        }
    }

    /// 打开视频文件（惰性：首个 update 才解码首帧）
    pub fn open(&self, path: impl Into<String>) -> Result<Video, VideoError> {
        Ok(Video {
            inner: self.inner.open(path)?,
        })
    }
}

/// 视频播放器（硬解解码 + 帧纹理载体）
pub struct Video {
    inner: BaseVideo,
}

impl Video {
    /// 推进解码与帧上传（每帧调用；dt = 距上次 update 的时长）
    pub fn update(&mut self, dt: Duration) -> Result<(), VideoError> {
        self.inner.update(dt)
    }

    /// 当前帧纹理（未出首帧 = None）
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.inner.texture()
    }

    /// 当前帧纹理视图（可直接作渲染附件/采样源）
    pub fn texture_view(&self) -> Option<Arc<wgpu::TextureView>> {
        self.inner.texture_view()
    }

    /// 视频原生尺寸（宽, 高）
    pub fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// 播放位置
    pub fn position(&self) -> Duration {
        self.inner.position()
    }

    /// 是否播放结束
    pub fn ended(&self) -> bool {
        self.inner.ended()
    }

    /// 视频/解码开关（暂停 = 关）
    pub fn set_video_enabled(&mut self, enabled: bool) {
        self.inner.set_video_enabled(enabled);
    }

    /// 视频/解码开关查询
    pub fn video_enabled(&self) -> bool {
        self.inner.video_enabled()
    }

    /// 是否含音轨
    pub fn has_audio(&self) -> bool {
        self.inner.has_audio()
    }

    /// 音轨音量（0.0..1.0）
    pub fn set_audio_volume(&self, volume: f32) {
        self.inner.set_audio_volume(volume);
    }

    /// 静音开关
    pub fn set_muted(&self, muted: bool) {
        self.inner.set_muted(muted);
    }
}
