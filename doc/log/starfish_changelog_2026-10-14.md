


---

## v0.9.20 发布注记（2026-10-17）

**主题：为 pygame-rs 全体系对接（批次三十四）做引擎适配**——本版本无
新功能面，全部改动围绕"让 RustPython 绑定层能干净地够到引擎能力"。

### 一、引擎增补（绑定层的前置依赖）

- **`render_targets_arc`**（render/target.rs）：MRT 多渲染目标会话的
  'static 收口变体——绑定桥需要把 RenderPass 驻留 thread-local，
  目标 `Arc` 由桥保活至 finish（crate 内唯一 unsafe，契约随码注释）；
  配套统一 `RenderPass::set_clip` 委托（Screen/Surface 分派，MRT v1
  静默忽略）。
- **`Batch` API 基层化**：新增 `BatchTopology{Triangles, Lines}` +
  `Batch::push_vertices(topo, texture, &[SpriteVertex])` 顶点组直入
  基层接口；`push_quad / push_quad_uv / push_quad_corners /
  push_geometry` 全部降级为基层**展开糖**（公开签名零变化，上层
  pygame-rs 无感）。设计动机：图元展开（quad 六顶点、三角退化、
  线段组）与"入流/切段/提交"分离后，新增图元 = 写一个展开函数，
  基层永不膨胀。新增 6 项单测（基层等价性——push_quad 与手写同
  坐标顶点组逐顶点一致/拓扑分仓/同纹理续段/空组 no-op/线几何路由/
  填充退化路径不变）。

### 二、架构收口（批次四十，用户主刀——有破坏性，pygame-rs 绑定已同步）

- **`pygame.locals` 模块撤编**：常量就近安置——`K_*` 键值与
  `KMOD_*` 修饰键并入 `pygame::key`、`BUTTON_*` 并入 `pygame::mouse`、
  窗口旗标（RESIZABLE/FULLSCREEN/...）并入 `pygame::display`；
- **`pygame.surface` 模块撤编**：Surface 构造入口改为
  `pygame::render::Surface::new / with_depth` 关联函数
  （with_depth = 附 Depth24Plus 深度缓冲，`render_depth` 前置）；
- **`GpuSettings.use_depth` 无用字段清除**（深度缓冲创建已由
  Screen 无条件承载，字段无消费方）；
- **`version::vernum` 更名 `get_version`**（语义不变：返回三元组；
  pygame-rs 绑定同步）；
- 旧 winit 时代探针（examples/winit-examples/*）归档至
  `examples/old_codes/`。

### 三、测试套件调整

- 套件 **25 → 23 案例**：`pygame_test_locals` 撤除（常量面由
  key/mouse/display 就近承载，selftest 断言并入既有案例）、
  `pygame_test_version` 撤除（版本一致性由 src 单测承载）；
  Cargo.toml 对应注册清理；
- transform 案例旋转动画改**预建帧轮换**（性能定约：循环内逐帧
  rotozoom 新建 GPU 表面曾致闪烁，引擎改唯一 id 键后分配开销仍
  违约定）；
- 新增 `examples/triangle.rs + triangle.wgsl` 实验样例（未注册
  构建清单，手工 `--example triangle` 运行）。

### 四、配套（pygame-rs 侧，独立仓库）

批次三十四：绑定面 26 模块全覆盖（mask/permission/video/time.Clock/
深度面/Session 相机裁剪/`pygame.targets` MRT 会话/sprite pyclass 桥/
with_batch 直通口 blit_area·quad/event poll 等补面）+ 探针 selftest
扩至 24 项桌面全绿——详见 `pygame-rs/CLAUDE.md` 批次 34 段。

### 升级提示

- `pygame.locals.*` → 按 K_*/KMOD/BUTTON/旗标分别改
  `pygame.key.* / pygame.mouse.* / pygame.display.*`；
- `pygame.surface.new/with_depth` → `pygame.render.Surface::new /
  with_depth`；
- `pygame.version.vernum` → `pygame.version.get_version`。

