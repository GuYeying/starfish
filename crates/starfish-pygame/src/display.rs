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
use starfish_render::settings::{GpuSettings, SurfaceSettings};
use starfish_we::{Window, WindowError, WindowFlags};
use crate::render::Screen;

thread_local! {
    static WINDOW: RefCell<Option<&'static Window>> = const { RefCell::new(None) };
    /// 窗口注册表（2026-11-16 多窗批次——创建序 Vec：泵序 = 创建序；
    /// "main" = 主窗。单窗形态表长 1——遍历零分支）
    static WINDOWS: RefCell<Vec<(String, &'static Window)>> = RefCell::new(Vec::new());
    /// 主窗 Screen 快捷槽（老代码别名——注册表见表 SCREENS）
    static SCREEN: RefCell<Option<&'static Screen>> = const { RefCell::new(None) };
    /// Screen 注册表（2026-11-16 拆口批次——"main" = 主窗；多窗口
    /// 批次按 id 扩展。get_screen() = "main" 快捷——全绑定面零翻新）
    static SCREENS: RefCell<std::collections::HashMap<String, &'static Screen>>
        = RefCell::new(std::collections::HashMap::new());
    /// 脚本级 GPU/表面配置暂存（2026-11-16 py 面透传——set_mode 脚本
    /// 侧经 pygame.display kwargs 注入，GPU 装配（set_mode_ex）时 take
    /// 消费；None = 平台默认。仅首次装配生效——交换链重开不吃新配置）
    static PENDING_SETTINGS: RefCell<Option<(Option<SurfaceSettings>, Option<GpuSettings>)>>
        = const { RefCell::new(None) };
}

/// 注入脚本级 GPU/表面配置（pygame.display.set_mode kwargs 的承载口
/// ——下一次 GPU 装配消费后清空；重复注入 = 后到覆盖）
pub fn set_pending_settings(
    surface: Option<SurfaceSettings>,
    gpu: Option<GpuSettings>,
) {
    PENDING_SETTINGS.with(|s| *s.borrow_mut() = Some((surface, gpu)));
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
// ⚠️ 后端声明位（VULKAN/DIRECTX/METAL/OPENGLES/OPENGL）已剔除
//（2026-11-16 拆步装配批次定约）——后端选择走 GpuSettings.backends
//（脚本期装配 kwargs / env / pending settings 承载）；透明窗用
// TRANSPARENT 位（DX12 推导自动承载）。

/// 已登记旗标全集（未登记位 → 警告并忽略，垫底原则）
pub const KNOWN_MASK: u32 = RESIZABLE
    | FULLSCREEN
    | NOFRAME
    | HIDDEN
    | ALWAYS_ON_TOP
    | TRANSPARENT;


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
        starfish_debug::log_debug!(&format!(
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
    // GPU 设置（后端声明位与透明呈现都在此承载；脚本级配置暂存优先
    // ——pygame.display.set_mode kwargs 注入，take 消费后清空。
    // ⚠️ 宿主先行装配形态（dev/runtime 壳在脚本执行前 set_mode_ex）
    // 下脚本暂存永不存在——env 兜底：PYGST_MEMORY_HINTS /
    // PYGST_POWER_PREF / PYGST_FRAME_LATENCY（装配前可读，运存优化
    // 主通道）
    let (pending_surface, pending_gpu) = PENDING_SETTINGS
        .with(|s| s.borrow_mut().take())
        .unwrap_or((None, None));
    let mut gpu_settings = pending_gpu.unwrap_or_else(|| {
        let mut g = GpuSettings::default();
        if let Ok(h) = std::env::var("PYGST_MEMORY_HINTS") {
            match h.as_str() {
                "memory_usage" => g.memory_hints = wgpu::MemoryHints::MemoryUsage,
                "performance" => g.memory_hints = wgpu::MemoryHints::Performance,
                other => starfish_debug::log_debug!(format!(
                    "[display] PYGST_MEMORY_HINTS 未识别：{other:?}（忽略）"
                )),
            }
        }
        if let Ok(p) = std::env::var("PYGST_POWER_PREF") {
            match p.as_str() {
                "low_power" | "low_energy" => {
                    g.power_preference = wgpu::PowerPreference::LowPower;
                }
                "high_performance" => {
                    g.power_preference = wgpu::PowerPreference::HighPerformance;
                }
                "none" => g.power_preference = wgpu::PowerPreference::None,
                other => starfish_debug::log_debug!(format!(
                    "[display] PYGST_POWER_PREF 未识别：{other:?}（忽略）"
                )),
            }
        }
        g
    });
    // ⚠️ 后端声明位已剔除（2026-11-16 定约）——后端选择走
    // GpuSettings.backends（pending settings kwargs / env / 默认平台
    // 最优）；透明创建位保留（DX12 推导自动承载）。
    let transparent = flags & TRANSPARENT != 0;
    let mut surface_settings = pending_surface.unwrap_or_default();
    if let Ok(v) = std::env::var("PYGST_FRAME_LATENCY") {
        match v.parse::<u32>() {
            Ok(lat) if (1..=8).contains(&lat) => {
                surface_settings.desired_maximum_frame_latency = lat;
            }
            _ => starfish_debug::log_debug!(
                "[display] PYGST_FRAME_LATENCY 非法（须 1..=8 数字，忽略）"
            ),
        }
    }
    #[cfg(target_os = "windows")]
    if transparent {
        // 逐像素透明 = DX12 专属（DirectComposition 呈现 + PreMultiplied
        // 预乘——推导自动承载，无需 flags 后端位）
        surface_settings = surface_settings.with_alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied);
        gpu_settings = gpu_settings
            .with_dx12_swapchain_kind(wgpu::Dx12SwapchainKind::DxgiFromVisual)
            .with_backends(wgpu::Backends::DX12)
            .with_backends(wgpu::Backends::DX12);
    }
    let window: &'static Window = Box::leak(Box::new(
        Window::builder()
            .title("starfish pygame")
            .size(size)
            .flags(wf)
            .build()
            .expect("display.set_mode: 建窗失败"),
    ));
    register_window("main", window);
    // Web：建窗初始位图 = 模板布局 × dpr（sync_canvas_size 定），Screen
    // 装配读 measure=位图——**set_size 必须先于装配**（逻辑→CSS/dpr +
    // 位图），否则装配 1540×1155 / set_size 800×600 之间的首帧渲染
    // depth/颜色附件不匹配（BeginRenderPass 校验失败，非 100% 缩放
    // 必现）。桌面 builder.size 已定窗，重复 set_size 无需。
    #[cfg(target_arch = "wasm32")]
    window.set_size(size);
    // 显式 backends 不可用 → wgpu 自身隐式退化（settings 是用户责任
    // ——后端位剔除后无 flags 回退面）
    let screen = Box::leak(Box::new(
        Screen::new_with(&window, surface_settings, gpu_settings).await?,
    ));
    if transparent {
        screen.set_transparent_bg();
    }
    register_screen("main", screen);
    Ok(())
}

