


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

---

## 批次三十五（2026-10-20）：pygame.mixer/sound 模块化 + 音频特效基层

### 背景

绑定审计发现：pygame-rs 的 mixer 绑定直接消费 `base::audio`（分层
倒挂），starfish pygame 层缺 Sound/Channel 类型与特效面。本批在
starfish 侧补齐模块化（绑定改造为后续）。

### 落地（`pygame/mixer.rs` 扩展）

- **`Sound`**：`new()`（内嵌提示音，Font(None) 惯例）/`from_bytes`
  （解码）/`from_data`（sndarray 桥）/`get_length`/`play(loops)`
  （自动找空闲通道 → 返回 Channel）/`play_on`；
- **`Channel`**：idx 句柄——play/stop/pause/resume/set_volume/
  fade_in/fade_out/get_busy/get_sound/add_effect；
- **混音器主控面**：num_channels（init 定格）/set_reserved/get_busy/
  set_master_volume/output_sample_rate；
- **内置特效 4 种**（`AudioEffect` 实现：Gain/Echo/Distortion/
  LowPass）：参数走 `Arc<AtomicU32>`（f32 bits）——**句柄 set 实时
  生效、RT 线程无锁读**（音频回调线程禁分配/禁锁的契约遵守）；
  `Effect` 句柄（Clone = 同实例别名）+ `set(key, value)` 字符串
  参数面 + `add_effect(channel, &Effect)` 挂载；
- **Echo 修正**：环长恒等于延迟帧数（不足补零/超出截断）——首版
  空环起步会立即出湿声；单测验证 10ms@48k 冲激在第 480 帧精确出现
  0.5 反馈回声；
- **单测 6 项全绿**（无音频设备依赖：特效为纯 DSP 函数）。

### Effect 绑定设计定约（Python 面，三级形态）

RT 线程铁律：`AudioEffect::process` 在音频回调线程——**Python 代码
永远不进音频线程**。自定义 effect 的正确形态：

- **L1 内置特效参数化**（本批就位）：Rust 内置特效 + Python 句柄
  实时调参（`fx.set(key, value)` 走共享原子）；
- **L2 数据面自定义**：sndarray 生成/变换波形 → make_sound → 播放
  （"自定义 DSP"以数据形态表达，sndarray 桥已就位）；
- **L3 参数自动化**（v2）：Python 帧循环喂关键帧/LFO 曲线 → Rust
  特效按帧推进（poll 模型同构：音频线程永不进解释器）。

### 待办（绑定改造，下一步）

- pygame-rs mixer 绑定从 `base::audio` 直摸改道
  `starfish::pygame::mixer`（bridge thread-local 混音器与
  pygame::mixer 单例**双实例并存**是当前分层倒挂的遗留——改造时
  需迁移 bridge 保活的生命周期）；
- 绑定新增 Channel/Effect pyclass + needs_gesture 对齐
  （2026-10-18 部署约定撤手势门后该接口已过时）。

---

## 批次三十六（2026-10-20）：CustomEvent 透传（starfish 层收口）

### 背景

绑定审计确认：starfish-window 已有完整 `UserEvent`（code: u32 +
`Arc<dyn Any + Send + Sync>` 载荷；`Window::push_event` 同线程 +
`EventProxy` 跨线程注入），但 pygame::event 的 translate 兜底把
`User(_)` 落进 `Event::Other` 且**载荷丢弃**——CustomEvent 绑定未做。

### 方案（用户定调：不走 SDL 严格路径，starfish-we 已固定）

- starfish 层**透传 code、不上浮载荷**：Any 载荷留在窗口层（Rust
  消费者经 starfish-window 直取）；Python 侧附加字段由 pygame-rs
  包裹层自理（PyObject 存 Python 侧队列，不跨层）；
- **申请** = `event::custom_code()` 计数器（单调递增、不复用、无上限
  ——SDL RegisterEvents 的对应物，但 starfish-window 的 code 无系统
  资源占用，分配器仅为发送方约定 code 提供不冲突保证；0 保留）；
- **回收** = 事件实例随队列逐帧排空（载荷 Arc 引用计数归零即释）；
  code 不回收（SDL 同款不提供 free）。

### 落地（pygame/event.rs）

- `Event::User { code }` 变体 + translate 透传臂（`User(ue) → User{code}`）；
- `custom_code()` 分配器 + `post_user(code)` 入队（同帧内 post 先于
  get 可见）；
- base::window re-export 补 UserEvent；
- 单测：透传 code / post_user 入队即取 / 分配器单调（event 3/3）；
- **pygame-rs 零改动**：from_engine 兜底分支自然吸收新变体（待绑定
  侧按自己的 PyObject 包裹方案消费 `Event::User { code }`）。

### 职责分界（定约）

| 层 | 职责 |
|---|---|
| starfish-window | UserEvent 全能力（code + Any 载荷 + 注入） |
| starfish pygame::event | code 透传 + custom_code 分配器 + post_user 入队 |
| pygame-rs | PyObject 队列 + 事件对象包裹 + 属性附加（其自主实现） |