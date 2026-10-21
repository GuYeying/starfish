# starfish-we 接入指南（面向 wgpu 渲染层 / 解释器宿主）

> English version: [wgpu-integration-guide.md](wgpu-integration-guide.md)。
>
> 读者：负责对接本库的 agent / 开发者。本文是唯一权威入口，
> 与 CLAUDE.md（仓库工程约定）、《android绑定运行问题与解决方案.md》
> （平台差异事故档案）互为补充。
> 文中所有"已验证"均标注验证等级：**真机实测** / 模拟器实测 / 编译期(check)。

---

## 一、这个库是什么、不是什么

**是**：跨平台**单窗口 + 事件**库，SDL 风格 poll 模型——
调用者拥有主循环，主动 `poll_event()` 拉事件；窗口操作都是普通方法调用。
专为"脚本解释器宿主自己驱动 tick"的场景设计（winit 的控制反转回调与此冲突）。

**不是**：渲染库。本库**一像素都不画**——渲染由你接 wgpu，
窗口实体通过 raw-window-handle / 裸句柄交给 wgpu 建 surface。

两条铁律贯穿全部 API：

1. **单窗口**：进程内同时最多一个 `Window`，第二个 `new` 返回
   `WindowError::AlreadyExists`。drop 后可重建。
2. **物理像素**：一切尺寸/坐标都是物理像素（非 CSS 逻辑像素、非缩放后
   逻辑坐标）——与 wgpu `SurfaceConfiguration.width/height` 直接对齐，无需换算。

---

## 二、接入三步

### 2.1 依赖与 feature

```toml
[dependencies]
starfish-we = { path = "…", features = ["wgpu"] }
```

| feature | 含义 |
|---|---|
| `wgpu` | 实现 raw-window-handle 0.6 trait；**不依赖 wgpu 本体** |
| `wgpu-auto` | 隐含 `wgpu` 并引入 wgpu 本体：`AutoSurface` 接管 resize→configure 循环 |
| `android-demo` | 内置 Android 演示宿主（logcat 事件循环 + 退出契约） |

事件是核心能力，不存在"仅建窗无事件"的构建形态。

### 2.2 最小建窗

```rust
use starfish_we::{Window, WindowError};

let mut window = Window::new("标题", (800, 600))?;          // 快捷方式
// 或全选项 builder（新增选项 = 新增方法，非破坏性）：
let mut window = Window::builder()
    .title("demo")
    .size((800, 600))          // 客户区，物理像素
    .resizable(true)           // 默认 true
    .borderless(false)         // 无边框
    .min_size((400, 300))      // 约束用户拖拽
    .max_size((1920, 1080))
    .always_on_top(false)
    .build()?;
```

### 2.3 主循环骨架（SDL 风格）

```rust
loop {
    while let Some(event) = window.poll_event() {   // 非阻塞；内部泵平台消息源
        match event { /* 见第三节 */ }
    }
    // ← 这里推进你自己的 tick：解释器执行字节码 / 渲染一帧
}
```

**不要**自己碰平台消息循环（Win32 PeekMessage / NSApp / ALooper）——
`poll_event` 内部已经泵了；外部再泵会抢事件。

---

## 三、事件语义全表

```rust
pub enum Event {
    CloseRequested,                 // 用户请求关闭。收到后结束循环并 drop Window
    Resized { width: u32, height: u32 },   // 客户区物理像素变化 → 重配 surface
    FocusGained, FocusLost,
    KeyDown { key: KeyCode, modifiers: Modifiers, repeat: bool },
    KeyUp { key: KeyCode, modifiers: Modifiers },
    TextInput { text: String },     // 最终文本（与 KeyDown 独立；首版无 IME 组合过程）
    MouseMoved { x: f64, y: f64 },  // 客户区左上原点，物理像素
    MouseDown { button: MouseButton }, MouseUp { button: MouseButton },
    MouseWheel { delta_x: f64, delta_y: f64 },  // 正值向上/向右（注意与各平台原生方向）
    Touch { phase: TouchPhase, finger: u64, x: f64, y: f64 },  // 仅触屏设备产生
    RedrawRequested,                // 平台要求重绘（wgpu 用户在此 present）
    Suspended, Resumed,             // 仅移动端/Web：surface 失效/恢复（硬生命周期）
}
```

