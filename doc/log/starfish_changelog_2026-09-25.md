# Starfish 更新日志 2026-09-25

## 批次 32（2026-09-25）：资源异步装载统一方案——Texture 三态句柄 + Web 流水线

> 用户定向："先把跨平台模型做好，对 4（Web 纹理异步装载）做统一方案
> 研究。"研究成文 `reference/资源异步装载统一方案.md`，四决策点用户全部
> 按推荐裁决（静默跳过 / 构造抛错 / 预算排水 v1 不做 / Android base_dir
> 本批修正）。本批 = 裁决落地。

### 设计背景

- 三平台"资源就绪"时序差：桌面同步全链（批次 31 已达）、Android 同步但
  base_dir 注入在 run 内、Web Device/fetch 双异步。
- **对设计稿 §4.3 的关键精简**：意图记忆（pending_play）是 Sound 一次性
  动作的需要；Texture 的 draw 逐帧重发——未就绪帧跳过、就绪自动出现，
  语义自动闭合 → 三层机制瘦身，App 层登记整层可省。

### 设计方案

- **三态句柄** `TexState = Loading | Ready(Arc<Texture>) | Failed(String)`，
  住 `Rc<RefCell<TexShared>>`（state + width/height）——py_new 只产
  payload（尚无 PyRef），共享单元先于对象存在，Rc clone 入 spawn_local
  任务即可回写（RP 对象 Rc 同线程，'static 成立）。
- **桌面/Android**：同步流水线不变，返回即 Ready；失败构造抛错（错误
  曝光在声明行）。
- **Web 流水线**：`ensure_device_slot_async`（async_new_context 幂等装配）
  → `io::read` fetch → `load_from_memory` 解码 → 上传 → 写回。顺序保证
  零机制：spawn_local 单队列顺序 drain，Device 任务先入队先完成。
- **`assemble_gpu_web` 统一两分支**（修正隐患）：先行槽有货 →
  `async_surface_from_context` 复用 Device 挂表面；否则整体 async_new——
  运行期永远只有一个 Device 实例（否则早期纹理绑错设备）。
- **draw/getset 语义**：Loading = 静默跳过；Failed = draw 抛错；
  `.ready` / `.error` 直查；repr 带 loading/failed 标记。
- **Android base_dir 注入提前**（批次 32b 一并）：`set_android_app`
  （app_entry 捕获点）即注入 io base_dir + RUST_BACKTRACE；run 内同值
  注入保留作直连兜底（幂等）——解锁 Android run 前资源构造。

### 关键保证

- 三平台同一脚本零 cfg：`if not tex.ready: 画加载屏 else: tex.draw(...)`
  ——桌面 .ready 恒真，加载屏分支自然退化为不可达。
- **Web 运行期单 Device 不变量**：Device 创建收敛于**带表面单点**
  （on_start `async_new`）——无表面 `requestAdapter` 在 headless WebGPU
  路径悬死（实测 90s+ 不响应），带表面请求从不失手。纹理流水线因此
  与 Device 解耦：fetch/decode 完成即存像素，Gpu 安装时**排水上传**
  （`PENDING_UPLOADS` 注册表 + `drain_pending_uploads`）；运行期晚构造
  则直传。中途探索过的"无表面先行 Device + 微任务让出去重"方案被此
  事实否决（初版实现即触发两 Device 赛跑 + 适配器悬死，日志为证）。
- **桌面/Android Device 先行不受影响**：无窗口 `new_context` 路径仅
  非 wasm 存在（同步 pollster 无悬死问题）。

### 测试状态

- pygame-rs `cargo test` **38 passed**（桌面行为不变）；starfish 基线
  69 passed（set_android_app 注入移位零破坏）；wasm32 构建 + bindgen 通过。
- **Web 实机闭环**（headless Edge 存活模式 + server.py 资源挂载，
  `web/render_hello.html`）：控制台锚点序列
  `set_mode 记账 → GPU 装配完成 → 纹理就绪(排水) 512x512`，
  截像素级确认（裸 PNG 解析）：**中心 200×200 区域 100% 非背景像素**
  ——fetch→decode→排水上传→绘制全链实锤。
- Android 真机验证待执行（用户实机清单；代码路径与桌面同构）。

## 批次 31（2026-09-25）：Device/窗口生命周期解绑——wgpu 本形初始化序

