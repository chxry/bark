const pi = radians(180.0);

struct Object {
    transform: mat4x4<f32>,
    normal_transform: mat3x3<f32>,
    diffuse_color: vec3<f32>,
    diffuse_id: u32,
    pbr_values: vec3<f32>,
    pbr_id: u32,
    normal_id: u32
}

struct FrameGlobals {
    camera_view: mat4x4<f32>,
    camera_proj: mat4x4<f32>,
    camera_view_proj_inv: mat4x4<f32>,
    shadow_source_mats: array<mat4x4<f32>, 4>,
    camera_pos: vec3<f32>
}

struct Light {
    direction: vec3<f32>,
    tag: u32,
    color: vec3<f32>
}
