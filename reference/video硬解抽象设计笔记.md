# video 硬解抽象设计笔记（base/video 开工依据）

> 状态：**P1 已实现**——Windows MF 后端（SourceReader 解复用+解码一体，NV12
> 系统内存输出）+ YUV→RGBA 纹理管线 + 状态机/遮挡开关，46 测试全绿（含 5 个
> YUV 转换单测）。待实现：macOS/Linux/Web 后端（§六 矩阵 P4/P5）、音轨注入
> （待 mixer 流声部 API，§九-7）、MF 编码往返集成测试（§九-新增）。
> 覆盖：格式选型、平台栈拆解、接口模型（管理器+句柄）、双容量模型、非侵入泵、
> 音频汇注入、遮挡模式、分阶段实施。
>
> **修订（2026-09-08 晚）**：音轨注入方案简化定稿——**不引入组/容量/预留协议**。
> 视频音轨 = mixer 的**普通流声部**（music 同款机制），mixer 侧零改动：
> `Video.set_audio_sink(mixer)` 注入后，解码 PCM 直接喂该流声部；
> 不注入（默认）= 纯画面，demux 直接跳过音轨（隐式省解码）。
> 组/容量/预留协议（§4.2/4.3 原 §4.3 版本）作废——并发由用户自律
> （原型规模），超载保护若将来需要，加"同时解码数计数器"（非队列）。
> video 模块级音量 `set_volume` 管自身音轨（替代原"组音量"用例）。

---

## 一、定位与原则

- **base/video** = 平台硬解抽象层：容器解复用（跨平台一份纯 Rust）+ 解码（平台差异，
  收敛于 backend trait）+ 帧纹理上传。
- **输出形态**：按需产出帧纹理的数据服务——与渲染管线、音频时钟对接；
  与音频的对称与不对称：音频是连续采样流入混音器，视频是离散帧序列变纹理。
- **非侵入式**：不注册任何每帧回调，全部工作发生在用户显式调用的
  `update(dt)` 里（与音频 `pump_streams(budget)` 同构的"手动泵"哲学）。
- **控制模型 = audio 模式**（而非 font/gfx 模式）：视频数据具有连续性（流式、
  有时钟、需缓冲），与音频流同构——故采用"管理器 + 句柄"而非"init-once 离散服务"。

## 二、格式选型：H.264 + MP4（定稿）

| 平台 | 硬解 API | H.264 | VP9 | AV1 | HEVC |
|---|---|---|---|---|---|
| Windows | Media Foundation | ✓ 普遍 | ✓ 多数 | GPU 依存 | 需扩展购买 |
| macOS/iOS | VideoToolbox | ✓ 普遍 | 新系统 | M3+ | ✓（Apple 芯片） |
| Linux | VAAPI/NVDEC/Vulkan Video | ✓ 普遍 | 尚可 | 新 GPU | 稀烂 |
| Android | MediaCodec | ✓ 普遍 | ✓ | 新机 | 稀烂 |
| Web | WebCodecs / `<video>` | ✓ 每浏览器必有 | ✓ | 依浏览器 | 混乱 |

- **H.264（AVC）Baseline 是唯一全平台硬解的格式**；容器 MP4 解复用有纯 Rust
  实现（`mp4` crate）。MVP = H.264 Baseline + MP4。
- 专利注记：H.264 有专利池——解码器分发实践先例充分（浏览器/OS/引擎全内置）；
  在意则 VP9（免版税）备选，硬件覆盖低一档（尤其 macOS）。
- 软解兜底（可选，v1 不做）：OpenH264（BSD + 思科预编译二进制）。

## 三、Linux 栈拆解（为什么"无系统 API"）

Windows/macOS：OS 厂商随系统发布统一媒体框架（MF / VideoToolbox），
API 存在性与硬件覆盖由厂商保证。Linux 无 OS 厂商，分层如下：

```text
内核层   V4L2 —— 内核唯一视频 API，主要面向采集；
         其 M2M 解码接口是【嵌入式 Linux】（Rockchip/i.MX 等 SoC）硬解正路，
         桌面显卡不走此路
桌面层   VA-API（libva）—— 事实标准但非系统 API：
         Intel 发起、Mesa 实现（覆盖 Intel/AMD）；
         NVIDIA 走第三方混合驱动（质量参差）；发行版通常带但无保证
厂商私有 NVDEC（NVIDIA）—— CUDA 私有库
新兴统一  Vulkan Video（VK_KHR_video_decode_h264/vp9/av1）
         —— Mesa（RADV/ANV）与 NVIDIA 都在支持，【统一未来】；
            但 wgpu 不暴露 Vulkan Video（需 ash 原生 Vulkan 与 wgpu 共存）
```

**准确表述**：Linux 上不存在"随系统保证存在、且统一覆盖所有 GPU"的解码 API——
选 API = 选硬件阵营（VA-API/NVDEC/Vulkan Video/V4L2 各一套初始化、表面管理、
色彩空间）。FFmpeg/GStreamer 存在的全部理由就是封装这些碎片（Chrome 亦然）。

