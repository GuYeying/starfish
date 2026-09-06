//! 字形光栅化：轮廓边 → 覆盖度位图
//!
//! 非零环绕（TrueType 语义）扫描线填充 + 每像素 4×4 超采样抗锯齿。
//! 纯函数、零依赖、可独立测试。质量不满意时可在不动上层 API 的前提下整体替换。

use ttf_parser::OutlineBuilder;

/// 一条线段（像素空间，y 向下为正）
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// 曲线细分段数
const QUAD_STEPS: usize = 8;
const CUBIC_STEPS: usize = 16;
/// 每像素单边采样数（总采样 = SAMPLES²  = 16）
pub(crate) const SAMPLES: u32 = 4;

/// 轮廓收集器：实现 `ttf_parser::OutlineBuilder`，
/// 把字形轮廓（二次/三次贝塞尔）拍平成线段集合
pub(crate) struct EdgeCollector {
    pub edges: Vec<Edge>,
    start: (f32, f32),
    current: (f32, f32),
}

impl EdgeCollector {
    pub fn new() -> Self {
        Self {
            edges: Vec::new(),
            start: (0.0, 0.0),
            current: (0.0, 0.0),
        }
    }

    fn push_edge(&mut self, x1: f32, y1: f32) {
        let (x0, y0) = self.current;
        if (x0 - x1).abs() > f32::EPSILON || (y0 - y1).abs() > f32::EPSILON {
            self.edges.push(Edge { x0, y0, x1, y1 });
        }
        self.current = (x1, y1);
    }
}

impl OutlineBuilder for EdgeCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = (x, y);
        self.current = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push_edge(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.current;
        for i in 1..=QUAD_STEPS {
            let t = i as f32 / QUAD_STEPS as f32;
            let mt = 1.0 - t;
            let px = mt * mt * x0 + 2.0 * mt * t * x1 + t * t * x;
            let py = mt * mt * y0 + 2.0 * mt * t * y1 + t * t * y;
            self.push_edge(px, py);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.current;
        for i in 1..=CUBIC_STEPS {
            let t = i as f32 / CUBIC_STEPS as f32;
            let mt = 1.0 - t;
            let px = mt * mt * mt * x0 + 3.0 * mt * mt * t * x1 + 3.0 * mt * t * t * x2 + t * t * t * x;
            let py = mt * mt * mt * y0 + 3.0 * mt * mt * t * y1 + 3.0 * mt * t * t * y2 + t * t * t * y;
            self.push_edge(px, py);
        }
    }

    fn close(&mut self) {
        let (sx, sy) = self.start;
        self.push_edge(sx, sy);
    }
}

/// 光栅化：非零环绕 + 每像素 SAMPLES×SAMPLES 超采样
///
/// `edges` 为像素空间坐标（已含缩放与 y 翻转），
/// 输出 height 行 × width 列的覆盖度（row-major，0~255）。
pub(crate) fn rasterize(edges: &[Edge], width: usize, height: usize) -> Vec<u8> {
    let mut coverage = vec![0u8; width * height];
    if width == 0 || height == 0 {
        return coverage;
    }

    let s = SAMPLES as f32;
    // 四舍五入保证满覆盖像素精确到 255（255/16 = 15.94 → 16，saturating 累加封顶）
    let contribution = (255.0 / (s * s)).round() as u8;
    let step = 1.0 / s;

    // 预过滤：只保留与位图垂直范围相交的非水平边
    let active: Vec<&Edge> = edges
        .iter()
        .filter(|e| {
            e.y0 != e.y1
                && e.y0.min(e.y1) < height as f32
                && e.y0.max(e.y1) > 0.0
        })
        .collect();

    for py in 0..height {
        let row = &mut coverage[py * width..(py + 1) * width];

        for sy in 0..SAMPLES {
            let y = py as f32 + (sy as f32 + 0.5) * step;

            // 该扫描线上的交点 (x, 环绕方向)；半开区间避免顶点重复计数
            let mut crossings: Vec<(f32, i32)> = active
                .iter()
                .filter_map(|e| {
                    let (yt, xb) = if e.y0 < e.y1 { (e.y0, e.x0) } else { (e.y1, e.x1) };
                    let (yb, xe) = if e.y0 < e.y1 { (e.y1, e.x1) } else { (e.y0, e.x0) };
                    if y < yt || y >= yb {
                        return None;
                    }
                    let t = (y - yt) / (yb - yt);
                    let x = xb + (xe - xb) * t;
                    let dir = if e.y1 > e.y0 { 1 } else { -1 };
                    Some((x, dir))
                })
                .collect();
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

            // 扫描该行：非零环绕的内部区间 [from, to)
            let mut winding: i32 = 0;
            let mut inside_from = 0.0f32;
            for (x, dir) in &crossings {
                let prev = winding;
                winding += dir;
                if prev == 0 && winding != 0 {
                    inside_from = *x;
                } else if prev != 0 && winding == 0 {
                    accumulate_span(row, inside_from, *x, width, s, step, contribution);
                }
            }
        }
    }
    coverage
}

