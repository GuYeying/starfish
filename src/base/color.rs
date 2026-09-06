use wgpu;


//这个是渲染引擎底层的颜色类型，也就是说他不需要和pygame的进行双向绑定！

/// 渲染专用浮点色，对应wgpu底层f64色彩，支持HDR
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    /// 基础构造
    pub fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
        Self { r, g, b, a }
    }

    /// RGB 默认 a=1.0
    pub fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self::new(r, g, b, 1.0)
    }

    /// 从0~255 u8四通道字节构造浮点色（自动转0.0~1.0）
    pub fn from_byte_tuple(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f64 / 255.0,
            g: g as f64 / 255.0,
            b: b as f64 / 255.0,
            a: a as f64 / 255.0,
        }
    }

    /// 转为0~255 u8四元组，超出0~1自动钳位并四舍五入
    pub fn to_byte_tuple(&self) -> (u8, u8, u8, u8) {
        fn f64_to_u8(v: f64) -> u8 {
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        }
        (
            f64_to_u8(self.r),
            f64_to_u8(self.g),
            f64_to_u8(self.b),
            f64_to_u8(self.a),
        )
    }

    /// 从上层 pygame::color::Color(u8) 转换
    pub fn from_byte_color(byte_color: &crate::pygame::color::Color) -> Self {
        Self::from_byte_tuple(byte_color.r, byte_color.g, byte_color.b, byte_color.a)
    }

    /// 转回上层字节色（超出0~1会截断）
    pub fn to_byte_color(&self) -> crate::pygame::color::Color {
        let (r, g, b, a) = self.to_byte_tuple();
        crate::pygame::color::Color::new(r, g, b, a)
    }

    /// 亮度缩放（HDR发光核心接口）
    pub fn scale_brightness(mut self, factor: f64) -> Self {
        self.r *= factor;
        self.g *= factor;
        self.b *= factor;
        self
    }

    /// 浮点插值 lerp
    pub fn lerp(self, other: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self {
            r: self.r * inv + other.r * t,
            g: self.g * inv + other.g * t,
            b: self.b * inv + other.b * t,
            a: self.a * inv + other.a * t,
        }
    }

    /// 转f64四元组对外暴露，上层仅操作元组，不接触wgpu
    pub fn to_tuple(&self) -> (f64, f64, f64, f64) {
        (self.r, self.g, self.b, self.a)
    }

    // ========== 仅本crate内部使用：转wgpu原生颜色 ==========
    pub(crate) fn to_wgpu(self) -> wgpu::Color {
        wgpu::Color {
            r: self.r,
            g: self.g,
            b: self.b,
            a: self.a,
        }
    }
}

// ========== 运算符重载 ==========
// 颜色 * 亮度标量
impl std::ops::Mul<f64> for Color {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self::Output {
        Self {
            r: self.r * rhs,
            g: self.g * rhs,
            b: self.b * rhs,
            a: self.a * rhs,
        }
    }
}

impl std::ops::MulAssign<f64> for Color {
    fn mul_assign(&mut self, rhs: f64) {
        self.r *= rhs;
        self.g *= rhs;
        self.b *= rhs;
        self.a *= rhs;
    }
}

// 颜色相加（光照叠加）
impl std::ops::Add for Color {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r + rhs.r,
            g: self.g + rhs.g,
            b: self.b + rhs.b,
            a: self.a + rhs.a,
        }
    }
}

impl std::ops::AddAssign for Color {
    fn add_assign(&mut self, rhs: Self) {
        self.r += rhs.r;
        self.g += rhs.g;
        self.b += rhs.b;
        self.a += rhs.a;
    }
}

// 颜色相减
impl std::ops::Sub for Color {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            r: self.r - rhs.r,
            g: self.g - rhs.g,
            b: self.b - rhs.b,
            a: self.a - rhs.a,
        }
    }
}

impl std::ops::SubAssign for Color {
    fn sub_assign(&mut self, rhs: Self) {
        self.r -= rhs.r;
        self.g -= rhs.g;
        self.b -= rhs.b;
        self.a -= rhs.a;
    }
}