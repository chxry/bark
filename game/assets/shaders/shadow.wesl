import package::util::{FrameGlobals, get_bone_transform};

var<immediate> object_transform: mat4x4<f32>;

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@group(1) @binding(0)
var<storage, read> skeletons: array<array<mat4x4<f32>, 64>>;

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @builtin(view_index) index: u32) -> @builtin(position) vec4<f32> {
    return frame.shadow_source_mats[index] * object_transform * vec4(pos, 1.0);
}

@vertex
fn vs_skinned(
    @location(0) pos: vec3<f32>,
    @location(4) bone_indices: vec4<u32>,
    @location(5) bone_weights: vec4<f32>,
     @builtin(view_index) index: u32
) -> @builtin(position) vec4<f32> {
    return frame.shadow_source_mats[index] * object_transform * get_bone_transform(skeletons[0], bone_indices, bone_weights) * vec4(pos, 1.0);
}

