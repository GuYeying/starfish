# 渲染统一 draw 面设计（per-draw 相机/变换 + 坐标系正交化）

> 2026-11-04 定案。目标：`with screen.render() as f:` 一个会话块内
> **per-draw 相机与变换**（正交球 + 透视球同画布、文字牌进 3D 空间），
> blit 归一为 draw_surface，坐标系弃 pygame y 向下硬兼容、直走 wgpu
> 标准正交（y 向上）。本方案是实施蓝图——按文末批次顺序执行。

## 一、调查结论（2026-11-04 全链路核实）

### starfish-gfx / starfish-font：**不需要改**

- 范式 = 标准渲染对象工厂 + 开发者持有数据（无 Renderer 状态）
- 相机契约：group0 binding0 = 64B MVP，**开发者计算写入**（lib.rs 头注）
- transform 已有 CPU 烘路径：`Geometry::transformed(&Mat4)`、
  `text_mesh_tf`（starfish-font/pipeline.rs）
- 3D 全套：`text_pipeline_3d`（深度遮挡）、`shape3d::{sphere, cube,
  cylinder, cone, capsule, plane}`
- text.wgsl 顶点本就是 pos3（"2D 传 z=0，3D 传世界坐标"）
- **"billboard 基座"历史考虑已剔除**（用户定夺：billboard 手动可算，
  永不是问题）

### 真正的手术台：starfish-pygame/src/render/

- `Batch`（batch.rs）：双流（tris/lines）+ **纹理切段**；MVP 是
  **pack 级一个**（encode 时 `ctx.camera.mvp` 一次性 uniform，整批共享）
  ——同画布无法双投影 / 3D 空间文字的根因
- `CameraState`（camera.rs）：全局状态机（Auto2d/Auto3d/Manual）——
  `set_default_camera` 全局接管即源于此，per-surface 无独立相机
- `Camera::ortho`（camera.rs:19）：`orthographic(0, w, height, 0, ...)`——
  y 翻转硬兼容 pygame（ADR-8）
- 既有可复用先例：`push_quad_corners_3d`（batch.rs——blit_transform 的
  CPU 烘执行原语）、`blit_transform`（target.rs:81——CPU 烘 model 进
  顶点角点）、段级 bind 切换（group1 纹理段级已有）
- shader/sprite.wgsl：group0 camera.mvp + group1 纹理；pos3+uv2+color4
  ——**改造后 shader 零改动**

## 二、改造设计

### 2.1 段级相机（batch.rs 核心）

- `Segment` 加 `mvp: Mat4` 字段
- `Stream::push` 段边界判定扩为：**纹理切换 ∪ MVP 变化**
- `Batch::set_mvp(m: Mat4)` 状态机（会话对象转调）：后续 push 以当前
  MVP 入流；段循环内 `set_bind_group(0, seg_cam_bind)`（group0 从
  pass 级降到段级——先例 = group1 纹理段级切换）
- encode：每段独立一次性 cam_buf + cam_bind（现有 "每 pack 一次性
  buffer" 同款防覆盖定约，按段复制；段数 = 纹理 × 相机组合，量小）
- **shader 零改动**：单 MVP uniform 保留

### 2.2 model 走 CPU 烘（不进 GPU）

- `set_transform(model: Mat4)`：收集层烘顶点——quad 用
  `push_quad_corners_3d`（model 变换四角）、图元用
  `Geometry::transformed(&model)`（已存在）、文字 surface 走
  draw_surface 同路
- 理由：WebGL2 无 push constants；dynamic offset 复杂；图元/文字顶点
  数量小，CPU 烘零风险零兼容坑（blit_transform 已实证该路径）

### 2.3 会话级状态机（target.rs RenderPass）

- `RenderPass` 加字段：`cur_mvp: Mat4`（初始 = rig.resolve_mvp(size)）、
  `cur_model: Mat4`（初始 = IDENTITY）
- 新接口（会话方法，每目标独立状态——天然 per-surface）：
  - `set_camera(camera: &Camera)` → `cur_mvp = camera.mvp()`
  - `reset_camera()` → 回 rig.resolve_mvp（Auto 兜底）
  - `set_transform(model: Mat4)` / `reset_transform()`
