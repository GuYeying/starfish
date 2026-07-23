use std::ffi::c_void;
use sdl3_sys::{rect::SDL_Point, video::{SDL_HitTestResult, SDL_Window}};



/// 点击测试模式
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTestMode {
    /// 关闭自定义点击测试，使用系统默认
    Disabled,
    /// 普通点击（常规游戏窗口）
    Normal,
    /// 全域可拖拽（无边框窗口）
    Draggable,
    /// 边缘可缩放（自定义无边框窗口）
    ResizableEdges,
}



//每个unsafe extern "C" fn都是不同类型，所以需要一个统一类型
/// HitTest 回调函数指针类型
pub type HitTestCb = unsafe extern "C" fn(
    *mut SDL_Window,
    *const SDL_Point,
    *mut c_void
) -> SDL_HitTestResult;



/// 1. 默认规则：所有区域正常响应点击（标准游戏主窗口）
pub unsafe extern "C" fn hit_test_normal(
    _win: *mut SDL_Window,
    _area: *const SDL_Point,
    _data: *mut c_void,
) -> SDL_HitTestResult {
    SDL_HitTestResult::NORMAL
}

/// 2. 全区域可拖拽（无边框窗口模拟标题栏拖拽）
pub unsafe extern "C" fn hit_test_draggable(
    _win: *mut SDL_Window,
    _area: *const SDL_Point,
    _data: *mut c_void,
) -> SDL_HitTestResult {
    SDL_HitTestResult::DRAGGABLE
}

/// 3. 边缘检测：窗口四周边缘支持缩放，内部正常点击（自定义无边框窗口）
pub unsafe extern "C" fn hit_test_resize_edges(
    _win: *mut SDL_Window,
    area: *const SDL_Point,
    _data: *mut c_void,
) -> SDL_HitTestResult {
    const EDGE_THRESHOLD: i32 = 8;
    let pt = unsafe { &*area };

    let (win_w, win_h) = {
        let mut w = 0;
        let mut h = 0;
        unsafe { sdl3_sys::video::SDL_GetWindowSize(_win, &mut w, &mut h) };
        (w, h)
    };

    // 四角
    if pt.x < EDGE_THRESHOLD && pt.y < EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_TOPLEFT;
    }
    if pt.x > win_w - EDGE_THRESHOLD && pt.y < EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_TOPRIGHT;
    }
    if pt.x < EDGE_THRESHOLD && pt.y > win_h - EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_BOTTOMLEFT;
    }
    if pt.x > win_w - EDGE_THRESHOLD && pt.y > win_h - EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_BOTTOMRIGHT;
    }

    // 四边
    if pt.y < EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_TOP;
    }
    if pt.y > win_h - EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_BOTTOM;
    }
    if pt.x < EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_LEFT;
    }
    if pt.x > win_w - EDGE_THRESHOLD {
        return SDL_HitTestResult::RESIZE_RIGHT;
    }

    // 窗口内部
    SDL_HitTestResult::NORMAL
}
