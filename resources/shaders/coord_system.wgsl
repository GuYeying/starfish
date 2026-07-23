@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

struct MatrixData {
    model: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
}
@group(1) @binding(0) var<uniform> mat_data: MatrixData;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) out_uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos = mat_data.proj * mat_data.view * mat_data.model * vec4(in.pos, 1.0);
    out.out_uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.out_uv);
}