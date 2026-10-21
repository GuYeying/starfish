# Starfish 更新日志 — 2026-09-06（第二批）

> 接续同日首篇（time SDL3 / font kerning / audio 单声道 / 多窗口标注 / 架构清理）。
> 本批主题：**font 架构重写（对齐 02-04 范式）** 与 **gfx 几何绘制模块（全新）**，
> 并依架构决策移除刚建立的 render/camera.rs（相机回归为开发者的普通资源）。
>
> 状态：`cargo test` 42/42 通过，示例编译零错误，新增依赖 0。

---

## 🔤 font — 架构重写（对齐 02-04 范式）

### Removed
- **`TextRenderer`**（专属渲染器）与 **`TextMesh`**（自造网格类型）整体删除——
  font 不再持有渲染状态，也不发明新对象类型
- `set_camera(width, height)` / `set_mvp` / `draw_text` 一并移除
  （被标准对象 + 矩阵接口取代）

### Added
- `font/pipeline.rs`：**标准渲染对象工厂**
  - `text_mesh_local(access, atlas, text, scale, color)`：基线左端锚定**原点**的本地空间网格
  - `text_mesh_tf(access, atlas, text, &Mat4, scale, color)`：本地布局 + 任意变换矩阵
  - `atlas_bind_group` / `text_pipeline` / `text_pipeline_3d`
    （3D 变体深度测试开启——世界空间文本可被场景遮挡）
- `layout_text_local`：本地空间布局（原点锚定）
- `transform_vertices(&[TextVertex], &Mat4)`：矩阵施加原语（uv/color 透传）
- `TextVertex` 升级为 **pos3**：2D 传 z=0，3D 传世界坐标——2D/3D 一套布局

### Changed
- 网格类型直接使用引擎标准 `Mesh`（`TextMesh` 删除）；
  实时更新（billboard / 动态文本）= 标准 `write_vertex_buffer` 写入
- 相机接口收敛为 **04 案例同款**：裸 uniform 缓冲 + bind group，
  MVP = projection × view × model 由开发者用 glam 合成并写入——
  **font 不保存任何变换状态**；billboard = 逐对象换 model 矩阵，顶点零重建
- `pipeline::text_pipeline` 系列文档写明着色器契约：
  group0 binding0 = 64 字节 MVP uniform（列主序），由开发者管理

### 定位声明
- font 服务**游戏内文本（2D/3D）**；UI 文本归 UI 库（自带文本栈，零耦合）
- Billboard 是开发者能力（逐对象 model 矩阵），非底层内置

---

## 📐 gfx — 几何绘制模块（全新）

### Added
- **`ShapeVertex`**（pos3 + color4，2D 传 z=0）：与 `TextVertex` 同哲学的几何顶点契约，
  `layout()` 供 `mesh_builder` 使用
- **`Geometry`**：生成结果容器（顶点 + u16 索引，索引恒存在统一绘制路径）；
  `transformed(&Mat4)` CPU 烘焙变换（平移/旋转/缩放）
- **耳切三角化** `geometry/tri.rs`：任意简单多边形（凸/凹、绕向不限）→ 索引；
  退化输入兜底扇形，永不 panic
- **2D 生成器**（`geometry/shape2d.rs`）：rect / rect_outline / circle / circle_outline /
  ellipse / line / polyline / regular_polygon / polygon / capsule2d
- **3D 生成器**（`geometry/shape3d.rs`）：cube（8 顶点 36 索引外向绕行）/
  UV sphere / plane / cylinder / cone / capsule3d（环带构造，极点重复环）
- **管线工厂**（`gfx/pipeline.rs`）：`fill_pipeline_2d / fill_pipeline_3d /
  line_pipeline_2d / line_pipeline_3d`（独立调试标签：gfx_fill_2d 等）+
  `shader()` + `SHAPE_WGSL`（内嵌单着色器，填充/线段两管线共用）
- **`shape_mesh(access, &Geometry)`**：几何数据 → 标准 Mesh
  （顶点缓冲带 COPY_DST，支持实时更新）
- 4 个纯函数测试（凸四边形切分 / 凹 L 形面积 / 顺时针输入 / 退化输入）

### 设计对齐（与 font 同构的四段式）
1. 基本类型（`ShapeVertex`，Pod + layout）
2. 生成器（纯函数，参数 → 几何）
3. 标准对象工厂（返回 Mesh / RenderPipeline，全部可绕过）
4. 绘制 = 02-04 范式（`pass.set_pipeline` / `set_bind_group` / `set_mesh` / `draw_indexed`）

### 形状参数 = 物理碰撞原语
Circle / Aabb / Obb（旋转盒）/ Sphere / Capsule / Cylinder / Plane ——
形状参数结构体与未来碰撞系统共用同一批定义，gfx 是它们的第一个消费者。

---

## 🪟 render — 相机层决策反转 + 多窗口标注

### Removed
- **`render/camera.rs`**（建立→依决策移除，净零）：
  相机 uniform 回归为**开发者的普通资源**（04 案例同款：裸缓冲 + bind group，
  MVP 由开发者用 glam 合成并经 `write_buffer` 写入）
- font/gfx 的相机便利函数 re-export 随之移除

### Changed
- 多窗口接口（`RenderEntry::surface_from_context` 系）文档标注
  **「仅开放，不具备开箱即用能力」**：模块级 `//!` 总声明 + 函数级警示横幅；
  官方支持形态回归**单窗口**（移动/鸿蒙表面语义不在承诺内）

---

## 🖼️ examples — draw_gfx 全形状演示（新建）

- 上半屏 2D：10 种形状（填充琥珀 + 描边青蓝，像素正交）
- 全屏 3D：地面平面 + 立方体/球/圆柱/圆锥/胶囊 3D（透视相机绕场景缓速环绕，深度开启）
- 双 pass 结构：3D 世界 pass（带深度附件）→ 2D HUD pass（无深度、Load 不清屏）
- 120 FPS 帧控

### Fixed
- 2D 无深度管线与带深度附件的 pass 混用导致的
  `IncompatibleDepthStencilAttachment` 验证错误（双 pass 拆分修复）

---

## 📊 状态

- 测试 **42/42**（本批新增：gfx 耳切 4）
- 示例 9（新增 draw_gfx）
- 新增依赖 0
