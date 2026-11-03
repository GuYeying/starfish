// 绑定组0：纹理视图(binding0) + 采样器(binding1)
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

// 顶点输入：对应VertexAttribute location 0/1/2
struct VertexInput {
    @location(0) pos: vec3f,
    @location(1) vert_color: vec3f,
    @location(2) uv: vec2f,
};

// 顶点输出，传给片元着色器
struct VertexOutput {
    @builtin(position) pos: vec4f,
    @location(0) out_color: vec3f,
    @location(1) out_uv: vec2f,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // 无相机投影，直接NDC坐标
    out.pos = vec4f(input.pos, 1.0);
    out.out_color = input.vert_color;
    out.out_uv = input.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    return textureSample(tex, samp, in.out_uv);
}