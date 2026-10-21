//! Window creation flags (SDL flag semantics; bit values/reservations are
//! specific to this crate).
//! 窗口创建标志（SDL flag 语义；值/预留位为本 crate 自定）。
//!
//! - A flag is the **initial state**; runtime-toggleable options live in the
//!   `Window::set_*` method family.
//!   flag = **初始状态**；运行期可切项走 `Window` 的 `set_*` 方法族。
//! - High bits are reserved: adding a new flag never breaks existing callers.
//!   高位预留：新 flag 加位不破坏旧调用者。
//! - Bits unsupported by a backend are ignored (a few warn); cross-platform
//!   scripts are unaffected.
//!   后端不支持的位：忽略（个别 warn），跨平台脚本不受影响。
//! - The bit × backend support matrix lives in the integration guide and in
//!   each backend module's docs.
//!   每个位 × 后端的支持矩阵见 wgpu接入指南.md / 各后端模块文档。

/// Window creation flags (bit combination; `Default` =
/// [`WindowFlags::RESIZABLE`] only).
/// 窗口创建标志（位组合；`Default` = 仅 [`WindowFlags::RESIZABLE`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowFlags(u32);

impl WindowFlags {
    /// The window can be resized by the user (includes the maximize button).
    /// Included by default.
    /// 可由用户调整大小（含最大化按钮）。默认已含。
    pub const RESIZABLE: Self = Self(1 << 0);
    /// Borderless fullscreen (window placement is saved on entry and
    /// restored on exit).
    /// 无边框全屏（进入时保存 placement，退出还原）。
    pub const FULLSCREEN: Self = Self(1 << 1);
    /// Borderless (no system title bar or frame).
    /// 无边框（无系统标题栏与边框）。
    pub const NOFRAME: Self = Self(1 << 2);
    /// Created invisible (hidden; can be toggled at runtime via show/hide).
    /// 创建即可见 = false（隐藏；运行期 show/hide 可切）。
    pub const HIDDEN: Self = Self(1 << 3);
    /// High pixel density (HiDPI, physical-pixel coordinates).
    /// 高像素密度（HiDPI 物理像素坐标）。
    pub const HIGH_PIXEL_DENSITY: Self = Self(1 << 4);
    /// Start with the mouse grabbed (confined to the client area;
    /// toggleable at runtime).
    /// 创建即抓取鼠标（限定客户区；运行期可切）。
    pub const MOUSE_GRABBED: Self = Self(1 << 5);
    /// Window always on top.
    /// 窗口置顶。
    pub const ALWAYS_ON_TOP: Self = Self(1 << 6);
    /// Transparent window (when the compositor supports it; v0 works on
    /// some Windows setups).
    /// 窗口透明（合成器支持时；v0 部分 Windows 平台生效）。
    pub const TRANSPARENT: Self = Self(1 << 7);
    /// Start minimized.
    /// 创建即最小化。
    pub const MINIMIZED: Self = Self(1 << 8);
    /// Start maximized.
    /// 创建即最大化。
    pub const MAXIMIZED: Self = Self(1 << 9);
    /// Layered window (Windows: `WS_EX_LAYERED` + `UpdateLayeredWindow`
    /// per-pixel alpha composition — the desktop-pet channel; the frame is
    /// updated via `Window::update_layered` instead of the swapchain).
    /// 分层窗（Windows：WS_EX_LAYERED + UpdateLayeredWindow 逐像素 alpha
    /// 合成——桌宠通道；帧经 `Window::update_layered` 上屏而非交换链）。
    pub const LAYERED: Self = Self(1 << 10);
    // Bits 1<<11 and above are reserved.
    // 1<<11 以上预留。

    /// Combine arbitrary flag bits.
    /// 组合任意标志位。
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// The raw bit pattern.
    /// 原始位。
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Build from raw bits (unknown bits are kept as-is; backends ignore
    /// what they don't support).
    /// 由原始位构造（未知的位原样保留，后端忽略）。
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// Whether this set contains `flag`.
    /// 是否含某位。
    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 == flag.0
    }
}

impl std::ops::BitOr for WindowFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for WindowFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Default for WindowFlags {
    /// Default = RESIZABLE (same as SDL: resizable by default).
    /// 默认 = RESIZABLE（SDL 同款：默认可调大小）。
    fn default() -> Self {
        Self::RESIZABLE
    }
}
