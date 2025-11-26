const pi = radians(180.0);

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
  diffuse_colour: vec3<f32>,
  diffuse_id: u32,
  pbr_arm: vec3<f32>,
  normal_id: u32,
  pbr_id: u32
}

struct Light {
  direction: vec3<f32>,
  tag: u32
}

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@group(0) @binding(1)
var<storage, read> lights: array<Light>;

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
  let world_pos = obj.transform * vec4(pos, 1.0);
  return VsOut(
    frame.camera * world_pos,
    world_pos.xyz,
    uv,
    obj.normal_transform * normal,
    vec4(obj.normal_transform * tangent.xyz, tangent.w)
  );
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
  let base_colour = textureSample(textures[obj.diffuse_id], tex_sampler, in.uv);
  if base_colour.a < 0.01 {
    discard;
  }
  let diffuse_colour = base_colour.rgb * obj.diffuse_colour;

  var normal = normalize(in.normal);
  if obj.normal_id > 0 {
    let normal_ts = textureSample(textures[obj.normal_id], tex_sampler, in.uv).xyz;

    let tangent = normalize(in.tangent.xyz);
    let bitangent = normalize(cross(normal, tangent) * in.tangent.w);
    let tbn = mat3x3(tangent, bitangent, normal);
    normal = normalize(tbn * (normal_ts * 2.0 - vec3(1.0)));
  }

  let view_dir = normalize(frame.camera_pos - in.world_pos);

  let pbr = textureSample(textures[obj.pbr_id], tex_sampler, in.uv).rgb * obj.pbr_arm;
  let ao = pbr.r;
  let roughness = clamp(pbr.g, 0.04, 1.0);
  let metallic = pbr.b;

  let ambient = diffuse_colour * 0.05 * ao;
  
  var light = ambient;
  for (var i = 0u;;i++) {
    if (lights[i].tag == 0) {
      break;
    }
    
    let light_dir = -lights[i].direction;
    let half_dir = normalize(light_dir + view_dir);
    let n_dot_l = max(dot(normal, light_dir), 0.0);
    let n_dot_v = max(dot(normal, view_dir), 0.0);
    let h_dot_v = max(dot(half_dir, view_dir), 0.0);

    let f0 = mix(vec3(0.04), diffuse_colour, metallic);
    let d = distribution_ggx(normal, half_dir, roughness);
    let g = geometry_smith(normal, view_dir, light_dir, roughness);
    let f = fresnel_schlick(h_dot_v, f0);

    let denom = max(4.0 * n_dot_l * n_dot_v, 0.001);
    let specular = d * g * f / denom;

    let kd = (1.0 - f) * (1.0 - metallic);
    let diffuse = kd * diffuse_colour / pi;

    light += (diffuse + specular) * n_dot_l;
  }

  return vec4(light, 1.0);
}
 
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(1.0 - cos_theta, 5.0);
}

fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;

    let denom = (n_dot_h2 * (a2 - 1.0) + 1.0);
    return a2 / (pi * denom * denom);
}

fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = (roughness + 1.0);
    let k = (r * r) / 8.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_l = max(dot(n, l), 0.0);
    let ggx1 = geometry_schlick_ggx(n_dot_v, roughness);
    let ggx2 = geometry_schlick_ggx(n_dot_l, roughness);
    return ggx1 * ggx2;
}

