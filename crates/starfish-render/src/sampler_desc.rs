// 禁止顶层导入wgpu过滤/寻址枚举，避免和自研枚举冲突

use std::hash::{Hash, Hasher};

pub use wgpu::{AddressMode, FilterMode, MipmapFilterMode,CompareFunction};

#[derive(Debug, Clone, PartialEq)]
pub struct SamplerDescriptor {
    // 基础过滤
    min_filter: FilterMode,
    mag_filter: FilterMode,
    mipmap_filter: MipmapFilterMode,

    // UVW寻址
    address_u: AddressMode,
    address_v: AddressMode,
    address_w: AddressMode,

    // 深度比较（阴影专用）
    compare: Option<CompareFunction>,

    // 各向异性等级
    anisotropy_clamp: u16,

    // 边界颜色（仅 ClampToBorder 生效）
    border_white: bool,
}

// 实现 Hash 用于缓存池 key
impl Hash for SamplerDescriptor {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.min_filter.hash(state);
        self.mag_filter.hash(state);
        self.mipmap_filter.hash(state);
        self.address_u.hash(state);
        self.address_v.hash(state);
        self.address_w.hash(state);
        self.compare.hash(state);
        self.anisotropy_clamp.hash(state);
        self.border_white.hash(state);
    }
}

impl SamplerDescriptor {
    /// 默认：线性过滤 + 三轴重复，无比较，无各向异性
    pub fn default() -> Self {
        Self {
            min_filter: FilterMode::Linear,
            mag_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Linear,
            address_u: AddressMode::Repeat,
            address_v: AddressMode::Repeat,
            address_w: AddressMode::Repeat,
            compare: None,
            anisotropy_clamp: 1,
            border_white: true,
        }
    }

    /// 完整自定义构造
    pub fn new(
        min_filter: FilterMode,
        mag_filter: FilterMode,
        mipmap_filter: MipmapFilterMode,
        address_u: AddressMode,
        address_v: AddressMode,
        address_w: AddressMode,
        compare: Option<CompareFunction>,
    ) -> Self {
        Self {
            min_filter,
            mag_filter,
            mipmap_filter,
            address_u,
            address_v,
            address_w,
            compare,
            anisotropy_clamp: 1,
            border_white: true,
        }
    }

    // ========== 业务预制模板（高频场景，不用手动new一堆参数） ==========
    /// 线性过滤 + 重复平铺（地形、砖块、地面）
    pub fn linear_repeat() -> Self {
        Self::new(
            FilterMode::Linear,
            FilterMode::Linear,
            MipmapFilterMode::Linear,
            AddressMode::Repeat,
            AddressMode::Repeat,
            AddressMode::Repeat,
            None,
        )
    }

    /// 线性过滤 + 边缘截断（UI、角色贴图、模型贴图）
    pub fn linear_clamp() -> Self {
        Self::new(
            FilterMode::Linear,
            FilterMode::Linear,
            MipmapFilterMode::Linear,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            None,
        )
    }

    /// 像素风：邻近过滤 + 重复
    pub fn nearest_repeat() -> Self {
        Self::new(
            FilterMode::Nearest,
            FilterMode::Nearest,
            MipmapFilterMode::Nearest,
            AddressMode::Repeat,
            AddressMode::Repeat,
            AddressMode::Repeat,
            None,
        )
    }

    /// 像素UI：邻近过滤 + 截断
    pub fn nearest_clamp() -> Self {
        Self::new(
            FilterMode::Nearest,
            FilterMode::Nearest,
            MipmapFilterMode::Nearest,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            None,
        )
    }

    /// 阴影贴图专用采样器（带 Less 深度比较）
    pub fn shadow_compare() -> Self {
        Self::new(
            FilterMode::Linear,
            FilterMode::Linear,
            MipmapFilterMode::Linear,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            AddressMode::ClampToEdge,
            Some(CompareFunction::Less),
        )
    }

    /// 8x各向异性线性重复（远景地面、大平面贴图）
    pub fn anisotropic_linear_repeat() -> Self {
        let mut cfg = Self::linear_repeat();
        cfg.anisotropy_clamp = 8;
        cfg
    }

    // ========== Getter 接口 ==========
    #[inline]
    pub fn min_filter(&self) -> FilterMode {
        self.min_filter
    }
    #[inline]
    pub fn mag_filter(&self) -> FilterMode {
        self.mag_filter
    }
    #[inline]
    pub fn mipmap_filter(&self) -> MipmapFilterMode {
        self.mipmap_filter
    }
    #[inline]
    pub fn address_u(&self) -> AddressMode {
        self.address_u
    }
    #[inline]
    pub fn address_v(&self) -> AddressMode {
        self.address_v
    }
    #[inline]
    pub fn address_w(&self) -> AddressMode {
        self.address_w
    }
    #[inline]
    pub fn compare(&self) -> Option<CompareFunction> {
        self.compare
    }
    #[inline]
    pub fn anisotropy(&self) -> u16 {
        self.anisotropy_clamp
    }
    #[inline]
    pub fn use_white_border(&self) -> bool {
        self.border_white
    }

    // ========== Builder 修改接口（链式调整参数） ==========
    #[inline]
    pub fn set_anisotropy(mut self, val: u16) -> Self {
        self.anisotropy_clamp = val;
        self
    }
    #[inline]
    pub fn set_border_white(mut self, enable: bool) -> Self {
        self.border_white = enable;
        self
    }
}