键值 `KeyCode` 按 W3C `KeyboardEvent.code` 命名（`KeyA`/`Digit1`/`ArrowUp`/
`Numpad0`…约 105 键 + `Other(原生码)`）；文本一律走 `TextInput`。
修饰键 `Modifiers { shift, ctrl, alt, meta }`（meta = Win 徽标 / macOS Command）。

**平台产生矩阵**（不产生的变体在该平台永不出现）：

| 事件 | Windows | macOS | Linux (Wayland) | iOS | Android | Web |
|---|---|---|---|---|---|---|
| CloseRequested | ✅ WM_CLOSE / Alt+F4 | ✅ 关闭按钮 | ✅ toplevel close_request / 连接丢失 | ❌（v0 无关闭概念） | ✅ Destroy / 返回键 | ❌（关页面即结束） |
| Resized | ✅ | ✅ | ✅ configure / 缩放变化 | ✅（仅初始；天然全屏不变） | ✅ | ✅ |
| Focus | ✅ | ✅ | ✅ wl_keyboard enter/leave | ❌（v0 未接） | ✅ | ✅ |
| 键盘 / TextInput | ✅ | ✅（NSEvent characters） | ✅（evdev→KeyCode；xkb 文本；客户端重复合成） | ❌（v0 未接） | ✅（IME 提交文本） | ✅（无 IME 组合） |
| 鼠标 | ✅ | ✅ | ✅（逻辑×scale=物理） | ❌（v0 触摸投递未接） | 合成自主手指 | ✅ |
| 触摸 | ❌ | ❌ | ❌（触屏协议未接） | ❌（v0；投递未接） | ✅ | ✅ |
| RedrawRequested | ✅ WM_PAINT | ❌（帧节奏=宿主 tick） | ✅ 初始 configure 时一条 | ❌（CADisplayLink 驱动 tick 本身） | ✅ RedrawNeeded/InitWindow | ✅ rAF 每帧合并 |
| Suspended / Resumed | ❌ | ✅ 最小化/还原 | ❌ | ❌（v0 生命周期未接） | ✅ Pause·Stop·TerminateWindow / Resume·InitWindow | ✅ 页面隐藏/可见 |
| User（自定义） | ✅ 宿主注入 | ✅ 宿主注入 | ✅ 宿主注入 | ✅ 宿主注入（确定性测试通道） | ✅ 宿主注入 | ✅ 宿主注入 |

---

## 三·五 自定义事件（`Event::User`）——SDL `RegisterEvents` 的类型化替代

宿主（解释器工作线程、任务系统、任何代码）向主循环注入消息的标准路径。
**平台无关**：通道在公共层实现，六平台行为完全一致。

```rust
use starfish_we::{Event, EventProxy, UserEvent};

// ① 取代理（可 Clone，Send + Sync）——交给任意工作线程
let proxy: EventProxy = window.event_proxy();
std::thread::spawn(move || {
    proxy.send(1, "脚本执行完成".to_string()).unwrap();   // code 自定义
    proxy.send(2, Timer { id: 7 }).unwrap();
});

// ② 主循环里接收
while let Some(event) = window.poll_event() {
    match event {
        Event::User(user) => match user.code {
            1 => { let msg = user.downcast_ref::<String>().unwrap(); }
            2 => { let timer: std::sync::Arc<Timer> = user.downcast().unwrap(); }
            _ => {}
        },
        _ => {}
    }
}

// ③ 同线程便捷注入（等价于 proxy.send，忽略失效错误）
window.push_event(3, 12345u64);
```

语义与注意：

- `UserEvent { code: u32, payload }`：`code` 是宿主自定义类别码；
  载荷为 `Any + Send + Sync` 的 **Arc 共享**（`Event: Clone` = 共享载荷，
  广播语义；`downcast_mut` / `downcast` 取唯一所有权需无其他共享者）。
