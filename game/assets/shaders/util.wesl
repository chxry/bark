const pi = radians(180.0);

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
