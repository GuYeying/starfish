# PROJECT STATUS — 进度快照与下一步计划

> 更新：2026-10-21（架构重构批次）。本仓库已重建为 git 仓库并完成
> workspace 化拆分——14 个引擎 crate + examples（DAG 与命令见根 `CLAUDE.md`，
> 批次记录见 `doc/log/starfish_changelog_2026-10-21.md`）；下文批次记录
> 对应拆分前的单 crate 时代。接手工作**先读本文档**，再读根 `CLAUDE.md`
> 与 `architecture/pygame层设计.md`（契约定稿 v1.0）。

---

## 一、当前阶段

**版本 v0.9.10（2026-10-11，批次二十五~二十六）：pygame 层测试套件
全模块覆盖 + 三端一致性两连修**——

- **模块运行测试套件**（`examples/pygame/`，25 案例 / 129 项自检）：
  每模块一案例（核心 9 + 扩展 16），桌面 129/129 全绿；Web 产物
  （wasm+js+html+索引页）与 Android 真机包（统一脚本出包）随批重出；
  索引与判读见 `examples/pygame/README.md`；
- **色彩三端字节级统一**（两连修，像素取证定案）：①案例侧 u8 风格
  f32 误入 push_quad 归一化约定（钳白）→ 全量归一化；②Windows 交换
  链默认选中 sRGB 格式（写入多一次编码）→ `settings.rs` 默认跳过
  sRGB 变体，回到契约 ④ Rgba8Unorm 直通语义——**三端创作值=显示值**；
- **time.tick 帧率控制正确化**（引擎 `base::time`）：语义改为上限
  节流（cap）——vsync 已达标不再补睡 + 量化带放行，消除"睡+等
  vsync"双重节流（60Hz 屏 53.8→59.6fps）；套件 `paced_frame` 改
  follow-vsync，Android 串联减半修复；三端帧率 = 各自刷新率；
- **上游窗口 crate 更名** starfish-window → starfish-we（v0.2.0）：
  Cargo `package =` 别名过渡，代码路径 `starfish_window::` 零改动；
- **pygame-rs 交由他人接管**（2026-10-11 定）——本仓库后续不涉其代码。

**里程碑（2026-10-18，批次四十一）：绑定层状态归属收编**——
pygame-rs bridge 瘦身七步（691→253 行，纯 RP 薄壳）的引擎侧：新增
`pygame::render::session`（活跃会话双槽 + MRT 保活表收回）与
`pygame::mixer`（生命周期 + 内嵌提示音）两模块；event 队列化（SDL
模型 `pump()` 灌队列返 Quit——**ESC 退出语义归脚本**）；`DrawTarget::
blit_area/push_points/push_points_indexed` 三 default method（area
blit 补齐源面 flush 保险）+ `font::preload/preloaded` 预载缓存。
**Web 自动播放策略定约为部署约定**（运行时手势门整体撤除——宿主
页面保证首次手势先行）。原则：行为一致——Rust 用户与 Python 用户
同一套引擎行为。详见 `doc/log/starfish_changelog_2026-10-18.md`。

**里程碑（2026-10-12，批次三十四）：pygame-rs 全体系对接完成**——
绑定面 26 模块全覆盖清零：mask/permission/video/time.Clock 三新模块
+ Surface 深度面（`depth_buffer=` 构造定约 §九.2）+ Session 相机/裁剪
+ **`pygame.targets` MRT 会话**（§3.1 预言形态）+ sprite pyclass 桥
（世代 id 回写 Python 对象）+ with_batch 直通口（blit_area/quad）+
event/display/io/KMOD 补面；探针 selftest 扩至 **24 项桌面全绿**，
web/APK 产物重出。引擎侧仅增补 render_targets_arc + 统一 set_clip
委托两处。详见 pygame-rs/CLAUDE.md 批次 34 段与
`pygame-rs/examples/binding_probe.py`。pygame-rs 移交收回、由 Claude
接手（2026-10-12 定）。

**里程碑（2026-09-30，批次二十二~二十四）：Web 音频闭环 + pygame-rs
绑定扩容四连**——AudioContext 捕获修复（cpal 0.18 平台包装层 as_inner
解包）+ Web 手势门定约（needs_gesture / 手势前 play 跳过）；
pygame-rs 批次 I–L：pygame.touch / pygame.gamepad（含 starfish 侧
gamepad facade + gilrs TLS 泄漏修）/ math 3D（Vector3/Quaternion/Mat4，
glam 直绑）/ draw.ellipse·arc + transform.smoothscale +
Surface.set_alpha。三端验收全绿，binding_probe 运行时自测 10/10
（详见 `doc/log/starfish_changelog_2026-09-30.md`）。

**里程碑（2026-09-28，批次十八）：RP 嵌入三平台全通**——window_test
（RP 嵌入 + Python 生成器门）Windows / Android 真机 / Web 同日齐跑，
六标记证据链全通（starting → set_mode → genesis → script → generator →
frame loop）。架构定稿见 `architecture/rustpython嵌入三端架构.md`。

