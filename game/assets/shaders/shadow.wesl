import package::util::{FrameGlobals};

var<immediate> object_transform: mat4x4<f32>;

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@group(1) @binding(0)
var<storage, read> skeletons: array<array<mat4x4<f32>, 100>>;

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
    let skeleton = skeletons[0];
    let bone_transform = skeleton[bone_indices.x] * bone_weights.x +
        skeleton[bone_indices.y] * bone_weights.y +
        skeleton[bone_indices.z] * bone_weights.z +
        skeleton[bone_indices.w] * bone_weights.w;
    return frame.shadow_source_mats[index] * object_transform * bone_transform * vec4(pos, 1.0);
}

