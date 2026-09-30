// pygame::render MRT 着色器（多渲染目标：一次 pass 写 N 个 color attachment）
//
// 与 sprite.wgsl 同构（group0 相机 / group1 纹理+采样器），差异仅片元
// 输出面：v1 = 镜像双写（两个目标同色）——deferred/pick 等异构输出
// 场景走自定义 WGSL（L2 层，布局文档随 v2 材质系统落地）。

struct Camera {
    mvp: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexInput {
    @location(0) pos: vec3f,
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

struct FragmentOutput {
    @location(0) color0: vec4f,
    @location(1) color1: vec4f,
};

@fragment
fn fs_main(in: VertexOutput) -> FragmentOutput {
    let c = textureSample(tex, samp, in.uv) * in.color;
    return FragmentOutput(c, c);
}
