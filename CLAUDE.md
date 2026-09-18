# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Starfish 是基于 **wgpu 30 + winit + cpal** 的跨平台游戏库（"次世代 pygame"，服务 Python 图形化原型开发者）。Rust 1.85+，edition 2024。项目文档、代码注释、更新日志全部使用**中文**。

**SDL3 已于 2026-09-08 全量退役**（窗口/事件→winit，音频设备→cpal，时间→std::time）。决策链、选型依据与语义变化清单见 `reference/SDL退役与平台迁移设计笔记.md`。

目标平台：桌面原生（Windows/Linux/macOS，规划 Android）+ **Web**。Web 目标正在从 `wasm32-unknown-emscripten`（已弃用）切换到 `wasm32-unknown-unknown`（winit/cpal/wgpu 官方 web 后端只支持后者），迁移进行中——下文"Web 构建"一节描述的是目标态，旧 emscripten 流程不要再用。

## 常用命令

```bash
cargo build                       # 原生构建（默认特性：gfx/font/video 全包含）
cargo test                        # 全部测试（全部纯逻辑测试）
cargo test <名称子串>              # 单个测试
cargo check --examples            # 检查所有示例编译（未启用特性的示例自动跳过）
cargo run --example 02_triangles  # 运行示例（01~11 按学习路线编号；02 是新循环模型的规范示例）
cargo run --features video --example 13_video_decode  # 特性化示例需显式开启对应特性
cargo check --no-default-features # 最小核心（渲染/窗口/循环/时间/资源/audio/web入口）
```

示例按类分目录（`examples/basics|render|draw|audio|platform|media/`），名称经
`Cargo.toml` 显式 `[[example]]` 段映射保持不变（`--example 02_triangles` 照旧）；
未启用特性的示例（06/07/13/14）自动跳过编译。

示例资源在 `resources/`（部分示例运行时读取，如 `resources/textures/wall.jpg`）。

## 特性（feature）

默认 `["gfx", "font", "video"]` 全包含；三个均可剔除：

| 特性 | 剔除的模块 | 剔除的外部依赖 | 适用场景 |
|---|---|---|---|
| `gfx` | `base/gfx`（几何/管线封装） | —（仅内部） | 自管几何 |
| `font` | `base/font` | ttf-parser | 无文本渲染 |
| `video` | `base/video`（六平台硬解） | windows(MF)/gstreamer/objc2系/jni/ndk-context/mp4/js-sys | 不用视频——**Linux 构建因此不再强制要求 gstreamer dev 包** |
| `gamepad` | `base/gamepad`（手柄状态表） | gilrs（win/linux/mac 原生；wasm 自持 Gamepad API 轮询；android/ios 空实现占位） | 无手柄需求 |
| `dialog` | `base/dialog`（统一异步对话框；桌面 rfd / Web alert·confirm+文件选择读入内存 / 移动端 robius DocumentPicker） | rfd（桌面）/ robius 系（移动端；**Android 构建需 ANDROID_JAR**） | 无弹窗/文件交互 |
| `net` | `base/net`（TCP 消息连接/UDP 轮询，`Connection` trait 统一接口，后台线程） | js-sys（仅 wasm WebSocket；**零 tokio**——线程直连） | 无网络需求 |
| `io` | `base/io`（统一异步 read/write/exists；原生 std::fs / Web fetch） | 无新增依赖（web-sys 特性已并入 wasm 段） | 只用 std::fs 的桌面项目 |

剔除不影响核心（渲染/窗口/循环/audio 恒参与编译；各可选模块互零引用）。
示例 06/07/13/14 已声明 `required-features`，特性未开时自动跳过。

**文件 IO 模块（`base/io.rs`，feature `io`，2026-09-18 重启旧 iofi 场景）**：
统一异步 API `read / read_text / write / write_text / exists`——原生 std::fs
直实现（阻塞包 async 壳），Web fetch 直实现（GET 读 / POST 保存，path 即
URL）。与批次 14 撤销的 iofi 的区别：Web 有 fetch 真实现，模块价值回归。
其余文件管理操作（list_dir/create_dir/删除等）v1 不设：原生用 std::fs，
Web fetch 无对应语义。

## 架构

### 双层 API

```
src/base/     底层封装（可脱离 pygame 思维直用）——库的主体
src/pygame/   pygame 风格接口（Color / Rect，Phase4 路线）
              # PyO3 绑定层接口契约（类钩子/单例/资源直绑/Web 结论）
              # 见 reference/pygame绑定层API设计稿.md
```

`base/` 内部：`render/`（渲染）、`gfx/`（几何）、`font/`（字体）、`audio/`（音频）、`time/`、`window/`（窗口+事件模型）、`app.rs`（循环模型）、`resources/`（图片加载）、`color.rs`、`error.rs`。**subsystem/ 已随 SDL3 退役删除。**

### 循环模型（base/app.rs）——引擎持循环，回调给应用

