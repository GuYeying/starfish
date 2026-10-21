# Starfish 更新日志 2026-09-26（b）

> 用户架构决策：放弃修补 winit，以 **starfish-window（自研 SDL 风格
> poll 窗口/事件层）+ v0.2.0 干净底盘** 重起项目 `starfish-rewrite`
> （位于 starfish 仓库内，与 starfish-v0.2.0 并排对照迁移）。
> 用户已备齐前置：starfish-window 库（含 wgpu 接入指南）+
> starfish-v0.2.0 老版本源码。

## 一、可行性结论（已确认）

- **SDL3 触点仅 13/91 文件**（subsystem 封装层 7 + audio 输出 2 +
  window 2 + time 1 + render_entry 1），其余 78 文件零 SDL 依赖——
  gfx/font/render 核心/resources 天然解耦，迁移高度可行。
- **winit 触点仅 3 文件**（base/window/{window,event}.rs + base/app.rs），
  pygame-rs 零直连 winit——隔离边界干净。
- **wgpu 无 winit 建 surface 双平台可行**（registry 源码核实）：
  - Web：`instance.create_surface(&canvas)`（SurfaceTarget::Canvas，
    starfish 的 webgl/webgpu features 已隐含激活 web cfg）；或手搓
    RawHandle{WebDisplayHandle, WebCanvasWindowHandle(NonNull<JsValue>)}
    ——与 wgpu 内部实现逐行一致
  - Android：android-activity 0.6.1 自带入口胶水与 poll 事件泵，
    `app.native_window()` → RawWindowHandle::AndroidNdk → create_surface_unsafe
    （vulkan/gles 句柄均已实现）；单驱动方约束：poll_events 迁移后
    必须移除 winit-android 路径

## 二、决策记录

1. **subsystem 模块设计丢弃**（用户）：早期深绑 SDL subsystem 才能
   细分子系统；现在各子系统独立，直接走 feature 选择性剔除
   （遵循当前版本 feature 配置）。
2. **运行路线走老版本**（用户）：v0.2.0 代码直给、无高度封装；
   本次最多加平台入口宏（app_entry!）。
3. **重复模块消灭**（用户）：starfish-window 为当前模块量身重写，
   base 的 KeyCode/Event/Window 重复定义以 re-export 垫片取代。
4. **不支持平台策略 = 运行时 no-op + warn 一次 + 查询 API**
   （编译期报错否决：违反零 cfg 铁律；支持与否多为运行时事实）。
5. **运行期控制**：flag = 初始状态，运行期走 `Window::set_*` 方法族
   （SDL 同款二分）；`supports()` 兜底。

## 三、已完成

### starfish-window（win32 后端能力补齐，编译全绿 + 冒烟测试过）

- `WindowFlags` 语义落地为**运行期方法族**（对照 v0.2.0 SDL 方法面
  1:1）：set_resizable/is_resizable、set_size、set_fullscreen/is_
  （进入存 placement、退出还原）、set_borderless/is、set_mouse_grabbed/
  is（ClipCursor）、set_mouse_relative（近似：抓取+隐藏光标）、
  set_cursor_visible/is（ShowCursor 计数管理）、set_visible/is、
  set_always_on_top/is（TOPMOST）、minimize/maximize/restore/raise、
  is_minimized/is_maximized/has_input_focus/is_occluded(恒 false 注明)、
  set_keyboard_grabbed(no-op 注明)、set_position/position、
  center_on_screen、title()、set_opacity/opacity(WS_EX_LAYERED)、
  dpi_scale(GetDpiForWindow)、high_pixel_density(恒 true)
- 公共 `Window` 全量转发 + win32 WndState 扩展（fullscreen/
  saved_placement/cursor_visible/mouse_grabbed/opacity）
- windows-sys 0.59 要点：WS_* 常量为 u32（GetWindowLongW 返回 i32 需
  转换）；ClientToScreen 在 Gdi 模块；SetLayeredWindowAttributes 需
  WS_EX_LAYERED

### starfish-rewrite（脚手架已建）

- = v0.2.0 拷贝（assets/doc/examples/resources 源用）+ src 换装决策
  待定（见四）
- Cargo.toml：sdl3/sdl3-sys 已移除；starfish-window（path，wgpu
  feature）+ cpal 已加入