- **时序保证**：`send` 之后下一次 `poll_event` 必定可见；
  `Event::User` 先于本轮平台事件派发，批量注入按序逐条（平台事件短暂
  延迟、不丢失）。
- `EventProxyClosed`：`Window` 销毁后 `send` 显式报错（SDL 没有这个保护）。
- 平台事件循环不感知用户事件：**不会**被平台层吞掉或改序；
  Android 的 `WINDOW_READY` 门等生命周期过滤同样不作用于它。
- `Event` 因携带动态载荷**不实现 `PartialEq`**，匹配用 `match`/`matches!`。

---

## 四、wgpu 对接契约（渲染循环模板）

```rust
use starfish_we::{Event, Window};

let mut window = Window::builder().title("app").size((1280, 720)).build()?;
let instance = wgpu::Instance::default();
let mut surface = Some(instance.create_surface(&window)?);   // 零 unsafe
let mut size = window.size();
// configure(&device, &SurfaceConfiguration { width: size.0, height: size.1, … })

loop {
    while let Some(event) = window.poll_event() {
        match event {
            Event::Resized { width, height } => {
                size = (width, height);
                // 用新尺寸重新 surface.configure(...)（先取 get_current_texture
                // 返回 Suboptimal/Outdated 也要重配——见 wgpu 文档）
            }
            Event::Suspended => { surface = None; }   // Android：surface 已被系统回收，
            Event::Resumed => { /* 用 current size 重建并 configure */ }
            Event::CloseRequested => { return Ok(()); }
            _ => {}
        }
    }
    if let Some(surface) = &surface {
        // get_current_texture → render → present（RedrawRequested 已含在 tick 节奏里）
    }
    // 解释器 tick 在此推进
}
```

可执行参考：`tests/wgpu_integration.rs`（Windows 真机验证：
create_surface → configure → 取帧 → 重配置再取帧，全程零 unsafe）。

各平台取窗口实体的差异：

| 平台 | surface 来源 | 状态 |
|---|---|---|
| Windows | `create_surface(&window)`（HWND+HINSTANCE 自动携带） | **真机实测** ✅ |
| macOS | 同左（NSWindow 的 view + layer 句柄自动携带） | trait 已实现，wgpu 链路待实测 |
| iOS | 同左（UIView + layer 句柄） | trait 已实现，待实测 |
| Android | 同左（ANativeWindow；仅 `Resumed` 期间有效） | trait 已实现，设备端待验 |
| Web | `window.canvas()` → 交给 wgpu 的 Web 后端（需浏览器 WebGPU） | 访问器就绪，待跑通 |
| Linux | 同左（`wl_display` + `wl_surface` 裸指针；要求 wayland-client 的 `system` 后端） | trait 已实现，wgpu 链路待实测 |

---

## 五、各平台驱动原理（agent 必须理解的部分）

### 5.1 Windows（Win32，`windows-sys` FFI）

- **机制**：OS 消息队列原生支持 poll。`poll_event` 用 `PeekMessageW(PM_REMOVE)`
  非阻塞把本线程消息泵干 → `DispatchMessageW` → `wndproc` 翻译进队列 → 取队头。
- 翻译层在 `platform/windows/translate.rs`，仅在 `event` feature 下编译；
  返回 `None` 的消息回落 `DefWindowProcW`（Alt+F4、菜单键等保持系统默认）。
- 进程声明 Per-Monitor V2 DPI 感知 → 坐标恒为物理像素。
- `WM_CLOSE` 只上报 `CloseRequested`，**不销毁窗口**；销毁由 drop `Window` 触发
  （`DestroyWindow`）。收到 CloseRequested 后继续 poll 是安全的。
- 鼠标拖拽出窗口：按下时 `SetCapture`，全部键抬起 `ReleaseCapture`。

### 5.2 Android（NativeActivity，`android-activity 0.6.1`）

