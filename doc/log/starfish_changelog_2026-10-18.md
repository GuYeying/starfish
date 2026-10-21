---

## v0.9.21 发布注记（2026-10-18）

**主题：绑定层状态归属收编（pygame-rs bridge 瘦身七步的引擎侧）**——
原 pygame-rs 绑定桥私有的四块引擎行为（渲染会话槽/混音器生命周期/
事件队列/字体预载缓存）升格为引擎公共 API；绑定桥从 691 行瘦身为
293 行纯 RP 薄壳。原则：**行为一致**——Rust 用户与 Python 用户同一
套引擎行为，绑定层零引擎状态。

### 一、新模块（状态归属收编）

- **`pygame::render::session`**（新文件）：活跃绘制会话的 thread-local
  承载（ADR-5 `with` 语法的引擎侧状态）。双槽并行（约定互斥不嵌套）：
  Screen 槽（`begin_screen`/`end_screen`/`with_screen`——get_screen 的
  &'static 使 `RenderPass<'static>` 天然成立，无 unsafe）+ Targets 槽
  （`begin_targets`/`end_targets`/`with_targets`）。**MRT 目标 Arc 保活
  表从绑定桥收回引擎**——`render_targets_arc` 的 SAFETY 契约
  （"目标由调用方保活至 finish"）就地兑现为 TARGETS_KEEPALIVE
  （end 时 finish 先行、清表在后），裸调 `render_targets_arc` 者自担
  保活（注释已改写）。错误面 `NoActiveSession`（绑定层映射 Python
  异常，Rust 侧直接处理）。
- **`pygame::mixer`**（新文件）：混音器生命周期 facade——thread-local
  单例保活（构造即开设备、Drop 即停）、`init(channels)`/`quit()`/
  `get_init()`/`with`（未 init 惰性建 8 通道）、内嵌标准提示音
  `beep()`（include_bytes 三平台零 IO）、`pump`（Web 流式解码驱动，
  桌面 no-op）。单测：beep 解码。**Web 自动播放策略 = 部署约定**
  （2026-10-18 同日定约修订）：原运行时手势门
  （`needs_gesture`/`gesture_locked`/`mark_gesture` + 事件泵手势标记）
  已撤——浏览器要求 AudioContext 创建落在用户手势调用栈内，本层不做
  任何检测，由宿主页面保证首次手势先行（嵌入页必然先点击；独立页用
  "点击启动"覆盖层把 wasm 启动挂进 click 回调）。三平台行为一致：
  音频随时可播，无钩子、无跳过路径、无额外错误处理。

### 二、行为变化（事件 SDL 队列模型）

- **`pygame::event` 队列化**：新增 `pump()`——宿主帧循环每帧一次，
  引擎事件全部翻译灌入内部队列（翻译时照常喂 key/mouse/touch 状态
  表），**窗口关闭请求经返回值上报**；手柄每帧排水（帧间边沿归
  宿主帧管线）。`get`/`poll`/`clear` 改为消费该队列（原实现直通
  窗口拉取）。内部零既有消费者，无迁移面。
- **ESC 退出语义不进引擎**：pump 只报窗口关闭（pygame 正统——QUIT
  不含 ESC）；ESC 退出是脚本层策略（绑定壳原"ESC 置退出旗标"的
  宿主便利已撤，各测试脚本本就自行处理）。
- **Web 音频手势门撤除**（绑定层同步）：`mixer::gesture_locked` 的
  play 前检查、事件泵的 KeyDown/MouseButtonDown/Touch 手势标记、
  `pygame.mixer.needs_gesture()` Python 查询全部移除——Web 场景由
  部署约定承载（pygame-rs 的 build_web.py 模板已内建"▶ 点击启动"
  覆盖层，wasm 启动整体落在首次点击之后，产物天然合规）；原脚本侧
  平台分支（binding_probe 音频双路 / font_test 路径字体平台判定）
  一并删除，三平台同代码。
- **Web 尺寸定约回退定型：页面即尺寸权威**（starfish-window web
  后端，当日多次迭代后用户拍板回退）：**CSS = 位图 = 逻辑尺寸，
  devicePixelRatio 完全不进引擎**——窗口大小控制权 100% 归页面
  （HTML/CSS/JS），引擎只镜像页面给 canvas 的布局：ResizeObserver
  + window resize 共用 `report_layout_change`（纯 CSS rect 去重），
  布局变化 → 位图 1:1 重同步 + 物理 `Resized` → rescale；纯浏览器
  缩放（dpr 变、rect 不变）零动作——纯视图变换，渲染零扰动。事件
  坐标 = CSS 客户坐标 − 原点（1:1）。`high_pixel_density` 恒 false。
  启动窗口期修复保留（set_mode web 分支装配前同步 set_size +
  begin_frame 帧纹理尺寸自检自愈）。**开发者侧窗口大小控制**（弥补
  引擎不做缩放适配）：产物模板 CSS 注释写明布局改法（800×600 默认/
  92vw 铺满/跟随窗口）+ script 注释内置 `setGameSize(w, h)` 运行时
  助手（取消注释即启用）+ `screen.get_size()` 查询；pygame-rs
  CLAUDE.md 已记录。
  **废弃方案谱系**（用户实测四轮定型后回退）：①"物理等大（CSS =
  逻辑/dpr）"——缩放往返物理漂移；②"CSS 补偿重写（物理恒定）"——
  canvas 视觉锁死；③"纯 no-op 解锁 + 比例映射"——同①漂移。结论：
  缩放适配语义无普适解，引擎不越权，控制权交页面。
- **Web 窗口销毁的 DOM 监听泄漏修复**（starfish-window web 后端）：
  `display::quit()` 丢窗后监听器的 Rust 侧 Closure 释放但
  `removeEventListener` 从未执行——此后每个 DOM 事件（按键等）打进
  已释放闭包，浏览器抛 "closure invoked recursively or after being
  dropped"。修复：`Listener` 结构记录 (target, 类型, 闭包)、drop 时
  自动解除；rAF 句柄记录 + drop 时 `cancelAnimationFrame`（已排队
  帧不再打进死闭包）。
- **Web 响应式画布（页面即布局权威）**：canvas 位图跟随页面 CSS 布局
  实时映射——引擎侧新增 `ResizeObserver` 观察 canvas（`ResizeWatch`
  打包保活回调 + drop disconnect），与 window-resize 监听共用
  "CSS rect 去重 → 位图同步 → 物理 Resized" 体（`report_layout_change`
  抽取共用）；flex/grid 重排、DevTools 停靠等不经 window resize 的
  布局变化从此也触发引擎 rescale（swapchain + 正交相机跟随）。宿主
  页面模型简化为"只放一个 `<canvas id="starfish-we">` 标签 + 自由
  CSS 布局"，引擎注入位图与渲染内容；pygame-rs 模板改为无写死属性 +
  自适应 CSS（92vw×86vh，页面改样式即改窗口大小）。固定渲染分辨率
  定约不变：位图 = CSS 布局 × 1，dpr 不参与。配套 `Screen` 尺寸查询
  （pygame 面新增 `screen.get_size()`）。

### 三、API 增补

- **`DrawTarget` 三个 default method**（`blit_centered` 先例延续）：
  - `blit_area(src, dest, area, tint)`——pygame blit 的 area 语义：
    像素子区域 → 归一 UV 的换算归引擎，且**补齐源面 flush 保险**
    （绑定桥原实现漏了 flush，源面未提交 batch 有内容丢失隐患——
    与 `Surface::blit` 同保险）；
  - `push_points(topo, texture, points, color)`——过程式点集直入
    （z=0、uv 取中心、统一色）；
  - `push_points_indexed(kind, points, indices, color)`——索引点集
    （GLES 兼容退化展开由 push_geometry 承载；indices 空 = 逐 3 顶点
    一切三角）。
- **`pygame::font::preload`/`preloaded`**：宿主预载字体字节缓存
  （按名）——异步 IO（Web fetch / 宿主 io 链）与 Font 同步构造器
  之间的桥，构造时同步命中。

### 四、配套（pygame-rs 侧，独立仓库）

批次四十一（bridge 瘦身七步）：①死代码/诊断遗留清理（rgba/DIAG/
devlog TCP 段 debug 门控）；②`sf_draw_*` 七件全局 + register 退役
（Session pymethod 直调 `pygame::draw`；sig_probe 的 draw 族收参
断言迁入 binding_probe 真会话 selftest：draw.positive/draw.negative）；
③④⑤⑥⑦ = 上文引擎侧四块的绑定面同步 + 收参壳化。**顺带修复**：
`pygame.Surface.fill/blit` 曾被注释（遗留）致 selftest 两项失败——
恢复为 DrawTarget trait 直调形态。验证：starfish 单测 109 过、
pygame-rs sig_probe 134 项 + binding_probe/draw_test/font_test 三
探针全绿、wasm32 双 crate 编译零错、Web 构建链通过。

### 升级提示

- Rust 侧帧循环建议改为 SDL 惯例：
  `while !event::pump() { /* frame */; for ev in event::get() { .. } }`
  ——原先"直接 `event::get()` 即排空窗口事件"的写法现在消费的是
  pump 灌入的队列（不 pump 则恒空）；
- 混音器生命周期建议走 `pygame::mixer`（自行持有 `base::audio::
  AudioMixer` 的写法不受影响，base 层未变）；
- `pygame::render::session` 的 begin/end 与自行持有 `RenderPass` 的
  写法并存；MRT 走 `begin_targets` 可免自管保活。
