# starfish 更新日志 2026-10-25 —— 渲染架构重构阶段 A/B：全局相机定约 / blit_transform 单接口 / 中间面客人合成

## 设计背景

- **诉求**（pygame-rs 渲染架构优化计划，用户四轮定调）：
  1. Material 名不副实（实为"pygame 固定渲染管线的状态材质"）——
     改名内部化，不暴露；
  2. Surface 里不该有 camera 字段——渲染 Surface 时不设相机则全局
     隐式；变换走 `blit(surface, mat4)` 单接口（Mat4 承载一切变换，
     零拷贝旋转、3D 空间放置同口）；Surface 定位纯化 = 离屏画布 +
     空间可放置纹理对象；
  3. 能不暴露 wgpu 底层就不暴露；不开 features 口；
  4. 细粒度管线控制经 starfish-render 已有的 builder 自动推导承载
     （`pygame.render.init(args)` 愿望口）。
- **三管线同构性核查**（调研结论）：sprite.wgsl 与 rapier 调试线
  shape.wgsl 已同构（`@group(0)@binding(0)` = 64B mat4x4 MVP，
  调试线本就带 camera_bind() 共享访问器）；font 的 text.wgsl 同
  约定；**egui 唯一不同构**（16B Locals、NDC 直写无矩阵、crates.io
  原版不可改）。
- **vendor fork 被否**（用户定夺）：不改 egui-wgpu（维护成本）——
  egui "优先考虑" = 架构上先消化它：**中间面方案**。

## 阶段 A：层级调整（pygame-rs 批次 52 对应）

1. **全局相机定约**（camera.rs / target.rs / mod.rs）：
   - `CameraState = Auto2d | Auto3d{fov_deg,near,far} | Manual(Camera)`
     状态机挂 Gpu；全局唯一 CameraRig（uniform+bind 一份）；
   - Screen/Surface/MRT 三处相机字段删除（每 Surface 省一份 64B
     uniform + bind group）；`Surface::set_camera` 删除（零调用者）；
   - pack 时按状态现算 MVP（`camera_for(size)`）：Auto 按目标自身
     尺寸隐式跟随（resize 天然生效免钩子）、Manual 用户 override；
   - `render::set_camera / reset_camera / camera_mvp / camera_state`
     公开——一处设置、全体生效；`RenderPass::set_camera`（会话形态）
     转发全局。
2. **相机 MVP 每 pack 一次性 uniform + bind**（batch.rs）：主屏多次
   pack 挂 pending 统一提交而 `write_buffer` 即时——共享 buffer 时
   同帧"正交→透视→正交"交替 pack 的 MVP 互相覆盖（下游
   blit_transform_demo 首轮盲验抓到：3D 卡片全部飞出屏外）。与 vbo
   同款一次性内联副本免疫（批次九坑的相机版）。
3. **blit_transform 单接口**（target.rs / batch.rs）：
   `DrawTarget::blit_transform(src, mat, tint)`——源面四角
   (0,0,0)(w,0,0)(w,h,0)(0,h,0) 统一 `transform_point3`：2D 仿射
   （零拷贝旋转/缩放/镜像，替代 transform 烘焙）与 3D 模型矩阵空间
   放置（`render_depth` 会话下与 3D 场景正确遮挡）同一入口；
   `push_quad_corners_3d`（pos3 角）原语。默认管线 cull=none
   （`RenderPipelineBuilder::new_2d` 默认已确认）——镜像矩阵绕序
   天然安全，无需 CPU 绕序 hack。
4. **Material → SpriteMaterial**（material.rs）：改名 + 全 pub(crate)
   内部化 + 定位注释改写；cull/blend 创建期愿望定型进管线 build
   （变体缓存键不变——一次定型仅一套变体）。SPRITE_WGSL 保持 pub。
5. **附件类型收敛**（batch.rs / target.rs）：PassColor/PassDepth 删除，
   EncodeCtx 直用 starfish ColorAttachment/DepthAttachment（`formats`
   平行数组供管线变体选取——附件本体无格式字段）——与 OverlayCtx
   同轨，pygame→starfish 手写转换层退役。
6. **RenderWish 愿望槽**（mod.rs）：dim/fov/near/far/cull/blend——
   set_mode 前 `store_wish` 配置、Gpu 装配时消费一次；下游
   `pygame.render.init(...)` 部分覆盖语义（后设优先）。

## 阶段 B：客人相机对齐（pygame-rs 批次 53 对应）

1. **中间面合成定约**（target.rs OverlayCtx）：
   - `pass_on_surface(label, target, clear, draw)`：任意离屏面上开
     客人通道（自带管线的绘制端先画进面）；
   - `blit_fullscreen(src)` / `blit_fullscreen_with(src, mvp)`（后者
     同日追加快照形态）：引擎 sprite 管线全屏合成——一次性相机
     uniform/vbo 与 encode() 同定约；MVP = `camera_mvp_for`（现算）
     或调用方排程时点快照；
   - `camera_mvp_for(size)` 公开（客人侧全局相机解算，与目标层 pack
     同一逻辑）。
   - **egui 接入**（下游 pygame_gui runtime）：egui_wgpu 原版零改，
     画进持久中间 Surface（Rgba8/1x 清透明——原 NDC 公式 1:1 成立）
     → 引擎合成回目标；渲染器恒单槽（直绘取消后无需按目标格式
     分套）。默认 Auto2d 像素级 1:1 零回归（pip_test 盲验）；
     `set_camera` 后 egui 跟随全局相机。