/// Screen 注册（SCREENS 表——Box::leak 静态活。SCREEN 快捷槽同步写
/// ——主窗别名）
fn register_screen(id: impl Into<String>, screen: &'static Screen) {
    let id = id.into();
    SCREENS.with(|m| m.borrow_mut().insert(id.clone(), screen));
    if id == "main" {
        SCREEN.with(|s| *s.borrow_mut() = Some(screen));
    }
}

/// 绘制终点（未 set_mode = 契约违例 panic，ADR-9）
pub fn get_screen() -> &'static Screen {
    screen(None).expect("display.get_screen: 须先 display.set_mode(...)")
}

/// 指定窗口绘制终点（多窗批次——契约 panic 同 get_screen）
pub fn get_screen_id(id: &str) -> &'static Screen {
    screen(Some(id))
        .unwrap_or_else(|| panic!("display.get_screen_id: 窗口 {id:?} 未装配"))
}

/// screen(id) —— Screen 查询（None = 主窗 "main"；多窗批次 id 实义化
/// ——per-id 装配后按 id 查表）
pub fn screen(id: Option<&str>) -> Option<&'static Screen> {
    let key = id.unwrap_or("main").to_string();
    SCREENS.with(|m| m.borrow().get(&key).copied())
}

/// 平台常量（编译期注入优先——`PYGST_PLATFORM` env（cross 构建显式
/// 传）；缺省 cfg 推导：windows/macos/linux/android/web/harmonyos/ios）
pub fn platform() -> &'static str {
    if let Some(p) = std::option_env!("PYGST_PLATFORM") {
        return p;
    }
    if cfg!(target_arch = "wasm32") {
        "web"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "harmonyos") {
        "harmonyos"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "windows"
    }
}

