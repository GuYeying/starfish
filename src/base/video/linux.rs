//! Linux 视频硬解后端（GStreamer 硬解聚合层）
//!
//! VAAPI/NVDEC/VDPAU 驱动生态无统一原生入口，GStreamer 是 Ubuntu 上最通用友好的
//! 硬解聚合层。本后端**只挑硬件解码器**（工厂 klass 含 `Hardware`），按 rank 降序
//! 逐个尝试建管线，全部失败（含无插件、显存输出不可下载等协商失败）→
//! [`VideoError::NoHardwareDecoder`]——不落任何软解兜底。
//!
//! 管线：`filesrc ! qtdemux ! h264parse ! <硬解器> ! videoconvert ! capsfilter(NV12) ! appsink`
//!
//! - appsink `sync=false` + `max-buffers=2`：拉取式解码，内存有界（最多领先消费 2 帧）
//! - 拉取语义与 Windows MF 同步 `ReadSample` 同契约：`pull_sample` 阻塞 ≈ 解码一帧耗时
//! - 输出统一 NV12 系统内存，色度平面几何由缓冲长度反推（与 MF 路径同一教训：
//!   不假设 `stride × height`，1088 对齐等布局由 `yuv::nv12_to_rgba` 自适应）

use std::time::Duration;

use gst::prelude::*;

use super::{DecodedFrame, DecodeBackend, FramePixels, Poll, VideoError};

/// 逐候选硬解器建管线并推到 PLAYING（含协商），失败返回 None 由调用方换下一个
fn try_build_pipeline(hw_factory: &gst::ElementFactory, path: &str) -> Option<GstPipeline> {
    gst::init().ok()?;

    let make = |factory: &str, err: &str| -> Option<gst::Element> {
        gst::ElementFactory::make(factory)
            .build()
            .map_err(|_| err.to_string())
            .ok()
    };

    let filesrc = make("filesrc", "缺少 gst-plugins-base/core（filesrc）")?;
    filesrc.set_property("location", path);

    // qtdemux：MP4 解复用（gst-plugins-good）
    let qtdemux = make("qtdemux", "缺少 gst-plugins-good（qtdemux）")?;
    // h264parse：裸流整理（gst-plugins-bad）
    let h264parse = make("h264parse", "缺少 gst-plugins-bad（h264parse）")?;
    let hwdec = hw_factory.create().build().ok()?;
    let videoconvert = make("videoconvert", "缺少 gst-plugins-base（videoconvert）")?;

    let caps = gst::Caps::new_simple("video/x-raw", &[("format", &"NV12")]);
    let appsink_el = gst::ElementFactory::make("appsink")
        .property("caps", caps)
        .property("sync", false)
        .property("max-buffers", 2u32)
        .property("drop", false)
        .build()
        .ok()?;
    let appsink = appsink_el
        .dynamic_cast::<gst_app::AppSink>()
        .ok()?;

    let pipeline = gst::Pipeline::new();
    for el in [&filesrc, &qtdemux, &h264parse, &hwdec, &videoconvert] {
        pipeline.add(el).ok()?;
    }
    pipeline.add(appsink_el.upcast_ref()).ok()?;

    // 静态链：filesrc → qtdemux；h264parse → 硬解器 → videoconvert → appsink
    filesrc.link(&qtdemux).ok()?;
    h264parse.link(&hwdec).ok()?;
    hwdec.link(&videoconvert).ok()?;
    videoconvert
        .link(&appsink.upcast_ref::<gst::Element>())
        .ok()?;

    // qtdemux 动态 pad：只接视频轨（音频轨保持未链接，数据自然丢弃）
    {
        let parse = h264parse.clone();
        qtdemux.connect_pad_added(move |_src, pad| {
            if pad.name().starts_with("video") {
                let sink = parse.static_pad("sink").unwrap();
                let _ = pad.link(&sink);
            }
        });
    }

    let mut pl = GstPipeline {
        pipeline,
        appsink,
        _hwdec: hwdec,
    };

    // 推到 PLAYING：协商失败/无硬解可用会在此处表现为状态切换失败，
    // 拉取语义要求先等 preroll 完成（首帧就位）再返回
    pl.pipeline
        .set_state(gst::State::Playing)
        .ok()?;
    let (result, _, _) = pl.pipeline.get_state(gst::ClockTime::from_seconds(5));
    if result != gst::StateChangeSuccess::Success {
        return None;
    }
    Some(pl)
}

