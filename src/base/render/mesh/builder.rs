use std::sync::Arc;
use bytemuck::cast_slice;
use wgpu::{BufferUsages, Device, IndexFormat, util::{BufferInitDescriptor, DeviceExt}};

use crate::base::render::{ mesh::{
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

    vertex_buffer_usages:BufferUsages,
    index_buffer_usages:BufferUsages,

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
            vertex_buffer_usages: BufferUsages::VERTEX,
            index_buffer_usages: BufferUsages::INDEX
        }
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


    pub fn build(
        self,
        label_vertex: Option<&str>,
        label_index: Option<&str>,
    ) -> Mesh {
        let buffer_layout = BufferLayout::vertex(&self.layout);

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

        if !self.short_indices.is_empty() {
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
            print!("16!!!");
        } else if !self.long_indices.is_empty() {
            let max_index = *self.long_indices.iter().max().unwrap();

            assert!(
                max_index < vertex_count,
                "Index {} out of range (vertex_count={})",
                max_index,
                vertex_count
            );

            index_count = self.long_indices.len() as u32;
            print!("32!!!");

            index_buffer = Some(Arc::new(self.device.create_buffer_init(&BufferInitDescriptor {
                label: label_index,
                contents: cast_slice(&self.long_indices),
                usage: self.index_buffer_usages,
            })));

            index_format = Some(IndexFormat::Uint32);
        }
        
        println!("{} {}", vertex_count,index_count);
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