**Linux 后端决策**：v1 软解兜底（OpenH264，BSD + 思科预编译二进制）或
FFmpeg 动态链接（LGPL）；硬解（VA-API/NVDEC 直连）v2 评估；Vulkan Video 远期观察。

## 四、接口模型：管理器 + 句柄 + 双容量

### 4.1 生命周期与状态机

```text
pygame.video.init()                    → VideoModule（管理器）创建，依赖 render
pygame.video.set_group(group)          → 绑定音频组（音频汇，见 §5）
pygame.video.set_max_videos(n)         → 视频并发总上限

vid = pygame.video.open(path, channel=None | ch)
   → 探测文件头（流清单/时长/有无音轨）；channel=None = 纯画面
   → 带通道：从组【原子预留】空闲通道，占用至视频结束
vid.set_video_enabled(bool)            → 视频解码开关（遮挡场景，运行时可切）
vid.play() / pause() / seek(t)         → 联合控制：画面 + 注入音轨
pygame.video.update(dt)                → 手动泵：推进所有活跃句柄
vid.close()                            → 通道归还组 + 资源注销
```

状态机：`Opened → Ready →(play)→ Playing ⇄ Paused →(Ended/close)→ 通道归还`

### 4.2 双容量模型（定稿）

| 容量 | 含义 | 管理者 |
|---|---|---|
| **视频并发上限** `set_max_videos(n)` | 全部视频（含纯画面）的解码/纹理预算 | 引擎侧 |
| **音频组通道数** `create_group(channels)` | **带音频视频**的并发上限 | mixer 侧 |

- 两容量**不对等是特性**：n=8 + 组通道 4 → 4 带声 + 4 纯画面，合法且可预期；
- 对应两个正交资源：解码/纹理预算（引擎 CPU/GPU）vs 混音预算（音频 CPU + 用户意图）；
- 对齐 audio：video 上限可设置（`set_max_videos`），audio 组容量可设置
  （`create_group(channels)`）——对称。

### 4.3 通道预留与归还（定稿）

- **预留时机 = open()**（声明即确定性，符合"选择阶段检测"初衷）；
  占用语义：从 open 起持续占用至 Ended/stop/close——**注意**：加载未播的视频
  也占通道（确定性优先于利用率，文档明示）；
- **归还是唯一的释放途径**：Ended / stop() / close()；
- **pause / seek 保持占用**（无缝恢复）；
- **组满 → open 抛 `PlaybackLimitReached`**（结构化错误），用户三选一：
  卸载其他视频重试 / 换 `channel=None` 纯画面 / 等待；
- 原子性：通道预留由绑定层在 open 内部完成（不做 get/open 两步，
  消除竞态窗口）；`group.free_count()` 提供预判查询；
- **通道归还不参与 sfx 窃取策略**：视频音频中断是灾难级体验，
  流声部（music/video 同类）不参与 sfx 的活跃预算窃取。

### 4.4 控制面（联合控制 + 遮挡模式）

```python
vid.set_video_enabled(False)   # 遮挡模式：只解音频（视频包丢弃，纹理冻结在最后一帧）
vid.set_video_enabled(True)    # 恢复：seek 至音轨位置（最近关键帧）→ 快速解码追帧
vid.play() / pause() / seek(t) # 联合控制：画面状态机 + 注入音轨（mixer 流暂停/恢复）
vid.set_volume(0.8)            # 视频级音量（作用于注入音轨）
```

- 音频开关（set_audio 注入与否）与视频开关（set_video_enabled）**正交**：
  完整 / 纯画面（无音频汇，demux 跳音轨）/ **遮挡**（音频开+视频关）/
  冻结（罕见）——四组合全合法，泵循环单一（包级分支）。
- **恢复对齐机制**：MP4 moov 表含 sync sample 表（随机访问现成）→
  查表定位 ≤ 主时钟位置的最近关键帧 → 连续解码【追帧】（不上屏）→
  追上后恢复。mp4 crate API 覆盖面实现期核实；兜底 = 打开时线性建关键帧索引。
- 引擎配套（可选增强）：`WindowEvent::Occluded` 事件翻译（winit 现成），
  供"引擎自动降级"场景；v1 先显式 API。

### 4.5 非侵入泵（定稿）

```rust
fn update(&mut self, dt: Duration) -> Result<()> {
    // ① 主时钟：有注入音轨 → 音轨位置；无 → 累计 dt
    // ② 泵循环（所有活跃句柄）：
    //    match demux.next_packet() {
    //        Audio(p) => 喂 mixer 流声部,
    //        Video(p) => if video_enabled { 解码+上传 } else { 丢弃 },
    //        Eof => break,
    //    }
    // ③ 不调 update = 全部视频冻结（非侵入式定义）
}
```

### 4.6 呈现：video 自带管线，gfx 零感知（定稿）

