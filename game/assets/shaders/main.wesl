import package::util::{pi, Object, FrameGlobals, Light};
enable wgpu_binding_array;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec4<f32>
}

var<immediate> object: Object;

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;
@group(0) @binding(1)
var<storage, read> lights: array<Light>;

@group(1) @binding(0)
var textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1)
var tex_sampler: sampler;

@group(2) @binding(0)
var shadow_map: texture_depth_2d_array;
@group(2) @binding(1)
var shadow_map_sampler: sampler_comparison;

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec4<f32>,
) -> VsOut {
    let world_pos = object.transform * vec4(pos, 1.0);
    return VsOut(
        frame.camera_proj * frame.camera_view * world_pos,
        world_pos.xyz,
        uv,
        object.normal_transform * normal,
        vec4(object.normal_transform * tangent.xyz, tangent.w)
    );
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base_color = textureSample(textures[object.diffuse_id], tex_sampler, in.uv);
    if base_color.a < 0.25 {
        discard;
    }
    let diffuse_color = base_color.rgb * object.diffuse_color;

    var normal = normalize(in.normal);
    if object.normal_id > 0 {
        let normal_rg = textureSample(textures[object.normal_id], tex_sampler, in.uv).rg;
        let n = normal_rg * 2.0 - vec2(1.0);
        let z = sqrt(max(0.0, 1.0 - dot(n, n)));
        let normal_ts = vec3(n, z);

        let tangent = normalize(in.tangent.xyz);
        let bitangent = normalize(cross(normal, tangent) * in.tangent.w);
        let tbn = mat3x3(tangent, bitangent, normal);
        normal = normalize(tbn * normal_ts);
    }

    let view_dir = normalize(frame.camera_pos - in.world_pos);

    let pbr = textureSample(textures[object.pbr_id], tex_sampler, in.uv).rgb * object.pbr_values;
    let ao = pbr.r;
    let roughness = clamp(pbr.g, 0.04, 1.0);
    let metallic = pbr.b;

    let ambient = diffuse_color * 0.05 * ao;

    let shadow = shadow_factor(in.world_pos);

    var light = ambient;
    for (var i = 0u; lights[i].tag != 0; i++) {  
        let light_dir = -lights[i].direction;
        let half_dir = normalize(light_dir + view_dir);
        let n_dot_l = max(dot(normal, light_dir), 0.0);
        let n_dot_v = max(dot(normal, view_dir), 0.0);
        let h_dot_v = max(dot(half_dir, view_dir), 0.0);

        let f0 = mix(vec3(0.04), diffuse_color, metallic);
        let d = distribution_ggx(normal, half_dir, roughness);
        let g = geometry_smith(normal, view_dir, light_dir, roughness);
        let f = fresnel_schlick(h_dot_v, f0);

        let denom = max(4.0 * n_dot_l * n_dot_v, 0.001);
        let specular = d * g * f / denom;

        let kd = (1.0 - f) * (1.0 - metallic);
        let diffuse = kd * diffuse_color / pi;

        light += lights[i].color * (diffuse + specular) * n_dot_l * shadow;
    }

    return vec4(tonemap_uncharted2(light), 1.0);
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

fn tonemap_uncharted2(color: vec3<f32>) -> vec3<f32> {
    let A = 0.15;
    let B = 0.50;  
    let C = 0.10;
    let D = 0.20;
    let E = 0.02;
    let F = 0.30;
    let W = 11.2;
    let bias = 2.0;

    let x = vec4(bias * color, W);
    let mapped = ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F;
    return mapped.xyz / mapped.w;
  }

fn shadow_factor(world_pos: vec3<f32>) -> f32 {
    let view_depth = -(frame.camera_view * vec4(world_pos, 1.0)).z;
    let cascades = array(5.0, 10.0, 25.0, 100.0);
    var cascade_index = 0;
    while (cascade_index < 4) {
        if (view_depth < cascades[cascade_index]) {
            break;
        }
        cascade_index++;
    }

    let light_space_pos = frame.shadow_source_mats[cascade_index] * vec4(world_pos, 1.0);
    let uv = vec2(light_space_pos.x * 0.5 + 0.5, light_space_pos.y * -0.5 + 0.5);
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 {
        return 1.0;
    }
    
    let texture_size = vec2<f32>(textureDimensions(shadow_map));
    let texel_size = 1.0 / texture_size;
    
    var shadow = 0.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            let offset = vec2(f32(x), f32(y)) * texel_size;
            shadow += textureSampleCompare(shadow_map, shadow_map_sampler, uv + offset, cascade_index, light_space_pos.z - 0.001);
        }
    }
    return shadow / 9.0;
}
