use glam::{Mat2, Mat3, Mat4, Vec2, Vec3, Vec4};



use bytemuck::bytes_of;

use crate::base::render::bind_group::field_type::StructType;


#[inline]
fn write<T: bytemuck::Pod>(dst: &mut Vec<u8>, value: &T) {
    dst.extend_from_slice(bytes_of(value));
}

#[inline]
fn pad(dst: &mut Vec<u8>, n: usize) {
    dst.resize(dst.len() + n, 0);
}


/// Uniform数据类型
///
/// 用于存储Shader Uniform参数。
#[derive(Clone, Debug)]
pub enum StructValue {

    F32(f32),
    I32(i32),
    U32(u32),

    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),

    IVec2([i32;2]),
    IVec3([i32;3]),
    IVec4([i32;4]),

    UVec2([u32;2]),
    UVec3([u32;3]),
    UVec4([u32;4]),


    Mat2(Mat2),
    Mat3(Mat3),
    Mat4(Mat4),


    F32Array(Vec<f32>),
    Vec3Array(Vec<Vec3>),
    Mat4Array(Vec<Mat4>),
}


impl StructValue {

    pub fn write_raw(&self, dst: &mut Vec<u8>) {
        match self {

            StructValue::F32(v) => {
                write(dst, v);
            }

            StructValue::I32(v) => {
                write(dst, v);
            }

            StructValue::U32(v) => {
                write(dst, v);
            }

            StructValue::Vec2(v) => {
                write(dst, v);
            }

            StructValue::Vec3(v) => {
                write(dst, v);

                // vec3 必须补4字节
                pad(dst, 4);
            }

            StructValue::Vec4(v) => {
                write(dst, v);
            }

            StructValue::IVec2(v) => {
                write(dst, v);
            }

            StructValue::IVec3(v) => {
                write(dst, v);
                pad(dst, 4);
            }

            StructValue::IVec4(v) => {
                write(dst, v);
            }

            StructValue::UVec2(v) => {
                write(dst, v);
            }

            StructValue::UVec3(v) => {
                write(dst, v);
                pad(dst, 4);
            }

            StructValue::UVec4(v) => {
                write(dst, v);
            }

            StructValue::Mat2(m) => {
                let cols = m.to_cols_array();

                write(dst, &[cols[0], cols[1]]);
                write(dst, &[cols[2], cols[3]]);
            }

            StructValue::Mat3(m) => {
                let cols = m.to_cols_array();

                for i in 0..3 {
                    write(dst, &[
                        cols[i * 3],
                        cols[i * 3 + 1],
                        cols[i * 3 + 2],
                    ]);

                    // 每列补到16字节
                    pad(dst, 4);
                }
            }

            StructValue::Mat4(m) => {
                write(dst, &m.to_cols_array());
            }

            StructValue::F32Array(arr) => {
                for v in arr {
                    write(dst, v);

                    // Uniform Array 每个元素 stride=16
                    pad(dst, 12);
                }
            }

            StructValue::Vec3Array(arr) => {
                for v in arr {
                    write(dst, v);
                    pad(dst, 4);
                }
            }

            StructValue::Mat4Array(arr) => {
                for m in arr {
                    write(dst, &m.to_cols_array());
                }
            }
        }
    }
    

    pub fn ty(&self) -> StructType {

        match self {

            Self::F32(_) =>
                StructType::F32,

            Self::I32(_) =>
                StructType::I32,

            Self::U32(_) =>
                StructType::U32,


            Self::Vec2(_) =>
                StructType::Vec2,

            Self::Vec3(_) =>
                StructType::Vec3,

            Self::Vec4(_) =>
                StructType::Vec4,


            Self::IVec2(_) =>
                StructType::IVec2,

            Self::IVec3(_) =>
                StructType::IVec3,

            Self::IVec4(_) =>
                StructType::IVec4,


            Self::UVec2(_) =>
                StructType::UVec2,

            Self::UVec3(_) =>
                StructType::UVec3,

            Self::UVec4(_) =>
                StructType::UVec4,


            Self::Mat2(_) =>
                StructType::Mat2,

            Self::Mat3(_) =>
                StructType::Mat3,

            Self::Mat4(_) =>
                StructType::Mat4,


            Self::F32Array(v)=>
                StructType::F32Array{
                    count:v.len()
                },

            Self::Vec3Array(v)=>
                StructType::Vec3Array{
                    count:v.len()
                },

            Self::Mat4Array(v)=>
                StructType::Mat4Array{
                    count:v.len()
                },
        }
    }

}