- `set_default_camera` / `CameraState` 全局机 **退役**：init(dim=) 只
  决定"默认 mvp 的形态"（Auto2d / Auto3d 透视），Manual 概念由会话级
  set_camera 取代
- 所有 push 路径（blit/gfxdraw/sprite/geometry）统一走
  `cur_mvp` + 烘 `cur_model`

### 2.4 blit → draw_surface 归一

- `DrawTarget::draw_surface(src, transform: Mat4, tint)` 上位
  （= 现 blit_transform 改名扩正），`draw_surface_centered` 便捷形态
- **blit / blit_centered 剔除**（调用方全改；demo 重写覆盖）
- font.render 产物（Surface）同一口——3D 文字牌 =
  `draw_surface(text_surf, model)` + 深度会话

### 2.5 坐标系正交化（camera.rs）

- `Camera::ortho` 重定义：`orthographic(-w/2, w/2, -h/2, h/2, 0, 1)`
  ——**y 向上、原点在屏幕中心**（wgpu NDC 同向；与 3D 透视相机
  看向原点、`pygame_physics` 米制系"原点在视口中心"**三方同构**，
  2D/3D/物理一个坐标系，调试线直接对齐）
- **联动换算的坐标**（泵层一次换算，语义 = 中心系屏幕坐标）：
  - `mouse::get_pos` / `set_pos`、`touch` 的 x/y、`Event.x/y`
    ——OS 事件左上原点 y 向下，泵层换算
    （`x - w/2`、`h/2 - y`）
  - 现有 2D 脚本/教程/demo 坐标全改中心系（demo 重写覆盖）
- letterbox `fit_ndc` 不动（对称变换，原点无关）

### 2.6 绑定层（pygame-rs src/rpy/pygame/）

- gfxdraw.rs：`route` 后的会话 batch push 全部带 cur_mvp/烘 model；
  新 Python 口（挂会话对象，**不挂模块**——多 surface 并发防串线）：
  - `f.set_camera(camera)` / `f.reset_camera()`
  - `f.set_transform(model)` / `f.reset_transform()`（model 收 9/16 数
    或 (x, y, rot, scale) 便捷形态——绑定层合成 Mat4）
- `pygame.render.set_default_camera` / `camera_state` 剔除；
  `camera_ortho` / `camera_perspective` 保留（产 Camera 传给
  `f.set_camera`）；`get_default_camera` 不再需要（会话级后各持有）
- `Camera`（pygame.math 之外的 render.Camera）暴露 mvp() 查询

## 三、批次顺序

1. **T1 starfish-pygame render 层**（本文档 2.1/2.3/2.5）：
   batch.rs 段级相机 + RenderPass 状态机 + camera.rs 正交化 +
   CameraState 退役；`cargo test -p starfish-pygame`（batch_tests
   更新为带 MVP 版）
2. **T2 draw.rs / sprite.rs / font.rs 调用方**：push 带 MVP +
   model 烘；blit → draw_surface（2.4）
3. **T3 pygame-rs 绑定**（2.6）：gfxdraw 状态机 + 会话口 + 剔旧 +
   事件/鼠标泵层 y 翻转
4. **T4 demo 重写**：hello_word 更新（正交球 + 透视球同画布各一颗、
   3D 空间文字牌、y 向上坐标验证）；桌面实跑 + wasm 构建
5. **T5 双仓提交**：starfish（render 层）+ pygame-rs（绑定层）；
   文档站同步（render 相机定约 note、gfxdraw 会话口、剔旧清单）

## 四、验收标准

- 同一 `with screen.render():` 块内：set_camera(ortho) 画球 +
  set_camera(perspective) 画球，同屏双投影 ✓
- `draw_surface(文字, transform)` 深度会话内进 3D 世界（游戏内 id）✓
- 2D 坐标 y 向上、原点屏幕中心（与 3D/物理同构）；鼠标事件与绘制一致 ✓
- 现有 gfxdraw 图元不 set_camera 时 Auto2d 兜底，行为仅 y 向翻转 ✓
- wasm 构建 + 桌面 0 错误 ✓
