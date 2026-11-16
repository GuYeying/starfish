use std::sync::Arc;
use bytemuck::cast_slice;
use wgpu::{BufferUsages, Device, IndexFormat, util::{BufferInitDescriptor, DeviceExt}};

use crate::{ mesh::{
    //attribute::VertexAttribute,
    buffer_layout::BufferLayout,
    mesh::Mesh,
}};




pub struct MeshBuilder {
    device : Arc<Device>,

    layout: Vec<wgpu::VertexFormat>,

    vertices: Vec<u8>,
    long_indices: Vec<u32>,
    short_indices: Vec<u16>,
    /// 原始字节索引（高级口——LE 字节 + 显式格式；越界校验调用方自担）
    index_bytes: Option<(Vec<u8>, wgpu::IndexFormat)>,

    vertex_buffer_usages:BufferUsages,
    index_buffer_usages:BufferUsages,

    /// 顶点布局实例步进（with_instance_stepped）
    instance_stepped: bool,
}

impl MeshBuilder {
    pub(crate) fn new(
        device : &Arc<Device>,
        layout: Vec<wgpu::VertexFormat>,
        vertices: Vec<u8>,
    ) -> Self {
        Self {
            device:device.clone(),
            layout,
            vertices,
            long_indices:Vec::new(),
            short_indices:Vec::new(),
            index_bytes:None,
            vertex_buffer_usages: BufferUsages::VERTEX,
            index_buffer_usages: BufferUsages::INDEX,
            instance_stepped: false,
        }
    }

    /// 顶点布局按**实例步进**（instanced draw——GPU 粒子：每实例一粒，
    /// pos 经顶点属性进 VS——顶点属性是固定功能通路，规避 VS 段纹理/
    /// SSBO 读取的多平台静默失效，2026-10-30 实测定案）
    pub fn with_instance_stepped(mut self) -> Self {
        self.instance_stepped = true;
        self
    }

    pub fn with_vertex_usages(mut self,usages:BufferUsages)->Self{
        self.vertex_buffer_usages = usages | BufferUsages::VERTEX | BufferUsages::COPY_DST;
        self
    }
    pub fn with_index_usages(mut self,usages:BufferUsages)->Self{
        self.index_buffer_usages =  usages | BufferUsages::INDEX;
        self
    }

    pub fn with_indices(self, indices: Vec<u32>) -> Self {
        self.with_long_indices(indices)
    }

    pub fn with_short_indices(mut self, indices: Vec<u16>) -> Self {
        self.short_indices = indices;
        self.long_indices = Vec::new();
        self
    }

    pub fn with_long_indices(mut self, indices: Vec<u32>) -> Self {
        self.long_indices = indices;
        self.short_indices = Vec::new();
        self
    }

    /// 原始字节索引（高级口——**LE 字节 + 显式格式**；越界校验调用方
    /// 自担——大 mesh / py 侧 bytes 直喂通路，免 Vec<u16/u32> 中转；
    /// 优先级高于 short/long 列表口）
    pub fn with_index_bytes(mut self, data: Vec<u8>, format: wgpu::IndexFormat) -> Self {
        self.index_bytes = Some((data, format));
        self.short_indices = Vec::new();
        self.long_indices = Vec::new();
        self
    }


    pub fn build(
        self,
        label_vertex: Option<&str>,
        label_index: Option<&str>,
    ) -> Mesh {
        // 步进模式：常规逐顶点 / 实例步进（with_instance_stepped）
        let buffer_layout = if self.instance_stepped {
            BufferLayout::instance(&self.layout)
        } else {
            BufferLayout::vertex(&self.layout)
        };

        // =========================
        // Vertex
        // =========================
        let stride: u64 = self
            .layout
            .iter()
            .map(|format| format.size() as u64)
            .sum();

        assert!(stride != 0, "Vertex stride cannot be zero.");

        let vertex_count = (self.vertices.len() as u64 / stride) as u32;

        let vertex_buffer = Arc::new(self.device.create_buffer_init(&BufferInitDescriptor {
            label: label_vertex,
            contents: &self.vertices,
            usage: self.vertex_buffer_usages,
        }));
                

        // =========================
        // Index
        // =========================
        let mut index_buffer = None;
        let mut index_format = None;
        let mut index_count = 0;

        if let Some((data, fmt)) = self.index_bytes {
            // 原始字节口（高级口——越界校验调用方自担；count = 字节数 /
            // 格式宽度）
            index_count = (data.len() as u64 / if fmt == wgpu::IndexFormat::Uint16 { 2 } else { 4 }) as u32;
            index_buffer = Some(Arc::new(self.device.create_buffer_init(&BufferInitDescriptor {
                label: label_index,
                contents: &data,
                usage: self.index_buffer_usages,
            })));
            index_format = Some(fmt);
        } else if !self.short_indices.is_empty() {
            let max_index = *self.short_indices.iter().max().unwrap();

            assert!(
                (max_index as u32) < vertex_count,
                "Index {} out of range (vertex_count={})",
                max_index,
                vertex_count
            );

            index_count = self.short_indices.len() as u32;


            index_buffer = Some(Arc::new(self.device.create_buffer_init(&BufferInitDescriptor {
                label: label_index,
                contents: cast_slice(&self.short_indices),
                usage: self.index_buffer_usages,
            })));

            index_format = Some(IndexFormat::Uint16);
        } else if !self.long_indices.is_empty() {
            let max_index = *self.long_indices.iter().max().unwrap();

            assert!(
                max_index < vertex_count,
                "Index {} out of range (vertex_count={})",
                max_index,
                vertex_count
            );

            index_count = self.long_indices.len() as u32;

            index_buffer = Some(Arc::new(self.device.create_buffer_init(&BufferInitDescriptor {
                label: label_index,
                contents: cast_slice(&self.long_indices),
                usage: self.index_buffer_usages,
            })));

            index_format = Some(IndexFormat::Uint32);
        }
        
        Mesh::new(
            vertex_buffer,
            index_buffer,
            self.vertex_buffer_usages,
            self.index_buffer_usages,
            index_format,
            buffer_layout,
            vertex_count,
            index_count,
        )
    }
}