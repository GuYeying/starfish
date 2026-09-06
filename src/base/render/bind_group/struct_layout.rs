pub use wgpu::ShaderStages;

use crate::base::render::bind_group::{field_layout::FieldLayout, field_type::StructType};


#[inline]
fn align_up(offset: u64, alignment: u64) -> u64 {
    (offset + alignment - 1) & !(alignment - 1)
}




#[derive(Clone, Debug)]
pub struct StructLayout {
    fields: Vec<FieldLayout>,
    stride: u64, // struct size aligned to max alignment
}


impl StructLayout {

    pub fn new(values: &[StructType]) -> StructLayout {

        let mut offset = 0;

        let mut fields = Vec::new();

        for value in values {

            let alignment = value.alignment();

            offset = align_up(offset, alignment);

            let size = value.gpu_size();

            fields.push(
                FieldLayout::new(
                    offset,
                    size,
                    alignment,
                )
            );


            offset += size;
        }


        let stride = align_up(offset, 16);


        Self {
            fields,
            stride,
        }
    }


    pub(crate) fn field(&self, index: usize) -> &FieldLayout {
        &self.fields[index]
    }


    pub fn stride(&self) -> u64 {
        self.stride
    }


    pub fn len(&self) -> usize {
        self.fields.len()
    }


}