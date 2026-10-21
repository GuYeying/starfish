# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

**starfish**：SDL 风格 pull 模型的跨平台游戏引擎（Rust 1.85+ / edition 2024 /
wgpu 30），目标"复刻 pygame"。三端：Windows / Web（wasm32-unknown-unknown）/
Android（arm64 真机）。项目文档、代码注释、更新日志**全部使用中文**。

2026-10-21 起本仓库为 cargo workspace，原单 crate 按其 feature 依赖关系拆分为
14 个引擎 crate + examples（见下）。拆分前的单 crate 工作指南（含完整"坑位
速查"）归档于 `doc/CLAUDE-engine.md`，仍然有效。

## 仓库布局

```
crates/
  starfish-std       # 核心共用：error / debug（console_log）/ app（block_on/spawn_local）
                     #   + app_entry! 入口宏；feature "we"（Android 宿主装配再导出）
  starfish-we        # 窗口/事件（SDL 风格 poll：Window::builder → poll_event → surface 上屏）
                     #   feature：we-wgpu（rwh 直通）/ wgpu-auto（自动 resize surface）
  starfish-time  starfish-permission  starfish-io
  starfish-net       # →permission        starfish-dialog  # →permission
  starfish-gamepad   # gilrs / Web Reflect 轮询
  starfish-render    # wgpu 封装（bind_group/mesh/pipeline/render_pass）+ resources（图片装载）
  starfish-gfx  starfish-font  starfish-audio（feature "dialog"=录音保存，默认开）
  starfish-video     # 平台硬解（MF/GStreamer/VideoToolbox/WebCodecs）+ yuv 转换并入
  starfish-pygame    # pygame 兼容层（契约 = architecture/pygame层设计.md，P1~P5 全落地）
examples/            # 示例 + 25 案例三端自检套件（[[example]] 双注册含 _android；
                     #   android/ APK 清单模板 + jniLibs/ + web/ wasm 产物 + resources/ 运行期资产）
doc/                 # PROJECT_STATUS.md（接手先读）+ log/ 批次日志 + CLAUDE-engine.md（归档）
architecture/        # pygame层设计.md 契约定稿；legacy/ 旧架构基线
reference/、pygame reference/   # 设计笔记与 pygame/SDL 语义参考
scripts/             # capture_window.ps1 等开发辅助脚本
```

依赖 DAG（严格单向，无环）：`std ← we ← render ← {gfx,font}`；
`std ← {time,permission,io}`；`permission ← {net,dialog}`；`dialog ←(可选) audio`；
`{std,audio} ← video`；全部 → `starfish-pygame` → `examples`。

## 常用命令

```bash
cargo check --workspace                 # 全工作区类型检查
cargo test --workspace                  # 全部测试（video 5 个缺样本失败为存量）
cargo test -p starfish-pygame           # pygame 层测试（原 cargo test --lib pygame::）
cargo test -p starfish-pygame pygame::rect    # 单模块；再追加测试名片段跑单个测试
cargo run -p examples --example hello         # 桌面跑 hello（绿窗）
cargo run -p examples --example pygame_test_rect   # pygame 自检案例（横幅全绿）

# Web（wasm-bindgen-cli 版本必须与依赖树严格一致，当前 0.2.127，
#  不一致会报 "not compatible with version of the wasm-bindgen in the dependency tree"）：
cargo build --release --target wasm32-unknown-unknown -p examples --example hello
wasm-bindgen --out-dir examples/web --target web target/wasm32-unknown-unknown/release/examples/hello.wasm
python -m http.server 8000 --directory examples/web   # → http://localhost:8000/hello.html

# Android APK：统一打包脚本随 pygame-rs 仓库（D:/Projects/Rust/pygame-rs/scripts/
# build_apk.py，本仓库未随迁）；模拟器冒烟必须 x86_64 包，渲染验收只能真机。

# 本机注意：链接类操作（cargo build/run/test）须在 VS 开发者环境中执行——
# 裸 bash 下 link.exe 会误解析到 Git 自带 GNU link。助手脚本：
# scripts\vs-cargo.bat <cargo 参数…>   （内部 call VsDevCmd.bat 再跑 cargo）
```

