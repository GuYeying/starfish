# starfish 更新日志 2026-10-24 —— pygame 层通用直绘能力：叠加通道 / 原始事件监听 / GPU 信息

## 设计背景

- **诉求**：让"自持管线、借通道直绘"的外接渲染器（UI 库、物理调试线等）
  能画上 pygame 屏幕。核心障碍：本层 batch 是**延迟提交**——`render()`
  会话期间没有活的 RenderPass，外接者无处落笔。
- **用户定约（2026-10-24）**：接口核心**先落 starfish 的 pygame 模块、
  再对接到绑定层（pygame-rs）**；且核心是**通用情况**——不针对任何
  特定库设计，具体库（egui / rapier）只是现役消费者，经绑定层对接。
- **挂接原理**：pack 的颜色/深度附件均为 `LoadOp::Load`（清屏归
  `begin_frame`），同一帧内、游戏 batch 提交之后开启的任何 Load 型
  通道按提交序自然叠加在游戏画面之上——通用直绘的挂接位由此而来。
- **红线**：引擎零第三方感知（接口面只暴露 wgpu / starfish-render
  公共类型，依赖单向红线）；本批接口对普通 Rust 用户同样是干净增益。

## 方案（落地形态）

1. **`Screen::with_overlay` / `Screen::defer_overlay`**（render/target.rs）
   ——叠加渲染通道**双形态**（自绘内容直绘屏幕的通用出口）：
   - **立即形态 `with_overlay(f)`**：调用点就地开通道直绘（次序敏感——
     须在游戏会话 pack 之后调用才不被反盖）；
   - **延迟形态 `defer_overlay(f)`**：任意时点调用（含游戏会话内）排程，
     `present()` 在本帧全部 batch pack 之后、交换链 present 之前统一
     执行——登记序即画家序，恒在游戏内容之上（GUI 类客人的推荐形态；
     表面未就绪的帧排程保留到下一帧）。
   两形态共用执行体 `run_overlay`（泛型 FnOnce——立即形态收借用闭包、
   延迟形态经 Box<dyn FnOnce> 入队）。**会话级形态**：`RenderPass::
   defer_overlay(f)`（同日追加）——"画到哪个目标"由会话对象表达（
   `with screen.render() as t: t.defer_overlay(...)`），Screen 会话路由
   present 排程、Surface/MRT 会话 v1 devlog 提示不支持；泛型闭包零
   第三方感知（依赖单向红线）。`OverlayCtx`：
   - `with_pass(label, depth, draw)`：开一条 Load 型通道、以
     `&mut RenderPass` 交付直绘（闭包内局部生命周期收口——附件切片
     不出方法体）；
   - `create_command_encoder()` + `submit_before(cmds)`：通道前命令的
     通用路径（资源上传 / 回调 prepare 等——须先于本上下文全部绘制
     通道执行的命令，用原生 wgpu encoder 记录、收口时先提交）；
   - `device() / queue() / access()`：GPU 句柄（自持管线创建期依赖）。
   - 深度契约：`depth=true` 附加深度附件（3D 内容），**每上下文深度
     附件只供一条通道**；管线深度关闭者（如 2D 线管线）必须传
     `false`——wgpu 对管线深度目标与通道附件严格校验。
2. **`event::add_raw_listener` / `remove_raw_listener(id)`**（event.rs）
   ——原始事件监听器**表**（通用多消费者，注册序即通知序）：`pump()`
   每条事件**先喂全部监听器、再走 pygame 翻译**。翻译有损（KeyDown
   不带修饰键等），需要完整事件词汇的消费者走这里；监听与队列两条
   通路互不干扰，监听器的存在不改变 pygame 面既有行为。监听器签名收
   `&Window`（与泵借用同源——消费者内不得再走 `display::with_window`，
   嵌套 borrow_mut 会 panic）。
3. **访问器 pub 化**：`display::with_window`（事件模块专用缝 → 公开，
   需要窗口 API 的消费者直用）；`render::gpu_info()` +
   `GpuInfo{device, queue, access}`（GPU 槽快照，自持管线创建期依赖；
   未建立 = `None`——绑定层映射用法错误，引擎侧不 panic）；
   `Screen::color_format() / sample_count()`（MSAA 须传给外接管线，
   否则 wgpu 校验错）。
