struct vs_out {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>
}

struct object {
  transform: mat4x4<f32>,
  texture_id: u32
}

@group(0) @binding(0)
var<uniform> camera: mat4x4<f32>;
@group(1) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var tex_sampler: sampler;

var<push_constant> obj: object;

@vertex
fn vs_main(
  @builtin(vertex_index) idx: u32,
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>
) -> vs_out {
  return vs_out(camera * obj.transform * vec4(pos, 1.0), uv);
}

@fragment
fn fs_main(in: vs_out) -> @location(0) vec4<f32> {
    return textureSample(textures[obj.texture_id], tex_sampler, in.uv);
}
 