- **机制反转**：入口/生命周期由系统驱动。宿主入口两种接法（等价）：
  ```rust
  // 方式一：宏一行接入（推荐）
  starfish_we::android_main!(my_app_main);   // 先 android_init 再交控制权
  fn my_app_main(app: starfish_we::AndroidApp) { /* 主循环 */ }

  // 方式二：手写入口（既有项目形态）
  #[unsafe(no_mangle)]
  fn android_main(app: AndroidApp) { starfish_we::android_init(app); /* … */ }
  ```
  此后本库把 `AndroidApp::poll_events(timeout=0)` + `input_events_iter()`
  的回调流**翻译进缓冲队列**，`poll_event` 排空——对宿主仍是纯 poll。
  宿主 crate 必须是 cdylib；用了宏就不要再手写 `android_main`（符号冲突）。
- **线程**：所有调用必须在 `android_main` 线程（android-activity 断言）。
- **生命周期映射**：`InitWindow→Resized+Resumed+Redraw`、`TerminateWindow→Suspended`、
  `Pause/Stop→Suspended`、`Destroy→CloseRequested`、`Back键→CloseRequested`。
- **进程复用（三个已修复的坑，勿回退）**：
  1. 用户"返回"只销毁 Activity，**进程被缓存**；再次启动时 `android_main`
     在同进程新线程二次执行 → `android_init` 必须覆盖式更新 app
     （曾用 OnceLock 导致二次启动闪退）并重置事件缓冲/就绪标记/单窗口占位。
  2. 旧实例迟到的 `Destroy` 会串扰进新实例事件流 → 曾致"二次启动秒退"
     → 现按 `WINDOW_READY` 门丢弃（未见 InitWindow 前的 Destroy 不可信）。
  3. **胶水 static 不支持同进程"销毁中途重建"**（上游 0.6.1 无修复）：
     新旧实例交叠时其 UI 线程回调会 abort。
     **宿主退出契约：收到 `CloseRequested`（或 `Suspended`）后立即终结进程**
     （demo 用 `libc::raise(SIGKILL)`；`exit(0)` 会留 RenderThread 的
     SIGABRT 退出噪音）。解释器宿主（引擎=进程）语义完全等价。
- **返回键陷阱**：KeyDown 可能被系统拦截不派发（只见 `Suspended`），
  退出判定不能只押按键事件。
- **TextInput**：来自系统 IME 提交（`InputEvent::TextEvent`）。
- 打包：内置演示宿主 = `android-demo` feature（`src/platform/android/entry.rs`）。
  构建需 `ANDROID_NDK_ROOT`：`cargo apk build --features android-demo`
  （根包；APK 元数据在根 Cargo.toml）。

### 5.3 Web（wasm32-unknown-unknown，`wasm-bindgen` + `web-sys`）

- **机制反转**：DOM 是回调驱动。事件监听器把事件**缓冲进队列**，
  `poll_event` 排空；主循环节奏由宿主驱动（解释器 tick 配合
  `setTimeout`/`requestAnimationFrame`）。
- **窗口实体 = `<canvas>`**：按 id `starfish-we` 查找，其次取页面
  第一个 canvas，都没有则自建并铺满。渲染对接用 `window.canvas()`
  拿 `HtmlCanvasElement` 交给 wgpu 的 Web 后端（需浏览器 WebGPU，
  Chrome/Edge 113+）。
- 键盘走 W3C `KeyboardEvent.code`，与 `KeyCode` 命名同源、直接映射。
- 坐标 = CSS 坐标 × `devicePixelRatio`（物理像素约定一致）；
  resize 时库会把 canvas.width/height 属性同步为物理尺寸。
- `requestAnimationFrame` 循环只置重绘标志（合并），`poll_event`
  每帧至多吐一条 `RedrawRequested`——把它当帧节拍用。
- **没有 `CloseRequested`**：页面关闭即结束。切换标签页 →
  `Suspended`/`Resumed`。
- 已知边界：IME 组合过程不上报（只收最终字符）；代理对字符首版丢弃；
  wheel 监听未 `preventDefault`（页面本身无滚动即可）。

### 5.4 macOS（AppKit，`objc2` 家族）

- **机制**：`poll_event` 用 `nextEventMatchingMask` 非阻塞泵干可用
  NSEvent——输入事件（键/文本/鼠标）直接翻译进缓冲；窗口级事件
  `sendEvent` 派发给 AppKit 以触发窗口委托（关闭→`CloseRequested`、
  resize→`Resized`、焦点、**最小化→`Suspended` / 还原→`Resumed`**），
  然后取队头。AppKit 仅限主线程（存储 `MainThreadMarker` 强制）。
