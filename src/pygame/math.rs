//! pygame.math 对应物：向量/四元数/矩阵（契约 §五 math 行）。
//!
//! **薄封装策略（与 [`crate::pygame::locals`] 同思路）**：类型本体直通
//! glam（类型化别名，零成本、bytemuck Pod 保持——P2 render 管线的
//! uniform 直传即可用），pygame 名字落在别名与扩展 trait 上：
//!
//! - [`Vector2`]/[`Vector3`]/[`Vector4`] = `glam::Vec2/3/4`——pygame 教程
//!   核心面方法经 [`Vector2Ext`]/[`Vector3Ext`] 补齐（rotate 度数制 /
//!   scale_to_length / polar / slerp 等 glam 没有的部分）；glam 已有的
//!   （length/normalize/dot/cross/lerp/distance…）直接可用不重复；
//! - [`Quaternion`]/[`Mat2`]/[`Mat3`]/[`Mat4`] = `glam::Quat/Mat2/3/4`——
//!   **starfish 扩展**（pygame 无对应，"3D 优先"设计稿决策）：3D 内容与
//!   P2 render 管线（Camera::mvp → Mat4）的地基，接口即 glam 原生面；
//! - 模块级标量函数（clamp/lerp/inv_lerp/remap/smoothstep…）对齐
//!   pygame.math 2.1.3+ 的自由函数面。
//!
//! 坐标系说明（ADR-8）：2D 走 pygame 语义 y 向下。rotate 的数值公式与
//! pygame 一致（x' = x·cosθ − y·sinθ，y' = x·sinθ + y·cosθ），屏幕观感
//! 因 y 向下为视觉顺时针——教程代码数值逐行可读不受影响。
//!
//! Python 专属面不做：elementwise 代理（Rust 运算符天然逐分量）、
//! enable_swizzling（属性膨胀换语法糖，Rust 无此需求）。

use glam::{EulerRot, Quat, Vec2, Vec3};

// ── 类型别名（pygame 名 → glam 本体，零成本直通）─────────────────

/// pygame.math.Vector2（= [`glam::Vec2`]，f32 二维向量）。
pub type Vector2 = Vec2;
/// pygame.math.Vector3（= [`glam::Vec3`]）。
pub type Vector3 = Vec3;
/// pygame.math.Vector4（= [`glam::Vec4`]；pygame 2.1+ 同名）。
pub type Vector4 = glam::Vec4;
/// 四元数（starfish 扩展，= [`glam::Quat`]）：3D 旋转的规范形态。
pub type Quaternion = Quat;
/// 2×2 矩阵（starfish 扩展）：2D 线性变换（rotate 内部即用它）。
pub type Mat2 = glam::Mat2;
/// 3×3 矩阵（starfish 扩展）：2D 齐次变换。
pub type Mat3 = glam::Mat3;
/// 4×4 矩阵（starfish 扩展）：3D 齐次变换，P2 Camera::mvp 的上传类型。
pub type Mat4 = glam::Mat4;

// ── Vector2 扩展（pygame 教程核心面，glam 缺口补齐）──────────────

/// [`Vector2`] 的 pygame 风格方法（`use ...::Vector2Ext as _;` 启用）。
///
/// 命名对齐 pygame.math.Vector2：`rotate` 度数制、`*_ip` 原地变体、
/// `distance_to`/`from_polar`/`as_polar`/`slerp` 全套。角度单位一律**度**
/// （pygame 惯例），`*_rad` 变体收弧度。
pub trait Vector2Ext: Sized {
    /// 旋转副本（度，对齐 pygame `rotate`：数值逆时针，y 向下屏幕观感
    /// 顺时针）。名取 `rotate_degrees`：glam `Vec2::rotate` 已占用该名
    /// （参数为向量，复数乘语义），固有方法优先故让名。
    fn rotate_degrees(self, degrees: f32) -> Self;
    /// 旋转副本（弧度）。
    fn rotate_rad(self, radians: f32) -> Self;
    /// 原地旋转（度），对应 pygame `rotate_ip`。
    fn rotate_degrees_ip(&mut self, degrees: f32);
    /// 原地旋转（弧度）。
    fn rotate_ip_rad(&mut self, radians: f32);
    /// 原地缩放到给定长度（零向量 panic，对齐 pygame 的 ValueError）。
    fn scale_to_length(&mut self, length: f32);
    /// 缩放到给定长度的值语义版（starfish 便捷，零向量 panic）。
    fn scaled_to_length(self, length: f32) -> Self;
    /// 原地归一化（零向量 panic，对齐 pygame）。
    fn normalize_ip(&mut self);
    /// 到 other 的距离（glam `distance` 的 pygame 别名）。
    fn distance_to(self, other: Self) -> f32;
    /// 到 other 的距离平方。
    fn distance_squared_to(self, other: Self) -> f32;
    /// 叉积标量（2D 叉 = z 分量；glam `perp_dot` 的 pygame 别名）。
    fn cross(self, other: Self) -> f32;
    /// 到 other 的带符号夹角（度；逆时针为正，starfish 便捷接口）。
    /// 名取 `angle_to_deg`：glam `Vec2::angle_to` 已占用该名（弧度制），
    /// 固有方法优先故让名。
    fn angle_to_deg(self, other: Self) -> f32;
    /// 极坐标读出 `(长度, 角度°)`，对应 pygame `as_polar`。
    fn as_polar(self) -> (f32, f32);
    /// 极坐标构造 `(长度, 角度°)`，对应 pygame `from_polar`。
    fn from_polar(rho: f32, phi_degrees: f32) -> Self;
    /// 球面插值：最短弧角度插值 + 长度线性插值（对齐 pygame slerp；
    /// 反向平行向量 panic——pygame 同场景抛 ValueError）。
    fn slerp(self, other: Self, t: f32) -> Self;
}

