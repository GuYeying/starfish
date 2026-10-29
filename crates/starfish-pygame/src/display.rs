//! pygame.display 对应物（契约 §七 / ADR-6：单窗口 + 模块级全局槽）
//!
//! `set_mode` 一次性建窗 + GPU 装配（进程一次）；`get_screen()` 形态 =
//! **`&'static Screen`**（Box::leak + Screen 内部 RefCell，绘制方法
//! `&self`）——教程代码 `let screen = display.get_screen();` 拿一次到处
//! 用，Python 绑定层零翻译。
//!
//! 平台执行模型说明：GPU 装配在 Web 上是纯异步（WebGPU 无阻塞模型），
//! 故 `set_mode` 为 **async**——`display.set_mode((800, 600)).await?;`
//! 一处 await 之外全是同步绘制（pygame 心智保留）。
//!
//! 全局槽 = thread_local（与 render 的 GPU 槽同理：主线程契约的类型级
//! 表达，wasm 的 Window/wgpu 类型非 Send/Sync）。

use std::cell::RefCell;

use starfish_render::render_entry::RenderContextError;
use starfish_we::Window;
use crate::render::Screen;

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static SCREEN: RefCell<Option<&'static Screen>> = const { RefCell::new(None) };
}


/// 可调整大小（对位 pygame RESIZABLE）
pub const RESIZABLE: u32 = 1 << 0;
/// 全屏（无边框占屏；对位 pygame FULLSCREEN）
pub const FULLSCREEN: u32 = 1 << 1;
/// 无边框（对位 pygame NOFRAME）
pub const NOFRAME: u32 = 1 << 2;
/// 创建时隐藏（对位 pygame WINDOW_HIDDEN）
pub const HIDDEN: u32 = 1 << 3;
/// 窗口置顶（对位 pygame WINDOW_ALWAYS_ON_TOP）
pub const ALWAYS_ON_TOP: u32 = 1 << 6;
/// 透明窗口（**创建期**——透明合成 + Screen 清屏改 alpha 0；
/// 桌宠/悬浮件形态。运行期不可追溯）
pub const TRANSPARENT: u32 = 1 << 7;
/// 后端声明（wgpu 在声明位集内自动挑选、不可用即隐式退化；
/// 未声明任何后端位 = `Backends::all()` 平台最优）。
/// 注意：GPU 初始化在 set_mode_ex 内进程一次——后端声明须在
/// 首次 set_mode 时给出（运行期不可重选）。
pub const VULKAN: u32 = 1 << 8;
pub const DIRECTX: u32 = 1 << 9;
pub const METAL: u32 = 1 << 10;
pub const OPENGLES: u32 = 1 << 11;
/// GL 上下文请求（2026-09-30 语义变更：由"显式报错"改为后端声明
/// = wgpu GL 后端，不可用隐式退化）
pub const OPENGL: u32 = 1 << 30;

/// 已登记旗标全集（未登记位 → 警告并忽略，垫底原则）
pub const KNOWN_MASK: u32 = RESIZABLE
    | FULLSCREEN
    | NOFRAME
    | HIDDEN
    | ALWAYS_ON_TOP
    | TRANSPARENT
    | VULKAN
    | DIRECTX
    | METAL
    | OPENGLES
    | OPENGL;


/// 建窗 + GPU 装配（契约 §七：**进程一次**；重复调用 = 契约违例 panic）
///
/// 尺寸即 display 逻辑尺寸（ADR-8：1 px ≡ 1 surface 逻辑像素；此后窗口
/// resize 不改 display 尺寸）。等价 [`set_mode_ex`]`(size, 0)`。
pub async fn set_mode(size: (u32, u32)) -> Result<(), RenderContextError> {
    set_mode_ex(size, 0).await
}

