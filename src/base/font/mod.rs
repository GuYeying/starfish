//! 字体模块（对位 pygame.font.Font）——**服务游戏内文本（2D/3D），不服务 UI**
//! （UI 库自带文本栈，与之零耦合）
//!
//! 分层：
//! - 数据侧：`Font` 解析 + 自研扫描线光栅化（`raster`）+ `build_atlas` 图集
//! - 布局侧：`layout_text[_with]` / `layout_positioned` → 标准顶点
//!   （[`TextVertex`]：pos3 + uv2 + color4——2D 传 z=0，3D 传世界坐标）
//! - 渲染侧：[`pipeline`] 产出**标准渲染对象**（Mesh / BindGroup / RenderPipeline），
//!   绘制与 examples/02-04 完全同构（`pass.set_mesh` + `pass.draw`）——
//!   **没有专属 Renderer，也没有隐藏状态**
//!
//! ```ignore
//! let font = Font::from_file("font.ttf", 48.0)?;
//! let atlas = font.build_atlas("Hello".chars(), &resouce)?;
//!
//! // 与 02-04 示例同构的标准对象装配
//! // 相机 uniform（04 案例同款：裸缓冲 + bind group，MVP 由开发者计算写入）
//! let camera_buffer = resouce.create_raw_buffer(Some("camera"), 64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
//! let camera_bind = resouce.bind_group_builder().uniform_raw(0, camera_buffer.clone(), 64).build(Some("camera_bind"));
//! let atlas_bind = pipeline::atlas_bind_group(&resouce, &atlas);
//! let mesh = pipeline::text_mesh_tf(&resouce, &atlas, "Hello", &mvp, 1.0, [1.0; 4]);
//! let pipeline = pipeline::text_pipeline(&resouce, &camera_bind, &atlas_bind, &mesh, 1);
//!
//! // 每帧（02-04 同款调用）
//! // 每帧：MVP = projection × view × model（glam 合成），写入相机缓冲
//! resouce.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&mvp.to_cols_array_2d()));
//! pass.set_pipeline(&pipeline);
//! pass.set_bind_group(0, &camera_bind);
//! pass.set_bind_group(1, &atlas_bind);
//! pass.set_mesh(&mesh);
//! pass.draw(0..mesh.vertex_count(), 0..1);
//!
//! // 实时更新网格（billboard / 动态文本）：标准 mesh 写入
//! resouce.write_vertex_buffer(&mesh, &new_bytes);
//! ```

pub mod pipeline;
pub mod raster;

pub use pipeline::{
    atlas_bind_group, text_mesh_local, text_mesh_tf, text_pipeline, text_pipeline_3d,
};

use std::collections::HashMap;
use std::sync::Arc;

use crate::base::render::render_resource_access::RenderResourceAccess;
use crate::base::render::texture::{
    MipmapPolicy, Texture, TextureDescriptor, TextureDim, TextureSemantic, TextureUsage,
};
use crate::base::resources::image::ImageData;

/// 文本顶点：**pos3**（2D 传 z=0，3D 传世界坐标）+ uv2 + color4
///
/// 公开类型：开发者可自行构造顶点序列（自定义排版/shaping）后
/// 经 `resouce.mesh_builder(TextVertex::layout(), bytes)` 接入标准渲染。
#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct TextVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl TextVertex {
    /// 顶点布局（`resouce.mesh_builder` 用）
    pub fn layout() -> Vec<wgpu::VertexFormat> {
        vec![
            wgpu::VertexFormat::Float32x3,
            wgpu::VertexFormat::Float32x2,
            wgpu::VertexFormat::Float32x4,
        ]
    }

    /// 顶点字节大小（32）
    pub const fn size() -> usize {
        std::mem::size_of::<Self>()
    }
}

/// 字体错误
#[derive(Debug)]
pub enum FontError {
    Io(std::io::Error),
    Parse(ttf_parser::FaceParsingError),
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::Io(e) => write!(f, "字体文件读取失败: {e}"),
            FontError::Parse(e) => write!(f, "字体解析失败: {e:?}"),
        }
    }
}

impl std::error::Error for FontError {}

