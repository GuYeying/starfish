//! pygame.draw 对应物（契约 §七：接受任何绘制终点；即时语义、batch 执行）
//!
//! 签名 = `fn(t: &impl DrawTarget, …, width: u32)`——**无返回值、无色参**
//! （颜色经 Batch 状态机 `set_color`/`set_wireframe` one-shot 设置）
//! （GPU 全帧 present 无脏矩形概念；pygame 的"受影响 Rect"返回随 Rect
//! 类型一并退役——碰撞/触发器归物理引擎）。`screen`（交换链背板）与
//! `Surface`（纹理背板）同一签名，教程形态保真
//! （`draw.rect(screen, [0,200,90], (10, 10, 60, 40), 0)`）。
//!
//! 执行 = 图形转图元几何（复用 `base::gfx::geometry::shape2d` 家底）→
//! 目标 batch → flip/flush 统一提交。全路径零 CPU 逐像素。
//!
//! ⚠️ v1 简化（P4 优化位）：**线族**（line/lines/arc）的 width > 0 描边
//! 一律 1px（LineList）；`rect` 的粗描边是四条实心边带（真 pygame 厚度
//! 向内语义）；pygame 其余形状的粗描边暂不做。

use starfish_gfx::geometry::{shape2d, shape3d};
use crate::render::{Batch, DrawTarget, GeometryKind, IRect};
use crate::Color;

fn line_outline(t: &impl DrawTarget, c: [f32; 4], g: &starfish_gfx::geometry::Geometry) {
    t.with_batch(|b: &mut Batch| b.push_geometry(GeometryKind::Line, g, [0.0, 0.0], c));
}

/// 矩形（width=0 填充，>0 描边）——契约 §七 正典：
/// `draw.rect(screen, [0, 200, 90], (10, 10, 60, 40), 0)`
///
/// 描边 width>1 = 四条实心边带（真粗边；1px 细边走线流）。
pub fn rect(t: &impl DrawTarget, r: IRect, width: u32) {
    let c = t.current_draw_color();
    let (x, y) = (r[0] as f32, r[1] as f32);
    let (w, h) = (r[2] as f32, r[3] as f32);
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
}

/// 椭圆弧（内切于 rect；对齐 pygame：**角度为弧度**；0 = +x 方向，向正角
/// 方向扫到 stop；width 恒 1px——v1 描边简化）
pub fn arc(
    t: &impl DrawTarget,
    r: IRect,
    start_angle: f32,
    stop_angle: f32,
    width: u32,
) {
    let _ = width; // v1 恒 1px
    let c = t.current_draw_color();
    let (x, y) = (r[0] as f32, r[1] as f32);
    let (w, h) = (r[2] as f32, r[3] as f32);
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
}

/// 线段（1px）
pub fn line(
    t: &impl DrawTarget,
    start: (i32, i32),
    end: (i32, i32),
    width: u32,
) {
    let _ = width; // v1 恒 1px
    let c = t.current_draw_color();
    let g = shape2d::line([start.0 as f32, start.1 as f32].into(), [end.0 as f32, end.1 as f32].into(), c);
    line_outline(t, c, &g);
}

/// 折线 / 多边形轮廓（closed 自动闭合；1px）
pub fn lines(
    t: &impl DrawTarget,
    closed: bool,
    points: &[(i32, i32)],
    width: u32,
) {
    let _ = width; // v1 恒 1px
    if points.len() < 2 {
        return;
    }
    let c = t.current_draw_color();
    let pts: Vec<glam::Vec2> = points.iter().map(|p| [p.0 as f32, p.1 as f32].into()).collect();
    let g = shape2d::polyline(&pts, closed, c);
    line_outline(t, c, &g);
}

/// 圆（width=0 填充扇形三角化，>0 描边；段数随半径自适应）
pub fn circle(
    t: &impl DrawTarget,
    center: (i32, i32),
    radius: u32,
    width: u32,
) {
    let c = t.current_draw_color();
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
}

/// 椭圆（内切于 rect；width=0 填充，>0 描边）
pub fn ellipse(t: &impl DrawTarget, r: IRect, width: u32) {
    let c = t.current_draw_color();
    let (x, y) = (r[0] as f32, r[1] as f32);
    let (w, h) = (r[2] as f32, r[3] as f32);
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
}

