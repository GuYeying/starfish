//! pygame::render::SpriteMaterial —— **pygame 固定渲染管线的状态材质**
//!（内部设施，不对外暴露；定制管线走 RenderResourceAccess 自建）
//!
//! v1 = 默认带纹理 alpha 混合材质：一套着色器（`shader/sprite.wgsl`，
//! "万能基础着色器"的 pygame 落位）× 小规模管线变体矩阵（颜色目标格式 ×
//! MSAA 采样数 × 填充/线拓扑 × 深度开关），按需惰性创建并缓存——用户只
//! 面对一个 draw 入口（绑定层设计稿 §4.4 的"管线变体矩阵"决策）。
//!
//! 细粒度控制 = **创建期愿望**（`pygame.render.init` / `pygame.init(dim)`
//! 定约，set_mode 前）：剔除模式（默认 none——镜像矩阵绕序天然安全）与
//! 混合模式（默认 alpha）随 Gpu 装配一次定型。
//!
//! 变体矩阵的格式维：交换链格式（Screen 直绘）与离屏 Rgba8Unorm
//! （Surface）不同——ADR-3"管线直绘不做格式转译"的两面。
//!
//! MRT（多渲染目标）：`shader/sprite_mrt.wgsl` 双输出着色器 + N 颜色
//! 目标管线（v1 镜像双写；异构输出走自定义材质）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use starfish_render::bind_group::bind_group::BindGroup;
use starfish_render::mesh::mesh::Mesh;
use starfish_render::pipeline::{CullMode, DepthMode, RenderPipeline};
use starfish_render::render_resource_access::RenderResourceAccess;
use starfish_render::shader_module::shader_module::ShaderModule;
use wgpu::{BlendState, ColorTargetState, ColorWrites, PrimitiveTopology};

use super::batch::SpriteVertex;
use super::Gpu;

/// 精灵着色器源码（内嵌；填充与线段两管线共用。pub = 派生参考）
pub const SPRITE_WGSL: &str = include_str!("shader/sprite.wgsl");

/// MRT 双输出着色器源码（多渲染目标 pass 专用）
pub const SPRITE_MRT_WGSL: &str = include_str!("shader/sprite_mrt.wgsl");

/// 单目标管线的深度格式（与 base SurfaceSettings 默认同源）
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// 混合愿望（render.init 的 "blend" 档位；一次定型）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteBlend {
    /// alpha 混合（默认——pygame blit 语义）
    Alpha,
    /// 不混合（直接替换目标）
    Opaque,
    /// 预乘 alpha 混合
    Premultiplied,
}

impl SpriteBlend {
    fn state(&self) -> Option<BlendState> {
        match self {
            SpriteBlend::Alpha => Some(BlendState::ALPHA_BLENDING),
            SpriteBlend::Opaque => None,
            SpriteBlend::Premultiplied => Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        }
    }
}

/// 默认精灵材质：带纹理 alpha 混合（pygame blit/fill/draw 的统一承载）
///——**pygame 固定渲染管线的状态材质**（内部类型；`pub(crate)` 全封）
pub(crate) struct SpriteMaterial {
    shader: Arc<ShaderModule>,
    shader_mrt: Arc<ShaderModule>,
    /// 顶点布局模板（1 零顶点——管线布局从示例对象推导的引擎既有设计）
    template: Mesh,
    /// 剔除愿望（默认 None：2D 精灵/镜像矩阵天然安全；用户可开 back/front）
    cull: CullMode,
    /// 混合愿望（默认 Alpha）
    blend: SpriteBlend,
    /// 管线变体矩阵缓存（主线程契约；Gpu 槽为线程局部，无 Sync 界）
    pipelines: RefCell<HashMap<(wgpu::TextureFormat, u32, bool, bool), Arc<RenderPipeline>>>,
    /// MRT 管线缓存（键 = 双颜色格式 × 采样 × 拓扑；v1 恒无深度）
    mrt_pipelines: RefCell<HashMap<((wgpu::TextureFormat, wgpu::TextureFormat), u32, bool), Arc<RenderPipeline>>>,
}

impl SpriteMaterial {
    pub(crate) fn new(access: &RenderResourceAccess, cull: CullMode, blend: SpriteBlend) -> Self {
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
            cull,
            blend,
            pipelines: RefCell::new(HashMap::new()),
            mrt_pipelines: RefCell::new(HashMap::new()),
        }
    }

    /// 取（或建）管线变体：`lines` = LineList 线拓扑；`depth` = 深度测试
    /// 开（Standard + 写入，深度格式 [`DEPTH_FORMAT`]，pass 须带深度附件）
    ///
    /// `camera_bind` 仅用于管线布局推导（bind group layout 一致性：
    /// 全部相机 bind 同构，任一示例即可——全局相机化后即全局 rig 本体）。
    pub(crate) fn pipeline(
        &self,
        gpu: &Gpu,
        camera_bind: &BindGroup,
        format: wgpu::TextureFormat,
        samples: u32,
        lines: bool,
        depth: bool,
    ) -> Arc<RenderPipeline> {
        let key = (format, samples, lines, depth);
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
            .cull(self.cull)
            .sample_count(samples.max(1));
        let mut builder = if depth {
            builder
                .depth_mode(DepthMode::Standard)
                .depth_write(true)
        } else {
            builder
        };
        // 色彩目标格式直指（ADR-3：交换链 / 离屏各自真实格式，不转译）；
        // 混合 = 创建期愿望（默认 alpha）
        builder.color_targets(&vec![Some(ColorTargetState {
            format,
            blend: self.blend.state(),
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
            .cull(self.cull)
            .sample_count(samples.max(1));
        let mut builder = builder;
        let targets: Vec<Option<ColorTargetState>> = formats
            .iter()
            .map(|f| {
                Some(ColorTargetState {
                    format: *f,
                    blend: self.blend.state(),
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
