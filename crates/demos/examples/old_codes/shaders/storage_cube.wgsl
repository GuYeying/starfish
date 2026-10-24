struct Camera {
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
};
struct ObjectData {
    model: mat4x4<f32>,
};

// Group0 相机Uniform
@group(0) @binding(0)
var<uniform> camera: Camera;

// Group1 物体存储数组
@group(1) @binding(0)
var<storage, read> objects: array<ObjectData>;

// Group2 纹理采样器
@group(2) @binding(0)
var tex: texture_2d<f32>;
@group(2) @binding(1)
var samp: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(input: VertexInput, @builtin(instance_index) instance: u32) -> VertexOutput {
    var out: VertexOutput;
    let obj = objects[instance];
    out.position = camera.proj * camera.view * obj.model * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}