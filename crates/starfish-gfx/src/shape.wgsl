// 几何形状着色器（填充/线段两管线共用，仅拓扑不同）
//
// group0 = 相机（MVP，像素正交或世界 view-proj 均可）
// 颜色烘焙在顶点里（pos3 + color4）。

struct Camera {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexInput {
    @location(0) pos: vec3f,
    @location(1) color: vec4f,
};

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) color: vec4f,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = camera.mvp * vec4(in.pos, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    return in.color;
}
