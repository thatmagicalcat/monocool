struct Uniform {
    projection: mat4x4<f32>,
    mouse_position: vec2<f32>,
    flashlight: u32,
    flashglith_radius: f32,
};

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coord: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> data: Uniform;

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    out.position = data.projection * vec4(model.position, 1.0);
    out.tex_coord = model.tex_coord;

    return out;
}


@group(1) @binding(0)
var t_diffuse: texture_2d<f32>;

@group(1) @binding(1)
var s_diffuse: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var mix: f32 = 0.0;

    if data.flashlight == 1 {
        if length(in.position.xy - data.mouse_position) < data.flashglith_radius {
            mix = 0.0;
        } else {
            mix = 0.9;
        }
    } else {
        mix = 0.0;
    }

    return mix(textureSample(t_diffuse, s_diffuse, in.tex_coord), vec4(0.0, 0.0, 0.0, 1.0), mix);
}