/// 光栅化后的单个字形（像素空间，y 向下）
pub struct RasterGlyph {
    pub width: u32,
    pub height: u32,
    /// alpha 覆盖度（row-major，0~255）
    pub coverage: Vec<u8>,
    /// 基线到字形位图左上角的偏移（像素）
    pub bearing: [f32; 2],
    /// 水平推进量（像素）
    pub advance: f32,
}

/// 单字形度量（图集 UV + 排版信息，均为字体原生尺寸像素）
#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    /// 图集内 UV（u0, v0, u1, v1）
    pub uv: [f32; 4],
    /// 位图尺寸（像素）
    pub size: [f32; 2],
    /// 基线到字形左上角偏移（像素，y 向下为正）
    pub bearing: [f32; 2],
    /// 水平推进量（像素）
    pub advance: f32,
}

/// 字形图集：一次构建，多次渲染
///
/// 无内置图集缓存：字符集变更即整集重建并重新上传——
/// 增量/驻留等生命周期策略交由上层按场景自建。
pub struct GlyphAtlas {
    /// 已上传 GPU 的图集纹理（Rgba8Unorm，白色字形 + alpha）
    pub texture: Arc<Texture>,
    /// 字形度量表（空白字形只有 advance，无 UV）
    pub glyphs: HashMap<char, GlyphMetrics>,
    /// kern 字距表（像素；字符对 → 距离，通常为负值收紧间距）
    pub kerning: HashMap<(char, char), f32>,
    /// 上沿（基线到行顶，像素）
    pub ascent: f32,
    /// 下沿（基线到行底，像素，负值）
    pub descent: f32,
    /// 行高（像素）
    pub line_height: f32,
    /// 图集尺寸（像素）
    pub size: [u32; 2],
}

/// 字体（对位 pygame.font.Font）
pub struct Font {
    data: Vec<u8>,
    face: ttf_parser::Face<'static>,
    size: f32,
}

impl Font {
    /// 从文件加载字体
    pub fn from_file(path: &str, size: f32) -> Result<Self, FontError> {
        let data = std::fs::read(path).map_err(FontError::Io)?;
        Self::from_bytes(data, size)
    }

    /// 从内存字节加载
    pub fn from_bytes(data: Vec<u8>, size: f32) -> Result<Self, FontError> {
        let face = ttf_parser::Face::parse(&data, 0).map_err(FontError::Parse)?;
        // SAFETY：face 借用 data 的生命周期被抹为 'static。
        // data 由 Font 私有持有、永不被移动/修改，Face 无 Drop 实现（不会解引用悬垂切片），
        // 字段析构顺序不影响安全性。这是无 owned_ttf_parser 依赖下的标准做法。
        let face: ttf_parser::Face<'static> = unsafe { std::mem::transmute(face) };
        Ok(Self {
            data,
            face,
            size: size.max(1.0),
        })
    }

    /// 字体像素大小
    pub fn size(&self) -> f32 {
        self.size
    }

    /// 基线到行顶（像素）
    pub fn ascent(&self) -> f32 {
        self.face.ascender() as f32 * self.scale()
    }

    /// 基线到行底（像素，负值）
    pub fn descent(&self) -> f32 {
        self.face.descender() as f32 * self.scale()
    }

    /// 行高（像素）
    pub fn line_height(&self) -> f32 {
        (self.face.ascender() - self.face.descender() + self.face.line_gap()) as f32 * self.scale()
    }

    /// 字体单位 → 像素的缩放系数
    fn scale(&self) -> f32 {
        self.size / self.face.units_per_em() as f32
    }

    /// 查询两字符间的 kern 字距（像素；kern 表缺失/无记录返回 None）
    ///
    /// 仅覆盖 Apple/OpenType `kern` 表（GPOS PairPos 暂不解析——
    /// 少数字体仅含 GPOS kerning，此类字体字距退化为 0，不影响正确性）。
    fn kerning(&self, left: char, right: char) -> Option<f32> {
        let l = self.face.glyph_index(left)?;
        let r = self.face.glyph_index(right)?;
        let kern = self.face.tables().kern?;
        for subtable in kern.subtables {
            // 只取水平排版子表；状态机子表的 glyphs_kerning 恒为 None，跳过
            if !subtable.horizontal || subtable.has_state_machine {
                continue;
            }
            if let Some(v) = subtable.glyphs_kerning(l, r) {
                return Some(v as f32 * self.scale());
            }
        }
        None
    }

