# Starfish 更新日志 2026-09-11

## 批次 1：示例 13 视频播放修复（bind group 缺口 + 解码阻塞定位 + 追帧上传纪律）

### 设计背景

示例 13（`examples/13_video_decode.rs`）处于半成品状态无法编译（变量作用域错乱、
API 签名过时）；修复编译后实机运行又出现**视频不播放 + 窗口无响应**。两个问题
分别定位为「库侧纹理接口缺口」与「debug 构建 CPU 色彩转换热路径」。

### 设计方案

**① 外部纹理源直绑口（`BindGroupBuilder::texture_view`）**

`Video` 持有裸 `wgpu::Texture`，而 `BindGroupBuilder::texture()` 只收库的
`Arc<Texture>`（构造函数全 `pub(crate)`），示例无论怎么写都接不进去。新增：

- `BindGroupBuilder::texture_view(binding, Arc<wgpu::TextureView>)`——已有
  `texture_view_array`（bindless）的单视图对应版，外部纹理源（视频帧等库外
  创建的纹理）的直绑口；
- `Video::texture_view() -> Option<Arc<wgpu::TextureView>>`——首帧建纹理时
  同步创建默认视图存储，随纹理同生命周期（重建即换新）。

**② 追帧上传纪律（`Video::update`）**

主时钟追帧循环原来对每个解码帧都做「CPU 转换 + 纹理上传」。定位实验表明
转换占单帧成本绝大部分，追帧连解多帧时帧回调被阻塞数百毫秒——消息泵停摆
→ Windows 判定窗口无响应。改为：泵内只保留最新解码帧，**中间帧解码即弃**
（跳过转换与上传），泵结束后仅上传最后一帧（含流结束前的末帧）。单次
`update` 的上传成本从 O(追帧数) 收敛到 O(1)。

**③ NV12→RGBA 整数定点化（`yuv::nv12_to_rgba`）**

定位实验（无窗口探针，1080p 源）：

| 构建 | 原浮点逐像素 | 整数定点后 |
|---|---|---|
| release | ~15-20 ms/帧 | ~5 ms/帧 |
| debug | **~100 ms/帧**（`round`/`clamp` 不内联） | ~35 ms/帧 |

浮点公式改写为 16.16 定点（系数 `round(coef×65536)`，移位前加半 LSB 舍入），
色值与浮点版 ±1；4:2:0 水平减半利用——相邻两像素共享一份 U/V。yuv 单元测试
锚点色值（黑/白/中灰/alpha/色度块共享）全部保持通过。

### 关键保证

- **窗口永不因视频解码失去响应**：单次 `update` 至多一次转换 + 一次上传；
  解码速度跟不上主时钟时自动进入丢帧追帧，播放节奏保持实时（丢帧不拖慢）。
- 定位方法论沉淀：**STA/MTA 排除实验**（COM 套间切换对耗时无影响，排除）；
  **debug/release 对照**锁定 CPU 转换热路径（MF 解码运行在 MF 自有线程，
  不受应用构建档位影响）。
- `Video::texture_view` 视图稳定性契约：同尺寸覆写路径不重建纹理，bind group
  装配一次管到底；仅尺寸变化时换新（调用方需重新装配）。

### 测试状态

- `cargo test --lib`：46 passed / 0 failed（含 yuv 5 个色值锚点测试）。
- 示例 13 debug/release 各冒烟 12s：无 panic、无解码失败、消息泵持续运行。
- 后续（v2 既定计划）：转换上移到采样着色器（Y/UV 双纹理直传），debug 下
  1080p 转换成本归零——`texture_view` 直绑口已为该方案铺路。

## 批次 2：NV12 色度平面偏移修复（顶部绿条）

### 设计背景

批次 1 修复后实机播放正常，但**画面顶部有一条绿带**。首帧布局转储定位：
解码器输出的 NV12 缓冲为 3,133,440 字节 = 1920 × **1088** × 1.5——Y 平面按
16 行对齐（1080 显示行 + 8 行填充），色度平面起点在 `stride × 1088`；而
`nv12_to_rgba` 按 `stride × 1080`（显示高）取色度，读到的是 Y 平面填充字节
（全 0）。NV12 中 U=V=0 经 BT.601 变换即强绿偏色 → 顶部 16 个显示行绿带，
且整幅色度垂直错位 8 个色度行（16 像素，肉眼不明显）。