```rust
impl VideoHandle {
    pub fn update(&mut self, dt: Duration) -> Result<()>;    // 泵
    pub fn draw(&self, ctx: &mut Ctx, rect: Rect) -> Result<()>;  // 自绘（私有 YUV→RGB 管线）
    pub fn texture(&self) -> TextureHandle;                  // 进阶：RGB 纹理供任意管线采样
}
```

- **gfx 不认识视频**：YUV→RGB 专用采样管线归 video 模块私有，
  render 模块只是基底（设备/命令编码器）；
- 纹理是 gfx 的通用货币——video 是纹理的生产者，`texture()` 供进阶场景
  把视频当普通纹理采样。

## 五、解码与渲染数据流（MVP 路线）

```text
MP4（mp4 crate 解复用，纯 Rust 跨平台）
 ├─ 视频轨 → H264Stream(AnnexB) → 平台硬解 → VideoFrame（YUV 三平面，系统内存）
 │                                    ↓ queue.write_texture（三平面直传）
 │              三张纹理 → 采样着色器 YUV→RGB（零 CPU 色彩转换）
 └─ 音频轨(AAC) → symphonia（isomp4+aac 特性，需核实）→ AudioMixer 流声部（主时钟）
```

两个 MVP 省力决策：

1. **硬解输出系统内存，不走零拷贝**：MF/VideoToolbox/VAAPI 均支持系统内存输出；
   `write_texture` 上传 YUV 三平面。放弃显存零拷贝（那需要 per-backend 的
   hal 互操作：DXGI/CVPixelBuffer/VA-Surface 各一套 unsafe）——v2 优化。
2. **YUV 直传 + 着色器转换**：三张 R8 纹理 + 采样着色器一次矩阵乘——
   零 CPU 色彩转换成本，视频纹理的标准技巧。

## 六、平台后端矩阵与实施阶段

| 平台 | 后端 | 阶段 |
|---|---|---|
| Windows | Media Foundation（MFT 同步模式，`windows` crate） | **P1（MVP 先行，开发平台）** |
| Web | `<video>` 元素 + `texImage2D`（浏览器全包解复用+解码；与 native 管线形状不同，抽象需容纳） | P3 |
| macOS | VideoToolbox（`objc2` + `objc2-video-toolbox`） | P4 |
| Linux | FFmpeg 动态链接（LGPL）/ GStreamer / 软解兜底 | P5（最后） |

抽象容纳 Web 的要点：Web 的"解码器"= `<video>` 元素（浏览器解复用+解码全包），
帧获取 = `texImage2D` 直接采样视频元素——输出形状仍是"帧纹理"，trait 层面
与其他后端同构。

## 七、许可注记

- **H.264**：有专利池（解码侧）；实践先例充分（浏览器/OS/引擎全内置）；
- **MP4 容器**：无问题；
- **FFmpeg**：LGPL，动态链接合规需注意；
- **OpenH264**：BSD + 思科预编译二进制（专利风险规避的业界方案）。

## 八、Python 侧最终形态（定稿脚本）

```python
import pygame

pygame.init()
pygame.mixer.init()
pygame.render.init()
pygame.font.init()
pygame.gfx.init()
pygame.video.init()                              # 管理器（依赖 render）
screen = pygame.display.set_mode((1280, 720))
pygame.init_finish()

vid_group = pygame.mixer.create_group("video", channels=4)   # 音频组：带音频视频上限
pygame.video.set_group(vid_group)                # 绑定音频组
pygame.video.set_max_videos(8)                   # 视频并发总上限（可 ≠ 组容量）

vid_a = pygame.video.open("cutscene_a.mp4")      # 组内原子预留通道 → 带音频
vid_b = pygame.video.open("cutscene_b.mp4",
                          channel=None)          # 显式纯画面（不占组通道）

def game(ctx):
    pygame.video.update(ctx.dt)                  # 手动泵：推进全部句柄（非侵入）
    vid_a.draw(ctx, (0, 0, 1280, 720))
    ctx.flip()
    yield

pygame.run(Game)                                 # 类钩子形态同构（load 排水 / update 泵）
```

## 九、待定项（实现期决策）

1. `mp4` crate 的 sync sample 随机访问 API 核实（关键帧 seek 依赖）
2. symphonia `isomp4` + `aac` 特性核实（音频轨走 symphonia 的可行性）
3. Windows MF 同步解码的具体 API 路径（MFT 同步模式 vs 异步 MFT）
4. Linux 后端终选（FFmpeg LGPL / GStreamer / OpenH264 软解）
5. `texture()` 的 RGB 纹理格式与生命周期（每次上传新纹理 vs 纹理池复用）
6. 多视频同时解码的 CPU 尖峰（Web 主线程）——并发上限计数器后手
7. **mixer 组机制扩展**（本设计的 audio 侧配套）：`create_group` 增加通道容量
   参数；组支持**流声部成员**（music/video 同类，此前组员预设为 sfx 短声部）；
   新增**预留/归还协议**（流声部长占用，不参与 sfx 窃取策略）
