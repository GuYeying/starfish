//! Windows Media Foundation 后端（SourceReader：解复用+解码一体）
//!
//! - 打开：`MFCreateSourceReaderFromURL`（内置 MP4 解复用 + H.264 解码器枚举：
//!   硬件 MFT 优先，OS 内置软解兜底——"OS 媒体栈"语义）
//! - 输出：NV12（系统内存）
//! - 音轨：v1 未接入（mixer 流声部 API 待定项 7）

use std::time::Duration;

use windows::core::Result as WResult;
use windows::Win32::Media::MediaFoundation as mf;

use super::{DecodedFrame, DecodeBackend, FramePixels, Poll, VideoError};

/// 视频流索引常量（强类型 i32 → u32）
const VIDEO_STREAM: u32 = mf::MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// MF 平台生命周期（Startup/Shutdown 配对；MF 内部引用计数，重复 Startup 安全）
struct MfGuard;

impl MfGuard {
    fn new() -> WResult<Self> {
        unsafe {
            // COM 初始化（幂等：已初始化返回 S_FALSE，放行）
            let hr = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
            let _ = hr;
            mf::MFStartup(mf::MF_VERSION, mf::MFSTARTUP_NOSOCKET)?;
        }
        Ok(Self)
    }
}

impl Drop for MfGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = mf::MFShutdown();
        }
    }
}

/// MF SourceReader 解码器（NV12 系统内存输出）
pub(crate) struct MfReader {
    reader: mf::IMFSourceReader,
    width: u32,
    height: u32,
    stride: usize,
    position: Duration,
    _guard: MfGuard,
}

