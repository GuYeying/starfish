# Starfish 开发进度记录

> 活文档：记录当前能力状态、特性矩阵、已完成批次索引与待办路线。
> 历史细节见 `doc/log/starfish_changelog_YYYY-MM-DD.md`（按日归档）；
> 视频架构设计见 `reference/video模块跨平台架构笔记.md`。
>
> 最后更新：2026-09-11

---

## 一、当前能力总览

### 平台支持矩阵（base/video 六平台硬解）

| 平台 | 后端 | 硬解框架 | 状态 |
|---|---|---|---|
| Windows | `base/video/windows.rs` | Media Foundation（DXVA） | ✅ 实机验证 |
| Linux(Ubuntu) | `base/video/linux.rs` | GStreamer 聚合（vaapi/nvdec） | ✅ 编译验证，待实机 |
| macOS/iOS | `base/video/apple.rs` | VideoToolbox（Require-HW 键） | ✅ 编译验证，待实机 |
| Web | `base/video/web.rs` | WebCodecs（prefer-hardware） | ✅ 编译验证，待实机 |
| Android | `base/video/android.rs` | MediaCodec（JNI） | ✅ 编译验证，待引擎安卓引导 |

- **硬解唯一策略**：无硬件解码器直接报 `VideoError::NoHardwareDecoder`，不落软解
- 格式承诺收敛：仅 H.264/MP4

### 设备接口（第一批）

| 设备 | 模块 | 覆盖 | 状态 |
|---|---|---|---|
| 手柄 | `base/gamepad`（feature `gamepad`） | win/linux/mac = gilrs；web = 自持 Gamepad API 轮询；android/ios = 空占位 | ✅ 编译验证，待实机手柄 |
| GPS | 未立项 | — | 路线：异步 + 权限层（五平台后端各自一套） |
| 摄像头 | 未立项 | — | 路线：活水视频源，复用 video 的 NV12/纹理管线（web 走 getUserMedia→HTMLVideoElement→wgpu 直拷） |

### 手柄 API 速查

```rust
let gp = ctx.gamepad();                     // 引擎每帧帧前刷新
gp.primary() -> Option<usize>               // 首个连接（连接顺序编号）
gp.is_connected(id) / .connected()
gp.is_pressed(id, Button::South)
gp.just_pressed(id, Button::South)          // 帧间边沿
gp.axis(id, Axis::LeftStickX) -> f32
```

---

## 二、Cargo 特性矩阵

`default = ["gfx", "font", "video", "gamepad"]`（默认全包含）

| 特性 | 剔除模块 | 剔除的外部依赖 |
|---|---|---|
| `gfx` | base/gfx | —（内部） |
| `font` | base/font | ttf-parser |
| `video` | base/video | windows / gstreamer×3 / objc2×7 / jni / ndk-context / mp4 / js-sys |
| `gamepad` | base/gamepad | gilrs（win/linux/mac；wasm 自持绑定不依赖 gilrs） |

恒编译核心：渲染 / 窗口 / 循环 / 时间 / 资源 / **audio** / web 入口。

---

## 三、已完成批次索引（2026-09-11，详见当日 changelog）

| 批次 | 内容 |
|---|---|
| 1 | 示例 13 修复：`BindGroupBuilder::texture_view` 直绑口 + `Video::texture_view()`；追帧上传纪律（O(N)→O(1)）；NV12→RGBA 整数定点化（debug 100ms→35ms/帧） |
| 2 | NV12 色度平面偏移修复（解码器 1088 对齐 → 顶部绿条）；色度起点由缓冲长度反推 |
| 3 | Video 全平台硬解批次 A：Windows 严格探测（D3D11 profile 集合，**勿用 MFT_ENUM_FLAG_HARDWARE**）+ Ubuntu GStreamer + macOS/iOS VideoToolbox |
| 4 | video 模块命名重构：平台命名（windows/linux/apple）+ 状态机独立（player.rs） |
| 5 | 批次 B/C：Web(WebCodecs) + Android(MediaCodec) 后端 |
| 6 | Cargo features 场景化裁剪（gfx/font/video 可剔除，默认全包含） |
| 7 | 设备接口第一批：手柄（gilrs 四平台 + 空占位） |

---

## 四、待办 / 路线

### 实机验证清单（用户执行）

- [ ] Ubuntu 24.04：装 `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad` + 按显卡 `gstreamer1.0-vaapi`；跑示例 13（不装 gst-libav 可顺带验证无软解兜底）
- [ ] macOS：直接跑示例 13（VideoToolbox 零依赖）
- [ ] Windows：接手柄跑 `cargo run --features gamepad --example 14_gamepad`
- [ ] Web：示例 13 出 wasm 后浏览器播放验证（视频 URL 相对页面基址）

### 设备接口后续批次

- [ ] **摄像头**：Windows MF 采集 / Linux v4l2src(gst) / Apple AVCapture / Web getUserMedia / Android Camera2——复用 video 的 NV12→纹理管线与 `texture_view` 直绑
- [ ] **GPS**：WinRT Geolocator / GeoClue(D-Bus) / CoreLocation / Web Geolocation / Android LocationManager；需先设计统一 Permission 模型（web/mobile 强权限）
- [ ] Android 手柄：需引擎安卓事件管线（android-activity 输入桥）

### 引擎级前置（阻塞项）

- [ ] 引擎安卓引导立项（android-activity + gradle 模板 + ndk_context 注入）——解锁 video/android 实测、Android 手柄/传感器等
- [ ] Web 视频示例（13 号出 wasm 产物的 wasm-bindgen CLI 流程文档化）

### 技术债 / 已知边界

- [ ] Web 视频加载为全文件 fetch（字节范围流式 + 渐进播放待做）
- [ ] WebCodecs 绑定为 js_sys Reflect 自持（无编译期类型保护）
- [ ] Linux(GStreamer) 与 Apple(VT)、Web、Android 四路后端为类型级验证（盲写），实机行为待验证
- [ ] Android 输出几何（crop/slice-height）按紧凑布局处理，异常布局需实机修正
- [ ] rumble 未做（gilrs 0.11 无公开力反馈 API）
- [ ] NV12→RGBA 上移 GPU 采样着色器（v2 既定：Y/UV 双纹理直传，debug 构建成本归零）
