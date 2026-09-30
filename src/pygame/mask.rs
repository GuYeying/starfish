//! pygame.mask 对应物（契约 §五 mask 行：GPU 管线产出、CPU 检测）
//!
//! **业界对照**（定案依据）：Godot BitMap（贴图阈值→位图，点击/贴图
//! 遮罩）与 pygame Mask 同属 **CPU 位图**路线——像素级碰撞检测按帧走
//! GPU 回读不现实，业界通行做法 = 资产装载期一次性生成位图 + 运行期
//! 纯 CPU 比对；Unity SpriteMask 类的**视觉遮罩**是另一族概念
//! （stencil/alpha 管线特性，v2 随自定义材质开放，与本模块无关）。
//!
//! 本层落位：[`Mask::from_surface_async`] = 一次性 GPU 回读（复用
//! image::save 的回读链路，慢路径只在构建期）→ alpha 阈值位图 →
//! [`overlap`]/[`Mask::overlap_area`] 等纯 CPU 检测（pygame 全套语义）。

use crate::pygame::render::{DrawTarget, Surface};
use crate::pygame::Rect;

/// 像素位掩码（1 bit/像素；碰撞与命中检测用）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    width: u32,
    height: u32,
    bits: Vec<bool>,
}

impl Mask {
    /// 纯色掩码（对位 `Mask(size, fill)`）
    pub fn new(size: (u32, u32), fill: bool) -> Self {
        Self {
            width: size.0,
            height: size.1,
            bits: vec![fill; (size.0 as usize) * (size.1 as usize)],
        }
    }

    /// 从离屏面构建（异步全平台；alpha > threshold 的像素置位）
    ///
    /// 一次性回读慢路径——建议资产装载期构建、缓存复用。
    pub async fn from_surface_async(s: &Surface, threshold: u8) -> Result<Self, crate::pygame::image::ImageError> {
        let rgba = crate::pygame::image::readback_rgba(s).await?;
        let (w, h) = s.size();
        let mut bits = Vec::with_capacity((w * h) as usize);
        for px in rgba.chunks_exact(4) {
            bits.push(px[3] > threshold);
        }
        Ok(Self { width: w, height: h, bits })
    }