    /// 光栅化单个字符
    ///
    /// 空白字符（空格等）返回 None 位图但携带 advance；未知字形返回 None。
    pub fn rasterize_char(&self, ch: char) -> Option<RasterGlyph> {
        let id = self.face.glyph_index(ch)?;
        let s = self.scale();
        let advance = self.face.glyph_hor_advance(id).unwrap_or(0) as f32 * s;

        // 无轮廓字形（空格等）：只保留推进量
        let Some(bb) = self.face.glyph_bounding_box(id) else {
            return Some(RasterGlyph {
                width: 0,
                height: 0,
                coverage: Vec::new(),
                bearing: [0.0, 0.0],
                advance,
            });
        };

        let x0 = bb.x_min as f32 * s;
        let y_top = bb.y_max as f32 * s; // 基线以上高度
        let w = ((bb.x_max as f32 * s) - x0).ceil().max(1.0) as usize;
        let h = ((bb.y_max as f32 - bb.y_min as f32) * s).ceil().max(1.0) as usize;

        // 收集轮廓并转像素空间（origin = bbox 左上角，y 向下）
        let mut collector = raster::EdgeCollector::new();
        self.face.outline_glyph(id, &mut collector);
        let edges: Vec<raster::Edge> = collector
            .edges
            .into_iter()
            .map(|e| raster::Edge {
                x0: e.x0 * s - x0,
                y0: y_top - e.y0 * s,
                x1: e.x1 * s - x0,
                y1: y_top - e.y1 * s,
            })
            .collect();

        let coverage = raster::rasterize(&edges, w, h);
        Some(RasterGlyph {
            width: w as u32,
            height: h as u32,
            coverage,
            bearing: [x0, y_top],
            advance,
        })
    }

    /// 光栅化字符集并构建图集（上传 GPU）
    ///
    /// 空白字形只进度量表不进图集。图集为白色字形 + alpha，
    /// 渲染时由管线乘以目标颜色。
    pub fn build_atlas(
        &self,
        chars: impl IntoIterator<Item = char>,
        access: &RenderResourceAccess,
    ) -> Result<GlyphAtlas, FontError> {
        let mut chars: Vec<char> = chars.into_iter().collect();
        chars.sort();
        chars.dedup();

        // 光栅化全部字形
        let glyphs: Vec<(char, RasterGlyph)> = chars
            .iter()
            .filter_map(|&c| self.rasterize_char(c).map(|g| (c, g)))
            .collect();

        // shelf 打包（按行排列，2px 间距）
        const PAD: u32 = 2;
        const MAX_WIDTH: u32 = 1024;
        let mut x = 0u32;
        let mut y = 0u32;
        let mut row_h = 0u32;
        let mut atlas_w = 1u32;
        let mut placements: Vec<(char, &RasterGlyph, u32, u32)> = Vec::new();
        for (c, g) in &glyphs {
            if g.width == 0 || g.height == 0 {
                continue; // 空白字形不进图集
            }
            if x + g.width + PAD > MAX_WIDTH && x > 0 {
                x = 0;
                y += row_h + PAD;
                row_h = 0;
            }
            placements.push((*c, g, x, y));
            x += g.width + PAD;
            row_h = row_h.max(g.height);
            atlas_w = atlas_w.max(x);
        }
        let atlas_h = y + row_h + PAD;

        // 填充 RGBA8：白字形 + 覆盖度 alpha
        let mut pixels = image::RgbaImage::new(atlas_w, atlas_h.max(1));
        for (_, g, px, py) in &placements {
            for row in 0..g.height {
                for col in 0..g.width {
                    let a = g.coverage[(row * g.width + col) as usize];
                    if a > 0 {
                        pixels.put_pixel(px + col, py + row, image::Rgba([255, 255, 255, a]));
                    }
                }
            }
        }

        let desc = TextureDescriptor::new(
            TextureSemantic::Color,
            TextureUsage::Sampled,
            TextureDim::D2,
            Some(MipmapPolicy::Disabled),
            None,
        );
        let texture = Arc::new(access.create_texture("font_atlas", &ImageData::Rgba8(pixels), desc));

        // 度量表
        let w = atlas_w as f32;
        let h = atlas_h as f32;
        let mut metrics = HashMap::new();
        for (c, g, px, py) in &placements {
            metrics.insert(
                *c,
                GlyphMetrics {
                    uv: [
                        *px as f32 / w,
                        *py as f32 / h,
                        (*px + g.width) as f32 / w,
                        (*py + g.height) as f32 / h,
                    ],
                    size: [g.width as f32, g.height as f32],
                    bearing: g.bearing,
                    advance: g.advance,
                },
            );
        }
        // 空白字形（空格等）：只留推进量
        for (c, g) in &glyphs {
            if (g.width == 0 || g.height == 0) && !metrics.contains_key(c) {
                metrics.insert(
                    *c,
                    GlyphMetrics {
                        uv: [0.0; 4],
                        size: [0.0; 2],
                        bearing: g.bearing,
                        advance: g.advance,
                    },
                );
            }
        }

        // kern 字距表：仅在小字符集（≤512 字形）时全对查询——
        // 大字符集（如全量 CJK）kern 收益趋零且查询量爆炸，直接跳过
        let mut kerning_map = HashMap::new();
        if glyphs.len() <= 512 {
            for (lc, _) in &glyphs {
                for (rc, _) in &glyphs {
                    if let Some(k) = self.kerning(*lc, *rc) {
                        kerning_map.insert((*lc, *rc), k);
                    }
                }
            }
        }

        Ok(GlyphAtlas {
            texture,
            glyphs: metrics,
            kerning: kerning_map,
            ascent: self.ascent(),
            descent: self.descent(),
            line_height: self.line_height(),
            size: [atlas_w, atlas_h],
        })
    }
}