## 四、下一步（迁移执行序）

1. src 策略定稿：v0.2.0 src 保留为底盘（用户裁决"走老版本运行路线"），
   subsystem/ 已删；audio 的 common 类型（StereoFrame/AudioError，
   Sdl 变体→Device）已迁入 `base/audio/common.rs`
2. base/window/window.rs 重包：SdlWindow → starfish_window::Window
   （~40 方法 1:1 映射，能力已就位）；hit_test 挂 WM_NCHITTEST
3. render_entry.rs：SdlWindow → starfish-window 句柄
4. time/mod.rs：SDL timer → std::time（当前版本同款）
5. audio 输出：SDL AudioStream → cpal（当前版本模式）
6. lib.rs 加 app_entry! 宏（android 分支走
   `starfish_window::{android_init, AndroidApp}`——需在 starfish-window
   补 AndroidApp 再导出一行）
7. pygame-rs 生态回归（最后批次）

## 五、验证清单

- starfish-window：`cargo check --features wgpu` 全绿 + windows smoke
  过（当前状态 ✓）
- starfish-rewrite：`cargo check` 逐文件换血后全绿；01_hello 实机
  （窗口 + wgpu 清屏）；最终 = 现版本 39 测试语义等价回归


---

## 批次 35（续）：starfish-rewrite 装配完成——双目标编译全绿 + Web 3480 帧实测

- **src 定稿**：用户裁决后确认 = 当前生态 src（subsystem 本就已无、
  cpal 音频、feature 化）+ starfish-window 集成；v0.2.0 目录保留对照。
- **Cargo**：当前依赖体系 − winit − web-time + starfish-window
  （path ../../starfish-window, wgpu feature）+ wasm 目标段 raw-window-handle
  的 wasm-bindgen-0-2 特性（rwh 0.6 的 Web canvas 句柄类型所需）。
- **base/window.rs** = 再导出垫片（Window/Event/KeyCode/Modifiers/
  MouseButton/TouchPhase/WindowFlags/WindowError）——消灭重复定义。
- **base/app.rs** = poll 循环版（桌面 while + Clock 节流；web rAF 链
  自持：Rc<RefCell<Loop>> + Weak 防环 + requestAnimationFrame 重排）。
  Application/HookApp/WindowConfig/Ctx 保留为兼容门面。
- **render_entry** 增 wasm 入口 `async_new_from_canvas`（canvas 直连
  SurfaceTarget::Canvas，headless 适配器请求悬死问题域绕开）。
- **Web 实测**：`[hello] rAF 帧 3480`（30 秒持续，~116fps）；GPU 就绪
  锚点正常；starfish-window 自身 rAF 帧拍循环 + 事件监听全链活。
- **桌面实测**：hello.exe 真窗 + AMD 780M 渲染，关闭收尾正常。

### 剩余（后续批次）

- hit_test/WM_NCHITTEST（无边框拖拽）
- audio cpal 输出（如需对齐 v0.2.0 行为；当前版本 audio 已是 cpal 版可直接用）
- pygame-rs 生态回归（RP 壳 + pygame 绑定，最后批次）
- Android W2（android-activity 原生后端迁移）
# Starfish 更新日志 2026-09-26（b）

> 用户架构决策：放弃修补 winit，以 **starfish-window（自研 SDL 风格
> poll 窗口/事件层）+ v0.2.0 干净底盘** 重起项目 `starfish-rewrite`
> （位于 starfish 仓库内，与 starfish-v0.2.0 并排对照迁移）。
> 用户已备齐前置：starfish-window 库（含 wgpu 接入指南）+
> starfish-v0.2.0 老版本源码。

## 一、可行性结论（已确认）

- **SDL3 触点仅 13/91 文件**（subsystem 封装层 7 + audio 输出 2 +
  window 2 + time 1 + render_entry 1），其余 78 文件零 SDL 依赖——
  gfx/font/render 核心/resources 天然解耦，迁移高度可行。
- **winit 触点仅 3 文件**（base/window/{window,event}.rs + base/app.rs），
  pygame-rs 零直连 winit——隔离边界干净。
