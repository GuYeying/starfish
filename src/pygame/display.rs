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

use crate::base::render::render_entry::RenderContextError;
use crate::base::window::Window;
use crate::pygame::render::Screen;

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static SCREEN: RefCell<Option<&'static Screen>> = const { RefCell::new(None) };
}

/// 建窗 + GPU 装配（契约 §七：**进程一次**；重复调用 = 契约违例 panic）
///
/// 尺寸即 display 逻辑尺寸（ADR-8：1 px ≡ 1 surface 逻辑像素；此后窗口
/// resize 不改 display 尺寸）。等价 [`set_mode_ex`]`(size, 0)`。
pub async fn set_mode(size: (u32, u32)) -> Result<(), RenderContextError> {
    set_mode_ex(size, 0).await
}

/// [`set_mode`] 的旗标扩展版（对位 `set_mode(size, flags)`；
/// 旗标常量见 [`crate::pygame::locals::display`]）
///
/// 未登记位 → 警告并忽略（垫底原则）。后端声明位（VULKAN/DIRECTX/
/// METAL/OPENGLES/OPENGL）→ GpuSettings.backends（wgpu 在声明位集内
/// 自动挑选，不可用即**隐式退化**回全后端重试一次）；透明位 → 创建期
/// 透明合成 + Screen 透明底清屏。
pub async fn set_mode_ex(size: (u32, u32), flags: u32) -> Result<(), RenderContextError> {
    use crate::base::render::settings::{GpuSettings, SurfaceSettings};
    use crate::base::window::WindowFlags;
    use crate::pygame::locals::display as dflags;

    let exists = SCREEN.with(|s| s.borrow().is_some());
    if exists {
        panic!("display.set_mode 进程一次（契约 §七）；重复调用不支持");
    }
    if flags & !dflags::KNOWN_MASK != 0 {
        crate::base::debug::console_log(&format!(
            "[pygame] set_mode: 未登记旗标位 0x{:X} 忽略（垫底原则）",
            flags & !dflags::KNOWN_MASK
        ));
    }
    let mut wf = WindowFlags::from_bits(0);
    for (bit, flag) in [
        (dflags::RESIZABLE, WindowFlags::RESIZABLE),
        (dflags::FULLSCREEN, WindowFlags::FULLSCREEN),
        (dflags::NOFRAME, WindowFlags::NOFRAME),
        (dflags::HIDDEN, WindowFlags::HIDDEN),
        (dflags::ALWAYS_ON_TOP, WindowFlags::ALWAYS_ON_TOP),
        (dflags::TRANSPARENT, WindowFlags::TRANSPARENT),
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
        (dflags::VULKAN, wgpu::Backends::VULKAN),
        (dflags::DIRECTX, wgpu::Backends::DX12),
        (dflags::METAL, wgpu::Backends::METAL),
        (dflags::OPENGLES | dflags::OPENGL, wgpu::Backends::GL),
    ] {
        if flags & bit != 0 {
            backends |= be;
        }
    }
    // 透明创建位：DirectComposition 呈现（DxgiFromVisual——wgpu 文档
    // 明示 DxgiFromHwnd "does not support transparency"）+ 表面
    // **显式 PreMultiplied**（管线 ALPHA_BLENDING 的 alpha 通道因子
    // One/OneMinusSrcAlpha 累积即预乘形式，匹配）。
    // ⚠ 逐像素透明 = **DX12 专属**（Vulkan Win32 表面只报 Opaque）——
    // 透明时强制 DX12 后端（平台矩阵：Windows ✓；Linux/macOS 窗口后端
    // 未实现——透明随平台后端落地；Android/Web 排除）
    let transparent = flags & dflags::TRANSPARENT != 0;
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
    // 显式后端不可用 → 隐式退化回全后端重试一次（用户定案：
    // 失败让 wgpu 自己隐式退化）
    let screen = Box::leak(Box::new(
        match Screen::new_with(&window, surface_settings.clone(), gpu_settings).await {
            Ok(s) => s,
            Err(_) if explicit_backends => {
                crate::base::debug::console_log(
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
/// 脚本 set_mode 为完整请求：set_size 跟随 + 旗标经 Window::set_* 即时
/// 生效。创建期一次性旗标（透明）垫底收下；OPENGL 垫底：引擎恒 wgpu）
pub fn apply_window_request(size: (u32, u32), flags: u32) {
    use super::locals::display as fl;
    if flags & fl::OPENGL != 0 {
        crate::base::debug::console_log("[display] OPENGL 旗标垫底（引擎恒 wgpu）");
    }
    with_window(|w| {
        w.set_size(size);
        w.set_resizable(flags & fl::RESIZABLE != 0);
        w.set_fullscreen(flags & fl::FULLSCREEN != 0);
        w.set_borderless(flags & fl::NOFRAME != 0);
        // HIDDEN 运行时语义 = 隐藏窗口（真 pygame 为创建期；垫底执行）
        if flags & fl::HIDDEN != 0 {
            w.set_visible(false);
        }
        if flags & fl::ALWAYS_ON_TOP != 0 {
            w.set_always_on_top(true);
        }
    });
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
        get_screen().resize(width, height);
    }
}

/// 事件模块的窗口访问缝（排空事件队列）；display 未就绪 → None
pub(crate) fn with_window<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
    WINDOW.with(|w| {
        let mut w = w.borrow_mut();
        w.as_mut().map(f)
    })
}
