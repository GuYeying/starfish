# ⭐ Starfish

> **底层跨平台 crate 库** —— 纯 Rust、零 Python 资产、调用方持主循环
> （pull 模型）。
> [pygamest](https://github.com/GuYeying/pygamest)（次世代 pygame）体系的
> **地基层**：这一层只提供可独立使用的引擎能力 crate；游戏开发的 pygame
> 形态见 pygamest 仓。

[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](https://www.rust-lang.org)
[![wgpu](https://img.shields.io/badge/wgpu-30.0-brightgreen.svg)](https://wgpu.rs/)

## 📐 设计定约

- **零 Python 资产**：除 `starfish-pygame`（上层形态的承载 crate）外，
  所有 crate 不含任何 Python 概念——`starfish-pygame → 引擎 crate` 单向
  依赖，引擎永不感知上层
- **pull 模型**：调用方持主循环——窗口事件主动轮询（starfish-we 的 SDL
  风格 poll），无回调框架、无隐藏线程边界
- **加密红线**：资源加密唯一归属 `starfish-pygame::sfx`，其余 crate
  **零加密语义**（starfish-io 只做字节搬运）
- **三平台**：Windows 桌面 / Web（wasm32）/ Android（鸿蒙 NEXT + 卓易通
  容器实测）

## 🧩 crate 矩阵（15 crates）

| crate | 职责 |
|---|---|
| [starfish-we](crates/starfish-we) | 跨平台窗口 + 事件（SDL 风格 poll 模型；multi-window feature）+ `app_entry!` 协作 |
| [starfish-render](crates/starfish-render) | wgpu 渲染封装：bind_group / mesh / pipeline / render_pass / shader_module / RenderEntry / Surface + 资源装载 |
| [starfish-gfx](crates/starfish-gfx) | 几何绘制：16 种 2D/3D 形状生成器（形状参数 = 物理碰撞原语），填充/描边双管线 |
| [starfish-font](crates/starfish-font) | 字体：自研扫描线光栅化 + 图集 + kern 字距 + pos3 顶点（零新增依赖） |
| [starfish-audio](crates/starfish-audio) | 音频引擎：cpal 设备层 + symphonia 解码 + 混音器 / 流式 BGM / 录音 / 效果器链 |
| [starfish-video](crates/starfish-video) | 视频硬解抽象（MF / GStreamer / VideoToolbox / WebCodecs，NV12→RGBA；无硬解直接报错） |
| [starfish-time](crates/starfish-time) | Clock / FixedTimestep / 节流（wasm 走 performance.now） |
| [starfish-io](crates/starfish-io) | 跨平台读写（native std::fs / web fetch no-store；纯字节搬运，零加密语义） |
| [starfish-net](crates/starfish-net) | TCP 消息连接（后台线程 + 4 字节分帧，轮询式）/ UDP 非阻塞轮询 |
| [starfish-permission](crates/starfish-permission) | 权限声明（Android normal 权限语义化） |
| [starfish-dialog](crates/starfish-dialog) | 文件打开/保存对话框（桌面 rfd / 移动 roibus / Web 下载形态） |
| [starfish-gamepad](crates/starfish-gamepad) | 手柄查询（gilrs） |
| [starfish-std](crates/starfish-std) | 公共地基：`app_entry!` 三平台入口宏 / Stopwatch / run_control |
| [starfish-debug](crates/starfish-debug) | 日志与诊断（分级 log + 慢调用哨兵 + Android 文件/TCP 通道） |
| [starfish-pygame](crates/starfish-pygame) | **唯一感知 Python 的 crate**：pygame 引擎面（pygamest 绑定的执行底座）+ 资源加密面 `sfx` |

## 🚀 示例

`examples/demos`（`-p demos` 装载）：

```bash
cargo run -p demos --example hello          # 最简窗口 + 清屏
cargo run -p demos --example triangle       # 三角形（wgpu 管线最面）
cargo run -p demos --example multi_window   # 多窗口
cargo run -p demos --example pygame_image_demo
```

`pygame_test_*` 家族 = 逐模块验收锚点（display/event/io/color/time/
surface/mask/key/mouse/math…，桌面与 Android 双形态）。

## 🛠️ 技术栈

| 组件 | 技术 | 版本 |
|------|------|------|
| 图形 API | [wgpu](https://wgpu.rs/) | 30.0 |
| 数学 | [glam](https://docs.rs/glam/) | 0.33 |
| 字体解析 | [ttf-parser](https://docs.rs/ttf-parser/) | 0.25 |
| 音频设备 | [cpal](https://docs.rs/cpal/) | — |
| 音频解码 | [symphonia](https://docs.rs/symphonia/) | 0.5 |
| 纹理加载 | [image](https://docs.rs/image/) | 0.25 |
| 物理调试 | [rapier-starfish-debug](https://github.com/GuYeying/rapier-starfish-debug) | rapier 0.36 |
| 序列化 | [serde](https://serde.rs/) | 1.0 |
| 内存映射 | [bytemuck](https://docs.rs/bytemuck/) | 1.23 |

## 🧩 生态仓库矩阵

| 仓库 | 角色 |
|---|---|
| [starfish](https://github.com/GuYeying/starfish) | 引擎 crate 库（本仓，纯 Rust，零 Python 资产） |
| [pygamest](https://github.com/GuYeying/pygamest) | Python 绑定层（RustPython 内嵌 + 原生 pygame 面——次世代 pygame 的 pygame 形态） |
| [pygamest-dev](https://github.com/GuYeying/pygamest-dev) | 开发解释器（python.exe 形态，零 Rust 工具链） |
| [pygamest-runtime](https://github.com/GuYeying/pygamest-runtime) | 游戏运行时（玩家单元，入口加密嵌入 + 散文件清单） |
| [egui-starfish-we](https://github.com/GuYeying/egui-starfish-we) | egui × starfish-we 适配（vendored egui-winit fork） |
| [rapier-starfish-debug](https://github.com/GuYeying/rapier-starfish-debug) | Rapier 物理适配 + 通用 debug 渲染 |

---

#### 致特别的人--星

---

## 📄 许可证
Apache License 2.0 © 2026 Starfish Lib Authors
MIT License © 2026 Starfish Lib Authors