struct GstPipeline {
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    /// 持有硬解器引用（管线内已 add，此字段只为错误信息可读性）
    _hwdec: gst::Element,
}

impl Drop for GstPipeline {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

/// GStreamer 硬解读取器（NV12 系统内存输出）
pub(crate) struct GstReader {
    pipe: GstPipeline,
    position: Duration,
}

impl GstReader {
    pub(crate) fn open(path: &str) -> Result<Self, VideoError> {
        gst::init().map_err(|e| VideoError::Backend(e.to_string()))?;

        // 硬解器发现：视频解码器工厂 ∩ Hardware 类 ∩ 可吃 H264，rank 降序
        // （factories_with_type 已按 rank 降序返回）
        let h264_caps = gst::Caps::new_simple("video/x-h264", &[]);
        let hw_decoders: Vec<_> = gst::ElementFactory::factories_with_type(
            gst::ElementFactoryType::VIDEO_DECODER,
            gst::Rank::MARGINAL,
        )
        .filter(|f| f.has_type(gst::ElementFactoryType::HARDWARE))
        .filter(|f| f.can_sink_any_caps(h264_caps.as_ref()))
        .collect();

        if hw_decoders.is_empty() {
            return Err(VideoError::NoHardwareDecoder);
        }

        // 逐候选尝试：协商失败（如显存输出无法落到系统内存）自动换下一个
        let mut last: Option<VideoError> = None;
        for factory in hw_decoders {
            match try_build_pipeline(&factory, path) {
                Some(pipe) => {
                    return Ok(Self {
                        pipe,
                        position: Duration::ZERO,
                    });
                }
                None => {
                    last = Some(VideoError::Backend(format!(
                        "硬解器 {} 建管线失败",
                        factory.name()
                    )));
                }
            }
        }
        Err(last.unwrap_or(VideoError::NoHardwareDecoder))
    }

    /// 阻塞拉取下一帧；检查总线错误避免死等
    fn read_sample(&mut self) -> Result<Option<DecodedFrame>, VideoError> {
        // 上游错误/EOS 非阻塞窥探（无 MainLoop，轮询式）
        let bus = self.pipe.pipeline.bus().unwrap();
        while let Some(msg) = bus.timed_pop_filtered(
            gst::ClockTime::ZERO,
            &[gst::MessageType::Error, gst::MessageType::Eos],
        ) {
            match msg.view() {
                gst::MessageView::Error(e) => {
                    return Err(VideoError::Backend(format!(
                        "{}: {}（debug: {}）",
                        e.src()
                            .map(|s| s.name())
                            .unwrap_or_default(),
                        e.error(),
                        e.debug().unwrap_or_default()
                    )));
                }
                gst::MessageView::Eos(..) => return Ok(None),
                _ => unreachable!(),
            }
        }

        let sample = match self.pipe.appsink.pull_sample() {
            Ok(s) => s,
            Err(e) => {
                // 拉取失败 + EOS 置位 = 正常流结束；否则为管线异常
                let eos: bool = self.pipe.appsink.property("eos");
                if eos {
                    return Ok(None);
                }
                return Err(VideoError::Backend(e.to_string()));
            }
        };

        let caps = sample
            .caps()
            .ok_or_else(|| VideoError::Backend("样本缺少 caps".into()))?;
        let info = gstreamer_video::VideoInfo::from_caps(caps)
            .map_err(|e| VideoError::Backend(format!("NV12 caps 解析失败: {e}")))?;
        let buffer = sample
            .buffer()
            .ok_or_else(|| VideoError::Backend("样本缺少缓冲".into()))?;
        let map = buffer
            .map_readable()
            .map_err(|e| VideoError::Backend(format!("缓冲映射失败: {e}")))?;

        let pts = sample
            .pts()
            .map(|t| Duration::from_nanos(t.nseconds()))
            .unwrap_or(self.position);

        Ok(Some(DecodedFrame {
            pixels: FramePixels::Nv12 {
                nv12: map.as_slice().to_vec(),
                stride: info.stride()[0] as usize,
            },
            width: info.width(),
            height: info.height(),
            pts,
        }))
    }
}

impl DecodeBackend for GstReader {
    fn poll_frame(&mut self) -> Result<Poll, VideoError> {
        let frame = self.read_sample()?;
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
