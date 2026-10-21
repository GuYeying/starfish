# video 模块跨平台架构笔记

> 本文面向后续维护者：完整盘点 `src/base/video/` 各模块的具体类型、运转流程，
> 以及"各平台运作流程不同，如何做到高度兼容"的设计方法。
> 决策链与实测细节见 `doc/log/starfish_changelog_2026-09-11.md` 批次 1–5。

---

## 一、模块地图与类型清单

```
src/base/video/
├── mod.rs        组装层：错误 / 帧类型 / 平台 seam trait / 入口 + cfg 分发
├── player.rs     播放状态机 Video（平台中立，应用唯一接触面）
├── yuv.rs        NV12 → RGBA 整数定点转换（纯函数 + 色值锚点测试）
├── mp4_demux.rs  MP4 解复用共享层（android / web 后端共用；test 进 Windows 构建）
├── windows.rs    Windows 后端（Media Foundation + D3D11 硬解探测）
├── linux.rs      Linux(Ubuntu) 后端（GStreamer 硬解聚合）
├── apple.rs      macOS / iOS 后端（AVAssetReader + VideoToolbox）
├── web.rs        Web 后端（WebCodecs + fetch，自持 js_sys 动态绑定）
└── android.rs    Android 后端（MediaCodec via JNI）
```

### 1. mod.rs —— 组装层

| 类型 | 形态 | 说明 |
|---|---|---|
| `VideoError` | 枚举 | `UnsupportedPlatform` / `NoHardwareDecoder`（硬解唯一策略）/ `Backend(String)` |
| `DecodedFrame` | 结构体 | `{ width, height, pts, pixels }`——平台后端的统一帧输出 |
| `FramePixels` | 枚举 | `Nv12 { nv12, stride }`（桌面）\| `VideoFrame(WasmVideoFrame)`（仅 wasm） |
| `Poll` | 枚举 | `Frame(DecodedFrame)` / `Pending`（帧在途）/ `Eos` |
| `DecodeBackend` | trait（pub(crate)） | `poll_frame()` + `is_ready()` + `position()`——**平台 seam，全部平台差异的收敛点** |
| `VideoModule` | 结构体 | 持 `device/queue`；`open(path) -> Result<Video, VideoError>` 唯一入口 + cfg 分发 |
| `WasmVideoFrame` | 包装（仅 wasm） | 包 `web_sys::VideoFrame`，**Drop 即 close()**，追帧丢弃及时释放 GPU 资源 |

### 2. player.rs —— 播放状态机（应用唯一接触面）

| 类型/方法 | 说明 |
|---|---|
| `Video { backend, device, queue, texture, view, size, clock, position, enabled, ended }` | 解码状态机 + 帧纹理管理 |
| `update(dt) -> Result<(), VideoError>` | **手动泵**：时钟推进、追帧、上传（下文 §三） |
| `texture() -> Option<&wgpu::Texture>` | 帧纹理（未解首帧为 None） |
| `texture_view() -> Option<Arc<wgpu::TextureView>>` | **直绑口**：配 `BindGroupBuilder::texture_view` 装配 bind group |
| `size() / position() / ended()` | 状态查询 |
| `set_video_enabled(bool) / video_enabled()` | 遮挡开关：关 = 时钟照走、解码停，恢复自动追帧 |
| （私有）`upload()` / `copy_frame_pixels()` | 帧像素 → 纹理（NV12 CPU 路径 / Web GPU 直拷路径） |

### 3. yuv.rs —— 色彩转换（纯函数）

| 项 | 说明 |
|---|---|
| `nv12_to_rgba(nv12, stride, width, height) -> Vec<u8>` | 整数 16.16 定点 BT.601（debug 构建敏感的热路径，比浮点快 ~3 倍）；4:2:0 水平减半；**色度平面起点由缓冲长度反推对齐高**（1088 对齐教训），不假设 `stride × height` |
| 测试 | 5 个色值锚点（黑/白/中灰/alpha/色度块共享）——定点化后语义不变的保证 |

### 4. mp4_demux.rs —— 解复用共享层（android / web 后端共用）

| 项 | 说明 |
|---|---|
| `Demuxer<R: Read + Seek>` | mp4 crate 封装：找 H.264 视频轨、样本迭代、SPS/PPS 提取 |
| `next_sample() -> (码流Annex-B, pts, 是否关键帧)` | 关键帧前置 SPS/PPS；`Ok(None)` = 流结束 |
| `sps_annexb() / pps_annexb()` | csd-0/csd-1 注入 MediaCodec 用 |
| `codec_string` | 从 SPS 提取 `avc1.{profile}{compat}{level}`，WebCodecs 配置用 |
| cfg | `any(android, wasm32, test)`——`test` 使 Windows 测试构建可用真实视频做集成测试 |

### 5. 平台后端（每个都实现 `DecodeBackend`）