### 设计方案

`nv12_to_rgba` 内部由缓冲长度反推对齐高：

```text
对齐高 = buf_len × 2 / (stride × 3)   （NV12 总长 = stride × 对齐高 × 3/2）
色度平面起点 = stride × 对齐高
```

- 缓冲恰好紧凑排布时（`buf_len = stride × h × 3/2`）公式精确退化为显示高
  h——既有行为与 5 个色值锚点测试零变化；
- 对齐高取偶 + 下限 h，保证切片索引安全；
- 无需新增 MF API（`MF_MT_MINIMUM_DISPLAY_APERTURE` 等留给未来 pan/scan 需求）。

### 关键保证

- **YUV 平面几何一律以缓冲实测为准，不以显示尺寸假设**——这是解码器输出
  的普遍事实（行对齐/高对齐因平台与解码器而异），修复收敛在纯函数内部，
  平台后端（mf.rs）无需感知。
- 平台差异定位方法论沉淀：布局疑问题先转储实机首帧关键字节（缓冲长/
  步距/候选偏移处的字节样本），一次实验即可在多个假设间裁决。

### 测试状态

- `cargo test --lib`：46 passed / 0 failed。
- 修复前后对照转储：色度@1080 偏移全 0（绿）→ 转换后首行 RGBA
  (253,255,253) 近白（正确），实机确认绿带消失。

## 批次 3：Video 全平台硬解（硬解唯一策略，无硬解直接报错）

### 设计背景

用户决策升级：**只支持硬解、不落地任何软解**，平台无硬件解码能力直接报错
（新 `VideoError::NoHardwareDecoder`）；目标平台全集 Windows / Ubuntu /
macOS / iOS / Android / Web；格式承诺收敛仅 H.264/MP4。选型原则：各平台
**系统硬解框架**（零体积嵌入；Linux 因驱动生态无统一入口，用 GStreamer 做
硬解聚合层，二进制只增几百 KB 绑定代码，C 库为系统动态库）。

### 设计方案（平台矩阵）

| 平台 | 后端 | "无硬解报错"实现点 | 本机验证度 |
|---|---|---|---|
| Windows | MF（现有）+ `hardware_h264_available()` 前置探测 | D3D11 VideoDevice 按 H264 profile 集合探测 1080p NV12 解码配置（`GetVideoDecoderConfigCount` / `CheckVideoDecoderFormat`） | ✅ 实机通过（AMD 780M） |
| Linux(Ubuntu) | `gst_reader.rs`：`filesrc ! qtdemux ! h264parse ! <硬解器> ! videoconvert ! capsfilter(NV12) ! appsink` | ElementFactory ∩ klass `Hardware` ∩ 可吃 `video/x-h264`，rank 降序逐个尝试建管线（显存输出不可下载等协商失败自动换下一个），全败 → 报错 | 盲写（cfg(linux) 不参与 Windows 编译），API 逐个对 docs.rs 0.23.7 核对 |
| macOS/iOS | `videotoolbox.rs`：AVAssetReader（outputSettings=nil 取压缩样本）+ `VTDecompressionSession` | 规格字典 `RequireHardwareAcceleratedVideoDecoder`，会话创建失败 → 报错 | ✅ 跨目标 `cargo check`（aarch64-apple-darwin / aarch64-apple-ios）类型级通过 |

- **Windows 探测的关键事实**：`MFT_ENUM_FLAG_HARDWARE`（驱动自带 MFT）在大多数
  机器上枚举不到硬解——Windows 标准硬解路径是微软 H.264 MFT + DXVA（注册为
  软件 MFT、解码实际跑 GPU）。故探测以 D3D11 VideoDevice 的解码配置为准；
  且各厂驱动注册的 profile 命名不一（A~F / VLD_NoFGT），按集合探测任一可用
  即支持。windows 0.62 漏列 `DXVA_ModeH264_VLD_NoFGT` 常量（1b81be6b-…），
  按 dxva.h 定义值补写。
- **上层契约零改动**：`DecodeBackend` seam 下新后端输出统一 NV12 系统内存，
  `Video::update` 泵、追帧纪律、NV12→RGBA 定点转换、`texture_view` 直绑全部复用。
