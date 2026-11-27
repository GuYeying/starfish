//! pygame::render::SpriteMaterial —— **pygame 固定渲染管线的材质工厂**
//!（内部设施，不对外暴露；定制管线走 RenderResourceAccess 自建）
//!
//! v1 = 默认带纹理混合材质：一套着色器（`shader/sprite.wgsl`，
//! "万能基础着色器"的 pygame 落位）× 小规模管线变体矩阵（颜色目标格式 ×
//! MSAA 采样数 × 填充/线拓扑 × 深度开关 × **混合 × 剔除**），按需惰性
//! 创建并缓存——用户只面对统一绘制入口。
//!
//! **状态归属（2026-11-27 参数化定约）**：本类型不再持有运行时状态。
//! 混合/剔除由每次绘制的 [`DrawState`] 显式携带（段级记录），提交时只读
//! 段、不读全局——同帧中途切换不会回溯污染已 push 的内容。
//!
//! 变体矩阵的格式维：交换链格式（Screen 直绘）与离屏 Rgba8Unorm
//! （Surface）不同——ADR-3"管线直绘不做格式转译"的两面。
//!
//! MRT（多渲染目标）：`shader/sprite_mrt.wgsl` 双输出着色器 + N 颜色
//! 目标管线（v1 镜像双写；站桩路径，恒 alpha/无剔除，不参与参数化）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use starfish_render::bind_group::bind_group::BindGroup;
use starfish_render::mesh::mesh::Mesh;
use starfish_render::pipeline::{CullMode, DepthMode, RenderPipeline};
use starfish_render::render_resource_access::RenderResourceAccess;
use starfish_render::shader_module::shader_module::ShaderModule;
use wgpu::{BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrites, PrimitiveTopology};

use super::batch::SpriteVertex;
use super::Gpu;

/// 精灵着色器源码（内嵌；填充与线段两管线共用。pub = 派生参考）
pub const SPRITE_WGSL: &str = include_str!("shader/sprite.wgsl");

/// MRT 双输出着色器源码（多渲染目标 pass 专用）
pub const SPRITE_MRT_WGSL: &str = include_str!("shader/sprite_mrt.wgsl");

/// 单目标管线的深度格式（与 base SurfaceSettings 默认同源）
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// 内容层混合档位（每次绘制显式携带；默认 alpha）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpriteBlend {
    /// alpha 混合（默认——pygame blit 语义）
    #[default]
    Alpha,
    /// 不混合（直接替换目标；`noblend` / `opaque` / `none`）
    NoBlend,
    /// 预乘 alpha 混合（素材须预乘）
    Premultiplied,
    /// 加色混合（发光/粒子）
    Additive,
    /// 正片叠底（阴影/压暗）
    Multiply,
}

impl SpriteBlend {
    fn state(self) -> Option<BlendState> {
        match self {
            SpriteBlend::Alpha => Some(BlendState::ALPHA_BLENDING),
            SpriteBlend::NoBlend => None,
            SpriteBlend::Premultiplied => Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            SpriteBlend::Additive => Some(BlendState::ADDITIVE),
            SpriteBlend::Multiply => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Zero,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Zero,
                    operation: BlendOperation::Add,
                },
            }),
        }
    }

    /// 名称解析（Python 面/别名）。未知值返回 Err 文案。
    pub fn from_name(name: &str) -> Result<Self, String> {
        match name {
            "alpha" | "blend" => Ok(SpriteBlend::Alpha),
            "opaque" | "none" | "noblend" | "no_blend" => Ok(SpriteBlend::NoBlend),
            "premultiplied" | "premult" => Ok(SpriteBlend::Premultiplied),
            "additive" | "add" => Ok(SpriteBlend::Additive),
            "multiply" | "mul" => Ok(SpriteBlend::Multiply),
            other => Err(format!(
                "blend 未识别：{other:?}（alpha/blend、opaque/none/noblend、premultiplied、additive、multiply）"
            )),
        }
    }
}

/// 单次绘制的管线状态（Copy 小对象；写进段、提交时读段）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrawState {
    pub blend: SpriteBlend,
    /// 只允许 None / Back（Front 仅供自定义管线；2D 永不剔除）
    pub cull: CullMode,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            blend: SpriteBlend::Alpha,
            cull: CullMode::None,
        }
    }
}

impl DrawState {
    /// 名称解析（blend/cull 任一为 None = 默认值）
    pub fn from_names(blend: Option<&str>, cull: Option<&str>) -> Result<Self, String> {
        let blend = match blend {
            Some(b) => SpriteBlend::from_name(b)?,
            None => SpriteBlend::Alpha,
        };
        let cull = match cull {
            Some("none") | None => CullMode::None,
            Some("back") => CullMode::Back,
            Some(other) => {
                return Err(format!(
                    "cull 未识别：{other:?}（none/back；front 仅供自定义管线）"
                ))
            }
        };
        Ok(Self { blend, cull })
    }
}