- 键码 = `NSEvent keyCode` 携带的 Carbon 虚拟键码，`translate.rs`
  全表映射到 `KeyCode`；`TextInput` 取 NSEvent characters（无 IME
  组合过程，与 Windows 首版约定一致）。
- **没有 `RedrawRequested`**：macOS 后端未接线；帧节奏就是宿主
  tick（解释器驱动），与 AppKit 的 drawRect 机制解耦。
- 创建时激活应用（否则后台状态下窗口拿不到键盘焦点）。
- 验证：CI 在 GitHub 的真实 Mac runner 上跑 WindowServer 探针
  （真实 NSApplication+NSWindow 建窗 + 注入事件往返）+ 全测试套件；
  **wgpu surface 链路未实测**（`HasWindowHandle` 是平台无关实现，
  理论直通）。

### 5.5 iOS（UIKit，`objc2` 家族）

- **入口（语句式宏）**：在 `fn main` 体内一行接入：
  ```rust
  fn main() {
      starfish_we::ios_main!(my_tick);   // 把主线程交给 UIApplicationMain，永不返回
  }
  fn my_tick(window: &starfish_we::Window) -> Option<i32> {
      // 每帧经 CADisplayLink 在主线程调用：poll 事件 + 渲染
      None // Some(code) 以该状态码退出应用
  }
  ```
  与 `android_main!` 的差异是刻意的：Android glue 的 ABI 要求顶层
  定义 `android_main` 函数；iOS 无此约定，**语句才是正确形状**
  （宏若展开成 `fn main` 定义，放进函数体会静默变成永不执行的
  嵌套函数——已踩过：类型检查全绿、运行时零声响）。
- **机制反转**（与 Android 同装置）：委托 `didFinishLaunching` 建窗
  （全屏 UIWindow + rootViewController）并安装 CADisplayLink；每帧
  驱动宿主 tick；`poll_event` 排空缓冲队列 + 零超时 runloop 泵。
  `Window::new` 在 iOS 上由入口内部调用，宿主不要自行调用。
- **装配坑（已编码修复，勿回退）**：① objc2 的类是惰性注册的，且
  默认运行时名含模块路径+版本——`UIApplicationMain` 按**名字**查
  委托类，必须先 `ClassType::class()` 强制注册 + `#[name]` 固定名；
  ② 启动结束时 key window 必须有 `rootViewController`，否则 UIKit
  抛 `NSInternalInconsistencyException`。
- 坐标 = bounds × scale（物理像素约定一致）；窗口天然全屏、无边框、
  不可调（平台固有属性，能力方法为固定值 no-op）。
- **v0 边界**：事件仅注入通道（`ios_inject_test_event`，确定性
  测试用）；真实触摸投递（需 responder 子类）、生命周期
  `Suspended`/`Resumed`、键盘/TextInput 均未接。
- 验证：CI 在 iPhone 模拟器内跑冒烟（`simctl install+launch`），
  结果文件协议 `STARTED→FRAME0→OK/FAIL/PANIC`（panic 钩子覆盖
  UIApplicationMain 引导期；ObjC 异常靠 `--stderr=` 抓取）判定
  注入事件往返通过；真机待验。

### 5.6 Linux（**仅 Wayland**，`smithay-client-toolkit` 0.21 + `wayland-client` 0.31）

- **X11 已按设计决策放弃**：不支援 X11 会话（XWayland 下跑的也是本后端
  ——它是原生 Wayland 客户端）。依赖 `system`+`dlopen` 后端：为 wgpu 提供
  真实 `wl_display*`/`wl_surface*` 指针（纯 Rust 后端没有 display 指针），
  运行期加载 libwayland 免链接期依赖。构建需 `pkg-config` + `libxkbcommon-dev`
  （xkbcommon 是 C 链接依赖，**Windows 主机交叉检查 Linux target 因此失效**
  ——Linux 编译验证由 CI 的 linux.yml 承担）。
