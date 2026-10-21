# Starfish 更新日志 — 2026-09-08

---

## 🎯 全日总览：平台底座更替——SDL3 退役，winit + cpal + Web 双后端落地

> 当日完成了一次完整的平台底座更替（三个实施批次 + 四项追记，细节见下文各节；
> 决策链与选型依据见 `reference/SDL退役与平台迁移设计笔记.md`、
> `reference/wasm编译改动清单与踩坑经验.md`、`reference/wasm运行时生命周期与尺寸竞态问题详解.md`）。

### 1. 架构依赖重新选择

- **SDL3 全量退役**（窗口/事件/时间/音频设备四个职责全部易主）：
  窗口/事件/主循环 → **winit 0.30**；音频设备层 → **cpal 0.18**；
  时间 → **std::time::Instant**（Web 分支 performance.now）。
- 选型依据：winit（rust-windowing，bevy/iced/egui 共同底座）与 cpal（RustAudio，
  rodio 底座）是各自领域下载量千万级的机构维护事实标准；SDL3 的 emscripten
  工具链冲突（CMake/emsdk vs wgpu EGL）不可持续，且 winit/cpal/wgpu 的 Web
  后端只认 `wasm32-unknown-unknown`。
- 直接依赖净变化 −2（sdl3/sdl3-sys）+2（winit/cpal）；`vendor/` 四个补丁、
  egl_stub、emsdk 工具链全部删除——**Web 工具链只剩 wasm-bindgen-cli 一项**。
- 手工整理 Cargo.toml：按职责分组、清除三项零使用依赖
  （env_logger/bitflags/cgmath——后者已被 glam 取代）。

### 2. 开发风格妥协（循环模型统一）

- **放弃双门设计**（"Python 持 while 的 poll 泵" + "自驱 run"并存），
  统一为**单门回调模型**：`run(app, WindowConfig)` +
  `Application { start / event / frame }`——引擎持循环，回调给应用。
- 妥协的代价与回报：pygame 传统写法（用户持 while）无法直接保留；回报是
  跨平台单一代码路径（桌面 Poll / Web rAF 同一 trait）、macOS 无 pump 风险、
  Web 可行。未来 PyO3 层以**生成器门面**（每帧一个 `yield`）把回调模型
  包装回 pygame 风格（pygbag 同款思路），同一份 Python 脚本桌面/浏览器通用。
- **free-threaded Python 3.14t 契约落地**：主线程 API 接入
  `base/rt.rs` debug_assert 兜底；音频全家任意线程可用。
- 平台差异封装为三件套，**应用代码零 cfg**：`InitSlot<T>`（异步资源槽位）、
  `web_entry!()` 宏（Web 入口 + panic 转发）、`base::web::console_log`。

### 3. 其他模块重新适配

- **time**：3 个调用点 → std::time（Web 分支 no-op sleep，节流归 rAF）；
  Clock/FixedTimestep 公共 API 零变化。
- **音频**：设备层收敛于 `base/audio/device.rs`（cpal 胶水）；
  **混音域 = 设备真实采样率**（SDL 的设备边界转换不复存在），采样率适配
  移到数据侧（SoundData::resample 一次性 / MusicStream Resampler）。
- **渲染**：`RenderEntry` 参数泛化为本库 `Window`（HasWindowHandle 直通）；
  SurfaceSettings 增加 caps 掩码（usage/present_mode 交集、view_formats
  Web 置空）；`RenderSurface` 新增 `size()`/尺寸自愈配套。
- **事件**：自有 `WindowEvent`/`KeyCode`/`MouseButton` 枚举（平台中立契约），
  键鼠状态表（轮询 + 事件双轨）。

### 4. wasm 支持（Web 目标切换 + 双后端）

- Web 目标：`wasm32-unknown-emscripten` → **`wasm32-unknown-unknown`**；
  wgpu `webgl`+`webgpu` 双特性——**"WebGPU 优先 / WebGL 兜底"自动降级实测打通**
  （`BrowserWebGpu` / `Gl` 双路渲染正确）。