> 用户裁决：批次 30 的惰性槽方案"是两边生命周期不一样进行强兼容，丑陋"，
> 授权走 **wgpu+winit 初始化生命周期的本形**（API 风格仍 pygame）。
> 解法不是"构造点阻塞等 Device"（批次 9~11 已否决：单线程死锁/Web 禁止），
> 而是**解绑**——wgpu 的 Device 本来就不需要窗口：Device 先行创建、窗口
> 后挂，三个惰性槽全部消失。

### 设计背景

- 批次 30 遗留的丑：PyTexture 三状态槽（pending/gpu/bind）+ draw 幂等
  分支 + 注册表，本质是"纹理上传"被迫绑定到"窗口表面存在"上。
- 事实：wgpu Device/Queue 与 Surface 无关——纹理是**设备资源**，与窗口
  毫无依赖。解绑后"构造即上传"自然成立。
- `EventLoop::create_window`（构造点提前建窗路线）已废弃且警告不当创建
  ——不采用。

### 设计方案

**base（加法，零破坏）**：

- `RenderContext.surface` → `Option<Arc<Surface>>`；新增 `new_headless`
  构造与 `resource_access(color, depth)`（本 context 的 device/queue +
  指定默认格式的访问层，pub）。
- `GpuSettings::to_adapter_headless()`：`compatible_surface: None` 的
  适配器请求（混合显卡机型按电源偏好取默认，不受"与显示窗口兼容"约束，
  已注明）。
- `RenderEntry::new_context / async_new_context`：无窗口独立上下文——
  "Device 存在"与"窗口存在"两个时刻正式解绑（设计稿 §4.4 设备/表面
  生命周期的引擎侧落点）。

**pygame-rs（槽位全消）**：

- `rpy` 设备先行槽 `DEVICE_SLOT { context, access(占位格式) }`：
  `ensure_device_slot()` 幂等装配（首张纹理构造触发；无头模式拒绝），
  `with_device_access` 出借（优先运行期 Gpu，其次先行槽）。
- `assemble_gpu` 升级：先行槽有存货 → `surface_from_context` 复用既有
  Device 挂窗口表面，access 以真实表面格式重建（纹理/bind group 与格式
  无关，先行创建的照常可用——同一 Device 实例内 wgpu 对象互通）。
- **PyTexture = { material, width, height }**：构造 = 解码 + Device 确保
  + 直接上传 + bind 入注册表，一步到位；draw 零分支。
- `set_mode` 返回 **Screen 对象**（§3.0 类型分立落地：真 pygame 同款
  "set_mode 即窗口句柄面"；width/height getset + repr）。with 渲染协议
  （`screen.render()` pass 作用域）为后续批次。

### 关键保证

- **脚本时序回归 pygame 本形**：`Texture` 构造返回 = GPU 就绪，任何帧
  可绘；构造早于/晚于 `set_mode`、帧内构造——同一语义，无窗口期。
- **错误语义诚实**：无头模式/Device 装配失败 → 构造点确定性报错
  （"图形设备不可用"）；Web 文件装载仍待 io fetch 异步句柄（平台边界
  如实暴露，不为统一而装统一）。
- **同一 Device 实例内互通**：先行 access 建的纹理/bind group 与运行期
  access 建的管线在 wgpu 同 Device 内合法混用；管线必须以真实表面格式
  构建（resource_access 文档注明占位/真实两段式）。

### 测试状态

- pygame-rs `cargo test` **38 passed**；starfish `cargo test` 全绿
  （base 加法零破坏）；wasm32 check 通过。
- **实机**：`render_hello` 日志序即新时序——set_mode 记账 → Device 先行
  装配（Texture 构造触发）→ run → 窗口表面挂载（复用先行 Device）→
  120 帧零校验错 → **exit 0**。

## 批次 30（2026-09-25）：pygame.render 绑定最小闭环——贴图四边形实机上屏

> 语义层（批次 29）测试全绿后的绑定壳批次：`pygame.render` 模块 +
> `Texture` 原生类 + 帧末 flush 渲染管线，`render_hello` 示例 120 帧
> 旋转贴图上屏，进程 exit 0。

### 设计背景

- 推进顺序（用户定案）：语义层先验证、绑定随后；本批次 = "最基本的
  渲染跑通"。
- pygame-ce 调研实锤：**不存在 `pygame.render` 模块**，真实 GPU 面 =
  `pygame._sdl2.video` 实验模块（Renderer/Texture/Image，无独立
  Sampler——采样质量是 Texture 参数）。绑定类名对齐该真实面，不照搬
  SDL3 GPU 命名（RenderDevice/Sampler/Buffer 查无实据）。
