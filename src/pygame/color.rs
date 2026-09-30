use std::collections::HashMap;
use std::ops::{Add, Div, Index, IndexMut, Mul, Rem, Sub};
/// 标准颜色常量映射 (THECOLORS)
use once_cell::sync::OnceCell;
use serde::Deserialize;


use crate::base::color::Color as F64Color;

/// 全局颜色存储，仅初始化一次
static THECOLORS_STORE: OnceCell<HashMap<String, (u8, u8, u8, u8)>> = OnceCell::new();

/// JSON 反序列化模型
#[derive(Debug, Deserialize)]
pub struct ColorJsonItem {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}


/// 一次性初始化全局颜色表
/// json_str: 标准JSON字符串，格式 {"颜色名": {"r":255,"g":0,"b":0,"a":255}, ... }
/// append_base: 是否追加 red/green/blue/white/black 基础内置色
/// 返回 Ok(()) 成功；Err 两种情况：已初始化 / JSON解析失败
pub fn init_colors(json_str: &str, append_base: bool) -> Result<(), String> {
    // 已初始化直接返回错误
    if THECOLORS_STORE.get().is_some() {
        return Err("THECOLORS already initialized, cannot re-init".to_string());
    }

    // 1. 解析JSON
    let custom_map: HashMap<String, ColorJsonItem> = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    // 2. 构建完整颜色字典
    let mut full_map = HashMap::new();

    // 可选追加基础内置色
    if append_base {
        full_map.insert("red".to_string(), (255, 0, 0, 255));
        full_map.insert("green".to_string(), (0, 255, 0, 255));
        full_map.insert("blue".to_string(), (0, 0, 255, 255));
        full_map.insert("white".to_string(), (255, 255, 255, 255));
        full_map.insert("black".to_string(), (0, 0, 0, 255));
    }

    // 3. 合并JSON解析出来的自定义颜色
    for (name, item) in custom_map {
        full_map.insert(name, (item.r, item.g, item.b, item.a));
    }

    // 存入全局单例
    THECOLORS_STORE
        .set(full_map)
        .map_err(|_| "Set global THECOLORS failed".to_string())?;

    Ok(())
}




// #[derive(Debug, Clone, Copy, PartialEq)]
// pub struct FloatColor {
//     pub r: f64,
//     pub g: f64,
//     pub b: f64,
//     pub a: f64,
// }

// impl FloatColor {
//     // 从配置字节色转浮点(0~1)
//     pub fn from_color(c: Color) -> Self {
//         let (r,g,b,a) = c.to_f64();
//         Self { r, g, b, a }
//     }

//     // 支持HDR，亮度放大
//     pub fn scale_brightness(mut self, factor: f64) -> Self {
//         self.r *= factor;
//         self.g *= factor;
//         self.b *= factor;
//         self
//     }

//     // 转回字节色（自动钳位0~1，超出部分截断）
//     pub fn to_color(&self) -> Color {
//         Color::from_f64(self.r, self.g, self.b, self.a)
//     }
// }



#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,

    // 缓存派生色彩空间，None = 未计算
    cmy_cache: Option<(f32, f32, f32)>,
    hsva_cache: Option<(f32, f32, f32, f32)>,
    hsla_cache: Option<(f32, f32, f32, f32)>,
    i1i2i3_cache: Option<(f32, f32, f32)>,
}