- 初始化 `spawn_local` 异步化；`ControlFlow::Wait`（Web）/`Poll`（桌面）分家；
  修正全库畸形 cfg（`target_arch = "wasm32-unknown-unknown"` 恒 false 写法）；
  设备级未捕获错误处理器（错误链完整展开）。
- 工具链：`rustup target add wasm32-unknown-unknown` + `wasm-bindgen-cli`
  （版本与依赖树严格一致，当前 0.2.128）。操作手册见
  `reference/wasm编译与运行指南.md`。

### 5. 状态

- `cargo test`：41/41 ✅（桌面）
- `cargo check --examples`：11 示例 0 error ✅
- wasm 目标（lib + 示例）：0 error ✅
- 浏览器渲染：无头截图实证（navy 底 + RGB 插值三角形）✅；
  双后端切换矩阵（BrowserWebGpu / Gl）✅

---

## 📐 架构决策一：wasm-bindgen 泄漏的根因修正与 vendor 补丁
> ⚠️ 历史定位：本批的 emscripten 方案在当日批次二/三中被整体取代
> （目标切 wasm32-unknown-unknown，vendor 补丁与链接配方全部退役），
> 保留作为决策痕迹。

### 设计背景

09-07 spike 的结论是"wgpu 30 对 wasm32 目标无条件依赖 wasm-bindgen"，
并用 `-sERROR_ON_UNDEFINED_SYMBOLS=0` + `web/index.html` 的
`instantiateWasm` 补桩双保险硬扛。本批逐层核查源码后**推翻该根因分析**：

| 层 | 门控实况（源码已验证） |
|---|---|
| wgpu 30 本体 | ✅ 正确。wasm-bindgen 系依赖在 `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))` 下声明 |
| wgpu-hal gles | ✅ 正确。build.rs 的 `Emscripten` 别名走 `emscripten.rs` + `egl.rs`（EGL 路径），不编译 wasm-bindgen 版 `web.rs` |
| **glow 0.17** | ❌ 病灶一。`[target.'cfg(target_arch = "wasm32")'.dependencies]`（js-sys / slotmap / wasm-bindgen / web-sys）未排除 emscripten；代码层其实正确（emscripten 走 native 模块），纯属依赖声明漏门 |
| **wgpu-types 30.0** | ❌ 病灶二。js-sys / web-sys 同款漏门；且 `wgpu-hal` 的 `gles` feature 无条件激活 `wgpu-types/web`，而 gles 在 emscripten 上是唯一后端、躲不开 |

`cargo tree -i wasm-bindgen --target wasm32-unknown-emscripten` 实证：
修复前 wasm-bindgen 全家在编译图内（经 glow 与 wgpu-types 两条路径）。

另有一个实证修正：用 node 解析旧产物 `11_web_triangles.wasm` 的**导入段**，
249 个导入全部是 emscripten 运行时（module "a"），**零个** wasm-bindgen 导入
——二进制里 grep 到的 `__wbindgen_*` 字符串只是 name 段符号名残留。
即：浏览器实例化早已不被阻塞，`instantiateWasm` 补桩钩子是永不触发的保险丝。

### 设计方案

遵循"上游修逻辑，本地修门控"的最小 vendor 原则（照 sdl3 补丁模式）：

1. **`vendor/glow`**：四个 wasm32 依赖段门控改为
   `cfg(all(target_arch = "wasm32", not(target_os = "emscripten")))`（一行式修复）。
2. **`vendor/wgpu-types`**：Cargo.toml 两个依赖段同款门控 + `external_image.rs`
   6 处 cfg 同步排除 emscripten。**自洽性**：下游消费者全部挂在 `cfg(webgl)`
   别名下（本就排除 emscripten），补丁不影响任何平台。
3. **Cargo.toml**：wgpu 改 `default-features = false` + 显式列出
   `std / parking_lot / dx12 / metal / vulkan / gles / wgsl`——摘掉 default 里的
   `"webgpu"`（它激活 `web` → `wgpu-types/web`）。desktop 上该 feature 本就惰性。

