import package::util::FrameGlobals;

struct VSOut {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> frame: FrameGlobals;

@vertex
fn vs_sky(@builtin(vertex_index) index: u32) -> VSOut {
    let vertices = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return VSOut(vec4(vertices[index], 0.0, 1.0), vertices[index]);
}

@fragment
fn fs_sky(in: VSOut) -> @location(0) vec4<f32> {
    let world_pos = frame.camera_view_proj_inv * vec4(in.ndc, 1.0, 1.0);

    let ray = normalize(world_pos.xyz / world_pos.w - frame.camera_pos);

    let t = max(ray.y, 0.0);
    let horizon = vec3(0.7, 0.8, 1.0);
    let zenith  = vec3(0.08, 0.25, 0.7);
    let color = mix(horizon, zenith, pow(t, 0.5));
    
    return vec4(color, 1.0);
}