// 构造重载，对应 Python 多 __init__
impl Color {
    /// 四通道 u8 构造 r,g,b,a=255
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            cmy_cache: None,
            hsva_cache: None,
            hsla_cache: None,
            i1i2i3_cache: None,
        }
    }

    /// 简化构造，默认 alpha=255
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }

    /// 从元组 (r,g,b) / (r,g,b,a) 构造
    pub fn from_tuple<T: Into<(u8, u8, u8)>>(t: T) -> Self {
        let (r, g, b) = t.into();
        Self::rgb(r, g, b)
    }

    pub fn from_tuple_a<T: Into<(u8, u8, u8, u8)>>(t: T) -> Self {
        let (r, g, b, a) = t.into();
        Self::new(r, g, b, a)
    }

    /// 从颜色名字符串构造（匹配 THECOLORS）。
    /// 颜色表未初始化时返回 None（适配改动：原实现 panic）；
    /// 先经 [`init_colors`] 装载颜色表。
    pub fn from_name(name: &str) -> Option<Self> {
        THECOLORS_STORE
            .get()
            .and_then(|map| map.get(name))
            .map(|&(r, g, b, a)| Self::new(r, g, b, a))
    }

    /// 清空所有派生色彩缓存（修改通道后自动调用）
    fn clear_cache(&mut self) {
        self.cmy_cache = None;
        self.hsva_cache = None;
        self.hsla_cache = None;
        self.i1i2i3_cache = None;
    }

    /// 更新颜色（对应 Python update 重载）
    pub fn update(&mut self, r: u8, g: u8, b: u8, a: u8) {
        self.r = r;
        self.g = g;
        self.b = b;
        self.a = a;
        self.clear_cache();
    }

    pub fn update_rgb(&mut self, r: u8, g: u8, b: u8) {
        self.update(r, g, b, self.a);
    }

    /// 标准化转 0.0 ~ 1.0 f32 四元组
    pub fn normalize(&self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        )
    }

    /// 伽马矫正
    pub fn correct_gamma(&self, gamma: f32) -> Self {
        let (r, g, b, _a) = self.normalize();
        let pow = |v: f32| v.powf(gamma);
        Self::new(
            (pow(r) * 255.0).clamp(0.0, 255.0) as u8,
            (pow(g) * 255.0).clamp(0.0, 255.0) as u8,
            (pow(b) * 255.0).clamp(0.0, 255.0) as u8,
            self.a,
        )
    }

    /// 限制通道数量（set_length）
    pub fn set_length(&mut self, len: usize) {
        match len {
            1 => {
                let gray = self.grayscale().r;
                self.update(gray, gray, gray, 255);
            }
            3 => self.a = 255,
            4 => {}
            _ => panic!("Color length only support 1/3/4"),
        }
        self.clear_cache();
    }

    /// 颜色插值 lerp
    pub fn lerp(&self, other: &Color, amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self::new(
            ((self.r as f32 * inv + other.r as f32 * t).round() as u8).clamp(0, 255),
            ((self.g as f32 * inv + other.g as f32 * t).round() as u8).clamp(0, 255),
            ((self.b as f32 * inv + other.b as f32 * t).round() as u8).clamp(0, 255),
            ((self.a as f32 * inv + other.a as f32 * t).round() as u8).clamp(0, 255),
        )
    }

    /// Alpha 预乘
    pub fn premul_alpha(&self) -> Self {
        let a = self.a as f32 / 255.0;
        Self::new(
            (self.r as f32 * a).round() as u8,
            (self.g as f32 * a).round() as u8,
            (self.b as f32 * a).round() as u8,
            self.a,
        )
    }

    /// 灰度图
    pub fn grayscale(&self) -> Self {
        let gray = (0.299 * self.r as f32 + 0.587 * self.g as f32 + 0.114 * self.b as f32)
            .round() as u8;
        Self::new(gray, gray, gray, self.a)
    }

    /// 取反 !color
    pub fn invert(&self) -> Self {
        Self::new(255 - self.r, 255 - self.g, 255 - self.b, self.a)
    }

    // -------------------- 派生色彩空间 Getter（惰性缓存） --------------------
    pub fn cmy(&mut self) -> (f32, f32, f32) {
        if self.cmy_cache.is_none() {
            let (r, g, b, _) = self.normalize();
            self.cmy_cache = Some((1.0 - r, 1.0 - g, 1.0 - b));
        }
        self.cmy_cache.unwrap()
    }

    pub fn hsva(&mut self) -> (f32, f32, f32, f32) {
        if self.hsva_cache.is_none() {
            let (r, g, b, a) = self.normalize();
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let delta = max - min;
            let mut h = 0.0;
            let s = if max == 0.0 { 0.0 } else { delta / max };
            let v = max;

            if delta != 0.0 {
                h = match max {
                    m if m == r => ((g - b) / delta) % 6.0,
                    m if m == g => ((b - r) / delta) + 2.0,
                    _ => ((r - g) / delta) + 4.0,
                };
                h *= 60.0;
                if h < 0.0 {
                    h += 360.0;
                }
            }
            self.hsva_cache = Some((h, s, v, a));
        }
        self.hsva_cache.unwrap()
    }

    pub fn hsla(&mut self) -> (f32, f32, f32, f32) {
        if self.hsla_cache.is_none() {
            let (r, g, b, a) = self.normalize();
            let max = r.max(g).max(b);
            let min = r.min(g).min(b);
            let l = (max + min) / 2.0;
            let delta = max - min;
            let mut h = 0.0;
            let s = if delta == 0.0 {
                0.0
            } else {
                if l < 0.5 {
                    delta / (max + min)
                } else {
                    delta / (2.0 - max - min)
                }
            };

            if delta != 0.0 {
                h = match max {
                    m if m == r => ((g - b) / delta) % 6.0,
                    m if m == g => ((b - r) / delta) + 2.0,
                    _ => ((r - g) / delta) + 4.0,
                };
                h *= 60.0;
                if h < 0.0 {
                    h += 360.0;
                }
            }
            self.hsla_cache = Some((h, s, l, a));
        }
        self.hsla_cache.unwrap()
    }

    pub fn i1i2i3(&mut self) -> (f32, f32, f32) {
        if self.i1i2i3_cache.is_none() {
            let (r, g, b, _) = self.normalize();
            let i1 = (r + g + b) / 3.0;
            let i2 = (r - b) / 2.0;
            let i3 = (2.0 * g - r - b) / 4.0;
            self.i1i2i3_cache = Some((i1, i2, i3));
        }
        self.i1i2i3_cache.unwrap()
    }
}