/// 本地空间文本布局：基线左端锚定在**原点**
///
/// 平移 / 旋转 / 缩放由开发者用矩阵施加（见 [`transform_vertices`]）——
/// font 不保存任何变换状态。
pub fn layout_text_local(
    atlas: &GlyphAtlas,
    text: &str,
    scale: f32,
    color: [f32; 4],
) -> Vec<TextVertex> {
    let mut out = Vec::new();
    let mut cursor = 0.0f32;
    let mut baseline = 0.0f32;
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if ch == '\n' {
            baseline += atlas.line_height * scale;
            cursor = 0.0;
            prev = None;
            continue;
        }
        let Some(m) = atlas.glyphs.get(&ch) else {
            continue;
        };
        if let Some(p) = prev {
            cursor += atlas.kerning.get(&(p, ch)).copied().unwrap_or(0.0);
        }
        if m.size[0] > 0.0 && m.size[1] > 0.0 {
            let x0 = (cursor + m.bearing[0]) * scale;
            let y0 = baseline - m.bearing[1] * scale;
            let w = m.size[0] * scale;
            let h = m.size[1] * scale;
            let [u0, v0, u1, v1] = m.uv;
            out.push(TextVertex { pos: [x0, y0, 0.0], uv: [u0, v0], color });
            out.push(TextVertex { pos: [x0 + w, y0, 0.0], uv: [u1, v0], color });
            out.push(TextVertex { pos: [x0, y0 + h, 0.0], uv: [u0, v1], color });
            out.push(TextVertex { pos: [x0 + w, y0, 0.0], uv: [u1, v0], color });
            out.push(TextVertex { pos: [x0 + w, y0 + h, 0.0], uv: [u1, v1], color });
            out.push(TextVertex { pos: [x0, y0 + h, 0.0], uv: [u0, v1], color });
        }
        cursor += m.advance;
        prev = Some(ch);
    }
    out
}

/// 矩阵变换：对顶点 `pos` 施加 4×4 矩阵（uv / color 透传）
///
/// 低级原语——矩阵的组合（平移 / 旋转 / 缩放）由开发者用 glam 自行完成。
pub fn transform_vertices(vertices: &[TextVertex], m: &glam::Mat4) -> Vec<TextVertex> {
    vertices
        .iter()
        .map(|v| {
            let p = m * glam::Vec4::new(v.pos[0], v.pos[1], v.pos[2], 1.0);
            TextVertex {
                pos: [p.x, p.y, p.z],
                uv: v.uv,
                color: v.color,
            }
        })
        .collect()
}