/// 多窗口能力（运行时判定——与 starfish-we multi-window 白名单同步：
/// feature 开启 + 平台 windows。we 层白名单扩展后同步此表）
pub fn multi_window_supported() -> bool {
    cfg!(feature = "multi-window") && platform() == "windows"
}

/// 窗口注册表遍历（泵纪律承载——每帧每窗轮询；创建序）
pub fn with_windows<R>(f: impl FnOnce(&[(String, &'static Window)]) -> R) -> Option<R> {
    WINDOWS.with(|w| {
        let w = w.borrow();
        if w.is_empty() {
            None
        } else {
            Some(f(&w)) // 修正下一段
        }
    })
}

/// ①' 多窗口窗口创建（multi-window 批次——id 注册表形态；平台不支持
/// = 运行时 Err（绑定层映射 Python 异常——用户定约：编译期 feature +
/// 平台常量注入，运行时检测抛错））
pub fn create_window_id(
    id: impl Into<String>,
    size: (u32, u32),
    flags: u32,
) -> Result<(), String> {
    if !multi_window_supported() {
        return Err(format!(
            "多窗口不支持当前构建/平台（feature multi-window 未启用或平台 {} \
             不在白名单——当前白名单：windows）",
            platform()
        ));
    }
    let id = id.into();
    if id == "main" {
        return Err("窗口 id \"main\" 为主窗保留——set_mode 的隐式 id，勿显式占用".into());
    }
    let exists = WINDOWS.with(|w| w.borrow().iter().any(|(k, _)| *k == id));
    if exists {
        return Err(format!("窗口 id {id:?} 已存在"));
    }
    let mut wf = WindowFlags::from_bits(0);
    for (bit, flag) in [
        (RESIZABLE, WindowFlags::RESIZABLE),
        (NOFRAME, WindowFlags::NOFRAME),
        (HIDDEN, WindowFlags::HIDDEN),
        (ALWAYS_ON_TOP, WindowFlags::ALWAYS_ON_TOP),
    ] {
        if flags & bit != 0 {
            wf = wf.union(flag);
        }
    }
    let window = Window::builder()
        .title(&id)
        .size(size)
        .flags(wf)
        .build()
        .map_err(|e| format!("建窗失败: {e}"))?;
    // Box::leak 注册（&'static Window——Vec realloc 移元素也不失效；
    // 窗口生命周期 = 进程尾，同 Screen 静态活定约）
    let leaked: &'static Window = Box::leak(Box::new(window));
    register_window(id, leaked);
    Ok(())
}

/// ②' 多窗口 GPU 装配（per-id——窗须先 create_window_id；settings
/// kwargs/env 同主窗通路）
pub fn create_screen_for(
    id: &str,
    surface_settings: Option<SurfaceSettings>,
    gpu_settings: Option<GpuSettings>,
) -> Result<(), String> {
    starfish_std::app::block_on(async move {
        // &'static Window（Box::leak 注册）——Copy 出闭包免借用逃逸
        let window = WINDOWS
            .with(|w| w.borrow().iter().find(|(k, _)| k == id).map(|(_, w)| *w))
            .ok_or_else(|| format!("窗口 {id:?} 不存在（先 create_window_id）"))?;
        let transparent = false;
        create_screen_at(window, surface_settings, gpu_settings, transparent, id.to_string())
            .await
            .map(|_| ())
            .map_err(|e| format!("GPU 装配失败: {e}"))
    })
}

/// per-id Screen 装配内核（id → SCREENS 表；主窗通路复用）
async fn create_screen_at(
    window: &Window,
    surface_settings: Option<SurfaceSettings>,
    gpu_settings: Option<GpuSettings>,
    transparent: bool,
    id: String,
) -> Result<&'static Screen, RenderContextError> {
    let mut surface_settings = surface_settings.unwrap_or_default();
    if let Ok(v) = std::env::var("PYGST_FRAME_LATENCY") {
        if let Ok(lat) = v.parse::<u32>() {
            if (1..=8).contains(&lat) {
                surface_settings.desired_maximum_frame_latency = lat;
            }
        }
    }
    let mut gpu_settings = gpu_settings.unwrap_or_else(|| {
        let mut g = GpuSettings::default();
        if let Ok(h) = std::env::var("PYGST_MEMORY_HINTS") {
            if h == "memory_usage" {
                g.memory_hints = wgpu::MemoryHints::MemoryUsage;
            }
        }
        if let Ok(p) = std::env::var("PYGST_POWER_PREF") {
            if p == "low_power" || p == "low_energy" {
                g.power_preference = wgpu::PowerPreference::LowPower;
            } else if p == "none" {
                g.power_preference = wgpu::PowerPreference::None;
            }
        }
        g
    });
    #[cfg(target_os = "windows")]
    if transparent {
        surface_settings =
            surface_settings.with_alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied);
        gpu_settings =
            gpu_settings.with_dx12_swapchain_kind(wgpu::Dx12SwapchainKind::DxgiFromVisual).with_backends(wgpu::Backends::DX12);
    }
    let screen = Box::leak(Box::new(
        Screen::new_with(window, surface_settings, gpu_settings).await?,
    ));
    if transparent {
        screen.set_transparent_bg();
    }
    register_screen(id, screen);
    Ok(screen)
}

/// 多窗窗口帧提交（per-id——flip 的多窗词汇）。
pub fn flip_id(id: &str) {
    screen(Some(id))
        .unwrap_or_else(|| panic!("display.flip_id: 窗口 {id:?} 未装配"))
        .present();
}

// ── 拆步装配口（2026-11-16 批次——GPU 装配挪脚本期；三口独立可组
// 合：create_window → create_screen → screen(None)。set_mode_ex =
// 串联糖的 async 形态（保留——web 宿主先行装配形态专用）；native
// 脚本期走 set_mode_blocking（settings kwargs/env 在此真消费）──

/// ① 窗口创建（we 层纯窗口——**不含 GPU**；flags = 窗口语义位
/// RESIZABLE/FULLSCREEN/NOFRAME/HIDDEN/ALWAYS_ON_TOP/TRANSPARENT——
/// 渲染后端位已迁 GpuSettings.backends，此口不再消费）
pub async fn create_window(size: (u32, u32), flags: u32) -> Result<&'static Window, WindowError> {
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
    let window = Window::builder()
        .title("starfish pygame")
        .size(size)
        .flags(wf)
        .build()?;
    // 注册即泵入表（⚠️ 事件泵遍历 WINDOWS——漏注册 = 消息队列不排水
    // → Windows 判定窗口无响应，2026-11-16 实测回归）
    let leaked: &'static Window = Box::leak(Box::new(window));
    register_window("main", leaked);
    Ok(leaked)
}

