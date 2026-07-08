import package::util::FrameGlobals;

var<immediate> object_transform: mat4x4<f32>;

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@vertex
fn vs_shadow(@location(0) pos: vec3<f32>, @builtin(view_index) index: u32) -> @builtin(position) vec4<f32> {
    return frame.shadow_source_mats[index] * object_transform * vec4(pos, 1.0);
}