- **分发矩阵**（`VideoModule::open`）：windows→MF / linux→GStreamer /
  macos·ios→VT / 其余（含 wasm）→`UnsupportedPlatform`（wasm 的 WebCodecs
  后端为批次 B；Android MediaCodec 为批次 C，被引擎层安卓支持阻塞）。
- Apple 后端细节：VT 回调（内部线程）内按实测 plane stride 拷出 NV12
  （不假设几何，延续 1088 对齐教训），经 channel 衔接同步拉取语义；
  单帧解码失败不致命（对齐 MF 容错语义），会话生命周期由 CFRetained 管理。

### 环境要求（实机测试清单）

- **Ubuntu 24.04**（构建）：`sudo apt install build-essential pkg-config libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev`
- **Ubuntu**（运行时插件）：`gstreamer1.0-plugins-good`（qtdemux）、
  `gstreamer1.0-plugins-bad`（h264parse）；硬解按硬件选装
  `gstreamer1.0-vaapi`（Intel/AMD）或 NVIDIA 驱动插件；**不装** gst-libav
  即天然验证"无软解"。GStreamer ≥ 1.24。
- **macOS**：`brew install gstreamer` 不需要（VT 为系统框架）——Apple 平台零依赖。
- 实测命令：`cargo run --release --example 13_video_decode`，预期正常播放；
  无硬解机器预期 panic 信息含 `NoHardwareDecoder`。

### 测试状态

- `cargo test --lib`：47 passed（含 hw-probe 机器相关诊断测试）。
- Windows 实机：示例 13 冒烟正常（硬解探测通过、播放正常）。
- `cargo check --target aarch64-apple-darwin / aarch64-apple-ios --lib`：零 error。
- `cargo check --target wasm32-unknown-unknown --lib`：新依赖零侵入。
- 已知边界：Linux 运行时行为与 macOS/iOS 运行时行为为盲写（类型级验证通过），
  以用户实机测试为准。

## 批次 4：video 模块命名重构（平台命名 + 状态机独立）

### 设计背景

批次 3 落地后后端文件按解码技术命名（`mf.rs`/`gst_reader.rs`/`videotoolbox.rs`），
读代码需脑内换算"技术 → 平台"；状态机 `Video` 与模块组装混在 `mod.rs`，
职责不一目了然。

### 设计方案

```
src/base/video/
├── mod.rs      模块组装：VideoError / DecodedFrame / DecodeBackend trait /
│               VideoModule 入口 + cfg 平台分发
├── player.rs   播放状态机（主时钟 / 追帧纪律 / 帧纹理管理，平台中立）
├── yuv.rs      NV12→RGBA 整数定点转换（纯函数 + 测试）
├── windows.rs  Media Foundation 后端（原 mf.rs）
├── linux.rs    GStreamer 后端（原 gst_reader.rs）
└── apple.rs    VideoToolbox 后端（原 videotoolbox.rs，覆盖 macOS+iOS）
```

- 纯重命名 + `Video` 迁移（`Video::new` 改 `pub(super)`，模块内聚），
  公开 API（`Video`/`VideoModule`/`VideoError`）与示例零变化。
- 条件编译仍只在 `mod.rs` 分发边界一处。

- Windows：47 passed，`cargo check --examples` 零错误。
- `aarch64-apple-darwin` / `wasm32-unknown-unknown`：零 error。

## 批次 5：批次 B/C 落地——Web(WebCodecs) + Android(MediaCodec) 后端

### 设计背景

补齐全平台矩阵最后两块：**Web**（wasm32-unknown-unknown）与 **Android**。
两者系统解码 API 均不吃容器（裸 H.264 码流），故新增共享解复用层。

### 设计方案

**① `mp4_demux.rs`——共享解复用层（纯 Rust，`mp4` crate）**

- 拉出 H.264 视频轨样本 → AVCC（长度前缀）转 Annex-B（起始码），关键帧
  前置 SPS/PPS；从 SPS 提取 profile/level 组 codec string（Web 配置用）
- cfg `any(android, wasm32, test)`：`test` 使 Windows 测试构建可用**真实
  示例视频**做集成测试（解复用/Annex-B/pts 单调性全部本机验证）

**② seam 扩展（异步解码的世界观差异）**

- `DecodedFrame.pixels: FramePixels`：`Nv12`（桌面）| `VideoFrame`（Web，
  cfg 门控）——Web 帧是 GPU 句柄，浏览器负责 YUV→RGB，零 CPU 转换
