# starfish 更新日志 2026-10-23 —— 多窗口支持：上下文/表面分离模型定稿 + we 平台白名单

## 设计背景

- **诉求**：crates/starfish-we 与 crates/starfish-render 落地多窗口。文献
  结论"多窗口下除 surface 外一切不重建"——在本架构已半落地：render 侧
  `new_context`（设备先行 headless 上下文）与 `surface_from_raw_handles`
  （复用 instance/adapter/device/queue 挂表面）早已存在，但形态有三个
  历史包袱：① `surface_from_context` 返回**第二个 RenderContext**（每窗
  一份内容完全相同的 ctx，纯冗余）；② `RenderContext.surface` 字段零
  消费者（单表面残留死重）；③ 注释自贬"仅开放，不具备开箱即用能力"。
- **we 侧单窗口不变量**三处落点：`window.rs` 的 `WINDOW_ALIVE` 全局占位
  （build 时 swap 检查）、web 平台 `STATE` thread_local、Android slot reset。
  关键利好：**Win32 后端天然支持多窗口**——`WndState` 本就 per-window
  （`GWLP_USERDATA` + per-hwnd wndproc），`poll_event` 泵线程级消息队列后
  各窗消息进各自 pending。
- **用户定稿**：多窗口行为平台差异过大，必须**编译期**让开发者知道目标
  平台不支持（不做运行期静默降级）；事件拓扑不要求与 SDL 完全一致，
  顺手即可。

## 方案（落地形态）

### render 侧：上下文/表面分离模型（接口收敛）

- **`RenderContext` headless 化为唯一形态**：删除零消费的 `surface` 字段；
  `new_headless` 并入 `new`（7 参，不再收表面）。
- **新多窗口核心入口**（feature `we`）：
  - `RenderEntry::new_surface(&ctx, &window, SurfaceSettings) -> RenderSurface`
    （阻塞桌面版）；
  - `RenderEntry::async_new_surface(...)`（异步版）；
  - `RenderEntry::async_new_surface_from_canvas(&ctx, canvas, ...)`（Web
    多 canvas 预留入口）。
  返回值只有 `RenderSurface`——多窗口 = 一个 RenderContext + N 个
  RenderSurface，surface 之外一切不重建。
- **删除** `surface_from_context` / `async_surface_from_context`（返回冗余
  ctx 的历史形态；全仓 grep 零调用方）。保留 raw handles 族与
  `surface_from_foreign_device`（第三方框架/外部设备场景，与多窗口无关）。
- **`new` / `async_new`（单窗口一步到位）签名不变**——pygame 层
  （`pygame::render::target`）与既有示例零迁移成本；其 surface-first 适配
  器选择路径原样保留（设备先行的混合显卡差异维持既有文档约定）。
- **多窗口共享约定**（落接口文档）：各窗口 `SurfaceSettings` 一致（默认即
  一致）→ 一份 `RenderResourceAccess` + 同参数管线全窗口共享；格式/MSAA
  不同的窗口以 `ctx.resource_access(格式, 格式)` 另建，管线按
  `(color_format, sample_count)` 分套（wgpu 硬约束，不得跨键复用）。

### we 侧：feature `multi-window` + 编译期平台白名单

- **feature `multi-window`**（默认关，零依赖）：开启后 `WINDOW_ALIVE`
  占位检查跳过（cfg 门控），`Window::builder().build()` 可多开；
  关闭时语义与此前完全一致（第二次 new 仍报 `AlreadyExists`）。
- **白名单 compile_error**（`we/src/lib.rs`）：
  `#[cfg(all(feature = "multi-window", not(target_os = "windows")))] compile_error!`
  ——白名单外目标直接编译失败。当前白名单：**Windows**（Win32 消息泵
  天然 per-window）。Linux/macOS 逐平台实测通过后向白名单追加 `target_os`
  即可；Web/Android 属语义性不支持（单 canvas / 单 Activity），维持白名单外。
- **事件拓扑（定稿）**：从哪个 `Window` poll 出的事件就归属谁——无 ID
  路由（相对 SDL"全局队列 + windowID 反查"的差异是加分项：归属由类型
  系统保证）。泵纪律：**每帧轮询每个窗口**（漏 poll 的窗口事件积压在
  自己 pending，不丢失但该窗界面停摆）；键盘进焦点窗（各平台焦点模型，
  SDL 同款）。 Crate/Window/AlreadyExists 文档同步改写。

### 验收示例

- `crates/demos/examples/multi_window.rs`（`[[example]]` 带
  `required-features = ["multi-window"]`；demos 增 feature 透传
  `multi-window = ["starfish-we/multi-window"]`）。形态：双窗各一清屏色
  （蓝/橙），独立 `begin_frame`/`present`；关任一窗另一窗继续跑，全关
  干净退出。`Win` 结构体字段序 = drop 序（surface 先于 window 销毁——
  wgpu surface 不得在窗口销毁后 present 的 'static 句柄契约）。
  无 `_android` 双注册（Android 白名单外编译期报错）。

## 关键保证

1. **默认形态零变化**：不开 feature 时 we 单窗口语义（`AlreadyExists`）、
   render 全部既有入口（`new`/`async_new`/`new_context`/raw handles 族）
   签名与行为不变；web/Android 构建路径零改动（cfg 隔离）。