### 关键保证

1. **链接期不再需要容忍旗标**：wasm-bindgen 全家退出 emscripten 编译图，
   emcc 无未解析符号，真实链接错误不再被掩盖。
2. **native 零影响**：补丁只动 wasm32 依赖段与 web 门控代码。
3. **可回退**：上游修复后删 vendor 目录与 `[patch.crates-io]` 条目即可。

---

## 📐 架构决策二：emscripten 上 gles 后端 EGL 上下文属性补丁（wgpu-hal vendor）

### 设计背景

链接打通后首次真实浏览器运行报：
`CreateSurface(FailedToCreateSurfaceForAnyBackend({}))`——**空的 errors map**
说明没有任何 hal 后端尝试过创建 surface：`wgpu-core::try_add_hal` 对
`Instance::init` 失败只走 `log::debug` 静默吞掉。顺着 `wgpu-hal gles → egl.rs →
emsdk libegl.js` 逐层核对，找到确定性死点：

- wgpu-hal `Inner::create` 构造 GLES context 属性用 **EGL 1.5 风格**：
  `CONTEXT_MAJOR_VERSION(0x30FB) = 3`（+ `CONTEXT_MINOR_VERSION`）；
- emsdk 的 EGL 仿真（`libegl.js`）是 **EGL 1.4 语义**：`eglCreateContext`
  的属性表只接受 `EGL_CONTEXT_CLIENT_VERSION(0x3098)` 与 `EGL_NONE`，
  其它一律 `EGL_BAD_ATTRIBUTE` 直接返回空句柄；
- 于是 `eglCreateContext` 必败 → `Instance::init` 必败 → 后端注册表为空 →
  表面上是 surface 错误，实际是后端从未初始化。
- **09-07 spike 只验证到链接+部署，未在浏览器真实渲染**——此问题当时已存在。

### 设计方案

1. **`vendor/wgpu-hal`**（`src/gles/egl.rs`）：Emscripten 分支改传
   `CONTEXT_CLIENT_VERSION = 3`（GLES 3 / WebGL2），native 路径原样保留。
2. **链接配方加 `-sMAX_WEBGL_VERSION=2`**：libegl.js 默认 `MAX_WEBGL_VERSION=1`
   时拒绝 client version 3；置 2 后 `eglCreateContext` 内部经
   `GL.createContext(canvas, {majorVersion: 2})` 创建 WebGL2 上下文。
3. **运行链路核对**（libegl.js 逐函数验证）：
   `eglGetDisplay(0) → 62000`、`eglInitialize → 1.4`、
   `eglCreateContext(CLIENT_VERSION=3) → 62004`、
   `eglMakeCurrent(62000, 0, 0, 62004)` 支持无 surface 绑定（adapter 枚举需要）、
   `eglCreateWindowSurface → 62006`（magic 默认 surface，忽略窗口值绑定
   `Module.canvas`）——与 wgpu-hal 的 `(WindowKind::Unknown, Rwh::Web)` 分支、
   EGL 1.4 `create_window_surface` 回退路径完全咬合。
4. **示例平台 cfg 修复**：`#[cfg(target_arch = "emscripten")]` 是**永不匹配**
   的错写（target_arch 只有 "wasm32"，rustc 只发 warning）——导致 wasm 构建
   静默编译出**桌面阻塞循环 main**（`window` 字段 unused、`Event` 导入 unused
   即此症状），且 `emscripten_loop` 模块从未编译、藏住了 `unsafe extern` 编译错。
   全部改为 `target_os = "emscripten"`。
5. **示例加 debug 日志**：`env_logger` debug 级初始化，浏览器控制台可直接看到
   wgpu/EGL 初始化失败的确切原因（本次排雷若非 `log::debug` 被吞可省一小时）。

### 关键保证

1. emscripten 上 gles 后端 `Instance::init` → `create_surface` →
   `request_adapter` 全链路与 emsdk EGL 仿真语义对齐。