    /// [`from_surface_async`] 的桌面同步壳
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_surface(s: &Surface, threshold: u8) -> Result<Self, crate::pygame::image::ImageError> {
        crate::base::app::block_on(Self::from_surface_async(s, threshold))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 像素位（越界 = false）
    pub fn get_at(&self, x: u32, y: u32) -> bool {
        if x < self.width && y < self.height {
            self.bits[(y * self.width + x) as usize]
        } else {
            false
        }
    }

    /// 写像素位（越界忽略）
    pub fn set_at(&mut self, x: u32, y: u32, value: bool) {
        if x < self.width && y < self.height {
            let i = (y * self.width + x) as usize;
            self.bits[i] = value;
        }
    }

    /// 置位像素数（对位 `Mask.count()`）
    pub fn count(&self) -> u32 {
        self.bits.iter().filter(|&&b| b).count() as u32
    }

    /// 全部置位/清零
    pub fn fill(&mut self, value: bool) {
        self.bits.iter_mut().for_each(|b| *b = value);
    }

    /// 反相
    pub fn invert(&mut self) {
        self.bits.iter_mut().for_each(|b| *b = !*b);
    }

    /// 与 other 的首个重叠点（自坐标；无重叠 = None；对位 `overlap`）
    ///
    /// `offset` = other 相对 self 的左上角偏移。逐位扫描序（行优先）。
    pub fn overlap(&self, other: &Mask, offset: (i32, i32)) -> Option<(i32, i32)> {
        let (x0, y0) = (offset.0.max(0) as u32, offset.1.max(0) as u32);
        let xe = self.width.min(x0 + other.width);
        let ye = self.height.min(y0 + other.height);
        for y in y0..ye {
            for x in x0..xe {
                let ox = (x - x0) as i32;
                let oy = (y - y0) as i32;
                if self.get_at(x, y)
                    && other.get_at(ox as u32, oy as u32)
                {
                    return Some((x as i32, y as i32));
                }
            }
        }
        None
    }

    /// 重叠像素总数（对位 `overlap_area`）
    pub fn overlap_area(&self, other: &Mask, offset: (i32, i32)) -> u32 {
        let (x0, y0) = (offset.0.max(0) as u32, offset.1.max(0) as u32);
        let xe = self.width.min(x0 + other.width);
        let ye = self.height.min(y0 + other.height);
        let mut n = 0;
        for y in y0..ye {
            for x in x0..xe {
                if self.get_at(x, y) && other.get_at((x - x0) as u32, (y - y0) as u32) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 重叠区域掩码（self ∧ other；对位 `overlap_mask`）
    pub fn overlap_mask(&self, other: &Mask, offset: (i32, i32)) -> Mask {
        let mut m = Mask::new((self.width, self.height), false);
        let (x0, y0) = (offset.0.max(0) as u32, offset.1.max(0) as u32);
        let xe = self.width.min(x0 + other.width);
        let ye = self.height.min(y0 + other.height);
        for y in y0..ye {
            for x in x0..xe {
                if self.get_at(x, y) && other.get_at((x - x0) as u32, (y - y0) as u32) {
                    m.set_at(x, y, true);
                }
            }
        }
        m
    }

    /// 最近邻缩放（对位 `Mask.scale`）
    pub fn scale(&self, size: (u32, u32)) -> Mask {
        let mut m = Mask::new(size, false);
        for y in 0..size.1 {
            for x in 0..size.0 {
                let sx = x * self.width / size.0.max(1);
                let sy = y * self.height / size.1.max(1);
                m.set_at(x, y, self.get_at(sx, sy));
            }
        }
        m
    }

    /// 渲染为可视化 Surface（置位 = 白色不透明，未置 = 透明；
    /// GPU 路径，调试预览用）
    pub fn to_surface(&self) -> Surface {
        let mut px = Vec::with_capacity(self.bits.len() * 4);
        for b in &self.bits {
            let v = if *b { 255 } else { 0 };
            px.extend_from_slice(&[v, v, v, v]);
        }
        Surface::from_rgba8((self.width, self.height), &px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot_mask() -> Mask {
        let mut m = Mask::new((4, 4), false);
        m.set_at(2, 2, true);
        m
    }

    #[test]
    fn count_invert_scale() {
        let mut m = Mask::new((4, 4), false);
        assert_eq!(m.count(), 0);
        m.set_at(0, 0, true);
        m.set_at(3, 3, true);
        assert_eq!(m.count(), 2);
        assert!(m.get_at(3, 3));
        assert!(!m.get_at(9, 9), "越界恒 false");
        m.invert();
        assert_eq!(m.count(), 14);
        let s = m.scale((2, 2));
        assert_eq!((s.size().0, s.size().1), (2, 2));
    }

    #[test]
    fn overlap_semantics() {
        let a = dot_mask(); // 位 (2,2)
        let mut b = dot_mask();
        // 偏移 (1,1)：b 的位落在 a 的 (3,3)——不重叠
        assert_eq!(a.overlap(&b, (1, 1)), None);
        assert_eq!(a.overlap_area(&b, (1, 1)), 0);
        // 偏移 0：两位同落 (2,2)——重叠
        assert_eq!(a.overlap(&b, (0, 0)), Some((2, 2)));
        assert_eq!(a.overlap_area(&b, (0, 0)), 1);
        let om = a.overlap_mask(&b, (0, 0));
        assert_eq!(om.count(), 1);
        b.fill(true);
        assert_eq!(a.overlap_area(&b, (0, 0)), 1, "重叠 = 双方置位的交集");
    }
}