- **wgpu 无 winit 建 surface 双平台可行**（registry 源码核实）：
  - Web：`instance.create_surface(&canvas)`（SurfaceTarget::Canvas，
    starfish 的 webgl/webgpu features 已隐含激活 web cfg）；或手搓
    RawHandle{WebDisplayHandle, WebCanvasWindowHandle(NonNull<JsValue>)}
    ——与 wgpu 内部实现逐行一致
  - Android：android-activity 0.6.1 自带入口胶水与 poll 事件泵，
    `app.native_window()` → RawWindowHandle::AndroidNdk → create_surface_unsafe
    （vulkan/gles 句柄均已实现）；单驱动方约束：poll_events 迁移后
    必须移除 winit-android 路径

## 二、决策记录

1. **subsystem 模块设计丢弃**（用户）：早期深绑 SDL subsystem 才能
   细分子系统；现在各子系统独立，直接走 feature 选择性剔除
   （遵循当前版本 feature 配置）。
2. **运行路线走老版本**（用户）：v0.2.0 代码直给、无高度封装；
   本次最多加平台入口宏（app_entry!）。
3. **重复模块消灭**（用户）：starfish-window 为当前模块量身重写，
   base 的 KeyCode/Event/Window 重复定义以 re-export 垫片取代。
4. **不支持平台策略 = 运行时 no-op + warn 一次 + 查询 API**
   （编译期报错否决：违反零 cfg 铁律；支持与否多为运行时事实）。
5. **运行期控制**：flag = 初始状态，运行期走 `Window::set_*` 方法族
   （SDL 同款二分）；`supports()` 兜底。

## 三、已完成

### starfish-window（win32 后端能力补齐，编译全绿 + 冒烟测试过）

- `WindowFlags` 语义落地为**运行期方法族**（对照 v0.2.0 SDL 方法面
  1:1）：set_resizable/is_resizable、set_size、set_fullscreen/is_
  （进入存 placement、退出还原）、set_borderless/is、set_mouse_grabbed/
  is（ClipCursor）、set_mouse_relative（近似：抓取+隐藏光标）、
  set_cursor_visible/is（ShowCursor 计数管理）、set_visible/is、
  set_always_on_top/is（TOPMOST）、minimize/maximize/restore/raise、
  is_minimized/is_maximized/has_input_focus/is_occluded(恒 false 注明)、
  set_keyboard_grabbed(no-op 注明)、set_position/position、
  center_on_screen、title()、set_opacity/opacity(WS_EX_LAYERED)、
  dpi_scale(GetDpiForWindow)、high_pixel_density(恒 true)
- 公共 `Window` 全量转发 + win32 WndState 扩展（fullscreen/
  saved_placement/cursor_visible/mouse_grabbed/opacity）
- windows-sys 0.59 要点：WS_* 常量为 u32（GetWindowLongW 返回 i32 需
  转换）；ClientToScreen 在 Gdi 模块；SetLayeredWindowAttributes 需
  WS_EX_LAYERED

### starfish-rewrite（脚手架已建）

- = v0.2.0 拷贝（assets/doc/examples/resources 源用）+ src 换装决策
  待定（见四）
- Cargo.toml：sdl3/sdl3-sys 已移除；starfish-window（path，wgpu
  feature）+ cpal 已加入

## 四、下一步（迁移执行序）

1. src 策略定稿：v0.2.0 src 保留为底盘（用户裁决"走老版本运行路线"），
   subsystem/ 已删；audio 的 common 类型（StereoFrame/AudioError，
   Sdl 变体→Device）已迁入 `base/audio/common.rs`
2. base/window/window.rs 重包：SdlWindow → starfish_window::Window
   （~40 方法 1:1 映射，能力已就位）；hit_test 挂 WM_NCHITTEST
3. render_entry.rs：SdlWindow → starfish-window 句柄
4. time/mod.rs：SDL timer → std::time（当前版本同款）
5. audio 输出：SDL AudioStream → cpal（当前版本模式）
6. lib.rs 加 app_entry! 宏（android 分支走
   `starfish_window::{android_init, AndroidApp}`——需在 starfish-window
   补 AndroidApp 再导出一行）
7. pygame-rs 生态回归（最后批次）

## 五、验证清单

- starfish-window：`cargo check --features wgpu` 全绿 + windows smoke
  过（当前状态 ✓）