/// [`set_mode`] 的旗标扩展版（对位 `set_mode(size, flags)`；
/// 旗标常量见 [`crate::locals::display`]）
///
/// 未登记位 → 警告并忽略（垫底原则）。后端声明位（VULKAN/DIRECTX/
/// METAL/OPENGLES/OPENGL）→ GpuSettings.backends（wgpu 在声明位集内
/// 自动挑选，不可用即**隐式退化**回全后端重试一次）；透明位 → 创建期
/// 透明合成 + Screen 透明底清屏。
pub async fn set_mode_ex(size: (u32, u32), flags: u32) -> Result<(), RenderContextError> {
    use starfish_render::settings::{GpuSettings, SurfaceSettings};
    use starfish_we::WindowFlags;


    let exists = SCREEN.with(|s| s.borrow().is_some());
    if exists {
        panic!("display.set_mode 进程一次（契约 §七）；重复调用不支持");
    }
    if flags & !KNOWN_MASK != 0 {
        starfish_std::debug::console_log(&format!(
            "[pygame] set_mode: 未登记旗标位 0x{:X} 忽略（垫底原则）",
            flags & !KNOWN_MASK
        ));
    }
    let mut wf = WindowFlags::from_bits(0);
    for (bit, flag) in [
        (RESIZABLE, WindowFlags::RESIZABLE),
        (FULLSCREEN, WindowFlags::FULLSCREEN),
        (NOFRAME, WindowFlags::NOFRAME),
        (HIDDEN, WindowFlags::HIDDEN),
        (ALWAYS_ON_TOP, WindowFlags::ALWAYS_ON_TOP),
        (TRANSPARENT, WindowFlags::TRANSPARENT),
    ] {
        if flags & bit != 0 {
            wf = wf.union(flag);
        }
    }
    // GPU 设置（后端声明位与透明呈现都在此承载）
    let mut gpu_settings = GpuSettings::default();
    // 后端声明位 → GpuSettings.backends（wgpu 位集内自动挑选；
    // 未声明 = Backends::all 平台最优。GL 位（OPENGL/OPENGLES）同语义，
    // 不可用隐式退化——旧"显式报错"垫底表已废）
    let mut backends = wgpu::Backends::empty();
    for (bit, be) in [
        (VULKAN, wgpu::Backends::VULKAN),
        (DIRECTX, wgpu::Backends::DX12),
        (METAL, wgpu::Backends::METAL),
        (OPENGLES | OPENGL, wgpu::Backends::GL),
    ] {
        if flags & bit != 0 {
            backends |= be;
        }
    }
    // Web：GL 旗标（OPENGL/OPENGLES——GLES 判定探针的创建期预扫描词置
    // 位）在 wasm 上追加 BROWSER_WEBGPU 优先位。wgpu 适配器择序 WebGPU
    // 先于 GL，旗标语义（"要 GLES 家族"）落为"WebGPU 优先、WebGL2 兜
    // 底"——强制单 GL 会落 glow web_sys 的采样参数 f32 切片不支持
    // panic（Sampler parameter for `f32` slice is not supported，
    // surface_size_probe 建窗即 trap，2026-10-29 真页实测）。
    #[cfg(target_arch = "wasm32")]
    if backends.intersects(wgpu::Backends::GL) {
        backends |= wgpu::Backends::BROWSER_WEBGPU;
    }
    // 透明创建位：DirectComposition 呈现（DxgiFromVisual——wgpu 文档
    // 明示 DxgiFromHwnd "does not support transparency"）+ 表面
    // **显式 PreMultiplied**（管线 ALPHA_BLENDING 的 alpha 通道因子
    // One/OneMinusSrcAlpha 累积即预乘形式，匹配）。
    // ⚠ 逐像素透明 = **DX12 专属**（Vulkan Win32 表面只报 Opaque）——
    // 透明时强制 DX12 后端（平台矩阵：Windows ✓；Linux/macOS 窗口后端
    // 未实现——透明随平台后端落地；Android/Web 排除）
    let transparent = flags & TRANSPARENT != 0;
    let mut surface_settings = SurfaceSettings::default();
    #[cfg(target_os = "windows")]
    if transparent {
        surface_settings = surface_settings.with_alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied);
        gpu_settings = gpu_settings
            .with_dx12_swapchain_kind(wgpu::Dx12SwapchainKind::DxgiFromVisual);
        backends |= wgpu::Backends::DX12;
    }
    let explicit_backends = backends != wgpu::Backends::empty();
    if explicit_backends {
        gpu_settings = gpu_settings.with_backends(backends);
    }
    let window = Window::builder()
        .title("starfish pygame")
        .size(size)
        .flags(wf)
        .build()
        .expect("display.set_mode: 建窗失败");
    // Web：建窗初始位图 = 模板布局 × dpr（sync_canvas_size 定），Screen
    // 装配读 measure=位图——**set_size 必须先于装配**（逻辑→CSS/dpr +
    // 位图），否则装配 1540×1155 / set_size 800×600 之间的首帧渲染
    // depth/颜色附件不匹配（BeginRenderPass 校验失败，非 100% 缩放
    // 必现）。桌面 builder.size 已定窗，重复 set_size 无需。
    #[cfg(target_arch = "wasm32")]
    window.set_size(size);
    // 显式后端不可用 → 隐式退化回全后端重试一次（用户定案：
    // 失败让 wgpu 自己隐式退化）
    let screen = Box::leak(Box::new(
        match Screen::new_with(&window, surface_settings.clone(), gpu_settings).await {
            Ok(s) => s,
            Err(_) if explicit_backends => {
                starfish_std::debug::console_log(
                    "[pygame] 声明后端不可用，回退全后端重试（wgpu 隐式退化）",
                );
                Screen::new_with(&window, surface_settings, GpuSettings::default()).await?
            }
            Err(e) => return Err(e),
        },
    ));
    if transparent {
        screen.set_transparent_bg();
    }
    WINDOW.with(|w| *w.borrow_mut() = Some(window));
    SCREEN.with(|s| *s.borrow_mut() = Some(screen));
    Ok(())
}