/// 硬解唯一策略前置校验：探测 GPU 是否支持 H.264 VLD 硬解（DXVA）。
///
/// 注意 Windows 现实：标准硬解路径是微软 H.264 MFT + DXVA（注册为软件 MFT、
/// 解码实际跑在 GPU 上），`MFT_ENUM_FLAG_HARDWARE`（驱动自带 MFT）反而枚举不到
/// 大多数机器的硬解能力。故以 D3D11 VideoDevice 的解码配置探测为准——
/// 无硬件解码配置时返回 false，调用方直接报 [`VideoError::NoHardwareDecoder`]。
pub(crate) fn hardware_h264_available() -> bool {
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_11_1,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_CREATE_DEVICE_FLAG,
        D3D11_VIDEO_DECODER_DESC, ID3D11Device, ID3D11VideoDevice,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

    unsafe {
        // 1. 建支持视频的 D3D11 硬件设备（特性级别自高向低兜底）
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext> = None;
        let mut created = false;
        for level in [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1] {
            if D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(D3D11_CREATE_DEVICE_VIDEO_SUPPORT.0),
                Some(&[level]),
                windows::Win32::Graphics::Direct3D11::D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .is_ok()
            {
                created = true;
                break;
            }
        }
        if !created {
            #[cfg(test)]
            eprintln!("[hw-probe] D3D11CreateDevice 失败");
            return false;
        }
        let Some(device) = device else {
            return false;
        };
        let Ok(video_device) = device.cast::<ID3D11VideoDevice>() else {
            #[cfg(test)]
            eprintln!("[hw-probe] cast ID3D11VideoDevice 失败");
            return false;
        };

        // 2. 驱动是否为 H264 暴露解码能力。各厂驱动注册的 profile 命名不一
        //    （A~F 系列 / VLD_NoFGT），遍历常用集合任一可用即视为支持。
        //    （windows 0.62 漏列 DXVA_ModeH264_VLD_NoFGT 常量，按 dxva.h 定义值补上）
        let profiles = [
            windows::Win32::Media::DirectShow::DXVA_ModeH264_E,
            windows::Win32::Media::DirectShow::DXVA_ModeH264_F,
            windows::Win32::Media::DirectShow::DXVA_ModeH264_D,
            windows::core::GUID::from_u128(0x1b81be6b_a0c7_11d3_b984_00c04f2e73c5),
        ];
        for profile in profiles {
            let desc = D3D11_VIDEO_DECODER_DESC {
                Guid: profile,
                SampleWidth: 1920,
                SampleHeight: 1080,
                OutputFormat: DXGI_FORMAT_NV12,
            };
            let has_config = video_device
                .GetVideoDecoderConfigCount(&desc)
                .map(|c| c > 0)
                .unwrap_or(false);
            if has_config {
                return true;
            }
            if video_device
                .CheckVideoDecoderFormat(&profile, DXGI_FORMAT_NV12)
                .map(|v| v.as_bool())
                .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }
}

impl MfReader {
    pub(crate) fn open(path: &str) -> WResult<Self> {
        let guard = MfGuard::new()?;
        let url = windows::core::HSTRING::from(path);

        let reader: mf::IMFSourceReader = unsafe {
            mf::MFCreateSourceReaderFromURL(&url, None)?
        };

        // 选通视频流，输出重定向为 NV12（SourceReader 自动插入解码器：
        // 硬件 MFT 优先，OS 内置软解兜底）
        unsafe {
            reader.SetStreamSelection(VIDEO_STREAM, true)?;
            let out_type = mf::MFCreateMediaType()?;
            out_type.SetGUID(&mf::MF_MT_MAJOR_TYPE, &mf::MFMediaType_Video)?;
            out_type.SetGUID(&mf::MF_MT_SUBTYPE, &mf::MFVideoFormat_NV12)?;
            reader.SetCurrentMediaType(VIDEO_STREAM, None, &out_type)?;
        }

        // 帧尺寸与行距（从实际输出类型读，含对齐）
        let current: mf::IMFMediaType =
            unsafe { reader.GetCurrentMediaType(VIDEO_STREAM)? };
        let packed = unsafe { current.GetUINT64(&mf::MF_MT_FRAME_SIZE)? };
        let width = (packed >> 32) as u32;
        let height = packed as u32;
        let stride = unsafe { current.GetUINT32(&mf::MF_MT_DEFAULT_STRIDE) }
            .unwrap_or(width) as usize;

        Ok(Self {
            reader,
            width,
            height,
            stride,
            position: Duration::ZERO,
            _guard: guard,
        })
    }

    /// 解码下一帧（NV12）；`None` = 流结束
    fn read_sample(&mut self) -> WResult<Option<DecodedFrame>> {
        unsafe {
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample: Option<mf::IMFSample> = None;
            self.reader.ReadSample(
                VIDEO_STREAM,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )?;

            // EOS 且无样本 → 流结束
            if flags & (mf::MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 && sample.is_none() {
                return Ok(None);
            }
            let Some(sample) = sample else {
                return Ok(None);
            };

            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut ptr: *mut u8 = std::ptr::null_mut();
            let mut cur = 0u32;
            buffer.Lock(&mut ptr, None, Some(&mut cur))?;
            // 拷出后立即解锁（MF 缓冲区要求成对调用）
            let nv12 = std::slice::from_raw_parts(ptr, cur as usize).to_vec();
            buffer.Unlock()?;

            // 时间戳：100ns 单位 → Duration
            let pts = Duration::from_nanos((ts as u64).saturating_mul(100));
            Ok(Some(DecodedFrame {
                pixels: FramePixels::Nv12 {
                    nv12,
                    stride: self.stride as usize,
                },
                width: self.width,
                height: self.height,
                pts,
            }))
        }
    }
}

impl DecodeBackend for MfReader {
    fn poll_frame(&mut self) -> Result<Poll, VideoError> {
        let frame = self.read_sample().map_err(|e| VideoError::Backend(e.to_string()))?;
        Ok(match frame {
            Some(f) => {
                self.position = f.pts;
                Poll::Frame(f)
            }
            None => Poll::Eos,
        })
    }

    fn position(&self) -> Duration {
        self.position
    }
}

#[cfg(test)]
mod tests {
    /// 机器相关诊断（不 assert）：探测本机 GPU H264 硬解能力
    #[test]
    fn probe_hw_available() {
        let ok = super::hardware_h264_available();
        eprintln!("[hw-probe] hardware_h264_available = {ok}");
    }
}