/// 窗口注册（WINDOWS 表——"main" 主窗同步 WINDOW 主槽别名；Box::leak
/// 静态活——Vec realloc 移元素免疫）
fn register_window(id: impl Into<String>, w: &'static Window) {
    let id = id.into();
    WINDOWS.with(|v| v.borrow_mut().push((id.clone(), w)));
    if id == "main" {
        WINDOW.with(|slot| *slot.borrow_mut() = Some(w));
    }
}

/// ② GPU 驱动创建（wgpu 装配 + Screen 静态注册——surface/gpu settings
/// 显式（None = 平台默认 + env 三口）；transparent = 窗口创建期的透明
/// 语义（DX12 DirectComposition + PreMultiplied 推导归此口））
pub async fn create_screen(
    window: &Window,
    surface_settings: Option<SurfaceSettings>,
    gpu_settings: Option<GpuSettings>,
    transparent: bool,
) -> Result<&'static Screen, RenderContextError> {
    let mut surface_settings = surface_settings.unwrap_or_default();
    if let Ok(v) = std::env::var("PYGST_FRAME_LATENCY") {
        match v.parse::<u32>() {
            Ok(lat) if (1..=8).contains(&lat) => {
                surface_settings.desired_maximum_frame_latency = lat;
            }
            _ => starfish_debug::log_debug!(
                "[display] PYGST_FRAME_LATENCY 非法（须 1..=8 数字，忽略）"
            ),
        }
    }
    let mut gpu_settings = gpu_settings.unwrap_or_else(|| {
        // env 三口兜底（运存优化主通道——装配前可读，宿主/脚本两形态
        // 通用：PYGST_MEMORY_HINTS / PYGST_POWER_PREF）
        let mut g = GpuSettings::default();
        if let Ok(h) = std::env::var("PYGST_MEMORY_HINTS") {
            match h.as_str() {
                "memory_usage" => g.memory_hints = wgpu::MemoryHints::MemoryUsage,
                "performance" => g.memory_hints = wgpu::MemoryHints::Performance,
                other => starfish_debug::log_debug!(format!(
                    "[display] PYGST_MEMORY_HINTS 未识别：{other:?}（忽略）"
                )),
            }
        }
        if let Ok(p) = std::env::var("PYGST_POWER_PREF") {
            match p.as_str() {
                "low_power" | "low_energy" => {
                    g.power_preference = wgpu::PowerPreference::LowPower;
                }
                "high_performance" => {
                    g.power_preference = wgpu::PowerPreference::HighPerformance;
                }
                "none" => g.power_preference = wgpu::PowerPreference::None,
                other => starfish_debug::log_debug!(format!(
                    "[display] PYGST_POWER_PREF 未识别：{other:?}（忽略）"
                )),
            }
        }
        g
    });
    // 透明创建位推导（DX12 DirectComposition + PreMultiplied——创建期
    // 透明语义归窗口，呈现系统推导归此口）
    #[cfg(target_os = "windows")]
    if transparent {
        surface_settings =
            surface_settings.with_alpha_mode(wgpu::CompositeAlphaMode::PreMultiplied);
        gpu_settings =
            gpu_settings.with_dx12_swapchain_kind(wgpu::Dx12SwapchainKind::DxgiFromVisual).with_backends(wgpu::Backends::DX12);
    }
    // Web：建窗初始位图 = 模板布局 × dpr——**set_size 先于装配**（装
    // 配尺寸/set_size 尺寸错位 → BeginRenderPass 校验失败）
    #[cfg(target_arch = "wasm32")]
    window.set_size(window.size());
    let screen = Box::leak(Box::new(
        Screen::new_with(window, surface_settings, gpu_settings).await?,
    ));
    if transparent {
        screen.set_transparent_bg();
    }
    register_screen("main", screen);
    Ok(screen)
}