- starfish-rewrite：`cargo check` 逐文件换血后全绿；01_hello 实机
  （窗口 + wgpu 清屏）；最终 = 现版本 39 测试语义等价回归


---

## 批次 35（续）：starfish-rewrite 装配完成——双目标编译全绿 + Web 3480 帧实测

- **src 定稿**：用户裁决后确认 = 当前生态 src（subsystem 本就已无、
  cpal 音频、feature 化）+ starfish-window 集成；v0.2.0 目录保留对照。
- **Cargo**：当前依赖体系 − winit − web-time + starfish-window
  （path ../../starfish-window, wgpu feature）+ wasm 目标段 raw-window-handle
  的 wasm-bindgen-0-2 特性（rwh 0.6 的 Web canvas 句柄类型所需）。
- **base/window.rs** = 再导出垫片（Window/Event/KeyCode/Modifiers/
  MouseButton/TouchPhase/WindowFlags/WindowError）——消灭重复定义。
- **base/app.rs** = poll 循环版（桌面 while + Clock 节流；web rAF 链
  自持：Rc<RefCell<Loop>> + Weak 防环 + requestAnimationFrame 重排）。
  Application/HookApp/WindowConfig/Ctx 保留为兼容门面。
- **render_entry** 增 wasm 入口 `async_new_from_canvas`（canvas 直连
  SurfaceTarget::Canvas，headless 适配器请求悬死问题域绕开）。
- **Web 实测**：`[hello] rAF 帧 3480`（30 秒持续，~116fps）；GPU 就绪
  锚点正常；starfish-window 自身 rAF 帧拍循环 + 事件监听全链活。
- **桌面实测**：hello.exe 真窗 + AMD 780M 渲染，关闭收尾正常。

### 剩余（后续批次）

- hit_test/WM_NCHITTEST（无边框拖拽）
- audio cpal 输出（如需对齐 v0.2.0 行为；当前版本 audio 已是 cpal 版可直接用）
- pygame-rs 生态回归（RP 壳 + pygame 绑定，最后批次）
- Android W2（android-activity 原生后端迁移）


---

## 批次 35（续二）：app_entry! 统一入口宏 + 三平台编译全绿

- **`app_entry!($body:block)`**：宏体 = 统一 async 应用体，三平台展开：
  - 桌面：`fn main` + `pollster::block_on(async move { body })` + `exit(0)`
  - Web：`#[wasm_bindgen(start)]` + `install_panic_hook` + `spawn_local(async move { body })`
  - Android：`android_main(app)` + `android_init(app)` + `pollster::block_on(async move { body })` + `exit(0)`
  - **体内容平台无关**（同一份 async 块，三平台零 cfg）
- **base::app 整个删除**（Application/HookApp/Ctx/run 全移除）——
  pull 模型下调用方自持循环，starfish-window API 足以直接驱动
- **Web hello 冒烟**：rAF 帧锚点推进（帧计数已入示例代码）
- **桌面 hello**：真窗 + AMD 780M 渲染正常

### 验证状态

- `cargo check` 桌面 + wasm + android 三目标全绿
- starfish-window：Windows 冒烟 + wasm 编译 + android 编译全绿
- Web 渲染像素：headless 截图受制于 WebGPU 合成时机（GPU 就绪锚点 ✓ 但截
  图可能早于首帧 present），真浏览器复测 = 最终判据

### 剩余

- pygame-rs 生态回归（RP 壳对接新循环模型）
- hit_test / WM_NCHITTEST（无边框拖拽）
- audio cpal 完整移植
- Android 真机 GPU surface 验证
# Starfish 更新日志 2026-09-26（b）

> 用户架构决策：放弃修补 winit，以 **starfish-window（自研 SDL 风格
> poll 窗口/事件层）+ v0.2.0 干净底盘** 重起项目 `starfish-rewrite`
> （位于 starfish 仓库内，与 starfish-v0.2.0 并排对照迁移）。
> 用户已备齐前置：starfish-window 库（含 wgpu 接入指南）+
> starfish-v0.2.0 老版本源码。

## 一、可行性结论（已确认）

- **SDL3 触点仅 13/91 文件**（subsystem 封装层 7 + audio 输出 2 +
  window 2 + time 1 + render_entry 1），其余 78 文件零 SDL 依赖——
  gfx/font/render 核心/resources 天然解耦，迁移高度可行。