- `DecodeBackend::next_frame → poll_frame() -> Poll{Frame,Pending,Eos}`：
  WebCodecs 解码异步，帧未到但流未结束 = `Pending`（上层时钟照走）；
  桌面后端恒为 Frame/Eos
- `is_ready() -> bool { true }`：Web fetch/配置期间 false，上层跳过泵但时钟照走
- `WasmVideoFrame` 包装：Drop 即 `close()`——追帧丢弃路径及时释放浏览器侧
  GPU 资源，不依赖 GC 终结器；纹理 usage 在 wasm 追加 RENDER_ATTACHMENT
  （WebGPU 外部图像拷贝的规范要求）

**③ `web.rs`——WebCodecs 后端**

- 加载：同步 `open` + spawn_local fetch 全文件（`path` 即 URL；字节范围
  流式为后续优化），装配产物经 `Rc<RefCell<Option<Inner>>>` 交回
- 硬解策略**平台偏差如实声明**：WebCodecs 的 `hardwareAcceleration` 只有
  prefer 系（API 无"强制"），用 `"prefer-hardware"` 为平台上限；
  `NotSupported` 类错误映射 `NoHardwareDecoder`
- 绑定：web-sys 的 WebCodecs 全部处于 unstable gate（需构建链加
  `--cfg=web_sys_unstable_apis`）——本后端**自持 js_sys Reflect 动态绑定**，
  不给构建链添 flag；VideoFrame 边界处经 `JsValue` 转 `web_sys::VideoFrame`
  （wgpu `ExternalImageSource::VideoFrame` 仅门控 wasm+web feature，不受影响）
- 喂流水位：decodeQueueSize ≤ 4；EOF 后 `flush()` 排空重排队列

**④ `android.rs`——MediaCodec 后端（JNI）**

- JavaVM 取自 `ndk_context`（依赖引擎侧安卓引导注入；未注入报 Backend）
- 硬解过滤：`MediaCodecList(ALL_CODECS)` + `isHardwareAccelerated`
  （API<29 名称启发式），无候选 → `NoHardwareDecoder`
- csd-0/csd-1 注入 SPS/PPS（Annex-B）；输出 YUV420Flexible 系统内存，
  stride 以 INFO_OUTPUT_FORMAT_CHANGED 实测为准
- 同步 dequeue 短超时轮询，与 MF 同契约；先拷后 release（缓冲归属纪律）
- Cargo：winit 启用 `android-native-activity`（安卓目标构建的引擎级前置，
  仅该目标生效）

### 关键保证

- 状态机（player.rs）对 6 平台完全一致：追帧纪律、纹理覆写、直绑口不变
- 平台差异仍只存在于 mod.rs 分发一处 + 后端文件内部
- 全部后端输出统一 NV12（桌面）或 GPU 帧直拷（Web），格式承诺仍收敛 H.264/MP4

### 测试状态

- Windows：50 passed（含 mp4_demux 真文件集成测试 3 例）；示例 13 冒烟正常
- 跨目标 `cargo check --lib` 零 error：`aarch64-linux-android` /
  `wasm32-unknown-unknown` / `aarch64-apple-darwin` / `aarch64-apple-ios`
- 已知边界：Android JNI 调用序列与 Web 浏览器实际解码行为为盲写（类型级
  验证通过），实机测试清单见批次 3；Android 运行另需引擎侧安卓引导立项

## 批次 6：Cargo features 场景化裁剪（gfx / font / video 可剔除）

### 设计背景

video 批次落地后，库对不需要视频的场景过重：Linux 构建强制要求
gstreamer dev 系统包、Windows 强制编译 MF/D3D11 探测、Apple/Android 强制
编译 objc2 系/jni。经引用核查，`gfx`/`font`/`video` 三模块**互零引用且
不被核心反向依赖**（纯叶子模块），具备干净剔除条件。

### 设计方案

```toml
[features]
default = ["gfx", "font", "video"]   # 默认全包含（行为与历史版本一致）
gfx   = []                            # 纯内部模块
font  = ["dep:ttf-parser"]
video = ["dep:windows", "dep:gstreamer", "dep:gstreamer-app", "dep:gstreamer-video",
         "dep:objc2", …（objc2 系 7 crate）, "dep:jni", "dep:ndk-context",
         "dep:mp4", "dep:js-sys"]
```