/// 任意简单多边形（耳切三角化，凹多边形支持；width=0 填充，>0 描边）
pub fn polygon(t: &impl DrawTarget, points: &[(i32, i32)], width: u32) {
    if points.len() < 3 {
        return;
    }
    let c = t.current_draw_color();
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
}

// ── 3D 形状（starfish-gfx shape3d 对接——2026-11-12 gfxdraw 对接缺口
// 补齐）：透视相机（gfxdraw.set_camera）+ render_depth 深度会话配套；
// 顶点 z 真实参与（batch SpriteVertex pos[3] 深度测试管线）──────────

fn fill3(t: &impl DrawTarget, g: starfish_gfx::geometry::Geometry) {
    let c = t.current_draw_color();
    t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], c));
}

/// 立方体（填充）：min/max 对角。
pub fn cube(t: &impl DrawTarget, min: [f32; 3], max: [f32; 3]) {
    let c = t.current_draw_color();
    let g = shape3d::cube([min[0], min[1], min[2]].into(), [max[0], max[1], max[2]].into(), c);
    fill3(t, g);
}

/// 球（填充）：经纬分段（segments_u ≥ 3 / segments_v ≥ 2）。
pub fn sphere(t: &impl DrawTarget, center: [f32; 3], radius: f32, segments_u: u32, segments_v: u32) {
    let g = shape3d::sphere([center[0], center[1], center[2]].into(), radius, segments_u, segments_v, t.current_draw_color());
    fill3(t, g);
}

/// 平面（填充）：center + right/up 基（尺寸沿基向量）。
pub fn plane(t: &impl DrawTarget, center: [f32; 3], right: [f32; 3], up: [f32; 3], size: [f32; 2]) {
    let g = shape3d::plane(
        [center[0], center[1], center[2]].into(),
        [right[0], right[1], right[2]].into(),
        [up[0], up[1], up[2]].into(),
        glam::Vec2::new(size[0], size[1]),
        t.current_draw_color(),
    );
    fill3(t, g);
}

/// 圆柱（填充）：base_center 起沿 +y 挤出 height。
pub fn cylinder(t: &impl DrawTarget, base_center: [f32; 3], radius: f32, height: f32, segments: u32) {
    let g = shape3d::cylinder([base_center[0], base_center[1], base_center[2]].into(), radius, height, segments, t.current_draw_color());
    fill3(t, g);
}

/// 圆锥（填充）：base_center 起沿 +y 顶点高 height。
pub fn cone(t: &impl DrawTarget, base_center: [f32; 3], radius: f32, height: f32, segments: u32) {
    let g = shape3d::cone([base_center[0], base_center[1], base_center[2]].into(), radius, height, segments, t.current_draw_color());
    fill3(t, g);
}

/// 平面多边形 3D（填充）：任意共面点列（flat [x, y, z, ...]，len % 3 == 0）。
pub fn polygon_3d(t: &impl DrawTarget, points: &[f32]) {
    let pts: Vec<glam::Vec3> = points
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]].into())
        .collect();
    let g = shape3d::polygon_3d(&pts, t.current_draw_color());
    fill3(t, g);
}

// ── 2D 补缺（starfish-gfx shape2d 已有、draw/gfxdraw 未包装的三件）────

/// 胶囊（填充）：线段 ab 两端圆心的凸包。
pub fn capsule(t: &impl DrawTarget, a: [f32; 2], b: [f32; 2], radius: f32, segments: u32) {
    let g = shape2d::capsule([a[0], a[1]].into(), [b[0], b[1]].into(), radius, segments, t.current_draw_color());
    t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], t.current_draw_color()));
}

/// 折线（1px）：closed = 闭合多边形轮廓。
pub fn polyline(t: &impl DrawTarget, points: &[[f32; 2]], closed: bool) {
    let pts: Vec<glam::Vec2> = points.iter().map(|p| [p[0], p[1]].into()).collect();
    let g = shape2d::polyline(&pts, closed, t.current_draw_color());
    line_outline(t, t.current_draw_color(), &g);
}

/// 正多边形（填充）：center/radius/sides/rotation。
pub fn regular_polygon(
    t: &impl DrawTarget,
    center: [f32; 2],
    radius: f32,
    sides: u32,
    rotation: f32,
) {
    let g = shape2d::regular_polygon(
        [center[0], center[1]].into(),
        radius,
        sides,
        rotation,
        t.current_draw_color(),
    );
    t.with_batch(|b| b.push_geometry(GeometryKind::Filled, &g, [0.0, 0.0], t.current_draw_color()));
}