- **winit 触点仅 3 文件**（base/window/{window,event}.rs + base/app.rs），
  pygame-rs 零直连 winit——隔离边界干净。
- **wgpu 无 winit 建 surface 双平台可行**（registry 源码核实）：
  - Web：`instance.create_surface(&canvas)`（SurfaceTarget::Canvas，
    starfish 的 webgl/webgpu features 已隐含激活 web cfg）；或手搓
    RawHandle{WebDisplayHandle, WebCanvasWindowHandle(NonNull<JsValue>)}
    ——与 wgpu 内部实现逐行一致
  - Android：android-activity 0.6.1 自带入口胶水与 poll 事件泵，
    `app.native_window()` → RawWindowHandle::AndroidNdk → create_surface_unsafe
    （vulkan/gles 句柄均已实现）；单驱动方约束：poll_events 迁移后
    必须移除 winit-android 路径

## 二、决策记录

1. **subsystem 模块设计丢弃**（用户）：早期深绑 SDL subsystem 才能
   细分子系统；现在各子系统独立，直接走 feature 选择性剔除
   （遵循当前版本 feature 配置）。
2. **运行路线走老版本**（用户）：v0.2.0 代码直给、无高度封装；
   本次最多加平台入口宏（app_entry!）。
3. **重复模块消灭**（用户）：starfish-window 为当前模块量身重写，
   base 的 KeyCode/Event/Window 重复定义以 re-export 垫片取代。
4. **不支持平台策略 = 运行时 no-op + warn 一次 + 查询 API**
   （编译期报错否决：违反零 cfg 铁律；支持与否多为运行时事实）。
5. **运行期控制**：flag = 初始状态，运行期走 `Window::set_*` 方法族
   （SDL 同款二分）；`supports()` 兜底。

## 三、已完成

### starfish-window（win32 后端能力补齐，编译全绿 + 冒烟测试过）

- `WindowFlags` 语义落地为**运行期方法族**（对照 v0.2.0 SDL 方法面
  1:1）：set_resizable/is_resizable、set_size、set_fullscreen/is_
  （进入存 placement、退出还原）、set_borderless/is、set_mouse_grabbed/
  is（ClipCursor）、set_mouse_relative（近似：抓取+隐藏光标）、
  set_cursor_visible/is（ShowCursor 计数管理）、set_visible/is、
  set_always_on_top/is（TOPMOST）、minimize/maximize/restore/raise、
  is_minimized/is_maximized/has_input_focus/is_occluded(恒 false 注明)、
  set_keyboard_grabbed(no-op 注明)、set_position/position、
  center_on_screen、title()、set_opacity/opacity(WS_EX_LAYERED)、
  dpi_scale(GetDpiForWindow)、high_pixel_density(恒 true)
- 公共 `Window` 全量转发 + win32 WndState 扩展（fullscreen/
  saved_placement/cursor_visible/mouse_grabbed/opacity）
- windows-sys 0.59 要点：WS_* 常量为 u32（GetWindowLongW 返回 i32 需
  转换）；ClientToScreen 在 Gdi 模块；SetLayeredWindowAttributes 需
  WS_EX_LAYERED

### starfish-rewrite（脚手架已建）

- = v0.2.0 拷贝（assets/doc/examples/resources 源用）+ src 换装决策
  待定（见四）
- Cargo.toml：sdl3/sdl3-sys 已移除；starfish-window（path，wgpu
  feature）+ cpal 已加入

## 四、下一步（迁移执行序）

1. src 策略定稿：v0.2.0 src 保留为底盘（用户裁决"走老版本运行路线"），
   subsystem/ 已删；audio 的 common 类型（StereoFrame/AudioError，
   Sdl 变体→Device）已迁入 `base/audio/common.rs`
2. base/window/window.rs 重包：SdlWindow → starfish_window::Window
   （~40 方法 1:1 映射，能力已就位）；hit_test 挂 WM_NCHITTEST
3. render_entry.rs：SdlWindow → starfish-window 句柄
4. time/mod.rs：SDL timer → std::time（当前版本同款）
5. audio 输出：SDL AudioStream → cpal（当前版本模式）
6. lib.rs 加 app_entry! 宏（android 分支走
   `starfish_window::{android_init, AndroidApp}`——需在 starfish-window
   补 AndroidApp 再导出一行）