| 文件 | 平台 | 后端类型 | 硬解框架 | 无硬解报错点 |
|---|---|---|---|---|
| `windows.rs` | Windows | `MfReader` | Media Foundation SourceReader（DXVA） | `hardware_h264_available()`：D3D11 VideoDevice 按 H264 profile 集合查 1080p NV12 解码配置 |
| `linux.rs` | Ubuntu | `GstReader` | GStreamer（VAAPI/NVDEC 聚合） | ElementFactory ∩ klass `Hardware` ∩ 可吃 `video/x-h264`，逐候选建管线全败即报 |
| `apple.rs` | macOS/iOS | `VtReader` | VideoToolbox | 规格字典 `RequireHardwareAcceleratedVideoDecoder`，会话创建失败即报 |
| `web.rs` | 浏览器 | `WebDecoder` | WebCodecs | `NotSupported` 类错误映射 `NoHardwareDecoder`（**平台偏差**：API 只允许 prefer 不允许强制，见 §五） |
| `android.rs` | Android | `MediaCodecReader` | MediaCodec | `MediaCodecList` 过滤 `isHardwareAccelerated`（API<29 名称启发式），无候选即报 |

各后端内部的关键私有类型：`MfGuard`（COM/MF 生命周期）、`GstPipeline`（Drop→Null 态）、
`CallbackCtx`（VT 回调通道）、`Inner`（web 异步装配产物，经 `Rc<RefCell<Option<_>>>` 交回）、
`Demuxer`+`JavaVM`/`GlobalRef`（android 的 JNI 持有）。

---

## 二、创建流程（`VideoModule::open`）

```
应用 start()：
  VideoModule::new(device.clone(), queue.clone())     ← 与渲染共享同一 wgpu 设备
     └─ open("resources/videos/x.mp4")
          ├─ windows: hardware_h264_available()? ──否→ Err(NoHardwareDecoder)
          │            └─ MfSourceReader：选通视频流，输出重定向 NV12
          ├─ linux:   枚举硬件解码器工厂 → 逐个搭管线试协商（失败换下一个）
          │            └─ filesrc!qtdemux!h264parse!<硬解>!videoconvert!capsfilter(NV12)!appsink
          ├─ apple:   AVAssetReader(outputSettings=nil → 压缩样本) 就绪；
          │            VTDecompressionSession 惰性建（首个样本给格式描述时）
          ├─ web:     同步返回 WebDecoder 句柄；内部 spawn_local：
          │            fetch(URL) → bytes → Demuxer → VideoDecoder.configure(prefer-hardware)
          ├─ android: ndk_context 取 JavaVM → JNI 查硬件解码器 → MediaCodec
          │            configure(format + csd-0/csd-1) → start
          └─ 其余:    Err(UnsupportedPlatform)
          → Video::new(Box<dyn DecodeBackend>, device, queue)   ← 纹理此刻不存在（惰性）
```

注意 web 的特殊性：`open` 立即返回（无阻塞 IO），加载在后台进行——
这正是 `is_ready()` 存在的原因。

## 三、帧泵运转（每帧 `frame()` 调 `video.update(dt)`）

```
update(dt)：
  ① ended 或 !is_ready() → 直接返回（时钟冻结：web 加载期）
  ② clock += dt                      ← 主时钟：上层唯一事实，只由 dt 驱动
  ③ while enabled && position < clock:      ← 追帧循环
        poll_frame()
        ├─ Frame(f) → position = f.pts；丢弃旧 pending，只留最新
        ├─ Pending  → break（帧在途：时钟照走，下次 update 继续）
        └─ Eos      → ended = true; break
  ④ upload(pending)                  ← 仅泵的最后一帧上屏（追帧中间帧解码即弃）

upload()：
  纹理不存在或尺寸变化 → create_texture（wasm 追加 RENDER_ATTACHMENT usage）+ 建默认视图
  ┌─ 桌面 Nv12：yuv::nv12_to_rgba（CPU 定点转换）→ queue.write_texture 整帧覆写
  └─ web  VideoFrame：queue.copy_external_image_to_texture（GPU 直拷，浏览器完成
     YUV→RGB）→ 帧经 WasmVideoFrame::drop 自动 close()
```

**渲染侧**（example 13）：首帧纹理就绪后恰好一次装配
`bind_group_builder().texture_view(0, video.texture_view())` + 全屏三角形管线；
之后每帧只重复 draw。同尺寸覆写不重建纹理 → 视图稳定，"绑定一次管到底"。

---

## 四、跨平台兼容的核心方法

各平台运作流程**客观上不同**（同步/异步、CPU/GPU、阻塞/回调、容器/裸流）。
本模块的兼容策略不是"抹平成相同流程"，而是 **五层收敛**：

### 收敛 1：seam 极小化——平台差异只允许暴露两个问题

`DecodeBackend` 只回答：
- `poll_frame()`：**下一帧在哪？**（Frame / Pending / Eos）
- `is_ready()`：**现在能解吗？**

一切平台细节（MF 的同步 ReadSample、GStreamer 的流线程 + appsink、
VT 的回调线程、WebCodecs 的 JS 任务队列、MediaCodec 的缓冲轮询）都被压进
后端文件内部，以"同步拉取假象"的形式对上呈现。条件编译因此只出现在
mod.rs 分发一处 + 后端文件内部；player/yuv/example 对平台零感知。

### 收敛 2：时钟与节奏归一——上层是唯一事实