4. **事件修饰键 parity**（同日追加，事件逐条消费者定约的配套）：
   `Event::KeyDown/KeyUp` 补 `modifiers: u32`（KMOD_* 位集——真 pygame
   `e.mod` 字段 parity；翻译时由引擎 Modifiers 经 `key::mods_to_bits`
   换算携带）；`key::mods_to_bits / bits_to_mods` 双向换算助手 pub
   （后者供绑定层从 `e.mod` 重建完整原始事件——egui 快捷键需要完整
   修饰键词汇）。
5. **事件队列帧尾清空**（同日追加，pygame-rs 侧用户定约："每次循环
   结束后不管用没用都要清空"）：`event::pump()` 起点排空上帧未消费
   事件——宿主不消费也不积压（陈旧 QUIT/键事件延迟爆发是积压的唯一下场）；
   `post_user` 改走 `USER_PENDING` 列、pump 时补进队列——**保一帧**：
   post 后下一帧 get/poll 可见（同帧 post→get 不再立即可见——契约
   变更，`custom_user_event_passes_code_through` 测试同步改写并补
   帧尾清空断言）；`clear()` 连带清 pending。既有消费路径（get/poll
   的排空语义、translate 状态表喂入）零改动。

## 关键保证

- 接口核心通用：三件能力（叠加通道 / 原始事件监听 / GPU 信息）的
  命名、语义、文档均按通用情况表述，具体库仅作现役消费者举例；
  今后任何"借通道直绘"的外接渲染器零引擎改动接入。
- 次序定约（文档化）：叠加绘制必须在游戏会话 pack 之后、present 之前
  ——提交序即画家序；未 begin 自动 begin（同 with_batch 语义），
  表面未就绪静默跳过（同 present 语义）。
- 既有行为零改动（事件清空一项除外，见 §5）：pygame 队列通路、
  translate 状态表喂入、present/pack 路径原样（无监听器时 pump 行为
  与改动前等价——唯 pump 起点多一次排空，消费型宿主不可感知）。

## 测试状态

- `cargo check -p starfish-pygame` 通过；`cargo test -p starfish-pygame`
  50/51（event 3/3 含帧尾清空新契约；唯一失败
  `font::tests::metrics_and_layout_math` 为存量字体路径环境问题，与本批无关）。
- 下游 pygame-rs 实测（rapier_test 15/15 + egui_test 14/14 全 PASS）：
  调试线 / egui 面板经叠加通道画上 pygame 屏幕，90/60 帧无异常——
  两个具体库作为现役消费者验证了通用接口的完备性。
- 帧尾清空追加后下游复测（pygame-rs 批次 43 拆分落地版）：sig_probe
  134/134 + rapier_test SUMMARY PASS + binding_probe 正常收尾 +
  egui_test 300 帧烟测——消费型宿主对新 pump 语义不可感知。

## 追加批次：Rect 类型退役（2026-10-24 用户定约）

**定约**：GPU 渲染下 `Rect` 未达到原版 pygame"描述 surface 信息"的等价
能力，且碰撞/触发器今后**统一走物理引擎**——Rect 类型整体退役：
- `rect.rs`（pygame.Rect 全量 parity + Point2D/Size/Line，418 行）删除
  （用户手工执行）；`starfish-gui` / `starfish-physics` 两 crate 一并
  移除（旧实验载体）。
- **draw.rs 重写**：全部绘制函数（rect/circle/line/lines/polygon/
  ellipse/arc）改**无返回值**签名——"受影响 Rect"是 CPU 脏矩形时代的
  产物，GPU 全帧 present 下无意义；内切矩形形参收 `IRect`。
- **`IRect` = `[i32; 4]` 数组别名**（`[x, y, w, h]`，render/mod.rs）：
  clip/scissor/blit_area 的内部几何载体——纯别名无方法面（用户定约：
  struct 载体也不要，typedef 数组形态最简）。
- `DrawTarget` trait 契约随之变更：`fill`/`blit`/`blit_centered`/
  `blit_area` 返回 `()`；`clip: Cell<Option<IRect>>`；
  `scissor_from(clip: IRect, …)` 按下标取数。
- 头部注释同步：draw 族 v1 简化口径修正（rect 粗描边=四条实心边带
  非旧注释的"一律 1px"；线族 line/lines/arc 恒 1px）。

## Rect 退役后测试状态

- `cargo check --workspace` 绿；`cargo test -p starfish-pygame`
  49/51（event 3/3 + Rect 相关用例随类型删除；唯一失败仍为存量
  `font::tests::metrics_and_layout_math` 字体路径环境问题）。
