//! GPU 能力特性（features）：许愿 → 掩码 → 暴露
//!
//! 引擎边界声明：
//! - 引擎只负责「开/不开」（选型函数 + [`resolve`] 掩码）和「暴露」
//!   （[`super::RenderContext::features`] / [`super::RenderContext::limits`] 查询），
//!   **不实现任何降级路径**
//! - 未获得能力位时调用相关 API，将得到 wgpu 原生 validation 错误
//! - 开发者以 `RenderContext::features()` 自查实际能力，自行分支
//!
//! 选型三层：
//! - [`core()`]：默认愿望（`GpuSettings` 默认引用），跨平台安全
//! - [`recommended()`] / [`special()`]：保留位，`enable_features` 一键加购
//! - 去掉清单：见模块尾部，触发回加条件时再收编
//!
//! ─── 保留位速查表（`enable_features(wgpu::Features::XXX)` 原始位直通） ───
//!
//! | feature 位 | 消费参数接口 | 场景 |
//! |---|---|---|
//! | `SHADER_F16` | 自写 WGSL `enable f16;` | 半精度着色 |
//! | `TEXTURE_COMPRESSION_BC` / `_ASTC` / `_ETC2` | `ImageData::Compressed` + `create_texture` | 压缩纹理（加载器归开发者实现） |
//! | `DUAL_SOURCE_BLENDING` | `BlendMode::Custom(wgpu::BlendState)` | 高级透明混合 |
//! | `TEXTURE_BINDING_ARRAY` + `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING` + `PARTIALLY_BOUND_BINDING_ARRAY` | （绑定数组 API 待 2D 批处理立项时开放） | 精灵批处理（对标 SRP Batcher） |
//! | `SHADER_FLOAT32_ATOMIC` | 自写 WGSL + `StorageBuffer` | OIT 无序透明、GPU 粒子 |
//! | `RG11B10UFLOAT_RENDERABLE` | `TextureDescriptor::with_format_override` + `color_targets` | 轻量 HDR 渲染目标 |
//! | `FLOAT32_FILTERABLE` | `SamplerDescriptor` + 浮点纹理绑定 | 计算/后处理采样 |
//! | `DEPTH32FLOAT_STENCIL8` | `SurfaceSettings::with_depth_format` | 深度模板组合格式 |
//! | `CLIP_DISTANCES` | 自写 WGSL | 用户裁剪面 |
//!
//! ─── 去掉清单（已裁决排除；触发回加条件时再收编进选型） ───
//!
//! | feature 位 | 去掉理由 | 回加条件 |
//! |---|---|---|
//! | `MULTI_DRAW_INDIRECT_COUNT` | 回报需 GPU-driven 万级实例兑现，接口未开 | GPU-driven 渲染立项 |
//! | `MULTIVIEW` | wgpu 支持面窄（基本 Vulkan only），无 VR 需求 | VR 需求出现 |
//! | `TEXTURE_FORMAT_16BIT_NORM` | 8bit / RGBA16F 可替，生态少用 | 出现实际精度需求 |
//! | `BGRA8UNORM_STORAGE` | 极少场景，转 RGBA8 可绕过 | 出现无法转换的需求 |
//! | `TIMESTAMP_QUERY_INSIDE_ENCODERS` / `_INSIDE_PASSES` | pass 级时间戳已够，支持面最差 | 需要 pass 内细粒度剖析 |

use wgpu::{Adapter, Features};

/// 第一层：核心 feature 愿望（`GpuSettings` 默认引用）
///
/// 四项均为 WebGPU 标准或桌面原生近乎全有，不存在任何会让设备创建失败的平台组合：
/// - `DEPTH_CLIP_CONTROL`：反向 Z / 无限远深度地基
///   （消费：`RenderPipelineBuilder::unclipped_depth`、`DepthMode::Reverse`）
/// - `INDIRECT_FIRST_INSTANCE`：GPU-driven 第一块砖（消费：`RenderPass::draw_indirect`）
/// - `TIMESTAMP_QUERY`：profiler 地基（消费：`begin_render_pass` 的 timestamp_writes 参数）
/// - `POLYGON_MODE_LINE`：线框（消费：`RenderPipelineBuilder::wireframe`）
///
/// Web / 移动端不支持的位由 [`resolve`] 掩码自动剥离。
/// 想摘除某项：`GpuSettings::disable_features(...)`。
pub fn core() -> Features {
    Features::DEPTH_CLIP_CONTROL
        | Features::INDIRECT_FIRST_INSTANCE
        | Features::TIMESTAMP_QUERY
        | Features::POLYGON_MODE_LINE
}

