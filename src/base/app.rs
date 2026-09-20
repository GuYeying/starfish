//! 应用循环模型：引擎持循环，回调给应用（"B 门"）
//!
//! 这是 base 的**唯一**入口形态，跨平台统一：
//!
//! - 桌面：winit 事件循环驱动（`ControlFlow::Poll` + `request_redraw`），
//!   帧率经 [`WindowConfig::with_fps_cap`] 节流（或交给 vsync 回压）
//! - Web（Step 4 落地）：浏览器 rAF 驱动同一 trait，应用代码零改动
//!
//! 选型记录（2026-09-08，见 doc/log）：放弃"外部泵（Python 持 while）"双门设计，
//! 统一走回调门——未来 PyO3 层以**生成器门面**（每帧一个 `yield`）把本模型
//! 包装回 pygame 风格，同一份 Python 脚本桌面/Web 通用（pygbag 同款思路）。
//!
//! # 示例
//!
//! ```ignore
//! use starfish::base::app::{run, Application, Ctx, WindowConfig};
//! use starfish::base::window::{KeyCode, WindowEvent};
//!
//! struct Game { /* 持有 RenderContext / RenderSurface 等 */ }
//!
//! impl Application for Game {
//!     fn start(&mut self, ctx: &mut Ctx) {
//!         let (_c, _r, surface) = RenderEntry::new(ctx.window(), None, None).unwrap();
//!         /* ... */
//!     }
//!     fn event(&mut self, e: &WindowEvent, ctx: &mut Ctx) {
//!         if let WindowEvent::KeyPressed(KeyCode::Escape) = e { ctx.exit(); }
//!     }
//!     fn frame(&mut self, ctx: &mut Ctx) {
//!         let dt = ctx.delta();
//!         /* 更新 + 渲染提交 + present */
//!     }
//! }
//!
//! run(Game::default(), WindowConfig::new("demo", 800, 600).with_fps_cap(120));
//! ```

use std::cell::{Ref, RefCell, RefMut};
use std::future::Future;
use std::rc::Rc;

use crate::base::time::Clock;
use crate::base::window::event::{
    map_key, KeyModifiers, KeyboardState, MouseButton, MouseState, WindowEvent,
};
use crate::base::window::Window;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent as WinitEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window as WinitWindow, WindowId};
#[cfg(target_os = "android")]
use winit::platform::android::EventLoopBuilderExtAndroid;
/// AndroidApp 再导出：`app_entry!` 宏的 android 分支经
/// `$crate::base::app::AndroidApp` 取类型——用户 crate 无需依赖 winit
/// （宏展开路径在用户 crate 解析，库内再导出是对外的唯一稳定锚点）。
#[cfg(target_os = "android")]
pub use winit::platform::android::activity::AndroidApp;

// ── Android OS 句柄捕获槽（app_entry! ↔ run 的桥）──────────────────
// `android_main` 把系统递入的 AndroidApp 存进来，随后 `run` 从这里取——
// 应用侧因此只有一个 `run`，与桌面/Web 完全对称（AndroidApp: Send + Sync，
// 上游 android-activity 明确保证）。
#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<AndroidApp> = std::sync::OnceLock::new();

/// 捕获 OS 递入的 AndroidApp（幂等：二次存入被忽略）。
/// 仅 `app_entry!` 生成的 android_main 调用。
#[cfg(target_os = "android")]
pub fn set_android_app(app: AndroidApp) {
    let _ = ANDROID_APP.set(app);
}

/// 取出捕获的 AndroidApp（未捕获 = run 被绕过 android_main 直接调用，可诊断）
#[cfg(target_os = "android")]
fn take_android_app() -> Option<AndroidApp> {
    ANDROID_APP.get().cloned()
}

/// 窗口/循环配置（[`run`] 入参）
#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub title: String,
    /// 初始客户区尺寸（逻辑像素）
    pub width: u32,
    pub height: u32,
    pub resizable: bool,
    pub maximized: bool,
    /// 帧率上限（0 = 不限，交给 vsync 回压； Fifo 呈现下勿与刷新率同档，见 Clock::tick 文档）
    pub fps_cap: u32,
    /// Web（wasm32-unknown-unknown）专属：接管页面中指定 id 的 `<canvas>` 元素
    /// 作为渲染画布（winit 默认自建 canvas 且不入 DOM）。桌面忽略此字段。
    pub web_canvas_id: Option<String>,
}