- 模块定位（用户定案）：render = **新 API 路线的地基模块**；旧
  Surface/blit 光栅模型是建在其上的"尽可能兼容"支持层。

### 设计方案

- **`pygame.render` 子模块注册**（`dependencies/pygame/render/`，
  BatchModule 注册流第 6 步）：挂 `Texture` 静态类
  （rustpython-derive，`make_static_type` 路径）。
- **Texture 生命周期 = §4.3 惰性句柄最小形态**：构造期只解码图片进
  内存（模板期无 GPU——引擎 run 装配在 `pygame.run` 之后）；**首绘
  惰性上传**（第一帧 on_frame 先 `ensure_gpu_lazy` 再步进脚本，脚本
  draw 时设备必在）。Web 文件装载确定性报错（io fetch 异步句柄属
  后续批次）。
- **`Texture.draw(dest, area=None, angle=0, flip_x=False, flip_y=False)`**
  （FuncArgs 手工绑，关键词惯用形态）：dest (x,y) 取纹理原尺寸 /
  (x,y,w,h) 拉伸；area 像素子矩形换算 uv（图集）；angle 绕中心旋转
  （`Transform2D` 烘顶点——语义层负责几何）；flip = uv 交换。
- **帧批记账/消费**：draw 记账进绑定层帧批（材质 id 分桶，语义层
  `Batch` 承载 + 新增 `drain()`）；帧末 `rpy::flush_render_frame`
  取批清零。
- **flush 渲染管线**（rpy，Gpu 槽扩 `RenderObjects`）：相机 MVP
  （`Camera2D` 像素正交 y 向下，视口 = 表面尺寸，每帧写跟随 resize）
  → 批物化（v1 逐帧重建 Mesh）→ 管线缓存（首个 mesh 作布局模板）→
  clear pass + 绘制 pass（逐材质桶换绑 group1）→ present。批空退化
  纯清屏。材质 bind group 住绑定层注册表（`BindGroup` 非 Clone——
  `with_material_bind` 出借引用）。
- **示例** `examples/render_hello*`：居中 256² 贴图逐帧旋转，30~60 帧
  换 flip_x，120 帧 return → 引擎收账 exit 0。

### 关键保证

- **模板期资源声明可用**：`tex = pygame.render.Texture(path)` 在
  `pygame.run` 之前合法（不触 GPU），首绘自动上传——惰性句柄哲学的
  最小落地，无头模式误用报确定性错误。
- **每帧一次资源创建的已知简化**（v1，注释在案）：Mesh 逐帧重建；
  全批共用 TEXTURE 变体管线。持久 Mesh + COPY_DST 覆写与多管线选择
  为既定后续。
- **分层红线不破**：base 零改动；GPU 资源住 rpy/绑定注册表，语义层
  零 GPU 触点不变。

### 测试状态

- pygame-rs `cargo test` **38 passed**（+1：`Batch::drain` 取批清零）。
- **实机闭环**：`cargo run --example render_hello`（AMD Vulkan,
  IntegratedGpu）——init/set_mode/run/GPU 装配日志齐，120 帧绘制零
  wgpu 校验错（有错即 panic 非零退出），**exit 0**。像素级确认归
  readback/肉眼（无头截图是 web 手段）。

## 批次 29（2026-09-25）：render 语义层（Rust 层先行）——通用 2D/3D 渲染管线

> 推进顺序（用户定向）：**先在 Rust 上实现并验证 `src/pygame/` 语义层
> （`cargo test` 全绿），通过后再做 rpy 绑定**。本批次 =
> 设计稿 §4.4 统一渲染管线（"2D = 正交相机 + 贴图四边形 + 关深度的 3D"）
> 的语义层本体，`rpy` 绑定零改动。

### 设计背景

- §4.4 定案方向（批次 26）尚无代码：统一顶点布局 pos3+uv2+color4、
  ColorMaterial/TextureMaterial、统一绘制入口、管线变体矩阵。
- `dependencies/pygame/render/` 三个占位注释文件（批次 28 在途脚手架）
  之外，render 模块零实现。
- 探索结论（本日盘点）：**base::font 即目标形态的活体模板**
  （TextVertex pos3+uv2+color4 / group0 相机 + group1 纹理采样的
  WGSL 布局 / 2d·3d 管线变体 / 静态 Mesh 脏重建）；面 B 全链路通用 API
  在位（mesh_builder 自定义顶点、bind_group_builder 纹理采样绑定、
  裸 64B MVP uniform 相机惯例）；`RenderPass::end(self)` 按值消费——
  base 注释明说是为 Python with/`__exit__` 预留。