2. native 渲染路径零改动（补丁全部包在 `#[cfg(Emscripten)]` 内）。
3. 诊断方法沉淀：**空 errors map = 后端没注册 ≠ surface 创建失败**，
   排查入口是 `try_add_hal` 的 `log::debug`。

---

## 📐 架构决策三：遮挡查询的平台边界（render_surface 修复）

### 设计背景

canvas 挂载修复后，实例/适配器/设备链路全部打通，但 `RenderSurface::new` 在
Web 上崩：`GLctx.disjointTimerQueryExt.createQueryEXT is not a function`。

调用链：`RenderSurface::new` **无条件创建** occlusion QuerySet（示例根本不用它，
纯属占位）→ wgpu-hal gles `gl.create_query()` → glow 回退逻辑
（`GenQueries` 未加载时调 `GenQueriesEXT`）→ emscripten 的 GL 仿真
**只实现 EXT 系查询函数**（libwebgl.js 无核心版 `_glGenQueries`）→
`glGenQueriesEXT` 内部调 `disjointTimerQueryExt.createQueryEXT()`——
而 WebGL2 的 `EXT_disjoint_timer_query_webgl2` 扩展**只有 queryCounter**，
没有 createQueryEXT（那是 WebGL1 的 API）→ TypeError。

### 设计方案

`RenderSurface` 的 `occlusion_query_set` 字段改 `Option<Arc<QuerySet>>`：
- native：照常创建（行为零变化）；
- `#[cfg(target_os = "emscripten")]`：不创建（`begin_frame` 的清屏 pass
  descriptor 传 `None`，wgpu 本就允许）。
遮挡查询归入 features.rs 的"原生 only 位"语义；cfg 落在驱动边界（项目总则）。

### 关键保证

1. native 零影响（同一构造代码原样保留在 not(emscripten) 分支）。
2. emscripten 上渲染 pass 不再携带无法兑现的查询集。

---

## 📊 状态

- `cargo tree -i wasm-bindgen --target wasm32-unknown-emscripten`：空 ✅
- 无容忍旗标链接成功；产物导入段仅 emscripten 运行时（node 核验）✅
- `cargo test --lib`：41/41 ✅
- 浏览器渲染验证：待最终确认（4 件套配方 + 三重 vendor 补丁后的产物已部署 `web/`）

## 📌 链接配方（当前 4 件套）

```
-lEGL -lGL                    EGL/GLES 链接桩
-sMAX_WEBGL_VERSION=2         EGL 仿真放行 GLES3/WebGL2 上下文
--js-library web/egl_stub.js  补齐 libegl.js 缺失的两个 EGL 1.5 平台函数（链接桩，运行路径未用）
```

---

# 批次二：SDL3 全量退役——平台迁移 Step 1~3 落地（winit + cpal + std::time）

> 决策链、选型依据、语义变化清单的完整版见 `reference/SDL退役与平台迁移设计笔记.md`。
> 本批把库的平台底座从 SDL3 整体迁到生态主线：窗口/事件 **winit 0.30**、
> 音频设备 **cpal 0.18**、时间 **std::time::Instant**，并确立新的循环模型。

## 📐 架构决策：SDL3 退役 + 循环模型统一为"引擎持循环 + Application 回调"

### 设计背景

- emscripten 目标上 SDL3（CMake/emsdk）与 wgpu（gles/EGL）两套工具链反复冲突，
  vendor 补丁 + EGL 桩 + 链接配方的维护成本不可持续；
- winit/cpal/wgpu 的官方 web 后端**只支持 wasm32-unknown-unknown**，emscripten
  支持已被生态移除——留恋 emscripten 即与生态主线为敌；
- 终态产品是 PyO3 的 pygame 风格 Python 库（首版对齐 free-threaded 3.14t）。
  经多轮权衡，放弃"Python 持 while 的 poll 泵"双门设计，**统一单门回调模型**，
  Python 侧将来以生成器门面（每帧一个 `yield`）包装回 pygame 风格
  （pygbag 同款思路，同一份脚本桌面/Web 通用）。