- 下游 pygame-rs 同步复测：sig_probe 124/124（Rect 段 12 断言随类型
  删除）+ binding_probe draw.positive/draw.negative/gfxdraw 全 PASS +
  rapier_test SUMMARY PASS + draw/font 保持窗形态 + egui 烟测零 panic。

## 追加修复：Windows 客户区光标滞留（starfish-we，2026-10-24）

**现象**（用户实测）：悬停窗口边框（非客户区）看到缩放光标后移入客户区，
缩放光标滞留不恢复。

**根因**：窗口类注册 `hCursor = NULL`——WM_SETCURSOR 落 DefWindowProc 时，
非客户区分支按命中测试正常设缩放光标，但 HTCLIENT 分支的
`SetCursor(类光标)` = `SetCursor(NULL)` 是**空操作**（等于"已处理、不改
光标"），上个光标（缩放）就留在了客户区。

**修复**（platform/windows/mod.rs wndproc）：新增 `WM_SETCURSOR` 臂——
`HTCLIENT` 显式 `LoadCursorW(NULL, IDC_ARROW)` + `SetCursor` 并返回
TRUE；其余命中测试回落 DefWindowProc（边框缩放光标保持系统语义）。
与 `set_cursor_visible`/ShowCursor 无交互（显示层计数优先于 SetCursor）。

## 追加批次：Surface 会话客人叠加通道（2026-10-24，画中画立项）

`defer_overlay` 的 Surface 会话落地（v1 仅 Screen 的边界拆除）：
- `Surface::defer_overlay` + `pending_overlays`（镜像 Screen 形态）；
  执行体收口在 **flush**——batch pack 之后统一执行，**batch 空也执行**
  （纯 UI / 纯调试线表面由此成立）；
- 颜色附件 = 本面纹理（Rgba8Unorm 离屏，LoadOp::Load 承接已绘内容）；
  深度附件 = `with_depth` 表面自持深度（Load 承接 pack 清后深度——
  3D 客人管线 depth=true 契约）；
- `RenderPass::defer_overlay` 的 Surface 分支接通（Rust 版 Surface::
  render() 会话同享；MRT 仍 v1 不支持）。
下游 pygame-rs 据此落地画中画（离屏 Surface 画 3D 物理调试动画 +
egui 内嵌面板 → blit 合成上屏，pip_test 5/5 PASS——下游批次 48）。

下游追加（pygame-rs 批次 48 修订）：egui 画中画目标尺寸覆盖——egui 的
screen_rect 与 ScreenDescriptor 按渲染目标取（Surface = 表面尺寸），
消除剪裁矩形越界 wgpu 校验 panic（绑定层问题，引擎侧无改动）。

## 追加批次：绑定收官支撑（pygame-rs 批次 49 对应）

- `session::begin_screen_depth`：Screen **深度会话**入口（与普通会话
  共用槽位——开启即替换；深度管线打包 + base 帧清屏清深度 1.0），
  下游 `screen.render_depth()` 真绑定撤桩；
- mixer `Sound` 增 pub `from_data` / `data` 桥（绑定层 PySound ↔
  engine Sound 包装）；
- 下游侧（本仓无关）：color init_colors 颜色表装载接入 install、
  Channel/Effect/parity 绑定面、parity_test 案例。

## 追加批次：叠加目标管线分套（pygame-rs 批次 50 对应）

- `Gpu` / `GpuInfo` 增 `offscreen_access`：default 颜色格式 = Rgba8Unorm
  的第二 RenderResourceAccess（`Screen::new` 装配期经
  `context.resource_access(Rgba8Unorm, DEPTH_FORMAT)` 建好）。客人
  渲染器（egui / rapier 调试线等自持管线者）按**渲染目标**选 access：
  wgpu 附件格式严格相等校验，交换链格式随平台漂移（Web/macOS =
  Bgra8、Windows = Rgba8）而离屏 Surface 恒 Rgba8——跨目标复用管线
  = 附件校验 panic（Web pip_test 实测：egui_pipeline BGRA8 vs Surface
  RGBA8）。"管线按 (color_format, sample_count) 分套"多窗口定约的
  叠加目标版；
- 下游侧（本仓无关）：egui Renderer 槽化（格式+样本数键）、rapier
  2D/3D 调试线槽键扩 (样本数, 目标系)、Surface 变体 ppp 恒 1.0
  （离屏无 DPI——窗口 175% 缩放渗入导致 egui 布局/剪裁错位内容裁空）。
