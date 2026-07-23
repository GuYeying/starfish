use wgpu::RenderPass as WgpuRenderPass;

use crate::base::render::{
    bind_group::bind_group::BindGroup,
    mesh::mesh::Mesh,
    pipeline::RenderPipeline,
};

use wgpu::Buffer;

pub struct RenderPass<'a> {
    inner: WgpuRenderPass<'a>,
}

impl<'a> RenderPass<'a> {
    pub(crate) fn new(inner: WgpuRenderPass<'a>) -> Self {
        Self {
            inner,
        }
    }

    // =========================
    // Pipeline
    // =========================
    pub fn set_pipeline(&mut self, pipeline: &RenderPipeline) {
        self.inner.set_pipeline(pipeline.pipeline());
    }

    // =========================
    // BindGroup
    // =========================
    pub fn set_bind_group(
        &mut self,
        index: u32,
        group: &BindGroup,
    ) {
        self.inner.set_bind_group(
            index,
            group.bind_group(),
            &[],
        );
    }


    pub fn set_bind_groups(
        &mut self,
        groups: &[(u32, &BindGroup)],
    ) {
        for (slot, g) in groups {
            self.set_bind_group(*slot, g);
        }
    }

    // =========================
    // Vertex / Index (底层能力)
    // =========================
    pub  fn set_vertex_buffer(&mut self, slot: u32, buffer: &Buffer) {
        self.inner.set_vertex_buffer(slot, buffer.slice(..));
    }

    pub fn set_index_buffer(&mut self, buffer: &Buffer, format: wgpu::IndexFormat) {
        self.inner.set_index_buffer(buffer.slice(..), format);
    }

    // =========================
    // Mesh（高级封装）
    // =========================
    pub fn set_mesh(&mut self, mesh: &Mesh) {
        self.inner.set_vertex_buffer(0, mesh.vertex_buffer().slice(..));

        if mesh.has_index_buffer() {
            self.inner.set_index_buffer(
                mesh.index_buffer().unwrap().slice(..),
                mesh.index_format().unwrap(),
            );
        }
    }

    pub fn draw_mesh(&mut self, mesh: &Mesh) {
        self.inner.set_vertex_buffer(0, mesh.vertex_buffer().slice(..));
        if mesh.has_index_buffer() {
            self.inner.set_index_buffer(
                mesh.index_buffer().unwrap().slice(..),
                mesh.index_format().unwrap());
            self.inner.draw_indexed(0..mesh.index_count(), 0, 0..1);
        } else {
            self.draw(
                0..mesh.vertex_count(),
                0..1,
            );
        }
    }


    pub fn draw_mesh_instanced(
        &mut self,
        mesh: &Mesh,
        instances: std::ops::Range<u32>,
    ) {
        self.set_mesh(mesh);

        if mesh.has_index_buffer() {
            self.draw_indexed(
                0..mesh.index_count(),
                0,
                instances,
            );
        } else {
            self.draw(
                0..mesh.vertex_count(),
                instances,
            );
        }
    }

    // =========================
    // Draw
    // =========================
    pub fn draw(
        &mut self,
        vertices: std::ops::Range<u32>,
        instances: std::ops::Range<u32>,
    ) {
        self.inner.draw(vertices, instances);
    }

    pub fn draw_indexed(
        &mut self,
        indices: std::ops::Range<u32>,
        base_vertex: i32,
        instances: std::ops::Range<u32>,
    ) {
        self.inner.draw_indexed(indices, base_vertex, instances);
    }

    // =========================
    // Indirect Draw（GPU Driven基础）
    // =========================
    pub fn draw_indirect(&mut self, buffer: &wgpu::Buffer, offset: u64) {
        self.inner.draw_indirect(buffer, offset);
    }

    pub fn draw_indexed_indirect(&mut self, buffer: &wgpu::Buffer, offset: u64) {
        self.inner.draw_indexed_indirect(buffer, offset);
    }

    // =========================
    // State（基础GPU状态）
    // =========================
    pub fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        self.inner.set_viewport(x, y, w, h, min_depth, max_depth);
    }

    pub fn set_scissor_rect(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) {
        self.inner.set_scissor_rect(x, y, w, h);
    }

    pub fn set_stencil_reference(&mut self, value: u32) {
        self.inner.set_stencil_reference(value);
    }

    pub fn set_blend_constant(&mut self, color: wgpu::Color) {
        self.inner.set_blend_constant(color);
    }

    // =========================
    // Debug
    // =========================
    pub fn push_debug_group(&mut self, label: &str) {
        self.inner.push_debug_group(label);
    }

    pub fn pop_debug_group(&mut self) {
        self.inner.pop_debug_group();
    }

    pub fn insert_debug_marker(&mut self, label: &str) {
        self.inner.insert_debug_marker(label);
    }


    pub fn end(self){

    }

}