### 设计方案（Step 1~3 实施内容）

1. **time 去 SDL**（`base/time/mod.rs`）：`performance_counter/frequency` →
   `std::time::Instant` 懒初始化固定原点（无先行初始化要求）；`timer::delay` →
   `thread::sleep` + 末段自旋；`wasm32-unknown-unknown` 分支 `sleep_until` 为
   no-op（Web 节流权归 rAF）。公共 API（Clock/FixedTimestep）零变化。
2. **音频设备层 → cpal**（新增 `base/audio/device.rs`，平台差异唯一收敛点）：
   - `subsystem/audio/`（SDL 设备胶水）整体删除；`StereoFrame`/`AudioError`/
     `AudioUserCallback` 迁入 `base/audio/common.rs`（自包含，零平台依赖）；
   - `AudioMixer::new(声道数)` 新签名（去 AudioSubsystem 参数与 SDL AudioSpec）；
     `AudioRecorder::new()` / `device_names()` 对齐；
   - 格式协商：f32 交错立体声（优先设备默认配置，否则支持列表取最高采样率）；
   - **混音域 = 设备真实采样率**：SDL 的设备边界转换不复存在，采样率适配移到
     数据侧——`load_sound`/`play_*` 对不匹配源经现成的 `SoundData::resample`
     一次性重采样，流式音乐本就按目标率走 Resampler；
   - `AudioError::Sdl` 变体退役 → `Device(String)`。
3. **窗口/事件 → winit + 新循环模型**：
   - 新增 `base/app.rs`：`run(app, WindowConfig)` + `trait Application {start/event/frame}`
     + `Ctx`（window/keyboard/mouse 状态表/delta/exit）。桌面由 winit
     `ControlFlow::Poll` + `request_redraw` 驱动，`fps_cap` 经 `Clock::tick` 节流；
   - 新增 `base/window/event.rs`：自有 `WindowEvent`/`KeyCode`/`MouseButton`/
     `KeyModifiers` 枚举（winit 风格命名，平台中立契约；pygame 别名留给 pygame/ 层）；
   - `base/window/window.rs` 重写：`Window` 包 winit 窗口，直接实现
     `HasWindowHandle`/`HasDisplayHandle` 转发，`render_entry.rs` 参数
     `&SdlWindow` → 本库 `&Window`（渲染层与窗口后端解耦）；
   - 11 个示例全部迁移到回调风格（SDL 事件/键盘轮询 → `WindowEvent`/`Ctx` 状态表
     逐项对照迁移，渲染与音频逻辑逐行保真）；
   - 删除：`subsystem/` 整目录、`window/hit_test.rs`（SDL 命中测试不迁移）、
     `Cargo.toml` 的 sdl3/sdl3-sys 依赖与 vendor/sdl3 补丁、`vendor/sdl3` 目录。

### 关键保证

- **测试全程安全网**：41 个测试全部纯逻辑（ring/clock/mixer/music/features/
  geometry/font），无一打开设备/窗口——每步替换后测试全绿。
- **后端可替换性收敛**：音频平台差异只在 `device.rs`；窗口后端类型不越过
  `event.rs` 的自有枚举与 `Window` 的句柄转发——未来换后端不动上层。
- **语义变化明码标价**：混音域=设备采样率；时间原点=进程内固定点；
  CloseRequested v1 不可否决；hit_test/相对鼠标 → drag/`set_relative_mouse` 模式。

## 📊 状态

- `cargo test`：41/41 ✅
- `cargo check --examples`：11 个示例 0 error ✅
- `cargo tree -i sdl3`：**包不存在**（依赖树零残留）✅；`grep sdl3 src/ examples/`：零残留 ✅
- 待办（Step 4）：Web 切 `wasm32-unknown-unknown`（wgpu webgl、winit web、
  spawn_local、cpal wasm-bindgen、统一三套 cfg 写法、删 vendor/{glow,wgpu-types,wgpu-hal}
  与 web/ 桩资产、emsdk 流程下线、web 示例补 `pump_streams`）