/// 串联糖（**native 同步**——block_on 三口；wasm 无阻塞模型不提供
/// ——web 宿主先行装配走 set_mode_ex）。settings kwargs/env 在此真
/// 消费（脚本期装配定约）。⚠️ 进程一次契约（重复调用 panic——同
/// set_mode_ex）。
#[cfg(not(target_arch = "wasm32"))]
pub fn set_mode_blocking(
    size: (u32, u32),
    flags: u32,
    surface_settings: Option<SurfaceSettings>,
    gpu_settings: Option<GpuSettings>,
) -> Result<(), String> {
    starfish_std::app::block_on(async move {
        let transparent = flags & TRANSPARENT != 0;
        let window = create_window(size, flags)
            .await
            .map_err(|e| format!("建窗失败: {e}"))?;
        create_screen(window, surface_settings, gpu_settings, transparent)
            .await
            .map_err(|e| format!("GPU 装配失败: {e}"))?;
        Ok(())
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

/// 请求屏幕方向（2026-11-12 竖屏应用支持）：`"portrait"` /
/// `"landscape"` / `"sensor_portrait"` / `"sensor_landscape"` /
/// `"sensor"` / `"full_sensor"` / `"unspecified"`（ActivityInfo 常量
/// 同语义）。Android 实装（Activity 级 setRequestedOrientation 经
/// ndk-context JNI——运行时即时覆盖 manifest 初始请求，configChanges
/// 已含 orientation 的宿主只重表面不重建 Activity）；桌面/web = no-op
/// Ok（方向是移动端概念，跨端脚本零分支）；OHOS = 站桩 Err。
pub fn set_orientation(orientation: &str) -> Result<(), String> {
    match with_window(|w| w.set_orientation(orientation)) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("display 未初始化（须先 set_mode）".into()),
    }
    // 意图记录在 we 层 Window::set_orientation（2026-11-12 编排下沉统一）
}


/// 鼠标事件穿透开关（点击落到下层窗口——桌宠形态配套；2026-11-07
/// 穿透设计稿 M1。Win32 实装；不支持平台 Err——能力位垫底原则。
/// ⚠️ feature `click-through` 门控——未启用 = 编译期即知不支持）
#[cfg(feature = "click-through")]
pub fn set_click_through(enable: bool) -> Result<(), String> {
    match with_window(|w| w.set_click_through(enable)) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("display 未初始化（须先 set_mode）".into()),
    }
}

/// 窗口当前是否鼠标穿透（OS ex-style 位读回）
#[cfg(feature = "click-through")]
pub fn get_click_through() -> bool {
    with_window(|w| w.is_click_through()).unwrap_or(false)
}

/// 任务栏（Dock）隐藏开关（桌宠形态配套：Win=WS_EX_TOOLWINDOW 位切换
/// ——Alt+Tab 一并隐藏；macOS=Dock 级 setActivationPolicy 切换；Linux
/// Wayland 站桩 Err。⚠️ feature `skip-taskbar` 门控——未启用 = 编译期
/// 即知不支持）
#[cfg(feature = "skip-taskbar")]
pub fn set_skip_taskbar(enable: bool) -> Result<(), String> {
    match with_window(|w| w.set_skip_taskbar(enable)) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("display 未初始化（须先 set_mode）".into()),
    }
}