impl Vector2Ext for Vec2 {
    fn rotate_degrees(self, degrees: f32) -> Self {
        self.rotate_rad(degrees.to_radians())
    }
    fn rotate_rad(self, radians: f32) -> Self {
        glam::Mat2::from_angle(radians) * self
    }
    fn rotate_degrees_ip(&mut self, degrees: f32) {
        *self = self.rotate_degrees(degrees);
    }
    fn rotate_ip_rad(&mut self, radians: f32) {
        *self = self.rotate_rad(radians);
    }
    fn scale_to_length(&mut self, length: f32) {
        let len = self.length();
        assert!(len > 0.0, "scale_to_length: 零向量不可缩放（对齐 pygame ValueError）");
        *self *= length / len;
    }
    fn scaled_to_length(self, length: f32) -> Self {
        let mut v = self;
        v.scale_to_length(length);
        v
    }
    fn normalize_ip(&mut self) {
        let len = self.length();
        assert!(len > 0.0, "normalize_ip: 零向量不可归一化（对齐 pygame ValueError）");
        *self /= len;
    }
    fn distance_to(self, other: Self) -> f32 {
        self.distance(other)
    }
    fn distance_squared_to(self, other: Self) -> f32 {
        self.distance_squared(other)
    }
    fn cross(self, other: Self) -> f32 {
        self.perp_dot(other)
    }
    fn angle_to_deg(self, other: Self) -> f32 {
        self.perp_dot(other).atan2(self.dot(other)).to_degrees()
    }
    fn as_polar(self) -> (f32, f32) {
        (self.length(), self.to_angle().to_degrees())
    }
    fn from_polar(rho: f32, phi_degrees: f32) -> Self {
        Vec2::from_angle(phi_degrees.to_radians()) * rho
    }
    fn slerp(self, other: Self, t: f32) -> Self {
        let (la, lb) = (self.length(), other.length());
        if la <= f32::EPSILON || lb <= f32::EPSILON {
            return self.lerp(other, t); // 零向量：确定性退化为线性
        }
        let start = self.to_angle();
        let mut delta = (other.to_angle() - start).rem_euclid(std::f32::consts::TAU);
        if delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU; // 收敛到最短弧 (-π, π]
        }
        assert!(
            (delta.abs() - std::f32::consts::PI).abs() > 1e-5,
            "slerp: 反向平行向量无定义（对齐 pygame ValueError）"
        );
        Vec2::from_angle(start + delta * t) * (la + (lb - la) * t)
    }
}

// ── Vector3 扩展 ─────────────────────────────────────────────────

/// [`Vector3`] 的 pygame 风格方法（`use ...::Vector3Ext as _;` 启用）。
///
/// dot/cross/length/normalize/lerp/slerp/reflect 等 glam 已有，直用；
/// 这里只补 pygame 命名缺口与度数制旋转。旋转方向 = glam 右手系
/// 轴角（与 pygame Vector3.rotate 的 Rodrigues 公式数值一致）。
pub trait Vector3Ext: Sized {
    /// 绕 axis 旋转副本（度；axis 为零向量 panic，对齐 pygame）。
    fn rotate(self, axis: Self, degrees: f32) -> Self;
    /// 绕 axis 旋转副本（弧度）。
    fn rotate_rad(self, axis: Self, radians: f32) -> Self;
    /// 到 other 的距离。
    fn distance_to(self, other: Self) -> f32;
    /// 到 other 的距离平方。
    fn distance_squared_to(self, other: Self) -> f32;
    /// 原地归一化（零向量 panic，对齐 pygame）。
    fn normalize_ip(&mut self);
}

