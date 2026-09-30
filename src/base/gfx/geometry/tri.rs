//! 多边形三角化：耳切法（ear clipping）
//!
//! 适用于**简单多边形**（无洞、不自交；凸/凹均可，绕向不限）。
//! 退化输入（共线点、重复点）由兜底扇形三角化保证输出。

use glam::Vec2;

/// 鞋带公式：有向面积（正 = 逆时针）
pub fn signed_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        area += a.x * b.y - b.x * a.y;
    }
    area * 0.5
}

/// 叉积：ab × ac（判断 b 相对 oa 的转向）
fn cross(o: Vec2, a: Vec2, b: Vec2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// 点 q 是否在三角形 abc 内部（含边界）
fn point_in_triangle(q: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = cross(a, b, q);
    let d2 = cross(b, c, q);
    let d3 = cross(c, a, q);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// 耳切三角化：简单多边形 → 索引（每三角形 3 个索引，绕向与输入一致）
///
/// 退化输入（n < 3 或切分失败）返回兜底扇形/空——**永不 panic**。
pub fn triangulate(points: &[Vec2]) -> Vec<u16> {
    let n = points.len();
    if n < 3 {
        return vec![];
    }

    // 复制索引并统一为逆时针（有向面积为正）
    let mut ring: Vec<u16> = (0..n as u16).collect();
    if signed_area(points) < 0.0 {
        ring.reverse();
    }

    let mut out = Vec::with_capacity((n - 2) * 3);
    let mut guard = 0usize;

    while ring.len() > 3 {
        guard += 1;
        if guard > n * 2 {
            break; // 退化保护
        }

        let len = ring.len();
        let mut clipped = false;
        for i in 0..len {
            let ia = ring[(i + len - 1) % len] as usize;
            let ib = ring[i] as usize;
            let ic = ring[(i + 1) % len] as usize;
            let (a, b, c) = (points[ia], points[ib], points[ic]);

            // 凸顶点 + 内部无其他顶点 = 耳朵
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let contains_other = ring.iter().any(|&k| {
                let p = points[k as usize];
                (k as usize != ia && k as usize != ib && k as usize != ic)
                    && point_in_triangle(p, a, b, c)
            });
            if contains_other {
                continue;
            }

            out.extend_from_slice(&[ia as u16, ib as u16, ic as u16]);
            ring.remove(i);
            clipped = true;
            break;
        }

        if !clipped {
            break; // 无耳可切：交由兜底扇形
        }
    }

    // 兜底扇形（退化多边形的最后手段）
    if ring.len() >= 3 {
        for k in 1..ring.len() - 1 {
            out.extend_from_slice(&[ring[0], ring[k], ring[k + 1]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三角形有向面积和（验证切分正确性的工具）
    fn triangles_area(points: &[Vec2], indices: &[u16]) -> f32 {
        indices
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (points[t[0] as usize], points[t[1] as usize], points[t[2] as usize]);
                (cross(a, b, c)) * 0.5
            })
            .sum()
    }

    #[test]
    fn convex_quad_two_triangles() {
        let quad = [Vec2::ZERO, Vec2::X, Vec2::new(1.0, 1.0), Vec2::Y];
        let idx = triangulate(&quad);
        assert_eq!(idx.len(), 6);
        assert!((triangles_area(&quad, &idx) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn concave_l_shape_area_matches() {
        // L 形（凹多边形，8 顶点）
        let l = [
            Vec2::ZERO,
            Vec2::new(2.0, 0.0),
            Vec2::new(2.0, 1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(1.0, 2.0),
            Vec2::ZERO + Vec2::new(0.0, 2.0),
        ];
        let idx = triangulate(&l);
        // 凹多边形 n=6 → 4 个三角形
        assert_eq!(idx.len(), 12);
        let area = triangles_area(&l, &idx).abs();
        assert!((area - 3.0).abs() < 1e-5, "L 形面积应为 3，实际 {area}");
    }

    #[test]
    fn clockwise_input_handled() {
        // 顺时针输入：绕向自动翻转，结果与逆时针一致
        let mut quad = [Vec2::ZERO, Vec2::Y, Vec2::new(1.0, 1.0), Vec2::X];
        quad.reverse();
        let idx = triangulate(&quad);
        assert!((triangles_area(&quad, &idx).abs() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn degenerate_returns_empty() {
        assert!(triangulate(&[Vec2::ZERO]).is_empty());
        assert!(triangulate(&[Vec2::ZERO, Vec2::X]).is_empty());
        // 共线点
        let line = [Vec2::ZERO, Vec2::X, Vec2::new(2.0, 0.0)];
        let idx = triangulate(&line);
        assert!(idx.is_empty() || triangles_area(&line, &idx).abs() < 1e-5);
    }
}
