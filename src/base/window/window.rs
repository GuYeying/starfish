use std::{ffi::{NulError, c_float}};
use sdl3::{Error, IntegerOrSdlError,video::{Display, Window as SdlWindow, WindowBuildError, WindowFlags, WindowPos}};
use sdl3_sys::{
    mouse::{SDL_CursorVisible, SDL_HideCursor, SDL_SetWindowRelativeMouseMode, SDL_ShowCursor}, video::*
};

use crate::base::subsystem::VideoSubsystem;

use super::{
    HitTestMode, 
    hit_test::{
        HitTestCb, hit_test_draggable, hit_test_normal, hit_test_resize_edges}
};


/// 游戏窗口封装，基于 SDL3 窗口，适配 wgpu + 全量游戏运行时接口
#[derive(Clone)]
pub struct Window {
    inner: SdlWindow,
}


//SDL_WINDOW_HIGH_PIXEL_DENSITY  无法运行时设置
impl Window {
    /// 创建新窗口
    pub fn new(
        video: &VideoSubsystem,
        title: &str,
        size: (u32, u32),
        flags: WindowFlags,
    ) -> Result<Self, WindowBuildError> {
        let window = video.window(title, size.0.max(1), size.1.max(1))
            .set_flags(flags)
            .build()?;
        Ok(Self { inner: window })
    }

    pub fn inner(&self)->&SdlWindow{
        &self.inner
    }