/// 默认精灵材质：带纹理混合（pygame blit/fill/draw/gfxdraw 的统一承载）
///——**纯管线变体工厂，无运行时状态**（内部类型；`pub(crate)` 全封）
pub(crate) struct SpriteMaterial {
    shader: Arc<ShaderModule>,
    shader_mrt: Arc<ShaderModule>,
    /// 顶点布局模板（1 零顶点——管线布局从示例对象推导的引擎既有设计）
    template: Mesh,
    /// 管线变体矩阵缓存（键 = 格式 × 采样 × 拓扑 × 深度 × **剔除 × 混合**
    /// ——状态由每次绘制的 [`DrawState`] 携带，变体按需编译；主线程契约）
    pipelines: RefCell<HashMap<
        (wgpu::TextureFormat, u32, bool, bool, CullMode, SpriteBlend),
        Arc<RenderPipeline>,
    >>,
    /// MRT 管线缓存（键 = 双颜色格式 × 采样 × 拓扑；站桩：恒 alpha/无剔除）
    mrt_pipelines: RefCell<HashMap<((wgpu::TextureFormat, wgpu::TextureFormat), u32, bool), Arc<RenderPipeline>>>,
}

impl SpriteMaterial {
    pub(crate) fn new(access: &RenderResourceAccess) -> Self {
        let shader = access
            .shader_module_builder(SPRITE_WGSL)
            .build(Some("pygame_sprite_shader"));
        let shader_mrt = access
            .shader_module_builder(SPRITE_MRT_WGSL)
            .build(Some("pygame_sprite_mrt_shader"));
        // 布局模板：单零顶点（只取其布局语义，不参与绘制）
        let template = access
            .mesh_builder(SpriteVertex::layout(), vec![0u8; SpriteVertex::SIZE])
            .build(None, None);
        Self {
            shader,
            shader_mrt,
            template,
            pipelines: RefCell::new(HashMap::new()),
            mrt_pipelines: RefCell::new(HashMap::new()),
        }
    }

    /// 参数化变体（**显式混合 / 剔除**——每次绘制的 [`DrawState`] 直达；
    /// 引擎内部 pass 恒 alpha/none，无全局状态可污染）
    pub(crate) fn pipeline_with(
        &self,
        gpu: &Gpu,
        camera_bind: &BindGroup,
        format: wgpu::TextureFormat,
        samples: u32,
        lines: bool,
        depth: bool,
        blend: SpriteBlend,
        cull: CullMode,
    ) -> Arc<RenderPipeline> {
        let key = (format, samples, lines, depth, cull, blend);
        if let Some(p) = self.pipelines.borrow().get(&key) {
            return p.clone();
        }
        let builder = gpu
            .access
            .render_pipeline_builder_2d(&self.shader)
            .topology(if lines {
                PrimitiveTopology::LineList
            } else {
                PrimitiveTopology::TriangleList
            })
            .cull(cull)
            // 正面绕序 = **Ccw**（2026-11-27 二次像素裁决，推翻同日的 Cw
            // 结论）：画面已是 y 向下域（世界 +y → 屏幕下方，三角形角点
            // 探针实测），与"几何按 CCW 外向书写"叠加后，外向面在管线判
            // 序里落在 Ccw 一侧。反向绕序双三角探针实证：cull=back 下
            // 法线朝相机的三角被留、反向的被剔，才是"外向面 = 正面"。
            .front_face(wgpu::FrontFace::Ccw)
            .sample_count(samples.max(1));
        let mut builder = if depth {
            builder
                .depth_mode(DepthMode::Standard)
                .depth_write(true)
        } else {
            builder
        };
        // 色彩目标格式直指（ADR-3：交换链 / 离屏各自真实格式，不转译）；
        // 混合 = 本次绘制的显式状态（默认 alpha）
        builder.color_targets(&vec![Some(ColorTargetState {
            format,
            blend: blend.state(),
            write_mask: ColorWrites::ALL,
        })]);
        // 白纹理 bind 作第二组布局示例（全部纹理 bind 同构）
        let pipeline = builder.build(
            &[camera_bind, &gpu.white_bind],
            &self.template,
            Some("pygame_sprite_pipeline"),
        );
        self.pipelines.borrow_mut().insert(key, pipeline.clone());
        pipeline
    }

    /// MRT 管线（N=2 颜色目标，双输出着色器；v1 无深度）
    pub(crate) fn pipeline_mrt(
        &self,
        gpu: &Gpu,
        camera_bind: &BindGroup,
        formats: &[wgpu::TextureFormat],
        samples: u32,
        lines: bool,
    ) -> Arc<RenderPipeline> {
        let key = ((formats[0], formats[1]), samples, lines);
        if let Some(p) = self.mrt_pipelines.borrow().get(&key) {
            return p.clone();
        }
        let builder = gpu
            .access
            .render_pipeline_builder_2d(&self.shader_mrt)
            .topology(if lines {
                PrimitiveTopology::LineList
            } else {
                PrimitiveTopology::TriangleList
            })
            // 站桩路径：恒无剔除 + alpha 混合（不参与 DrawState 参数化）
            .cull(CullMode::None)
            .sample_count(samples.max(1));
        let mut builder = builder;
        let targets: Vec<Option<ColorTargetState>> = formats
            .iter()
            .map(|f| {
                Some(ColorTargetState {
                    format: *f,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })
            })
            .collect();
        builder.color_targets(&targets);
        let pipeline = builder.build(
            &[camera_bind, &gpu.white_bind],
            &self.template,
            Some("pygame_sprite_mrt_pipeline"),
        );
        self.mrt_pipelines
            .borrow_mut()
            .insert(key, pipeline.clone());
        pipeline
    }
}
