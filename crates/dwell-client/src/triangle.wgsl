// The WebGPU probe's triangle (24a): one triangle in a fixed colour, centred on the target.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array(vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(0.0, 0.5));
    return vec4(p[i], 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4(1.0, 0.5, 0.0, 1.0);
}
