struct VertexInput {
    @location(0) pos: vec3<f32>,
    @location(1) color: vec3<f32>,
}
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(1) out_color: vec3<f32>,
}
@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(in.pos, 1.0);
    out.out_color = in.color;
    return out;
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.out_color, 1.0);
}