/// 绘制终点（未 set_mode = 契约违例 panic，ADR-9）
pub fn get_screen() -> &'static Screen {
    SCREEN.with(|s| {
        *s.borrow()
            .as_ref()
            .expect("display.get_screen: 须先 display.set_mode(...)")
    })
}

/// 帧提交（batch flush + present；即 Screen::present 的 pygame 词汇）
pub fn flip() {
    get_screen().present();
}

/// 窗口标题（pygame set_caption）
pub fn set_caption(title: &str) {
    with_window(|w| w.set_title(title));
}

/// 窗口请求运行时生效（批次 O + 尺寸扩展——真身窗口由宿主壳先行装配，
/// 桌面 letterbox 复现钩子（诊断用）：`STARFISH_DESKTOP_LETTERBOX=宽x高`
/// 环境变量生效时，窗口实测尺寸钉为该值、逻辑恒 = set_mode 请求——
/// Android 窗口化行为（物理≠逻辑 + 引擎等比居中映射）在本机复现，
/// 供自绘管线/叠加通道的 letterbox 对齐做本地调试。未设置 = None
pub(crate) fn desktop_letterbox() -> Option<(u32, u32)> {
    static CACHED: std::sync::OnceLock<Option<(u32, u32)>> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("STARFISH_DESKTOP_LETTERBOX").ok().and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
    })
}

/// 脚本 set_mode 为完整请求：set_size 跟随 + 旗标经 Window::set_* 即时
/// 生效。创建期一次性旗标（透明）垫底收下；OPENGL 垫底：引擎恒 wgpu）
pub fn apply_window_request(size: (u32, u32), flags: u32) {

    if flags & OPENGL != 0 {
        starfish_std::debug::console_log("[display] OPENGL 旗标垫底（引擎恒 wgpu）");
    }
    // 落地耗时打点（慢调用哨兵——启动诊断用）
    let t = starfish_std::debug::Stopwatch::now();
    let desktop_lb = desktop_letterbox();
    with_window(|w| {
        // 复现钩子：窗口实测钉为环境变量指定尺寸（逻辑≠物理的前提）
        w.set_size(desktop_lb.unwrap_or(size));
        // FULLSCREEN 解除 min=max 钉死（Wayland 合成器的约束引擎在全屏
        // 态也吃 max_size——钉请求尺寸时 configure 尺寸原地不变，窗口
        // 字母箱钉在请求尺寸，Mutter 真机实测 2026-10-29）
        w.set_resizable(flags & RESIZABLE != 0 || flags & FULLSCREEN != 0);
        w.set_fullscreen(flags & FULLSCREEN != 0);
        w.set_borderless(flags & NOFRAME != 0);
        // HIDDEN 运行时语义 = 隐藏窗口（真 pygame 为创建期；垫底执行）
        if flags & HIDDEN != 0 {
            w.set_visible(false);
        }
        if flags & ALWAYS_ON_TOP != 0 {
            w.set_always_on_top(true);
        }
    });
    // FULLSCREEN 旗标：尺寸请求作废——画布改取原生窗口实测尺寸
    // （Android 嵌入 = 外部窗口实测值，满屏 1:1 零变形零映射；Linux
    // Wayland = 全屏 configure 回落后的窗口实测值——后端 set_fullscreen
    // 内有界往返收集 configure，此处同步可取；实测失败垫底回请求尺寸）。
    // 非全屏 = 请求尺寸原样（GLES 缩放铺满，既定语义）。
    // "set_mode 与 fullscreen 处理"的语义修正定案
    let effective = if flags & FULLSCREEN != 0 {
        // 全屏画布 = 原生窗口实测尺寸（Android 嵌入 = 外部窗实测；桌面 =
        // 全屏 apply 后的窗实测——Win32 SetWindowPos 同步发 WM_SIZE、
        // Wayland 后端 set_fullscreen 内先解钉 commit 再有界往返收
        // configure，均可同步取到；Web 全屏为 no-op，实测 = 请求 = 原
        // 行为）。实测失败垫底回请求尺寸。
        match with_window(|w| w.size()) {
            Some(native) if native.0 > 0 && native.1 > 0 => native,
            _ => size,
        }
    } else {
        size
    };
    // 逻辑画布登记：Android 窗口化下 = 脚本请求尺寸（引擎等比居中
    // 映射到物理）；桌面/物理可跟随平台 = 物理即逻辑（handle_resized
    // 内统一回填）
    #[cfg(target_os = "android")]
    if get_init() {
        get_screen().set_logical_size(effective.0, effective.1);
    }
    // 尺寸变更同步落地（2026-10-26 web 黑屏根因修复）：set_mode 本是
    // 同步语义（真 pygame 同）——画布位图在 set_size 处已即时变化，
    // 引擎侧 surface/depth 若等异步事件下一帧才重配置，脚本 set_mode
    // 返回后立即绘制即撞"深度 800×600 vs 颜色 400×300"校验失败 →
    // CommandBuffer 永久失效。此处同步补发引擎 resize；同尺寸由
    // RenderSurface::resize 短路（重复事件零成本）。
    // Android 窗口化例外：物理表面恒为原生窗口实测尺寸（不随请求
    // 缩小——缩小会在 buffer 内角落锚定），脚本画布由呈现映射缩放
    #[cfg(not(target_os = "android"))]
    if get_init() {
        match desktop_lb {
            // 复现钩子：物理跟随窗口实测尺寸（钩子值），逻辑钉回
            // set_mode 请求——物理≠逻辑的 Android 窗口化状态在本机成立
            Some((fw, fh)) => {
                handle_resized(fw, fh);
                if get_init() {
                    get_screen().set_logical_size(size.0, size.1);
                }
            }
            None => handle_resized(effective.0, effective.1),
        }
    }
}