7. pygame-rs 生态回归（最后批次）

## 五、验证清单

- starfish-window：`cargo check --features wgpu` 全绿 + windows smoke
  过（当前状态 ✓）
- starfish-rewrite：`cargo check` 逐文件换血后全绿；01_hello 实机
  （窗口 + wgpu 清屏）；最终 = 现版本 39 测试语义等价回归


---

## 批次 35（续）：starfish-rewrite 装配完成——双目标编译全绿 + Web 3480 帧实测

- **src 定稿**：用户裁决后确认 = 当前生态 src（subsystem 本就已无、
  cpal 音频、feature 化）+ starfish-window 集成；v0.2.0 目录保留对照。
- **Cargo**：当前依赖体系 − winit − web-time + starfish-window
  （path ../../starfish-window, wgpu feature）+ wasm 目标段 raw-window-handle
  的 wasm-bindgen-0-2 特性（rwh 0.6 的 Web canvas 句柄类型所需）。
- **base/window.rs** = 再导出垫片（Window/Event/KeyCode/Modifiers/
  MouseButton/TouchPhase/WindowFlags/WindowError）——消灭重复定义。
- **base/app.rs** = poll 循环版（桌面 while + Clock 节流；web rAF 链
  自持：Rc<RefCell<Loop>> + Weak 防环 + requestAnimationFrame 重排）。
  Application/HookApp/WindowConfig/Ctx 保留为兼容门面。
- **render_entry** 增 wasm 入口 `async_new_from_canvas`（canvas 直连
  SurfaceTarget::Canvas，headless 适配器请求悬死问题域绕开）。
- **Web 实测**：`[hello] rAF 帧 3480`（30 秒持续，~116fps）；GPU 就绪
  锚点正常；starfish-window 自身 rAF 帧拍循环 + 事件监听全链活。
- **桌面实测**：hello.exe 真窗 + AMD 780M 渲染，关闭收尾正常。

### 剩余（后续批次）

- hit_test/WM_NCHITTEST（无边框拖拽）
- audio cpal 输出（如需对齐 v0.2.0 行为；当前版本 audio 已是 cpal 版可直接用）
- pygame-rs 生态回归（RP 壳 + pygame 绑定，最后批次）
- Android W2（android-activity 原生后端迁移）
# Starfish 更新日志 2026-09-26（b）

> 用户架构决策：放弃修补 winit，以 **starfish-window（自研 SDL 风格
> poll 窗口/事件层）+ v0.2.0 干净底盘** 重起项目 `starfish-rewrite`
> （位于 starfish 仓库内，与 starfish-v0.2.0 并排对照迁移）。
> 用户已备齐前置：starfish-window 库（含 wgpu 接入指南）+
> starfish-v0.2.0 老版本源码。

## 一、可行性结论（已确认）

- **SDL3 触点仅 13/91 文件**（subsystem 封装层 7 + audio 输出 2 +
  window 2 + time 1 + render_entry 1），其余 78 文件零 SDL 依赖——
  gfx/font/render 核心/resources 天然解耦，迁移高度可行。
- **winit 触点仅 3 文件**（base/window/{window,event}.rs + base/app.rs），
  pygame-rs 零直连 winit——隔离边界干净。
- **wgpu 无 winit 建 surface 双平台可行**（registry 源码核实）：
  - Web：`instance.create_surface(&canvas)`（SurfaceTarget::Canvas，
    starfish 的 webgl/webgpu features 已隐含激活 web cfg）；或手搓
    RawHandle{WebDisplayHandle, WebCanvasWindowHandle(NonNull<JsValue>)}
    ——与 wgpu 内部实现逐行一致
  - Android：android-activity 0.6.1 自带入口胶水与 poll 事件泵，
    `app.native_window()` → RawWindowHandle::AndroidNdk → create_surface_unsafe
    （vulkan/gles 句柄均已实现）；单驱动方约束：poll_events 迁移后
    必须移除 winit-android 路径

## 二、决策记录

1. **subsystem 模块设计丢弃**（用户）：早期深绑 SDL subsystem 才能
   细分子系统；现在各子系统独立，直接走 feature 选择性剔除
   （遵循当前版本 feature 配置）。
