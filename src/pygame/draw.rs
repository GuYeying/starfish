//! pygame.draw 对应物（契约 §七：接受任何绘制终点；即时语义、batch 执行）
//!
//! 签名 = `fn(t: &impl DrawTarget, color, …, width: u32) -> Rect`——
//! `screen`（交换链背板）与 `Surface`（纹理背板）同一签名，教程形态
//! 保真（`draw.rect(screen, [0,200,90], pos, 0)`）。
//!
//! 执行 = 图形转图元几何（复用 `base::gfx::geometry::shape2d` 家底）→
//! 目标 batch → flip/flush 统一提交。全路径零 CPU 逐像素。
//!
//! ⚠️ v1 简化（P4 优化位）：`width > 0` 的描边一律 **1px**（LineList）；
//! pygame 的粗描边（width 2+）暂不做。

use crate::base::gfx::geometry::shape2d;
use crate::pygame::render::{Batch, DrawTarget, GeometryKind};
use crate::pygame::{Color, Rect};

fn rgba(c: impl Into<Color>) -> [f32; 4] {
    let (r, g, b, a) = c.into().normalize();
    [r, g, b, a]
}

fn line_outline(t: &impl DrawTarget, color: [f32; 4], g: &crate::base::gfx::geometry::Geometry) {
    t.with_batch(|b: &mut Batch| b.push_geometry(GeometryKind::Line, g, [0.0, 0.0], color));
}

/// 矩形（width=0 填充，>0 描边）——契约 §七 正典：
/// `draw.rect(screen, [0, 200, 90], pos, 0)`
///
/// 描边 width>1 = 四条实心边带（真粗边；1px 细边走线流）。
pub fn rect(t: &impl DrawTarget, color: impl Into<Color>, r: Rect, width: u32) -> Rect {
    let c = rgba(color);
    let (x, y) = (r.left as f32, r.top as f32);
    let (w, h) = (r.width as f32, r.height as f32);
    if width == 0 {
        t.with_batch(|b| b.push_quad(x, y, w, h, c, None));
    } else if width == 1 {
        let g = shape2d::rect_outline([x, y].into(), [x + w, y + h].into(), c);
        line_outline(t, c, &g);
    } else {
        // 四边带（pygame 语义：厚度向内；钳到半边长避免翻转）
        let bw = (width as f32).min(w / 2.0);
        let bh = (width as f32).min(h / 2.0);
        t.with_batch(|b| {
            b.push_quad(x, y, w, bh, c, None); // 上
            b.push_quad(x, y + h - bh, w, bh, c, None); // 下
            b.push_quad(x, y + bh, bw, h - 2.0 * bh, c, None); // 左
            b.push_quad(x + w - bw, y + bh, bw, h - 2.0 * bh, c, None); // 右
        });
    }
    r
}

/// 椭圆弧（对齐 pygame：**角度为弧度**；0 = +x 方向，向正角方向扫到
/// stop；width>1 恒 1px——v1 描边简化）
pub fn arc(
    t: &impl DrawTarget,
    color: impl Into<Color>,
    r: Rect,
    start_angle: f32,
    stop_angle: f32,
    width: u32,
) -> Rect {
    let _ = width; // v1 恒 1px
    let c = rgba(color);
    let (x, y) = (r.left as f32, r.top as f32);
    let (w, h) = (r.width as f32, r.height as f32);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    // 扫角分段（每 ~10° 一段，最少 4 段）
    let sweep = stop_angle - start_angle;
    let segs = ((sweep.abs() * 180.0 / std::f32::consts::PI).ceil() as u32).clamp(4, 256);
    let pts: Vec<glam::Vec2> = (0..=segs)
        .map(|i| {
            let a = start_angle + sweep * i as f32 / segs as f32;
            glam::Vec2::new(cx + (w / 2.0) * a.cos(), cy + (h / 2.0) * a.sin())
        })
        .collect();
    let g = shape2d::polyline(&pts, false, c);
    line_outline(t, c, &g);
    r
}

