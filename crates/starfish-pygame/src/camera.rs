//! pygamest Camera 类型——**显式传入，杜绝兜底**。
//!
//! 单一职责：letterbox 数学只在 Camera 内部（`Fit::fit` 的唯一调用点
//! 在渲染呈现侧）。投影（ortho/perspective）+ 视图 + letterbox 三合一
//! 组装 MVP，同时承载坐标映射（逻辑 ↔ 物理）与裁剪区换算。
//!
//! 定约（2026-11-10 相机系统重构）：
//! - **显式优于隐式**：渲染会话必须显式传入 `Camera`，不再有 Auto2d/
//!   Auto3d 兜底自装。
//! - **单一职责**：letterbox 数学只在 Camera 内部，target.rs 不再散布。
//! - **平行于 starfish-io::read 的 async 范式**：接口恒定，平台差异
//!   （物理/逻辑尺寸关系）压在 Camera 内部。

use glam::Mat4;
use starfish_std::letterbox::Fit;

/// 2D/3D 投影种类（Camera 构造时定型，替代旧 CameraKind 兜底枚举）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// 正交（2D）：中心原点、y 向上；视野 = 逻辑画布全域
    Ortho,
    /// 透视（3D）：fov 度数制、右手系、near/far 裁剪
    Perspective {
        fov_y_degrees: f32,
        near: f32,
        far: f32,
    },
}

/// 显式相机对象——**渲染会话必传**，没有默认兜底。
///
/// 职责全景：
/// - MVP 组装：projection × view × letterbox_fit（NDC 域仿射）
/// - 坐标映射：逻辑 ↔ 物理（事件逆映射 / 触摸注入正向）
/// - 裁剪区换算：逻辑裁剪矩形 → 物理像素裁剪
///
/// 构造：`Camera::ortho(logical, physical)` 或
/// `Camera::perspective(fov, logical, physical, near, far)`
/// ——物理/逻辑尺寸关系（letterbox）在构造时锁定，resize 走 `resize()`。
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    /// 视图矩阵（2D 恒 identity；3D = look_at 等）
    view: Mat4,
    /// 投影矩阵（ortho / perspective）
    projection: Mat4,
    /// 投影种类（诊断用）
    projection_kind: Projection,
    /// 逻辑画布尺寸（set_mode 语义尺寸）
    logical_size: (u32, u32),
    /// 物理视口尺寸（窗口 / 原生表面实际像素）
    physical_size: (u32, u32),
    /// letterbox 适配结果（构造时锁定；resize 时重算）
    fit: Fit,
}

impl Camera {
    // ── 构造 ─────────────────────────────────────────────────────

    /// 2D 正交相机（中心原点、y 向上；逻辑画布全域视野）
    pub fn ortho(logical: (u32, u32), physical: (u32, u32)) -> Self {
        let (lw, lh) = (logical.0.max(1) as f32, logical.1.max(1) as f32);
        let (hw, hh) = (lw * 0.5, lh * 0.5);
        Self::build(
            Mat4::IDENTITY,
            glam::camera::rh::proj::directx::orthographic(-hw, hw, -hh, hh, 0.0, 1.0),
            Projection::Ortho,
            logical,
            physical,
        )
    }

    /// 3D 透视相机（fov 度数制 y 向视野；右手系）
    pub fn perspective(
        fov_y_degrees: f32,
        logical: (u32, u32),
        physical: (u32, u32),
        near: f32,
        far: f32,
    ) -> Self {
        let aspect = (logical.0.max(1) as f32 / logical.1.max(1) as f32).max(1e-4);
        let projection = glam::camera::rh::proj::directx::perspective(
            fov_y_degrees.to_radians(),
            aspect,
            near,
            far,
        );
        Self::build(
            Mat4::IDENTITY,
            projection,
            Projection::Perspective {
                fov_y_degrees,
                near,
                far,
            },
            logical,
            physical,
        )
    }