impl Vector3Ext for Vec3 {
    fn rotate(self, axis: Self, degrees: f32) -> Self {
        self.rotate_rad(axis, degrees.to_radians())
    }
    fn rotate_rad(self, axis: Self, radians: f32) -> Self {
        assert!(axis.length() > 1e-6, "rotate: 零轴不可旋转（对齐 pygame ValueError）");
        Quat::from_axis_angle(axis.normalize(), radians) * self
    }
    fn distance_to(self, other: Self) -> f32 {
        self.distance(other)
    }
    fn distance_squared_to(self, other: Self) -> f32 {
        self.distance_squared(other)
    }
    fn normalize_ip(&mut self) {
        let len = self.length();
        assert!(len > 0.0, "normalize_ip: 零向量不可归一化（对齐 pygame ValueError）");
        *self /= len;
    }
}

// ── Quaternion 扩展（starfish 扩展面的度数便捷）──────────────────

/// [`Quaternion`] 的度数制便捷构造/读出（glam 原生为弧度制）。
pub trait QuaternionExt: Sized {
    /// 轴角构造（度），等价 `Quat::from_axis_angle(axis, rad)`。
    fn from_axis_angle_deg(axis: Vec3, degrees: f32) -> Self;
    /// 欧拉角构造（度，X→Y→Z 分量序），等价 `Quat::from_euler(rad…)`。
    fn from_euler_deg(x: f32, y: f32, z: f32) -> Self;
    /// 读出轴角（度），等价 `Quat::to_axis_angle` 的度数版。
    fn to_axis_angle_deg(self) -> (Vec3, f32);
}

impl QuaternionExt for Quat {
    fn from_axis_angle_deg(axis: Vec3, degrees: f32) -> Self {
        Quat::from_axis_angle(axis, degrees.to_radians())
    }
    fn from_euler_deg(x: f32, y: f32, z: f32) -> Self {
        Quat::from_euler(EulerRot::XYZ, x.to_radians(), y.to_radians(), z.to_radians())
    }
    fn to_axis_angle_deg(self) -> (Vec3, f32) {
        let (axis, angle) = self.to_axis_angle();
        (axis, angle.to_degrees())
    }
}

// ── 模块级标量函数（对齐 pygame.math 2.1.3+ 自由函数面）──────────

/// 线性插值（t 无界时外推）。
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// 夹到 `[min, max]`。
pub fn clamp(x: f32, min: f32, max: f32) -> f32 {
    x.max(min).min(max)
}

/// 逆线性插值：v 在 `[a, b]` 中的比例 t（除零保护：a≈b 时返回 0）。
pub fn inv_lerp(a: f32, b: f32, v: f32) -> f32 {
    let d = b - a;
    if d.abs() <= f32::EPSILON { 0.0 } else { (v - a) / d }
}

/// 区间重映射：v 从 `[in_a, in_b]` 线性映射到 `[out_a, out_b]`。
pub fn remap(in_a: f32, in_b: f32, out_a: f32, out_b: f32, v: f32) -> f32 {
    lerp(out_a, out_b, inv_lerp(in_a, in_b, v))
}