// ==================== 下标访问 [idx] ====================
impl Index<usize> for Color {
    type Output = u8;
    fn index(&self, idx: usize) -> &Self::Output {
        match idx {
            0 => &self.r,
            1 => &self.g,
            2 => &self.b,
            3 => &self.a,
            _ => panic!("Color index out of range, only 0-3"),
        }
    }
}

impl IndexMut<usize> for Color {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        self.clear_cache();
        match idx {
            0 => &mut self.r,
            1 => &mut self.g,
            2 => &mut self.b,
            3 => &mut self.a,
            _ => panic!("Color index out of range, only 0-3"),
        }
    }
}

// ==================== 迭代 IntoIterator ====================
impl IntoIterator for Color {
    type Item = u8;
    type IntoIter = std::array::IntoIter<u8, 4>;
    fn into_iter(self) -> Self::IntoIter {
        [self.r, self.g, self.b, self.a].into_iter()
    }
}

// ==================== 运算符重载 ====================
impl Add for Color {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(
            self.r.saturating_add(rhs.r),
            self.g.saturating_add(rhs.g),
            self.b.saturating_add(rhs.b),
            self.a.saturating_add(rhs.a),
        )
    }
}

impl Sub for Color {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(
            self.r.saturating_sub(rhs.r),
            self.g.saturating_sub(rhs.g),
            self.b.saturating_sub(rhs.b),
            self.a.saturating_sub(rhs.a),
        )
    }
}

impl Mul for Color {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Self::new(
            ((self.r as u16 * rhs.r as u16) / 255) as u8,
            ((self.g as u16 * rhs.g as u16) / 255) as u8,
            ((self.b as u16 * rhs.b as u16) / 255) as u8,
            ((self.a as u16 * rhs.a as u16) / 255) as u8,
        )
    }
}

impl Div for Color {
    type Output = Self;
    fn div(self, rhs: Self) -> Self::Output {
        let div = |a: u8, b: u8| if b == 0 { 0 } else { (a as u16 * 255 / b as u16) as u8 };
        Self::new(div(self.r, rhs.r), div(self.g, rhs.g), div(self.b, rhs.b), div(self.a, rhs.a))
    }
}

impl Rem for Color {
    type Output = Self;
    fn rem(self, rhs: Self) -> Self::Output {
        Self::new(self.r % rhs.r, self.g % rhs.g, self.b % rhs.b, self.a % rhs.a)
    }
}

// 取反 !color
impl std::ops::Not for Color {
    type Output = Self;
    fn not(self) -> Self::Output {
        self.invert()
    }
}

// ==================== 类型转换 ====================
impl From<Color> for i32 {
    fn from(c: Color) -> Self {
        ((c.r as i32) << 24) | ((c.g as i32) << 16) | ((c.b as i32) << 8) | c.a as i32
    }
}

impl From<Color> for f32 {
    fn from(c: Color) -> Self {
        let (r, g, b, a) = c.normalize();
        r + g + b + a
    }
}

impl Color {
    pub fn len(&self) -> usize {
        4
    }

    pub fn contains(&self, value: u8) -> bool {
        self.r == value || self.g == value || self.b == value || self.a == value
    }


    /// 输出 0.0~1.0 f64 四元组，用于转渲染浮点色
    pub fn to_f64_tuple(&self) -> (f64, f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
            self.a as f64 / 255.0,
        )
    }

    /// 从浮点元组转回字节色（自动钳位 0~1）
    pub fn from_f64_tuple(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self::new(Color::to_byte(r), Color::to_byte(g), Color::to_byte(b), Color::to_byte(a))
    }


    #[inline]
    pub(crate) fn to_f64_color(&self)->F64Color{
        F64Color::new(
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
            self.a as f64 / 255.0,
        )
    }

    #[inline]
    pub(crate) fn from_f64_color(&self,f64_color:F64Color)->Self{
        Color::new(
            Color::to_byte(f64_color.r),
            Color::to_byte(f64_color.g),
            Color::to_byte(f64_color.b),
            Color::to_byte(f64_color.a),
        )
    }

    #[inline]
    fn to_byte(v: f64) -> u8 {
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

// ==================== Hash 支持 ====================
impl std::hash::Hash for Color {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.r.hash(state);
        self.g.hash(state);
        self.b.hash(state);
        self.a.hash(state);
    }
}

