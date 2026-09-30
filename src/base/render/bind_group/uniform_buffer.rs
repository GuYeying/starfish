use std::sync::Arc;
use wgpu::{Device, Queue};

use crate::base::render::{bind_group::{struct_layout::StructLayout, field_value::StructValue}};
use wgpu::Buffer;


// ✔ 你现在更干净的结构应该是
// UniformBuffer（核心）
// BindGroup（GPU绑定）
// UniformLayout（CPU布局）



//每个更新的field
#[derive(Clone, Debug)]
pub struct UniformUpdate {
    pub index: usize,
    pub value: StructValue,
}

//这个用来代替之前直接创建BUffer，后面会利用context接口直接创建完成对Buffer代替
pub struct UniformBuffer {
    buffer: Arc<Buffer>,
    layout: Arc<StructLayout>,

    // index -> value（核心）
    values: Vec<Option<StructValue>>,

    data: Vec<u8>,
    dirty: bool,
}

impl UniformBuffer {

     pub(crate) fn new(device: &Arc<Device>, layout: &Arc<StructLayout>) -> Self {
        let size = layout.stride();

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size,
            usage: wgpu::BufferUsages::UNIFORM
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            buffer: Arc::new(buffer),
            values: vec![None; layout.len()],
            layout:layout.clone(),
            data: vec![0; size as usize],
            dirty: true,
        }
    }

    pub(crate) fn arc_buffer(&self)->&Arc<Buffer>{
        &self.buffer
    }

    pub fn is_dirty(&self)->bool{
        self.dirty
    }


    pub fn set(&mut self, field: usize, value: StructValue) {
        if field >= self.values.len() {
            self.values.resize_with(field + 1, || None);
        }

        self.values[field] = Some(value);
        self.dirty = true;
    }

    //cpu上传到gpu
    pub fn upload(&mut self, queue: &Queue) {
        if !self.dirty {
            return;
        }

        self.rebuild();

        queue.write_buffer(&self.buffer, 0, &self.data);

        self.dirty = false;
    }


    //CPU 内存组装
    fn rebuild(&mut self) {
        self.data.fill(0);

        for (index, maybe_value) in self.values.iter().enumerate() {
            let Some(value) = maybe_value else {
                continue;
            };

            let field = &self.layout.field(index);

            let mut tmp = Vec::new();
            value.write_raw(&mut tmp);

            let dst = &mut self.data
                [field.offset() as usize .. field.offset() as usize + tmp.len()];

            dst.copy_from_slice(&tmp);
        }

        self.dirty = true;
    }

    // pub fn write<T: bytemuck::Pod>(&self, queue: &Queue, data: &[T]) {
    //     let bytes = bytemuck::cast_slice(data);
    //     //self.buffer.write(queue, bytes);
    //     queue.write_buffer(&self.buffer, 0, &bytes);
    // }

    // pub fn write_offset<T: bytemuck::Pod>(&self, queue: &Queue, offset: u64, data: &[T]) {
    //     let bytes = bytemuck::cast_slice(data);
    //     //self.buffer.write_data(queue, offset, bytes);
    //     queue.write_buffer(&self.buffer, offset, &bytes);
    // }


}