    fn build(
        view: Mat4,
        projection: Mat4,
        kind: Projection,
        logical: (u32, u32),
        physical: (u32, u32),
    ) -> Self {
        let fit = Fit::fit(
            physical.0 as f64,
            physical.1 as f64,
            logical.0 as f64,
            logical.1 as f64,
        );
        Self {
            view,
            projection,
            projection_kind: kind,
            logical_size: logical,
            physical_size: physical,
            fit,
        }
    }

    // ── 视图 ─────────────────────────────────────────────────────

    /// 替换视图矩阵（3D 位姿：look_at 等；2D 恒 identity）
    pub fn with_view(mut self, view: Mat4) -> Self {
        self.view = view;
        self
    }

    // ── 核心：MVP（letterbox 内聚——唯一出口）───────────────────

    /// 纯投影 × 视图（不含 letterbox——raw_render 自管相机的原始量）
    pub fn raw_mvp(&self) -> Mat4 {
        self.projection * self.view
    }

    /// **letterbox 内聚 MVP**（唯一上传量）：NDC 域 letterbox 仿射 ×
    /// projection × view。Auto2d 下与旧"正交(物理)×平移×缩放"逐位等价；
    /// 3D 纵横比按逻辑画布。这是 pack 段级 MVP 的唯一来源。
    pub fn mvp(&self) -> Mat4 {
        self.fit_ndc() * self.raw_mvp()
    }

    /// NDC 域 letterbox 仿射（把整幅 NDC 收进画布子矩形）
    pub(crate) fn fit_ndc(&self) -> Mat4 {
        let (pw, ph) = (
            self.physical_size.0.max(1) as f32,
            self.physical_size.1.max(1) as f32,
        );
        let (lw, lh) = (
            self.logical_size.0.max(1) as f32,
            self.logical_size.1.max(1) as f32,
        );
        let sx = (lw * self.fit.scale as f32) / pw;
        let sy = (lh * self.fit.scale as f32) / ph;
        let bx = 2.0 * self.fit.offset_x as f32 / pw - 1.0 + sx;
        let by = 1.0 - 2.0 * self.fit.offset_y as f32 / ph - sy;
        glam::Mat4::from_translation(glam::Vec3::new(bx, by, 0.0))
            * glam::Mat4::from_scale(glam::Vec3::new(sx, sy, 1.0))
    }

    // ── 坐标映射（唯一出口——target.rs 不再散布 Fit::fit）────────

    /// 逻辑点 → 物理点（正向：触摸注入 / 绘制映射）
    pub fn logical_to_physical(&self, x: f64, y: f64) -> (f64, f64) {
        self.fit.logical_to_physical(x, y)
    }

    /// 物理点 → 逻辑点（逆向：事件坐标进脚本坐标系）
    pub fn physical_to_logical(&self, x: f64, y: f64) -> (f64, f64) {
        self.fit.physical_to_logical(x, y)
    }

    /// 逻辑裁剪矩形 → 物理像素裁剪（scissor 用）
    pub fn map_scissor(&self, rect: [i32; 4]) -> [i32; 4] {
        let (pw, ph) = (
            self.physical_size.0.max(1) as i32,
            self.physical_size.1.max(1) as i32,
        );
        let (mx, my) = self.fit.logical_to_physical(rect[0] as f64, rect[1] as f64);
        let w = (rect[2] as f32 * self.fit.scale as f32) as i32;
        let h = (rect[3] as f32 * self.fit.scale as f32) as i32;
        [mx as i32, my as i32, w.min(pw), h.min(ph)]
    }

    // ── 尺寸/属性访问器 ──────────────────────────────────────────

    /// 逻辑画布尺寸
    pub fn logical_size(&self) -> (u32, u32) {
        self.logical_size
    }

    /// 物理视口尺寸
    pub fn physical_size(&self) -> (u32, u32) {
        self.physical_size
    }

    /// letterbox 适配结果（诊断/呈现映射用）
    pub fn fit(&self) -> Fit {
        self.fit
    }