2. **编译期平台把关**：多窗口诉求在白名单外平台于编译期报错，杜绝
   运行期半残行为。
3. **surface 之外不重建**：`new_surface` 复用 context 的
   instance/adapter/device/queue，仅新建交换链（含深度/MSAA 随表面）。
4. **销毁顺序契约**：surface 必须先于其窗口 drop（示例以字段序落实）。

## 测试状态

- `cargo check --workspace`（默认形态）✅ 全绿（rapier-sf dead_code 为
  存量警告）；
- `cargo check/build -p demos --features multi-window --example multi_window`
  ✅；实机冒烟 ✅：双窗进程稳定运行（begin_frame 持续 acquire ok，
  ~30 loop/s = 双 Fifo surface 顺序 present 预期节奏），taskkill 收尾；
- **待人工验收**：逐窗点 X 关闭归属（关主窗副窗继续跑 / 全关退出）——
  自动化 SendMessage 被会话权限拦截，示例标题已自带验收指引；
- Web/Android 不受影响（feature 默认关 + cfg 隔离，未重出包）。

---

## 批次二：rapier-sf 迁出 workspace（独立项目化）

### 背景与决策

- rapier-sf（Rapier2D 物理适配层骨架，三文件：`Cargo.toml`/`lib.rs`/
  `world.rs`，step 空转、debug_geometries 返回空）原在 `crates/rapier-sf`
  由 `members = ["crates/*"]` 通配收编。物理适配层的演进节奏与引擎主线
  不同步，迁出为独立项目：**`D:/Projects/Rust/rapier-sf`**。
- 迁移内容：三源文件原样搬运；manifest 自持化（workspace 继承字段内联，
  starfish 家族依赖改 `../starfish/crates/*` 路径依赖）；复制本仓库
  `.cargo/config.toml` 镜像配置（直连 crates.io 不通，坑位 8 在新仓库
  同样生效）；补 `.gitignore`（/target）。feature 从骨架的
  `default=["dd","ddd"]`（2d/3d 双开，代码只用 2d）规范为
  `default=["2d"]`、`3d = ["dep:rapier3d"]`（不点亮）。
- 参考基线：`D:/Projects/Rust/rapier_render_test`（rapier 0.36
  `DebugRenderPipeline` + 自定义 `DebugRenderBackend` 收集线段 → LineList
  直绘），预设计文档已吸收其形态。

### 预设计定稿（全文见 `rapier-sf/doc/设计文档.md`）

- **gfx 够用性分析（用户问询定案）**：gfx 顶点格式
  （`ShapeVertex{pos3,color4}` 可自构）、线拓扑家族（`shape2d::line` 等
  全 LineList）、管线（`line_pipeline_2d` 现成）三面都兼容物理 debug
  线段流——**功能够用，v1 绘制面采用 gfx 积木**；已知天花板 =
  `Geometry` u16 索引（≈2.1 万线段/帧上限），换轨点内建（`LineSink`
  独立成模块，触发即换 render 直绘自持管线，接口不动）；
- **we 定案**：库本体零依赖、不设 feature——render 公共面不含 we，
  开窗/事件/循环归宿主；we 仅进 `[dev-dependencies]` 供 demo 示例；
- **"不懒人化"落点**：不复刻 rapier API（骨架 `BodyDesc` 平行描述结构
  在 M1 删除，rapier builder 直通 + `world.get()` 逃生口）；
- 渲染内容/类型控制 = `DebugRenderMode` 位 + `DebugRenderStyle` 直透
  （rapier 原生配置面），收集器与绘制器分离。

### 测试状态

- 迁出后 `cargo check --workspace`（starfish 侧）✅ 全绿；
- `D:/Projects/Rust/rapier-sf` 独立 `cargo check` ✅（骨架 dead_code
  警告为存量；首次编译暴露新仓库缺镜像配置，已复制解决）；
- v1 落地（M1 world 薄封装起步）在 rapier-sf 仓库另批推进。
- **同日追记**：rapier-sf 侧 M1~M3 已全部完成（25/25 单测绿 + demo 实机
  冒烟 ~130fps 零校验错误）——物理世界薄封装/固定步长积分器/通用 debug
  渲染三件套（数据面 LineSink + 绘制面 DebugLinesRenderer）落地，验收
  示例 `debug_render_demo`（键位 1-5 切换渲染内容位、+/- 调缩放）。
  实测坑清单（HSLA 色彩/Mesh 计数定值/2D 管线无深度等）见彼仓库
  `doc/设计文档.md` 变更记录 v0.3。
- **同日追记二**：rapier-sf 3D 维度点亮（M4a）——feature `2d`/`3d` 互斥
  双编译、手写 look-at 透视相机、`debug_render_demo_3d` 环绕相机实机
  冒烟通过；双形态测试 3D 27/27 + 2D 回归 26/26。定约见彼仓库设计文档
  §7 与变更记录 v0.4。
- **同日追记三**：rapier-sf **改名 rapier-starfish-debug** 并上传 GitHub
  （GuYeying/rapier-starfish-debug，main）；维度定案升级为**双开并存**
  （`dim2::`/`dim3::` 命名空间，三形态测试 26/27/44 全绿）；debug 渲染器
  明约"非注入式"，`camera_bind()`/相机矩阵公开供宿主自建 gfx 管线共享。
  后续彼仓库演进以 GitHub 仓库与彼 README/设计文档为准。
