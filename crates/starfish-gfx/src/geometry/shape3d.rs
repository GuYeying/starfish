//! 3D 形状生成器（纯函数；外向绕行，配合 `fill_pipeline_3d`）

use glam::{Vec2, Vec3};

use super::super::ShapeVertex;
use super::Geometry;

const TAU: f32 = std::f32::consts::TAU;

/// 立方体（轴对齐）：8 顶点 36 索引（12 三角形，外向绕行）
pub fn cube(min: Vec3, max: Vec3, color: [f32; 4]) -> Geometry {
    let (x0, y0, z0) = (min.x, min.y, min.z);
    let (x1, y1, z1) = (max.x, max.y, max.z);
    let v = |p: [f32; 3]| ShapeVertex::new(p, color);
    let vertices = vec![
        v([x0, y0, z0]), // 0
        v([x1, y0, z0]), // 1
        v([x0, y1, z0]), // 2
        v([x1, y1, z0]), // 3
        v([x0, y0, z1]), // 4
        v([x1, y0, z1]), // 5
        v([x0, y1, z1]), // 6
        v([x1, y1, z1]), // 7
    ];
    // 每面 4 顶点 2 三角形，绕向外向（Ccw）
    let indices: Vec<u16> = vec![
        0, 1, 5, 0, 5, 4, // y0
        2, 6, 7, 2, 7, 3, // y1
        4, 5, 7, 4, 7, 6, // z1
        1, 0, 2, 1, 2, 3, // z0
        0, 4, 6, 0, 6, 2, // x0
        5, 1, 3, 5, 3, 7, // x1
    ];
    Geometry::indexed(vertices, indices)
}

/// UV 球（填充）：经纬分段，极点环重复（含退化三角形，绘制无影响）
///
/// 顶点数 = (segments_v + 1) × (segments_u + 1)，索引数 = segments_v × segments_u × 6
pub fn sphere(
    center: Vec3,
    radius: f32,
    segments_u: u32,
    segments_v: u32,
    color: [f32; 4],
) -> Geometry {
    let su = segments_u.max(3);
    let sv = segments_v.max(2);
    let mut g = Geometry::default();

    for vv in 0..=sv {
        let phi = std::f32::consts::PI * vv as f32 / sv as f32; // 0 = +y 极点
        let (sp, cp) = (phi.sin(), phi.cos());
        for uu in 0..=su {
            let theta = TAU * uu as f32 / su as f32;
            let (st, ct) = (theta.sin(), theta.cos());
            let p = center + radius * Vec3::new(sp * ct, cp, sp * st);
            g.push([p.x, p.y, p.z], color);
        }
    }
    for vv in 0..sv {
        for uu in 0..su {
            let a = vv * (su + 1) + uu;
            let b = a + su + 1;
            let c = a + 1;
            let d = b + 1;
            g.extend_indices(&[a as u16, b as u16, c as u16]);
            g.extend_indices(&[b as u16, d as u16, c as u16]);
        }
    }
    g
}

/// 有限平面（四边形）：`right`/`up` 为互相垂直的两个边向量方向
pub fn plane(center: Vec3, right: Vec3, up: Vec3, size: Vec2, color: [f32; 4]) -> Geometry {
    let r = right.normalize_or_zero() * (size.x * 0.5);
    let u = up.normalize_or_zero() * (size.y * 0.5);
    let corners = [
        center - r - u,
        center + r - u,
        center + r + u,
        center - r + u,
    ];
    let vertices: Vec<ShapeVertex> =
        corners.iter().map(|&p| ShapeVertex::new([p.x, p.y, p.z], color)).collect();
    Geometry::indexed(vertices, vec![0, 1, 2, 0, 2, 3])
}

/// 圆柱（轴 = +Y，`base_center` 为底面圆心）：侧面 + 上下端盖
pub fn cylinder(
    base_center: Vec3,
    radius: f32,
    height: f32,
    segments: u32,
    color: [f32; 4],
) -> Geometry {
    let seg = segments.max(3);
    let mut g = Geometry::default();

    // 侧面：底环 + 顶环（含重复首点，共 seg+1 × 2）
    for yy in 0..2 {
        let y = base_center.y + if yy == 0 { 0.0 } else { height };
        for i in 0..=seg {
            let a = TAU * i as f32 / seg as f32;
            let (s, c) = (a.sin(), a.cos());
            g.push(
                [base_center.x + radius * c, y, base_center.z + radius * s],
                color,
            );
        }
    }
    let top_start = (seg + 1) as u16;
    for i in 0..seg as u16 {
        let (b0, b1, t0, t1) = (i, i + 1, top_start + i, top_start + i + 1);
        g.extend_indices(&[b0, t0, t1]);
        g.extend_indices(&[b0, t1, b1]);
    }

    // 端盖扇形（顶面 +y 向上、底面 -y 向下）
    let top_center = g.vertex_len() as u16;
    g.push([base_center.x, base_center.y + height, base_center.z], color);
    let bottom_center = g.vertex_len() as u16;
    g.push([base_center.x, base_center.y, base_center.z], color);
    for i in 0..seg as u16 {
        let j = (i + 1) % seg as u16;
        let bi = i;
        let bj = j;
        g.extend_indices(&[top_center, top_start + bi, top_start + bj]);
        g.extend_indices(&[bottom_center, bj, bi]);
    }
    g
}

