
/// 二维线段，两点构成，用于 clipline 裁剪线段
#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub start: (i32, i32),
    pub end: (i32, i32),
}

impl Line {
    pub fn new(sx: i32, sy: i32, ex: i32, ey: i32) -> Self {
        Self {
            start: (sx, sy),
            end: (ex, ey),
        }
    }
}