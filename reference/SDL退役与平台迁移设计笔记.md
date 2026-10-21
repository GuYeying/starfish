# SDL3 退役与平台迁移设计笔记（2026-09-08 定稿）

> 本文是跨多轮讨论的**决策定稿记录**：为什么退役 SDL3、新循环模型为何这样设计、
> 各模块替换选型与拆除顺序。实施批次见 doc/log 对应日期。

## 1. 决策链（背景）

1. **emscripten 工具链战争不可持续**：SDL3（CMake 源码编译 + emsdk 移植）与
   wgpu（gles/EGL 路径）在 wasm32-unknown-emscripten 上两套工具链反复冲突，
   靠 vendor 补丁（sdl3/glow/wgpu-types/wgpu-hal）+ EGL 桩 + 链接配方硬扛。
2. **生态事实**：winit / cpal 的 web 后端**只支持 wasm32-unknown-unknown**，
   emscripten 支持早已移除——"转 winit"与"留 emscripten"互斥，真正的决策是
   **Web 目标切 unknown-unknown**（wgpu 官方 web 特性同样只认该目标）。
3. **循环归属**：最终产品形态是 PyO3 的 pygame 风格 Python 库。Rust 持循环 +
   Python 回调会造成每帧跨 FFI 回调、GIL 编排、pygame 风格崩塌；反复权衡后
   **统一走"引擎持循环 + Application 回调"单门模型**，Python 侧用
   **生成器门面**（每帧一个 `yield`）包装回 pygame 风格（pygbag 同款思路，
   同一份脚本桌面/Web 通用）。放弃的双门（poll 泵 + run）方案留档于此。

## 2. 新循环模型（base/app.rs）

- `run(app: impl Application, config: WindowConfig) -> !`：唯一入口，**必须主线程**。
- `trait Application { start / event / frame }`：窗口就绪、平台事件、每帧三时机。
- `Ctx`：window / keyboard / mouse（引擎维护的状态表，对齐 pygame 的
  `get_pressed` 双轨）/ delta / exit。
- 桌面驱动：winit `ControlFlow::Poll` + `request_redraw`；`fps_cap` 经
  `Clock::tick` 节流（vsync 回压存在时勿与刷新率同档，见 Clock 文档）。
- Web（Step 4）：浏览器 rAF 驱动同一 trait，应用代码零改动。
- **事件模型平台中立**（base/window/event.rs）：自有 `WindowEvent`/`KeyCode`/
  `MouseButton` 枚举，后端（winit）翻译；pygame 常量（K_w 等）由 pygame/ 层做别名。

## 3. 选型表（维护风险评估过）

| 职责 | 旧（SDL3） | 新 | 理由 |
|---|---|---|---|
| 窗口/事件/循环 | sdl3 + sdl3-sys | **winit 0.30** | rust-windowing 机构，bevy/iced/egui 共同底座 |
| 音频设备层 | sdl3 audio | **cpal 0.18** | RustAudio 机构，rodio 底座；0.17+ Stream Send+Sync（服务 free-threaded 契约）；WebAudio/AudioWorklet 双 web 后端 |
| 时间 | SDL timer | **std::time::Instant**（+ unknown-unknown 上 sleep no-op，rAF 节流） | 零第三方依赖 |
| 解码 | symphonia | 不动 | 本就平台中立 |
| 渲染 | wgpu 30 | 不动（web 阶段加 webgl/webgpu 特性） | — |
| 手柄/haptic | sdl3 joystick/haptic | 删除（零使用者）；后续独立评估 gilrs（自带 rumble） | — |
| camera/sensor | 空占位 | 删除，**宁缺毋滥**；camera 若未来做：web 先行（getUserMedia 薄层），native 届时评估生态（nokhwa 维护停滞是前车之鉴） | — |

直接依赖净变化：−2（sdl3/sdl3-sys）+2（winit/cpal）；vendor/ 四补丁、egl_stub、
emsdk 流程全部退场。

## 4. 语义变化清单（迁移必须知道的）

1. **混音域 = 设备真实采样率**：SDL 在设备边界做任意规格转换，cpal 不做。
   采样率适配移到数据侧——`SoundData::resample`（SFX，load_sound/play 时一次性）
   + MusicStream 的 Resampler（流式本就按目标率重采样）。`AudioMixer::new` 签名
   从 `(subsystem, 采样率, 声道数)` 改为 `(声道数)`。
2. **时间原点**：从"SDL 初始化时刻"改为"进程内首次调用 now() 的固定原点"，
   Clock 无需任何先行初始化。
3. **窗口关闭不可否决**（v1）：CloseRequested 到达即置退出，仍派发事件供收尾。
4. **RenderEntry::new 第一参数**：`&window.inner()` → `&window`（Window 直接实现
   HasWindowHandle/HasDisplayHandle，wgpu 建表面不再经第三方转发）。
5. **hit_test → drag_window**：SDL 命中测试回调未迁移（winit 用 drag 模式），
   相关 API 从 v1 移除。

## 5. 线程契约（为 free-threaded Python 3.14t 首版预留）

PyO3 首版对齐 free-threaded CPython。契约（**已落地**：`base/rt.rs` 主线程锚点 +
`debug_assert_main_thread`，接线于 run / RenderEntry::new / surface_from_context /
RenderSurface::begin_frame·present·resize——调试构建 panic 定位误用，release 零成本）：

- **仅主线程**：`run()`、窗口操作、事件派发、surface present（文档 + debug_assert）。
- **任意线程**：AudioMixer/MusicPlayer/SFX（原子音量 + SPSC 环 + CmdSlot 的
  既有设计直接达标）、输入快照读、（远期）Mesh 数据构建与上传（wgpu Queue 本身
  Send+Sync）。
- cpal Stream（0.17+）Send+Sync ✓。
- v1 不做命令编组；debug_assert 兜底误用。

## 6. 拆除顺序（已执行）

1. ✅ time 去 SDL（3 调用点 → std::time；unknown-unknown 分支预留 no-op sleep）
2. ✅ 音频设备层 → cpal（device.rs 为平台差异唯一收敛点；subsystem/audio 整体退役）
3. ✅ Application trait + winit 桌面 + 自有事件枚举 + 11 示例迁移；
   **sdl3/sdl3-sys/vendor/sdl3 从依赖树与磁盘移除——SDL3 完全剔除**
4. ⬜ Web 切 wasm32-unknown-unknown（wgpu webgl 特性、winit web、spawn_local
   初始化、cpal wasm-bindgen、统一三套 cfg 写法、删 vendor/{glow,wgpu-types,wgpu-hal}
   与 web/ 桩资产、web 示例补 pump_streams）
5. ⬜ pygame/ 层 + PyO3 生成器门面（GIL→attach / check_signals / 异常边界 ~30 行胶水；
   cp314t 单独 wheel，abi3 在 3.14t 不可用）