    /// 是否启用高像素密度（SDL3：创建时 flag 决定，运行时只读）
    pub fn high_pixel_density(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_HIGH_PIXEL_DENSITY) != 0
    }

    
    // -------------------------------------------------------------------------
    // 1. 窗口尺寸 & 缩放控制（菜单/对局核心）
    // -------------------------------------------------------------------------

    /// 设置窗口是否可拖拽缩放
    pub fn set_resizable(&self, resizable: bool) {
        unsafe {
            SDL_SetWindowResizable(self.inner.raw(), resizable);
        }
    }

    /// 获取当前窗口是否允许缩放
    pub fn is_resizable(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_RESIZABLE) != 0
    }

    /// 设置窗口逻辑尺寸
    pub fn set_size(&mut self, width: u32, height: u32)-> Result<(), IntegerOrSdlError> {
        self.inner.set_size(width, height)
    }

    /// 获取窗口逻辑尺寸
    pub fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// 获取窗口物理像素尺寸（wgpu 渲染、高分屏必用）
    pub fn pixel_size(&self) -> (u32, u32) {
        self.inner.size_in_pixels()
    }

    /// 获取窗口DPI缩放系数（逻辑坐标 ↔ 像素坐标换算）
    pub fn dpi_scale(&self) -> f32 {
        unsafe { SDL_GetWindowDisplayScale(self.inner.raw()) }
    }

    // -------------------------------------------------------------------------
    // 2. 全屏 / 无边框 模式切换（游戏核心窗口模式）
    // -------------------------------------------------------------------------

    /// 切换独占全屏 / 窗口模式
    pub fn set_fullscreen(&mut self, fullscreen: bool)-> Result<(), Error> {
        self.inner.set_fullscreen(fullscreen)
    }

    /// 判断当前是否为独占全屏
    pub fn is_fullscreen(&self) -> bool {
        
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_FULLSCREEN) != 0
    }

    /// 设置无边框窗口（伪全屏首选）
    pub fn set_borderless(&self, borderless: bool) {
        
        unsafe {
            SDL_SetWindowBordered(self.inner.raw(), !borderless);
        }
    }

    /// 判断当前是否为无边框
    pub fn is_borderless(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_BORDERLESS) != 0
    }

    // -------------------------------------------------------------------------
    // 3. 鼠标控制（FPS/3D 游戏必备）
    // -------------------------------------------------------------------------

    /// 锁定鼠标在窗口内
    pub fn set_mouse_grabbed(&mut self, grab: bool) ->bool {
        self.inner.set_mouse_grab(grab)

    }

    /// 判断鼠标是否被窗口锁定
    pub fn is_mouse_grabbed(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_MOUSE_GRABBED) != 0
    }

    /// 开启/关闭鼠标相对运动模式（视角旋转核心）
    pub fn set_mouse_relative(&mut self, enable: bool) {
        unsafe {
            SDL_SetWindowRelativeMouseMode(self.inner.raw(), enable);
        }
    }

    /// 判断是否开启鼠标相对模式
    pub fn is_mouse_relative(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_MOUSE_RELATIVE_MODE) != 0
    }

    /// 显示/隐藏系统鼠标光标（游戏内常用）
    pub fn set_cursor_visible(&self, visible: bool)->bool {
        unsafe {
            if visible{
                SDL_ShowCursor()
            }else{
                SDL_HideCursor()
            }
        }
    }

    pub fn is_cursor_visible(&self)->bool {
        unsafe {SDL_CursorVisible()}
    }

    // -------------------------------------------------------------------------
    // 4. 窗口显示、状态、置顶、焦点
    // -------------------------------------------------------------------------

    /// 显示/隐藏窗口
    pub fn set_visible(&self, visible: bool)->bool {
        if visible {
            unsafe { SDL_ShowWindow(self.inner.raw()) }
        } else {
            unsafe { SDL_HideWindow(self.inner.raw()) }
        }
    }

    /// 判断窗口是否可见（SDL3：无 HIDDEN 标记即为可见）
    pub fn is_visible(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_HIDDEN) == 0
    }

    /// 窗口置顶（悬浮游戏/调试面板）
    pub fn set_always_on_top(&self, top: bool) {
        unsafe {
            SDL_SetWindowAlwaysOnTop(self.inner.raw(), top);
        }
    }

    /// 判断窗口是否置顶
    pub fn is_always_on_top(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_ALWAYS_ON_TOP) != 0
    }

    /// 最小化窗口
    pub fn minimize(&mut self)->bool {
        self.inner.minimize()
    }

    /// 最大化窗口
    pub fn maximize(&mut self)->bool {
        self.inner.maximize()
    }

    /// 从最小/最大化还原窗口
    pub fn restore(&mut self)->bool {
        self.inner.restore()
    }

    /// 置顶并激活窗口，获取输入焦点
    pub fn raise(&mut self)->bool {
        self.inner.raise()
    }

    /// 判断窗口是否被其他窗口遮挡
    pub fn is_occluded(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_OCCLUDED) != 0
    }

    /// 判断窗口是否最小化
    pub fn is_minimized(&self) -> bool {
        self.inner.is_minimized()
    }

    /// 判断窗口是否最大化
    pub fn is_maximized(&self) -> bool {
        self.inner.is_maximized()
    }

    /// 判断窗口是否拥有输入焦点
    pub fn has_input_focus(&self) -> bool {
        self.inner.has_input_focus()
    }

    // -------------------------------------------------------------------------
    // 5. 键盘独占（全屏游戏防焦点丢失）
    // -------------------------------------------------------------------------

    /// 独占键盘输入
    pub fn set_keyboard_grabbed(&mut self, grab: bool) -> bool {
        self.inner.set_keyboard_grab(grab)
    }

    /// 判断键盘是否被独占
    pub fn is_keyboard_grabbed(&self) -> bool {
        let flags = unsafe { SDL_GetWindowFlags(self.inner.raw()) };
        (flags & SDL_WINDOW_KEYBOARD_GRABBED) != 0
    }

    // -------------------------------------------------------------------------
    // 6. 窗口位置控制（移动窗口、居中）
    // -------------------------------------------------------------------------
    //需要额外封装一个Display。
    pub fn get_display(&self) -> Result<Display, Error>{
        self.inner.get_display()
    }

    /// 将窗口在主显示器居中显示
    pub fn center_on_screen(&mut self) -> Result<(), sdl3::Error> {
        let display = self.get_display()?;
        let bounds = display.get_bounds()?;

        let (win_w, win_h) = {
            let (w, h) = self.size();
            (w as i32, h as i32)
        };
        let x = bounds.x + (bounds.w - win_w) / 2;
        let y = bounds.y + (bounds.h - win_h) / 2;
        self.set_position(WindowPos::from(x), WindowPos::from(y));
        Ok(())
    }

    /// 设置窗口屏幕坐标
    pub fn set_position(&mut self, x: WindowPos, y: WindowPos)->bool {
        self.inner.set_position(x, y)
    }

    /// 获取窗口屏幕坐标
    pub fn position(&self) -> (i32, i32){
        self.inner.position()
    }

    // -------------------------------------------------------------------------
    // 7. 窗口标题（动态修改、显示帧率/版本）
    // -------------------------------------------------------------------------

    /// 设置窗口标题
    pub fn set_title(&mut self, title: &str)-> Result<(), NulError> {
        self.inner.set_title(title)
    }

    /// 获取当前窗口标题
    pub fn title(&self) -> &str {
        self.inner.title()
    }
    // -------------------------------------------------------------------------
    // 8. 窗口透明/不透明度（半透窗口、桌面挂件）
    // -------------------------------------------------------------------------

    /// 设置窗口整体不透明度
    /// opacity: 0.0(完全透明) ~ 1.0(完全不透明)
    pub fn set_opacity(&mut self, opacity: f32) -> Result<(), Error> {
        let val: f32 = opacity.clamp(0.0, 1.0) as c_float;
        self.inner.set_opacity(val)
    }

    /// 获取当前窗口不透明度
    pub fn opacity(&self) -> Result<f32, Error> {
        self.inner.opacity()
    }

    /// 设置透明窗口是否接受鼠标点击
    /// 启用/禁用 自定义点击测试（透明窗口专用）
    /// 禁用 = 使用系统默认点击规则；启用 = 使用自定义回调
    /// 设置窗口点击测试模式
    pub fn set_hit_test_mode(&self, mode: HitTestMode) {
        unsafe {
            let cb: sdl3_sys::video::SDL_HitTest = match mode {
                HitTestMode::Disabled => None,
                HitTestMode::Normal => Some(hit_test_normal as HitTestCb),
                HitTestMode::Draggable => Some(hit_test_draggable as HitTestCb),
                HitTestMode::ResizableEdges => Some(hit_test_resize_edges as HitTestCb),
            };
            //self.inner.set_hit_test(hit_test)
            SDL_SetWindowHitTest(self.inner.raw(), cb, std::ptr::null_mut());
        }
    }

    // -------------------------------------------------------------------------
    // 9. 通用：窗口标记位
    // -------------------------------------------------------------------------

    /// 获取原始SDL窗口标记
    pub fn raw_flags(&self) -> SDL_WindowFlags {
        self.inner.window_flags()
    }
}