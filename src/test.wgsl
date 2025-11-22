struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) world_pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) normal: vec3<f32>,
  @location(3) tangent: vec4<f32>
}

struct FrameGlobals {
  camera: mat4x4<f32>,
  camera_pos: vec3<f32>
}

struct Object {
  transform: mat4x4<f32>,
  normal_transform: mat3x3<f32>,
  diffuse_id: u32,
  normal_id: u32
}

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;
@group(1) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var tex_sampler: sampler;

var<push_constant> obj: Object;

@vertex
fn vs_main(
  @builtin(vertex_index) idx: u32,
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) normal: vec3<f32>,
  @location(3) tangent: vec4<f32>,
) -> VsOut {
  return VsOut(
    frame.camera * obj.transform * vec4(pos, 1.0),
    pos,
    uv,
    obj.normal_transform * normal,
    vec4(obj.normal_transform * tangent.xyz, tangent.w)
  );
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let diffuse_tex = textureSample(textures[obj.diffuse_id], tex_sampler, in.uv);
  let normal_tex = textureSample(textures[obj.normal_id], tex_sampler, in.uv).xyz;

  if diffuse_tex.a < 0.01 {
    discard;
  }

  let n = normalize(in.normal);
  let t = normalize(in.tangent.xyz);
  let b = normalize(cross(n, t) * in.tangent.w);
  let tbn = mat3x3(t, b, n);
  let normal = normalize(tbn * (normal_tex * 2.0 - vec3(1.0)));

  let light_dir = normalize(vec3(-1.0, 0.5, 0.0));
  let cam_dir = frame.camera_pos - in.world_pos;
  let half = normalize(light_dir + cam_dir);
  let diffuse = max(dot(normal, light_dir), 0.0);
  let specular = pow(max(dot(normal, half), 0.0), 30.0);
  let light = 0.05 + diffuse + specular;

  return vec4(diffuse_tex.rgb * light, 1.0);
  // return vec4(0.5 * (normal + vec3(1.0)), 1.0);
}
 