/// 窗口当前是否任务栏隐藏（OS 状态读回；stub 平台恒 false）
#[cfg(feature = "skip-taskbar")]
pub fn get_skip_taskbar() -> bool {
    with_window(|w| w.is_skip_taskbar()).unwrap_or(false)
}

/// 安装（或替换）系统托盘图标（桌宠形态配套：RGBA 图标 + 悬停提示 +
/// 平铺菜单；左键=Tray 点击事件、右键=弹菜单、菜单点击=Tray 菜单事件，
/// 均经 `event::get` 上浮。⚠️ feature `tray` 门控——未启用 = 编译期即
/// 知不支持。`surface` 须为离屏 Surface（Screen 背板不回读）——图标
/// 回读走 image::readback_rgba（image.save 同款慢路径）。
/// wasm 拆分：无阻塞模型（block_on 不存在）且平台本就 Err 站桩——
/// 直接 Err 兜底，不进回读）
#[cfg(all(feature = "tray", target_arch = "wasm32"))]
pub fn set_tray(
    _surface: &crate::render::Surface,
    _tooltip: &str,
    _menu: &[(String, String)],
) -> Result<(), String> {
    Err("tray: web 平台不支持（浏览器沙箱）".into())
}

#[cfg(all(feature = "tray", not(target_arch = "wasm32")))]
pub fn set_tray(
    surface: &crate::render::Surface,
    tooltip: &str,
    menu: &[(String, String)],
) -> Result<(), String> {
    use crate::render::DrawTarget as _; // surface.size()（DrawTarget trait 方法）
    let read = starfish_std::app::block_on(crate::image::readback_rgba(surface))
        .map_err(|e| format!("tray 图标回读失败: {e}"))?;
    let (icon_w, icon_h) = surface.size();
    let spec = starfish_we::TraySpec {
        icon_rgba: read,
        icon_w,
        icon_h,
        tooltip: tooltip.to_string(),
        menu: menu.to_vec(),
    };
    match with_window(|w| w.set_tray(spec)) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("display 未初始化（须先 set_mode）".into()),
    }
}