无论平台解码是快是慢、是阻塞还是异步：
- **主时钟 = 累计 dt**（上层驱动），平台 pts 只用于推进 `position`；
- 追帧 = `position < clock` 时快进：**中间帧解码即弃，只上传最新**——
  上传成本 O(追帧数)→O(1)，帧回调永不长阻塞（窗口无响应问题的教训）；
- Web 的异步性被 `Pending`/`is_ready` 吸收：帧没到 → 本泵提前收工但时钟照走，
  播放节奏仍由实时 dt 保证（丢帧不拖慢）。

### 收敛 3：像素归一——一个枚举臂，而非两套状态机

公共货币是 NV12 系统内存（桌面三平台完全一致）。Web 无法给出 NV12
（解码结果本来就是 GPU 帧，`copyTo` 异步不可逆）——差异被局部化为
`FramePixels` 枚举的一个 match 臂：

```
Nv12      → CPU 定点转换 → write_texture
VideoFrame → GPU 直拷（copy_external_image_to_texture）→ close
```

纹理创建/覆写/视图管理对两者完全一致。**差异只出现在"拷贝"这一步。**

### 收敛 4：几何不假设——一律实测

YUV 平面几何（行距对齐、高度对齐如 1080→1088、UV 平面起点）各解码器不同：

| 平台 | 几何来源 |
|---|---|
| Windows | MF `MF_MT_DEFAULT_STRIDE`；色度偏移由**缓冲长度反推对齐高** |
| Linux | `gst_video::VideoInfo`（stride/offset 实测） |
| macOS/iOS | CVPixelBuffer **plane** stride/基址逐平面实测 |
| Android | INFO_OUTPUT_FORMAT_CHANGED 的 `stride`（含 slice-height 语义） |

这是"顶部绿条"事故的方法论沉淀：**布局疑问题不假设，转储/读取实机实测值。**

### 收敛 5：能力探测统一报错——检测点不同，错误类型相同

"无硬解直接报错"在各平台的检测机制完全不同，但对外只有
`VideoError::NoHardwareDecoder` 一个出口：

| 平台 | 检测机制 |
|---|---|
| Windows | D3D11 `GetVideoDecoderConfigCount`（H264 profile 集合 × 1080p NV12）——注意 `MFT_ENUM_FLAG_HARDWARE` 枚举不到微软 DXVA 路径，不能用 |
| Linux | GStreamer 工厂 klass 含 `Hardware` + sink caps 交验 |
| macOS/iOS | VT 规格 `RequireHardwareAcceleratedVideoDecoder`（会话创建失败） |
| Android | `MediaCodecList` + `isHardwareAccelerated`（API<29 名称启发式） |
| Web | `NotSupported` 类错误（**平台上限偏差**：API 只有 prefer 系，浏览器可自行软解，无法强制） |

### 附：线程模型

全部平台统一"视频固定主线程"契约（文档 + `debug_assert`）：
平台内部线程（MF/MF 工作队列、GStreamer 流线程、VT 回调线程、WebCodecs
JS 任务、cpal 同款思路）被限制在后端内部，经 channel / appsink 队列 /
回调入队等桥接回主线程的拉取语义。后端结构体多为 `!Send`（COM 套间亲和），
架构上就禁止了跨线程误用。

---

## 五、已知偏差与边界（如实声明）

1. **Web 无法强制硬解**：`hardwareAcceleration` 仅 prefer 系；浏览器可静默软解。
   `NotSupported` 错误仍会映射 `NoHardwareDecoder`（连解码器都没有的场景）。
2. **Web 加载是全文件 fetch**：字节范围流式 + 渐进播放为后续优化。
3. **WebCodecs 绑定自持**（js_sys Reflect 动态调用）：web-sys 中该 API 处于
   unstable gate，为不加构建 flag 而为之；代价是无编译期类型保护。
4. **Android 运行依赖引擎侧安卓引导**（android-activity 注入 `ndk_context`）；
   引擎主线尚无安卓引导流程，后端就绪但需立项后才能端到端运行。
5. **Android 输出几何**：YUV420Flexible 实践即 NV12，但 crop/slice-height 细节
   因驱动而异，v1 按实测 stride + 紧凑布局处理，异常布局需实机修正。
6. **CFR 假设**：Web/Android 的 pts 直接采用样本/回调时间戳；变帧率流在
   追帧模型下按实时节奏播放，不产生额外问题。

## 六、扩展指南（新平台接入）

1. 新建 `平台名.rs`，实现 `DecodeBackend`（`poll_frame`/`is_ready`/`position`）；
2. 硬解唯一策略：无硬件解码器的检测点放在 `open`，返回 `NoHardwareDecoder`；
3. 输出对齐 `DecodedFrame`（优先 NV12；确需 GPU 帧则扩 `FramePixels` 臂）；
4. `mod.rs` 分发处加一个 cfg 分支 + 模块声明——**仅此一处条件编译**；
5. Cargo 增平台段依赖（注意 wasm 体积：纯 Rust 优先，系统库其次，静态嵌入回避）；
6. 跨目标 `cargo check --target <目标>` 类型验证 + 实机清单进日志。
