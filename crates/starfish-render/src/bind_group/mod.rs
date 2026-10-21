//! 材质系统
//!
//! BindGroup 不直接代表 GPU BindGroup。
//! 它只描述 Shader 所需资源。
//!
//! Renderer 在真正绘制时，负责将 BindGroup
//! 转换为 BindGroup。
pub mod bind_group;
pub mod builder;
pub mod bindings;
pub mod field_value;
pub mod struct_layout;
pub mod uniform_buffer;
pub mod storage_buffer;
pub mod field_layout;
pub mod field_type;