2. **调试线全局跟随**（rapier-starfish-debug + 下游）：
   `DebugLinesRenderer/3D::set_camera_mvp`（底层直写原语，列主
   `[[f32;4];4]`）；下游 `follow_global_camera(enable)`（默认 false
   ——构造 look-at / ppm 显式相机优先；开启后 draw 前每帧同步全局
   MVP）。font/gfxdraw 走 Batch 路径在阶段 A 已自动对齐（零改造）。
3. **快照时序修复**（同日追加，用户审查发现）：主屏 egui 收口在
   present 帧末——帧内多次 set_camera 时现算 MVP 拿到的是帧末状态；
   下游 DeferredFrame 排程时点快照 `camera_mvp_for(目标尺寸)`，
   执行用 `blit_fullscreen_with`（"draw_ui 时刻的相机"精确语义：
   同帧 A 相机主屏 / B 相机离屏面板各得其所）。

## 关键保证

- **全局相机 = 时间维状态而非一次性配置**：set → 画 → pack 配对
  （每 pack 一次性 uniform）；任意目标任意次数切换正确。
- **egui 零改**（crates.io 原版）：中间面方案无 fork 维护负担；架构
  上 egui 降格为"画布客人"，变换由引擎管线承载——与 Surface
  "离屏画布 + 空间纹理对象"定位自洽。
- 批次 51 深度定约（with_pass 恒 Clear 1.0）全程不回退。

## 测试状态

- `cargo check -p starfish-pygame` 绿；单测 49 过（唯一失败
  `font::metrics::metrics_and_layout_math` 为存量问题，与本批无关）。
- 下游 pygame-rs 实测（全部盲验——无提示问法）：blit_transform_demo
  三轮（首轮抓出相机 uniform 覆盖 bug 后修复；2D 三联 + 透视空间
  放置近大远小全可辨）SUMMARY PASS；surf_probe/pip_test/rapier3d_test
  SUMMARY PASS（批次 51 成果零回退）；binding_probe 46/46（含
  targets.mrt 与 render 模块挂载）；透视跟随冒烟 240 帧无 panic
  （identity-view 透视下全屏 quad z=0 恰在相机平面退化不可见属数学
  一致预期——实用跟随形态为带 view 的相机）。
- Web：blit_transform_demo / pip_test / egui_test / surf_probe /
  rapier3d_test wasm 重打包，blit_demo Web 盲验过（Bgra8 分套无回归）。

## 提交链

- starfish：8b4b701（阶段 A）→ 3993aa4（阶段 B 引擎侧）→ 6e3fe77
  （快照形态）
- rapier-starfish-debug：5c913e7（set_camera_mvp 原语）
- pygame-rs：b131e95（阶段 A 绑定层 + pygame.render 模块）→ a4bf8f9
  （阶段 B 绑定层）→ 06b4e11（egui 快照）

## 追加批次：注释整理与架构可读性（2026-10-25 收官清扫）

渲染架构 A/B/C/D 落地后的代码整理（零行为变更——编译 + 回归
custom_pipeline_demo/blit_transform_demo SUMMARY PASS +
binding_probe 46/46 验证）：

- **API 正名收官**（用户四轮定夺）：`set_camera` → `set_default_camera`
  （全链路：引擎 render::set_default_camera + RenderPass 同名 +
  pygame.render.set_default_camera + 三会话 t.set_default_camera——
  语义钉死 = `with render()` 批绘制的**隐式默认相机**，raw_render 自管
  不参与）；`reset_camera` Python 面删除（引擎内部 render.init 愿望
  切换保留）；raw_render 相机彻底自管（camera= 托管撤销；
  `access.write_buffer(buf, mat4)` 特判 Mat4 直写自有 bind group）。
- **绑定层 render.rs 分区**：工具区（Debug 兜底宏/kwargs 解析）/
  Camera / 全局相机 / 愿望 init / 资源两入口（ResourceAccess +
  FromArgs 收参 + 资源句柄）/ 主屏查询 / raw_render 各段段头横线
  与导航注释补齐；kw_int/kw_f64/kw_str 等工具函数 doc 补全。
- **引擎侧 doc 补全**：CameraRig::raw 关联 Gpu::camera_for 现写
  语义；新增公开接口（with_surface / texture_view / pass_on_surface /
  blit_fullscreen(_with) / camera_mvp_for / RenderWish / store_wish /
  follow_global_camera / set_camera_mvp）doc 核对齐全——含"何时调 /
  定约 / 坑位"三层信息。
