// pygame::render 万能精灵着色器（契约 ADR-4 单一通用管线的 v1 落位）
//
// 双 group 结构（对齐 base::font::text.wgsl 的既有约定）：
//   group0 = 相机（MVP——唯一上传的变换量，Camera::mvp() 产出）
//   group1 = 纹理 + 采样器
//
// 顶点 = pos3 + uv2 + color4（"统一顶点布局"：图形学底层数据无 2D/3D 之分）
// 片元 = 纹理色 × 顶点色（调制语义）——三路同管线：
//   ① 纹理精灵：color = 白（透传纹理）
//   ② 纯色图元：纹理 = 1×1 白，color = 目标色
//   ③ 文字图集：白色字形 × color（base::font 图集直接可挂）

struct Camera {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexInput {
    @location(0) pos: vec3f,     // 2D: z=0（像素坐标，y 向下）；3D: 世界坐标
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = camera.mvp * vec4(in.pos, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    // 调制：纹理 × 顶点色（线性分量相乘；alpha 同乘——pygame blit 语义）
    return textureSample(tex, samp, in.uv) * in.color;
}