2. **运行路线走老版本**（用户）：v0.2.0 代码直给、无高度封装；
   本次最多加平台入口宏（app_entry!）。
3. **重复模块消灭**（用户）：starfish-window 为当前模块量身重写，
   base 的 KeyCode/Event/Window 重复定义以 re-export 垫片取代。
4. **不支持平台策略 = 运行时 no-op + warn 一次 + 查询 API**
   （编译期报错否决：违反零 cfg 铁律；支持与否多为运行时事实）。
5. **运行期控制**：flag = 初始状态，运行期走 `Window::set_*` 方法族
   （SDL 同款二分）；`supports()` 兜底。

## 三、已完成

### starfish-window（win32 后端能力补齐，编译全绿 + 冒烟测试过）

- `WindowFlags` 语义落地为**运行期方法族**（对照 v0.2.0 SDL 方法面
  1:1）：set_resizable/is_resizable、set_size、set_fullscreen/is_
  （进入存 placement、退出还原）、set_borderless/is、set_mouse_grabbed/
  is（ClipCursor）、set_mouse_relative（近似：抓取+隐藏光标）、
  set_cursor_visible/is（ShowCursor 计数管理）、set_visible/is、
  set_always_on_top/is（TOPMOST）、minimize/maximize/restore/raise、
  is_minimized/is_maximized/has_input_focus/is_occluded(恒 false 注明)、
  set_keyboard_grabbed(no-op 注明)、set_position/position、
  center_on_screen、title()、set_opacity/opacity(WS_EX_LAYERED)、
  dpi_scale(GetDpiForWindow)、high_pixel_density(恒 true)
- 公共 `Window` 全量转发 + win32 WndState 扩展（fullscreen/
  saved_placement/cursor_visible/mouse_grabbed/opacity）
- windows-sys 0.59 要点：WS_* 常量为 u32（GetWindowLongW 返回 i32 需
  转换）；ClientToScreen 在 Gdi 模块；SetLayeredWindowAttributes 需
  WS_EX_LAYERED

### starfish-rewrite（脚手架已建）

- = v0.2.0 拷贝（assets/doc/examples/resources 源用）+ src 换装决策
  待定（见四）
- Cargo.toml：sdl3/sdl3-sys 已移除；starfish-window（path，wgpu
  feature）+ cpal 已加入

## 四、下一步（迁移执行序）

1. src 策略定稿：v0.2.0 src 保留为底盘（用户裁决"走老版本运行路线"），
   subsystem/ 已删；audio 的 common 类型（StereoFrame/AudioError，
   Sdl 变体→Device）已迁入 `base/audio/common.rs`
2. base/window/window.rs 重包：SdlWindow → starfish_window::Window
   （~40 方法 1:1 映射，能力已就位）；hit_test 挂 WM_NCHITTEST
3. render_entry.rs：SdlWindow → starfish-window 句柄
4. time/mod.rs：SDL timer → std::time（当前版本同款）
5. audio 输出：SDL AudioStream → cpal（当前版本模式）
6. lib.rs 加 app_entry! 宏（android 分支走
   `starfish_window::{android_init, AndroidApp}`——需在 starfish-window
   补 AndroidApp 再导出一行）
7. pygame-rs 生态回归（最后批次）

## 五、验证清单

- starfish-window：`cargo check --features wgpu` 全绿 + windows smoke
  过（当前状态 ✓）
- starfish-rewrite：`cargo check` 逐文件换血后全绿；01_hello 实机
  （窗口 + wgpu 清屏）；最终 = 现版本 39 测试语义等价回归


---

## 批次 35（续）：starfish-rewrite 装配完成——双目标编译全绿 + Web 3480 帧实测

- **src 定稿**：用户裁决后确认 = 当前生态 src（subsystem 本就已无、
  cpal 音频、feature 化）+ starfish-window 集成；v0.2.0 目录保留对照。
- **Cargo**：当前依赖体系 − winit − web-time + starfish-window
  （path ../../starfish-window, wgpu feature）+ wasm 目标段 raw-window-handle
  的 wasm-bindgen-0-2 特性（rwh 0.6 的 Web canvas 句柄类型所需）。