- 上述外部依赖全部改 `optional = true`；`audio` 恒参与编译（轻、游戏必用）
- `base/mod.rs` 三处模块声明加 `#[cfg(feature)]`；核心（渲染/窗口/循环/
  时间/资源/audio/web入口）恒编译
- 示例 06(gfx)/07(font)/13(video) 声明 `required-features`，特性未开时
  自动跳过编译；运行需 `cargo run --features video --example 13_video_decode`
- CLAUDE.md 命令与架构段同步更新

### 关键保证

- **默认行为与历史版本一致**（default 全包含）
- `--no-default-features` = 最小核心，video 的全部平台系统依赖（gstreamer
  dev 包等）不再被要求
- 条件编译仍收敛：feature cfg 在 mod.rs 一处，后端内部平台 cfg 格局不变

### 测试状态

- Windows 特性矩阵：default / --no-default-features / 单开 gfx / 单开
  font / 单开 video——全部零 error；测试 50（默认）/ 30（核心，video 系
  测试随特性缺席，自洽）
- 示例：默认 `--examples` 零错误（06/07/13 跳过）；`--example 13 --features
  video` 零错误
- 跨目标：wasm（默认/含video）、aarch64-apple-darwin（video）、
  aarch64-linux-android（video）零 error

## 批次 7：设备接口第一批——手柄（gamepad feature）

### 设计背景

设备接口体系首批落地：手柄（轮询输入）、GPS（异步定位+权限）、摄像头
（媒体流→纹理）三类中，手柄性质最纯、生态最成熟（gilrs 一 crate 覆盖
win/linux/mac 原生 + wasm 浏览器 Gamepad API），先行实施；
camera/GPS 列后续批次（camera 可复用 video 的 NV12/纹理管线）。

### 设计方案

**API 形态**（对齐键鼠状态表模式）：`Ctx.gamepad() -> &GamepadState` 轮询读取；
引擎在 `about_to_wait` 帧前刷新（`delta` 写入后、`frame` 前）——不新增
`Application` 回调（避免破坏 trait 签名）。`Button`/`Axis` 为自有枚举
（SDL 兼容布局，gilrs 变体一一对应；Web 标准布局映射）。

```rust
ctx.gamepad().primary() -> Option<usize>          // 首个连接
.is_connected(id) / .connected()                   // 热插拔即时反映
.is_pressed(id, Button::South)                     // 状态
.just_pressed(id, Button::South)                   // 帧间边沿（轮询模式标配）
.axis(id, Axis::LeftStickX) -> f32                 // -1..=1 / 扳机 0..=1
```

**平台矩阵与实现**：

| 平台 | 事件源 | 说明 |
|---|---|---|
| win/linux/mac | gilrs 0.11（`next_event` 非阻塞排水） | GamepadId 不透明 → Connected 锚定 + 连接顺序分配自有编号；ButtonChanged 阈值 0.5 归一按压态 |
| Web | 自持 js_sys Reflect 轮询 `navigator.getGamepads()` | Gamepad API 为稳定接口，不经 web-sys unstable 门控（与视频同法）；标准布局索引映射 |
| Android/iOS | 空实现占位（恒空表） | 待引擎事件管线立项；API 面不断（跨平台代码可编译） |

- gilrs 初始化失败（无 udev 等环境）降级空表不 panic
- feature：`gamepad = ["dep:gilrs"]`，入 default；gilrs target-gate
  win/linux/macos（wasm 用不到 gilrs——**gilrs 0.11 的 wasm 走 wgi feature
  会连带 web_sys unstable cfg，故 wasm 自持绑定**，与视频同决策）
- rumble 移除：gilrs 0.11 层无公开力反馈 API（仅 core 内部），如实声明不做

### 关键保证

- `just_pressed` 语义：本帧新按下（含帧内按下又释放）；实现为 prev 快照 +
  帧内事件集，不依赖事件不丢
- 热插拔即时反映；无手柄环境全 API 返回默认值、示例空表可跑

### 测试状态

- Windows：50 passed；`--no-default-features`（剔除 gamepad）零 error；
  示例 14 实机运行正常（无手柄空表路径）
- 跨目标零 error：wasm（双模式）/ android+gamepad（空实现）/ darwin+gamepad
- 实机手柄（Xbox/PS 控制器）验证待用户执行：`cargo run --features gamepad
  --example 14_gamepad`
