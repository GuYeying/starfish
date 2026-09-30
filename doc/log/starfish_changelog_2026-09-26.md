# Starfish 更新日志 2026-09-26

## 批次 34（2026-09-26）：render_hello 双包——APK + Web 出包

> 用户指令："为当前的项目打包 apk 和 web"。render 模块最小闭环
> （批次 30~33 成果）首次三平台分发。

### 设计方案

- **Android 双注册**：`[[example]] render_hello_android`（cdylib，同源
  文件）——xtask `resolve_android_example` 按约定解析。
- **资产引导落盘**（kit asset_path 惯例）：`render_hello.rs` 入口
  `bootstrap_asset()` 把内嵌 `wall.jpg`（include_bytes）写到
  `io::resolve("resources/textures/wall.jpg")`——**脚本三平台同一
  字符串**：桌面 = pygame-rs/resources/（一次性生成物，入 .gitignore）；
  Android = 私有目录（base_dir 已在捕获点注入，批次 32）；Web 无文件
  系统，fetch 直达 server 挂载的 `/resources`，引导按 cfg 跳过。
- **出包**：APK = `cargo xtask android render_hello --dir pygame-rs
  --build`（四步管线：构建 → APK 组装 → 签名 → 产物）；Web =
  wasm release 构建 + wasm-bindgen → `pygame-rs/web/`。

### 关键保证

- 脚本单源三平台（`include_str!` 内嵌给 Android；fetch 给 Web；fs 给
  桌面），资产同串解析——新平台接入 = 双注册 + 资产引导两处。

### 测试状态

- 桌面：bootstrap 落盘 + 全链 exit 0。
- APK：`pygame-rs/target/android-apk/render_hello_android.apk`
  （四步管线全过，签名校验通过；真机安装待用户实机清单）。
- Web：`pygame-rs/web/render_hello.html`（wasm 47.8MB + js + 脚本；
  server.py `--web-dir pygame-rs/web --resources-dir resources` 起服，
  默认端口 8021）——批次 33 已像素级验证，本批仅同步资产路径重出包。

### 实测修正（用户真机反馈：Web 黑屏 / Android 闪退）

- **第一层根因 = 脚本 120 帧自动退出**（无头验证遗留）：真机上跑约
  2 秒即返程——Web：脚本 return → 引擎退出 → winit web 的
  `unreachable!` 陷阱 → WebGPU 上下文销毁 → 黑屏；Android：
  `process::exit(0)` 自杀 = 观感闪退。修正：脚本改**常驻循环**
  （QUIT/ESC 退出，Android 返回键 = QUIT）；render_hello 补 web start
  块 + `install_panic_hook`。
- **第二层根因（常驻后暴露，用户复测仍黑屏/闪退）= 双 bug 叠加**：
  1. **winit-web Wait 帧链冻结**（base 修复）：帧内 `request_redraw`
     发生在 runner `borrow_mut` 期间 → 重绘事件只能排队；而
     `apply_control_flow` 对 **Wait 状态不挂任何调度器** → 排队重绘
     永无人清 → **首帧后帧链冻结**。虚拟时间验证会掩盖它（虚拟 rAF
     连发持续排水）。修复：web 每帧重挂
     `ControlFlow::WaitUntil(now + fps_cap/16ms)`（base/app.rs
     about_to_wait + `web-time` 依赖）——setTimeout 驱动帧链自持。
  2. **vendor RP 的 f64 陷阱**（批次 30 遗留，双平台）：此版
     `f64: TryFromObject` **只接受精确实例 PyFloat**，Python int 被
     拒（"Expected type 'float' but 'int' found"）——`dest=(192, 112,
     256, 256)` int 元组在**首绘即 TypeError** → 生成器死 →
     script_quit。此前桌面"exit 0"验证被静默早退骗过（首绘死也是
     exit 0）。修复：`arg_f64`（int/f64 双试，texture.rs，draw 参数
     解析共用件）。
- **验证**：桌面 flush#1~12 **批=1**（修复前同点位静默死）；Web
  常驻无退出 + 截图中心 100% 非背景；双 ABI APK（armv8 + x86_64 模拟
  器包）重出。