impl WindowConfig {
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width,
            height,
            resizable: true,
            maximized: false,
            fps_cap: 0,
            web_canvas_id: None,
        }
    }

    /// Web：接管页面中 `<canvas id="...">` 元素（桌面无效果）
    pub fn with_web_canvas_id(mut self, id: impl Into<String>) -> Self {
        self.web_canvas_id = Some(id.into());
        self
    }

    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn with_maximized(mut self, maximized: bool) -> Self {
        self.maximized = maximized;
        self
    }

    pub fn with_fps_cap(mut self, fps_cap: u32) -> Self {
        self.fps_cap = fps_cap;
        self
    }
}

/// 引擎上下文：事件回调与帧回调的应用侧视图
///
/// 键鼠状态表由引擎从事件流维护——轮询式输入（`ctx.keyboard().is_pressed(..)`）
/// 与事件式输入并存，对齐 pygame 的 `get_pressed` / `event.get` 双轨。
///
/// **单窗口模型**：引擎持有一个主窗口，`ctx.window()` 即它。
pub struct Ctx {
    /// 主窗口（引擎持有，唯一）
    window: Window,
    keyboard: KeyboardState,
    mouse: MouseState,
    /// 手柄状态表（feature = "gamepad"；每帧帧前刷新，见 [`Self::gamepad`]）
    #[cfg(feature = "gamepad")]
    gamepad: crate::base::gamepad::GamepadState,
    delta: f32,
    exit: bool,
}

impl Ctx {
    /// 主窗口（单窗口模型的唯一窗口）
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// 键盘状态表（v1：全局，取最后聚焦窗口）
    pub fn keyboard(&self) -> &KeyboardState {
        &self.keyboard
    }

    /// 鼠标状态表（v1：全局，取最后聚焦窗口）
    pub fn mouse(&self) -> &MouseState {
        &self.mouse
    }

    /// 手柄状态表（feature = "gamepad"）
    ///
    /// 引擎每帧帧前排水设备事件刷新快照；应用轮询读取
    /// （`is_pressed`/`just_pressed`/`axis`，对齐键鼠的轮询+差量双轨）。
    #[cfg(feature = "gamepad")]
    pub fn gamepad(&self) -> &crate::base::gamepad::GamepadState {
        &self.gamepad
    }

    /// 主窗口客户区尺寸（物理像素）
    pub fn size(&self) -> (u32, u32) {
        self.window.size()
    }

    /// 本帧真实间隔（秒，未缩放）——由引擎时钟在每帧前更新
    pub fn delta(&self) -> f32 {
        self.delta
    }

    /// 请求退出应用（关闭全部窗口，当前帧后生效）
    pub fn exit(&mut self) {
        self.exit = true;
    }

    /// 是否已请求退出
    pub fn exit_requested(&self) -> bool {
        self.exit
    }
}

/// 应用 trait：引擎回调的时机表（单窗口模型）
pub trait Application {
    /// 主窗口获得**首个有效尺寸**后调用一次：建渲染表面、加载资源。
    /// Web 上此时 ctx.size() 已可信（早于 GPU 初始化，尺寸竞态从顺序上消除）。
    /// 注意：最早的若干事件（含携带真实尺寸的 Resized）可能先于 start 到达，
    /// 句柄未就绪时跳过即可。
    fn start(&mut self, _ctx: &mut Ctx) {}

    /// 平台事件（每帧前按到达顺序逐个派发；最早的事件可能先于 start）
    /// `win` = 主窗口
    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    /// 帧回调：更新 + 渲染提交 + present 在这里
    fn frame(&mut self, ctx: &mut Ctx);
}


/// 跨平台异步资源槽位：把"Web 异步初始化 / 桌面同步初始化"的平台差异
/// 封进 [`init`](Self::init)，应用代码**零 cfg**。
///
/// Web 上 GPU 资源必须异步构建（无阻塞模型），桌面是同步的——本类型以
/// "槽位 + 未就绪即跳过"的模式让两侧共享同一套应用结构：
///
/// ```ignore
/// struct App { gpu: InitSlot<Gpu> }
///
/// fn start(&mut self, ctx: &mut Ctx) {
///     let slot = self.gpu.clone();
///     let window = ctx.window().clone();
///     slot.init(async move { build_gpu(&window).await });
///     // 桌面：阻塞跑完立即填入；Web：排队到浏览器任务队列，就绪后填入
/// }
///
/// fn frame(&mut self, ctx: &mut Ctx) {
///     let Some(gpu) = self.gpu.get_mut() else { return }; // 未就绪：本帧跳过（正常状态）
///     /* 渲染——两平台完全相同 */
/// }
/// ```
///
/// 内部为 `Rc<RefCell>`（非 Send）：Application 本就运行在主线程
///（见线程契约，doc/reference），与 free-threaded 契约不冲突。
pub struct InitSlot<T> {
    slot: Rc<RefCell<Option<T>>>,
}