---

# 批次三：Step 4 落地——Web 切换 wasm32-unknown-unknown，浏览器渲染打通

> 设计依据见 `reference/SDL退役与平台迁移设计笔记.md`。本批把 Web 目标从
> emscripten 切到生态主线 `wasm32-unknown-unknown`，同一份 `Application`
> 代码桌面/浏览器双端运行。

## 📐 架构决策与实现

### 目标与工具链
- Web 目标：`wasm32-unknown-emscripten` → **`wasm32-unknown-unknown`**；
  wgpu 加 `webgl` 特性（WebGL2），cpal 加 `wasm-bindgen` 特性（WebAudio）。
- **工具链只剩一样**：`wasm-bindgen-cli`（版本必须与依赖树 wasm-bindgen 严格一致，
  当前 0.2.126）。emsdk/emcc/4 件套链接配方/egl_stub/activate_wasm.bat 全部退役；
  `.cargo/config.toml` 的 emscripten 段删除；vendor/{glow,wgpu-types,wgpu-hal}
  三个补丁删除——**vendor/ 目录消失，crates.io 原版直接可用**。

### 循环模型 Web 适配（base/app.rs）
- `run()` Web 变体：`spawn_local` 进入事件循环后**立即返回**（返回类型 `()`，
  桌面 `-> !`）——浏览器主线程不能阻塞。
- **`ControlFlow::Wait`（Web）/ `Poll`（桌面）分家**：winit web 的 Poll 调度
  策略是 Scheduler.yield/setTimeout（"as fast as possible"，非 vsync），
  实测帧循环以 CPU 全速空转 → 页面卡死级卡顿。Wait 下帧节奏由
  `request_redraw` → **canvas rAF** 驱动（每 vsync 一帧）；resumed 末尾补一次
  初始 request_redraw 启动帧链。
- **`WindowConfig::with_web_canvas_id`**：接管页面 `<canvas id>` 元素
  （winit 默认自建 canvas 且不入 DOM——文档化行为，嵌入网页必须显式传）。

### 渲染适配
- 初始化：`RenderEntry::async_new` + `spawn_local`（裸 wasm 无阻塞模型；
  pollster cfg 修正为 `not(target_arch = "wasm32")`——原
  `target_arch = "wasm32-unknown-unknown"` 是**永不成立的畸形 cfg**，已全库修正）。
- **时间**：`performance.now()` 适配（std Instant 在该目标不可用）；
  `sleep_until` no-op，节流权归浏览器 rAF。
- `SurfaceSettings::to_wgpu` 加"许愿→掩码"：usage/present_mode 与 caps 取交集；
  **view_formats（Unorm↔Srgb 重解释）WebGL2 不支持，Web 上置空**。
- 遮挡查询 Web 上不创建（cfg `target_arch = "wasm32"`）。
- **设备级未捕获错误处理器**（wasm 分支）：沿 source 链展开完整原因后抛出——
  默认处理器只打印顶层 "Validation Error"，细节全丢；本处理器让 Web 端
  错误直接给出精确原因（本次调试立功）。

### 示例 11 的 Web 模式（规范样例）
- 资源构建 async（spawn_local 注入 `Rc<RefCell<Option<Gpu>>>` 共享槽位，
  `frame` 未就绪静默跳过）；渲染代码两平台零分叉。
- **`Resized` 事件必须调 `surface.resize()`**：winit 接管 canvas 后初始
  inner_size 为 0×0，真实尺寸经 ResizeObserver 异步到达；不处理则表面停在
  初始尺寸（1×1 帧缓冲被 CSS 拉伸 → "全屏纯色"假象）。

## 🔍 调试方法论（本批沉淀）
无头 Edge（`--headless=new --enable-logging=stderr --virtual-time-budget`）抓控制台
+ `--screenshot` 截图 + Python 裸解析 PNG 像素 → 渲染问题闭环定位无需人工盯屏。
隔离实验链：纯红清屏（管线正确性）→ navy 无 draw（clear 正确性）→
位置硬编码（几何 vs 颜色链路）→ 像素值反推 sRGB 编码闭合 → **1×1 帧缓冲拉伸**
根因实锤（NDC 中心重心插值 0.5/0.25/0.25 → (188,137,137) 精确吻合）。