/// 第二层：保留位——高性价比 + 支持度持续向好
///
/// `enable_features(features::recommended())` 一键加购（原生 only 位在 Web 上自动掩码）：
/// - `SHADER_F16`：WebGPU 标准位，三端支持度持续上涨，零接口成本
/// - `TEXTURE_COMPRESSION_BC / ASTC / ETC2`：桌面/移动各自通吃的压缩生态，显存带宽红利
/// - `DUAL_SOURCE_BLENDING`：WebGPU 标准化完成，高级透明正解
/// - 绑定数组三件套：2D 批处理器最大杠杆（桌面原生全支持）
pub fn recommended() -> Features {
    Features::SHADER_F16
        | Features::TEXTURE_COMPRESSION_BC
        | Features::TEXTURE_COMPRESSION_ASTC
        | Features::TEXTURE_COMPRESSION_ETC2
        | Features::DUAL_SOURCE_BLENDING
        | Features::TEXTURE_BINDING_ARRAY
        | Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | Features::PARTIALLY_BOUND_BINDING_ARRAY
}

/// 第二层：保留位——特殊/复杂渲染场景
///
/// `enable_features(features::special())` 一键加购：
/// - `SHADER_FLOAT32_ATOMIC`：OIT 无序透明、GPU 粒子的唯一路径
/// - `RG11B10UFLOAT_RENDERABLE`：轻量 HDR 渲染目标
/// - `FLOAT32_FILTERABLE`：计算/后处理的浮点纹理采样
/// - `DEPTH32FLOAT_STENCIL8`：组合深度模板格式便利
/// - `CLIP_DISTANCES`：编辑器式用户裁剪面
pub fn special() -> Features {
    Features::SHADER_FLOAT32_ATOMIC
        | Features::RG11B10UFLOAT_RENDERABLE
        | Features::FLOAT32_FILTERABLE
        | Features::DEPTH32FLOAT_STENCIL8
        | Features::CLIP_DISTANCES
}

/// 与 [`recommended()`] / [`special()`] 配套的上限愿望
///
/// 基线之上追加绑定数组元素数——注意 `max_binding_array_elements_per_shader_stage`
/// 的 wgpu 默认值是 **0**，绑定数组 feature 位必须配合此 limit 才真正可用。
///
/// 用法：
/// `GpuSettings::default().enable_features(recommended()).with_limits(recommended_limits())`
///
/// 平台安全性同 feature 掩码：愿望经 `resolve_limits` 钳制到硬件支持范围。
pub fn recommended_limits() -> wgpu::Limits {
    wgpu::Limits {
        max_binding_array_elements_per_shader_stage: 512,
        ..wgpu::Limits::defaults()
    }
}

/// 愿望掩码：愿望 & 硬件能力
///
/// wgpu 30 的 `DeviceDescriptor` 无 `optional_features` 字段，
/// 「先请求适配器、再掩码进 required_features」即等价实现——
/// 硬件不支持的位自动消失，设备创建永不因愿望而失败。
#[inline]
pub fn resolve(wish: Features, adapter: &Adapter) -> Features {
    wish & adapter.features()
}

/// 缺差集合：想要但没拿到的位
#[inline]
pub fn missing(wish: Features, granted: Features) -> Features {
    wish.difference(granted)
}

/// 能力位名字列表（日志/诊断用）
pub fn names(feats: Features) -> Vec<&'static str> {
    feats.iter_names().map(|(name, _)| name).collect()
}

/// 打印能力报告（创建设备后调用一次）
pub fn report(wish: Features, granted: Features) {
    println!("===== wgpu feature report =====");
    println!("wish:    {:?}", names(wish));
    println!("granted: {:?}", names(granted));
    let miss = missing(wish, granted);
    if miss.is_empty() {
        println!("missing: (none)");
    } else {
        println!("missing: {:?}", names(miss));
    }
    println!("===============================");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_has_four_bits() {
        assert_eq!(names(core()).len(), 4);
    }

    #[test]
    fn selection_bit_counts() {
        assert_eq!(names(recommended()).len(), 8);
        assert_eq!(names(special()).len(), 5);
        // 三层互不重叠
        assert_eq!(missing(core(), recommended()), core());
        assert_eq!(missing(special(), recommended()), special());
    }

    #[test]
    fn missing_reports_ungranted() {
        assert_eq!(missing(core(), Features::empty()), core());
        assert_eq!(missing(core(), core()), Features::empty());

        // 部分授予：缺的两位精确报出
        let partial = Features::TIMESTAMP_QUERY | Features::POLYGON_MODE_LINE;
        assert_eq!(
            missing(core(), partial),
            Features::DEPTH_CLIP_CONTROL | Features::INDIRECT_FIRST_INSTANCE
        );
    }
}