/// 平滑阶跃（Hermite 3t²−2t³），对应 pygame.math.smoothstep。
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp(inv_lerp(e0, e1, x), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 更平滑阶跃（六次），对应 pygame.math.smootherstep。
pub fn smootherstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp(inv_lerp(e0, e1, x), 0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

// ── 测试（零平台依赖，P1 验收口径：单元测试纯逻辑）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn close2(a: Vec2, b: Vec2) -> bool {
        close(a.x, b.x) && close(a.y, b.y)
    }

    #[test]
    fn vector2_rotate_degrees_parity() {
        // pygame: Vector2(1,0).rotate(90) == (0,1)
        assert!(close2(Vec2::X.rotate_degrees(90.0), Vec2::Y));
        assert!(close2(Vec2::new(3.0, 0.0).rotate_degrees(90.0), Vec2::new(0.0, 3.0)));
        // 弧度同解
        assert!(close2(Vec2::X.rotate_rad(std::f32::consts::FRAC_PI_2), Vec2::Y));
        // 旋转保长
        let v = Vec2::new(3.0, -4.0).rotate_degrees(37.0);
        assert!(close(v.length(), 5.0));
        // _ip 变体
        let mut w = Vec2::X;
        w.rotate_degrees_ip(90.0);
        assert!(close2(w, Vec2::Y));
    }

    #[test]
    fn vector2_scale_and_normalize() {
        let mut v = Vec2::new(3.0, 4.0);
        v.scale_to_length(10.0);
        assert!(close2(v, Vec2::new(6.0, 8.0)));
        assert!(close(v.scaled_to_length(5.0).length(), 5.0));
        let mut u = Vec2::new(0.0, 2.0);
        u.normalize_ip();
        assert!(close2(u, Vec2::Y));
    }

    #[test]
    fn vector2_scale_zero_panics() {
        let mut z = Vec2::ZERO;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            z.scale_to_length(1.0);
        }));
        assert!(r.is_err());
    }

    #[test]
    fn vector2_polar_and_angles() {
        let v = Vec2::from_polar(5.0, 30.0);
        assert!(close(v.length(), 5.0));
        let (rho, phi) = v.as_polar();
        assert!(close(rho, 5.0) && close(phi, 30.0));
        // 带符号夹角：X → Y 逆时针 90°
        assert!(close(Vec2::X.angle_to_deg(Vec2::Y), 90.0));
        assert!(close(Vec2::Y.angle_to_deg(Vec2::X), -90.0));
        // 2D 叉 = z 分量
        assert!(close(Vec2::X.cross(Vec2::Y), 1.0));
        assert!(close(Vec2::X.distance_to(Vec2::Y), std::f32::consts::SQRT_2));
    }

    #[test]
    fn vector2_slerp_shortest_arc() {
        let mid = Vec2::X.slerp(Vec2::Y, 0.5);
        assert!(close2(mid, Vec2::from_angle(std::f32::consts::FRAC_PI_4)));
        // 长度线性插值
        let m = Vec2::X.slerp(Vec2::Y * 3.0, 0.5);
        assert!(close(m.length(), 2.0));
        // 平行同向退化为线性
        let s = Vec2::X.slerp(Vec2::X * 2.0, 0.25);
        assert!(close2(s, Vec2::new(1.25, 0.0)));
    }

    #[test]
    fn vector3_rotate_axis_angle() {
        // 绕 Z 轴 90°：X → Y（与 pygame Rodrigues 公式数值一致）
        assert!(close2(Vec3::X.rotate(Vec3::Z, 90.0).truncate(), Vec2::Y));
        let v = Vec3::new(1.0, 2.0, 3.0).rotate(Vec3::Y, 45.0);
        assert!(close(v.length(), (1.0f32 * 1.0 + 2.0 * 2.0 + 3.0 * 3.0).sqrt()));
    }

    #[test]
    fn quaternion_axis_angle_roundtrip() {
        let q = Quat::from_axis_angle_deg(Vec3::Z, 90.0);
        let r = q * Vec3::X;
        assert!(close(r.x, 0.0) && close(r.y, 1.0) && close(r.z, 0.0));
        let (axis, angle) = q.to_axis_angle_deg();
        assert!(close(angle, 90.0));
        assert!(close(axis.z, 1.0));
    }

    #[test]
    fn matrix_aliases_align_glam() {
        // 别名即 glam 本体：Pod 直通 uniform（P2 Camera::mvp 的上传类型）
        let m: Mat4 = Mat4::IDENTITY;
        assert_eq!(m, glam::Mat4::IDENTITY);
        let p = m.transform_point3(Vec3::ONE);
        assert!(close(p.x, 1.0));
        // Mat2 参与 2D 旋转同路径
        assert!(close2(Mat2::from_angle(std::f32::consts::FRAC_PI_2) * Vec2::X, Vec2::Y));
    }

    #[test]
    fn scalar_functions() {
        assert!(close(lerp(0.0, 10.0, 0.3), 3.0));
        assert!(close(clamp(5.0, 0.0, 3.0), 3.0));
        assert!(close(inv_lerp(0.0, 10.0, 2.5), 0.25));
        assert!(close(remap(0.0, 10.0, 100.0, 200.0, 5.0), 150.0));
        assert!(close(smoothstep(0.0, 1.0, 0.5), 0.5));
        assert!(close(smootherstep(0.0, 1.0, 0.0), 0.0));
        assert!(close(smootherstep(0.0, 1.0, 1.0), 1.0));
        assert!(close(inv_lerp(1.0, 1.0, 0.5), 0.0)); // 除零保护
    }
}