## 📊 状态
- `cargo test`：41/41 ✅（桌面回归）
- `cargo check/build --target wasm32-unknown-unknown`：0 error ✅
- 无头浏览器截图：RGB 三角形 + navy 底正确渲染 ✅
- 已知良性现象：启动时控制台出现一次 winit 的
  "Using exceptions for control flow"（其退出同步栈的既定机制，非错误）
- 遗留小项：wgpu 的 `webgpu` 特性（WebGPU 后端）留评估；AudioWorklet 低延迟
  音频留评估；多窗口 Web 语义未验证

## 📌 追记：WebGPU 后端开启，"WebGPU 优先 / WebGL 兜底"能力实测打通

- wgpu 同时编译 `webgl` + `webgpu` 特性（当初摘掉 `webgpu` 是为 emscripten
  规避 wasm-bindgen 泄漏，该理由已随目标退役消失）。`Backends::all()` 下
  wgpu 按 `navigator.gpu` 可用性自动选择后端，**应用层零分叉**。
- 无头验证矩阵：默认配置 → `后端=BrowserWebGpu`（渲染正确）；强制
  `Backends::GL` → `后端=Gl`（渲染正确，Rgba8UnormSrgb 编码）。同一份
  二进制、两种色彩编码（Srgb 目标 188,137,137 / 非 Srgb 原始 128,64,64）
  恰好互证后端确实切换。
- 示例 11 保留一行后端报告日志（`adapter_info().backend`），用户可直接
  确认所选后端。两后端差异（view_formats/遮挡查询等保守掩码按 target
  生效，WebGPU 路径同样适用、无害）。
- **追记 2（竞态修复）**：`Resized` 事件可能先于异步资源初始化到达（共享槽位
  尚为 `None`）导致 resize 被跳过、表面永久卡在 1×1（WebGPU 路径呈"深红画布"
  =(128,64,64) 线性原始字节拉伸；GL 路径为 (188,137,137)）。修复：`RenderSurface`
  新增 `size()`，示例 11 帧循环做尺寸自愈（`ctx.size()` ≠ 表面配置即 resize），
  覆盖一切事件/异步交错顺序。完整分析见 `reference/wasm编译改动清单与踩坑经验.md`
  坑 4b / 坑 10（Web 双后端自动降级实测）。
- **追记 3（零 cfg 封装）**：新增三个封装件吸收 Web 平台差异，示例 11 应用代码
  达到零 cfg——①`base::app::InitSlot<T>`（跨平台异步资源槽位：桌面阻塞跑完/
  Web spawn_local 排队，`get_mut()` 未就绪跳过）；②`starfish::web_entry!()` 宏
  （生成 cfg 化的 wasm_bindgen(start) 入口 + panic 转发，桌面展开为空；
  `console_error_panic_hook` 移入正式依赖供宏内置使用）；③`base::web::console_log`
  （wasm→控制台 / 桌面→stdout）。渲染帧循环的尺寸自愈为平台无关代码，
  两端共用。示例 11 已重写为零 cfg 规范范本。
- **追记 4（运行时安全审计 + 线程契约落地）**：全库审计 Web 上不可用的系统调用
  （thread/spawn/join、block_on、env、fs）的 cfg 覆盖——结论：音频线程族
  （stream 字段二选一/music retire/ring 测试线程）、block_on 三处、env::temp_dir
  （仅测试）均已正确覆盖，无需新增。**落地两项**：①`base/rt.rs` 主线程契约
  兜底正式实现（run 钉 OnceLock 锚点 + `debug_assert_main_thread` 接线
  RenderEntry::new / surface_from_context / RenderSurface::begin_frame/present/
  resize——调试构建 panic 定位跨线程误用，release 零成本；wasm 单线程恒过）；
  ②`process::exit` 加 wasm 排除 cfg（wasm 上会 trap 整个页面实例；当前因
  winit web run_app 永不返回而不可达，防御未来行为变化）。记录不 cfg 项：
  fs 在 Web 返回 Err（资源走内嵌/fetch 方案，见指南 3.2）；音频 autoplay
  为运行时行为非 cfg 范畴。