### 设计方案

**base 零改动**：统一顶点/材质/管线约定全部住 pygame 语义层
（`pygame-rs/src/pygame/render/`，wgpu/glam 已在 pygame-rs 依赖图），
`base/gfx` 的 ShapeVertex 不动。新增依赖
`glam = { version = "0.33.2", features = ["bytemuck"] }`（与 starfish
合并为同一实例）。

纯逻辑五件（全部 `cargo test` 可测）：

| 文件 | 内容 |
|---|---|
| `vertex.rs` | `UniVertex{pos3,uv2,color4}`（36B，Pod）+ `layout()` + `quad()`（xy→uv 对射、tint 直入顶点色）+ `QUAD_INDICES` |
| `camera.rs` | `Mvp`（64B 列主序 uniform，Pod）·`Camera2D`（像素正交 **y 向下**，视口中心可平移）·`Camera3D`（persp，RH 惯例）——引擎"自算 MVP 写 uniform"惯例的类型化补全 |
| `material.rs` | `MaterialKind{Color,Texture}` ·`Topology` ·`VariantKey{blend,textured,topology}`（变体矩阵纯逻辑面；base BlendMode 直接复用） |
| `batch.rs` | `Batch`（按 VariantKey 分桶的顶点/索引积累，u16 索引 debug 断言）·`Transform2D`（绕 origin 缩放→旋转→平移，§4.4"transform → 顶点变换"的落地）·`push_geometry`（gfx Geometry→UniVertex，uv 置零、顶点色×tint） |
| `target.rs` | **RenderTarget 协议的语义层本体**：`PassAttachments`（Screen 与离屏 Surface 同构——都只是附件组）+ `PassRecorder`（with 块作用域录制）→ `finish()` 产出 `PassPlan` 回放清单；真实视图与回放执行器住 rpy 绑定层（后续批次） |

薄 GPU 门面（对齐 gfx/font 范式：无 Renderer、标准对象归开发者）：

- `unified.wgsl`：group0 相机 mat4 / group1 纹理+采样器；
  **最终色 = 采样 × 顶点色**——纯色材质 = 1×1 白纹理技巧，同一条管线
  服务纯色与贴图，变体矩阵收敛（混合 × 拓扑）。
- `pipeline.rs`：`white_texture` / `nearest_sampler`（pygame 默认最近邻）
  / `linear_sampler`（smoothscale 对应物）/ `camera_buffer`+
  `camera_bind_group`（64B 裸 uniform 惯例）/ `material_bind_group`
  （texture 0 + sampler 1，全材质同布局）/ `bucket_mesh`（COPY_DST
  动态批）/ `pipeline_2d/3d`（2D 关深度，3D 开深度）。

### 关键保证

- **语义层可独立验证**（推进顺序的前提）：纯逻辑件零 GPU 触点，26 个
  新测试全部 `cargo test` 覆盖；GPU 门面只是 base 标准 builder 的组装，
  实机链路验证随 rpy 绑定批次的首个示例闭环。
- **tint 烘焙进顶点色**（偏离 §4.4 材质参数 uniform 一处，已注明）：
  `bind_group_builder` 的 uniform 可见性硬编码 VERTEX（base 侧既定），
  fragment 侧材质参数暂不可绑——顶点色乘制语义等价（font 同款），
  builder 支持双阶段可见性后接 binding2。
- **u16 索引纪律**（gfx 同源）：单桶 >65535 顶点 debug 断言，提示分桶。
- **分层红线不破**：base 零改动零 pygame 依赖；语义层不依赖 rpy；
  录制层不持有真实 GPU 视图（描述与句柄分离，回放执行器归 rpy）。

### 测试状态

- pygame-rs `cargo test` **37 passed**（26 渲染语义层新测试 + 8 M2 +
  2 color 桥 + 1 rect）；render 模块零警告。
- wasm32-unknown-unknown `--lib` check 通过（glam/wgpu 均跨平台，
  语义层全量参编）。

## 批次 28（2026-09-25）：绑定入口标准落地——BindingModule 静态表 + Event 原生类 derive 化