- **sctk 0.21 装配**：0.21 把各模块 `delegate_*!` 宏合并为单一
  `delegate_dispatch2!`——装配 = `delegate_registry!` +
  `impl ProvidesRegistryState`（`registry_handlers![OutputState, SeatState]`）
  + `delegate_dispatch2!` 三件套。
- **建窗即"空提交"**：xdg toplevel 以一次**不带缓冲的 commit** 映射，合成器
  回首个 configure 后才画 shm 底色（SlotPool，双缓冲兜底）并提交——此后窗口
  可见、`size()` 已知。init 里的 `roundtrip` 是全库唯一的阻塞点（与 Windows
  建窗泵消息同级）。
- **非阻塞六步泵**：`flush` → `dispatch_pending` → `prepare_read`+`read`
  （WouldBlock=空闲，wayland-client 内部已消化为 Ok(0)）→ `dispatch_pending`
  → 客户端按键重复合成 → pop。**连接丢失/协议错误 = 恰好一条
  `CloseRequested`**（宿主干净退出，Android 退出契约同一哲学）。
- **键盘**：sctk 给的 `raw_code` 是**裸 evdev 码**（+8 的 xkb 偏移只在它内部
  查 keysym 用），`translate.rs` 全表映射 W3C `KeyCode`；`TextInput` 为 xkb
  合成 UTF-8（含 compose）；合成器不重复按键 → 按 `RepeatInfo` 客户端自合成
  （修饰键不重复）。
- **坐标**：configure 尺寸是逻辑像素，物理 = 逻辑 × 整数 scale（分数缩放 v1
  忽略）；滚轮 value120 优先 discrete 兜底，垂直轴取负（wl 正=向下，本库
  正=向上）。
- **能力近似表**（文档化取舍）：title/fullscreen/maximize/minimize 真实现；
  resizable = min=max 尺寸惯用法（xdg 无 resizable 请求）；无边框 =
  xdg-decoration 请求 + sctk-adwaita CSD（Mutter 无 xdg-decoration 时自动
  以 Adwaita 标题栏/关闭按钮/拖拽/边缘缩放兜底，KWin 的 Server 装饰则让位）；position/opacity/always-on-top/鼠标抓取/相对
  鼠标/隐藏光标 = no-op（各自协议属后续项）；`set_size` 平铺合成器下仅咨询性。
- **验证**：CI 在 headless Weston 下跑 `wayland_smoke`（连接→configure→shm
  提交→事件排空→单窗口守卫→drop 重建）；交互输入（打字/滚轮/关闭）已
  在真机 Wayland 会话（VMware Ubuntu VM）验收通过（2026-10-11）。

---

## 六、红线清单（违反即出错）

1. 所有 `Window` 方法必须在**创建线程**调用（Win32 线程亲和 /
   AppKit 主线程 / android_main 线程）；`Sync` 仅为满足 wgpu
   `create_surface` 的 bound，跨线程调用方法属违反契约。
2. 进程内最多一个 `Window`；Android 上占位按"运行"计算
   （`android_init` 已自动重置）。
3. 尺寸/坐标一律物理像素，直接喂 `SurfaceConfiguration`。
4. Android：`Resumed` 之外不要使用 surface（`raw_handle` 会报不可用）；
   渲染循环必须处理 `Suspended`（销毁 surface）/`Resumed`（重建）。
5. Android：退出用进程终结（见 5.2 契约），**不要**指望从
   `android_main` return 后进程还能复用。
6. 不要自行泵平台消息循环。
7. 事件枚举是 `#[non_exhaustive]`：match 请留 `_` 臂。
8. Linux：所有方法限创建线程；合成器连接丢失/协议错误会以**恰好一条
   `CloseRequested`** 呈现——收到后退出循环即可，不要再依赖后续事件。

---

## 七、诊断速查