// ==================== 契约适配：Into<Color> 转换家族 ====================
// 契约 API（fill/draw::*/set_at）统一收 `impl Into<Color>`：
// 数组/元组/u32 字面量即颜色。默认 alpha=255（无 alpha 形式）。

impl From<[u8; 3]> for Color {
    fn from(c: [u8; 3]) -> Self {
        Self::rgb(c[0], c[1], c[2])
    }
}

impl From<[u8; 4]> for Color {
    fn from(c: [u8; 4]) -> Self {
        Self::new(c[0], c[1], c[2], c[3])
    }
}

impl From<(u8, u8, u8)> for Color {
    fn from(c: (u8, u8, u8)) -> Self {
        Self::rgb(c.0, c.1, c.2)
    }
}

impl From<(u8, u8, u8, u8)> for Color {
    fn from(c: (u8, u8, u8, u8)) -> Self {
        Self::new(c.0, c.1, c.2, c.3)
    }
}

impl From<(i32, i32, i32)> for Color {
    fn from(c: (i32, i32, i32)) -> Self {
        Self::rgb(c.0.clamp(0, 255) as u8, c.1.clamp(0, 255) as u8, c.2.clamp(0, 255) as u8)
    }
}

impl From<(i32, i32, i32, i32)> for Color {
    fn from(c: (i32, i32, i32, i32)) -> Self {
        Self::new(
            c.0.clamp(0, 255) as u8,
            c.1.clamp(0, 255) as u8,
            c.2.clamp(0, 255) as u8,
            c.3.clamp(0, 255) as u8,
        )
    }
}

/// f32 归一化色（0.0~1.0，引擎/glam 惯例；越界钳制）→ Color
impl From<[f32; 4]> for Color {
    fn from(c: [f32; 4]) -> Self {
        let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self::new(ch(c[0]), ch(c[1]), ch(c[2]), ch(c[3]))
    }
}

impl From<(f32, f32, f32, f32)> for Color {
    fn from(c: (f32, f32, f32, f32)) -> Self {
        Self::from([c.0, c.1, c.2, c.3])
    }
}

/// 0xRRGGBB（alpha=255）/ 0xRRGGBBAA
impl From<u32> for Color {
    fn from(v: u32) -> Self {
        if v <= 0xFF_FFFF {
            Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
        } else {
            Self::new((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
        }
    }
}

impl From<Color> for [u8; 4] {
    fn from(c: Color) -> Self {
        [c.r, c.g, c.b, c.a]
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construct_and_normalize() {
        let c = Color::rgb(255, 128, 0);
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 0, 255));
        assert_eq!(c.normalize(), (1.0, 128.0 / 255.0, 0.0, 1.0));
    }

    #[test]
    fn into_from_family() {
        let a: Color = [10u8, 20, 30].into();
        assert_eq!((a.r, a.g, a.b, a.a), (10, 20, 30, 255));
        let b: Color = (10u8, 20, 30, 40).into();
        assert_eq!((b.r, b.g, b.b, b.a), (10, 20, 30, 40));
        let c: Color = 0xFF_80_40u32.into(); // RGB → alpha=255
        assert_eq!((c.r, c.g, c.b, c.a), (255, 128, 64, 255));
        let d: Color = 0x80_40_20_10u32.into(); // RGBA
        assert_eq!((d.r, d.g, d.b, d.a), (128, 64, 32, 16));
        let bytes: [u8; 4] = c.into();
        assert_eq!(bytes, [255, 128, 64, 255]);
    }

    #[test]
    fn operators_and_index() {
        let a = Color::rgb(100, 200, 50);
        let b = a + Color::rgb(100, 100, 100);
        assert_eq!(b[0], 200); // 加法通道饱和由实现语义决定
        assert_eq!(a[0], 100);
        let mut c = a;
        c[1] = 10;
        assert_eq!(c.g, 10);
    }

    #[test]
    fn thecolors_requires_init_then_loads() {
        // 初始化一次（若已初始化则 Ok/Err 均可接受——幂等探针）
        let json = r#"{"red": {"r":255,"g":0,"b":0,"a":255}}"#;
        let _ = init_colors(json, true);
        assert!(Color::from_name("red").is_some());
        assert!(Color::from_name("nonexistent").is_none());
    }
}