/// 把一个内部区间的采样命中累计进像素行
fn accumulate_span(
    row: &mut [u8],
    from: f32,
    to: f32,
    width: usize,
    s: f32,
    step: f32,
    contribution: u8,
) {
    if to <= from {
        return;
    }
    let start_px = (from.floor() as i64).clamp(0, width as i64 - 1) as usize;
    let end_px = (to.ceil() as i64).clamp(0, width as i64) as usize;
    for px in start_px..end_px {
        for sx in 0..s as u32 {
            let x = px as f32 + (sx as f32 + 0.5) * step;
            if x >= from && x < to {
                row[px] = row[px].saturating_add(contribution);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_square_edges() -> Vec<Edge> {
        // (1,1)-(5,5) 的正方形（像素空间，顺时针）
        vec![
            Edge { x0: 1.0, y0: 1.0, x1: 5.0, y1: 1.0 },
            Edge { x0: 5.0, y0: 1.0, x1: 5.0, y1: 5.0 },
            Edge { x0: 5.0, y0: 5.0, x1: 1.0, y1: 5.0 },
            Edge { x0: 1.0, y0: 5.0, x1: 1.0, y1: 1.0 },
        ]
    }

    #[test]
    fn square_interior_full_exterior_empty() {
        let cov = rasterize(&unit_square_edges(), 6, 6);
        let at = |x: usize, y: usize| cov[y * 6 + x];
        // 内部全满
        assert_eq!(at(3, 3), 255);
        assert_eq!(at(2, 2), 255);
        // 外部全空
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(5, 0), 0);
    }

    #[test]
    fn boundary_is_antialiased() {
        // 非整数坐标的正方形：边界像素必然部分覆盖
        let edges = vec![
            Edge { x0: 1.5, y0: 1.5, x1: 4.5, y1: 1.5 },
            Edge { x0: 4.5, y0: 1.5, x1: 4.5, y1: 4.5 },
            Edge { x0: 4.5, y0: 4.5, x1: 1.5, y1: 4.5 },
            Edge { x0: 1.5, y0: 4.5, x1: 1.5, y1: 1.5 },
        ];
        let cov = rasterize(&edges, 6, 6);
        let at = |x: usize, y: usize| cov[y * 6 + x];
        // 边界像素部分覆盖（0 < a < 255）
        let a = at(1, 3);
        assert!(a > 0 && a < 255, "边界覆盖度 = {a}");
        // 完全在外/完全在内
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(3, 3), 255);
    }

    #[test]
    fn nonzero_wind_hole() {
        // 同心两个正方形：外圈顺时针 + 内圈逆时针 → 中心是洞（非零环绕）
        let mut edges = unit_square_edges();
        edges.extend([
            Edge { x0: 2.0, y0: 2.0, x1: 2.0, y1: 4.0 },
            Edge { x0: 2.0, y0: 4.0, x1: 4.0, y1: 4.0 },
            Edge { x0: 4.0, y0: 4.0, x1: 4.0, y1: 2.0 },
            Edge { x0: 4.0, y0: 2.0, x1: 2.0, y1: 2.0 },
        ]);
        let cov = rasterize(&edges, 6, 6);
        let at = |x: usize, y: usize| cov[y * 6 + x];
        assert_eq!(at(3, 3), 0, "洞中心应为空");
        assert_eq!(at(1, 3) > 0 || at(2, 1) > 0 || at(1, 1) > 0 || at(5, 5) >= 0, true);
        // 环带处有覆盖
        assert!(at(1, 3) > 0, "外圈环带应有覆盖");
    }
}
