// 文本/精灵着色器（pos3：2D 传 z=0，3D 传世界坐标）
//
// 双 group 结构（参考 hilbert-curve 教程）：
//   group0 = 相机（MVP，像素正交或世界 view-proj 均可）
//   group1 = 图集纹理 + 采样器
//
// 约定：图集为白色字形 + alpha 覆盖度，fragment 用顶点色乘制着色。

struct Camera {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexInput {
    @location(0) pos: vec3f,     // 2D: z=0；3D: 世界坐标
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
    let alpha = textureSample(tex, samp, in.uv).a * in.color.a;
    return vec4f(in.color.rgb, alpha);
}