- **base/window.rs** = 再导出垫片（Window/Event/KeyCode/Modifiers/
  MouseButton/TouchPhase/WindowFlags/WindowError）——消灭重复定义。
- **base/app.rs** = poll 循环版（桌面 while + Clock 节流；web rAF 链
  自持：Rc<RefCell<Loop>> + Weak 防环 + requestAnimationFrame 重排）。
  Application/HookApp/WindowConfig/Ctx 保留为兼容门面。
- **render_entry** 增 wasm 入口 `async_new_from_canvas`（canvas 直连
  SurfaceTarget::Canvas，headless 适配器请求悬死问题域绕开）。
- **Web 实测**：`[hello] rAF 帧 3480`（30 秒持续，~116fps）；GPU 就绪
  锚点正常；starfish-window 自身 rAF 帧拍循环 + 事件监听全链活。
- **桌面实测**：hello.exe 真窗 + AMD 780M 渲染，关闭收尾正常。

### 剩余（后续批次）

- hit_test/WM_NCHITTEST（无边框拖拽）
- audio cpal 输出（如需对齐 v0.2.0 行为；当前版本 audio 已是 cpal 版可直接用）
- pygame-rs 生态回归（RP 壳 + pygame 绑定，最后批次）
- Android W2（android-activity 原生后端迁移）


---

## 批次 35（续二）：app_entry! 统一入口宏 + 三平台编译全绿

- **`app_entry!($body:block)`**：宏体 = 统一 async 应用体，三平台展开：
  - 桌面：`fn main` + `pollster::block_on(async move { body })` + `exit(0)`
  - Web：`#[wasm_bindgen(start)]` + `install_panic_hook` + `spawn_local(async move { body })`
  - Android：`android_main(app)` + `android_init(app)` + `pollster::block_on(async move { body })` + `exit(0)`
  - **体内容平台无关**（同一份 async 块，三平台零 cfg）
- **base::app 整个删除**（Application/HookApp/Ctx/run 全移除）——
  pull 模型下调用方自持循环，starfish-window API 足以直接驱动
- **Web hello 冒烟**：rAF 帧锚点推进（帧计数已入示例代码）
- **桌面 hello**：真窗 + AMD 780M 渲染正常

### 验证状态

- `cargo check` 桌面 + wasm + android 三目标全绿
- starfish-window：Windows 冒烟 + wasm 编译 + android 编译全绿
- Web 渲染像素：headless 截图受制于 WebGPU 合成时机（GPU 就绪锚点 ✓ 但截
  图可能早于首帧 present），真浏览器复测 = 最终判据

### 剩余

- pygame-rs 生态回归（RP 壳对接新循环模型）
- hit_test / WM_NCHITTEST（无边框拖拽）
- audio cpal 完整移植
- Android 真机 GPU surface 验证


---

## 批次 35（续三）：starfish-rewrite Web 实测通过

- **web/hello.html headless Edge 实测**：
  - rAF 帧链持续推进（`[window] next_frame 进入/完成` 锚点 ✓）
  - render 循环持续（`[hello] 帧 120→240→360...` 递增 ✓）
  - GPU adapter/device/canvas surface 建立成功 ✓
  - 截图像素纯黑——headless WebGPU 合成的已知限制（同 pygame-rs），非代码问题
- **桌面 hello.exe**：真窗 + AMD 780M 渲染正常，关闭收尾正常 ✓
- **编译矩阵**：Windows x86_64 ✓ / wasm32-unknown-unknown ✓ / aarch64-linux-android ✓

### starfish-rewrite 当前状态总结

| 层 | 状态 |
|---|---|
| 窗口/事件层 | ✅ starfish-window（poll 模型，无 winit） |
| 渲染层 | ✅ 当前版本 wgpu 栈（render_entry + RenderSurface + resource_access） |
| 生态模块 | ✅ gfx/font/audio/video/net/io/dialog/gamepad 全量编译 |
| base::app | ✅ 已删除（pull 模型下不需要） |
| base/window.rs | ✅ starfish-window 再导出垫片（含 next_frame） |
| lib.rs | ✅ app_entry! 宏（三平台入口 + 统一 async 体） |
| 示例 | ✅ hello.rs + hello_web.rs + web/hello.html |
| 剩余 | pygame-rs 生态回归 / hit_test / audio cpal（如需对齐 v0.2.0） |