- **Android 模拟器实测（x86_64 AVD）**：资产引导/Device 先行/with 块
  记账/flush 全链日志正常，**3000+ 帧持续渲染、进程存活、退出原因
  零**——三层修复后模拟器不再闪退。遗留观察：模拟器表面黑屏（99% 纯
  黑，清屏色也不可见）——渲染进入陈旧 ANativeWindow（启动期窗口对象
  被系统重建，自愈重配置的是旧窗口；批次 22 已记录"模拟器 GL 下引擎
  探针同样异常"，模拟器 GPU 环境为已知问题域）。**真机复测（新 armv8
  包）为最终判据**；模拟器黑屏作为已知问题挂账（方向：ANativeWindow
  重建监听 / Surface 重建挂到 Resized）。
- **教训入册**：① 分发示例必须常驻；② web start + panic hook 是
  wasm 示例标配；③ **"exit 0" 不构成帧验证**——帧计数锚点（flush#
  逐帧 + 帧循环退出原因）已常驻化，进分发验证清单；④ vendor RP 数值
  参数一律 int/f64 双试；⑤ `begin_frame`/`present` 失败跳帧不 panic
  （渲染表面"未就绪即跳过"），Android 启动期表面抖动不再炸线程；
  ⑥ **MSAA resolve 必须与帧命令同批提交**（present 前完成）——迟一帧
  提交写的是已 present 的旧交换链图，MSAA 表面即永久黑屏（本批第四层
  根因，顺带消除桌面 MSAA 下的一帧滞后）。

---

## 批次 33（2026-09-26）：with 语法 pass 作用域——Screen.render + RenderPass

> 用户定向的下一垂直面：`with screen.render() as r: r.draw(...)`——
> 语义层批次 29 的录制协议在此接线。同批搭车：render.pyi 契约桩补账、
> Texture 桌面装载路径统一走 `io::resolve`。

### 设计背景

- 语义层 `target.rs`（批次 29）的 RenderTarget 协议录制件已备；本批把
  **with 生命周期**接到绑定层，Screen 从纯数据句柄升级为渲染入口。
- 用户定案形态（兼容层架构 §3.1/§4.4 支柱④）：主屏高级对象 + 离屏
  `with` 语法 render pass 闭环。本批先立主屏协议；离屏目标分化随后。

### 设计方案

- **`Screen.render()` → `RenderPass` 对象**（`dependencies/pygame/render/
  pass.rs`，derive 静态类型）：`__enter__` 返回自身；`draw(texture, dest,
  area, angle, flip_x, flip_y)` 记录四边形意图（参数解析与 Texture.draw
  **共用 `parse_draw_args`/`DrawSpec`**——同一提取自 texture.rs，语义面
  完全一致）；纹理状态闸门 `ensure_drawable`（Loading 静默跳过 / Failed
  报错）同样共用。
- **`__exit__` = 意图并入帧批**（不自行 present）：pass 内绘制经
  `record_quad` 入 `FRAME_BATCHES`，帧末 `flush_render_frame` 统一
  clear+绘制+present——与"帧末统一提交"架构一致，避免双 present 冲突。
  离屏目标（Texture 作 target）需要目标分化时再引入 PassPlan 回放执行器。
- ** RP with 协议**：`#[pymethod] fn __enter__(zelf: PyRef<Self>)` /
  `fn __exit__(&self, FuncArgs, vm)` 返回 None(falsy) 不抑制异常——
  vendor 版 with 语句与 derive 方法直配，无额外机制。
- **Texture 路径统一 `io::resolve`**（配套批次 32 的 Android base_dir
  提前注入）：桌面装载 `image::open(io::resolve(path))`。base 侧为此把
  `io::resolve` 从 imp 模块提为公开面（路径规则单一事实源；一行加法）。

### 关键保证

- **脚本面**：`with screen.render() as r: r.draw(tex, ...)` 三平台同形；
  pass 作用域内绘制保序、成组；块内异常不被吞（__exit__ falsy 返回）。
- **与直绘等价**：`r.draw` 与 `tex.draw` 共用参数解析与帧批路径——像素
  输出逐位一致（截图对比同为 100% 中心覆盖）。
- **零新增机制**：无注册表、无回调——with 只是录制分组，提交仍走帧末
  单点。

### 测试状态

- pygame-rs `cargo test` **39 passed**（新增 **t9 with 协议**：headless
  下 `Screen.render()` + `__enter__/__exit__` 生命周期往返；真实绘制经
  render_hello 实机验证）。
- **桌面实机**：with 块版 render_hello exit 0（时序日志同批次 31）。
- **Web 实机**：锚点 `GPU 装配完成 → 纹理就绪(排水) 512x512` + 截图
  裸 PNG 解析中心 200×200 **100% 非背景像素**——with 块路径像素级等价。