/// 文本布局：产出 `TextVertex` 顶点序列（6 顶点/字形，三角形序列，非索引）
///
/// * `pos`：首字形基线左端（2D 传 z=0；3D 传世界坐标）——纯平移；
///   需要旋转/缩放请用 [`layout_text_local`] + [`transform_vertices`]
/// * `color`：烘焙进顶点（管线乘制图集 alpha）
pub fn layout_text_with(
    glyphs: &HashMap<char, GlyphMetrics>,
    line_height: f32,
    kerning: &HashMap<(char, char), f32>,
    text: &str,
    pos: [f32; 3],
    scale: f32,
    color: [f32; 4],
) -> Vec<TextVertex> {
    layout_text_with_local(glyphs, line_height, kerning, text, scale, color)
        .into_iter()
        .map(|mut v| {
            v.pos = [v.pos[0] + pos[0], v.pos[1] + pos[1], v.pos[2] + pos[2]];
            v
        })
        .collect()
}

fn layout_text_with_local(
    glyphs: &HashMap<char, GlyphMetrics>,
    line_height: f32,
    kerning: &HashMap<(char, char), f32>,
    text: &str,
    scale: f32,
    color: [f32; 4],
) -> Vec<TextVertex> {
    let mut out = Vec::new();
    let mut cursor = 0.0f32;
    let mut baseline = 0.0f32;
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if ch == '\n' {
            baseline += line_height * scale;
            cursor = 0.0;
            prev = None;
            continue;
        }
        let Some(m) = glyphs.get(&ch) else {
            continue;
        };
        if let Some(p) = prev {
            cursor += kerning.get(&(p, ch)).copied().unwrap_or(0.0);
        }
        if m.size[0] > 0.0 && m.size[1] > 0.0 {
            let x0 = (cursor + m.bearing[0]) * scale;
            let y0 = baseline - m.bearing[1] * scale;
            let w = m.size[0] * scale;
            let h = m.size[1] * scale;
            let [u0, v0, u1, v1] = m.uv;
            out.push(TextVertex { pos: [x0, y0, 0.0], uv: [u0, v0], color });
            out.push(TextVertex { pos: [x0 + w, y0, 0.0], uv: [u1, v0], color });
            out.push(TextVertex { pos: [x0, y0 + h, 0.0], uv: [u0, v1], color });
            out.push(TextVertex { pos: [x0 + w, y0, 0.0], uv: [u1, v0], color });
            out.push(TextVertex { pos: [x0 + w, y0 + h, 0.0], uv: [u1, v1], color });
            out.push(TextVertex { pos: [x0, y0 + h, 0.0], uv: [u0, v1], color });
        }
        cursor += m.advance;
        prev = Some(ch);
    }
    out
}

/// [`Font::build_atlas`] 产出的图集的便捷布局入口
pub fn layout_text(
    atlas: &GlyphAtlas,
    text: &str,
    pos: [f32; 3],
    scale: f32,
    color: [f32; 4],
) -> Vec<TextVertex> {
    layout_text_with(&atlas.glyphs, atlas.line_height, &atlas.kerning, text, pos, scale, color)
}

