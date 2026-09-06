#[derive(Clone, Debug)]
pub enum StructType {

    F32,
    I32,
    U32,

    Vec2,
    Vec3,
    Vec4,

    IVec2,
    IVec3,
    IVec4,

    UVec2,
    UVec3,
    UVec4,


    Mat2,
    Mat3,
    Mat4,


    F32Array {
        count: usize,
    },

    Vec3Array {
        count: usize,
    },

    Mat4Array {
        count: usize,
    },

}




impl StructType {

    /// std140 alignment
    pub fn alignment(&self) -> u64 {
        match self {

            StructType::F32
            | StructType::I32
            | StructType::U32 => 4,


            StructType::Vec2
            | StructType::IVec2
            | StructType::UVec2 => 8,


            StructType::Vec3
            | StructType::Vec4
            | StructType::IVec3
            | StructType::IVec4
            | StructType::UVec3
            | StructType::UVec4
            | StructType::Mat2
            | StructType::Mat3
            | StructType::Mat4 => 16,

            // array base alignment = element alignment
            StructType::F32Array{count: _ } => 16,

            StructType::Vec3Array{count: _ } => 16,

            StructType::Mat4Array{count: _ } => 16,
        }
    }


    /// 实际数据大小，不考虑 std140 padding
    pub fn cpu_size(&self) -> u64 {
        match self {

            StructType::F32
            | StructType::I32
            | StructType::U32 => 4,


            StructType::Vec2
            | StructType::IVec2
            | StructType::UVec2 => 8,


            StructType::Vec3
            | StructType::IVec3
            | StructType::UVec3 => 12,


            StructType::Vec4
            | StructType::IVec4
            | StructType::UVec4 => 16,


            StructType::Mat2 => 16,
            StructType::Mat3 => 36,
            StructType::Mat4 => 64,


            StructType::F32Array{count} =>
                *count as u64 * 4,


            StructType::Vec3Array{count} =>
                *count as u64 * 12,


            StructType::Mat4Array{count} =>
                *count as u64 * 64,
        }
    }


    /// std140实际占用大小
    ///
    /// layout offset递增使用这个
    pub fn gpu_size(&self) -> u64 {

        match self {

            StructType::F32
            | StructType::I32
            | StructType::U32 => 4,


            StructType::Vec2
            | StructType::IVec2
            | StructType::UVec2 => 8,


            // vec3在std140占16
            StructType::Vec3
            | StructType::IVec3
            | StructType::UVec3
            | StructType::Vec4
            | StructType::IVec4
            | StructType::UVec4 => 16,


            StructType::Mat2 => {
                // 两个vec2列，每列stride=16
                16 * 2
            }


            StructType::Mat3 => {
                // 三个vec4列
                16 * 3
            }


            StructType::Mat4 => {
                16 * 4
            }


            // array:
            // 每个元素stride=16
            StructType::F32Array{count} => {
                *count as u64 * 16
            }


            StructType::Vec3Array{count} => {
                *count as u64 * 16
            }


            StructType::Mat4Array{count} => {
                *count as u64 * 64
            }
        }
    }


    pub fn is_array(&self)->bool {
        matches!(
            self,
            StructType::F32Array{..}
            | StructType::Vec3Array{..}
            | StructType::Mat4Array{..}
        )
    }


    pub fn stride(&self)->u64 {

        match self {

            StructType::F32Array{..}
            | StructType::Vec3Array{..}
                =>16,


            StructType::Mat4Array{..}
                =>64,


            _=>self.gpu_size()
        }
    }

}