## 裁剪（原 feature 场景化裁剪的 crate 化等价物）

按需依赖 sub-crate 即达裁剪：轻量逻辑层 = `starfish-std` + `starfish-time` +
`starfish-io`；带窗渲染 = 加 `starfish-we`（feature `wgpu-auto`）+ `starfish-render`；
完整 pygame 体验 = `starfish-pygame`（拉全部）。保留的 feature：
`starfish-we{we-wgpu, wgpu-auto}`、`starfish-std{we}`、`starfish-audio{dialog}`。

## 关键机制

- **入口统一**：`starfish_std::app_entry!(app_body());`（或块形态
  `app_entry!({ ... })`）——三平台驱动器内化：桌面 block_on+exit / Web
  `#[wasm_bindgen(start)]`+spawn_local+panic hook / Android android_main 桥接
  （Android 构建需 `starfish-std` feature `we`）。Web 构建要求用户 crate 可见
  wasm-bindgen（examples 的 dev-deps 提供）。
- **循环模型**：调用方持 while 循环 + `Window::poll_event()` 拉事件；无控制
  反转、无回调；运行循环归调用方（pygame/RustPython 的 `while True` 世界观）。
- **依赖双向红线（硬性）**：pygame 层可直用各引擎 crate 类型；引擎 crate
  永不引用 pygame 层。

## 坑位速查（实测踩过，改动前先读；完整版见 doc/CLAUDE-engine.md）

1. **Android 模拟器 GLES 死穴**：wgpu GLES 后端 `Surface::configure` 恒报
   Invalid surface——**渲染验收只能真机**；模拟器只验逻辑层（且必须 x86_64 包，
   arm64 包走翻译层结果不可信）。
2. **web canvas HiDPI**：已修（we 钉内联 CSS 尺寸）；回归探针
   `examples/web/resize_probe.html`；headless 复现必须 `--force-device-scale-factor`。
3. **wasm 诊断必须走 `starfish_std::debug::console_log`**（println! 无处可去）；
   Android 日志 `adb logcat -s RustStdoutStderr`。
4. **headless**：`--screenshot` 用绝对路径；`--disable-gpu` 会让 WebGPU
   requestAdapter 恒败（严禁加）；首跑黑屏多为管线编译竞态——复跑一次再排查。
5. **Web 循环必须每帧 `next_frame().await` 帧拍**：漏写 = 同步死循环冻结页面
   （无报错）；应用 future 在 web 上不要完成（驻留）。
6. **Android 示例双注册约定**：桌面 `<名>` + `[[example]] <名>_android`
   （cdylib）成对注册，打包脚本靠 `<名>_android` 兄弟条目解析——只注册单边
   会被拒绝。
7. **Linux 构建系统依赖**：dialog→libgtk-3-dev、we→pkg-config+libxkbcommon-dev、
   video→gstreamer dev 包；Windows 主机无法交叉检查 Linux target（Linux 编译
   验证由 CI linux.yml 承担）。
8. 本仓库 `.cargo/config.toml` 走 rsproxy 镜像（直连 crates.io 不通时），
   仅影响本仓库，可按需删除。

## 文档惯例

- **每日更新日志**：`doc/log/starfish_changelog_YYYY-MM-DD.md`，按批次记录
  （设计背景/方案/关键保证/测试状态）；批次收尾 = 更新日志 + 同步
  architecture 文档。
- **进度快照**：`doc/PROJECT_STATUS.md`——当前阶段、下一步计划、关键文件
  指针；接手工作先读它。
- `doc/CLAUDE-engine.md`：拆分前的单 crate 工作指南（完整坑位速查、批次
  历史说明），仍具参考价值。