```rust
run(app, WindowConfig::new("标题", 宽, 高).with_fps_cap(120));  // 唯一入口，必须主线程
impl Application for App {
    fn start(&mut self, ctx: &mut Ctx) {}        // 窗口就绪后一次：RenderEntry、资源构建
    fn event(&mut self, e: &WindowEvent, ctx: &mut Ctx) {}  // 平台事件逐个派发
    fn frame(&mut self, ctx: &mut Ctx);          // 每帧：更新 + 渲染 + present
}
```

- 事件模型（`base/window/event.rs`）**平台中立**：自有 `WindowEvent`/`KeyCode`/`MouseButton` 枚举，后端（winit）翻译；pygame 常量（`K_w` 等）未来由 pygame/ 层做别名。
- **单窗口模型**（2026-09-14 定稿，多窗口剔除）：引擎持一个主窗口，`ctx.window()` 直取；窗口关闭（点 ×）= 应用退出。渲染侧 `RenderSurface` 与窗口 1:1。键鼠状态全局。
- 键鼠状态表由引擎维护（`ctx.keyboard().is_pressed(..)` 轮询 + 事件双轨，对齐 pygame）。
- 窗口关闭（CloseRequested）v1 语义：自动退出，不可否决。
- 选型记录：放弃"Python 持 while 的 poll 泵"双门设计，统一单门回调；未来 PyO3 层用生成器门面（每帧一个 `yield`）包装回 pygame 风格。

### 渲染（base/render/）

- **特性许愿系统**：三层愿望（core / recommended / special）+ 硬件掩码——设备创建**永不因愿望失败**，`RenderContext::features()` 运行时自查。
- **标准渲染对象 = 开发者持有数据**：`Mesh` / 管线 / `BindGroup` 等由 Builder 构造，无隐藏全局状态。
- `RenderEntry::new(ctx.window(), ...)`：窗口句柄经本库 `Window` 的 `HasWindowHandle`/`HasDisplayHandle` 转发直通 wgpu，不再经第三方转发；`pollster::block_on` 仅在 `cfg(not(target_arch = "wasm32-unknown-unknown"))` 分支。

### 音频（base/audio/）——跨平台驱动分离是核心架构决策

- `AudioMixer`（多声道 + 分组总线 + 效果器链）、流式 BGM（SPSC 环形缓冲）、录音（WAV 导出）、symphonia 解码（OGG/MP3/FLAC/WAV）。
- **设备层 = `base/audio/device.rs`（cpal 胶水），平台差异唯一收敛点**；上层全部平台中立纯逻辑。
- **混音域 = 设备真实采样率**（cpal 无 SDL 式设备边界转换）：采样率适配在数据侧——SFX 经 `SoundData::resample` 在 load/play 时一次性重采样，流式音乐由 MusicStream 的 Resampler 按混音域重采样。
- **DecoderPump**：解码核心提炼为独立纯逻辑结构体。native 用后台线程驱动（灌满即睡）；无线程环境由游戏循环每帧调 `pump_streams(budget)` 预算式驱动（当前无示例调用，Web 迁移落地后 web 示例必须调用）。
- 平台规则：条件编译只出现在"谁来驱动"的边界；共用核心**不得假设线程存在**。

### 时间（base/time/）

`std::time::Instant` 固定原点（进程内首次调用），无任何平台依赖；`wasm32-unknown-unknown` 上 `sleep_until` 为 no-op（Web 节流权归浏览器 rAF）。

### 线程契约（为 free-threaded Python 3.14t 首版预留）

仅主线程：`run()`、窗口操作、事件派发、present；任意线程：AudioMixer/MusicPlayer/SFX、输入快照读（cpal Stream 0.17+ 为 Send+Sync）。v1 靠文档 + debug_assert，不做命令编组。

## Web 构建（wasm32-unknown-unknown，已打通）

> 完整操作指南（前置准备/自己的项目上 Web/FAQ/无头验证）见 `reference/wasm编译与运行指南.md`；
> 运行时生命周期与尺寸竞态的原理推导见 `reference/wasm运行时生命周期与尺寸竞态问题详解.md`。

```bash
rustup target add wasm32-unknown-unknown        # 一次性
cargo build --release --target wasm32-unknown-unknown --example 11_web_triangles
wasm-bindgen --out-dir web --target web \
  target/wasm32-unknown-unknown/release/examples/11_web_triangles.wasm
cd web && python -m http.server 8000            # 浏览器打开 http://localhost:8000/11.html
```

- **唯一工具链依赖**：`wasm-bindgen-cli`，版本必须与依赖树 wasm-bindgen 严格一致（当前 0.2.128，`grep 'name = "wasm-bindgen"' Cargo.lock` 可查）。**升级 wasm-bindgen 依赖时必须同步升级 CLI**（实测踩过：webgpu 特性使 lock 浮动到新版本 → 旧 CLI 报 schema 不匹配）。
- emscripten 全套（emsdk/emcc/链接配方/egl_stub）已退役删除，vendor/ 补丁已清空——crates.io 原版直接可用。
- **双后端已开启并实测**：wgpu 同时编 `webgl` + `webgpu`，`Backends::all()` 下 `navigator.gpu` 存在选 WebGPU（实测 `BrowserWebGpu`）、缺失自动落 WebGL2（实测 `Gl`）——同一份二进制自动降级，示例 11 的控制台日志会打印实际后端。

