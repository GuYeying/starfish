//! 2D 形状生成器（纯函数；z = 0，y 向下或向上由相机决定）

use glam::Vec2;

use super::super::ShapeVertex;
use super::Geometry;
use super::tri::triangulate;

/// 矩形（填充）：`min`/`max` 为对角两点
pub fn rect(min: Vec2, max: Vec2, color: [f32; 4]) -> Geometry {
    let v = |p: Vec2| ShapeVertex::new([p.x, p.y, 0.0], color);
    Geometry::indexed(
        vec![v(min), v(Vec2::new(max.x, min.y)), v(max), v(Vec2::new(min.x, max.y))],
        vec![0, 1, 2, 0, 2, 3],
    )
}

/// 矩形描边（LineList，1px）
pub fn rect_outline(min: Vec2, max: Vec2, color: [f32; 4]) -> Geometry {
    let v = |p: Vec2| ShapeVertex::new([p.x, p.y, 0.0], color);
    let (a, b, c, d) = (v(min), v(Vec2::new(max.x, min.y)), v(max), v(Vec2::new(min.x, max.y)));
    // 四条边，每边独立线段（LineList）
    Geometry::indexed(
        vec![a, b, b, c, c, d, d, a],
        (0..8u16).collect(),
    )
}

/// 圆（填充）：扇形三角化，`segments` 为边数（≥3）
pub fn circle(center: Vec2, radius: f32, segments: u32, color: [f32; 4]) -> Geometry {
    let seg = segments.max(3);
    let mut g = Geometry::default();
    g.push([center.x, center.y, 0.0], color);
    for i in 0..=seg {
        let a = std::f32::consts::TAU * i as f32 / seg as f32;
        g.push([center.x + radius * a.cos(), center.y + radius * a.sin(), 0.0], color);
    }
    for i in 1..=seg {
        g.extend_indices(&[0, i as u16, i as u16 + 1]);
    }
    g
}

/// 圆描边（LineList，1px）
pub fn circle_outline(center: Vec2, radius: f32, segments: u32, color: [f32; 4]) -> Geometry {
    let seg = segments.max(3);
    let mut g = Geometry::default();
    for i in 0..seg {
        let a = std::f32::consts::TAU * i as f32 / seg as f32;
        g.push([center.x + radius * a.cos(), center.y + radius * a.sin(), 0.0], color);
    }
    for i in 0..seg {
        let j = (i + 1) % seg;
        g.extend_indices(&[i as u16, j as u16]);
    }
    g
}

/// 椭圆（填充）：`radii` 为两个半轴
pub fn ellipse(center: Vec2, radii: Vec2, segments: u32, color: [f32; 4]) -> Geometry {
    let seg = segments.max(3);
    let mut g = Geometry::default();
    g.push([center.x, center.y, 0.0], color);
    for i in 0..=seg {
        let a = std::f32::consts::TAU * i as f32 / seg as f32;
        g.push(
            [center.x + radii.x * a.cos(), center.y + radii.y * a.sin(), 0.0],
            color,
        );
    }
    for i in 1..=seg {
        g.extend_indices(&[0, i as u16, i as u16 + 1]);
    }
    g
}

/// 线段（LineList，1px）
pub fn line(a: Vec2, b: Vec2, color: [f32; 4]) -> Geometry {
    let mut g = Geometry::default();
    g.push([a.x, a.y, 0.0], color);
    g.push([b.x, b.y, 0.0], color);
    g.extend_indices(&[0, 1]);
    g
}

/// 折线/多边形轮廓（LineList，1px；`closed` 自动闭合首尾）
pub fn polyline(points: &[Vec2], closed: bool, color: [f32; 4]) -> Geometry {
    let mut g = Geometry::default();
    if points.len() < 2 {
        return g;
    }
    let n = points.len();
    let pairs = if closed { n } else { n - 1 };
    for i in 0..pairs {
        let a = points[i];
        let b = points[(i + 1) % n];
        let va = g.push([a.x, a.y, 0.0], color);
        let vb = g.push([b.x, b.y, 0.0], color);
        g.extend_indices(&[va, vb]);
    }
    g
}

/// 正多边形（填充）：`sides` ≥ 3
pub fn regular_polygon(
    center: Vec2,
    radius: f32,
    sides: u32,
    rotation: f32,
    color: [f32; 4],
) -> Geometry {
    let sides = sides.max(3);
    let points: Vec<Vec2> = (0..sides)
        .map(|i| {
            let a = rotation + std::f32::consts::TAU * i as f32 / sides as f32;
            Vec2::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect();
    polygon(&points, color)
}

/// 任意简单多边形（填充）：耳切三角化，凹多边形支持，绕向不限
pub fn polygon(points: &[Vec2], color: [f32; 4]) -> Geometry {
    let indices = triangulate(points);
    let mut g = Geometry::default();
    for p in points {
        g.push([p.x, p.y, 0.0], color);
    }
    g.extend_indices(&indices);
    g
}

/// 胶囊 2D（填充）：线段 ab 两端为圆心的凸包——物理胶囊碰撞体的可视化原语
///
/// 凸多边形，直接从线段中点扇形三角化（无需耳切）。
pub fn capsule(a: Vec2, b: Vec2, radius: f32, segments: u32, color: [f32; 4]) -> Geometry {
    let seg = segments.max(4);
    let axis = (b - a).normalize_or_zero();
    if axis == Vec2::ZERO {
        return circle(a, radius, seg * 2, color);
    }
    let normal = Vec2::new(-axis.y, axis.x);

    // 轮廓：a 端半圆（+normal → -axis → -normal）+ b 端半圆（-normal → +axis → +normal）
    let mut outline: Vec<Vec2> = Vec::with_capacity(seg as usize * 2 + 2);
    for i in 0..=seg {
        let t = std::f32::consts::FRAC_PI_2
            + std::f32::consts::PI * i as f32 / seg as f32;
        let p = a + axis * (t.cos() * radius) + normal * (t.sin() * radius);
        outline.push(p);
    }
    for i in 0..=seg {
        let t = -std::f32::consts::FRAC_PI_2
            + std::f32::consts::PI * i as f32 / seg as f32;
        let p = b + axis * (t.cos() * radius) + normal * (t.sin() * radius);
        outline.push(p);
    }

    // 凸多边形扇形：中点为扇心
    let center = (a + b) * 0.5;
    let mut g = Geometry::default();
    g.push([center.x, center.y, 0.0], color);
    for p in &outline {
        g.push([p.x, p.y, 0.0], color);
    }
    let n = outline.len() as u16;
    for i in 0..n {
        let j = (i + 1) % n;
        g.extend_indices(&[0, 1 + i, 1 + j]);
    }
    g
}
