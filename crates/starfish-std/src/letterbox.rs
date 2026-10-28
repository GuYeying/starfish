//! 等比 letterbox 适配数学（fit-center）：**纯计算，零平台依赖**——
//! 窗口/呈现/输入各层共用的唯一换算源。
//!
//! 归属定约：letterbox 的**应用**在 starfish-pygame 呈现层（pack MVP/
//! 事件逆映射/egui 合成，平台无关——iOS/鸿蒙原生免费继承）；本模块只
//! 收敛**换算公式**（谁用谁调，宿主壳的 Kotlin/ArkTS 版按同公式各自
//! 实现）。避免把呈现语义下沉 std（std 不懂渲染），也避免各层公式
//! 漂移（偏移类事故的根源）。
//!
//! 模型：逻辑画布（lw×lh）等比缩放 s = min(nw/lw, nh/nh) 后居中于
//! 物理视口（nw×nh）——短轴铺满、长轴留黑边、零变形。

/// fit-center 适配结果：等比缩放 + 居中偏移（逻辑 → 物理）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// 等比缩放（短轴比；物理 = 逻辑 × scale + offset）
    pub scale: f64,
    /// 居中偏移（物理系，长轴两侧黑边 = |n - l×scale|/2）
    pub offset_x: f64,
    pub offset_y: f64,
}

impl Fit {
    /// 计算 fit-center（逻辑画布 lw×lh 适配进物理视口 nw×nh；
    /// 逻辑零尺寸 = 恒等回退）
    pub fn fit(native_w: f64, native_h: f64, logical_w: f64, logical_h: f64) -> Self {
        if native_w <= 0.0 || native_h <= 0.0 || logical_w <= 0.0 || logical_h <= 0.0 {
            return Self { scale: 1.0, offset_x: 0.0, offset_y: 0.0 };
        }
        let scale = (native_w / logical_w).min(native_h / logical_h);
        Self {
            scale,
            offset_x: (native_w - logical_w * scale) / 2.0,
            offset_y: (native_h - logical_h * scale) / 2.0,
        }
    }

    /// 逻辑点 → 物理点（触摸注入前的宿主换算用正向；引擎逆映射见
    /// physical_to_logical）
    pub fn logical_to_physical(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.offset_x, y * self.scale + self.offset_y)
    }

    /// 物理点 → 逻辑点（呈现映射的逆变换——事件坐标进入脚本坐标系）
    pub fn physical_to_logical(&self, x: f64, y: f64) -> (f64, f64) {
        if self.scale <= 0.0 {
            return (x, y);
        }
        ((x - self.offset_x) / self.scale, (y - self.offset_y) / self.scale)
    }

    /// 逻辑画布在物理视口中的落位矩形（x, y, w, h——诊断/调试叠加用）
    pub fn logical_rect_in_physical(&self, logical_w: f64, logical_h: f64) -> (f64, f64, f64, f64) {
        (self.offset_x, self.offset_y, logical_w * self.scale, logical_h * self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_aspect_is_identity() {
        let f = Fit::fit(1600.0, 1200.0, 800.0, 600.0);
        assert_eq!(f.scale, 2.0);
        assert_eq!(f.logical_to_physical(0.0, 0.0), (0.0, 0.0));
        assert_eq!(f.logical_to_physical(800.0, 600.0), (1600.0, 1200.0));
    }

    #[test]
    fn shorter_logical_gets_horizontal_bars() {
        // 逻辑 4:3 适配 21:9 视口：等比铺满短轴（高），水平居中留黑边
        let f = Fit::fit(2412.0, 1084.0, 1024.0, 768.0);
        assert!((f.scale - 1084.0 / 768.0).abs() < 1e-9);
        let (ox, oy, w, h) = f.logical_rect_in_physical(1024.0, 768.0);
        assert!((ox - (2412.0 - w) / 2.0).abs() < 1e-9);
        assert!((oy - (1084.0 - h) / 2.0).abs() < 1e-9);
        assert!((h - 1084.0).abs() < 1e-9); // 短轴铺满
    }

    #[test]
    fn physical_to_logical_inverts_logical_to_physical() {
        let f = Fit::fit(2772.0, 1240.0, 800.0, 600.0);
        for (px, py) in [(0.0, 0.0), (400.0, 300.0), (2772.0, 1240.0)] {
            let (lx, ly) = f.logical_to_physical(px, py);
            let (bx, by) = f.physical_to_logical(lx, ly);
            assert!((bx - px).abs() < 1e-9 && (by - py).abs() < 1e-9);
        }
    }

    #[test]
    fn zero_dimensions_fall_back_to_identity() {
        let f = Fit::fit(0.0, 0.0, 800.0, 600.0);
        assert_eq!(f.scale, 1.0);
        assert_eq!(f.logical_to_physical(12.0, 34.0), (12.0, 34.0));
    }
}