    /// 投影种类
    pub fn projection_kind(&self) -> &Projection {
        &self.projection_kind
    }

    // ── resize ────────────────────────────────────────────────────

    /// 物理视口尺寸变化（交换链 resize 时调用——重算 letterbox fit）
    pub fn resize(&mut self, physical: (u32, u32)) {
        self.physical_size = physical;
        self.fit = Fit::fit(
            physical.0 as f64,
            physical.1 as f64,
            self.logical_size.0 as f64,
            self.logical_size.1 as f64,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn ndc(m: Mat4, x: f32, y: f32) -> (f32, f32) {
        let p = m * Vec4::new(x, y, 0.0, 1.0);
        (p.x / p.w, p.y / p.w)
    }

    #[test]
    fn ortho_2d_desktop_identity() {
        // 桌面逻辑==物理 → letterbox 恒等 → MVP = 纯正交
        let cam = Camera::ortho((800, 600), (800, 600));
        let m = cam.mvp();
        let (x, y) = ndc(m, 0.0, 0.0);
        assert!(x.abs() < 1e-5 && y.abs() < 1e-5);
        let (x, y) = ndc(m, 400.0, 300.0);
        assert!((x - 1.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn ortho_letterbox_scales_and_centers() {
        // 逻辑 800×600 → 物理 1280×720：scale = min(1280/800, 720/600) = 1.2
        // 画布 NDC 区域：x ∈ [-1+bx, -1+bx+2sx]，等比收进画布子矩形
        let cam = Camera::ortho((800, 600), (1280, 720));
        let m = cam.mvp();
        // 逻辑中心 (0,0) → 物理中心 NDC (0,0)
        let (x, y) = ndc(m, 0.0, 0.0);
        assert!(x.abs() < 1e-4 && y.abs() < 1e-4, "center: ({x},{y})");
        // 画布右边界 (400,0) → NDC x = bx+sx ≈ 1-(黑边比)
        let (x, _) = ndc(m, 400.0, 0.0);
        assert!(x < 1.0 && x > 0.6, "right edge x={x}");
    }

    #[test]
    fn perspective_3d_aspect_from_logical() {
        let cam = Camera::perspective(60.0, (800, 600), (800, 600), 0.1, 100.0);
        let m = cam.mvp();
        assert!(m.to_cols_array().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn coordinate_mapping_roundtrip() {
        let cam = Camera::ortho((800, 600), (1280, 720));
        let (px, py) = cam.logical_to_physical(100.0, 50.0);
        let (lx, ly) = cam.physical_to_logical(px, py);
        assert!((lx - 100.0).abs() < 1e-6 && (ly - 50.0).abs() < 1e-6);
    }

    #[test]
    fn resize_recomputes_fit() {
        let mut cam = Camera::ortho((800, 600), (800, 600));
        assert!((cam.fit().scale - 1.0).abs() < 1e-6);
        cam.resize((1600, 900));
        // scale = min(1600/800, 900/600) = 1.5
        assert!((cam.fit().scale - 1.5).abs() < 1e-6);
    }

    #[test]
    fn matmul_composes_transforms() {
        let a = Camera::ortho((800, 600), (800, 600));
        let _ = a.mvp(); // 确保 letterbox 内聚不 panic
    }
}
// 追加：Screen 集成方法（阶段一 1.2 集成口——Screen.camera 显式设置）
impl Camera {
    /// 从 RenderWish 构造（Screen.refresh_camera 内部使用——
    /// dim3d 旗标决定 ortho/perspective，物理尺寸取自 surface）
    pub(crate) fn from_wish(
        wish: &crate::render::RenderWish,
        logical: (u32, u32),
        physical: (u32, u32),
    ) -> Self {
        if wish.dim3d {
            Self::perspective(wish.fov_deg, logical, physical, wish.near, wish.far)
        } else {
            Self::ortho(logical, physical)
        }
    }
}
