# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

本文件指导 Claude Code 在本仓库（starfish-rewrite）工作时的行为。

> ⚠️ 根目录 `README.md` 是**旧架构（SDL3 + 01~10 编号示例）的遗留文档**，
> 与本仓库 rewrite 现状不符（SDL3 已退役，示例仅剩 hello）——以本文件
> 与 `doc/PROJECT_STATUS.md` 为准。

## 项目概述

starfish-rewrite 是 **starfish 游戏引擎的主线**：基于 **wgpu 30 + 自研
starfish-window** 的 SDL 风格跨平台游戏库（"复刻 pygame"）。Rust 1.85+，
edition 2024。项目文档、代码注释、更新日志全部使用**中文**。

- **为什么重写**：旧架构（winit）是控制反转回调模型，引擎持循环，
  无法把运行循环交给脚本语言（pygame/RustPython 的 `while True` 世界观）；
  本仓库用自研 starfish-window（SDL 风格 poll 模型：调用方持循环 +
  `poll_event` 拉事件）替代 winit，运行循环归调用方——这是立项根因。
- **三平台**：Windows / Web(wasm32-unknown-unknown) / Android(arm64 真机)。
  hello 案例三平台纯绿已验收（2026-09-27）。
- **pygame 兼容层**：`src/pygame/`（契约见 `architecture/pygame层设计.md`
  定稿 v1.0+v1.3 修订）——GPU 路线 Surface、单一通用渲染管线（camera 即
  MVP）、接口形态对齐 pygame。**契约主线 P1~P5 全部落地**（含渲染会话
  `RenderPass`[with 语法]、color+depth、MRT、transform/mask/sndarray/
  touch、image.save 回读）。**依赖双向规则（硬性）：pygame 层可直用
  starfish 类型入接口；starfish（base/）永不引用 pygame 类型。**

## 仓库布局

```
├── src/base/            # 引擎（render/gfx/font/audio/video/window/time/...）
├── src/pygame/          # pygame 兼容层（契约 P1~P5 全落地；architecture/pygame层设计.md）
├── src/data/            # 引擎数据（colors.json 颜色表，供 pygame::color::init_colors 装载）
├── starfish-window/     # ★ 自研窗口/事件 crate（已 vendor 进仓库，src/base/window 的本体）
├── pygame-rs/           # ★ Python 绑定 crate（RP 嵌入三端全通；scripts/build_apk.py = 统一 APK 打包）
├── android/             # NativeActivity APK 清单模板（根示例用；pygame-rs/android/ 为其自持副本）
├── examples/            # hello + pygame 三件套（probe/hello/image_demo，均双 target 注册）
├── web/                 # wasm 产物 + 各示例 html + resize_probe.html（回归探针）
├── architecture/        # 设计契约（pygame层设计.md）；legacy/ 为旧架构基线文档
├── doc/log/             # 每日更新日志（按批次记录，含历史）
├── reference/           # 平台问题定位文档 + 旧设计稿
├── pygame reference/    # pygame/SDL 语义参考笔记（Surface 方案、mixer API 覆盖等，pygame 层设计输入）
└── doc/PROJECT_STATUS.md# ★ 进度快照与下一步计划（先读这个！）
```

## 常用命令

```bash
cargo build                                  # 桌面构建（默认特性全包含）
cargo test --lib                             # 库测试（注：base::video 5 个真样本依赖测试
                                             #   因缺 resources/videos/sample-5s.mp4 失败，
                                             #   存量问题，样本补入 resources 后自愈）
cargo test --lib pygame::                    # 只跑 pygame 层测试（避开 video 存量失败）
cargo test --lib pygame::rect                # 跑单个模块；再加 <测试名> 片段跑单个测试
cargo run --example hello                    # 桌面跑 hello（绿窗）

# Web（wasm-bindgen-cli 版本必须与依赖树严格一致，当前 0.2.127，
#  不一致会报 "not compatible with version of the wasm-bindgen in the dependency tree"）：
cargo build --release --target wasm32-unknown-unknown --example hello
wasm-bindgen --out-dir web --target web target/wasm32-unknown-unknown/release/examples/hello.wasm
cd web && python -m http.server 8000         # http://localhost:8000/hello.html

# Android APK 统一打包（pygame-rs/scripts/build_apk.py，全 crate 共用；xtask 已退役）：
python pygame-rs/scripts/build_apk.py binding_probe      # pygame-rs 示例（默认 --dir pygame-rs）
python pygame-rs/scripts/build_apk.py hello --dir .      # 根 crate 示例
# 链路：cargo ndk（RP 恒 release）→ strip → aapt2 重造 base（lib_name 对齐示例名）
#   → 组装（dex+.so DEFLATED）→ zipalign → 签名；产物 <crate>/target/android-apk/<名>_android.apk
# 手动兜底链与排障：reference/安卓APK打包lib_name坑与真机诊断手册.md §二
# 模拟器冒烟必须 x86_64 原生包：-t x86_64（arm64 包会被 berberis 翻译层跑，结果不可信）
```