/// 物理点 → 逻辑点（输入坐标映射；呈现映射的逆变换。display 未就绪 =
/// 原样透传——事件可先于 set_mode 到达）
pub fn map_point(x: f64, y: f64) -> (f64, f64) {
    SCREEN.with(|s| match s.borrow().as_ref() {
        Some(screen) => screen.map_point_to_logical(x, y),
        None => (x, y),
    })
}

/// 窗口整体不透明度（0.0 透明 .. 1.0 不透明；桌面窗口合成——
/// 桌宠/悬浮件形态。与 TRANSPARENT 创建旗标正交：前者调窗口整体
/// alpha，后者让 alpha 0 区域透出桌面）
pub fn set_opacity(opacity: f32) {
    with_window(|w| w.set_opacity(opacity.clamp(0.0, 1.0)));
}

/// 终结 display（pygame display.quit）：窗口与槽位回收；此后 get_screen
/// 会 panic（pygame 同语义）。泄漏的 &'static Screen 不回收（进程级槽）。
pub fn quit() {
    WINDOW.with(|w| *w.borrow_mut() = None);
    SCREEN.with(|s| *s.borrow_mut() = None);
}

/// display 是否已就绪（垫底自查口；不报错）
pub fn get_init() -> bool {
    SCREEN.with(|s| s.borrow().is_some())
}

/// 窗口物理尺寸变化的自愈接线（批次二十）：渲染表面交换链 + 正交相机
/// 跟随物理尺寸。ADR-8：display 逻辑尺寸不跟随（800×600 语义保留）。
/// Android 旋转（TerminateWindow/InitWindow 重建 ANativeWindow）后的
/// 必经恢复路径——由 `event::translate` 在 Resized 事件处自动调用。
pub(crate) fn handle_resized(width: u32, height: u32) {
    if get_init() {
        // 批次二十四诊断：Android 走 devlog（文件+TCP 真机可取证）
        #[cfg(target_os = "android")]
        starfish_std::debug::devlog(&format!(
            "[display] resize 自愈 → {width}×{height}"
        ));
        // request_resize（非 resize）：帧进行中的缩放延迟到帧边界落地——
        // 帧中重配置 = 深度新/颜色旧附件尺寸不匹配（批次二十三实测命令缓冲无效）
        get_screen().request_resize(width, height);
        // 桌面 letterbox 复现钩子生效时：逻辑归 set_mode 所有（物理跟随
        // 窗口实测）——与 Android 窗口化同一语义
        if desktop_letterbox().is_some() {
            return;
        }
        // 桌面/web：物理窗口变化 = 逻辑画布跟随（pygame 语义——窗口即
        // 画布）；Android 逻辑归脚本 set_mode 所有（物理恒原生窗口，
        // 呈现映射等比缩放）
        #[cfg(not(target_os = "android"))]
        get_screen().set_logical_size(width, height);
    }
}

/// 窗口访问缝（原事件模块专用，2026-10-24 起公开——外接输入消费者如
/// egui 适配层需要 `&Window`）；display 未就绪 → `None`。
/// ⚠️ 借用即 `WINDOW` 槽 `borrow_mut`——嵌套调用会 panic（排队到外层
/// 闭包外再做）。
pub fn with_window<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
    WINDOW.with(|w| {
        let mut w = w.borrow_mut();
        w.as_mut().map(f)
    })
}