/// 移除托盘图标（未安装时空操作）
#[cfg(feature = "tray")]
pub fn remove_tray() -> Result<(), String> {
    match with_window(|w| w.remove_tray()) {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("display 未初始化（须先 set_mode）".into()),
    }
}

/// 窗口请求运行时生效（批次 O + 尺寸扩展——真身窗口由宿主壳先行装配，
/// 桌面 letterbox 复现钩子（诊断用）：`STARFISH_DESKTOP_LETTERBOX=宽x高`
/// 环境变量生效时，窗口实测尺寸钉为该值、逻辑恒 = set_mode 请求——
/// Android 窗口化行为（物理≠逻辑 + 引擎等比居中映射）在本机复现，
/// 供自绘管线/叠加通道的 letterbox 对齐做本地调试。未设置 = None
pub(crate) fn desktop_letterbox() -> Option<(u32, u32)> {
    starfish_debug::letterbox_sim()
}

/// 脚本 set_mode 为完整请求：set_size 跟随 + 旗标经 Window::set_* 即时
/// 生效。创建期一次性旗标（透明）垫底收下。
pub fn apply_window_request(size: (u32, u32), flags: u32) {
    // 落地耗时打点（慢调用哨兵——启动诊断用）
    let t = starfish_std::debug::Stopwatch::now();
    // starfish-debug 全屏形态模拟（创建时声明）：脚本 set_mode 一律按
    // FULLSCREEN 旗标处理——移动端全屏形态的本机对照
    let flags = if starfish_debug::fullscreen_sim() { flags | FULLSCREEN } else { flags };
    let desktop_lb = desktop_letterbox();
    // ── 编排下沉（2026-11-12：starfish-we Window::set_display_mode 生态
    // 统一口——自动方向 + 全屏转换序 + 实测交换全在引擎窗口层，本层
    // 只补 pygame 语义位 HIDDEN/ALWAYS_ON_TOP 与同步 resize）──
    let effective = with_window(|w| {
        Some(w.set_display_mode(
            desktop_lb.unwrap_or(size),
            flags & FULLSCREEN != 0,
            flags & NOFRAME != 0,
            flags & RESIZABLE != 0,
        ))
    })
    .flatten()
    .unwrap_or(size);
    with_window(|w| {
        // HIDDEN 运行时语义 = 隐藏窗口（真 pygame 为创建期；垫底执行）
        if flags & HIDDEN != 0 {
            w.set_visible(false);
        }
        if flags & ALWAYS_ON_TOP != 0 {
            w.set_always_on_top(true);
        }
    });
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

/// 物理点 → **逻辑中心系**点（输入坐标映射的统一收口：物理 → 逻辑
/// 画布（letterbox 逆映射）→ **y 翻转 + 中心原点化**——坐标系正交化
/// 定约：脚本坐标 = 中心原点、y 向上，与 3D 透视/物理米制系同构）。
/// display 未就绪 = 原样透传——事件可先于 set_mode 到达
pub fn map_point(x: f64, y: f64) -> (f64, f64) {
    SCREEN.with(|s| match s.borrow().as_ref() {
        Some(screen) => {
            let (lx, ly) = screen.map_point_to_logical(x, y);
            let (lw, lh) = screen.logical_size();
            // y 翻转 + 中心原点化（逻辑画布域内：左上 (0,0) → (-w/2, +h/2)）
            (lx - lw as f64 / 2.0, lh as f64 / 2.0 - ly)
        }
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
        starfish_debug::log_debug!(&format!(
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
pub fn with_window<R>(f: impl FnOnce(&Window) -> R) -> Option<R> {
    // &Window 契约（we Window 方法面全 &self——set_display_mode/
    // set_size/…；&'static 槽免 owned 拷贝）
    WINDOW.with(|w| w.borrow().as_ref().map(|win| f(win)))
}
