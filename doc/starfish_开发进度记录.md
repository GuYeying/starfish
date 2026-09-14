# Starfish 开发进度记录

> 活文档：记录当前能力状态、特性矩阵、已完成批次索引与待办路线。
> 历史细节见 `doc/log/starfish_changelog_YYYY-MM-DD.md`（按日归档）；
> 视频架构设计见 `reference/video模块跨平台架构笔记.md`。
>
> 最后更新：2026-09-14

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
| ~~GPS~~ | **取消**（用户决策 2026-09-14） | — | 定位类需求出现时，建议直接针对目标平台调 API |
| ~~摄像头~~ | **取消**（用户决策 2026-09-14，camera 模块已移除） | — | 同上：跨平台抽象层不如针对目标平台直调 API |

### 系统/网络能力（第二批）

| 能力 | 模块 | 覆盖 | 状态 |
|---|---|---|---|
| ~~本地文件~~ **已移除**（原 `base/iofi`） | — | 决策：std::fs 透传包装无增值；Web 无文件系统。IO 定位 = 桌面 std::fs 直用 + dialog 选文件；Web dialog 读内存 + net | 移除完成 |
| 对话框（统一异步，**定稿只做打开/保存**） | `base/dialog`（feature `dialog`） | 桌面 rfd / Web input[file]+Blob 下载 / 移动端 robius（**Android 构建需 ANDROID_JAR**） | ✅ 编译验证，待实机 |
| 网络（TCP 消息 + UDP） | `base/net`（feature `net`，`Connection` trait 统一接口） | 原生五平台 std::net 线程直连；Web WebSocket；**Web-UDP 显式不支持** | ✅ TCP 回环自动测试；WS 待联调 |

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

`default = ["gfx", "font", "video", "gamepad", "dialog", "net"]`（默认全包含）

| 特性 | 剔除模块 | 剔除的外部依赖 |
|---|---|---|
| `gfx` | base/gfx | —（内部） |
| `font` | base/font | ttf-parser |
| `video` | base/video | windows / gstreamer×3 / objc2×7 / jni / ndk-context / mp4 / js-sys |
| `gamepad` | base/gamepad | gilrs（win/linux/mac；wasm 自持绑定不依赖 gilrs） |
| `dialog` | base/dialog | rfd（桌面；web 消息框用 js-sys） |
| `net` | base/net | js-sys（仅 wasm WebSocket；零 tokio） | |

恒编译核心：渲染 / 窗口 / 循环 / 时间 / 资源 / **audio** / web 入口。

---

## 三、已完成批次索引（详见 doc/log/ 按日 changelog）

| 批次 | 内容 |
|---|---|
| 1 | 示例 13 修复：`BindGroupBuilder::texture_view` 直绑口 + `Video::texture_view()`；追帧上传纪律（O(N)→O(1)）；NV12→RGBA 整数定点化（debug 100ms→35ms/帧） |
| 2 | NV12 色度平面偏移修复（解码器 1088 对齐 → 顶部绿条）；色度起点由缓冲长度反推 |
| 3 | Video 全平台硬解批次 A：Windows 严格探测（D3D11 profile 集合，**勿用 MFT_ENUM_FLAG_HARDWARE**）+ Ubuntu GStreamer + macOS/iOS VideoToolbox |
| 4 | video 模块命名重构：平台命名（windows/linux/apple）+ 状态机独立（player.rs） |
| 5 | 批次 B/C：Web(WebCodecs) + Android(MediaCodec) 后端 |
| 6 | Cargo features 场景化裁剪（gfx/font/video 可剔除，默认全包含） |
| 7 | 设备接口第一批：手柄（gilrs 四平台 + 空占位） |
| 8 | 文档体系同步（README 现代化 / 进度活文档 / 跨会话记忆）——09-12 |
| 9 | 示例分类目录化（basics/render/draw/audio/platform/media）+ README 截图更新——09-12 |
| 10 | iofi + dialog + net 三件套（阻塞本地 IO / 原生对话框 / 零 tokio 网络）——09-14 |

---

## 四、待办 / 路线

### 实机验证清单（用户执行）

- [ ] Ubuntu 24.04：装 `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-good gstreamer1.0-plugins-bad` + 按显卡 `gstreamer1.0-vaapi`；跑示例 13（不装 gst-libav 可顺带验证无软解兜底）
- [ ] macOS：直接跑示例 13（VideoToolbox 零依赖）
- [ ] Windows：接手柄跑 `cargo run --features gamepad --example 14_gamepad`
- [ ] Web：示例 13 出 wasm 后浏览器播放验证（视频 URL 相对页面基址）

### 设备接口后续批次

- [x] ~~摄像头 / GPS~~：**已取消**（2026-09-14 用户决策——此类与硬件/系统强绑定的能力，跨平台抽象层不如针对目标平台直调 API；camera 半成品模块已整体移除）
- [ ] Android 手柄：需引擎安卓事件管线（android-activity 输入桥）

### 新三件套待办

- [ ] dialog 实机验证：打开/保存全流程（桌面 rfd 原生 / Web input[file]+下载 / 移动端 robius）
- [ ] dialog/Android 构建需 `ANDROID_JAR` 环境变量（robius 编译 Java 胶水）——随引擎安卓引导配齐
- [ ] dialog/iOS 构建验证（darwin 目标已过；iOS 真机待引擎引导）
- [ ] WebSocket 真服务器联调（当前仅 TCP 回环自动验证；浏览器侧 URL 需 ws:// 前缀或裸 host:port 自动归一）
- [ ] Linux dialog 构建需 libgtk-3-dev（rfd GTK3 后端）——CI/文档注明
- [ ] iofi/Web 配额 5MB 上限——大文件需求出现时评估 OPFS 异步版

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
