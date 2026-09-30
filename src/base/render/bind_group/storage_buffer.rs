

use std::sync::Arc;
use wgpu::{Device, Queue};
use::wgpu::Buffer;
use crate::base::render::bind_group::struct_layout::StructLayout;
use crate::base::render::bind_group::field_value::StructValue;

pub struct StorageBuffer {
    buffer: Arc<Buffer>,
    layout: Arc<StructLayout>, // ✔ 可选（核心差异）
    count: usize,
    data: Vec<u8>,
    dirty: bool,
}

impl StorageBuffer {


    pub(crate) fn new(
        device:&Arc<Device>,
        layout: &Arc<StructLayout>,
        count: usize,
        label: Option<&str>,
    ) -> Self {

        let size = layout.stride() as usize * count;


        let buffer = Arc::new(
            device.create_buffer(
                &wgpu::BufferDescriptor {
                    label,
                    size: size as u64,
                    usage:
                        wgpu::BufferUsages::STORAGE |
                        wgpu::BufferUsages::COPY_DST |
                        wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }
            )
        );


        Self {
            buffer,
            layout:layout.clone(),
            count,
            data: vec![0; size],
            dirty: true,
        }
    }



    pub(crate) fn arc_buffer(&self)->&Arc<Buffer>{
        &self.buffer
    }

    pub fn is_dirty(&self)->bool{
        self.dirty
    }

    pub fn count(&self)->usize{
        self.count
    }
    
    pub fn set_element(&mut self,element:usize,values:&[StructValue]){

        let base = element as u64 * self.layout.stride();


        let offset = base;


        for (i,value) in values.iter().enumerate()
        {
            let field =  self.layout.field(i);

            let mut bytes = Vec::new();

            value.write_raw(&mut bytes);

            let dst = offset + field.offset();

            let end = dst as usize + bytes.len();

            self.data[dst as usize..end].copy_from_slice(&bytes);
        }


        self.dirty=true;
    }

    pub fn set_by_index(&mut self, index: usize, value: StructValue) {

        let field = &self.layout.field(index);

        let mut tmp = Vec::new();
        value.write_raw(&mut tmp);

        let end = field.offset() as usize + tmp.len();

        if self.data.len() < end {
            self.data.resize(end, 0);
        }

        self.data[field.offset() as usize..end]
            .copy_from_slice(&tmp);

        self.dirty = true;
    }

    pub fn upload(&mut self, queue: &Queue) {
        if !self.dirty {
            return;
        }

        queue.write_buffer(&self.buffer, 0, &self.data);
        self.dirty = false;
    }


    pub fn write(&mut self, offset: usize, data: &[u8]) {
        let end = offset + data.len();

        if self.data.len() < end {
            self.data.resize(end, 0);
        }

        self.data[offset..end].copy_from_slice(data);
        self.dirty = true;
    }


    pub fn write_offset<T: bytemuck::Pod>(&self, queue: &Queue, offset: u64, data: &[T]) {
        let bytes = bytemuck::cast_slice(data);
        //self.buffer.write_data(queue, offset, bytes);
        queue.write_buffer(&self.buffer, offset, &bytes);
    }

}