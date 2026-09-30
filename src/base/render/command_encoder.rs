use std::num::NonZero;

use wgpu::{CommandEncoder as WgpuCommandEncoder, QuerySet, RenderPassTimestampWrites};
use crate::base::render::render_pass::{attachments::{ColorAttachment, DepthAttachment}, render_pass::RenderPass};




// 转换切片：自定义 ColorAttachment 切片 → wgpu 原生附件切片临时数组
#[inline]
fn convert_color_attachments<'a>(src: &'a [&Option<ColorAttachment>]) -> Vec<Option<wgpu::RenderPassColorAttachment<'a>>> {
    src.iter()
        .map(|opt| opt.as_ref().map(|ca| ca.to_wgpu()))
        .collect()
}

// 转换深度附件
#[inline]
fn convert_depth_attachment<'a>(src: &'a Option<DepthAttachment>) -> Option<wgpu::RenderPassDepthStencilAttachment<'a>> {
    src.as_ref().map(|da| da.to_wgpu())
}


/// 构建常规无高级查询的渲染通道描述符
#[inline]
fn make_render_pass_desc<'a>(
    label: Option<&'a str>,
    color_attachments: &'a [Option<wgpu::RenderPassColorAttachment<'a>>],
    depth_stencil_attachment: Option<wgpu::RenderPassDepthStencilAttachment<'a>>,
    timestamp_writes:Option<RenderPassTimestampWrites<'a>>,
    occlusion_query_set:Option<&'a QuerySet>,
    multiview_mask:Option<NonZero<u32>>
) -> wgpu::RenderPassDescriptor<'a> {
    wgpu::RenderPassDescriptor {
        label,
        color_attachments,
        depth_stencil_attachment,
        timestamp_writes: timestamp_writes,
        occlusion_query_set: occlusion_query_set,
        multiview_mask: multiview_mask,
    }
}


pub struct CommandEncoder {
    inner: WgpuCommandEncoder, // 去掉Arc，独占
}

impl CommandEncoder {


    pub(crate) fn new(inner: WgpuCommandEncoder) -> Self {
        Self { inner }
    }

    /// 开启渲染通道
    /// label: 调试标签
    /// color_attachments: 自定义颜色附件数组
    /// depth_stencil_attachment: 可选自定义深度附件
    pub fn begin_render_pass<'a>(
        &'a mut self,
        label: &str,
        color_attachments: &'a [&Option<ColorAttachment>],
        depth_stencil_attachment: Option<DepthAttachment>,
        timestamp_writes:Option<RenderPassTimestampWrites<'a>>,
        occlusion_query_set:Option<&'a QuerySet>,
        multiview_mask:Option<NonZero<u32>>
    ) -> RenderPass<'a> {
        // 1. 批量转换自定义附件为 wgpu 原生临时附件
        let wgpu_color_atts = convert_color_attachments(color_attachments);
        let wgpu_depth_att = convert_depth_attachment(&depth_stencil_attachment);
        // 2. 构造标准描述符
        let render_desc = make_render_pass_desc(
            Some(label), 
            &wgpu_color_atts,
            wgpu_depth_att,
            timestamp_writes,
            occlusion_query_set,
            multiview_mask,
            );
        // 3. 开启通道并包装
        let raw_pass = self.inner.begin_render_pass(&render_desc);
        RenderPass::new(raw_pass)
    }


    // 提交命令队列
    pub fn finish(self) -> wgpu::CommandBuffer {
        self.inner.finish()
    }

    /// 写入时间戳（需 `Features::TIMESTAMP_QUERY`，`features::core()` 默认已含；
    /// query_set 的类型必须为 `QueryType::Timestamp`）
    ///
    /// 结果回读流程：`resolve_query_set` → 缓冲 `map_async` → `get_mapped_range`。
    pub fn write_timestamp(&mut self, query_set: &QuerySet, query_index: u32) {
        self.inner.write_timestamp(query_set, query_index);
    }

    /// 把查询结果解析进缓冲区（每结果 8 字节 u64）
    ///
    /// `query_indices` 为要解析的查询下标范围；
    /// 目标缓冲建议用 `RenderResourceAccess::create_query_result_buffer`
    /// （MAP_READ | COPY_DST | QUERY_RESOLVE），提交后 `map_async` 回读。
    pub fn resolve_query_set(
        &mut self,
        query_set: &QuerySet,
        query_indices: std::ops::Range<u32>,
        destination: &wgpu::Buffer,
        destination_offset: u64,
    ) {
        self.inner
            .resolve_query_set(query_set, query_indices, destination, destination_offset);
    }

    /// 拷贝缓冲区到缓冲区
    pub fn copy_buffer_to_buffer(
        &mut self,
        source: &wgpu::Buffer,
        source_offset: u64,
        destination: &wgpu::Buffer,
        destination_offset: u64,
        copy_size: u64,
    ) {
        self.inner
            .copy_buffer_to_buffer(source, source_offset, destination, destination_offset, copy_size);
    }
}