## 关键机制

### 入口统一（app_entry! 宏，src/lib.rs）

```rust
starfish::app_entry!(app_body());   // 或块形态 app_entry!({ ... });
```

三平台驱动器内化：桌面 `block_on`+exit / web `#[wasm_bindgen(start)]`+
`spawn_local`+panic hook / Android `android_main→android_init→main` 桥接。
驱动器经 `base::app`（block_on/spawn_local 再导出）`$crate` 化，用户
crate 零新增依赖；唯一约束：web 构建需用户 crate 可见 wasm-bindgen。

### 循环模型（starfish-window，SDL 风格 poll）

调用方持循环：`Window::builder().build()` → `while` 循环内
`window.poll_event()` 拉事件 → `surface.begin_frame/present` 上屏。
无控制反转、无回调。Android 的系统回调事件由 starfish-window 内部
"回调写入、poll 排空"翻转成队列。

### 特性（features）

默认 `gfx/font/video/gamepad/dialog/net/io` 全包含，可裁剪；audio/渲染/
窗口/循环/时间为核心恒编译（同旧引擎体系，见 Cargo.toml 注释）。

## 坑位速查（都是实测踩过的，改动前先读）

1. **Android 模拟器 GLES 死穴**：wgpu GLES 后端在模拟器上
   `Surface::configure` 恒报 `Invalid surface`（新旧架构同败，环境层
   问题）——**渲染验收只能真机**；模拟器只验逻辑层（且必须 x86_64 包）。
2. **web canvas HiDPI**：canvas 无 CSS 尺寸时布局=width 属性，"属性=
   client×dpr"会正反馈爆炸（DevTools 停靠/缩放窗口触发）——已修
   （starfish-window 钉内联 CSS 尺寸），回归探针 `web/resize_probe.html`，
   headless 复现 HiDPI 问题必须加 `--force-device-scale-factor`。
3. **虚拟时间冻结 delta**：headless `--virtual-time-budget` 只适用
   纯渲染静态验证；时间类验证用存活模式收 console。
4. **wasm 诊断输出必须走 `console_log`**（println! 无处可去，见
   `base::debug`）；Android 日志 `adb logcat -s RustStdoutStderr`。
5. **headless 截图 `--screenshot` 用绝对路径**（相对路径落 Edge 版本目录）。
6. **Android 示例双注册约定**：桌面示例 `<名>` + `[[example]] <名>_android`
   （crate-type cdylib）成对注册，打包脚本 build_apk.py 靠 `<名>_android`
   兄弟条目解析——只注册单边会被拒绝。
7. **headless 两连坑**：`--disable-gpu` 会让 WebGPU `requestAdapter` 恒败
   （GPU 类验证严禁加）；首跑黑屏多为新管线编译竞态——**复跑一次再排查**
   （`--enable-logging=stderr` 收 console 是 web 唯一可见诊断通道）。
8. **Web 循环必须每帧 `next_frame().await` 帧拍**：漏写 = 同步死循环
   冻结渲染进程（无报错、画面全黑）；应用 future 在 web 上不要完成
   （驻留），否则迟到的微任务会踩已 drop 的绑定闭包（console 报
   closure invoked recursively）。

详见 `reference/`（web渲染不绿 / web画布尺寸正反馈爆炸 / android闪退与
Invalid surface 三份定位文档 + 旧架构全套设计笔记）。

## 文档惯例

- **每日更新日志**：`doc/log/starfish_changelog_YYYY-MM-DD.md`，按批次
  记录（设计背景/方案/关键保证/测试状态）；批次收尾动作 = 更新日志 +
  同步 architecture 文档。
- **进度快照**：`doc/PROJECT_STATUS.md`——当前阶段、下一步计划、关键
  文件指针；接手工作先读它。
- `reference/` 放设计讨论与技术选型笔记；`architecture/` 放设计契约与
  模块架构（legacy/ 是旧 winit 架构基线，供对照）；`pygame reference/`
  放 pygame/SDL 语义与 API 对照笔记（pygame 层实现时的语义输入）。
- `pygame-rs`（Python 绑定 crate）**尚未带入本仓库**——它当前绑定旧
  引擎，待 pygame 层成熟后迁移重绑。
