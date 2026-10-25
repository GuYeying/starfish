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
wasm-bindgen --out-dir target/web --target web target/wasm32-unknown-unknown/release/examples/hello.wasm
python -m http.server 8000 --directory target/web   # → http://localhost:8000/hello.html
# （产物入 target/web——构建产物不进源码树；pygame 23 案例批量产物在
#  target/web-pygame-test/，索引页 pygame_test_index.html）

# Android APK：统一打包脚本随 pygame-rs 仓库（D:/Projects/Rust/pygame-rs/scripts/
# build_apk.py，本仓库未随迁）；模拟器冒烟必须 x86_64 包，渲染验收只能真机。
# 产物统一入 target/android-apk（--out 指定）；classes.dex 复用件在
# target/classes.dex（历史 APK 兜底提取，robius 胶水与示例无关）。

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
- **多窗口**（feature `multi-window`，2026-10-23 定稿）：编译期平台白名单
  （当前仅 Windows，白名单外 `compile_error!`，不做运行期降级）；形态 =
  一个 `RenderContext` + N 个 `RenderSurface`（`RenderEntry::new_context` →
  `new_surface`，surface 之外一切不重建）；事件归属**被 poll 的窗口**
  （无 ID 路由），每帧须轮询每个窗口；surface 必须先于其窗口 drop。
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

---

## 主线迁移与承接记忆（自 D:/Projects/Rust/starfish 合并，2026-10-22 由 Claude 同步）

- **仓库关系**：本仓库 = 原 `D:/Projects/Rust/starfish`（单 crate 主线，
  **已退役不再维护**，v0.9.10/v0.9.20 tag 与全部批次日志归档在其
  `doc/log/`）+ `D:/Projects/Rust/starfish-we`（窗口 crate 上游，随
  本仓库 crates/starfish-we 继续演进）合并而成。上游旧目录已删。
- **下游**：`D:/Projects/Rust/pygame-rs`（RustPython 绑定层，Claude
  接手；其批次 34 记录见该仓库 CLAUDE.md 批次 34 段）。
- **近期批次（合并前完成，随迁）**：
  - 批次二十六：pygame 自检套件 23 案例（基线 129 项自检三端）；
  - 批次三十四：pygame-rs 全体系对接（绑定面 26 模块全覆盖：mask/
    permission/video/time.Clock/深度面/Session 相机裁剪/pygame.targets
    MRT 会话/sprite pyclass 桥/with_batch 直通口 blit_area·quad/
    event poll·clear/KMOD 常量），探针 selftest **24 项桌面全绿**；
  - 批次三十五：pygame::mixer 模块化（Sound/Channel/主控面/内置特效
    Gain·Echo·Distortion·LowPass/Effect 句柄，单测 6 项）；
  - 批次三十六：CustomEvent 透传（`Event::User { code }` +
    `custom_code()` 分配器 + `post_user`；**载荷不上浮**——Any 留
    窗口层，PyObject 由 pygame-rs 包裹层自理）；
  - 引擎增补：`render_targets_arc`（MRT 'static 收口）、统一
    `RenderPass::set_clip` 委托、Batch API 基层化
    （`BatchTopology` + `push_vertices`，quad 族/geometry 降为展开糖）。

## 跨平台行为定约（像素级一致性的既定决策）

- **色彩 = linear 直通**（创作值即显示值；桌面交换链已钉 linear 格式，
  web canvas 天然 linear——web 偏"深"是正确的，勿再"修"）；
- **web 窗口 = set_mode 尺寸权威**（2026-10-20 二次回退定案）：canvas
  位图+内联 CSS 创建时一次钉死，引擎无 resize 机器（ResizeObserver/
  window-resize 监听已回退移除）；浏览器缩放/窗口变化 = 对固定画布
  做视图缩放或裁切（Windows 桌面同语义）。勿重新引入自动 rescale；
- **`time::Clock::tick` 返回秒**（pygame 为毫秒——绑定位换算）；
- **video 验收降级**：三端能播 + pygame 接口可调即达标（不追样本
  覆盖率，缺样本的 5 个 video 测试失败为已豁免存量）。

## 坑位增补（承接自主线批次二十六/三十四/三十五，与前文 8 条并列）

9. **并行 rustc 大目标集 = 页面文件耗尽**（os error 1455）→
   E0786/E0463/"no_std" 幻觉错误全是连锁症状，`-j 4` 限流即愈——
   见"invalid metadata"先疑内存再疑缓存；
10. **`cargo build … | grep -c` 零匹配退出码非零** → `&&` 链静默断裂
    （wasm-bindgen 绑到旧产物）——产物链验证看**产物时间戳**；
11. **绑定面铁律（pygame-rs）**：①同类型仅一个 `#[pyclass]` impl 块
    （跨文件第二个 E0119）；②pymethod 直写 `Option<T>` 形参不做
    缺省（可省参须 FromArgs 结构体 `#[pyarg(optional)]`）；③
    **pymethod panic 穿透 Python 异常层带崩进程**（绑定面禁 panic）；
    ④Constructor tuple Args 无法变参（缺省形态须 derive struct）；
12. **音频 RT 线程铁律**：`AudioEffect::process` 在音频回调线程——
    禁分配/禁锁/禁 Python（特效参数走 `Arc<AtomicU32>` f32 bits，
    句柄 set 实时生效）；**Python 代码永远不进音频线程**；
13. `examples/web/resize_probe.html` 已随旧目录丢失（CLAUDE.md 坑 2
    引用的回归探针）——**待重建**；
14. 引擎 crate 内测试引用 crate 自身路径用 `crate::`（`starfish::`
    只在跨 crate 场景）。

15. **全局相机定约（2026-10-24 渲染架构批次 A）**：相机 uniform
    （`@group(0)@binding(0) 64B mat4x4 MVP`）**全局唯一**（Gpu 挂
    CameraRig + `CameraState = Auto2d | Auto3d | Manual`；pack 按状态
    现算，Auto 随目标尺寸隐式）；Screen/Surface/MRT 无相机字段。
    **相机 MVP 每 pack 一次性 uniform buffer + bind**（与批 vbo 同款
    定约）——主屏多次 pack 挂 pending 统一提交而 write_buffer 即时，
    共享 buffer 时同帧正交/透视交替 pack 互相覆盖（实测 3D 内容飞出
    屏外）；EncodeCtx 附件直用 starfish ColorAttachment/DepthAttachment
    （PassColor/PassDepth 已删）。Material 已正名 SpriteMaterial
    （pub(crate) 内部设施，cull/blend 创建期愿望定型）。
16. **中间面合成定约（2026-10-25 渲染架构批次 B）**：自带管线的客人
    （egui_wgpu 等——与引擎相机定约不同构者）走**中间面**：客人画进
    持久 Surface（`OverlayCtx::pass_on_surface`），引擎 sprite 管线
    `OverlayCtx::blit_fullscreen` 合成回目标（MVP = `camera_mvp_for`，
    一次性 uniform/vbo 与 encode() 同定约）——客人零改即吃全局相机。
    相机统一解算口：`render::camera_mvp_for(size)`（Manual override /
    Auto 按尺寸，与目标层 pack 同一解算）。
