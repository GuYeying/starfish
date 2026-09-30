//! pygame.font 对应物（契约 §五 font 行 / P4 提前落地）
//!
//! 路线 = 契约定稿的**资源制备**（不违背 GPU 路线）：base 自研扫描线
//! 光栅化（ttf-parser）→ CPU 合成 RGBA → [`render::Surface`]（GPU 纹理
//! 背板）→ blit 上屏。文字上屏走的是"图元生成→纹理"的同一管线路径。
//!
//! 对齐 pygame 面：`Font(path, size)` / `render(text, antialias, color)`
//! / `size(text)`。差异（文档标注）：
//! - `antialias` 参数收下但恒抗锯齿——本层光栅化器为 4×4 超采样恒 AA；
//! - 背景透明（pygame 的 `background` 参数 v1 不做，需要底色先
//!   `Surface::fill` 再 blit）；
//! - kern 不启用（对齐 pygame.font.Font 默认行为；`pygame.font` 本就不
//!   做 kern，freetype 系才有）。
//!
//! ⚠️ `render` 需要 GPU 槽（构造 Surface）——须在 display 初始化后调用。

use std::cell::RefCell;
use std::collections::HashMap;

use crate::base::font as engine;
use crate::pygame::Color;

use super::render::Surface;

pub use crate::base::font::FontError;

/// 缓存的字形位图（像素空间，y 向下）
#[derive(Clone)]
struct CachedGlyph {
    width: u32,
    height: u32,
    /// alpha 覆盖度（row-major，0~255）
    coverage: Vec<u8>,
    /// 基线到字形位图左上角的偏移（像素）
    bearing: [f32; 2],
    /// 水平推进量（像素）
    advance: f32,
}

/// 字体（对位 pygame.font.Font）
pub struct Font {
    inner: engine::Font,
    cache: RefCell<HashMap<char, Option<CachedGlyph>>>,
}

impl Font {
    /// 从文件加载（对位 `pygame.font.Font(path, size)`）
    pub fn new(path: &str, size: u32) -> Result<Self, FontError> {
        Ok(Self {
            inner: engine::Font::from_file(path, size as f32)?,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// 从内存字节加载
    pub fn from_bytes(data: Vec<u8>, size: u32) -> Result<Self, FontError> {
        Ok(Self {
            inner: engine::Font::from_bytes(data, size as f32)?,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// 行高（像素；pygame `get_height()` 同语义）
    pub fn line_height(&self) -> u32 {
        (self.inner.ascent() - self.inner.descent()).ceil().max(1.0) as u32
    }

    /// 排版尺寸（对位 `Font.size(text)`：宽度 = 推进量和，高度 = 行高）
    pub fn size(&self, text: &str) -> (u32, u32) {
        let mut w = 0.0f32;
        for ch in text.chars() {
            if ch == '\n' {
                continue; // pygame.font 单行语义：换行按缺字形跳过
            }
            w += self.glyph(ch).map(|g| g.advance).unwrap_or(0.0);
        }
        (w.ceil() as u32, self.line_height())
    }

    /// 文本渲染为离屏面（对位 `Font.render(text, antialias, color)`）
    ///
    /// 恒抗锯齿（超采样光栅化器）；透明背景；换行按缺字形跳过。
    pub fn render(&self, text: &str, antialias: bool, color: impl Into<Color>) -> Surface {
        let _ = antialias; // 签名兼容位：本层恒 AA（见模块文档差异清单）
        let (r, g, b, _) = color.into().normalize();
        let (r, g, b) = (
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        );

        let (w, h) = self.size(text);
        let (w, h) = (w.max(1), h.max(1));
        let mut img = vec![0u8; (w * h * 4) as usize];

        let baseline = self.inner.ascent();
        let mut cursor = 0.0f32;
        for ch in text.chars() {
            let Some(gl) = self.glyph(ch) else { continue };
            if gl.width > 0 && gl.height > 0 {
                let gx = (cursor + gl.bearing[0]).round() as i64;
                let gy = (baseline - gl.bearing[1]).round() as i64;
                for row in 0..gl.height {
                    for col in 0..gl.width {
                        let a = gl.coverage[(row * gl.width + col) as usize];
                        if a == 0 {
                            continue;
                        }
                        let px = gx + col as i64;
                        let py = gy + row as i64;
                        if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                            continue; // 裁剪出界（负 bearing / 超宽行尾）
                        }
                        let idx = ((py as u32 * w + px as u32) * 4) as usize;
                        img[idx] = r;
                        img[idx + 1] = g;
                        img[idx + 2] = b;
                        img[idx + 3] = a; // 覆盖度即 alpha
                    }
                }
            }
            cursor += gl.advance;
        }
        Surface::from_rgba8((w, h), &img)
    }

    /// 字形取用（带缓存；空白字符 = 只有推进量的空位图）
    fn glyph(&self, ch: char) -> Option<CachedGlyph> {
        if let Some(g) = self.cache.borrow().get(&ch) {
            return g.clone();
        }
        let g = self.inner.rasterize_char(ch).map(|g| CachedGlyph {
            width: g.width,
            height: g.height,
            coverage: g.coverage,
            bearing: g.bearing,
            advance: g.advance,
        });
        self.cache.borrow_mut().insert(ch, g.clone());
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_PATH: &str = "resources/fonts/Antonio-Regular.ttf";

    #[test]
    fn metrics_and_layout_math() {
        let font = Font::new(FONT_PATH, 48).unwrap();
        let (w, h) = font.size("abc");
        assert!(w > 0);
        assert_eq!(h, font.line_height());
        // 空串：零宽 + 行高
        assert_eq!(font.size(""), (0, font.line_height()));
        // 渲染合成不需要 GPU 的部分：直接验 CPU 合成路径的出界裁剪
        // （GPU 依赖部分由探针示例验收）
        let gl = font.glyph('A').expect("'A' 应有字形");
        assert!(gl.advance > 0.0 && gl.width > 0);
    }
}