**Web 关键坑位（都是实测踩过的，改动前先读）**：

1. **`ControlFlow::Wait`（Web）/ `Poll`（桌面）分家**（app.rs）：winit web 的 Poll 调度策略是 Scheduler.yield/setTimeout（非 vsync，"as fast as possible"），会让帧循环 CPU 全速空转卡死页面。Wait 下帧节奏 = `request_redraw` → canvas rAF；resumed 末尾必须补一次初始 request_redraw 启动帧链。
2. **`Resized` 事件必须调 `surface.resize()`，且帧循环要有尺寸自愈**：winit 接管 canvas 后初始 inner_size 为 0×0，真实尺寸经 ResizeObserver **异步**到达，还可能与异步资源初始化**赛跑**（事件先到 → resize 被跳过 → 表面永久 1×1 → 被 CSS 拉伸成"全屏纯色"）。运行时生命周期的完整推导见 `reference/wasm运行时生命周期与尺寸竞态问题详解.md`。
3. **接管页面 canvas 用 `WindowConfig::with_web_canvas_id("canvas")`**：winit 默认自建 canvas 且不入 DOM。
4. **view_formats（Unorm↔Srgb 重解释）WebGL2 不支持**：`SurfaceSettings::to_wgpu` 已在 Web 上置空并做 usage/present_mode 掩码；WebGL2 交换链格式仅 `[Rgba8Unorm, Rgba8UnormSrgb, Rgba16Float]`。
5. **未捕获错误处理器**（render_entry.rs wasm 分支）沿 source 链展开完整原因——Web 上 wgpu 错误直接给精确信息，别删。
6. 诊断工具链：无头 `msedge --headless=new --enable-logging=stderr --virtual-time-budget=6000 --screenshot=x.png <url>` + Python 裸解析 PNG 像素，可闭环定位渲染问题。启动时控制台的 "Using exceptions for control flow" 是 winit 既定机制，非错误。
7. **无头虚拟时间会冻结 `ctx.delta()`（实测 2026-09-18）**：`--virtual-time-budget` 下 rAF 自续链（每帧 request_redraw）且无外部真实事件解锁时，`performance.now()` 帧间不前进 → delta≡0 → 视频时钟/一切 dt 累计停滞，且**无任何报错**。虚拟时间模式只适用于纯渲染类静态验证；**时间相关测试（视频/动画/计时）用存活模式**：不带 budget/screenshot 启动 `msedge --headless=new <url>`，`--enable-logging=stderr` 收割 console 流（诊断输出必须走 `console_log`，wasm 上 `println!` 无处可去），真实时间等足后 `taskkill /T /F` 收尾。`--screenshot` 相对路径会落到 Edge 版本目录，一律用绝对路径。

## Android 构建（arm64-v8a，已打通）

> 完整指南（前置/分步/新示例接入/APK 模板）见 `reference/android构建与运行指南.md`。

```bash
cargo xtask list                                            # 全部示例 + Android 支持注册表
cargo xtask android 03_texture                              # 自动解析 *_android；构建→打包→部署
cargo xtask android 16_dialog --build                       # 仅出 APK（target/android-apk/）
cargo xtask android 15_empty_window --abi x86_64            # 模拟器 ABI
cargo xtask android 15_empty_window --no-default-features --features dialog,gfx   # 特性勾选
```

- 工具本体 = `xtask/` crate（xtask 模式，Rust 原生跨平台；特性→Android 支持登记在
  `FEATURE_ANDROID_SUPPORT` 表，**新增模块加一行**）。旧 bash 脚本 `scripts/android_run_example.sh`
  兼容保留。
- winit 走 `android-native-activity`：APK 用系统 `NativeActivity` 模板（`android/AndroidManifest.xml`），`android_main` → `run_android` 与桌面回调一致。
- **API 26 是硬性下限**（cpal 的 AAudio；cargo-ndk 默认 21 会报找不到 `libaaudio`），且 platform flag 是大写 `-P`/`--platform`。
- Android 示例采用**同源双注册**：同一文件注册两条 `[[example]]`（桌面 bin + `*_android` cdylib；bin 与 cdylib 不能混用），文件内 `#[unsafe(no_mangle)] fn android_main`（cfg 分家）。资源 cfg 分家：桌面读 `resources/`，Android 内嵌/私有目录落盘。
- dialog 需要 APK 内 **classes.dex**（robius 的 FilePickerFragment）+ `hasCode="true"`——xtask 自动并入。
- Rust 日志/panic 进 logcat tag `RustStdoutStderr`：`adb logcat -s RustStdoutStderr`。

## 文档惯例

- **每日更新日志**：`doc/log/starfish_changelog_YYYY-MM-DD.md`，按批次记录架构决策（设计背景 / 设计方案 / 关键保证 结构），含当天测试状态。
- 根目录 `未记录到日志的` 是待整理进日志的会话记录暂存文件。
- `reference/` 存放设计讨论与技术选型笔记（SDL 退役决策、跨平台方案、着色器、AssetManager 等）。
