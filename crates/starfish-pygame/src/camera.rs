//! pygamest Camera 类型——**显式传入，杜绝兜底**（域分层 v3：2026-11-19）。
//!
//! 单一职责：letterbox 数学只在 Camera 内部（`Fit::fit` 的唯一调用点
//! 在渲染呈现侧）。投影（ortho/perspective）+ 视图 + letterbox 三合一
//! 组装 MVP，同时承载坐标映射（逻辑 ↔ 物理）与裁剪区换算。
//!
//! 定约（2026-11-10 相机系统重构；2026-11-19 域分层 v3 升级）：
//! - **显式唯一**：Camera 是唯一相机类型（旧 render::camera::Camera /
//!   CameraKind Auto2d/Auto3d 兜底已退役）——`set_camera` 一次设置
//!   跨会话/resize/换画布缓存；未显式 = 无相机，呈现走 NDC 恒等
//!   （[`Self::ndc`]）默认域。
//! - **两段式**：相机恒为 world→画布基底（`mvp()` = fit_ndc ×
//!   projection × view，构造入参 logical=画布意图）；画布→表面
//!   letterbox 由 Screen::present_fit 独立现算，消费点统一组合。
//! - **单一职责**：letterbox 数学只在 Camera 内部，target.rs 不再散布。

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

    /// NDC 恒等相机（**无显式相机的默认域具象**——wgpu 裁剪空间直通）：
    /// mvp() = identity，fit 恒等。present_mvp 无显式相机时返回本矩阵
    /// ——raw 顶点 NDC 直通 + present_fit 单段 letterbox。
    pub fn ndc() -> Self {
        Self::build(
            Mat4::IDENTITY,
            glam::camera::rh::proj::directx::orthographic(-1.0, 1.0, -1.0, 1.0, 0.0, 1.0),
            Projection::Ortho,
            (1, 1),
            (1, 1),
        )
    }

    /// NDC 恒等 MVP（静态便捷口——present_mvp 无显式相机的默认域）
    pub fn ndc_mvp() -> Mat4 {
        Self::ndc().mvp()
    }

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

    /// 物理点 → 逻辑点（逆向：事件坐标进脚本坐标系）
    pub fn physical_to_logical(&self, x: f64, y: f64) -> (f64, f64) {
        self.fit.physical_to_logical(x, y)
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

    #[test]
    fn perspective_floor_plane_ndc_probe() {
        // gfx3d 地板 plane 缺渲染取证（水平 quad y 恒定透视不渲染挂账）：
        // 透视相机（55°/画布 800×600/near 0.1 far 100）+ 场景平移
        // (0,1.6,-7)——水平 quad 顶点 y=-1、x/z ±4 的 NDC 输出。y 应随
        // z_dist 变化（投影为梯形非水平线）；z NDC 须在 [0,1]（directx）。
        let cam = Camera::perspective(55.0, (800, 600), (800, 600), 0.1, 100.0)
            .with_view(Mat4::from_translation(glam::Vec3::new(0.0, 1.6, -7.0)));
        let m = cam.mvp();
        let proj = |x: f32, y: f32, z: f32| -> (f32, f32, f32) {
            let p = m * Vec4::new(x, y, z, 1.0);
            (p.x / p.w, p.y / p.w, p.z / p.w)
        };
        let near = proj(-4.0, -1.0, -4.0);
        let far = proj(4.0, -1.0, 4.0);
        println!("plane NDC near={:?} far={:?}", near, far);
        assert!(near.1.is_finite() && far.1.is_finite());
        // y NDC 随 z_dist 变化（梯形非水平线）——全等 = 投影 y 行退化
        assert!(
            (near.1 - far.1).abs() > 1e-3,
            "y NDC 退化：水平面投影成线（{} vs {}）",
            near.1,
            far.1
        );
        assert!(near.1.abs() <= 1.0 && far.1.abs() <= 1.0, "y NDC 出界");
        assert!(
            (0.0..=1.0).contains(&near.2) && (0.0..=1.0).contains(&far.2),
            "z NDC 出界：{:?} {:?}",
            near.2,
            far.2
        );
    }
}