impl<T> InitSlot<T> {
    pub fn new() -> Self {
        Self { slot: Rc::new(RefCell::new(None)) }
    }

    /// 就绪资源的只读视图（未就绪返回 None）
    pub fn get(&self) -> Option<Ref<'_, T>> {
        let b = self.slot.borrow();
        b.is_some().then(|| Ref::map(b, |o| o.as_ref().unwrap()))
    }

    /// 就绪资源的可变视图（未就绪返回 None）
    pub fn get_mut(&self) -> Option<RefMut<'_, T>> {
        let b = self.slot.borrow_mut();
        b.is_some().then(|| RefMut::map(b, |o| o.as_mut().unwrap()))
    }
}

impl<T> Default for InitSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for InitSlot<T> {
    fn clone(&self) -> Self {
        Self { slot: self.slot.clone() }
    }
}

impl<T: 'static> InitSlot<T> {
    /// 执行异步初始化并填入槽位。
    ///
    /// 桌面：阻塞至完成（与同步初始化等价）；Web：排队到浏览器任务队列，
    /// 完成后自动填入——期间 [`frame`](Application::frame) 对未就绪静默跳过。
    pub fn init<F>(&self, fut: F)
    where
        F: Future<Output = T> + 'static,
    {
        #[cfg(target_arch = "wasm32")]
        {
            let slot = self.slot.clone();
            wasm_bindgen_futures::spawn_local(async move {
                *slot.borrow_mut() = Some(fut.await);
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            *self.slot.borrow_mut() = Some(pollster::block_on(fut));
        }
    }
}

/// 运行应用直至 [`Ctx::exit`] 或窗口关闭。**必须在主线程调用。**
///
/// Web（wasm32-unknown-unknown）：winit 由浏览器 rAF 驱动，事件循环不能阻塞
/// 浏览器主线程——本函数经 `spawn_local` 进入事件循环后**立即返回**（返回类型
/// 为 `()` 与桌面不同）；桌面：阻塞驱动至循环结束（`-> !`）。
#[cfg(target_arch = "wasm32")]
pub fn run(app: impl Application + 'static, config: WindowConfig) {
    wasm_bindgen_futures::spawn_local(async move {
        let event_loop =
            EventLoop::new().expect("winit EventLoop 创建失败（run 必须在主线程调用）");
        inner_run(event_loop, app, config);
    });
}

/// 运行应用直至 [`Ctx::exit`] 或窗口关闭。**必须在主线程调用。**
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn run(app: impl Application, config: WindowConfig) -> ! {
    let event_loop = EventLoop::new().expect("winit EventLoop 创建失败（run 必须在主线程调用）");
    inner_run(event_loop, app, config)
}

/// Android 上的 [`run`]：从模块内捕获槽取 OS 句柄（`app_entry!` 生成的
/// `android_main` 已捕获），完成 Android 专属引导后进入与桌面/Web 完全
/// 相同的 [`inner_run`]——**三个平台的 run 从此同名**。
#[cfg(target_os = "android")]
pub fn run(app: impl Application, config: WindowConfig) -> ! {
    let android_app = take_android_app().expect(
        "run() 未捕获 AndroidApp——Android 上必须经 android_main 进入（app_entry! 已自动处理）",
    );
    // 统一收编各示例 android_main 的样板：backtrace 打开 + 应用私有目录注入
    // （唯一事实源 = io 的 base_dir；需要绝对路径的场景经 `io::base_dir()` 读）。
    // 注入在 EventLoop 创建之前 ⇒ start()/frame() 必然晚于注入，资产落盘与
    // io 相对路径从此无需应用侧任何 cfg（旧式 android_main 手工
    // `io::set_base_dir` 的写法被此处吸收，同值重复注入幂等无害）。
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    if let Some(dir) = android_app.internal_data_path() {
        #[cfg(feature = "io")]
        crate::base::io::set_base_dir(&dir);
    }
    // 把 ndk-context 里的 Application 换成**真正的 NativeActivity**：
    // android-activity 初始化时存的是 Application（init.rs get_application），
    // 而 requestPermissions / getFragmentManager 等 Activity 独有方法在
    // Application 上不存在——dialog（robius show）与录音权限申请曾因此
    // 全线失败（实机报 "no non-static method ... on android/app/Application"，
    // 2026-09-18 探针定位）。Activity 本身也是 Context，既有消费方（video 的
    // MediaCodec 链路）不受影响。
    // SAFETY：android-activity 已初始化 ndk-context（持有 Application 全局引用）；
    // 此处 release 后立刻以同一 VM + NativeActivity 全局引用重初始化，两步之间
    // 无并发读取（同一线程、启动期）。指针有效性由 android-activity 的全局引用保证。
    unsafe {
        ndk_context::release_android_context();
        ndk_context::initialize_android_context(
            android_app.vm_as_ptr().cast(),
            android_app.activity_as_ptr().cast(),
        );
    }
    let event_loop = match winit::event_loop::EventLoop::builder()
        .with_android_app(android_app)
        .build()
    {
        Ok(l) => l,
        // 同缓存进程的二次实例（winit 每进程一次 EventLoop 的硬限制，
        // EVENT_LOOP_CREATED 静态量 Android 上永不复位）：不再 panic 闪退，
        // 而是干净退出本进程——僵尸进程随之清除，用户下一次点击即正常进入。
        Err(e) => {
            eprintln!("[starfish] 二次实例进入（{e:?}），退出残留进程");
            std::process::exit(0);
        }
    };
    inner_run(event_loop, app, config)
}

/// 三平台共用的循环驱动尾：节流策略 → Adapter 装配 → 驱动至退出 → 收进程。
/// 平台专属只剩两处 cfg：节流（Web=Wait / 其余=Poll）与退出语义。
///
/// EventLoop 由各入口构建好传入（Android 经 android-activity 引导、桌面/Web
/// 经 `EventLoop::new`），本函数不再关心"循环怎么来"。
fn inner_run<A: Application>(
    event_loop: winit::event_loop::EventLoop<()>,
    app: A,
    config: WindowConfig,
) -> ! {
    // 钉主线程锚点：此后所有主线程 API 的调试断言以此为基准
    crate::base::debug::mark_main_thread();
    // Web：必须用 Wait——Poll 的调度策略是 Scheduler.yield/setTimeout
    //（"as fast as possible"，非 vsync），会让帧循环以 CPU 全速空转卡死页面；
    // Wait 下帧节奏由 request_redraw → canvas rAF 驱动（每 vsync 一帧）。
    // 桌面/安卓：Poll（事件到达即处理 + about_to_wait 连续帧，无此调度问题）。
    #[cfg(target_arch = "wasm32")]
    event_loop.set_control_flow(ControlFlow::Wait);
    #[cfg(not(target_arch = "wasm32"))]
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut adapter = Adapter {
        app,
        config,
        ctx: None,
        clock: Clock::new(),
        event_buf: Vec::with_capacity(16),
        started: false,
        start_wait: 0,
    };
    let result = event_loop.run_app(&mut adapter);
    // 先跑完应用与窗口（含音频流等）的析构，再收进程——process::exit 跳过 Drop
    drop(adapter);
    // 退出语义三平台收敛于此：
    // - Web：winit web 的 run_app 永不返回（以控制流异常退回浏览器），
    //   process::exit 会 trap 整个页面实例，故不参与
    // - 桌面：Err 记日志收 1，正常收 0
    // - Android：循环结束 = Activity 结束。系统此时只 finish Activity、
    //   保留缓存进程，而 winit 每进程仅允许创建一次 EventLoop——用户从
    //   桌面图标再进时，同缓存进程的二次 android_main 必然 RecreationAttempt
    //   panic（实测：返回退出后再点图标闪退，后台划掉才正常）。因此显式
    //   收掉整个进程，对齐桌面"run 返回即进程结束"语义（游戏退出即杀进程
    //   在 Android 生态是常规做法；drop 已先行清理音频流等）。
    #[cfg(target_arch = "wasm32")]
    {
        let _ = result;
        unreachable!("winit web 事件循环不应返回（以控制流异常退回浏览器）");
    }
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
    {
        if let Err(e) = result {
            eprintln!("[starfish] 事件循环异常退出: {e}");
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    #[cfg(target_os = "android")]
    {
        let _ = result;
        std::process::exit(0);
    }
}

struct Adapter<A: Application> {
    app: A,
    config: WindowConfig,
    ctx: Option<Ctx>,
    clock: Clock,
    /// 事件翻译缓冲（跨帧复用，避免每事件分配）
    event_buf: Vec<WindowEvent>,
    /// app.start 是否已执行（启动门：等首个有效尺寸）
    started: bool,
    /// 启动门等待计数（尺寸长期为 0 的兜底）
    start_wait: u32,
}

impl<A: Application> ApplicationHandler for Adapter<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 部分平台 resumed 可能多次触发；主窗只建一次
        if self.ctx.is_some() {
            return;
        }
        let winit_window = event_loop
            .create_window(build_attrs(&self.config))
            .expect("窗口创建失败");
        self.ctx = Some(Ctx {
            window: Window::from_winit(winit_window),
            keyboard: KeyboardState::default(),
            mouse: MouseState::default(),
            #[cfg(feature = "gamepad")]
            gamepad: crate::base::gamepad::GamepadState::new(),
            delta: 0.0,
            exit: false,
        });
        // 注意：此处【不】调用 app.start——Web 上此时窗口尺寸还是 0×0
        //（真实尺寸经 ResizeObserver 异步到达）。start 延迟到 about_to_wait
        // 中"首个有效尺寸"时执行（见下），让 GPU/服务以正确尺寸初始化，
        // 从顺序上消除尺寸竞态；request_redraw 踢第一脚启动事件循环。
        let ctx = self.ctx.as_mut().unwrap();
        ctx.window.request_redraw();
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WinitEvent) {
        let Some(ctx) = self.ctx.as_mut() else {
            return;
        };
        // 单窗口：非主窗事件忽略
        if ctx.window.winit().id() != id {
            return;
        }
        let win = ctx.window.clone();

        // 状态表更新 + 事件翻译（一次 winit 事件可产生多个语义事件，
        // 如 KeyDown + TextInput）
        self.event_buf.clear();
        translate(&event, ctx, &mut self.event_buf);
        for ev in &self.event_buf {
            self.app.event(&win, ev, ctx);
        }

        // 关闭语义：主窗口关闭 → 应用退出
        if self
            .event_buf
            .iter()
            .any(|e| matches!(e, WindowEvent::CloseRequested))
        {
            ctx.exit = true;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(ctx) = self.ctx.as_mut() else {
            return;
        };
        if ctx.exit {
            event_loop.exit();
            return;
        }

        // ── 启动门：等首个有效窗口尺寸，再执行 app.start ──
        // Web 上真实尺寸由 ResizeObserver 异步送达（桌面在窗口创建后立即可得）。
        // 让事件先跑起来、尺寸先就位，GPU/服务再以正确尺寸初始化——
        // 尺寸竞态从顺序上消除（自愈仅作兜底）。等待期保持 rAF 轮询；
        // 超时（60 帧仍 0×0，如极端无头环境）按当前尺寸兜底启动。
        if !self.started {
            // Android 追加硬门：winit 的 inner_size 在该平台返回显示器尺寸（恒非零，
            // 探不出真窗口就绪）；ANativeWindow 由 onNativeWindowCreated 异步送达，
            // 早于它建 GPU 表面会拿空句柄 → configure 报 "Invalid surface"。
            // 句柄可用性（Err=Unavailable）即就绪信号；不设兜底超时——
            // 可见 Activity 的 native window 必然到达（实测崩溃修复，2026-09-17）。
            #[cfg(target_os = "android")]
            if raw_window_handle::HasWindowHandle::window_handle(&ctx.window).is_err() {
                ctx.window.request_redraw();
                return;
            }
            if ctx.size() == (0, 0) && self.start_wait < 60 {
                self.start_wait += 1;
                ctx.window.request_redraw();
                return;
            }
            self.started = true;
            self.app.start(ctx);
        }

        // 帧节流（fps_cap=0 时不睡）+ 帧回调 + 请求全部窗口下一帧
        let dt = self.clock.tick(self.config.fps_cap);
        ctx.delta = dt;
        // 手柄轮询刷新（feature = "gamepad"）：排水设备事件、更新状态表，
        // 应用在 frame 里经 ctx.gamepad() 读取（对齐 delta 的帧前刷新先例）
        #[cfg(feature = "gamepad")]
        ctx.gamepad.poll();
        self.app.frame(ctx);
        ctx.window.request_redraw();
    }
}




/// WindowConfig → winit 窗口属性（主窗）
fn build_attrs(cfg: &WindowConfig) -> winit::window::WindowAttributes {
    // mut 仅 wasm 分支（canvas 接管）用到，桌面为免告警豁免
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_mut))]
    let mut attrs = WinitWindow::default_attributes()
        .with_title(cfg.title.clone())
        .with_inner_size(LogicalSize::new(cfg.width, cfg.height))
        .with_resizable(cfg.resizable)
        .with_maximized(cfg.maximized);

    // Web：优先接管页面里指定 id 的 <canvas>（嵌入模型）；
    // 未指定/找不到元素时回落 winit 默认行为（自建 canvas，不入 DOM）
    #[cfg(target_arch = "wasm32")]
    if let Some(id) = &cfg.web_canvas_id {
        if let Some(el) = web_sys::window()
            .expect("Web 环境无全局 window")
            .document()
            .expect("Web 环境无 document")
            .get_element_by_id(id)
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            attrs = attrs.with_canvas(Some(el.unchecked_into::<web_sys::HtmlCanvasElement>()));
        }
    }
    attrs
}