> 收束批次 26 遗留的半迁移态：`bindings.rs` 旧单体注册退役（Event 以
> `(type, key)` 元组近似，无属性面——t2/t3 失败根因），绑定实现按设计稿
> §5.1/§5.2 全量落位 `rpy/dependencies/pygame/`，Event 升级为
> rustpython-derive 静态类型。

### 设计背景

- 批次 26"全原生化"进行到一半：`dependencies/pygame/{mod.rs,event.rs}`
  已写好新版实现但**未声明进模块树**（`rpy/mod.rs` 无 `mod dependencies;`，
  整个目录不参编），`bindings.rs` 仍是旧单体 `register_all`（元组事件）——
  运行的永远是旧路径，M2 测试 t2/t3 因 `e.type` 属性访问失败。
- 新版 `dependencies/pygame/mod.rs` 引用的 `BindingModule` trait 尚不存在、
  `present_frame` 只有悬空注释、`sub_module` 定义了但没调用、`mod event;`
  未声明——四处断线。

### 设计方案

- **入口标准**（`bindings.rs` 重写）：`BindingModule` trait（`name()` +
  `register(vm)`；`Sync` 超 trait 供静态表持有）+ `BINDINGS` 静态表 +
  `register_all` 总入口。三个装载入口（`engine_main` / `run_headless` /
  `exec_probe`）零改动。
- **实现下沉接线**（`rpy/mod.rs` 声明 `mod dependencies;`）：
  `dependencies/pygame/mod.rs` 补 `mod event;`、Event 类挂载
  （`pygame.Event`）、display/event/draw/time 四子模块挂载（t2/t3 依赖
  `pygame.event.get`）；`ensure_gpu_lazy` 导入随实现 cfg 门控（wasm 不参编）。
- **Event 原生类 = rustpython-derive 静态类型**（`dependencies/pygame/
  event.rs`，旧动态堆类型草稿 `rpy/event.rs` 删除）：struct 级
  `#[pyclass]`（PyClassDef + StaticType）+ `#[derive(PyPayload)]` + impl 级
  `#[pyclass(with(Constructor, Representable))]`。属性面 `type`/`key`/
  `pos`（pygetset，与 `pygame/__init__.pyi` 契约同形）。
- **Python 侧构造 = Constructor trait**（vendor derive 无 `#[new]` 宏）：
  `Args = FuncArgs` 手工绑定——元组 `Args` 只收位置参数，而
  `Event(type="quit", key=27)` 关键词形态是事件对象的惯用面；
  `take_positional_keyword` 三参双收 + 余量校验（多余位置/未知关键字报
  TypeError）。
- **`present_frame` 补齐**（rpy/mod.rs，统一帧管线第三环）：核心状态
  `clear_color` → `RenderSurface::begin_frame` → `present`；GPU 槽未装配
  静默跳过。Real 模式 on_frame 闭环为
  `ensure_gpu_lazy → step_via_slots → 退出闸门 → present_frame`。

### 关键保证

- **静态类初始化路径分野**（本次实测复踩，已写进 event.rs 注释）：扩展
  模块必须走 `PyClassImpl::make_static_type()`（static_cell 幂等 +
  extend_class + 基类槽继承）；VM 核心 types/zoo 专用的
  `StaticType::init_builtin_type()` 裸建类**不填槽**——getattro 为 None，
  实例属性访问直接 panic。二次 init 恒 panic（"double initialization"），
  注册期恰好一次，实例构造一律 `static_type()` 取同一类对象。
- **类对象单例**：`isinstance` / match 类模式对 `pygame.Event` 恒成立
  （旧动态类方案做不到——每事件构造新类对象）。
- **事件语义不变**：`event.get` 排空缓冲 → 原生 Event 实例列表；
  `type: str`（locals 常量同源）/ `key: int` / `pos: tuple`。
- **平台零分叉**：条件编译只出现在"谁来驱动"边界（桌面 ensure_gpu_lazy /
  wasm async 装配），应用代码无 cfg。

### 测试状态

- pygame-rs `cargo test` **11 passed**：8 M2 行为（含新增 **t7
  Event 原生类**：事件属性三元组 + match 类模式解构 + Python 侧关键词
  构造 + pos 属性，锁 derive 类完整行为面）+ 2 color 桥 + 1 rect。
- `cargo check --examples` 通过；`cargo check --target
  wasm32-unknown-unknown --lib` 通过——dependencies/pygame 的 wasm 分支
  （on_start 异步 GPU 装配 / gpu_install / with_web_canvas_id）首次参编，
  M3 编译态不回退。

---