**pygame Rust 层：契约对照表除 scrap（挂起）外全部 ✅**——P1~P5 全链
落地：transform/mask/sndarray/touch/**sprite（Godot 式重设计）**、
image.save 回读、会话/深度/MRT。39/39 单元测试；探针覆盖全能力三平台。

**Python 绑定层（pygame-rs）**：M0 嵌入冒烟 ✅（批次十六）→ M1 先行
切片（生成器门 + sf 扁平绑定面）→ **三平台实证 ✅（批次十八）**。
Android 闪退根因 = 打包时 `android.app.lib_name` 与 .so 不匹配
（应用代码从未执行过），已修复并固化 aapt2 正规打包链（见
`reference/安卓APK打包lib_name坑与真机诊断手册.md`）。

**pygame 层后续候选**（均另立细案）：transform nearest 采样变体、
MRT 异构输出/深度（自定义材质 v2）、Surface 层级 transform（v2）、
scrap（UI 底层后）、wgpu 验证错误并发 console_log（web 诊断改进）。

里程碑回看（2026-09-27 单日）：hello 三平台 → app_entry! 统一 →
web canvas 修复 → 契约定稿 v1.0 → P1 → 独立化捆绑 → P2 render+font →
P3 API 面 → P4 image → 会话/深度/MRT → API 完善包 → mask/sndarray/
touch → sprite → RenderPass 统一 → **RP 嵌入 M0**（批次一~十六）。

## 二、已完成里程碑（2026-09-27 单日）

| # | 里程碑 | 关键产物 |
|---|---|---|
| 1 | hello 三平台纯绿（web 清屏色修复 + Android android_main 闪退修复） | `target/android-apk/hello_android.apk` |
| 2 | app_entry! 宏统一入口（hello 三入口归一为 1 行） | `src/lib.rs` + `src/base/app.rs` |
| 3 | web canvas 正反馈爆炸修复（starfish-window 钉 CSS 尺寸 + resize 去重 + set_size 双重缩放） | `starfish-window/src/platform/web/mod.rs` |
| 4 | pygame 契约定稿 v1.0（GPU 路线 / 单一通用管线 / DrawTarget / DataBridge / 依赖双向规则） | `architecture/pygame层设计.md` |
| 5 | P1 基本类型：Color/Rect 回收 + locals 常量枢纽 + math 薄封装（20/20 测试） | `src/pygame/` |
| 6 | 项目独立化捆绑（见 §五） | 本仓库自包含 |
| 7 | **P2 render 底座 + font 落地**：单一通用管线（sprite.wgsl 三路同管）/Texture 三位一体/Screen+Surface+DrawTarget(&self)/Batch 双流/Camera/材质变体矩阵/BufferProxy/font 资源制备路线 | `src/pygame/render/` + `src/pygame/font.rs` + `examples/pygame_probe.rs` |
| 8 | P3 API 面：display/event/key/mouse/time/draw/version + pygame hello 三平台验收 | `src/pygame/{display,event,key,mouse,time,draw,version}.rs` + `examples/pygame_hello.rs` |
| 9 | P4 image：load/load_async/load_from_bytes → Surface + save（GPU 回读 + base::io 跨平台） | `src/pygame/image.rs` + `examples/pygame_image_demo.rs` |
| 10 | **渲染会话 / 深度 / MRT**：RenderPass<'a> 统一（with 语法 ADR-5 v1.2/v1.3）、render_depth（深度管线变体）、render_targets（双输出着色器）、set_camera | `render/target.rs` + `render/shader/` |
| 11 | API 完善包：display 旗标 set_mode_ex / set_clip(scissor) / draw.arc / rect 粗描边 / **transform（P5 提前）** / Color f32 转换 | 批次十一 |
| 12 | P5 尾巴：**mask**（回读位图+CPU 检测）/ **sndarray**（SoundArray↔SoundData）/ **touch**（手指状态表） | 批次十三 |
| 13 | **sprite Godot 式重设计**（池化 Group+世代 id+(z,y) 排序会话绘制；❌场景树/信号/ECS） | `src/pygame/sprite.rs` + 细案 `reference/` |
| 14 | 用户实测反馈修复：RenderPass 统一（批次十五）/ MRT fill 覆盖 / web 501 / Android 退出语义 | 批次九~十五 |

**批次二十五~二十六（2026-10-11）**：pygame 模块运行测试套件**全模块
覆盖**落地——`examples/pygame/` 25 案例（核心 9 + 扩展 16），桌面冒烟
**129/129 全绿**；案例索引与判读见 `examples/pygame/README.md`，
Android 真机包按统一脚本出包。上游窗口 crate 更名 starfish-we，
Cargo `package =` 别名过渡（代码路径不变）。

## 三、下一步

**① Surface depth：Rust 层已完备（2026-10-11 核查，无需新代码）**——
`Surface::with_depth/has_depth/render_depth`（带校验断言）+ Screen 深度
缓冲默认创建（Depth24Plus 无条件）+ resize 深度重建，全部就位且
pygame_probe（with_depth + render_depth 会话演示）600 帧×2 回归零错。
**剩余 = 绑定层透传 → 已落地（2026-10-12 批次三十四）**：
`pygame.Surface((w,h), depth_buffer=True)` → with_depth + `has_depth()`
+ `render_depth` 会话均已绑定（§九.2 定约）；绑定面本体
（bridge/native_pygame/pygame_impls）已扩至 26 模块全覆盖，
行为回归 = 探针 selftest 24 项（三端）。

**② rp_main_loop 挂起 → 已作废（2026-10-11）**：该示例已在 pygame-rs
重构中移除（binding_probe 为存活的等价案例且三端验证通过）——旧挂起
观察（批次十七旧包）随之失效。若未来重做 while-True 归 rpy 形态的
Android 案例，按批次二十四工具箱直接新建即可。

**③ 打包脚本已落地（批次二十一）**：`pygame-rs/scripts/build_apk.py`
（xtask 退役）——pygame-rs 自持、根示例共用、独立化友好。

**④ Android 真机回归**：三个 demo APK（probe/hello/image_demo）装机
过一遍（批次十六起挂账）。

**⑤ pygame 层后续候选**（均另立细案）：transform nearest 采样、MRT
异构输出/深度（自定义材质 v2）、Surface 层级 transform、scrap（UI
底层后）。

## 四、关键文件指针

| 主题 | 文件 |
|---|---|
| pygame 契约（定稿 v1.0） | `architecture/pygame层设计.md` |
| **RP 嵌入三端架构（v1.0，批次十八定稿）** | `architecture/rustpython嵌入三端架构.md` |
| **RP 嵌入全案（原理/魔改/绑定/踩坑全集）** | `architecture/rustpython嵌入三端全案.md` |
| **安卓打包 lib_name 坑 + 诊断手册** | `reference/安卓APK打包lib_name坑与真机诊断手册.md` |
| 宏入口（app_entry!） | `src/lib.rs` + `src/base/app.rs` |
| 窗口/事件（poll 模型本体） | `starfish-window/src/`（web 后端坑最多：`platform/web/mod.rs`） |
| 渲染核心（RenderEntry/Surface/资源） | `src/base/render/` |
| 几何家底（图元生成） | `src/base/gfx/geometry/shape2d.rs` |
| **pygame render 底座（P2）** | `src/pygame/render/`（shader/sprite.wgsl 三路同管）+ `src/pygame/font.rs` |
| **验收探针三件套** | `examples/pygame_{probe,hello,image_demo}.rs`（`cargo run --example …`；probe 600 帧后桌面退出/Web 驻留） |
| **RP 嵌入三端案例** | `pygame-rs/examples/window_test.rs`（生成器门；`cargo run -p pygame-rs --example window_test`） |
| **sf 绑定面** | `pygame-rs/src/rpy/sf.rs`（fill/rect/flip/quit_requested） |
| **sprite 重设计细案** | `reference/pygame sprite 的 Godot 式重设计细案.md` |
| 三份平台问题定位文档 | `reference/`（web渲染不绿 / web画布尺寸正反馈爆炸 / android闪退与Invalid surface） |
| 每日批次日志 | `doc/log/starfish_changelog_2026-09-{27,28}.md`（批次一~十八） |
| 旧架构基线文档（winit 版，供对照） | `architecture/legacy/` |
| 回归探针 | `web/resize_probe.html` |

## 五、项目独立化捆绑（2026-09-27 完成）

本仓库已自包含，可整体迁出独立工作：

- **starfish-window 已 vendor**（`starfish-window/`，源副本仍在
  `D:/Projects/Rust/starfish-window`），Cargo.toml 已改指
  `path = "starfish-window"`，构建验证通过；
- **xtask** 自旧仓库 git HEAD 恢复（`cargo xtask` 别名已配）——示例
  注册表**动态解析 Cargo.toml `[[example]]`**，已适配 hello/hello_android
  双注册约定（实测 `cargo xtask list` 通过；一键链 `cargo xtask android
  hello` 按 `[[example]]` 兄弟条目解析，代码路径已核对）；
- **android/**（NativeActivity 清单模板）、旧架构基线文档
  （`architecture/legacy/`）、全套历史 changelog 与设计笔记（reference/
  doc/log）已并入；
- **未带入（需要时从旧处取）**：`pygame-rs/`（Python 绑定 crate，
  untracked 无 git 历史，绑定的是旧引擎，待 pygame 层成熟后迁移重绑）、
  RustPython 源码包（用户处另有）；
- **存量问题**：`cargo test --lib` 中 base::video 5 个测试因缺真样本
  媒体失败（`real_sample_file` 类），样本补入 resources 后应自愈；
  与 pygame/渲染改动无关。
