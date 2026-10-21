# Starfish 更新日志 — 2026-09-07

> 接续 2026-09-06 批次。本批聚焦于**音频 DecoderPump 架构决策记录**、
> **Web/Emscripten 编译验证（spike）**与 **Web 跨平台条件编译机制**的设计说明。
> 状态：`cargo test` 41/41 通过，示例编译零错误，新增依赖 0。

---

## 📐 架构决策：DecoderPump 解码核心与驱动分离

### 设计背景

流式音频的解码逻辑原先**写死在后台线程的闭包里**——隐含前提是平台必须
支持 `std::thread`。浏览器（WASM）没有 `std::thread`，`thread::spawn`
在 wasm 上运行时不可用，导致流式音乐在 Web 上整个失效。

### 设计方案

将解码逻辑从线程闭包中提炼为独立的 `DecoderPump`
结构体（纯逻辑 + 环写入，无线程假设），由调用方驱动：

| 平台 | 驱动者 | 行为 |
|---|---|---|
| Windows / Linux / macOS | 后台解码线程（自动创建） | 拼命解码灌环，灌满即让位睡眠——native 行为零变化 |
| Web（emscripten） | 游戏循环每帧调 `pump_streams(budget)` | 每帧限预算解码（约 1ms），主线程零阻塞 |
| Android | 同 native（后台线程） | 完整支持 |

### 开发者接口（不变）

```rust
// 桌面：什么都不用做（线程自动维持缓冲，和以前完全一样）
mixer.music_load_file("bgm.ogg")?;

// Web：每帧加一行
mixer.pump_streams(2048);   // 预算 = 本帧最多解码的帧数
```

### 关键保证

1. native 行为零变化：线程驱动调用同一个 `pump_budget`，park / 命令 / EOF 语义原样保留
2. Web 零阻塞：pump 是非阻塞的有限工作，预算用完即返回，不会卡主线程
3. 性能可控：symphonia 解码约 20~50 倍实时速度，维持 2 秒缓冲每秒只需 ~5% 单核时间，摊到每帧约 0.4ms

---

## 📐 架构决策：cfg 条件编译的"一套接口两个行为"

### 机制说明

`MusicStream` 结构体的字段和行为在**编译期**按目标平台二选一：

```rust
pub struct MusicStream {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) worker: Option<std::thread::JoinHandle<()>>,  // native：后台线程
    #[cfg(target_arch = "wasm32")]
    pub(crate) pump: Option<worker::DecoderPump>,            // Web：解码泵
}
```

### 编译产物对比

| 目标 | 二进制中存在的路径 |
|---|---|
| 桌面（native） | `worker` 线程句柄，后台线程自动解码灌环 |
| Web（emscripten） | `pump` 解码泵，游戏循环驱动 |

**关键认知**：这不是运行时 if/else——是编译期就把另一个平台的实现整体从二进制里删掉。native 二进制里没有一行 web 代码，wasm 二进制里没有一行线程代码。

### 无条件编译的共用部分

DecoderPump 解码核心、环形缓冲、重采样器、效果器链、混音循环——双平台共用同一份代码。条件编译只出现在"谁来驱动"的边界上（共 10 处 cfg，全部集中在 `stream/mod.rs`）。

---

## 📊 状态

- 测试 **41/41** 通过，示例编译零错误，新增依赖 0

---

## 🧪 Web/Emscripten 编译验证（spike 结论）

### 环境要求

- emsdk 6.0.9（emcc 6.0.9 + Ninja + node 24.19.0）
- `activate_wasm.bat` 已固化：emsdk 环境 + CMake/Ninja 路径 + `CMAKE_TOOLCHAIN_FILE` + 生成器变量
- CC=emcc CXX=em++（显式指定，避免 MSYS 路径污染）

### 构建链验证结果

| 层 | 结果 | 说明 |
|---|---|---|
| SDL3 C 库 × emscripten | ✅ | CMake 配置成功（Platform: Emscripten-1），全子系统 ON |
| sdl3 Rust 封装 | ❌→✅ | `raw_window_handle.rs:118` u64→c_ulong 已通过 vendor 补丁（`window as _`）修复 |
| wgpu 全库编译 | ✅ | 移除 wasm32 段 webgpu/webgl 特性后通过（那些特性拉入 wasm-bindgen 链） |
| 链接 | ⚠️ | 需 `-sERROR_ON_UNDEFINED_SYMBOLS=0` + `-lEGL` + `-lGL` + `--js-library egl_stub.js` |
| 产物 | ✅ | .js (873KB) + .wasm (4.6MB) 已部署至 `web/` 目录 |

### SDL3 × emscripten 编译链要求（已固化至 activate_wasm.bat）

| 环境变量 | 值 |
|---|---|
| `CMAKE_TOOLCHAIN_FILE_wasm32_unknown_emscripten` | `D:/Toolchains/emsdk/upstream/emscripten/cmake/Modules/Platform/Emscripten.cmake` |
| `CMAKE_GENERATOR_wasm32_unknown_emscripten` | `Ninja` |
| `CC` / `CXX` | `emcc` / `em++` |
| PATH 追加 | emsdk root + upstream/emscripten + VS Ninja + CMake/bin + emsdk node |

### 根因分析：wasm-bindgen 占位导入

wgpu 30.0.0 对 wasm32 目标无条件依赖 `wasm-bindgen` / `web-sys` / `js-sys`（Cargo.toml 的 wasm32 段声明）。
这些依赖编译时产生的 `__wbindgen_placeholder__` 等运行时符号需要 wasm-bindgen CLI 后处理来解析——
但 wasm-bindgen CLI 不支持 emscripten 构建的 wasm 产物。

**结论**：SDL3 × emscripten 构建链完全可行，但 wgpu 的 GLES 后端（glow crate）在 wasm32 目标上
无条件引入 wasm-bindgen 运行时——导致 emscripten 构建的 wasm 包含无法解析的导入符号。
这是 wgpu × glow 生态的已知限制，非项目代码问题。