| 平台 | 看什么 | 命令 |
|---|---|---|
| Windows | 窗口+事件演示 | `cargo run --example basic` |
| Windows | wgpu 集成回归 | `cargo test --features wgpu --test wgpu_integration` |
| Windows | 全套冒烟 | `cargo test` |
| macOS | CI 冒烟（真实 Mac runner：探针+全套件） | push 后 `gh run watch`（`.github/workflows/macos.yml` 的 `macos` job） |
| iOS | CI 模拟器冒烟（建窗+注入往返） | 同上的 `ios-sim` job；失败看 result.txt 终态 + `sim_stderr.txt` artifact |
| Linux | CI 编译矩阵 + headless Weston 下的真实 Wayland 冒烟 | push 后 `gh run watch`（`.github/workflows/linux.yml` 的 `linux` job） |
| Linux | 本机真实验收（Wayland 会话） | `cargo run --example basic`；卡住用 `WAYLAND_DEBUG=1` 全线协议追踪 |
| Android | 事件/生命周期日志 | `adb logcat -s starfish`；panic 另加 `RustStdoutStderr` |
| Android | 打包安装 | `export ANDROID_NDK_ROOT=<ndk> && cargo apk build --features android-demo && adb install -r target/debug/apk/starfish-demo.apk` |
| Web | 页面日志 + console | `python -m http.server 8000 -d testweb` → http://localhost:8000 （`scripts/build_web.sh` 重打包） |
| 全平台编译矩阵 | `cargo check --target <t> --features wgpu`；**例外**：Linux target 因 xkbcommon（C 链接依赖）无法在 Windows 主机交叉检查，其编译验证由 linux.yml 承担 | |

故障定性方法论（来自《android绑定运行问题与解决方案.md》，通用）：
先复现并收割崩溃信息（logcat/tombstone），崩点 ≠ 根因；平台编译矩阵
全绿 ≠ 平台能跑（TLS 键、进程复用这类纯运行期差异 check 查不出）；
系统异步给的东西（surface、canvas、adapter）一律"等平台就绪信号再拿"。

---

## 八、当前完成度（2026-10-11，对接前必读）

| 平台 | 实现程度 | 验证等级 |
|---|---|---|
| Windows | ~95%（缺：IME 组合、DPI 逻辑像素、全屏/图标/位置、运行时 resizable） | 真机实测 + 自动化测试 |
| macOS | ~90%（缺：RedrawRequested 接线、IME 组合、全屏/图标/位置、运行时 resizable） | CI 真实 WindowServer 探针 + 全测试套件 ✅；wgpu 链路待实测 |
| iOS | ~60%（v0：建窗+帧驱动+注入通道；缺：真实触摸、生命周期、键盘/TextInput、真机验证） | CI 模拟器冒烟 ✅（注入事件往返）；真机待验 |
| Android | ~90%（缺：GameActivity、手势导航、软键盘控制 API、saveInstanceState） | 模拟器实测（四轮快打 0 panic）；真机待复测 |
| Web | ~85%（缺：IME、rwh Web 注册表、wheel preventDefault） | 事件层已经人肉点验 ✅；wgpu-WebGPU 链路未跑通 |
| Linux | ~85%（Wayland-only；缺：触摸、指针约束/相对鼠标、分数缩放、IME preedit、xdg-activation） | 真机 Wayland 会话实测 ✅（交互输入/关闭/CSD 装饰全链路，2026-10-11 VMware Ubuntu）+ CI headless Weston 冒烟 ✅ |

版本事实：raw-window-handle **0.6**（`#[non_exhaustive]`，用 `new()` 构造器；
`WindowsDisplayHandle`/`AndroidDisplayHandle` 是空结构；Android 变体名是
`AndroidNdk`）；wgpu 兼容版本 **30**（`get_current_texture` 返回枚举
`CurrentSurfaceTexture`，非 Result；`SurfaceConfiguration` 必填 `color_space`）；
wasm-bindgen 钉在 **0.2.127**（须与 wasm-bindgen-cli 版本精确一致）；
Linux = sctk **0.21.1**（`delegate_dispatch2!` 单宏装配）+ wayland-client
**0.31**（`system`+`dlopen`，wgpu 互操作必需）+ xkbcommon **0.8**（C 链接
依赖：构建期 pkg-config 探测，Windows 主机不能交叉检查 Linux target）。
