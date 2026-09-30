use std::sync::Arc;

use wgpu::{Buffer, BufferUsages, IndexFormat, Queue};

use crate::base::{error::Error, render::mesh::buffer_layout::BufferLayout};






#[derive(Clone)]
pub struct Mesh {

    vertex_buffer: Arc<Buffer>,
    index_buffer: Option<Arc<Buffer>>,

    vertex_buffer_usages:BufferUsages,
    index_buffer_usages:BufferUsages,

    index_format: Option<IndexFormat>,

    layout: BufferLayout,

    vertex_count: u32,
    index_count: u32,

    
}

impl Mesh {
    pub(crate) fn new(
        vertex_buffer:  Arc<Buffer>,
        index_buffer: Option<Arc<Buffer>>,
        vertex_buffer_usages:BufferUsages,
        index_buffer_usages:BufferUsages,
        index_format:Option<IndexFormat>,
        layout: BufferLayout,
        vertex_count: u32,
        index_count: u32,
        
    ) -> Self {
        Self {
            vertex_buffer,
            index_buffer,
            vertex_buffer_usages,
            index_buffer_usages,
            layout,
            vertex_count,
            index_count,
            index_format
        }
    }


    pub(crate) fn vertex_buffer(&self)-> &Arc<Buffer>{
        &self.vertex_buffer
    }


    pub(crate) fn index_buffer(&self)-> Option<&Arc<Buffer>>{
        self.index_buffer.as_ref()
    }


    // =========================
    // getters
    // =========================

    #[inline]
    pub fn vertex_buffer_usages(&self)->BufferUsages{
        self.vertex_buffer_usages
    }
    #[inline]
    pub fn index_buffer_usages(&self)->BufferUsages{
        self.index_buffer_usages
    }

    #[inline]
    pub(crate) fn layout(&self)->BufferLayout{
        self.layout.clone()
    }

    #[inline]
    pub fn vertex_count(&self) -> u32 {
        self.vertex_count
    }

    #[inline]
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    #[inline]
    pub fn index_format(&self) -> Option<IndexFormat> {
        self.index_format
    }

    #[inline]
    pub fn has_index_buffer(&self) -> bool {
        self.index_buffer.is_some()
    }


    pub(crate) fn write_vertex_data(
        &self,
        queue:&Queue,
        data:&[u8]
    ){
        queue.write_buffer(
            &self.vertex_buffer,
            0,
            data,
        );
    }


    pub(crate) fn write_index_data(
        &self,
        queue: &Queue,
        data: &[u8],
    ) -> Result<(), Error> {

        let buffer = self
            .index_buffer
            .as_ref()
            .ok_or_else(|| Error::from("index buffer is None"))?;


        queue.write_buffer(
            buffer,
            0,
            data,
        );


        Ok(())
    }

}