/// 圆锥（轴 = +Y，`base_center` 为底面圆心）：侧面 + 底面
pub fn cone(
    base_center: Vec3,
    radius: f32,
    height: f32,
    segments: u32,
    color: [f32; 4],
) -> Geometry {
    let seg = segments.max(3);
    let mut g = Geometry::default();

    let apex = g.vertex_len() as u16;
    g.push([base_center.x, base_center.y + height, base_center.z], color);
    let base_center_idx = g.vertex_len() as u16;
    g.push([base_center.x, base_center.y, base_center.z], color);
    let ring_start = g.vertex_len() as u16;
    for i in 0..=seg {
        let a = TAU * i as f32 / seg as f32;
        let (s, c) = (a.sin(), a.cos());
        g.push(
            [base_center.x + radius * c, base_center.y, base_center.z + radius * s],
            color,
        );
    }

    for i in 0..seg as u16 {
        let j = i + 1;
        // 侧面
        g.extend_indices(&[apex, ring_start + i, ring_start + j]);
        // 底面
        g.extend_indices(&[base_center_idx, ring_start + j, ring_start + i]);
    }
    g
}

/// 胶囊 3D（线段 ab 两端为球心的凸包）——物理胶囊碰撞体的可视化原语
///
/// 环带构造：a 端半球环带 → a/b 直段双环 → b 端半球环带（极点用重复点环，
/// 退化三角形绘制无影响）。`cap_rows` 为每端半球的环数。
pub fn capsule(
    a: Vec3,
    b: Vec3,
    radius: f32,
    segments: u32,
    cap_rows: u32,
    color: [f32; 4],
) -> Geometry {
    let seg = segments.max(4);
    let rows = cap_rows.max(2);
    let axis = (b - a).normalize_or_zero();
    if axis == Vec3::ZERO {
        return sphere(a, radius, seg, rows * 2, color);
    }

    // 正交基
    let helper = if axis.dot(Vec3::Y).abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let u = axis.cross(helper).normalize();
    let v = axis.cross(u).normalize();

    let mut g = Geometry::default();

    // 极点 a（-axis 方向）
    let pole_a = g.vertex_len() as u16;
    let pa = a - axis * radius;
    g.push([pa.x, pa.y, pa.z], color);

    // 环带：a 半球（α 递减到 0）→ a 平面 → b 平面 → b 半球（α 递增）
    let mut ring_starts: Vec<u16> = Vec::new();
    let mut push_ring = |center: Vec3, ring_radius: f32| {
        ring_starts.push(g.vertex_len() as u16);
        for i in 0..=seg {
            let th = TAU * i as f32 / seg as f32;
            let (st, ct) = (th.sin(), th.cos());
            let p = center + u * (ring_radius * ct) + v * (ring_radius * st);
            g.push([p.x, p.y, p.z], color);
        }
    };
    for k in (1..=rows).rev() {
        let alpha = std::f32::consts::FRAC_PI_2 * k as f32 / (rows + 1) as f32;
        push_ring(a - axis * (radius * alpha.sin()), radius * alpha.cos());
    }
    push_ring(a, radius);
    push_ring(b, radius);
    for k in 1..=rows {
        let alpha = std::f32::consts::FRAC_PI_2 * k as f32 / (rows + 1) as f32;
        push_ring(b + axis * (radius * alpha.sin()), radius * alpha.cos());
    }

    // 极点 b（+axis 方向）
    let pole_b = g.vertex_len() as u16;
    let pb = b + axis * radius;
    g.push([pb.x, pb.y, pb.z], color);

    // 环间四边形
    for r_i in 0..ring_starts.len() - 1 {
        let (s0, s1) = (ring_starts[r_i], ring_starts[r_i + 1]);
        for i in 0..seg as u16 {
            let j = i + 1;
            g.extend_indices(&[s0 + i, s1 + i, s1 + j]);
            g.extend_indices(&[s0 + i, s1 + j, s0 + j]);
        }
    }
    // 极点扇形
    let first = ring_starts[0];
    for i in 0..seg as u16 {
        let j = i + 1;
        g.extend_indices(&[pole_a, first + i, first + j]);
    }
    let last = *ring_starts.last().unwrap();
    for i in 0..seg as u16 {
        let j = i + 1;
        g.extend_indices(&[pole_b, last + j, last + i]);
    }
    g
}