- **追记 5（启动门：事件先行，尺寸竞态从顺序上消除）**：`app.start` 从
  resumed（尺寸尚为 0×0）推迟到**首个有效窗口尺寸**到达后执行（about_to_wait
  启动门；等待期 rAF 轮询，60 帧超时按当前尺寸兜底）。顺序变为：事件循环先
  活起来 → Resized 携带真实尺寸 → GPU/服务以正确尺寸初始化——"Resized 先于
  资源就绪"的竞态从顺序上消除，帧循环尺寸自愈降级为兜底保险。trait 契约同步
  更新（start 在首个有效尺寸后调用；最早的事件可能先于 start 到达）。示例 11
  编号注释随之更新（[7] resumed / [8] Resized / [9] start / [10]-[12] 帧循环）。
  验证：41/41 + 无头 3/3 渲染回归全绿。
- **追记 6（多窗口 v1 落地）**：base 窗口层支持运行时多窗口——①`ctx.create_window(cfg)`
  返回 `InitSlot<Window>`（窗口创建需 ActiveEventLoop，仅回调内可得 → 登记请求、
  about_to_wait 物化，与资源惰性初始化同款模式）；②事件按窗路由（`event` 首参
  为窗口句柄）+ `window_created`/`window_closed` 钩子；③关闭语义：CloseRequested
  = 销毁该窗（应用在 event 里收尾该窗资源），最后一窗关闭 = 应用退出；④渲染侧
  零改动——每窗一个 `RenderSurface`（`surface_from_context` 共享设备既为多窗
  配套）。示例 12 演示双窗独立渲染。键鼠状态 v1 全局（最后聚焦窗），文档标注。
  验证：41/41 + 示例/wasm 0 error。
- **追记 7（video 模块 P1 落地 + 真机验证）**：`base/video` 三件套——①`mod.rs`：
  VideoModule（管理器，持 device/queue）+ Video 句柄状态机（update 手动泵 /
  set_video_enabled 遮挡开关 / texture() 帧纹理 / position / ended）；
  ②`yuv.rs`：NV12→RGBA 转换（BT.601 有限范围，纯函数 + 5 单测：黑白/中灰/
  alpha/4:2:0 块共享）；③`mf.rs`：MF SourceReader 后端（MFStartup 守卫配对 /
  SourceReader 选流 + NV12 输出重定向 / Lock 拷贝 / EOS 处理）。示例 13 离屏
  解码（无窗口：offscreen device + 手动泵 + 进度打印）。
- 真机数据（Windows 11，sample-5s.mp4 1920×1080 H.264，5.73s）：release 3.37s
  完成 344 泵全量解码+YUV→RGBA+纹理上传（**0.59× 实时，余量 1.7×**）；
  dev profile 17.2s（f32 标量转换未优化所致，v2 上 shader 消除）。
  纹理句柄同尺寸覆写稳定（绑定一次管到底），尺寸变化重建（version counter
  待办）。音轨注入待 mixer 流声部 API（§九-7）。
- **追记 8（video P1 真机验证 + 示例 13）**：示例 13 离屏解码验证（offscreen wgpu
  设备 + 手动泵 + 进度打印，无窗口依赖）。真机数据：1920×1080 H.264 5.73s →
  release **3.37s 完成全量解码+YUV→RGBA+纹理上传（0.59× 实时）**；dev 17.2s
  （f32 标量逐像素转换慢，v2 shader 化消除）。纹理同尺寸覆写稳定（绑定一次
  管到底）。视频呈现（YUV 采样管线）与音轨注入待后续（§九-7/待定项）。
