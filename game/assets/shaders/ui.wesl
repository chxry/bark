@vertex
fn vs_ui() -> @builtin(position) vec4<f32> {
    return vec4(1.0, 1.0, 1.0, 1.0);
}


@fragment
fn fs_ui() -> @location(0) vec4<f32> {
    return vec4(1.0, 1.0, 1.0, 1.0);   
}