/// 自定义排版入口（复杂文字系统的定制缝）
///
/// 阿拉伯/天城文等需要 shaping 的文字，请先经外部 shaper（如 harfbuzz）
/// 得到字形与绝对笔位置，再把 `(字符, 绝对基线位置)` 序列喂到这里——
/// 本函数只负责字形 → 顶点，不做任何排版假设。
pub fn layout_positioned(
    atlas: &GlyphAtlas,
    items: &[(char, [f32; 3])],
    scale: f32,
    color: [f32; 4],
) -> Vec<TextVertex> {
    let mut out = Vec::new();
    for (ch, p) in items {
        let Some(m) = atlas.glyphs.get(ch) else {
            continue;
        };
        if m.size[0] <= 0.0 || m.size[1] <= 0.0 {
            continue;
        }
        let x0 = p[0] + m.bearing[0] * scale;
        let y0 = p[1] - m.bearing[1] * scale;
        let z = p[2];
        let w = m.size[0] * scale;
        let h = m.size[1] * scale;
        let [u0, v0, u1, v1] = m.uv;
        out.push(TextVertex { pos: [x0, y0, z], uv: [u0, v0], color });
        out.push(TextVertex { pos: [x0 + w, y0, z], uv: [u1, v0], color });
        out.push(TextVertex { pos: [x0, y0 + h, z], uv: [u0, v1], color });
        out.push(TextVertex { pos: [x0 + w, y0, z], uv: [u1, v0], color });
        out.push(TextVertex { pos: [x0 + w, y0 + h, z], uv: [u1, v1], color });
        out.push(TextVertex { pos: [x0, y0 + h, z], uv: [u0, v1], color });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_PATH: &str = "resources/fonts/Antonio-Regular.ttf";

    #[test]
    fn font_metrics_and_raster() {
        let font = Font::from_file(FONT_PATH, 48.0).unwrap();
        assert!(font.line_height() > 0.0);
        assert!(font.ascent() > font.descent());

        let g = font.rasterize_char('A').expect("'A' 应有字形");
        assert!(g.width > 0 && g.height > 0);
        assert!(g.advance > 0.0);
        assert!(g.coverage.iter().any(|&a| a > 128), "应有实心像素");
        assert!(g.coverage.iter().any(|&a| a == 0), "字形外应留空");

        // 空格：无位图但有推进
        let sp = font.rasterize_char(' ').unwrap();
        assert_eq!(sp.width, 0);
        assert!(sp.advance > 0.0);
    }

    #[test]
    fn layout_produces_six_vertices_per_glyph() {
        let mut glyphs = HashMap::new();
        glyphs.insert(
            'a',
            GlyphMetrics {
                uv: [0.0, 0.0, 1.0, 1.0],
                size: [10.0, 10.0],
                bearing: [1.0, 8.0],
                advance: 12.0,
            },
        );

        let verts = layout_text_with(
            &glyphs,
            40.0,
            &HashMap::new(),
            "aa",
            [10.0, 20.0, 0.0],
            1.0,
            [1.0; 4],
        );
        assert_eq!(verts.len(), 12); // 2 字形 × 6 顶点

        // 第二个字形起点 = 第一个的推进量
        assert!((verts[6].pos[0] - (10.0 + 1.0 + 12.0)).abs() < 1e-5);
        // y = 基线 - bearing
        assert!((verts[0].pos[1] - (20.0 - 8.0)).abs() < 1e-5);
        // z 透传（3D 场景）
        assert_eq!(verts[0].pos[2], 0.0);
        // uv 完整覆盖 [0,1]
        assert_eq!(verts[0].uv[0], 0.0);
        assert_eq!(verts[1].uv[0], 1.0);
    }

    #[test]
    fn layout_applies_kerning() {
        let mut glyphs = HashMap::new();
        glyphs.insert(
            'a',
            GlyphMetrics {
                uv: [0.0, 0.0, 1.0, 1.0],
                size: [10.0, 10.0],
                bearing: [0.0, 8.0],
                advance: 12.0,
            },
        );
        let mut kerning = HashMap::new();
        kerning.insert(('a', 'a'), -2.0f32);

        let verts = layout_text_with(
            &glyphs,
            40.0,
            &kerning,
            "aa",
            [10.0, 20.0, 0.0],
            1.0,
            [1.0; 4],
        );
        // 第二个字形起点 = 前一推进量 + kern(-2)
        assert!(
            (verts[6].pos[0] - (10.0 + 12.0 - 2.0)).abs() < 1e-5,
            "x = {}",
            verts[6].pos[0]
        );
    }

    #[test]
    fn layout_newline_resets_cursor() {
        let mut glyphs = HashMap::new();
        glyphs.insert(
            'a',
            GlyphMetrics {
                uv: [0.0, 0.0, 1.0, 1.0],
                size: [10.0, 10.0],
                bearing: [0.0, 8.0],
                advance: 12.0,
            },
        );
        let verts = layout_text_with(
            &glyphs,
            40.0,
            &HashMap::new(),
            "a\na",
            [0.0, 20.0, 0.0],
            1.0,
            [1.0; 4],
        );
        assert_eq!(verts.len(), 12);
        // 第二行基线下移一行，x 归零
        assert!((verts[6].pos[0] - 0.0).abs() < 1e-5);
        assert!((verts[6].pos[1] - (20.0 + 40.0 - 8.0)).abs() < 1e-5);
    }
}