/// 线段（1px）
pub fn line(
    t: &impl DrawTarget,
    color: impl Into<Color>,
    start: (i32, i32),
    end: (i32, i32),
    width: u32,
) -> Rect {
    let _ = width; // v1 恒 1px
    let c = rgba(color);
    let g = shape2d::line([start.0 as f32, start.1 as f32].into(), [end.0 as f32, end.1 as f32].into(), c);
    line_outline(t, c, &g);
    bounds(&[start, end])
}

/// 折线 / 多边形轮廓（closed 自动闭合；1px）
pub fn lines(
    t: &impl DrawTarget,
    color: impl Into<Color>,
    closed: bool,
    points: &[(i32, i32)],
    width: u32,
) -> Rect {
    let _ = width; // v1 恒 1px
    if points.len() < 2 {
        return Rect::new(0, 0, 0, 0);
    }
    let c = rgba(color);
    let pts: Vec<glam::Vec2> = points.iter().map(|p| [p.0 as f32, p.1 as f32].into()).collect();
    let g = shape2d::polyline(&pts, closed, c);
    line_outline(t, c, &g);
    bounds(points)
}

/// 圆（width=0 填充扇形三角化，>0 描边；段数随半径自适应）
pub fn circle(
    t: &impl DrawTarget,
    color: impl Into<Color>,
    center: (i32, i32),
    radius: u32,
    width: u32,
) -> Rect {
    let c = rgba(color);
    let ctr = [center.0 as f32, center.1 as f32].into();
    let segments = (radius.max(12)).clamp(24, 64);
    let g = if width == 0 {
        shape2d::circle(ctr, radius as f32, segments, c)
    } else {
        shape2d::circle_outline(ctr, radius as f32, segments, c)
    };
    if width == 0 {
        t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], c));
    } else {
        line_outline(t, c, &g);
    }
    let r = radius as i32;
    Rect::new(center.0 - r, center.1 - r, radius as i32 * 2, radius as i32 * 2)
}

/// 椭圆（内切于 rect；width=0 填充，>0 描边）
pub fn ellipse(t: &impl DrawTarget, color: impl Into<Color>, r: Rect, width: u32) -> Rect {
    let c = rgba(color);
    let (x, y) = (r.left as f32, r.top as f32);
    let (w, h) = (r.width as f32, r.height as f32);
    let center = [x + w / 2.0, y + h / 2.0].into();
    let segments = 48;
    let g = if width == 0 {
        shape2d::ellipse(center, [w / 2.0, h / 2.0].into(), segments, c)
    } else {
        // 描边 = 椭圆轮廓（polyline 闭合）
        let pts: Vec<glam::Vec2> = (0..segments)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / segments as f32;
                glam::Vec2::new(x + w / 2.0 + (w / 2.0) * a.cos(), y + h / 2.0 + (h / 2.0) * a.sin())
            })
            .collect();
        shape2d::polyline(&pts, true, c)
    };
    if width == 0 {
        t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], c));
    } else {
        line_outline(t, c, &g);
    }
    r
}

/// 任意简单多边形（耳切三角化，凹多边形支持；width=0 填充，>0 描边）
pub fn polygon(t: &impl DrawTarget, color: impl Into<Color>, points: &[(i32, i32)], width: u32) -> Rect {
    if points.len() < 3 {
        return Rect::new(0, 0, 0, 0);
    }
    let c = rgba(color);
    let pts: Vec<glam::Vec2> = points.iter().map(|p| [p.0 as f32, p.1 as f32].into()).collect();
    let g = if width == 0 {
        shape2d::polygon(&pts, c)
    } else {
        shape2d::polyline(&pts, true, c)
    };
    if width == 0 {
        t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], c));
    } else {
        line_outline(t, c, &g);
    }
    bounds(points)
}

/// 点集包围盒（pygame draw.* 返回受影响 Rect）
fn bounds(points: &[(i32, i32)]) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y) in points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
