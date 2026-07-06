struct FrameGlobals {
  camera_mat: mat4x4<f32>,
  shadow_caster_mat: mat4x4<f32>,
  camera_pos: vec3<f32>
}

var<immediate> object_transform: mat4x4<f32>;

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    return frame.shadow_caster_mat * object_transform * vec4(pos, 1.0);
}

