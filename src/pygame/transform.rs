//! pygame.transform 对应物（契约 §五 transform 行 / P5 提前）
//!
//! **GPU 采样实现**（契约定稿）：源纹理 → 顶点变换 quad（连续坐标 +
//! 线性采样）→ 新 Surface——质量优于 CPU 旋转（绑定层设计稿 §4.4
//! 映射表），全程零 CPU 逐像素。
//!
//! 对齐面：`flip_x` / `flip_y` / `scale` / `rotate`（度数制——pygame
//! transform.rotate 同为度）/ `rotozoom`（旋转 + 缩放复合）。差异：
//! smoothscale 与 scale 在本层等同（采样器恒线性；像素风需求 v2 提供
//! nearest 变体位）。
//!
//! ⚠️ 产物 = 新 Surface（源不变）；须在 display 初始化后调用。

use crate::pygame::render::{DrawTarget, Surface, Texture};

/// 水平镜像（uv 翻转，零重采样）
pub fn flip_x(s: &Surface) -> Surface {
    let (w, h) = (s.size().0 as f32, s.size().1 as f32);
    warped(
        s,
        s.size(),
        [[w, 0.0], [0.0, 0.0], [0.0, h], [w, h]],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
}

/// 垂直镜像（uv 翻转，零重采样）
pub fn flip_y(s: &Surface) -> Surface {
    let (w, h) = (s.size().0 as f32, s.size().1 as f32);
    warped(
        s,
        s.size(),
        [[0.0, h], [w, h], [w, 0.0], [0.0, 0.0]],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
}

/// 缩放到指定尺寸（线性采样；对位 `transform.scale`/`smoothscale`）
pub fn scale(s: &Surface, size: (u32, u32)) -> Surface {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    warped(
        s,
        (size.0.max(1), size.1.max(1)),
        [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    )
}

/// 旋转（度数制，逆时针；产物尺寸 = 旋转包围盒，内容居中不裁剪）
pub fn rotate(s: &Surface, angle_degrees: f32) -> Surface {
    let (w, h) = (s.size().0 as f32, s.size().1 as f32);
    let (ow, oh) = rotated_bounds(w, h, angle_degrees);
    rotozoom_corners(s, (ow, oh), angle_degrees, 1.0)
}

/// 旋转 + 缩放复合（对位 `transform.rotozoom`；zoom = 1.0 原尺寸）
pub fn rotozoom(s: &Surface, angle_degrees: f32, zoom: f32) -> Surface {
    let (w, h) = (s.size().0 as f32 * zoom, s.size().1 as f32 * zoom);
    let (ow, oh) = rotated_bounds(w, h, angle_degrees);
    rotozoom_corners(s, (ow, oh), angle_degrees, zoom)
}

// ── 内部 ─────────────────────────────────────────────────────────

/// 旋转 + 缩放的公共执行体：源纹理 → 四角变换 quad → 新 Surface
fn rotozoom_corners(
    s: &Surface,
    out_size: (u32, u32),
    angle_degrees: f32,
    zoom: f32,
) -> Surface {
    let out = Surface::new(out_size);
    let (ow, oh) = (out_size.0 as f32, out_size.1 as f32);
    let (cx, cy) = (ow / 2.0, oh / 2.0);
    let (w, h) = (s.size().0 as f32 * zoom / 2.0, s.size().1 as f32 * zoom / 2.0);
    let (c, sn) = (angle_degrees.to_radians().cos(), angle_degrees.to_radians().sin());
    // 角偏移（TL/TR/BR/BL）→ 旋转 → 平移到产物中心
    let corner = |ox: f32, oy: f32| [cx + ox * c - oy * sn, cy + ox * sn + oy * c];
    let corners = [
        corner(-w, -h),
        corner(w, -h),
        corner(w, h),
        corner(-w, h),
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    warped_corners(&out, s, corners, uvs);
    out
}

/// 翻转/缩放的公共执行体：目标尺寸下四角直绘
fn warped(
    s: &Surface,
    out_size: (u32, u32),
    corners: [[f32; 2]; 4],
    uvs: [[f32; 2]; 4],
) -> Surface {
    let out = Surface::new(out_size);
    warped_corners(&out, s, corners, uvs);
    out
}

/// 会话绘制：源纹理 → 四角 quad → 目标（打包即提交）
fn warped_corners(out: &Surface, s: &Surface, corners: [[f32; 2]; 4], uvs: [[f32; 2]; 4]) {
    let f = out.render();
    f.with_batch(|b| b.push_quad_corners(corners, uvs, [1.0; 4], Some(s.texture())));
    f.end(); // Surface 会话：打包 = 提交
}

/// 旋转包围盒尺寸（纯逻辑）
fn rotated_bounds(w: f32, h: f32, angle_degrees: f32) -> (u32, u32) {
    let (c, sn) = (angle_degrees.to_radians().cos().abs(), angle_degrees.to_radians().sin().abs());
    // ceil 带容差：90° 的 cos 在 f32 下是 6e-9 非 0，直接 ceil 会虚增 1px
    let ceil_eps = |v: f32| (v - 1e-3).ceil().max(1.0) as u32;
    (ceil_eps(w * c + h * sn), ceil_eps(w * sn + h * c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_bounds_math() {
        // 0°/90° 互换宽高
        assert_eq!(rotated_bounds(100.0, 50.0, 0.0), (100, 50));
        assert_eq!(rotated_bounds(100.0, 50.0, 90.0), (50, 100));
        assert_eq!(rotated_bounds(100.0, 50.0, 180.0), (100, 50));
        // 45° = (w+h)/√2
        let (w, h) = rotated_bounds(100.0, 100.0, 45.0);
        assert_eq!((w, h), (142, 142));
        // 负角同幅值
        assert_eq!(rotated_bounds(100.0, 50.0, -90.0), (50, 100));
    }
}