/// winit 事件 → 本引擎事件 + 键鼠状态表更新
fn translate(event: &WinitEvent, ctx: &mut Ctx, out: &mut Vec<WindowEvent>) {
    match event {
        WinitEvent::KeyboardInput { event: key, .. } => {
            // Android 返回键：NativeActivity 的输入队列接管按键后，系统默认的
            // "返回 = 结束 Activity"不再生效 → 引擎把 BrowserBack 翻译成
            // CloseRequested，对齐"窗口关闭 = 应用退出"语义（桌面 × 不受影响；
            // 实测卓易通/鸿蒙，2026-09-17）
            #[cfg(target_os = "android")]
            if key.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::BrowserBack) {
                if key.state == ElementState::Pressed {
                    out.push(WindowEvent::CloseRequested);
                }
                return;
            }
            if let PhysicalKey::Code(code) = key.physical_key {
                let k = map_key(code);
                match key.state {
                    ElementState::Pressed => {
                        ctx.keyboard.press(k);
                        out.push(WindowEvent::KeyPressed(k));
                    }
                    ElementState::Released => {
                        ctx.keyboard.release(k);
                        out.push(WindowEvent::KeyReleased(k));
                    }
                }
                if key.state == ElementState::Pressed {
                    if let Some(text) = key.text.as_ref() {
                        for ch in text.chars() {
                            out.push(WindowEvent::TextInput(ch));
                        }
                    }
                }
            }
        }
        WinitEvent::ModifiersChanged(m) => {
            let s = m.state();
            ctx.keyboard.set_modifiers(KeyModifiers {
                shift: s.shift_key(),
                ctrl: s.control_key(),
                alt: s.alt_key(),
                win: s.super_key(),
            });
        }
        WinitEvent::MouseInput { state, button, .. } => {
            let b = match *button {
                winit::event::MouseButton::Left => MouseButton::Left,
                winit::event::MouseButton::Right => MouseButton::Right,
                winit::event::MouseButton::Middle => MouseButton::Middle,
                winit::event::MouseButton::Back => MouseButton::Back,
                winit::event::MouseButton::Forward => MouseButton::Forward,
                winit::event::MouseButton::Other(n) => MouseButton::Other(n),
            };
            match state {
                ElementState::Pressed => {
                    ctx.mouse.press(b);
                    out.push(WindowEvent::MousePressed(b));
                }
                ElementState::Released => {
                    ctx.mouse.release(b);
                    out.push(WindowEvent::MouseReleased(b));
                }
            }
        }
        WinitEvent::CursorMoved { position, .. } => {
            ctx.mouse.set_position(position.x, position.y);
            out.push(WindowEvent::MouseMoved {
                x: position.x,
                y: position.y,
            });
        }
        WinitEvent::MouseWheel { delta, .. } => {
            let (x, y) = match *delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => (x, y),
                winit::event::MouseScrollDelta::PixelDelta(p) => {
                    // 1 行 ≈ 40px：把像素滚动折算成行语义
                    (p.x as f32 / 40.0, p.y as f32 / 40.0)
                }
            };
            out.push(WindowEvent::MouseWheel { x, y });
        }
        WinitEvent::CloseRequested => {
            // 关闭语义：
            // window_closed；最后一窗关闭 → 应用退出）。仍派发事件供应用收尾。
            out.push(WindowEvent::CloseRequested);
        }
        WinitEvent::Resized(size) => out.push(WindowEvent::Resized {
            width: size.width,
            height: size.height,
        }),
        WinitEvent::Focused(f) => out.push(WindowEvent::Focused(*f)),
        _ => {}
    }
}
